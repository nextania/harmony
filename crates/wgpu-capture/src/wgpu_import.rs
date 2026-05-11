use crate::platform::ImportBackend;
use crate::{CaptureFrame, Result};

/// Imports a captured frame into a `wgpu::Texture` for GPU processing. This is
/// designed so that frames can be sent directly to `iced` for rendering with minimal copies.
pub struct WgpuImporter {
    backend: ImportBackend,
}

impl WgpuImporter {
    /// Detect and choose the `wgpu` backend.
    ///
    /// # Errors
    /// Returns [`crate::Error::UnsupportedBackend`] if the device uses a backend
    /// other than Vulkan or D3D12 (Windows) / Vulkan (Linux).
    pub fn new(device: &wgpu::Device) -> Result<Self> {
        let backend = Self::detect_backend(device)?;
        Ok(WgpuImporter { backend })
    }

    /// Import a [`CaptureFrame`] as a [`wgpu::Texture`] that can be used in shaders.
    pub fn import(
        &mut self,
        frame: &CaptureFrame,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        desc: &wgpu::TextureDescriptor<'_>,
    ) -> Result<wgpu::Texture> {
        self.backend.import(frame, device, queue, desc)
    }

    fn detect_backend(device: &wgpu::Device) -> Result<ImportBackend> {
        #[cfg(windows)]
        {
            use crate::platform::windows;
            // try Vulkan first
            if let Ok(imp) = windows::import_vk::VulkanWin32Importer::new(device) {
                return Ok(ImportBackend::Vulkan(imp));
            }
            if let Ok(imp) = windows::import_dx12::Dx12Importer::new(device) {
                return Ok(ImportBackend::Dx12(imp));
            }
            return Err(crate::Error::UnsupportedBackend);
        }

        #[cfg(target_os = "linux")]
        {
            use crate::platform::linux;
            if let Ok(imp) = linux::import_vk::VulkanLinuxImporter::new(device) {
                return Ok(ImportBackend::VulkanLinux(imp));
            }
            return Err(crate::Error::UnsupportedBackend);
        }

        #[cfg(not(any(windows, target_os = "linux")))]
        Err(crate::Error::UnsupportedBackend)
    }
}
