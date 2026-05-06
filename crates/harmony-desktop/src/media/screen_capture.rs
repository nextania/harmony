use std::num::NonZeroU32;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use anyhow::{ensure, Context, Result};
use scap::capturer::{Capturer, Options, Resolution};
use scap::frame::{Frame, FrameType, VideoFrame, YUVFrame};
use scap::Target;
use tokio::sync::mpsc;
use vk_video::parameters::{RateControl, VideoParameters};
use vk_video::{BytesEncoder, RawFrameData, VulkanDevice, VulkanInstance};

use crate::media::{
    codec,
    video::{hardware::nv12_to_rgba, Frame as DecodedFrame},
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
        let mut encoder_size: Option<(u32, u32)> = None;

        while !stop_thread.load(Ordering::Relaxed) {
            let frame = match capturer.get_next_frame() {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::warn!("screen capture frame error: {e}");
                    break;
                }
            };

            let (width, height, nv12) = match frame_to_nv12(frame) {
                Ok(frame) => frame,
                Err(e) => {
                    tracing::warn!("screen capture frame conversion failed; dropping frame: {e:#}");
                    continue;
                }
            };

            if encoder.is_none() || encoder_size != Some((width, height)) {
                match create_encoder(width, height, &config) {
                    Ok(enc) => {
                        encoder = Some(enc);
                        encoder_size = Some((width, height));
                    }
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

    let (width, height, thumb) = frame_to_rgba(frame)?;

    Ok(DecodedFrame {
        width,
        height,
        rgba: thumb.into(),
    })
}

fn frame_to_nv12(frame: Frame) -> Result<(u32, u32, Vec<u8>)> {
    match frame {
        Frame::Video(VideoFrame::YUVFrame(yuv)) => yuv_frame_to_nv12(&yuv),
        Frame::Video(VideoFrame::BGRA(bgra)) => packed_4_to_nv12(
            dim(bgra.width),
            dim(bgra.height),
            &bgra.data,
            yuv::bgra_to_yuv_nv12,
        ),
        Frame::Video(VideoFrame::BGRx(bgrx)) => packed_4_to_nv12(
            dim(bgrx.width),
            dim(bgrx.height),
            &bgrx.data,
            yuv::bgra_to_yuv_nv12,
        ),
        Frame::Video(VideoFrame::RGBx(rgbx)) => packed_4_to_nv12(
            dim(rgbx.width),
            dim(rgbx.height),
            &rgbx.data,
            yuv::rgba_to_yuv_nv12,
        ),
        Frame::Video(VideoFrame::XBGR(xbgr)) => {
            let (width, height, rgba) =
                xbgr_to_rgba(dim(xbgr.width), dim(xbgr.height), &xbgr.data)?;
            packed_4_to_nv12(width, height, &rgba, yuv::rgba_to_yuv_nv12)
        }
        Frame::Video(VideoFrame::BGR0(bgr)) => packed_4_to_nv12(
            dim(bgr.width),
            dim(bgr.height),
            &bgr.data,
            yuv::bgra_to_yuv_nv12,
        ),
        Frame::Video(VideoFrame::RGB(rgb)) => packed_3_to_nv12(
            dim(rgb.width),
            dim(rgb.height),
            &rgb.data,
            yuv::rgb_to_yuv_nv12,
        ),
        _ => anyhow::bail!("unsupported screen capture frame type"),
    }
}

fn frame_to_rgba(frame: Frame) -> Result<(u32, u32, Vec<u8>)> {
    match frame {
        Frame::Video(VideoFrame::BGRA(f)) => bgra_to_rgba(dim(f.width), dim(f.height), &f.data),
        Frame::Video(VideoFrame::BGRx(f)) => bgrx_to_rgba(dim(f.width), dim(f.height), &f.data),
        Frame::Video(VideoFrame::RGBx(f)) => rgbx_to_rgba(dim(f.width), dim(f.height), &f.data),
        Frame::Video(VideoFrame::XBGR(f)) => xbgr_to_rgba(dim(f.width), dim(f.height), &f.data),
        Frame::Video(VideoFrame::BGR0(f)) => bgr0_to_rgba(dim(f.width), dim(f.height), &f.data),
        Frame::Video(VideoFrame::RGB(f)) => rgb_to_rgba(dim(f.width), dim(f.height), &f.data),
        Frame::Video(VideoFrame::YUVFrame(f)) => {
            let (width, height, nv12) = yuv_frame_to_nv12(&f)?;
            Ok((width, height, nv12_to_rgba(&nv12, width, height)))
        }
        _ => anyhow::bail!("unsupported frame type for thumbnail"),
    }
}

fn yuv_frame_to_nv12(frame: &YUVFrame) -> Result<(u32, u32, Vec<u8>)> {
    let (width, height) = even_dimensions(dim(frame.width), dim(frame.height))?;
    let y_stride = positive_stride(frame.luminance_stride, width)?;
    let uv_stride = positive_stride(frame.chrominance_stride, width)?;

    let y_plane = copy_plane_rows(
        &frame.luminance_bytes,
        y_stride,
        width as usize,
        height as usize,
    )
    .context("invalid NV12 luminance plane")?;
    let uv_plane = copy_plane_rows(
        &frame.chrominance_bytes,
        uv_stride,
        width as usize,
        (height / 2) as usize,
    )
    .context("invalid NV12 chrominance plane")?;

    let mut nv12 = y_plane;
    nv12.extend_from_slice(&uv_plane);
    Ok((width, height, nv12))
}

fn packed_4_to_nv12(
    width: u32,
    height: u32,
    data: &[u8],
    convert: fn(
        &mut yuv::YuvBiPlanarImageMut<'_, u8>,
        &[u8],
        u32,
        yuv::YuvRange,
        yuv::YuvStandardMatrix,
        yuv::YuvConversionMode,
    ) -> std::result::Result<(), yuv::YuvError>,
) -> Result<(u32, u32, Vec<u8>)> {
    let source_width = width.max(1);
    let (width, height) = even_dimensions(width, height)?;
    let stride = source_width * 4;
    ensure_packed_len(data, stride, width * 4, height)?;
    let nv12 = convert_packed_to_nv12(width, height, data, stride, convert)?;
    Ok((width, height, nv12))
}

fn packed_3_to_nv12(
    width: u32,
    height: u32,
    data: &[u8],
    convert: fn(
        &mut yuv::YuvBiPlanarImageMut<'_, u8>,
        &[u8],
        u32,
        yuv::YuvRange,
        yuv::YuvStandardMatrix,
        yuv::YuvConversionMode,
    ) -> std::result::Result<(), yuv::YuvError>,
) -> Result<(u32, u32, Vec<u8>)> {
    let source_width = width.max(1);
    let (width, height) = even_dimensions(width, height)?;
    let stride = source_width * 3;
    ensure_packed_len(data, stride, width * 3, height)?;
    let nv12 = convert_packed_to_nv12(width, height, data, stride, convert)?;
    Ok((width, height, nv12))
}

fn convert_packed_to_nv12(
    width: u32,
    height: u32,
    data: &[u8],
    stride: u32,
    convert: fn(
        &mut yuv::YuvBiPlanarImageMut<'_, u8>,
        &[u8],
        u32,
        yuv::YuvRange,
        yuv::YuvStandardMatrix,
        yuv::YuvConversionMode,
    ) -> std::result::Result<(), yuv::YuvError>,
) -> Result<Vec<u8>> {
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

    convert(
        &mut image,
        data,
        stride,
        yuv::YuvRange::Limited,
        yuv::YuvStandardMatrix::Bt709,
        yuv::YuvConversionMode::Balanced,
    )
    .context("packed frame to NV12 conversion failed")?;

    drop(image);
    let mut nv12 = y_plane;
    nv12.extend_from_slice(&uv_plane);
    Ok(nv12)
}

fn dim(value: i32) -> u32 {
    value.max(1) as u32
}

fn ensure_packed_len(data: &[u8], stride: u32, row_bytes: u32, height: u32) -> Result<()> {
    let needed = stride as usize * (height.saturating_sub(1)) as usize + row_bytes as usize;
    ensure!(
        data.len() >= needed,
        "packed frame has {} bytes but needs at least {needed}",
        data.len()
    );
    Ok(())
}

fn even_dimensions(width: u32, height: u32) -> Result<(u32, u32)> {
    let width = width & !1;
    let height = height & !1;
    ensure!(width > 0 && height > 0, "frame must be at least 2x2 pixels");
    Ok((width, height))
}

fn positive_stride(stride: i32, min_stride: u32) -> Result<usize> {
    let stride = if stride <= 0 {
        min_stride
    } else {
        stride as u32
    };
    ensure!(
        stride >= min_stride,
        "frame stride {stride} is smaller than row width {min_stride}"
    );
    Ok(stride as usize)
}

fn copy_plane_rows(src: &[u8], stride: usize, row_bytes: usize, rows: usize) -> Result<Vec<u8>> {
    if rows == 0 {
        return Ok(Vec::new());
    }
    let needed = stride * (rows - 1) + row_bytes;
    ensure!(
        src.len() >= needed,
        "plane has {} bytes but needs at least {needed}",
        src.len()
    );

    let mut out = Vec::with_capacity(row_bytes * rows);
    for row in src.chunks(stride).take(rows) {
        out.extend_from_slice(&row[..row_bytes]);
    }
    Ok(out)
}

fn bgra_to_rgba(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        data.len() >= width as usize * height as usize * 4,
        "BGRA frame is too short for {width}x{height}"
    );
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data.chunks_exact(4).take((width * height) as usize) {
        out.push(chunk[2]); // R
        out.push(chunk[1]); // G
        out.push(chunk[0]); // B
        out.push(chunk[3]); // A
    }
    Ok(out)
}

fn bgrx_to_rgba(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        data.len() >= width as usize * height as usize * 4,
        "BGRx frame is too short for {width}x{height}"
    );
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data.chunks_exact(4).take((width * height) as usize) {
        out.push(chunk[2]); // R
        out.push(chunk[1]); // G
        out.push(chunk[0]); // B
        out.push(255); // A (opaque)
    }
    Ok(out)
}

