use std::sync::Arc;

use anyhow::{Context, Result};
use gpu_video::{
    BytesDecoder, EncodedInputChunk, VulkanDevice, VulkanInstance,
    parameters::{DecoderParameters, VulkanAdapterDescriptor, VulkanDeviceDescriptor},
};

use crate::media::{
    codec,
    video::{Frame, VideoDecoder},
};

pub struct HardwareVideoDecoder {
    device: Arc<VulkanDevice>,
    decoder: Option<BytesDecoder>,
    awaiting_keyframe: bool,
}

// TODO:
fn contains_keyframe_nal(data: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            let nal_type = data[i + 3] & 0x1f;
            if nal_type == 5 || nal_type == 7 {
                return true;
            }
            i += 4;
        } else {
            i += 1;
        }
    }
    false
}

impl HardwareVideoDecoder {
    pub fn new() -> Result<Self> {
        let instance =
            VulkanInstance::new().context("failed to create Vulkan instance for video decode")?;
        let adapter = instance
            .create_adapter(&VulkanAdapterDescriptor::default())
            .context("failed to create Vulkan adapter for video decode")?;
        let device = adapter
            .create_device(&VulkanDeviceDescriptor::default())
            .context("failed to create Vulkan device for video decode")?;

        Ok(Self {
            device,
            decoder: None,
            awaiting_keyframe: true,
        })
    }
}

impl VideoDecoder for HardwareVideoDecoder {
    fn codec_id(&self) -> u8 {
        codec::VIDEO_H264
    }

    fn decode(&mut self, data: &[u8]) -> Result<Vec<Frame>> {
        if self.awaiting_keyframe {
            if !contains_keyframe_nal(data) {
                // don't poison the decoder
                return Ok(Vec::new());
            }
            self.decoder = None;
            self.awaiting_keyframe = false;
        }

        let is_new = self.decoder.is_none();
        let decoder = self.decoder.get_or_insert_with(|| {
            tracing::info!("creating new Vulkan bytes decoder for H.264");
            self.device
                .create_bytes_decoder_h264(DecoderParameters::default())
                .expect("failed to create Vulkan bytes decoder")
        });

        let preview: Vec<u8> = data.iter().take(16).copied().collect();
        tracing::debug!(
            "decode {} bytes (new_decoder={is_new}), first bytes: {preview:02x?}",
            data.len()
        );

        let chunk = EncodedInputChunk { data, pts: None };
        let raw_frames = match decoder.decode(chunk) {
            Ok(frames) => frames,
            Err(e) => {
                self.decoder = None;
                // IMPORTANT: the decoder breaks permanently whenever
                // it errors (likely because it got a P frame before a keyframe)
                // and needs to be recreated with a fresh keyframe.
                self.awaiting_keyframe = true;
                return Err(anyhow::anyhow!("H.264 decode error: {e:?}"));
            }
        };

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
        if let Some(decoder) = self.decoder.as_mut() {
            match decoder.flush() {
                Ok(raw_frames) => raw_frames
                    .into_iter()
                    .map(|f| {
                        let w = f.data.width;
                        let h = f.data.height;
                        let rgba = nv12_to_rgba(&f.data.frame, w, h);
                        Frame {
                            width: w,
                            height: h,
                            rgba: rgba.into(),
                        }
                    })
                    .collect(),
                Err(e) => {
                    tracing::warn!("H.264 decoder flush: {e:#}");
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        }
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
