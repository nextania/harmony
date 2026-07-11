use anyhow::Context;
use openh264::decoder::Decoder;
use openh264::formats::YUVSource;

use crate::media::{
    codec,
    video::{Frame, VideoDecoder},
};

pub struct SoftwareVideoDecoder {
    decoder: Option<Decoder>,
    frame_count: u64,
}

impl SoftwareVideoDecoder {
    pub fn new() -> Self {
        Self {
            decoder: None,
            frame_count: 0,
        }
    }
}

impl VideoDecoder for SoftwareVideoDecoder {
    fn codec_id(&self) -> u8 {
        codec::VIDEO_H264
    }

    fn decode(&mut self, data: &[u8]) -> anyhow::Result<Vec<Frame>> {
        let decoder = self
            .decoder
            .get_or_insert_with(|| Decoder::new().expect("failed to create OpenH264 decoder"));

        self.frame_count += 1;

        match decoder.decode(data).context("OpenH264 decode error")? {
            Some(yuv) => {
                let (w, h) = yuv.dimensions();
                let (w, h) = (w as u32, h as u32);
                let (y_stride, u_stride, v_stride) = yuv.strides();

                let planar = yuv::YuvPlanarImage {
                    y_plane: yuv.y(),
                    y_stride: y_stride as u32,
                    u_plane: yuv.u(),
                    u_stride: u_stride as u32,
                    v_plane: yuv.v(),
                    v_stride: v_stride as u32,
                    width: w,
                    height: h,
                };

                let mut rgba = vec![0u8; (w * h * 4) as usize];
                yuv::yuv420_to_rgba(
                    &planar,
                    &mut rgba,
                    w * 4,
                    yuv::YuvRange::Limited,
                    yuv::YuvStandardMatrix::Bt709,
                )
                .context("YUV to RGBA conversion failed")?;

                Ok(vec![Frame {
                    width: w,
                    height: h,
                    rgba: rgba.into(),
                }])
            }
            None => Ok(Vec::new()),
        }
    }

    fn flush(&mut self) -> Vec<Frame> {
        Vec::new()
    }
}
