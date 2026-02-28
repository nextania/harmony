use crate::media::{
    codec,
    video::{Frame, VideoDecoder},
};

pub struct SoftwareVideoDecoder;

impl SoftwareVideoDecoder {
    pub fn new() -> Self {
        SoftwareVideoDecoder
    }
}

impl VideoDecoder for SoftwareVideoDecoder {
    fn codec_id(&self) -> u8 {
        codec::VIDEO_H264
    }

    fn decode(&mut self, _data: &[u8]) -> anyhow::Result<Vec<Frame>> {
        anyhow::bail!("software video decoding is not implemented yet");
    }

    fn flush(&mut self) -> Vec<Frame> {
        Vec::new()
    }
}
