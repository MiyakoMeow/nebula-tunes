//! # Nebula Tunes 主程序

#![warn(missing_docs)]
#![warn(clippy::must_use_candidate)]
#![warn(clippy::must_use_unit)]
#![warn(clippy::redundant_clone)]
#![warn(clippy::redundant_closure_for_method_calls)]
#![warn(clippy::redundant_else)]
#![warn(clippy::redundant_feature_names)]

mod filesystem;

use std::{collections::HashMap, path::Path, path::PathBuf, time::Duration};

use anyhow::Result;
use async_fs as afs;
use bms_rs::chart_process::types::PlayheadEvent;
use bms_rs::{bms::prelude::*, chart_process::prelude::*};
use bytemuck::{Pod, Zeroable};
use chardetng::EncodingDetector;
use clap::Parser;
use gametime::{TimeSpan, TimeStamp};
use rodio::{Sink, stream::OutputStream};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler, dpi::LogicalSize, event::WindowEvent,
    event_loop::ActiveEventLoop, event_loop::EventLoop, window::WindowId,
};

#[derive(Parser)]
struct ExecArgs {
    #[arg(long)]
    bms_path: Option<PathBuf>,
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

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod)]
struct Instance {
    pos: [f32; 2],
    size: [f32; 2],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Zeroable, Pod)]
struct ScreenUniform {
    size: [f32; 2],
}

struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    screen_buffer: wgpu::Buffer,
    quad_vb: wgpu::Buffer,
    idx_buf: wgpu::Buffer,
    instance_buf: wgpu::Buffer,
    index_count: u32,
    window: winit::window::Window,
    logical_size: [f32; 2],
}

impl Renderer {
    async fn new(window: winit::window::Window) -> Result<Self> {
        let instance = wgpu::Instance::default();
        let surface = unsafe {
            instance.create_surface_unsafe(
                wgpu::SurfaceTargetUnsafe::from_window(&window)
                    .map_err(|e| anyhow::anyhow!(e.to_string()))?,
            )
        }?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: Some(&surface),
            })
            .await
            .map_err(|e| anyhow::anyhow!("request_adapter failed: {:?}", e))?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::default(),
                trace: wgpu::Trace::Off,
                label: None,
            })
            .await?;
        let size = window.inner_size();
        let format = surface.get_capabilities(&adapter).formats[0];
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: wgpu::CompositeAlphaMode::Opaque,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("rect.wgsl").into()),
        });
        let screen_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screen-uniform"),
            size: std::mem::size_of::<ScreenUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rect-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rect-bg"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: screen_buffer.as_entire_binding(),
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("rect-pl"),
            bind_group_layouts: &[&bind_group_layout],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<[f32; 2]>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &[wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x2,
                            offset: 0,
                            shader_location: 0,
                        }],
                    },
                    wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &[
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 0,
                                shader_location: 1,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x2,
                                offset: 8,
                                shader_location: 2,
                            },
                            wgpu::VertexAttribute {
                                format: wgpu::VertexFormat::Float32x4,
                                offset: 16,
                                shader_location: 3,
                            },
                        ],
                    },
                ],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let quad_vertices: [[f32; 2]; 4] = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]];
        let quad_vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-vb"),
            contents: bytemuck::cast_slice(&quad_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let indices: [u16; 6] = [0, 1, 2, 0, 2, 3];
        let idx_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("quad-ib"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let instance_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance-buf"),
            size: (std::mem::size_of::<Instance>() * 1024) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let logical_size = [size.width as f32, size.height as f32];
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            screen_buffer,
            quad_vb,
            idx_buf,
            instance_buf,
            index_count: 6,
            window,
            logical_size,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.logical_size = [width as f32, height as f32];
        }
    }

    fn upload_screen_uniform(&self) {
        let uni = ScreenUniform {
            size: self.logical_size,
        };
        self.queue
            .write_buffer(&self.screen_buffer, 0, bytemuck::bytes_of(&uni));
    }

    fn draw(&self, instances: &[Instance]) -> Result<()> {
        self.upload_screen_uniform();
        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.queue
            .write_buffer(&self.instance_buf, 0, bytemuck::cast_slice(instances));
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });
        {
            let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("render-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            rpass.set_pipeline(&self.pipeline);
            rpass.set_bind_group(0, &self.bind_group, &[]);
            rpass.set_vertex_buffer(0, self.quad_vb.slice(..));
            rpass.set_vertex_buffer(1, self.instance_buf.slice(..));
            rpass.set_index_buffer(self.idx_buf.slice(..), wgpu::IndexFormat::Uint16);
            rpass.draw_indexed(0..self.index_count, 0, 0..instances.len() as u32);
        }
        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

struct Audio {
    stream: OutputStream,
    sinks: Vec<Sink>,
    cache: HashMap<PathBuf, Arc<[u8]>>,
}

impl Audio {
    fn new() -> Result<Self> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()?;
        Ok(Self {
            stream,
            sinks: Vec::new(),
            cache: HashMap::new(),
        })
    }

    fn cached_bytes(&mut self, path: &Path) -> Result<Arc<[u8]>> {
        if let Some(b) = self.cache.get(path) {
            return Ok(b.clone());
        }
        let bytes = std::fs::read(path)?;
        let arc: Arc<[u8]> = Arc::from(bytes);
        self.cache.insert(path.to_path_buf(), arc.clone());
        Ok(arc)
    }

    fn play_file(&mut self, path: &Path) -> Result<()> {
        let bytes = self.cached_bytes(path)?;
        let cursor = std::io::Cursor::new(bytes);
        let sink = rodio::play(self.stream.mixer(), cursor)?;
        self.sinks.push(sink);
        Ok(())
    }

    fn cleanup(&mut self) {
        self.sinks.retain(|s| !s.empty());
    }
}

