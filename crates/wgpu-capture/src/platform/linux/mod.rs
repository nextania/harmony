pub(crate) mod capture;
pub(crate) mod encode;
pub(crate) mod import_vk;

use std::os::fd::OwnedFd;

pub(crate) fn drm_fourcc_to_vk_format(fourcc: drm_fourcc::DrmFourcc) -> Option<ash::vk::Format> {
    match fourcc {
        drm_fourcc::DrmFourcc::Argb8888 => Some(ash::vk::Format::B8G8R8A8_UNORM),
        drm_fourcc::DrmFourcc::Xrgb8888 => Some(ash::vk::Format::B8G8R8A8_UNORM),
        drm_fourcc::DrmFourcc::Abgr8888 => Some(ash::vk::Format::R8G8B8A8_UNORM),
        drm_fourcc::DrmFourcc::Xbgr8888 => Some(ash::vk::Format::R8G8B8A8_UNORM),
        _ => None,
    }
}

pub(crate) struct LinuxFrame {
    pub display_fd: OwnedFd,
    pub encode_fd: OwnedFd,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub fourcc: drm_fourcc::DrmFourcc,
    pub modifier: u64,
}
