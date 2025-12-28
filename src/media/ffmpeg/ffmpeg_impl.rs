//! `FFmpeg` 视频解码器实现（桌面端）

use super::{DecodedFrame, VideoDecoder};
use anyhow::{Context, Result};
use ffmpeg_next as ffmpeg;
use std::path::Path;

/// `FFmpeg` 视频解码器
pub struct FFmpegVideoDecoder {
    decoder: ffmpeg::decoder::Video,
    scaler: ffmpeg::software::scaling::Context,
    frame_index: u64,
    width: u32,
    height: u32,
    fps: f64,
    input: ffmpeg::format::context::Input,
}

impl FFmpegVideoDecoder {
    /// 创建新的 `FFmpeg` 视频解码器
    pub fn new(path: &Path) -> Result<Self> {
        ffmpeg::init()?;

        let input = ffmpeg::format::input(&path)
            .with_context(|| format!("无法打开视频文件: {}", path.display()))?;

        let video_stream = input
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or_else(|| anyhow::anyhow!("未找到视频流"))?;

        let context_decoder =
            ffmpeg::codec::context::Context::from_parameters(video_stream.parameters())
                .with_context(|| "无法创建解码器上下文")?;

        let decoder = context_decoder
            .decoder()
            .video()
            .with_context(|| "无法创建视频解码器")?;

        let width = decoder.width();
        let height = decoder.height();

        // 计算 FPS
        let fps = video_stream.avg_frame_rate().numerator() as f64
            / video_stream.avg_frame_rate().denominator() as f64;

        // 配置像素格式转换为 RGBA8
        let scaler = ffmpeg::software::scaling::Context::get(
            decoder.format(),
            width,
            height,
            ffmpeg::format::Pixel::RGBA,
            width,
            height,
            ffmpeg::software::scaling::Flags::BILINEAR,
        )
        .with_context(|| "无法创建缩放器")?;

        Ok(Self {
            decoder,
            scaler,
            frame_index: 0,
            width,
            height,
            fps,
            input,
        })
    }
}

impl VideoDecoder for FFmpegVideoDecoder {
    fn decode_next_frame(&mut self) -> Result<Option<DecodedFrame>> {
        let _packet = ffmpeg::Packet::empty();

        // 读取数据包并发送给解码器
        loop {
            let mut frame = ffmpeg::frame::Video::empty();
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => {
                    // 成功解码一帧
                    let mut rgba_frame = ffmpeg::frame::Video::empty();
                    self.scaler.run(&frame, &mut rgba_frame)?;

                    let width = rgba_frame.width();
                    let height = rgba_frame.height();
                    let stride = rgba_frame.stride(0);
                    let data = rgba_frame.data(0);

                    // 复制 RGBA 数据
                    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                    for y in 0..height {
                        let row_start = y as usize * stride;
                        let row_end = row_start + (width * 4) as usize;
                        rgba.extend_from_slice(&data[row_start..row_end]);
                    }

                    let timestamp = frame.timestamp().unwrap_or(0) as f64;

                    self.frame_index += 1;
                    return Ok(Some(DecodedFrame {
                        rgba,
                        width,
                        height,
                        timestamp,
                        frame_index: self.frame_index - 1,
                    }));
                }
                Err(ffmpeg::Error::Eof) => {
                    return Ok(None);
                }
                Err(ffmpeg::Error::Other { errno: _ }) => {
                    // 需要更多数据包
                    match self.input.packets().next() {
                        Some((_stream, pkt)) => {
                            self.decoder.send_packet(&pkt)?;
                        }
                        None => {
                            // 文件结束，发送空包以刷新解码器
                            self.decoder.send_packet(&ffmpeg::Packet::empty())?;
                        }
                    }
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
    }

    fn seek_to_frame(&mut self, frame_idx: u64) -> Result<()> {
        let target_time = frame_idx as f64 / self.fps;

        // 转换为流时间基
        if let Some(video_stream) = self.input.streams().best(ffmpeg::media::Type::Video) {
            let time_base = video_stream.time_base();
            let timestamp = (target_time / time_base.numerator() as f64
                * time_base.denominator() as f64) as i64;

            self.input.seek(timestamp, ..)?;
            self.decoder.flush();
            self.frame_index = frame_idx;
        }

        Ok(())
    }

    fn current_frame_index(&self) -> u64 {
        self.frame_index
    }

    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn fps(&self) -> f64 {
        self.fps
    }
}
