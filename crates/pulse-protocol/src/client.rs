use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use dashmap::DashMap;
use pulse_api::{AvailableTrack, MediaHint, WtMessageC2S, WtMessageS2C};
use tokio::sync::{mpsc, oneshot};

use crate::events::PulseEvent;
use crate::media::MediaRouter;
use crate::mls::MlsClient;
use crate::transport;

/// Configuration for connecting to a Pulse server.
#[derive(Clone, Debug)]
pub struct PulseClientOptions {
    /// WebTransport server URL, e.g. `https://pulse.example.com:4433`.
    pub server_url: String,
    /// Session ID obtained from the Harmony server.
    pub session_id: String,
    /// Session token obtained from the Harmony server.
    pub session_token: String,
    /// Call ID obtained from the Harmony server.
    pub call_id: String,
}

/// Handle to a connected Pulse session.
///
/// Provides async methods for producing/consuming tracks and sending media data.
/// MLS group management (key exchange, commits, epoch transitions) is handled
/// automatically in the background.
pub struct PulseClient {
    command_tx: mpsc::UnboundedSender<ClientCommand>,
    pending_requests: Arc<DashMap<String, oneshot::Sender<()>>>,
    media_router: MediaRouter,
}

enum ClientCommand {
    Send(WtMessageC2S),
    SendMedia { track_id: String, data: Vec<u8> },
    Shutdown,
}

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

impl PulseClient {
    /// Connect to a Pulse server and start the background event loop.
    ///
    /// Returns the client handle and a receiver for control events.
    /// The connection is fully established (including authentication) before returning.
    pub async fn connect(
        options: PulseClientOptions,
    ) -> Result<(Self, mpsc::UnboundedReceiver<PulseEvent>)> {
        let mls = MlsClient::new(&options.session_id, &options.call_id)?;
        let key_package = mls.serialized_key_package()?;

        let client_config = wtransport::ClientConfig::default();
        let endpoint = wtransport::Endpoint::client(client_config)
            .context("Failed to create WebTransport client endpoint")?;

        let connection = endpoint
            .connect(&options.server_url)
            .await
            .context("Failed to connect to Pulse server")?;
        let connection = Arc::new(connection);

        let (mut send_stream, mut recv_stream) = connection
            .open_bi()
            .await
            .context("Failed to open bidirectional stream")?
            .await
            .context("Failed to establish bidirectional stream")?;

        transport::send_message(
            &mut send_stream,
            WtMessageC2S::Connect {
                session_token: options.session_token.clone(),
                key_package,
            },
        )
        .await
        .context("Failed to send Connect message")?;

        let mut recv_buffer = Vec::new();
        let first_msg = transport::recv_message(&mut recv_stream, &mut recv_buffer)
            .await
            .context("Failed to receive initial server message")?;

        let (session_id, available_tracks) = match first_msg {
            WtMessageS2C::Connected {
                id,
                available_tracks,
            } => (id, available_tracks),
            WtMessageS2C::Disconnected { reconnect } => {
                bail!(
                    "Server rejected connection{}",
                    if reconnect.is_some() {
                        " (redirect offered)"
                    } else {
                        ""
                    }
                );
            }
            other => bail!("Unexpected first message from server: {:?}", other),
        };

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let pending_requests: Arc<DashMap<String, oneshot::Sender<()>>> = Arc::new(DashMap::new());
        let media_router = MediaRouter::new();

        let _ = event_tx.send(PulseEvent::Connected {
            id: session_id.clone(),
            available_tracks: available_tracks.clone(),
        });

        let connection = Arc::clone(&connection);
        let pending = Arc::clone(&pending_requests);
        let router = media_router.clone();
        let event_tx = event_tx.clone();

        tokio::spawn(async move {
            if let Err(e) = event_loop(
                connection,
                send_stream,
                recv_stream,
                recv_buffer,
                mls,
                command_rx,
                event_tx,
                pending,
                router,
            )
            .await
            {
                tracing::error!("Pulse client event loop exited with error: {e:#}");
            }
        });

        Ok((
            Self {
                command_tx,
                pending_requests,
                media_router,
            },
            event_rx,
        ))
    }

    /// Start producing a track with the given ID and media hint.
    ///
    /// Waits for the server to confirm with `ProduceStarted` before returning.
    pub async fn produce_track(&self, id: String, media_hint: MediaHint) -> Result<()> {
        self.send_and_wait(
            &id,
            WtMessageC2S::StartProduce {
                id: id.clone(),
                media_hint,
            },
        )
        .await
        .context("Timed out waiting for ProduceStarted")
    }

