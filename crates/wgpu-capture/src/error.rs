use crate::Codec;

/// Errors that can occur during capture, encoding, or import.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("thread exited/panicked")]
    Thread,

    #[error("unsupported wgpu backend - must be Vulkan or D3D12 (Windows) / Vulkan (Linux)")]
    UnsupportedBackend,

    #[error("unsupported codec: {0}")]
    UnsupportedCodec(Codec),

    #[error("unsupported capture target")]
    UnsupportedCaptureTarget,

    #[error("Vulkan error: {0}")]
    Vulkan(#[from] ash::vk::Result),

    #[error("unable to find Vulkan memory type")]
    VulkanMemory,

    #[cfg(windows)]
    #[error("Windows error: {0}")]
    Windows(#[from] windows::core::Error),

    #[cfg(windows)]
    #[error("COM error")]
    Com,

    #[cfg(target_os = "linux")]
    #[error("libva error: {0}")]
    Vaapi(#[from] libva::VaError),

    #[cfg(target_os = "linux")]
    #[error("missing VAAPI device")]
    VaapiDeviceMissing,

    #[cfg(target_os = "linux")]
    #[error("no suitable streams found")]
    NoSuitableStreams,

    #[cfg(target_os = "linux")]
    #[error("encode error: {0}")]
    Encode(#[from] cros_codecs::encoder::EncodeError),

    #[cfg(target_os = "linux")]
    #[error("import error: {0}")]
    Import(String),

    #[cfg(target_os = "linux")]
    #[error("ashpd error: {0}")]
    Ashpd(#[from] ashpd::Error),

    #[cfg(target_os = "linux")]
    #[error("nix error: {0}")]
    Nix(#[from] nix::errno::Errno),

    #[cfg(not(any(windows, target_os = "linux")))]
    #[error("unsupported platform")]
    UnsupportedPlatform,
}

pub type Result<T> = std::result::Result<T, Error>;
