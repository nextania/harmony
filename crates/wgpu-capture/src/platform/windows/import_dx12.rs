use std::collections::HashMap;

use crossbeam_queue::ArrayQueue;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Graphics::Direct3D12::ID3D12Resource;
use windows::core::Interface;

use super::directx::create_nt_handle;
use crate::platform::windows::CAPTURE_POOL_SIZE;
use crate::{CaptureFrame, Result};

pub(crate) struct Dx12Importer {
    cache: HashMap<usize, ID3D12Resource>,
    cache_insertions: ArrayQueue<usize>,
}

impl Dx12Importer {
    pub(crate) fn new(device: &wgpu::Device) -> Result<Self> {
        unsafe { device.as_hal::<wgpu::hal::api::Dx12>() }
            .ok_or(crate::Error::UnsupportedBackend)?;
        Ok(Dx12Importer {
            cache: HashMap::new(),
            cache_insertions: ArrayQueue::new(CAPTURE_POOL_SIZE),
        })
    }

    pub(crate) fn evict(&mut self, last: usize) {
        if self.cache_insertions.len() == CAPTURE_POOL_SIZE {
            let Some(evicted) = self.cache_insertions.pop() else {
                unreachable!();
            };
            if evicted != last {
                self.cache.remove(&evicted);
            }
            self.cache_insertions.push(last).ok();
        }
    }

    pub(crate) fn import(
        &mut self,
        frame: &CaptureFrame,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        wgpu_desc: &wgpu::TextureDescriptor<'_>,
    ) -> Result<wgpu::Texture> {
        let width = frame.0.width();
        let height = frame.0.height();

        let texture_key = frame.0.0.texture.as_raw() as usize;

        unsafe {
            let guard = device
                .as_hal::<wgpu::hal::api::Dx12>()
                .ok_or(crate::Error::UnsupportedBackend)?;
            let raw_d3d12 = guard.raw_device();

            let resource = if let Some(cached) = self.cache.get(&texture_key) {
                cached.clone()
            } else {
                let nt_handle = create_nt_handle(&frame.0.0)?;
                let mut resource_opt: Option<ID3D12Resource> = None;
                raw_d3d12.OpenSharedHandle(nt_handle, &mut resource_opt)?;
                CloseHandle(nt_handle).ok();
                let resource = resource_opt.unwrap();
                self.cache.insert(texture_key, resource.clone());
                self.evict(texture_key);
                resource
            };

            let hal_texture = wgpu::hal::dx12::Device::texture_from_raw(
                resource,
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureDimension::D2,
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                1,
                1,
            );

            let wgpu_texture =
                device.create_texture_from_hal::<wgpu::hal::api::Dx12>(hal_texture, wgpu_desc);
            Ok(wgpu_texture)
        }
    }
}