    /// Stop producing a track with the given ID.
    ///
    /// Waits for the server to confirm with `ProduceStopped` before returning.
    pub async fn stop_producing(&self, id: String) -> Result<()> {
        self.send_and_wait(&id, WtMessageC2S::StopProduce { id: id.clone() })
            .await
            .context("Timed out waiting for ProduceStopped")
    }

    /// Start consuming a remote track with the given ID.
    ///
    /// Registers the media receiver *before* sending the request so no datagrams
    /// are missed. Returns a receiver that yields raw media data for this track.
    ///
    /// Waits for the server to confirm with `ConsumeStarted` before returning.
    pub async fn consume_track(&self, track: &AvailableTrack) -> Result<mpsc::UnboundedReceiver<Vec<u8>>> {
        let rx = self.media_router.subscribe(&track);

        match self
            .send_and_wait(&track.id, WtMessageC2S::StartConsume { id: track.id.clone() })
            .await
        {
            Ok(()) => Ok(rx),
            Err(e) => {
                // Clean up on failure
                self.media_router.unsubscribe(&track.id);
                Err(e).context("Timed out waiting for ConsumeStarted")
            }
        }
    }

    /// Stop consuming a remote track with the given ID.
    ///
    /// Waits for the server to confirm with `ConsumeStopped` before returning.
    /// The per-track media receiver will be unsubscribed after confirmation.
    pub async fn stop_consuming(&self, id: String) -> Result<()> {
        self.send_and_wait(&id, WtMessageC2S::StopConsume { id: id.clone() })
            .await
            .context("Timed out waiting for ConsumeStopped")?;

        self.media_router.unsubscribe(&id);
        Ok(())
    }

    /// Send raw media data for a track as an unreliable datagram.
    pub fn send_media(&self, track_id: &str, data: &[u8]) -> Result<()> {
        self.command_tx
            .send(ClientCommand::SendMedia {
                track_id: track_id.to_string(),
                data: data.to_vec(),
            })
            .map_err(|_| anyhow::anyhow!("Client event loop has shut down"))?;
        Ok(())
    }

    /// Gracefully disconnect from the Pulse server.
    pub fn disconnect(&self) {
        let _ = self
            .command_tx
            .send(ClientCommand::Send(WtMessageC2S::Disconnect {}));
        let _ = self.command_tx.send(ClientCommand::Shutdown);
    }

    /// Internal helper: send a command and wait for the server confirmation
    /// via a oneshot, with a timeout.
    async fn send_and_wait(&self, id: &str, message: WtMessageC2S) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.pending_requests.insert(id.to_string(), tx);

        self.command_tx
            .send(ClientCommand::Send(message))
            .map_err(|_| anyhow::anyhow!("Client event loop has shut down"))?;

        tokio::time::timeout(REQUEST_TIMEOUT, rx)
            .await
            .map_err(|_| anyhow::anyhow!("Request timed out after {REQUEST_TIMEOUT:?}"))?
            .map_err(|_| anyhow::anyhow!("Event loop dropped the response channel"))?;

        Ok(())
    }
}

