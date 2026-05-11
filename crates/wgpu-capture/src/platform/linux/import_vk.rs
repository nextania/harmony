use std::os::fd::{AsRawFd, IntoRawFd};

use ash::vk;

use crate::platform::find_memory_type;
use crate::platform::linux::drm_fourcc_to_vk_format;
use crate::{CaptureFrame, Result};

pub(crate) struct VulkanLinuxImporter;

impl VulkanLinuxImporter {
    pub(crate) fn new(_device: &wgpu::Device) -> Result<Self> {
        Ok(VulkanLinuxImporter)
    }

    pub(crate) fn import(
        &self,
        frame: &CaptureFrame,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        wgpu_desc: &wgpu::TextureDescriptor<'_>,
    ) -> Result<wgpu::Texture> {
        let linux_frame = &frame.0.0;
        let fd = &linux_frame.display_fd;
        let width = linux_frame.width;
        let height = linux_frame.height;
        let modifier = linux_frame.modifier;

        if modifier != 0 {
            return Err(crate::Error::Import(format!(
                "unsupported DMA-buf modifier {:#x} for Vulkan display import (only LINEAR=0 supported)",
                modifier
            )));
        }

        let vk_format = drm_fourcc_to_vk_format(linux_frame.fourcc).ok_or_else(|| {
            crate::Error::Import(format!(
                "unsupported fourcc {:?} for Vulkan display import",
                linux_frame.fourcc
            ))
        })?;

        let guard = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }
            .ok_or(crate::Error::UnsupportedBackend)?;

        let raw_device = guard.raw_device();
        let instance = guard.shared_instance().raw_instance();
        let physical_device = guard.raw_physical_device();

        let mut ext_img_info = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);

        let image_info = vk::ImageCreateInfo::default()
            .push_next(&mut ext_img_info)
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk_format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::LINEAR)
            .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);

        let vk_image = unsafe { raw_device.create_image(&image_info, None) }?;
        let mem_reqs = unsafe { raw_device.get_image_memory_requirements(vk_image) };
        let ext_mem_fd_fn = ash::khr::external_memory_fd::Device::new(instance, raw_device);
        let mut fd_props = vk::MemoryFdPropertiesKHR::default();
        unsafe {
            ext_mem_fd_fn.get_memory_fd_properties(
                vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT,
                fd.as_raw_fd(),
                &mut fd_props,
            )?;
        }
        let mem_props = unsafe { instance.get_physical_device_memory_properties(physical_device) };
        let mem_type_idx = find_memory_type(
            &mem_props,
            mem_reqs.memory_type_bits & fd_props.memory_type_bits,
            vk::MemoryPropertyFlags::empty(),
        )
        .ok_or_else(|| crate::Error::VulkanMemory)?;

        let vulkan_owned = nix::unistd::dup(fd)?;
        let mut import_fd_info = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(vulkan_owned.into_raw_fd());

        let dma_buf_size = match nix::sys::stat::fstat(fd) {
            Ok(s) if s.st_size > 0 => s.st_size as u64,
            _ => mem_reqs.size,
        };

        let alloc_info = vk::MemoryAllocateInfo::default()
            .push_next(&mut import_fd_info)
            .allocation_size(dma_buf_size)
            .memory_type_index(mem_type_idx);

        let vk_memory = unsafe { raw_device.allocate_memory(&alloc_info, None)? };
        unsafe { raw_device.bind_image_memory(vk_image, vk_memory, 0)? };
        let hal_desc = wgpu::hal::TextureDescriptor {
            label: wgpu_desc.label,
            size: wgpu_desc.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu_desc.format,
            usage: wgpu::TextureUses::RESOURCE | wgpu::TextureUses::COPY_SRC,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: vec![],
        };

        let hal_texture = unsafe {
            guard.texture_from_raw(
                vk_image,
                &hal_desc,
                None,
                wgpu::hal::vulkan::TextureMemory::Dedicated(vk_memory),
            )
        };

        drop(guard);

        let wgpu_texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(hal_texture, wgpu_desc)
        };

        Ok(wgpu_texture)
    }
}
