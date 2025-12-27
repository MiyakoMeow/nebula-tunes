//! # Nebula Tunes 主程序

/// 系统配置模块
mod config;
/// 文件系统工具模块
mod filesystem;
mod loops;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use anyhow::Result;
use async_fs as afs;
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use bytemuck::{Pod, Zeroable};
use chardetng::EncodingDetector;
use clap::Parser;
use futures_lite::future;
use gametime::TimeSpan;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::{ElementState, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::WindowId,
};

use crate::config::load_sys;
use crate::loops::{ControlMsg, InputMsg, VisualMsg, audio, main_loop, visual};

#[derive(Parser)]
/// 命令行参数
struct ExecArgs {
    #[arg(long)]
    /// 指定要加载的 BMS 文件路径
    bms_path: Option<PathBuf>,
}

/// 将按键映射到轨道索引
const fn key_to_lane(key: Key) -> Option<usize> {
    match key {
        Key::Scratch(_) => Some(0),
        Key::Key(n) => match n {
            1..=7 => Some(n as usize),
            _ => None,
        },
        _ => None,
    }
}

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod)]
/// 单个矩形实例（位置、大小、颜色）
pub struct Instance {
    /// 中心坐标（x, y）
    pos: [f32; 2],
    /// 尺寸（宽, 高）
    size: [f32; 2],
    /// 颜色（RGBA）
    color: [f32; 4],
}

/// 视觉应用状态
struct App {
    /// 窗口实例
    window: winit::window::Window,
    /// 渲染器实例
    renderer: Option<visual::Renderer>,
    /// 视觉消息接收端
    visual_rx: mpsc::Receiver<VisualMsg>,
    /// 最新一帧的实例列表
    latest_instances: Vec<Instance>,
}

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.renderer.take();
    }
}

/// 视觉事件处理器
struct Handler {
    /// 可选的视觉应用状态
    app: Option<App>,
    /// 视觉消息接收端
    visual_rx: Option<mpsc::Receiver<VisualMsg>>,
    /// 控制消息发送端
    control_tx: mpsc::SyncSender<ControlMsg>,
    /// 输入消息发送端
    input_tx: mpsc::SyncSender<InputMsg>,
    /// 键位到轨道索引映射
    key_map: HashMap<KeyCode, usize>,
}

impl Handler {
    /// 创建视觉事件处理器并建立键位映射
    fn new(
        visual_rx: mpsc::Receiver<VisualMsg>,
        control_tx: mpsc::SyncSender<ControlMsg>,
        input_tx: mpsc::SyncSender<InputMsg>,
        key_codes: Vec<KeyCode>,
    ) -> Self {
        let mut map = HashMap::new();
        for (i, code) in key_codes.into_iter().enumerate().take(8) {
            map.insert(code, i);
        }
        Self {
            app: None,
            visual_rx: Some(visual_rx),
            control_tx,
            input_tx,
            key_map: map,
        }
    }
}