fn bgr0_to_rgba(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        data.len() >= width as usize * height as usize * 4,
        "BGR0 frame is too short for {width}x{height}"
    );
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data.chunks_exact(4).take((width * height) as usize) {
        out.push(chunk[2]); // R
        out.push(chunk[1]); // G
        out.push(chunk[0]); // B
        out.push(255); // A (opaque)
    }
    Ok(out)
}

fn rgbx_to_rgba(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        data.len() >= width as usize * height as usize * 4,
        "RGBx frame is too short for {width}x{height}"
    );
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data.chunks_exact(4).take((width * height) as usize) {
        out.push(chunk[0]); // R
        out.push(chunk[1]); // G
        out.push(chunk[2]); // B
        out.push(255); // A (opaque)
    }
    Ok(out)
}

fn rgb_to_rgba(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        data.len() >= width as usize * height as usize * 3,
        "RGB frame is too short for {width}x{height}"
    );
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data.chunks_exact(3).take((width * height) as usize) {
        out.push(chunk[0]); // R
        out.push(chunk[1]); // G
        out.push(chunk[2]); // B
        out.push(255); // A (opaque)
    }
    Ok(out)
}

fn xbgr_to_rgba(width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        data.len() >= width as usize * height as usize * 4,
        "XBGR frame is too short for {width}x{height}"
    );
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for chunk in data.chunks_exact(4).take((width * height) as usize) {
        out.push(chunk[3]); // R
        out.push(chunk[2]); // G
        out.push(chunk[1]); // B
        out.push(255); // A (opaque)
    }
    Ok(out)
}
