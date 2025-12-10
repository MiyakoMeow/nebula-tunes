//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

use std::{
    path::Path,
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::{
    asset::{
        AssetPath, AssetPlugin, UnapprovedPathMode,
        io::{AssetSourceBuilder, AssetSourceId},
    },
    audio::{AudioPlayer, AudioSource, PlaybackSettings},
    platform::collections::HashMap,
    prelude::*,
};
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use chardetng::EncodingDetector;
use clap::Parser;

mod test_archive_plugin;
use test_archive_plugin::TestArchivePlugin;

fn choose_paths_by_ext(path: &Path, exts: &[&str]) -> Vec<PathBuf> {
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };
    let parent = path.parent().unwrap_or(Path::new("."));
    let entries: Vec<(String, String, PathBuf)> = std::fs::read_dir(parent)
        .map(|rd| {
            rd.filter_map(|r| {
                let p = r.ok()?.path();
                let is_file = std::fs::metadata(&p).ok()?.is_file();
                if !is_file {
                    return None;
                }
                let fs_stem = p.file_stem()?.to_str()?.to_string();
                let fs_ext = p.extension()?.to_str()?.to_string();
                Some((fs_stem, fs_ext, p))
            })
            .collect()
        })
        .unwrap_or_default();

    exts.iter()
        .flat_map(|ext| {
            entries.iter().filter_map(|(s, e, p)| {
                if s == stem && e.eq_ignore_ascii_case(ext) {
                    Some(p.clone())
                } else {
                    None
                }
            })
        })
        .collect()
}

fn main() {
    let args = ExecArgs::parse();
    // 测试模式下使用 MinimalPlugins，否则使用 DefaultPlugins
    if args.test_archive_path.is_some() {
        App::new()
            .insert_resource(args)
            .add_plugins(MinimalPlugins)
            .add_plugins(TestArchivePlugin)
            .run();
        return;
    };
    // 正常模式下使用 DefaultPlugins
    let mut app = App::new();
    app.register_asset_source("fs", AssetSourceBuilder::platform_default(".", None));
    app.insert_resource(args)
        .add_plugins(DefaultPlugins.set(AssetPlugin {
            unapproved_path_mode: UnapprovedPathMode::Deny,
            ..Default::default()
        }))
        .add_systems(Startup, setup_scene_7k)
        .add_systems(Startup, load_bms_file)
        .add_systems(
            Update,
            (
                start_when_audio_ready,
                process_chart_events,
                render_visible_chart,
            ),
        )
        .run();
}

#[derive(Parser, Resource)]
struct ExecArgs {
    #[arg(long)]
    test_archive_path: Option<PathBuf>,
    #[arg(long)]
    bms_path: Option<PathBuf>,
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

fn load_bms_file(mut commands: Commands, asset_server: Res<AssetServer>, args: Res<ExecArgs>) {
    let Some(bms_path) = args.bms_path.as_ref() else {
        return;
    };
    let bms_bytes = std::fs::read(bms_path).unwrap();
    // Parse Bms
    let mut det = EncodingDetector::new();
    det.feed(&bms_bytes, true);
    let enc = det.guess(None, true);
    let (bms_str, _, _) = enc.decode(&bms_bytes);
    let BmsOutput { bms, warnings: _ } = bms_rs::bms::parse_bms(&bms_str, default_config());
    let bms = bms.unwrap();
    // Setup Processor
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or(BaseBpm(120.0.into()));
    let processor = BmsProcessor::new::<KeyLayoutBeat>(
        &bms,
        VisibleRangePerBpm::new(&base_bpm, Duration::from_secs_f32(0.6)),
    );
    // Load audio
    let mut audio_handles = HashMap::new();
    let mut audio_paths = HashMap::new();
    let bms_dir = bms_path.parent().unwrap_or(Path::new("."));
    for (id, audio_path) in processor.audio_files() {
        let base = bms_dir.join(audio_path);
        let candidates = choose_paths_by_ext(&base, &["flac", "wav", "ogg", "mp3"]);
        let chosen = candidates.first().cloned().unwrap_or(base);
        let ap = AssetPath::from_path(&chosen).with_source(AssetSourceId::from("fs"));
        let handle: Handle<AudioSource> = asset_server.load_override(ap);
        audio_handles.insert(id, handle);
        audio_paths.insert(id, chosen);
    }
    commands.insert_resource(BmsProcessStatus {
        processor,
        audio_handles,
        audio_paths,
        started: false,
        warned_missing: false,
    });
}

fn start_when_audio_ready(mut status: ResMut<BmsProcessStatus>, assets: Res<Assets<AudioSource>>) {
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
        status.processor.start_play(Instant::now());
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
    mut status: ResMut<BmsProcessStatus>,
    assets: Res<Assets<AudioSource>>,
) {
    if !status.started {
        return;
    }
    let now = Instant::now();
    let handles = status.audio_handles.clone();
    let mut to_spawn: Vec<(AudioPlayer, PlaybackSettings)> = Vec::new();
    for evp in status.processor.update(now) {
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
    mut status: ResMut<BmsProcessStatus>,
    mut vis: ResMut<ChartVisualState>,
    mut q_notes: Query<(&mut Transform, &mut Visibility), With<NoteMarker>>,
) {
    if !status.started {
        return;
    }
    let now = Instant::now();
    let mut alive: Vec<ChartEventId> = Vec::new();
    for ev in status.processor.visible_events(now) {
        let ChartEvent::Note { side, key, .. } = ev.event() else {
            continue;
        };
        if *side != PlayerSide::Player1 {
            continue;
        }
        let Some(idx) = key_to_lane(*key) else {
            continue;
        };
        let x = lane_x(idx);
        let y = -VISIBLE_HEIGHT / 2.0 + ev.display_ratio().as_f64() as f32 * VISIBLE_HEIGHT;
        if let Some(entity) = vis.notes.get(&ev.id()) {
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
            vis.notes.insert(ev.id(), entity);
        }
        alive.push(ev.id());
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
