//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

mod filesystem;

use std::{path::Path, path::PathBuf, time::Duration};

use anyhow::Result;
use async_fs as afs;
use bevy::{
    asset::{
        AssetPath, AssetPlugin, UnapprovedPathMode,
        io::{AssetSourceBuilder, AssetSourceId},
    },
    audio::{AudioPlayer, AudioSource, PlaybackSettings},
    platform::collections::HashMap,
    prelude::*,
    tasks::{IoTaskPool, Task, futures::check_ready},
};
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use chardetng::EncodingDetector;
use clap::Parser;
use gametime::{TimeSpan, TimeStamp};
use num_traits::ToPrimitive;

fn main() {
    let args = ExecArgs::parse();
    let mut app = App::new();
    app.register_asset_source("fs", AssetSourceBuilder::platform_default(".", None));
    app.insert_resource(args)
        .insert_resource(NowStamp::default())
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            unapproved_path_mode: UnapprovedPathMode::Deny,
            ..Default::default()
        }))
        .add_systems(Startup, setup_scene_7k)
        .add_systems(Startup, load_bms_file)
        .add_systems(
            Update,
            (
                update_now_stamp,
                poll_bms_load_task,
                start_when_audio_ready,
                process_chart_events,
                render_visible_chart,
            )
                .chain(),
        )
        .run();
}

#[derive(Parser, Resource)]
struct ExecArgs {
    #[arg(long)]
    bms_path: Option<PathBuf>,
}

#[derive(Resource)]
struct BmsLoadTask(Task<Result<(BmsProcessor, HashMap<WavId, PathBuf>)>>);

async fn load_bms_and_collect_paths(
    bms_path: PathBuf,
) -> Result<(BmsProcessor, HashMap<WavId, PathBuf>)> {
    let bms_bytes = afs::read(&bms_path).await?;
    let mut det = EncodingDetector::new();
    det.feed(&bms_bytes, true);
    let enc = det.guess(None, true);
    let (bms_str, _, _) = enc.decode(&bms_bytes);
    let BmsOutput { bms, warnings: _ } = bms_rs::bms::parse_bms(&bms_str, default_config());
    let bms = bms.unwrap();
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or(BaseBpm(120.0.into()));
    let processor = BmsProcessor::new::<KeyLayoutBeat>(
        &bms,
        VisibleRangePerBpm::new(
            &base_bpm,
            TimeSpan::from_duration(Duration::from_secs_f32(0.6)),
        ),
    );
    let bms_dir = bms_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut audio_paths: HashMap<WavId, PathBuf> = HashMap::new();
    let child_list: Vec<PathBuf> = processor
        .audio_files()
        .into_values()
        .map(std::path::Path::to_path_buf)
        .collect();
    let index = filesystem::choose_paths_by_ext_async(
        &bms_dir,
        &child_list,
        &["flac", "wav", "ogg", "mp3"],
    )
    .await;
    for (id, audio_path) in processor.audio_files().into_iter() {
        let stem = audio_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(std::string::ToString::to_string);
        let base = bms_dir.join(audio_path);
        let chosen = stem.and_then(|s| index.get(&s).cloned()).unwrap_or(base);
        audio_paths.insert(id, chosen);
    }
    Ok((processor, audio_paths))
}

#[derive(Resource)]
struct BmsProcessStatus {
    processor: BmsProcessor,
    audio_handles: HashMap<WavId, Handle<AudioSource>>,
    audio_paths: HashMap<WavId, PathBuf>,
    started: bool,
    warned_missing: bool,
}

#[derive(Component)]
struct NoteMarker;

#[derive(Resource, Default)]
struct ChartVisualState {
    notes: HashMap<ChartEventId, Entity>,
}

#[derive(Resource, Clone, Copy)]
struct NowStamp(TimeStamp);

impl Default for NowStamp {
    fn default() -> Self {
        Self(TimeStamp::start())
    }
}

const LANE_COUNT: usize = 8;
const LANE_WIDTH: f32 = 60.0;
const LANE_GAP: f32 = 8.0;
const VISIBLE_HEIGHT: f32 = 600.0;
const NOTE_HEIGHT: f32 = 12.0;

fn total_width() -> f32 {
    LANE_COUNT as f32 * LANE_WIDTH + (LANE_COUNT as f32 - 1.0) * LANE_GAP
}

fn lane_x(idx: usize) -> f32 {
    let left = -total_width() / 2.0 + LANE_WIDTH / 2.0;
    left + idx as f32 * (LANE_WIDTH + LANE_GAP)
}

fn key_to_lane(key: Key) -> Option<usize> {
    match key {
        Key::Scratch(_) => Some(0),
        Key::Key(n) => match n {
            1..=7 => Some(n as usize),
            _ => None,
        },
        _ => None,
    }
}

fn setup_scene_7k(mut commands: Commands) {
    commands.spawn((Camera2d, Transform::default(), GlobalTransform::default()));
    for i in 0..LANE_COUNT {
        commands.spawn((
            Sprite {
                color: Color::srgb(0.15, 0.15, 0.18),
                custom_size: Some(Vec2::new(LANE_WIDTH, VISIBLE_HEIGHT)),
                ..Default::default()
            },
            Transform::from_xyz(lane_x(i), 0.0, 0.0),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
        ));
    }
    commands.spawn((
        Sprite {
            color: Color::srgb(0.9, 0.9, 0.9),
            custom_size: Some(Vec2::new(total_width(), 4.0)),
            ..Default::default()
        },
        Transform::from_xyz(0.0, -VISIBLE_HEIGHT / 2.0 + 2.0, 1.0),
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
    ));
    commands.insert_resource(ChartVisualState::default());
}

