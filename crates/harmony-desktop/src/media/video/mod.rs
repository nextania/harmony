pub mod hardware;
pub mod software;

use anyhow::Result;
use bytes::Bytes;

use crate::media::codec;

#[derive(Debug, Clone)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Bytes,
}

pub trait VideoDecoder: Send {
    fn codec_id(&self) -> u8;
    fn decode(&mut self, data: &[u8]) -> Result<Vec<Frame>>;
    fn flush(&mut self) -> Vec<Frame>;
}

pub fn create_video_decoder(codec: u8) -> Result<Box<dyn VideoDecoder>> {
    match codec {
        codec::VIDEO_H264 => {
            let decoder = hardware::HardwareVideoDecoder::new();
            if let Ok(decoder) = decoder {
                Ok(Box::new(decoder))
            } else {
                // fall back to software decoder (e.g. unsupported GPU)
                let software_decoder = software::SoftwareVideoDecoder::new();
                Ok(Box::new(software_decoder))
            }
        }
        other => anyhow::bail!("unsupported video codec: 0x{other:02x}"),
    }
}
