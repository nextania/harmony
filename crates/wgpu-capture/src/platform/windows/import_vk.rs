use std::collections::HashMap;

use ash::vk;
use crossbeam_queue::ArrayQueue;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_STAGING, ID3D11Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::Interface;

use crate::platform::find_memory_type;
use crate::platform::windows::CAPTURE_POOL_SIZE;
use crate::{CaptureFrame, Result};

use super::directx::create_nt_handle;

struct CachedVkTexture {
    image: vk::Image,
    memory: vk::DeviceMemory,
}

pub(crate) struct VulkanWin32Importer {
    cache: HashMap<usize, CachedVkTexture>,
    cache_insertions: ArrayQueue<usize>,
    raw_device: ash::Device,
}

impl VulkanWin32Importer {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self> {
        let raw_device = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }
            .ok_or(crate::Error::UnsupportedBackend)?
            .raw_device()
            .clone();
        Ok(VulkanWin32Importer {
            cache: HashMap::new(),
            cache_insertions: ArrayQueue::new(CAPTURE_POOL_SIZE),
            raw_device,
        })
    }

    pub(crate) fn evict(&mut self, last: usize) {
        if self.cache_insertions.len() == CAPTURE_POOL_SIZE {
            let Some(evicted) = self.cache_insertions.pop() else {
                unreachable!();
            };
            if evicted != last {
                if let Some(cached) = self.cache.remove(&evicted) {
                    unsafe {
                        self.raw_device.destroy_image(cached.image, None);
                        self.raw_device.free_memory(cached.memory, None);
                    }
                }
            }
            self.cache_insertions.push(last).ok();
        }
    }

    pub(crate) fn import(
        &mut self,
        frame: &CaptureFrame,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        wgpu_desc: &wgpu::TextureDescriptor<'_>,
    ) -> Result<wgpu::Texture> {
        let texture_key = frame.0.0.texture.as_raw() as usize;
        let luid = unsafe {
            let dxgi: IDXGIDevice = frame.0.0.device.cast()?;
            let adapter = dxgi.GetAdapter()?;
            adapter.GetDesc()?.AdapterLuid
        };
        let guard = unsafe { device.as_hal::<wgpu::hal::api::Vulkan>() }
            .ok_or(crate::Error::UnsupportedBackend)?;

        let raw_device = guard.raw_device();
        let instance = guard.shared_instance().raw_instance();
        let physical_device = guard.raw_physical_device();

        let mut id_props = vk::PhysicalDeviceIDProperties::default();
        let mut props2 = vk::PhysicalDeviceProperties2::default().push_next(&mut id_props);
        unsafe { instance.get_physical_device_properties2(physical_device, &mut props2) };

        if id_props.device_luid
            != unsafe { std::mem::transmute::<windows::Win32::Foundation::LUID, [u8; 8]>(luid) }
        {
            return import_cross_gpu(frame, device, queue, wgpu_desc);
        }

        if !self.cache.contains_key(&texture_key) {
            let nt_handle = create_nt_handle(&frame.0.0)?;
            let mut ext_img_info = vk::ExternalMemoryImageCreateInfo::default()
                .handle_types(vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE);
            let image_info = vk::ImageCreateInfo::default()
                .push_next(&mut ext_img_info)
                .image_type(vk::ImageType::TYPE_2D)
                .format(vk::Format::B8G8R8A8_UNORM)
                .extent(vk::Extent3D {
                    width: frame.0.0.width,
                    height: frame.0.0.height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::OPTIMAL)
                .usage(vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_SRC)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .initial_layout(vk::ImageLayout::UNDEFINED);
            let vk_image = unsafe { raw_device.create_image(&image_info, None) }?;

            let mem_reqs = unsafe { raw_device.get_image_memory_requirements(vk_image) };
            let mem_props =
                unsafe { instance.get_physical_device_memory_properties(physical_device) };
            let ext_mem_win32_fn =
                ash::khr::external_memory_win32::Device::new(instance, raw_device);
            let mut handle_props = vk::MemoryWin32HandlePropertiesKHR::default();
            unsafe {
                ext_mem_win32_fn.get_memory_win32_handle_properties(
                    vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE,
                    nt_handle.0 as isize,
                    &mut handle_props,
                )
            }?;
            let mem_type_idx = find_memory_type(
                &mem_props,
                mem_reqs.memory_type_bits & handle_props.memory_type_bits,
                vk::MemoryPropertyFlags::DEVICE_LOCAL,
            )
            .ok_or(crate::Error::VulkanMemory)?;

            let mut dedicated_info = vk::MemoryDedicatedAllocateInfo::default().image(vk_image);
            let mut import_info = vk::ImportMemoryWin32HandleInfoKHR::default()
                .handle_type(vk::ExternalMemoryHandleTypeFlags::D3D11_TEXTURE)
                .handle(nt_handle.0 as isize);
            let alloc_info = vk::MemoryAllocateInfo::default()
                .push_next(&mut dedicated_info)
                .push_next(&mut import_info)
                .allocation_size(mem_reqs.size)
                .memory_type_index(mem_type_idx);
            let vk_memory = unsafe { raw_device.allocate_memory(&alloc_info, None) }?;
            unsafe { raw_device.bind_image_memory(vk_image, vk_memory, 0) }?;

            unsafe { CloseHandle(nt_handle).ok() };
            self.cache.insert(
                texture_key,
                CachedVkTexture {
                    image: vk_image,
                    memory: vk_memory,
                },
            );
            self.evict(texture_key);
        }

        let cached = self.cache.get(&texture_key).unwrap();
        let vk_image = cached.image;
        let hal_desc = wgpu::hal::TextureDescriptor {
            label: wgpu_desc.label,
            size: wgpu_desc.size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Bgra8Unorm,
            usage: wgpu::TextureUses::RESOURCE | wgpu::TextureUses::COPY_SRC,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: vec![],
        };

        // only drop when the importer is dropped
        let drop_guard: wgpu::hal::DropCallback = Box::new(|| {});
        let hal_texture = unsafe {
            guard.texture_from_raw(
                vk_image,
                &hal_desc,
                Some(drop_guard),
                wgpu::hal::vulkan::TextureMemory::External,
            )
        };

        let wgpu_texture = unsafe {
            device.create_texture_from_hal::<wgpu::hal::api::Vulkan>(hal_texture, wgpu_desc)
        };
        Ok(wgpu_texture)
    }
}