async fn event_loop(
    connection: Arc<wtransport::Connection>,
    mut send_stream: wtransport::stream::SendStream,
    mut recv_stream: wtransport::stream::RecvStream,
    mut recv_buffer: Vec<u8>,
    mut mls: MlsClient,
    mut command_rx: mpsc::UnboundedReceiver<ClientCommand>,
    event_tx: mpsc::UnboundedSender<PulseEvent>,
    pending_requests: Arc<DashMap<String, oneshot::Sender<()>>>,
    media_router: MediaRouter,
) -> Result<()> {
    let mut heartbeat_interval = tokio::time::interval(HEARTBEAT_INTERVAL);
    heartbeat_interval.tick().await;

    loop {
        tokio::select! {
            msg_result = transport::recv_message(&mut recv_stream, &mut recv_buffer) => {
                let msg = msg_result.context("Failed to receive message from server")?;
                handle_server_message(
                    msg,
                    &mut send_stream,
                    &mut mls,
                    &event_tx,
                    &pending_requests,
                    &media_router,
                ).await?;
            }

            datagram_result = transport::recv_datagram(&connection) => {
                match datagram_result {
                    Ok(track_data) => {
                        if mls.has_group() {
                            media_router.dispatch(&track_data.id, track_data.data, &mls);
                        } else {
                            tracing::warn!("Received media before MLS group initialized; dropping");
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to receive datagram: {e:#}");
                    }
                }
            }

            Some(command) = command_rx.recv() => {
                match command {
                    ClientCommand::Send(msg) => {
                        transport::send_message(&mut send_stream, msg).await?;
                    }
                    ClientCommand::SendMedia { track_id, data } => {
                        if mls.has_group() {
                            match mls.encrypt_media(&data) {
                                Ok(ciphertext) => {
                                    if let Err(e) = transport::send_datagram(&connection, &track_id, &ciphertext) {
                                        tracing::warn!("Failed to send encrypted media: {e:#}");
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to encrypt media for track {}: {e:#}", track_id);
                                }
                            }
                        } else {
                            tracing::warn!("Attempted to send media before MLS group initialized");
                        }
                    }
                    ClientCommand::Shutdown => {
                        tracing::info!("Pulse client shutting down");
                        return Ok(());
                    }
                }
            }

            _ = heartbeat_interval.tick() => {
                transport::send_message(&mut send_stream, WtMessageC2S::Heartbeat {}).await?;
            }
        }
    }
}

async fn handle_server_message(
    msg: WtMessageS2C,
    send_stream: &mut wtransport::stream::SendStream,
    mls: &mut MlsClient,
    event_tx: &mpsc::UnboundedSender<PulseEvent>,
    pending_requests: &Arc<DashMap<String, oneshot::Sender<()>>>,
    media_router: &MediaRouter,
) -> Result<()> {
    match msg {
        WtMessageS2C::ProduceStarted { id } => {
            resolve_pending(pending_requests, &id);
        }
        WtMessageS2C::ProduceStopped { id } => {
            resolve_pending(pending_requests, &id);
        }
        WtMessageS2C::ConsumeStarted { id } => {
            resolve_pending(pending_requests, &id);
        }
        WtMessageS2C::ConsumeStopped { id } => {
            resolve_pending(pending_requests, &id);
            media_router.unsubscribe(&id);
        }

        WtMessageS2C::TrackAvailable { track } => {
            let _ = event_tx.send(PulseEvent::TrackAvailable(track));
        }
        WtMessageS2C::TrackUnavailable { id } => {
            media_router.unsubscribe(&id);
            let _ = event_tx.send(PulseEvent::TrackUnavailable(id));
        }

        WtMessageS2C::Connected {
            id,
            available_tracks,
        } => {
            let _ = event_tx.send(PulseEvent::Connected {
                id,
                available_tracks,
            });
        }
        WtMessageS2C::Disconnected { reconnect } => {
            let _ = event_tx.send(PulseEvent::Disconnected { reconnect });
        }

        WtMessageS2C::Heartbeat {} => {}

        WtMessageS2C::InitializeGroup {
            external_sender_credential,
            external_sender_signature_key,
        } => {
            if let Err(e) =
                mls.initialize_group(&external_sender_credential, &external_sender_signature_key)
            {
                tracing::error!("Failed to initialize MLS group: {e:#}");
            }
        }

        WtMessageS2C::MlsProposals { proposals } => match mls.create_commit(&proposals) {
            Ok((commit_data, epoch, welcome_data)) => {
                transport::send_message(
                    send_stream,
                    WtMessageC2S::MlsCommit {
                        commit_data,
                        epoch,
                        welcome_data,
                    },
                )
                .await?;
            }
            Err(e) => {
                tracing::error!("Failed to create MLS commit: {e:#}");
            }
        },

        WtMessageS2C::MlsCommit {
            epoch,
            commit_data,
            welcome_data,
        } => {
            let result = if !mls.has_group() {
                if let Some(ref welcome) = welcome_data {
                    mls.join_from_welcome(welcome)
                } else {
                    Err(anyhow::anyhow!(
                        "Received MlsCommit without welcome data and no group initialized"
                    ))
                }
            } else {
                mls.apply_commit(&commit_data)
            };

            match result {
                Ok(()) => {
                    transport::send_message(
                        send_stream,
                        WtMessageC2S::CommitAck {
                            epoch: mls.current_epoch(),
                        },
                    )
                    .await?;
                    tracing::debug!(epoch = mls.current_epoch(), "Acknowledged MLS epoch");
                }
                Err(e) => {
                    tracing::error!("Failed to apply MLS commit (epoch {epoch}): {e:#}");
                }
            }
        }

        WtMessageS2C::EpochReady { epoch } => {
            mls.on_epoch_ready(epoch);
            let _ = event_tx.send(PulseEvent::EpochReady(epoch));
        }
    }

    Ok(())
}

fn resolve_pending(pending_requests: &Arc<DashMap<String, oneshot::Sender<()>>>, id: &str) {
    if let Some((_, sender)) = pending_requests.remove(id) {
        let _ = sender.send(());
    }
}