impl ApplicationHandler for Handler {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = winit::window::Window::default_attributes()
            .with_title("Nebula Tunes")
            .with_inner_size(LogicalSize::new(
                (visual::total_width() + visual::RIGHT_PANEL_GAP + visual::VISIBLE_HEIGHT) as f64
                    + 64.0,
                visual::VISIBLE_HEIGHT as f64 + 64.0,
            ));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => w,
            Err(_) => return,
        };

        let renderer = match (|| -> Result<visual::Renderer> {
            let instance = wgpu::Instance::default();
            let surface = unsafe {
                instance.create_surface_unsafe(
                    wgpu::SurfaceTargetUnsafe::from_window(&window)
                        .map_err(|e| anyhow::anyhow!(e.to_string()))?,
                )
            }?;
            let adapter =
                future::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    power_preference: wgpu::PowerPreference::HighPerformance,
                    force_fallback_adapter: false,
                    compatible_surface: Some(&surface),
                }))
                .map_err(|e| anyhow::anyhow!("request_adapter failed: {:?}", e))?;
            let (device, queue) =
                future::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                    memory_hints: wgpu::MemoryHints::default(),
                    trace: wgpu::Trace::Off,
                    label: None,
                }))?;
            let size = window.inner_size();
            let caps = surface.get_capabilities(&adapter);
            let format = caps
                .formats
                .first()
                .copied()
                .unwrap_or(wgpu::TextureFormat::Bgra8Unorm);
            let config = wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format,
                width: size.width,
                height: size.height,
                present_mode: wgpu::PresentMode::FifoRelaxed,
                alpha_mode: wgpu::CompositeAlphaMode::Opaque,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            };
            visual::Renderer::new(surface, device, queue, config)
        })() {
            Ok(r) => r,
            Err(_) => return,
        };

        let rx = match self.visual_rx.take() {
            Some(r) => r,
            None => return,
        };
        self.app = Some(App {
            window,
            renderer: Some(renderer),
            visual_rx: rx,
            latest_instances: visual::base_instances(),
        });
        let _ = self.control_tx.try_send(ControlMsg::Start);
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {}
            WindowEvent::Resized(size) => {
                if let Some(app) = self.app.as_mut()
                    && let Some(renderer) = app.renderer.as_mut()
                {
                    renderer.resize(size.width, size.height);
                    app.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let lane = match event.physical_key {
                    PhysicalKey::Code(code) => self.key_map.get(&code).copied(),
                    _ => None,
                };
                if let Some(idx) = lane {
                    match event.state {
                        ElementState::Pressed => {
                            let _ = self.input_tx.try_send(InputMsg::KeyDown(idx));
                        }
                        ElementState::Released => {
                            let _ = self.input_tx.try_send(InputMsg::KeyUp(idx));
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(app) = self.app.as_mut()
                    && let Some(renderer) = app.renderer.as_mut()
                {
                    loop {
                        match app.visual_rx.try_recv() {
                            Ok(msg) => match msg {
                                VisualMsg::Instances(instances) => {
                                    app.latest_instances = instances;
                                }
                                VisualMsg::Bga(path) => {
                                    let _ = renderer.update_bga_image_from_path(&path);
                                }
                            },
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => break,
                        }
                    }
                    let _ = renderer.draw(&app.latest_instances);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(app) = self.app.as_mut() {
            app.window.request_redraw();
        }
    }
}

/// 加载 BMS 文件并收集音频/BGA 资源路径映射
///
/// # Errors
///
/// - 读取 BMS 文件失败
/// - 编码探测或解码失败
/// - BMS 解析失败
async fn load_bms_and_collect_paths(
    bms_path: PathBuf,
    travel: TimeSpan,
) -> Result<(
    BmsProcessor,
    HashMap<WavId, PathBuf>,
    HashMap<BmpId, PathBuf>,
)> {
    let bms_bytes = afs::read(&bms_path).await?;
    let mut det = EncodingDetector::new();
    det.feed(&bms_bytes, true);
    let enc = det.guess(None, true);
    let (bms_str, _, _) = enc.decode(&bms_bytes);
    let BmsOutput { bms, warnings: _ } = bms_rs::bms::parse_bms(&bms_str, default_config());
    let Ok(bms) = bms else {
        anyhow::bail!("failed to parse BMS")
    };
    // print bms info
    println!("Title: {:?}", bms.music_info.title);
    println!("Artist: {:?}", bms.music_info.artist);
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or(BaseBpm(120.0.into()));
    println!("BaseBpm: {}", base_bpm.value());
    let processor =
        BmsProcessor::new::<KeyLayoutBeat>(&bms, VisibleRangePerBpm::new(&base_bpm, travel));
    let bms_dir = bms_path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let mut audio_paths: HashMap<WavId, PathBuf> = HashMap::new();
    let mut bmp_paths: HashMap<BmpId, PathBuf> = HashMap::new();
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
    // 为BMP资源建立路径映射（不加载）
    let bmp_list: Vec<PathBuf> = processor
        .bmp_files()
        .into_values()
        .map(std::path::Path::to_path_buf)
        .collect();
    let bmp_index =
        filesystem::choose_paths_by_ext_async(&bms_dir, &bmp_list, &["bmp", "jpg", "jpeg", "png"])
            .await;
    for (id, bmp_path) in processor.bmp_files().into_iter() {
        let stem = bmp_path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(std::string::ToString::to_string);
        let base = bms_dir.join(bmp_path);
        let chosen = stem
            .and_then(|s| bmp_index.get(&s).cloned())
            .unwrap_or(base);
        bmp_paths.insert(id, chosen);
    }
    Ok((processor, audio_paths, bmp_paths))
}

fn main() -> Result<()> {
    let sys = load_sys(Path::new("config_sys.toml"))?;
    let args = ExecArgs::parse();
    let event_loop = EventLoop::new()?;
    let (pre_processor, pre_audio_paths, pre_bmp_paths) = if let Some(bms_path) = args.bms_path {
        let (p, ap, bp) = future::block_on(load_bms_and_collect_paths(
            bms_path,
            sys.judge.visible_travel,
        ))?;
        (Some(p), ap, bp)
    } else {
        (None, HashMap::new(), HashMap::new())
    };
    let (control_tx, control_rx) = mpsc::sync_channel::<loops::ControlMsg>(1);
    let (visual_tx, visual_rx) = mpsc::sync_channel::<VisualMsg>(2);
    let (input_tx, input_rx) = mpsc::sync_channel::<InputMsg>(64);
    let (audio_tx, audio_rx) = mpsc::sync_channel::<audio::Msg>(64);
    let (audio_event_tx, audio_event_rx) = mpsc::sync_channel::<audio::Event>(1);
    let _audio_thread = thread::spawn(move || {
        audio::run_audio_loop(audio_rx, audio_event_tx);
    });
    let _main_thread = thread::spawn(move || {
        main_loop::run(
            pre_processor,
            pre_audio_paths,
            pre_bmp_paths,
            control_rx,
            visual_tx,
            input_rx,
            main_loop::JudgeParams {
                travel: sys.judge.visible_travel,
                windows: sys.judge.windows(),
            },
            audio_tx,
            audio_event_rx,
        );
    });
    let mut handler = Handler::new(visual_rx, control_tx, input_tx, sys.keys.lanes);
    event_loop.run_app(&mut handler)?;
    Ok(())
}