impl Drop for VulkanWin32Importer {
    fn drop(&mut self) {
        for (_, cached) in self.cache.drain() {
            unsafe {
                self.raw_device.destroy_image(cached.image, None);
                self.raw_device.free_memory(cached.memory, None);
            }
        }
    }
}

/// Fallback for cross-GPU capture (e.g. integrated + discrete) where shared handles
/// can't be imported directly by Vulkan.
fn import_cross_gpu(
    frame: &CaptureFrame,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    wgpu_desc: &wgpu::TextureDescriptor<'_>,
) -> Result<wgpu::Texture> {
    let win = &frame.0.0;
    let width = win.width;
    let height = win.height;

    // 1. create a CPU-readable staging texture on the capture device
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut staging_opt: Option<windows::Win32::Graphics::Direct3D11::ID3D11Texture2D> = None;
    unsafe {
        win.device
            .CreateTexture2D(&staging_desc, None, Some(&mut staging_opt))
    }?;
    let staging = staging_opt.unwrap();
    let ctx = unsafe { win.device.GetImmediateContext() }?;

    // 2. copy the shared texture to the staging texture and map it
    let src: ID3D11Resource = win.texture.cast()?;
    let dst: ID3D11Resource = staging.cast()?;
    unsafe { ctx.CopyResource(&dst, &src) };
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe { ctx.Map(&dst, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }?;
    let row_pitch = mapped.RowPitch as usize;
    let tight_row = (width * 4) as usize;

    let pixel_data: Vec<u8> = if row_pitch == tight_row {
        let total = row_pitch * height as usize;
        unsafe { std::slice::from_raw_parts(mapped.pData as *const u8, total).to_vec() }
    } else {
        let mut packed = Vec::with_capacity(tight_row * height as usize);
        for row in 0..height as usize {
            let row_ptr = unsafe { (mapped.pData as *const u8).add(row * row_pitch) };
            packed.extend_from_slice(unsafe { std::slice::from_raw_parts(row_ptr, tight_row) });
        }
        packed
    };
    unsafe { ctx.Unmap(&dst, 0) };

    // 3. create a plain wgpu texture on the Vulkan device and upload
    let mut desc_with_dst = wgpu_desc.clone();
    desc_with_dst.usage |= wgpu::TextureUsages::COPY_DST;
    let texture = device.create_texture(&desc_with_dst);
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixel_data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );

    Ok(texture)
}