fn load_bms_file(mut commands: Commands, args: Res<ExecArgs>) {
    let Some(bms_path) = args.bms_path.clone() else {
        return;
    };
    let pool = IoTaskPool::get();
    let task = pool.spawn(load_bms_and_collect_paths(bms_path));
    commands.insert_resource(BmsLoadTask(task));
}

fn poll_bms_load_task(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    task_res: Option<ResMut<BmsLoadTask>>,
) {
    let Some(mut task) = task_res else {
        return;
    };
    if let Some(result) = check_ready(&mut task.0) {
        match result {
            Ok((processor, audio_paths)) => {
                let mut audio_handles = HashMap::new();
                for (id, chosen) in &audio_paths {
                    let ap = AssetPath::from_path(chosen).with_source(AssetSourceId::from("fs"));
                    let handle: Handle<AudioSource> = asset_server.load_override(ap);
                    audio_handles.insert(*id, handle);
                }
                commands.insert_resource(BmsProcessStatus {
                    processor,
                    audio_handles,
                    audio_paths,
                    started: false,
                    warned_missing: false,
                });
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }
        commands.remove_resource::<BmsLoadTask>();
    }
}

fn update_now_stamp(mut now_stamp: ResMut<NowStamp>) {
    now_stamp.0 = TimeStamp::now();
}

fn start_when_audio_ready(
    status: Option<ResMut<BmsProcessStatus>>,
    assets: Res<Assets<AudioSource>>,
    now_stamp: Res<NowStamp>,
) {
    let Some(mut status) = status else {
        return;
    };
    if status.started {
        return;
    }
    let mut missing: Vec<WavId> = Vec::new();
    for (id, handle) in &status.audio_handles {
        let Some(_) = assets.get(handle) else {
            missing.push(*id);
            continue;
        };
    }
    if missing.is_empty() {
        status.processor.start_play(now_stamp.0);
        status.started = true;
    } else if !status.warned_missing {
        for id in missing {
            if let Some(p) = status.audio_paths.get(&id) {
                eprintln!("音频未载入: #WAV{:03} -> {}", id.0, p.to_string_lossy());
            } else {
                eprintln!("音频未载入: #WAV{:03}", id.0);
            }
        }
        status.warned_missing = true;
    }
}

fn process_chart_events(
    mut commands: Commands,
    status: Option<ResMut<BmsProcessStatus>>,
    assets: Res<Assets<AudioSource>>,
    now_stamp: Res<NowStamp>,
) {
    let Some(mut status) = status else {
        return;
    };
    if !status.started {
        return;
    }
    let handles = status.audio_handles.clone();
    let mut to_spawn: Vec<(AudioPlayer, PlaybackSettings)> = Vec::new();
    for evp in status.processor.update(now_stamp.0) {
        let wav = match evp.event() {
            ChartEvent::Note {
                wav_id: Some(wav), ..
            }
            | ChartEvent::Bgm { wav_id: Some(wav) } => wav,
            _ => continue,
        };
        let Some(handle) = handles.get(wav) else {
            continue;
        };
        if assets.get(handle).is_none() {
            continue;
        }
        to_spawn.push((AudioPlayer::new(handle.clone()), PlaybackSettings::DESPAWN));
    }
    if !to_spawn.is_empty() {
        commands.spawn_batch(to_spawn);
    }
}

fn render_visible_chart(
    mut commands: Commands,
    status: Option<ResMut<BmsProcessStatus>>,
    mut vis: ResMut<ChartVisualState>,
    mut q_notes: Query<(&mut Transform, &mut Visibility), With<NoteMarker>>,
    _now_stamp: Res<NowStamp>,
) {
    let Some(mut status) = status else {
        return;
    };
    if !status.started {
        return;
    }
    let mut alive: Vec<ChartEventId> = Vec::new();
    for ev in status.processor.visible_events() {
        let (playhead_event, range) = ev;
        let ChartEvent::Note { side, key, .. } = playhead_event.event() else {
            continue;
        };
        if *side != PlayerSide::Player1 {
            continue;
        }
        let Some(idx) = key_to_lane(*key) else {
            continue;
        };
        let x = lane_x(idx);
        let ratio_value = range.start().as_ref();
        let y = -VISIBLE_HEIGHT / 2.0
            + ToPrimitive::to_f64(ratio_value).unwrap_or(0.0) as f32 * VISIBLE_HEIGHT;
        if let Some(entity) = vis.notes.get(&playhead_event.id()) {
            if let Ok((mut tf, mut v)) = q_notes.get_mut(*entity) {
                tf.translation.x = x;
                tf.translation.y = y;
                *v = Visibility::Visible;
            }
        } else {
            let entity = commands
                .spawn((
                    Sprite {
                        color: Color::srgb(0.3, 0.7, 1.0),
                        custom_size: Some(Vec2::new(LANE_WIDTH - 4.0, NOTE_HEIGHT)),
                        ..Default::default()
                    },
                    Transform::from_xyz(x, y, 2.0),
                    GlobalTransform::default(),
                    Visibility::default(),
                    InheritedVisibility::default(),
                    NoteMarker,
                ))
                .id();
            vis.notes.insert(playhead_event.id(), entity);
        }
        alive.push(playhead_event.id());
    }
    let obsolete: Vec<ChartEventId> = vis
        .notes
        .keys()
        .filter(|id| !alive.contains(id))
        .cloned()
        .collect();
    for id in obsolete {
        if let Some(&entity) = vis.notes.get(&id)
            && let Ok((_, mut v)) = q_notes.get_mut(entity)
        {
            *v = Visibility::Hidden;
        }
    }
}
