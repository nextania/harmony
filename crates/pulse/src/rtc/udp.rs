use std::{collections::VecDeque, io::ErrorKind, sync::Arc, time::Instant};

use async_std::net::UdpSocket;
use str0m::{
    media::{KeyframeRequest, MediaData, Mid},
    net::{Protocol, Receive},
    Input,
};

use super::peer::Client;

#[derive(Clone, Debug)]
pub enum Propagated {
    /// When we have nothing to propagate.
    Noop,

    /// Poll client has reached timeout.
    Timeout(Instant),

    /// A new incoming track opened.
    // TrackOpen(String, String),

    /// Data to be propagated from one client to another.
    MediaData(String, Arc<MediaData>),

    /// A keyframe request from one client to the source.
    KeyframeRequest(String, KeyframeRequest, String, Mid),
}

impl Propagated {
    /// Get client id, if the propagated event has a client id.
    pub fn client_id(&self) -> Option<String> {
        match self {
            // Propagated::TrackOpen(c, _)
            Propagated::MediaData(c, _) | Propagated::KeyframeRequest(c, _, _, _) => {
                Some(c.clone())
            }
            _ => None,
        }
    }
}

pub async fn read_socket_input<'a>(socket: &UdpSocket, buf: &'a mut Vec<u8>) -> Option<Input<'a>> {
    buf.resize(2000, 0);

    match socket.recv_from(buf).await {
        Ok((n, source)) => {
            buf.truncate(n);

            // Parse data to a DatagramRecv, which help preparse network data to
            // figure out the multiplexing of all protocols on one UDP port.
            let Ok(contents) = buf.as_slice().try_into() else {
                return None;
            };

            return Some(Input::Receive(
                Instant::now(),
                Receive {
                    proto: Protocol::Udp,
                    source,
                    destination: socket.local_addr().unwrap(),
                    contents,
                },
            ));
        }

        Err(e) => match e.kind() {
            // Expected error for set_read_timeout(). One for windows, one for the rest.
            ErrorKind::WouldBlock | ErrorKind::TimedOut => None,
            _ => panic!("UdpSocket read failed: {e:?}"),
        },
    }
}

/// Poll all the output from the client until it returns a timeout.
/// Collect any output in the queue, transmit data on the socket, return the timeout
pub async fn poll_until_timeout(
    client: &mut Client,
    queue: &mut VecDeque<Propagated>,
    socket: &UdpSocket,
) -> Instant {
    loop {
        if !client.rtc.is_alive() {
            // This client will be cleaned up in the next run of the main loop.
            return Instant::now();
        }

        let propagated = client.poll_output(socket).await;

        if let Propagated::Timeout(t) = propagated {
            return t;
        }

        queue.push_back(propagated)
    }
}
