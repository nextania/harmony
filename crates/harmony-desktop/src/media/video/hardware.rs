use std::sync::Arc;

use anyhow::{Context, Result};
use vk_video::{EncodedInputChunk, VulkanDevice, VulkanInstance};

use crate::media::{
    codec,
    video::{Frame, VideoDecoder},
};

pub struct HardwareVideoDecoder {
    device: Arc<VulkanDevice>,
}

impl HardwareVideoDecoder {
    pub fn new() -> Result<Self> {
        let instance =
            VulkanInstance::new().context("failed to create Vulkan instance for video decode")?;
        let adapter = instance
            .create_adapter(None)
            .context("failed to create Vulkan adapter for video decode")?;
        let device = adapter
            .create_device(
                wgpu::Features::empty(),
                wgpu::ExperimentalFeatures::disabled(),
                wgpu::Limits::default(),
            )
            .context("failed to create Vulkan device for video decode")?;

        Ok(Self { device })
    }
}

impl VideoDecoder for HardwareVideoDecoder {
    fn codec_id(&self) -> u8 {
        codec::VIDEO_H264
    }

    fn decode(&mut self, data: &[u8]) -> Result<Vec<Frame>> {
        let mut decoder = self
            .device
            .create_bytes_decoder(vk_video::parameters::DecoderParameters::default())
            .context("failed to create bytes decoder")?;

        let chunk = EncodedInputChunk { data, pts: None };
        let raw_frames = decoder
            .decode(chunk)
            .map_err(|e| anyhow::anyhow!("H.264 decode error: {e:?}"))?;

        let mut frames = Vec::with_capacity(raw_frames.len());
        for frame in raw_frames {
            let nv12 = &frame.data.frame;
            let w = frame.data.width;
            let h = frame.data.height;

            let rgba = nv12_to_rgba(nv12, w, h);
            frames.push(Frame {
                width: w,
                height: h,
                rgba: rgba.into(),
            });
        }

        Ok(frames)
    }

    fn flush(&mut self) -> Vec<Frame> {
        Vec::new()
    }
}

pub fn nv12_to_rgba(nv12: &[u8], width: u32, height: u32) -> Vec<u8> {
    let y_size = (width * height) as usize;
    let y_plane = &nv12[..y_size];
    let uv_plane = &nv12[y_size..];

    let image = yuv::YuvBiPlanarImage {
        y_plane,
        y_stride: width,
        uv_plane,
        uv_stride: width,
        width,
        height,
    };

    let mut rgba = vec![0u8; (width * height * 4) as usize];
    yuv::yuv_nv12_to_rgba(
        &image,
        &mut rgba,
        width * 4,
        yuv::YuvRange::Limited,
        yuv::YuvStandardMatrix::Bt709,
        yuv::YuvConversionMode::Balanced,
    )
    .expect("NV12 to RGBA conversion failed");

    rgba
}