struct App {
    renderer: Renderer,
    processor: Option<BmsProcessor>,
    audio_paths: HashMap<WavId, PathBuf>,
    last_log_sec: u64,
    audio_plays_this_sec: u32,
    audio: Option<Audio>,
}

impl App {
    fn handle_audio_events(&mut self, events: &[PlayheadEvent], _now: TimeStamp) {
        let Some(p) = self.processor.as_ref() else {
            return;
        };
        let Some(_start) = p.started_at() else { return };
        let Some(audio) = self.audio.as_mut() else {
            return;
        };
        for ev in events {
            if let ChartEvent::Note {
                side, key, wav_id, ..
            } = ev.event()
            {
                if *side != PlayerSide::Player1 {
                    continue;
                }
                let Some(_idx) = key_to_lane(*key) else {
                    continue;
                };
                if let Some(wav_id) = wav_id.as_ref()
                    && let Some(path) = self.audio_paths.get(wav_id)
                    && audio.play_file(path).is_ok()
                {
                    self.audio_plays_this_sec = self.audio_plays_this_sec.saturating_add(1);
                }
            }
            if let ChartEvent::Bgm { wav_id } = ev.event()
                && let Some(wav_id) = wav_id.as_ref()
                && let Some(path) = self.audio_paths.get(wav_id)
                && audio.play_file(path).is_ok()
            {
                self.audio_plays_this_sec = self.audio_plays_this_sec.saturating_add(1);
            }
        }
    }

    fn start_if_ready(&mut self, now: TimeStamp) {
        if let Some(p) = &mut self.processor
            && p.started_at().is_none()
        {
            p.start_play(now);
            self.last_log_sec = 0;
            self.audio_plays_this_sec = 0;
        }
    }

    fn build_instances(&mut self) -> Vec<Instance> {
        let mut instances: Vec<Instance> = Vec::with_capacity(1024);
        for i in 0..LANE_COUNT {
            instances.push(Instance {
                pos: [lane_x(i), 0.0],
                size: [LANE_WIDTH, VISIBLE_HEIGHT],
                color: [0.15, 0.15, 0.18, 1.0],
            });
        }
        instances.push(Instance {
            pos: [0.0, -VISIBLE_HEIGHT / 2.0 + 2.0],
            size: [total_width(), 4.0],
            color: [0.9, 0.9, 0.9, 1.0],
        });
        if let Some(p) = self.processor.as_mut()
            && p.started_at().is_some()
        {
            for (ev, ratio) in p.visible_events() {
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
                let y = -VISIBLE_HEIGHT / 2.0 + ratio.as_f64() as f32 * VISIBLE_HEIGHT;
                instances.push(Instance {
                    pos: [x, y],
                    size: [LANE_WIDTH - 4.0, NOTE_HEIGHT],
                    color: [0.3, 0.7, 1.0, 1.0],
                });
            }
        }
        instances
    }

