use std::path::PathBuf;

use anyhow::{Result, anyhow};
use async_fs::{self as afs, File};
use async_zip::base::read::seek::ZipFileReader;
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use chardetng::EncodingDetector;
use futures_lite::io::BufReader;
use futures_lite::{StreamExt, stream};

#[derive(Message)]
pub struct ArchiveFound(pub PathBuf);

#[derive(Message)]
pub struct ArchiveReadFinished {
    pub path: PathBuf,
    pub lines: Vec<String>,
}

#[derive(Message)]
pub struct ScanArchives(pub PathBuf);

#[derive(Resource, Default)]
struct ReadTasks(Vec<(PathBuf, Task<Result<Vec<String>>>)>);

#[derive(Resource)]
struct PendingTasks(usize);

pub struct ArchivePlugin;

impl Plugin for ArchivePlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<ArchiveFound>()
            .add_message::<ArchiveReadFinished>()
            .add_message::<ScanArchives>()
            .insert_resource(ReadTasks::default())
            .insert_resource(PendingTasks(0))
            .add_systems(Startup, on_scan_message)
            .add_systems(
                Update,
                (spawn_read_tasks, poll_read_tasks, print_and_exit_on_done),
            );
    }
}

fn on_scan_message(
    mut found_writer: MessageWriter<ArchiveFound>,
    mut exit: MessageWriter<AppExit>,
    mut pending: ResMut<PendingTasks>,
    mut scan_reader: MessageReader<ScanArchives>,
) {
    for ScanArchives(path) in scan_reader.read() {
        let res = futures_lite::future::block_on(async {
            if afs::metadata(&path).await?.is_dir() {
                let mut dir = afs::read_dir(&path).await?;
                let entries: Vec<Result<afs::DirEntry, std::io::Error>> =
                    StreamExt::collect(&mut dir).await;
                let entries: Vec<afs::DirEntry> =
                    entries.into_iter().collect::<Result<Vec<_>, _>>()?;
                let paths: Vec<anyhow::Result<Option<PathBuf>>> = stream::iter(entries)
                    .then(|entry| async move {
                        let ft = entry.file_type().await?;
                        if !ft.is_file() {
                            return Ok(None);
                        }
                        let p = entry.path();
                        let is_zip = p
                            .extension()
                            .and_then(|s| s.to_str())
                            .map(|s| s.eq_ignore_ascii_case("zip"))
                            .unwrap_or(false);
                        Ok(if is_zip { Some(p) } else { None })
                    })
                    .collect()
                    .await;
                let archives: Vec<PathBuf> = paths
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                for p in &archives {
                    found_writer.write(ArchiveFound(p.clone()));
                }
                Ok(archives.len())
            } else {
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .map(str::to_ascii_lowercase)
                    .unwrap_or_default();
                match ext.as_str() {
                    "zip" => {
                        found_writer.write(ArchiveFound(path.to_path_buf()));
                        Ok(1)
                    }
                    _ => Err(anyhow!("unsupported archive: {}", ext)),
                }
            }
        });
        match res {
            Ok(count) => {
                pending.0 = pending.0.saturating_add(count);
                if pending.0 == 0 {
                    exit.write(AppExit::Success);
                }
            }
            Err(e) => {
                eprintln!("{}", e);
                exit.write(AppExit::Success);
            }
        }
    }
}

fn spawn_read_tasks(mut tasks: ResMut<ReadTasks>, mut ev_found: MessageReader<ArchiveFound>) {
    let pool = AsyncComputeTaskPool::get();
    for ArchiveFound(path) in ev_found.read() {
        let task = pool.spawn(read_lines_from_zip(path.clone()));
        tasks.0.push((path.clone(), task));
    }
}

fn poll_read_tasks(
    mut tasks: ResMut<ReadTasks>,
    mut pending: ResMut<PendingTasks>,
    mut ev_done: MessageWriter<ArchiveReadFinished>,
) {
    let mut i = 0;
    while i < tasks.0.len() {
        let (path, task) = &mut tasks.0[i];
        if let Some(result) = check_ready(task) {
            match result {
                Ok(lines) => {
                    ev_done.write(ArchiveReadFinished {
                        path: path.clone(),
                        lines,
                    });
                }
                Err(e) => {
                    eprintln!("{}", e);
                }
            }
            pending.0 = pending.0.saturating_sub(1);
            let (_p, finished_task) = tasks.0.swap_remove(i);
            drop(finished_task);
        } else {
            i += 1;
        }
    }
}

fn print_and_exit_on_done(
    mut ev_done: MessageReader<ArchiveReadFinished>,
    pending: Res<PendingTasks>,
    mut exit: MessageWriter<AppExit>,
) {
    for ev in ev_done.read() {
        println!("{}", ev.path.to_string_lossy());
        for line in &ev.lines {
            println!("{}", line);
        }
    }
    if pending.0 == 0 {
        exit.write(AppExit::Success);
    }
}

async fn read_lines_from_zip(zip_path: PathBuf) -> Result<Vec<String>> {
    let file = File::open(&zip_path).await?;
    let mut out: Vec<String> = Vec::new();
    let reader = BufReader::new(file);
    let mut zip = ZipFileReader::new(reader).await?;
    let len = zip.file().entries().len();
    for index in 0..len {
        let (name, is_bms) = {
            let entry = zip.reader_with_entry(index).await?;
            let name_raw = entry.entry().filename().as_bytes();
            let mut det = EncodingDetector::new();
            det.feed(name_raw, true);
            let enc = det.guess(None, true);
            let (name_cow, _, _) = enc.decode(name_raw);
            let name = name_cow.into_owned();
            let is_bms = name
                .rsplit('.')
                .next()
                .map(|ext| ext.eq_ignore_ascii_case("bms"))
                .unwrap_or(false);
            (name, is_bms)
        };
        if !is_bms {
            continue;
        }
        let mut reader = zip.reader_with_entry(index).await?;
        let mut bytes = Vec::new();
        reader.read_to_end_checked(&mut bytes).await?;
        out.push(name);
        let mut det = EncodingDetector::new();
        det.feed(&bytes, true);
        let enc = det.guess(None, true);
        let (cow, _, _) = enc.decode(&bytes);
        let s = cow.into_owned();
        for line in s.lines().take(5) {
            out.push(line.to_string());
        }
    }
    Ok(out)
}
