use std::sync::atomic::{AtomicU32, Ordering};

use anyhow::{Context, Result};
use pulse_types::fragment::{FragmentAssembler, ReassembledDatagram};
use pulse_types::{WtFragmentedTrackData, WtMessageC2S, WtMessageS2C};
use wtransport::Connection;
use wtransport::stream::{RecvStream, SendStream};

/// Send a control message over the bidirectional stream.
///
/// Wire format: 4-byte big-endian u32 length prefix followed by the rkyv-serialized payload.
pub async fn send_message(send: &mut SendStream, message: WtMessageC2S) -> Result<()> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&message)
        .context("Failed to serialize C2S message")?;
    let len = bytes.len() as u32;
    send.write_all(&len.to_be_bytes()).await?;
    send.write_all(&bytes).await?;
    Ok(())
}

/// Receive a control message from the bidirectional stream.
///
/// Reads into `buffer`, parsing length-prefixed rkyv frames. Returns the deserialized message.
/// `buffer` retains any leftover bytes for the next call.
pub async fn recv_message(recv: &mut RecvStream, buffer: &mut Vec<u8>) -> Result<WtMessageS2C> {
    loop {
        if let Some((msg, consumed)) = try_parse_message(buffer)? {
            buffer.drain(..consumed);
            return Ok(msg);
        }

        let mut tmp = [0u8; 8192];
        let n = recv.read(&mut tmp).await?.context("Stream closed")?;
        buffer.extend_from_slice(&tmp[..n]);
    }
}

fn try_parse_message(buffer: &[u8]) -> Result<Option<(WtMessageS2C, usize)>> {
    if buffer.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    if buffer.len() < 4 + len {
        return Ok(None);
    }
    let message: WtMessageS2C =
        rkyv::api::high::from_bytes::<_, rkyv::rancor::Error>(&buffer[4..4 + len])
            .map_err(|e| anyhow::anyhow!("Failed to deserialize S2C message: {e}"))?;
    Ok(Some((message, 4 + len)))
}

const FRAGMENT_ENVELOPE_OVERHEAD: usize = 128;

const FALLBACK_MAX_DATAGRAM_SIZE: usize = 1200;

pub fn send_datagram(
    connection: &Connection,
    track_id: &str,
    data: &[u8],
    sequence_counter: &AtomicU32,
) -> Result<()> {
    let max_datagram = connection
        .max_datagram_size()
        .unwrap_or(FALLBACK_MAX_DATAGRAM_SIZE);
    let max_payload = max_datagram.saturating_sub(FRAGMENT_ENVELOPE_OVERHEAD);
    if max_payload == 0 {
        anyhow::bail!("Max datagram size ({max_datagram}) is too small for even an empty fragment");
    }
    let fragment_count = data.len().div_ceil(max_payload);
    if fragment_count > u16::MAX as usize {
        anyhow::bail!(
            "Data too large: {fragment_count} fragments required (max {})",
            u16::MAX
        );
    }
    let sequence_id = sequence_counter.fetch_add(1, Ordering::Relaxed);
    for (i, chunk) in data.chunks(max_payload.max(1)).enumerate() {
        let fragment = WtFragmentedTrackData {
            id: track_id.to_string(),
            sequence_id,
            fragment_index: i as u16,
            fragment_count: fragment_count as u16,
            data: chunk.to_vec(),
        };
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&fragment)
            .context("Failed to serialize fragment")?;
        connection
            .send_datagram(bytes)
            .map_err(|e| anyhow::anyhow!("Failed to send datagram fragment {i}: {e}"))?;
    }

    Ok(())
}

pub async fn recv_datagram(
    connection: &Connection,
    assembler: &mut FragmentAssembler,
) -> Result<Option<ReassembledDatagram>> {
    let datagram = connection.receive_datagram().await?;
    let fragment: WtFragmentedTrackData =
        rkyv::api::high::from_bytes::<_, rkyv::rancor::Error>(&datagram)
            .map_err(|e| anyhow::anyhow!("Failed to deserialize fragment: {e}"))?;
    Ok(assembler.insert(fragment))
}
