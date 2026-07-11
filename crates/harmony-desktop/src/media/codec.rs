// IMPORTANT: audio packets are prefixed with 1 byte identifying the codec,
// while video packets are prefixed with 7 bytes including the codec,
// sequence number, and timestamp

pub const AUDIO_OPUS: u8 = 0x01;

pub const VIDEO_H264: u8 = 0x01;
pub const VIDEO_AV1: u8 = 0x02;

pub const FRAME_HEADER_LEN: usize = 7; // codec(u8) + seq(u16 BE) + timestamp_ms(u32 BE)

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

pub fn prepend_frame_header(codec: u8, seq: u16, timestamp_ms: u32, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(FRAME_HEADER_LEN + data.len());
    out.push(codec);
    out.extend_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(&timestamp_ms.to_be_bytes());
    out.extend_from_slice(data);
    out
}

pub fn strip_frame_header(packet: &[u8]) -> Option<(u8, u16, u32, &[u8])> {
    if packet.len() < FRAME_HEADER_LEN {
        return None;
    }
    let codec = packet[0];
    let seq = u16::from_be_bytes([packet[1], packet[2]]);
    let timestamp_ms = u32::from_be_bytes([packet[3], packet[4], packet[5], packet[6]]);
    Some((codec, seq, timestamp_ms, &packet[FRAME_HEADER_LEN..]))
}

#[derive(Clone, Debug)]
pub struct EncodedPacket {
    pub codec: u8,
    pub data: Vec<u8>,
    pub capture_ts_us: u64,
    pub keyframe: bool,
}

pub fn detect_keyframe(codec: u8, data: &[u8]) -> bool {
    // we need to detect when there is a keyframe, because the
    // decoder doesn't like when we pass it a mid-stream frame
    // when it hasn't been fed the keyframe
    match codec {
        VIDEO_H264 => h264_is_keyframe(data),
        VIDEO_AV1 => av1_is_keyframe(data),
        _ => false,
    }
}

fn h264_is_keyframe(data: &[u8]) -> bool {
    // Annex-B
    if data.len() >= 4
        && data[0] == 0
        && data[1] == 0
        && (data[2] == 1 || (data[2] == 0 && data[3] == 1))
    {
        let mut i = 0;
        while i + 3 < data.len() {
            if data[i] == 0 && data[i + 1] == 0 {
                let (sc_len, ok) = if data[i + 2] == 1 {
                    (3usize, true)
                } else if data[i + 2] == 0 && i + 3 < data.len() && data[i + 3] == 1 {
                    (4usize, true)
                } else {
                    (0, false)
                };
                if ok {
                    let nal = i + sc_len;
                    if nal < data.len() {
                        let nal_type = data[nal] & 0x1F;
                        if nal_type == 5 || nal_type == 7 {
                            return true;
                        }
                    }
                    i = nal;
                    continue;
                }
            }
            i += 1;
        }
        return false;
    }

    // AVCC
    let mut i = 0;
    while i + 4 <= data.len() {
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        i += 4;
        if i >= data.len() {
            break;
        }
        let nal_type = data[i] & 0x1F;
        if nal_type == 5 || nal_type == 7 {
            return true;
        }
        i = i.saturating_add(len);
    }
    false
}

fn av1_is_keyframe(data: &[u8]) -> bool {
    let mut i = 0;
    while i < data.len() {
        let header = data[i];
        let obu_type = (header >> 3) & 0x0F;
        let has_ext = (header >> 2) & 0x01;
        let has_size = (header >> 1) & 0x01;
        i += 1;
        if has_ext == 1 {
            i += 1;
        }
        if obu_type == 1 {
            // OBU_SEQUENCE_HEADER
            return true;
        }
        if has_size == 1 {
            let Some((size, consumed)) = read_leb128(&data[i..]) else {
                break;
            };
            i += consumed + size as usize;
        } else {
            break;
        }
    }
    false
}

fn read_leb128(data: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (idx, &byte) in data.iter().take(8).enumerate() {
        value |= ((byte & 0x7F) as u64) << (idx * 7);
        if byte & 0x80 == 0 {
            return Some((value, idx + 1));
        }
    }
    None
}

pub fn now_micros() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}
