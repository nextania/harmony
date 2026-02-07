use anyhow::{Context, Result};
use pulse_api::{WtMessageC2S, WtMessageS2C, WtTrackData};
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

/// Send media track data as an unreliable datagram.
pub fn send_datagram(connection: &Connection, track_id: &str, data: &[u8]) -> Result<()> {
    let track_data = WtTrackData {
        id: track_id.to_string(),
        data: data.to_vec(),
    };
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&track_data)
        .context("Failed to serialize track data")?;
    connection
        .send_datagram(bytes)
        .map_err(|e| anyhow::anyhow!("Failed to send datagram: {e}"))?;
    Ok(())
}

/// Receive media track data from an incoming datagram.
pub async fn recv_datagram(connection: &Connection) -> Result<WtTrackData> {
    let datagram = connection.receive_datagram().await?;
    let track_data: WtTrackData = rkyv::api::high::from_bytes::<_, rkyv::rancor::Error>(&datagram)
        .map_err(|e| anyhow::anyhow!("Failed to deserialize track data: {e}"))?;
    Ok(track_data)
}
