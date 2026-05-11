use ash::vk;

#[cfg(target_os = "linux")]
pub(crate) mod linux;

#[cfg(windows)]
pub(crate) mod windows;

#[cfg(windows)]
pub(crate) struct PlatformFrame(pub(crate) windows::WindowsFrame);

#[cfg(target_os = "linux")]
pub(crate) struct PlatformFrame(pub(crate) linux::LinuxFrame);

impl PlatformFrame {
    pub(crate) fn width(&self) -> u32 {
        #[cfg(windows)]
        return self.0.width;
        #[cfg(target_os = "linux")]
        return self.0.width;
    }

    pub(crate) fn height(&self) -> u32 {
        #[cfg(windows)]
        return self.0.height;
        #[cfg(target_os = "linux")]
        return self.0.height;
    }
}

pub(crate) fn create_capturer(
    target: crate::CaptureTarget,
) -> crate::Result<Box<dyn crate::Capturer>> {
    #[cfg(windows)]
    return windows::capture::WindowsCapturer::new(target).map(|c| Box::new(c) as _);
    #[cfg(target_os = "linux")]
    return linux::capture::LinuxCapturer::new(target).map(|c| Box::new(c) as _);
    #[cfg(not(any(windows, target_os = "linux")))]
    Err(crate::Error::UnsupportedPlatform)
}

pub(crate) fn create_encoder(
    config: crate::EncodeConfig,
) -> crate::Result<Box<dyn crate::EncodeSession>> {
    #[cfg(windows)]
    return windows::encode::MfEncoder::new(config).map(|e| Box::new(e) as _);
    #[cfg(target_os = "linux")]
    return linux::encode::VaapiEncoder::new(config).map(|e| Box::new(e) as _);
    #[cfg(not(any(windows, target_os = "linux")))]
    Err(crate::Error::UnsupportedPlatform)
}

pub(crate) enum ImportBackend {
    #[cfg(windows)]
    Vulkan(windows::import_vk::VulkanWin32Importer),
    #[cfg(windows)]
    Dx12(windows::import_dx12::Dx12Importer),
    #[cfg(target_os = "linux")]
    VulkanLinux(linux::import_vk::VulkanLinuxImporter),
}

impl ImportBackend {
    pub(crate) fn import(
        &mut self,
        frame: &crate::CaptureFrame,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        desc: &wgpu::TextureDescriptor<'_>,
    ) -> crate::Result<wgpu::Texture> {
        match self {
            #[cfg(windows)]
            Self::Vulkan(imp) => imp.import(frame, device, queue, desc),
            #[cfg(windows)]
            Self::Dx12(imp) => imp.import(frame, device, queue, desc),
            #[cfg(target_os = "linux")]
            Self::VulkanLinux(imp) => imp.import(frame, device, queue, desc),
        }
    }
}

pub(crate) fn find_memory_type(
    props: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    required: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..props.memory_type_count).find(|&i| {
        (type_bits & (1 << i)) != 0
            && props.memory_types[i as usize]
                .property_flags
                .contains(required)
    })
}
