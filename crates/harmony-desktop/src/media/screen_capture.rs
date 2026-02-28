use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use scap::Target;
use scap::capturer::{Capturer, Options, Resolution};
use scap::frame::{Frame, FrameType, VideoFrame};
use tokio::sync::mpsc;
use vk_video::parameters::{RateControl, VideoParameters};
use vk_video::{BytesEncoder, RawFrameData, VulkanDevice, VulkanInstance};

use crate::media::{
    codec,
    video::{Frame as DecodedFrame, hardware::nv12_to_rgba},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenQuality {
    P720,
    P1080,
    P1440,
}

impl ScreenQuality {
    fn to_resolution(self) -> Resolution {
        match self {
            ScreenQuality::P720 => Resolution::_720p,
            ScreenQuality::P1080 => Resolution::_1080p,
            ScreenQuality::P1440 => Resolution::_1440p,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScreenCaptureConfig {
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub quality: ScreenQuality,
}

impl Default for ScreenCaptureConfig {
    fn default() -> Self {
        Self {
            fps: 30,
            bitrate_kbps: 2500,
            quality: ScreenQuality::P1080,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CaptureTargetInfo {
    pub title: String,
    pub target: Target,
    pub thumbnail: Option<DecodedFrame>,
}

pub struct ScreenCaptureSession {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ScreenCaptureSession {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn list_targets_with_thumbnails() -> Vec<CaptureTargetInfo> {
    scap::get_all_targets()
        .into_iter()
        .map(|target| {
            let title = target_label(&target);
            let thumbnail = capture_thumbnail(&target).ok();
            CaptureTargetInfo {
                title,
                target,
                thumbnail,
            }
        })
        .collect()
}

pub fn start_screen_capture(
    target: Target,
    config: ScreenCaptureConfig,
) -> Result<(ScreenCaptureSession, mpsc::UnboundedReceiver<Vec<u8>>)> {
    let (tx, rx) = mpsc::unbounded_channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);

    let handle = thread::spawn(move || {
        let options = Options {
            fps: config.fps.max(1),
            show_cursor: true,
            show_highlight: false,
            target: Some(target),
            crop_area: None,
            output_type: FrameType::YUVFrame,
            output_resolution: config.quality.to_resolution(),
            excluded_targets: None,
            captures_audio: false,
            exclude_current_process_audio: false,
        };

        let mut capturer = match Capturer::build(options) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("screen capture init failed: {e}");
                return;
            }
        };

        capturer.start_capture();

        let mut encoder: Option<BytesEncoder> = None;

        while !stop_thread.load(Ordering::Relaxed) {
            let frame = match capturer.get_next_frame() {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::warn!("screen capture frame error: {e}");
                    break;
                }
            };

            let (width, height, nv12) = match frame {
                Frame::Video(VideoFrame::YUVFrame(yuv)) => {
                    let width = (yuv.width.max(1)) as u32;
                    let height = (yuv.height.max(1)) as u32;
                    let mut nv12 =
                        Vec::with_capacity(yuv.luminance_bytes.len() + yuv.chrominance_bytes.len());
                    nv12.extend_from_slice(&yuv.luminance_bytes);
                    nv12.extend_from_slice(&yuv.chrominance_bytes);
                    (width, height, nv12)
                }
                Frame::Video(VideoFrame::BGRA(bgra)) => {
                    let width = (bgra.width.max(1)) as u32;
                    let height = (bgra.height.max(1)) as u32;
                    let nv12 = bgra_to_nv12(width, height, &bgra.data);
                    (width, height, nv12)
                }
                _ => {
                    tracing::warn!("screen capture returned unsupported frame type; dropping");
                    continue;
                }
            };

            if encoder.is_none() {
                match create_encoder(width, height, &config) {
                    Ok(enc) => encoder = Some(enc),
                    Err(e) => {
                        tracing::error!("failed to initialize screen encoder: {e:#}");
                        break;
                    }
                }
            }

            let raw = RawFrameData {
                frame: nv12,
                width,
                height,
            };

            let enc = encoder.as_mut().expect("encoder is initialized");
            match enc.encode(
                &vk_video::Frame {
                    data: raw,
                    pts: None,
                },
                false,
            ) {
                Ok(chunk) => {
                    let packet = codec::prepend_codec_byte(codec::VIDEO_H264, &chunk.data);
                    let _ = tx.send(packet);
                }
                Err(e) => {
                    if matches!(e, vk_video::VulkanEncoderError::NoMemory) {
                        tracing::warn!("screen frame dropped due to GPU memory pressure: {e}");
                    } else {
                        tracing::warn!("screen encode failed; dropping frame: {e}");
                    }
                }
            }
        }

        capturer.stop_capture();
    });

    Ok((
        ScreenCaptureSession {
            stop,
            handle: Some(handle),
        },
        rx,
    ))
}

fn create_encoder(width: u32, height: u32, config: &ScreenCaptureConfig) -> Result<BytesEncoder> {
    let instance = VulkanInstance::new().context("failed to create Vulkan instance")?;
    let adapter = instance
        .create_adapter(None)
        .context("failed to create Vulkan adapter")?;
    let device: Arc<VulkanDevice> = adapter
        .create_device(
            wgpu::Features::empty(),
            wgpu::ExperimentalFeatures::disabled(),
            wgpu::Limits::default(),
        )
        .context("failed to create Vulkan device")?;

    let video = VideoParameters {
        width: NonZeroU32::new(width).context("invalid width")?,
        height: NonZeroU32::new(height).context("invalid height")?,
        target_framerate: config.fps.max(1).into(),
    };

    let avg_bitrate = config.bitrate_kbps.max(250) as u64 * 1000;
    let max_bitrate = avg_bitrate.saturating_mul(2);

    let params = device
        .encoder_parameters_high_quality(
            video,
            RateControl::VariableBitrate {
                average_bitrate: avg_bitrate,
                max_bitrate,
                virtual_buffer_size: Duration::from_secs(2),
            },
        )
        .context("failed to create encoder parameters")?;

    device
        .create_bytes_encoder(params)
        .context("failed to create bytes encoder")
}

fn target_label(target: &Target) -> String {
    match target {
        Target::Display(d) => format!("Display: {}", d.title),
        Target::Window(w) => format!("Window: {}", w.title),
    }
}

fn capture_thumbnail(target: &Target) -> Result<DecodedFrame> {
    let options = Options {
        fps: 1,
        target: Some(target.clone()),
        output_type: FrameType::BGRAFrame,
        output_resolution: Resolution::_720p,
        ..Default::default()
    };

    let mut capturer = Capturer::build(options).context("thumbnail capture init failed")?;
    capturer.start_capture();
    let frame = capturer
        .get_next_frame()
        .context("thumbnail capture frame failed")?;
    capturer.stop_capture();

    let (width, height, thumb) = match frame {
        Frame::Video(VideoFrame::BGRA(f)) => (
            f.width as u32,
            f.height as u32,
            bgra_to_rgba(f.width as u32, f.height as u32, &f.data),
        ),
        Frame::Video(VideoFrame::BGR0(f)) => (
            f.width as u32,
            f.height as u32,
            bgr0_to_rgba(f.width as u32, f.height as u32, &f.data),
        ),
        Frame::Video(VideoFrame::RGB(f)) => (
            f.width as u32,
            f.height as u32,
            rgb_to_rgba(f.width as u32, f.height as u32, &f.data),
        ),
        Frame::Video(VideoFrame::YUVFrame(f)) => {
            let mut nv12 = Vec::with_capacity(f.luminance_bytes.len() + f.chrominance_bytes.len());
            nv12.extend_from_slice(&f.luminance_bytes);
            nv12.extend_from_slice(&f.chrominance_bytes);
            (
                f.width as u32,
                f.height as u32,
                nv12_to_rgba(&nv12, f.width as u32, f.height as u32),
            )
        }
        _ => anyhow::bail!("unsupported frame type for thumbnail"),
    };

    Ok(DecodedFrame {
        width,
        height,
        rgba: thumb.into(),
    })
}

fn bgra_to_rgba(_width: u32, _height: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(4) {
        out.push(chunk[2]); // R
        out.push(chunk[1]); // G
        out.push(chunk[0]); // B
        out.push(chunk[3]); // A
    }
    out
}

fn bgr0_to_rgba(_width: u32, _height: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    for chunk in data.chunks_exact(4) {
        out.push(chunk[2]); // R
        out.push(chunk[1]); // G
        out.push(chunk[0]); // B
        out.push(255); // A (opaque)
    }
    out
}

fn rgb_to_rgba(_width: u32, _height: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 3 * 4);
    for chunk in data.chunks_exact(3) {
        out.push(chunk[0]); // R
        out.push(chunk[1]); // G
        out.push(chunk[2]); // B
        out.push(255); // A (opaque)
    }
    out
}

fn bgra_to_nv12(width: u32, height: u32, data: &[u8]) -> Vec<u8> {
    let y_size = (width * height) as usize;
    let uv_size = y_size / 2;
    let mut y_plane = vec![0u8; y_size];
    let mut uv_plane = vec![0u8; uv_size];

    let mut image = yuv::YuvBiPlanarImageMut {
        y_plane: yuv::BufferStoreMut::Borrowed(&mut y_plane),
        y_stride: width,
        uv_plane: yuv::BufferStoreMut::Borrowed(&mut uv_plane),
        uv_stride: width,
        width,
        height,
    };

    yuv::bgra_to_yuv_nv12(
        &mut image,
        data,
        width * 4,
        yuv::YuvRange::Limited,
        yuv::YuvStandardMatrix::Bt709,
        yuv::YuvConversionMode::Balanced,
    )
    .expect("BGRA to NV12 conversion failed");

    drop(image);
    let mut nv12 = y_plane;
    nv12.extend_from_slice(&uv_plane);
    nv12
}
