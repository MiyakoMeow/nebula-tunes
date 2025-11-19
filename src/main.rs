//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

use anyhow::{Result, anyhow};
use async_fs::{self as afs, File};
use async_zip::base::read::seek::ZipFileReader;
use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, futures::check_ready};
use chardetng::EncodingDetector;
use futures_lite::io::BufReader;
use futures_lite::{StreamExt, stream};
use std::path::PathBuf;

const MAX_CONCURRENCY: usize = 8;

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_systems(Startup, start_async_read)
        .add_systems(Update, poll_task_and_exit)
        .run();
}

#[derive(Resource)]
struct ReadTask(Task<Result<Vec<String>>>);

fn start_async_read(mut commands: Commands) {
    let zip_path: PathBuf = PathBuf::from(std::env::args_os().nth(1).expect("missing zip path"));

    let task = AsyncComputeTaskPool::get().spawn(read_lines(zip_path));
    commands.insert_resource(ReadTask(task));
}

fn poll_task_and_exit(task_res: Option<ResMut<ReadTask>>, mut exit: MessageWriter<AppExit>) {
    if let Some(mut task) = task_res
        && let Some(result) = check_ready(&mut task.0)
    {
        match result {
            Ok(lines) => {
                lines.into_iter().for_each(|line| println!("{}", line));
                exit.write(AppExit::Success);
            }
            Err(e) => {
                eprintln!("{}", e);
                exit.write(AppExit::Success);
            }
        }
    }
}

async fn read_lines_from_archive(zip_path: PathBuf) -> Result<Vec<String>> {
    let file = File::open(&zip_path).await?;
    let ext = zip_path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    match ext.as_str() {
        "zip" => {
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
                out.push(zip_path.to_string_lossy().into_owned());
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
        _ => Err(anyhow!("unsupported archive: {}", ext)),
    }
}

async fn read_lines(path: PathBuf) -> Result<Vec<String>> {
    if afs::metadata(&path).await?.is_dir() {
        let mut dir = afs::read_dir(&path).await?;
        let entries: Vec<Result<afs::DirEntry, std::io::Error>> =
            StreamExt::collect(&mut dir).await;
        let entries: Vec<afs::DirEntry> = entries.into_iter().collect::<Result<Vec<_>, _>>()?;

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

        let pool = AsyncComputeTaskPool::get();
        stream::iter(archives.chunks(MAX_CONCURRENCY))
            .then(move |chunk| {
                let tasks = chunk
                    .iter()
                    .cloned()
                    .map(|p| pool.spawn(read_lines_from_archive(p)));
                async move {
                    stream::iter(tasks)
                        .then(|t| t)
                        .fold(Ok(Vec::new()), |acc, v| match (acc, v) {
                            (Ok(mut acc), Ok(v)) => {
                                acc.extend(v);
                                Ok(acc)
                            }
                            (Err(e), _) => Err(e),
                            (_, Err(e)) => Err(e),
                        })
                        .await
                }
            })
            .fold(Ok(Vec::new()), |acc, chunk_res| match (acc, chunk_res) {
                (Ok(mut acc), Ok(v)) => {
                    acc.extend(v);
                    Ok(acc)
                }
                (Err(e), _) => Err(e),
                (_, Err(e)) => Err(e),
            })
            .await
    } else {
        read_lines_from_archive(path).await
    }
}
