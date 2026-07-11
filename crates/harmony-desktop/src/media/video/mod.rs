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
            if let Ok(decoder) = hardware::HardwareVideoDecoder::new() {
                return Ok(Box::new(decoder));
            }
            Ok(Box::new(software::SoftwareVideoDecoder::new()))
        }
        codec::VIDEO_AV1 => {
            // TODO: implement AV1 decoder
            anyhow::bail!("AV1 video decoding is not yet supported")
        }
        other => anyhow::bail!("unsupported video codec: 0x{other:02x}"),
    }
}
