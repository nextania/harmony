use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use arc_swap::ArcSwap;
use tokio::sync::mpsc;
use wgpu_capture::{
    CaptureFrame, CaptureTarget, Codec, EncodeConfig, EncodeOutput, EncodeSession, create_capturer,
    create_encoder,
};

use crate::media::{codec, video::Frame as DecodedFrame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenQuality {
    P720,
    P1080,
    P1440,
}

impl ScreenQuality {
    fn target_height(self) -> u32 {
        match self {
            ScreenQuality::P720 => 720,
            ScreenQuality::P1080 => 1080,
            ScreenQuality::P1440 => 1440,
        }
    }
}

fn compute_encode_resolution(src_w: u32, src_h: u32, quality: ScreenQuality) -> (u32, u32) {
    let target_h = quality.target_height();
    if src_h <= target_h {
        return (src_w, src_h);
    }
    let scale = target_h as f64 / src_h as f64;
    let w = ((src_w as f64 * scale).round() as u32).max(2);
    let h = target_h;
    // round width to even
    (w & !1, h)
}

#[derive(Debug, Clone)]
pub struct ScreenCaptureConfig {
    pub fps: u32,
    pub bitrate_kbps: u32,
    pub quality: ScreenQuality,
    pub source_width: u32,
    pub source_height: u32,
}

impl Default for ScreenCaptureConfig {
    fn default() -> Self {
        Self {
            fps: 30,
            bitrate_kbps: 2500,
            quality: ScreenQuality::P1080,
            source_width: 1920,
            source_height: 1080,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CaptureTargetInfo {
    pub title: String,
    pub target: CaptureTarget,
    pub thumbnail: Option<DecodedFrame>,
    pub source_width: u32,
    pub source_height: u32,
}

#[derive(Debug, Clone)]
pub enum CaptureTargetList {
    Portal(CaptureTargetInfo),
    Targets(Vec<CaptureTargetInfo>),
}

pub struct ScreenCaptureSession {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl ScreenCaptureSession {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            handle.join().ok();
        }
    }
}

fn probe_encoder_codec() -> (Codec, u8) {
    let probe_config = EncodeConfig {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 2_500_000,
        codec: Codec::H264,
        output: EncodeOutput::new(|_| {}),
    };
    match create_encoder(probe_config) {
        Ok(encoder) => {
            encoder.finish().ok();
            return (Codec::H264, codec::VIDEO_H264);
        }
        Err(e) => {
            tracing::warn!("H.264 encoder probe failed: {e}");
        }
    }

    // TODO: prefer av1
    let probe_config = EncodeConfig {
        width: 1920,
        height: 1080,
        fps: 30,
        bitrate_bps: 2_500_000,
        codec: Codec::AV1,
        output: EncodeOutput::new(|_| {}),
    };
    match create_encoder(probe_config) {
        Ok(encoder) => {
            encoder.finish().ok();
            (Codec::AV1, codec::VIDEO_AV1)
        }
        Err(e) => {
            tracing::warn!("AV1 encoder probe failed: {e}");
            (Codec::H264, codec::VIDEO_H264)
        }
    }
}

pub async fn list_targets_with_thumbnails() -> CaptureTargetList {
    #[cfg(target_os = "linux")]
    {
        CaptureTargetList::Portal(CaptureTargetInfo {
            title: "Screen".to_string(),
            target: CaptureTarget::System,
            thumbnail: None,
            source_width: 0,
            source_height: 0,
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let raw_targets =
            match tokio::task::spawn_blocking(|| wgpu_capture::enumerate_targets()).await {
                Ok(Ok(targets)) => targets,
                Ok(Err(e)) => {
                    tracing::error!("enumerate_targets failed: {e}");
                    return CaptureTargetList::Targets(Vec::new());
                }
                Err(e) => {
                    tracing::error!("spawn_blocking for enumerate_targets panicked: {e}");
                    return CaptureTargetList::Targets(Vec::new());
                }
            };

        let infos: Vec<CaptureTargetInfo> = match tokio::task::spawn_blocking(move || {
            raw_targets
                .into_iter()
                .map(|t| {
                    let thumbnail = wgpu_capture::capture_screenshot(&t.target)
                        .ok()
                        .flatten()
                        .map(|(w, h, rgba)| DecodedFrame {
                            width: w,
                            height: h,
                            rgba: bytes::Bytes::from(rgba),
                        });
                    CaptureTargetInfo {
                        title: t.title,
                        target: t.target,
                        thumbnail,
                        source_width: t.width,
                        source_height: t.height,
                    }
                })
                .collect()
        })
        .await
        {
            Ok(infos) => infos,
            Err(e) => {
                tracing::error!("spawn_blocking for screenshots panicked: {e}");
                Vec::new()
            }
        };

        CaptureTargetList::Targets(infos)
    }
}

pub fn start_screen_capture(
    target: CaptureTarget,
    config: ScreenCaptureConfig,
) -> Result<(
    ScreenCaptureSession,
    mpsc::Receiver<codec::EncodedPacket>,
    Arc<ArcSwap<Option<CaptureFrame>>>,
    mpsc::UnboundedReceiver<()>,
    Arc<AtomicBool>,
)> {
    let (tx, rx) = mpsc::channel(60);
    let (tick_tx, tick_rx) = mpsc::unbounded_channel();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    let latest_frame = Arc::new(ArcSwap::from_pointee(None::<CaptureFrame>));
    let latest_frame_for_ret = Arc::clone(&latest_frame);
    let keyframe_requested = Arc::new(AtomicBool::new(false));
    let keyframe_requested_ret = Arc::clone(&keyframe_requested);

    let handle = thread::Builder::new()
        .name("screen-capture".into())
        .spawn(move || {
            let mut capturer = match create_capturer(target) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("screen capture init failed: {e}");
                    return;
                }
            };
            if let Err(e) = capturer.start() {
                tracing::error!("screen capture start failed: {e}");
                return;
            }

            let (src_w, src_h) = if config.source_width > 0 && config.source_height > 0 {
                (config.source_width, config.source_height)
            } else {
                let first_frame = loop {
                    if stop_thread.load(Ordering::Relaxed) {
                        capturer.stop();
                        return;
                    }
                    match capturer.next_frame() {
                        Some(f) => break f,
                        None => thread::sleep(Duration::from_millis(1)),
                    }
                };
                (first_frame.width(), first_frame.height())
            };

            let (enc_w, enc_h) = compute_encode_resolution(src_w, src_h, config.quality);

            let (actual_codec, codec_byte) = probe_encoder_codec();

            let start_time = std::time::Instant::now();
            let tx_for_callback = tx.clone();
            let encoder_config = EncodeConfig {
                width: enc_w,
                height: enc_h,
                fps: config.fps.max(1),
                bitrate_bps: config.bitrate_kbps.max(250) * 1000,
                codec: actual_codec,
                output: EncodeOutput::new(move |encoded_data: Vec<u8>| {
                    let packet = codec::EncodedPacket {
                        codec: codec_byte,
                        keyframe: codec::detect_keyframe(codec_byte, &encoded_data),
                        capture_ts_us: start_time.elapsed().as_micros() as u64,
                        data: encoded_data,
                    };
                    match tx_for_callback.try_send(packet) {
                        Ok(()) => {}
                        Err(mpsc::error::TrySendError::Full(_)) => {
                            tracing::debug!("screen capture: frame dropped, consumer lagging");
                        }
                        Err(mpsc::error::TrySendError::Closed(_)) => {}
                    }
                }),
            };

            let mut encoder: Box<dyn EncodeSession> = match create_encoder(encoder_config) {
                Ok(e) => e,
                Err(e) => {
                    tracing::error!("screen encoder init failed: {e}");
                    capturer.stop();
                    return;
                }
            };

            while !stop_thread.load(Ordering::Relaxed) {
                if keyframe_requested.swap(false, Ordering::Relaxed)
                    && let Err(e) = encoder.request_keyframe()
                {
                    tracing::warn!("screen encoder request_keyframe: {e}");
                }
                match capturer.next_frame() {
                    Some(frame) => {
                        latest_frame.store(Arc::new(Some(frame.clone())));
                        tick_tx.send(()).ok();
                        if let Err(e) = encoder.submit_frame(&frame) {
                            tracing::warn!("screen encoder submit failed: {e}");
                            break;
                        }
                    }
                    None => thread::sleep(Duration::from_millis(1)),
                }
            }

            capturer.stop();
            if let Err(e) = encoder.finish() {
                tracing::warn!("screen encoder finish: {e}");
            }
        })?;

    Ok((
        ScreenCaptureSession {
            stop,
            handle: Some(handle),
        },
        rx,
        latest_frame_for_ret,
        tick_rx,
        keyframe_requested_ret,
    ))
}