    fn log_tick(&mut self, now: TimeStamp) {
        let Some(p) = self.processor.as_mut() else {
            return;
        };
        let Some(start) = p.started_at() else { return };
        let elapsed = now.checked_elapsed_since(start).unwrap_or(TimeSpan::ZERO);
        let sec = (elapsed.as_nanos().max(0) as u64) / 1_000_000_000;
        if sec != self.last_log_sec {
            let visible = p.visible_events().count();
            println!(
                "elapsed={}s visible={} audio={}",
                sec, visible, self.audio_plays_this_sec
            );
            self.audio_plays_this_sec = 0;
            self.last_log_sec = sec;
        }
    }
}

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
    // print bms info
    println!("Title: {:?}", bms.music_info.title);
    println!("Artist: {:?}", bms.music_info.artist);
    let base_bpm = StartBpmGenerator
        .generate(&bms)
        .unwrap_or(BaseBpm(120.0.into()));
    println!("BaseBpm: {}", base_bpm.value());
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

fn main() -> Result<()> {
    let args = ExecArgs::parse();
    let event_loop = EventLoop::new()?;
    let (pre_processor, pre_audio_paths) = if let Some(bms_path) = args.bms_path {
        let (p, ap) = pollster::block_on(load_bms_and_collect_paths(bms_path))?;
        (Some(p), ap)
    } else {
        (None, HashMap::new())
    };
    struct Handler {
        app: Option<App>,
        pre_processor: Option<BmsProcessor>,
        pre_audio_paths: HashMap<WavId, PathBuf>,
    }
    impl ApplicationHandler for Handler {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let attrs = winit::window::Window::default_attributes()
                .with_title("Nebula Tunes")
                .with_inner_size(LogicalSize::new(
                    total_width() as f64 + 64.0,
                    VISIBLE_HEIGHT as f64 + 64.0,
                ));
            let window = match event_loop.create_window(attrs) {
                Ok(w) => w,
                Err(_) => return,
            };
            let renderer = match pollster::block_on(Renderer::new(window)) {
                Ok(r) => r,
                Err(_) => return,
            };
            self.app = Some(App {
                renderer,
                processor: self.pre_processor.take(),
                audio_paths: std::mem::take(&mut self.pre_audio_paths),
                last_log_sec: 0,
                audio_plays_this_sec: 0,
                audio: Audio::new().ok(),
            });
        }
        fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            _id: WindowId,
            event: WindowEvent,
        ) {
            match event {
                WindowEvent::CloseRequested => {
                    event_loop.exit();
                }
                WindowEvent::Resized(size) => {
                    if let Some(app) = self.app.as_mut() {
                        app.renderer.resize(size.width, size.height);
                        app.renderer.window.request_redraw();
                    }
                }
                WindowEvent::RedrawRequested => {
                    let now = TimeStamp::now();
                    if let Some(app) = self.app.as_mut() {
                        app.start_if_ready(now);
                        if let Some(p) = app.processor.as_mut() {
                            let events: Vec<_> = p.update(now).collect();
                            app.handle_audio_events(&events, now);
                        }
                        let instances = app.build_instances();
                        let _ = app.renderer.draw(&instances);
                        app.log_tick(now);
                        if let Some(a) = app.audio.as_mut() {
                            a.cleanup();
                        }
                    }
                }
                _ => {}
            }
        }
        fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
            if let Some(app) = self.app.as_mut() {
                app.renderer.window.request_redraw();
            }
        }
    }
    let mut handler = Handler {
        app: None,
        pre_processor,
        pre_audio_paths,
    };
    event_loop.run_app(&mut handler)?;
    Ok(())
}
