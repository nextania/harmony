// IMPORTANT: every media packet starts with a single byte identifying the codec, followed by the raw encoded data

pub const AUDIO_OPUS: u8 = 0x01;

pub const VIDEO_H264: u8 = 0x01;

pub fn strip_codec_byte(packet: &[u8]) -> Option<(u8, &[u8])> {
    if packet.is_empty() {
        return None;
    }
    Some((packet[0], &packet[1..]))
}

pub fn prepend_codec_byte(codec: u8, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + data.len());
    out.push(codec);
    out.extend_from_slice(data);
    out
}
