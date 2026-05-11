pub mod error;
mod platform;
mod wgpu_import;

use std::sync::Arc;

pub use error::{Error, Result};
pub use wgpu_import::WgpuImporter;

/// An opaque handle to a single captured frame residing in GPU memory.
/// - **Windows**: wraps a `ID3D11Texture2D` from the shared capture pool.
/// - **Linux**: wraps two cloned DMA-buf file descriptors (one for display, one
///   for encode).
#[derive(Clone)]
pub struct CaptureFrame(pub(crate) std::sync::Arc<platform::PlatformFrame>);

impl CaptureFrame {
    /// Width of the captured frame in pixels.
    pub fn width(&self) -> u32 {
        self.0.width()
    }

    /// Height of the captured frame in pixels.
    pub fn height(&self) -> u32 {
        self.0.height()
    }

    /// Unique per-capture identifier.
    pub fn frame_id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }
}

/// Selection of what to capture.
#[derive(Debug, Clone)]
pub enum CaptureTarget {
    /// A monitor, identified by its system index.
    Monitor(usize),
    /// A specific window, identified by its raw handle.
    Window(u64),
    /// Provided by the system. On Linux, `xdg-desktop-portal` handles the selection UI.
    System,
}

/// Implemented by platform capturers. Produced via [`create_capturer`].
pub trait Capturer: Send {
    /// Begin capture. Must be called before `next_frame()`.
    fn start(&mut self) -> Result<()>;
    /// Stop capturing and release platform resources.
    fn stop(&mut self);

    /// Poll without blocking for the next captured frame. Returns `None`
    /// if no new frame is available.
    fn next_frame(&mut self) -> Option<CaptureFrame>;
}

/// Creates a capturer for the given target.
pub fn create_capturer(target: CaptureTarget) -> Result<Box<dyn Capturer>> {
    platform::create_capturer(target)
}

/// Codec selection for the encode pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codec {
    H264,
    AV1,
}

impl std::fmt::Display for Codec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Codec::H264 => f.write_str("H.264"),
            Codec::AV1 => f.write_str("AV1"),
        }
    }
}

/// Callback that receives encoded frame data for network transport.
///
/// The callback is invoked on the encoder's background thread with raw encoded
/// bitstream data: H.264 NAL units (length-prefixed) or AV1 OBUs.
pub struct EncodeOutput(pub(crate) std::sync::Arc<dyn Fn(Vec<u8>) + Send + Sync + 'static>);

impl EncodeOutput {
    /// Create an output that delivers encoded bytes to `f`.
    pub fn new<F: Fn(Vec<u8>) + Send + Sync + 'static>(f: F) -> Self {
        EncodeOutput(std::sync::Arc::new(f))
    }
}

impl std::fmt::Debug for EncodeOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EncodeOutput(...)")
    }
}

/// Configuration for the hardware video encoder.
#[derive(Debug)]
pub struct EncodeConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub bitrate_bps: u32,
    pub codec: Codec,
    pub output: EncodeOutput,
}

/// A live hardware encoding session.
pub trait EncodeSession: Send {
    /// Submit a frame to the encoder without blocking. The callback will be invoked
    /// asynchronously when output is available.
    fn submit_frame(&mut self, frame: &CaptureFrame) -> Result<()>;

    /// Dynamically change the target average bitrate (bits per second).
    ///
    /// Takes effect on the next submitted frame. Useful for adapting to
    /// changing network conditions during a video conferencing session.
    fn set_bitrate(&mut self, bitrate_bps: u32) -> Result<()>;

    /// Request that the next submitted frame be encoded as a keyframe (IDR).
    ///
    /// This is the encoder-side response to a PLI (Picture Loss Indication)
    /// from a receiver that has lost sync. To prevent keyframe storms,
    /// only the first within a 1-second window triggers a new keyframe.
    fn request_keyframe(&mut self) -> Result<()>;

    /// Flush and finalise output.
    fn finish(self: Box<Self>) -> Result<()>;
}

/// Creates a hardware encoder session.
pub fn create_encoder(config: EncodeConfig) -> Result<Box<dyn EncodeSession>> {
    platform::create_encoder(config)
}
