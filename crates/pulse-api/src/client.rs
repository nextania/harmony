use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use dashmap::DashMap;
use moq_native::moq_net::{
    self, BroadcastProducer, GroupProducer, Origin, OriginProducer, Track, TrackProducer,
};
use pulse_types::{
    AvailableTrack, MediaHint, WtMessageC2S, WtMessageS2C, decode_media_header,
    encode_media_header, priority_for_hint, track_name_for_hint, track_names,
};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::events::PulseEvent;
use crate::mls::MlsClient;

/// Configuration for connecting to a Pulse server.
#[derive(Clone, Debug)]
pub struct PulseClientOptions {
    /// MoQ/WebTransport server URL, e.g. `https://pulse.example.com:4433`.
    pub server_url: String,
    /// Session ID obtained from the Harmony server.
    pub session_id: String,
    /// Session token obtained from the Harmony server.
    pub session_token: String,
    /// Call ID obtained from the Harmony server.
    pub call_id: String,
}

#[derive(Clone, Debug)]
pub struct MediaFrame {
    pub capture_ts_us: u64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

const MAX_GROUP_BACKLOG: u64 = 3;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Handle to a connected Pulse session.
#[derive(Clone)]
pub struct PulseClient {
    command_tx: mpsc::UnboundedSender<ClientCommand>,
    pending_requests: Arc<DashMap<String, oneshot::Sender<()>>>,
    call_id: String,
    session_id: String,
}

enum ClientCommand {
    SendCtl(WtMessageC2S),
    CreateProducer {
        media_hint: MediaHint,
        reply: oneshot::Sender<Result<()>>,
    },
    WriteMedia {
        media_hint: MediaHint,
        capture_ts_us: u64,
        keyframe: bool,
        data: Vec<u8>,
    },
    StopProducer {
        media_hint: MediaHint,
    },
    // TODO:
    StartConsume {
        track: AvailableTrack,
        sink: mpsc::UnboundedSender<MediaFrame>,
    },
    Shutdown,
}

impl PulseClient {
    /// Connect to a Pulse server and start the background event loop.
    pub async fn connect(
        options: PulseClientOptions,
    ) -> Result<(Self, mpsc::UnboundedReceiver<PulseEvent>)> {
        let mls = MlsClient::new(&options.session_id, &options.call_id)?;
        let key_package = mls.serialized_key_package()?;

        let origin = Origin::random().produce();

        let mut client_config = moq_native::ClientConfig::default();
        client_config.tls.disable_verify = Some(true); // TODO: proper cert validation
        let client = client_config.init().context("Failed to init MoQ client")?;

        let mut url: url::Url = options
            .server_url
            .parse()
            .context("Invalid Pulse server URL")?;
        url.set_path("/call");
        url.query_pairs_mut()
            .clear()
            .append_pair("token", &options.session_token);

        let session = client
            .with_publish(origin.consume())
            .with_consume(origin.clone())
            .connect(url)
            .await
            .context("Failed to connect to Pulse server")?;

        let c2s_path = format!(
            "calls/{}/{}/{}",
            options.call_id,
            options.session_id,
            track_names::CTL_C2S
        );
        let mut ctl_broadcast = origin
            .create_broadcast(&c2s_path)
            .context("Failed to create control broadcast")?;
        let mut ctl_track = ctl_broadcast
            .create_track(Track::new(track_names::CTL_C2S))
            .context("Failed to create control track")?;
        let mut ctl_group = ctl_track
            .append_group()
            .context("Failed to open control group")?;
        write_ctl_frame(&mut ctl_group, &WtMessageC2S::Join { key_package })
            .context("Failed to send Join")?;

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let pending_requests: Arc<DashMap<String, oneshot::Sender<()>>> = Arc::new(DashMap::new());
        let mls = Arc::new(Mutex::new(mls));

        // NOTE: dropping ANY cloned instance of moq_net::Session closes the connection
        let ctx = EventCtx {
            origin,
            session,
            mls,
            call_id: options.call_id.clone(),
            session_id: options.session_id.clone(),
            event_tx: event_tx.clone(),
            pending_requests: pending_requests.clone(),
            ctl_group,
            _ctl_broadcast: ctl_broadcast,
            _ctl_track: ctl_track,
            producers: DashMap::new(),
        };

        tokio::spawn(async move {
            if let Err(e) = event_loop(ctx, command_rx).await {
                tracing::error!("Pulse client event loop exited: {e:#}");
            }
        });

        Ok((
            Self {
                command_tx,
                pending_requests,
                call_id: options.call_id,
                session_id: options.session_id,
            },
            event_rx,
        ))
    }

    /// Start producing a track. Creates the MoQ media track and announces it.
    pub async fn produce_track(&self, _id: String, media_hint: MediaHint) -> Result<()> {
        let (reply, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::CreateProducer {
                media_hint: media_hint.clone(),
                reply,
            })
            .map_err(|_| anyhow::anyhow!("event loop shut down"))?;
        tokio::time::timeout(REQUEST_TIMEOUT, reply_rx)
            .await
            .map_err(|_| anyhow::anyhow!("timed out creating producer after {REQUEST_TIMEOUT:?}"))?
            .map_err(|_| anyhow::anyhow!("event loop dropped reply"))??;

        let track_id = track_name_for_hint(&media_hint).to_string();
        self.send_and_wait(
            &track_id,
            WtMessageC2S::StartProduce {
                id: track_id.clone(),
                media_hint,
            },
        )
        .await
        .context("Timed out waiting for ProduceStarted")
    }

    /// Stop producing a track.
    pub async fn stop_producing(&self, id: String) -> Result<()> {
        let media_hint = hint_from_track_name(&id);
        if let Some(media_hint) = media_hint {
            let _ = self
                .command_tx
                .send(ClientCommand::StopProducer { media_hint });
        }
        self.send_and_wait(&id, WtMessageC2S::StopProduce { id: id.clone() })
            .await
            .context("Timed out waiting for ProduceStopped")
    }

    /// Subscribe to a remote track. Returns a stream of decrypted media frames.
    pub async fn consume_track(
        &self,
        track: &AvailableTrack,
    ) -> Result<mpsc::UnboundedReceiver<MediaFrame>> {
        let (sink, rx) = mpsc::unbounded_channel();
        self.command_tx
            .send(ClientCommand::StartConsume {
                track: track.clone(),
                sink,
            })
            .map_err(|_| anyhow::anyhow!("event loop shut down"))?;

        self.send_and_wait(
            &track.id,
            WtMessageC2S::StartConsume {
                id: track.id.clone(),
            },
        )
        .await
        .context("Timed out waiting for ConsumeStarted")?;
        Ok(rx)
    }

    /// Stop consuming a remote track.
    pub async fn stop_consuming(&self, id: String) -> Result<()> {
        self.send_and_wait(&id, WtMessageC2S::StopConsume { id: id.clone() })
            .await
            .context("Timed out waiting for ConsumeStopped")
    }

    /// Write an encoded access unit for a track. `keyframe` rolls a new MoQ group.
    pub fn send_media(
        &self,
        media_hint: MediaHint,
        capture_ts_us: u64,
        keyframe: bool,
        data: &[u8],
    ) -> Result<()> {
        self.command_tx
            .send(ClientCommand::WriteMedia {
                media_hint,
                capture_ts_us,
                keyframe,
                data: data.to_vec(),
            })
            .map_err(|_| anyhow::anyhow!("event loop shut down"))?;
        Ok(())
    }

    /// Ask the producer of `track_id` to emit a keyframe (PLI). Fire-and-forget.
    pub fn request_keyframe(&self, track_id: &str) -> Result<()> {
        self.command_tx
            .send(ClientCommand::SendCtl(WtMessageC2S::RequestKeyFrame {
                track_id: track_id.to_string(),
            }))
            .map_err(|_| anyhow::anyhow!("event loop shut down"))?;
        Ok(())
    }

    /// Send a periodic receiver report so the producer can adapt. Fire-and-forget.
    pub fn send_receiver_report(
        &self,
        track_id: &str,
        lost: u32,
        received: u32,
        jitter_ms: u32,
    ) -> Result<()> {
        self.command_tx
            .send(ClientCommand::SendCtl(WtMessageC2S::ReceiverReport {
                track_id: track_id.to_string(),
                lost,
                received,
                jitter_ms,
            }))
            .map_err(|_| anyhow::anyhow!("event loop shut down"))?;
        Ok(())
    }

    /// Gracefully disconnect (closes the MoQ session).
    pub fn disconnect(&self) {
        let _ = self.command_tx.send(ClientCommand::Shutdown);
    }

    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    async fn send_and_wait(&self, id: &str, message: WtMessageC2S) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.pending_requests.insert(id.to_string(), tx);
        self.command_tx
            .send(ClientCommand::SendCtl(message))
            .map_err(|_| anyhow::anyhow!("event loop shut down"))?;
        tokio::time::timeout(REQUEST_TIMEOUT, rx)
            .await
            .map_err(|_| anyhow::anyhow!("request timed out after {REQUEST_TIMEOUT:?}"))?
            .map_err(|_| anyhow::anyhow!("event loop dropped the response channel"))?;
        Ok(())
    }
}

struct MediaProducer {
    _broadcast: BroadcastProducer,
    track: TrackProducer,
    group: Option<GroupProducer>,
    next_seq: u64,
}

struct EventCtx {
    origin: OriginProducer,
    session: moq_net::Session,
    mls: Arc<Mutex<MlsClient>>,
    call_id: String,
    session_id: String,
    event_tx: mpsc::UnboundedSender<PulseEvent>,
    pending_requests: Arc<DashMap<String, oneshot::Sender<()>>>,
    ctl_group: GroupProducer,
    _ctl_broadcast: BroadcastProducer,
    _ctl_track: TrackProducer,
    producers: DashMap<String, MediaProducer>, // track_name -> producer
}

async fn event_loop(
    mut ctx: EventCtx,
    mut command_rx: mpsc::UnboundedReceiver<ClientCommand>,
) -> Result<()> {
    let (s2c_tx, mut s2c_rx) = mpsc::unbounded_channel();
    tokio::spawn(read_s2c(
        ctx.origin.clone(),
        ctx.call_id.clone(),
        ctx.session_id.clone(),
        s2c_tx,
    ));

    loop {
        tokio::select! {
            msg = s2c_rx.recv() => {
                let Some(msg) = msg else {
                    tracing::warn!("s2c control reader ended; shutting down Pulse session");
                    break;
                };
                handle_server_message(&mut ctx, msg).await?;
            }

            command = command_rx.recv() => {
                let Some(command) = command else {
                    tracing::info!("All PulseClient handles dropped; shutting down Pulse session");
                    break;
                };
                match command {
                    ClientCommand::SendCtl(msg) => {
                        if let Err(e) = write_ctl_frame(&mut ctx.ctl_group, &msg) {
                            tracing::warn!("Failed to write control frame: {e:#}");
                        }
                    }
                    ClientCommand::CreateProducer { media_hint, reply } => {
                        let _ = reply.send(create_producer(&mut ctx, media_hint));
                    }
                    ClientCommand::WriteMedia { media_hint, capture_ts_us, keyframe, data } => {
                        write_media(&mut ctx, media_hint, capture_ts_us, keyframe, data).await;
                    }
                    ClientCommand::StopProducer { media_hint } => {
                        ctx.producers.remove(track_name_for_hint(&media_hint));
                    }
                    ClientCommand::StartConsume { track, sink } => {
                        spawn_consumer(&ctx, track, sink);
                    }
                    ClientCommand::Shutdown => {
                        ctx.session.close(moq_net::Error::Cancel);
                        return Ok(());
                    }
                }
            }

            _ = ctx.session.closed() => {
                let _ = ctx.event_tx.send(PulseEvent::Disconnected { reconnect: None });
                break;
            }
        }
    }
    Ok(())
}

async fn read_s2c(
    origin: OriginProducer,
    call_id: String,
    session_id: String,
    tx: mpsc::UnboundedSender<WtMessageS2C>,
) {
    let path = format!("calls/{call_id}/{session_id}/{}", track_names::CTL_S2C);
    let sub_origin = origin.consume();
    let Some(bc) = sub_origin.announced_broadcast(&path).await else {
        tracing::warn!("server control track never announced");
        return;
    };
    let mut s2c = match bc.subscribe_track(&Track::new(track_names::CTL_S2C)) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("Failed to subscribe to s2c control track: {e:#}");
            return;
        }
    };

    loop {
        let mut group = match s2c.next_group().await {
            Ok(Some(g)) => g,
            Ok(None) => return,
            Err(e) => {
                tracing::warn!("s2c control track error: {e:#}");
                return;
            }
        };
        loop {
            match group.read_frame().await {
                Ok(Some(frame)) => {
                    match rkyv::api::high::from_bytes::<WtMessageS2C, rkyv::rancor::Error>(&frame) {
                        Ok(msg) => {
                            if tx.send(msg).is_err() {
                                return; // event loop gone
                            }
                        }
                        Err(e) => tracing::warn!("Failed to decode s2c control frame: {e:#}"),
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("s2c control frame read error: {e:#}");
                    return;
                }
            }
        }
    }
}

fn create_producer(ctx: &mut EventCtx, media_hint: MediaHint) -> Result<()> {
    let track_name = track_name_for_hint(&media_hint);
    if ctx.producers.contains_key(track_name) {
        return Ok(());
    }
    let path = format!("calls/{}/{}/{}", ctx.call_id, ctx.session_id, track_name);
    let mut broadcast = ctx
        .origin
        .create_broadcast(&path)
        .ok_or_else(|| anyhow::anyhow!("failed to create media broadcast {path}"))?;
    let track = broadcast
        .create_track(Track::new(track_name).with_priority(priority_for_hint(&media_hint)))
        .context("failed to create media track")?;
    ctx.producers.insert(
        track_name.to_string(),
        MediaProducer {
            _broadcast: broadcast,
            track,
            group: None,
            next_seq: 0,
        },
    );
    Ok(())
}

async fn write_media(
    ctx: &mut EventCtx,
    media_hint: MediaHint,
    capture_ts_us: u64,
    keyframe: bool,
    data: Vec<u8>,
) {
    let ciphertext = {
        let mls = ctx.mls.lock().await;
        if !mls.has_group() {
            tracing::warn!("Dropping media before MLS group ready");
            return;
        }
        match mls.encrypt_media(&data) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to encrypt media: {e:#}");
                return;
            }
        }
    };

    let track_name = track_name_for_hint(&media_hint);
    let Some(mut producer) = ctx.producers.get_mut(track_name) else {
        tracing::warn!("Write to unknown producer {track_name}");
        return;
    };

    // new group for each keyframe
    if keyframe || producer.group.is_none() {
        if let Some(mut old) = producer.group.take() {
            let _ = old.finish();
        }
        let seq = producer.next_seq;
        producer.next_seq += 1;
        match producer.track.create_group(seq.into()) {
            Ok(g) => producer.group = Some(g),
            Err(e) => {
                tracing::warn!("Failed to create media group: {e:#}");
                return;
            }
        }
    }

    let mut payload = Vec::with_capacity(pulse_types::MEDIA_FRAME_HEADER_LEN + ciphertext.len());
    payload.extend_from_slice(&encode_media_header(capture_ts_us));
    payload.extend_from_slice(&ciphertext);

    if let Some(group) = producer.group.as_mut() {
        if let Err(e) = group.write_frame(payload) {
            tracing::warn!("Failed to write media frame: {e:#}");
        }
    }
}

fn spawn_consumer(ctx: &EventCtx, track: AvailableTrack, sink: mpsc::UnboundedSender<MediaFrame>) {
    let origin = ctx.origin.clone();
    let mls = ctx.mls.clone();
    let call_id = ctx.call_id.clone();
    let track_name = track_name_for_hint(&track.media_hint).to_string();
    let producer_session = track.session_id.clone();

    tokio::spawn(async move {
        let path = format!("calls/{call_id}/{producer_session}/{track_name}");
        let sub_origin = origin.consume();
        let Some(bc) = sub_origin.announced_broadcast(&path).await else {
            tracing::warn!("Remote track {path} never announced");
            return;
        };
        let mut tc = match bc.subscribe_track(&Track::new(track_name.as_str())) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to subscribe to {path}: {e:#}");
                return;
            }
        };

        let mut last_seq: u64 = 0;
        loop {
            // if we fall behind, discard and jump ahead
            if let Some(latest) = tc.latest() {
                if latest.saturating_sub(last_seq) > MAX_GROUP_BACKLOG {
                    tc.start_at(latest);
                }
            }

            let mut group = match tc.next_group().await {
                Ok(Some(g)) => g,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("Track {path} error: {e:#}");
                    break;
                }
            };
            last_seq = group.sequence;
            let mut first = true;
            loop {
                match group.read_frame().await {
                    Ok(Some(frame)) => {
                        let keyframe = first;
                        first = false;
                        let Some(ts) = decode_media_header(&frame) else {
                            continue;
                        };
                        let ciphertext = &frame[pulse_types::MEDIA_FRAME_HEADER_LEN..];
                        let plaintext = {
                            let mls = mls.lock().await;
                            mls.decrypt_media(&producer_session, ciphertext)
                        };
                        match plaintext {
                            Ok(data) => {
                                if sink
                                    .send(MediaFrame {
                                        capture_ts_us: ts,
                                        keyframe,
                                        data,
                                    })
                                    .is_err()
                                {
                                    return; // consumer dropped
                                }
                            }
                            Err(e) => tracing::warn!("Decrypt failed for {path}: {e:#}"),
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        // producer aborted this group, skip
                        tracing::warn!("Frame read error on {path}: {e:#}");
                        break;
                    }
                }
            }
        }
    });
}

async fn handle_server_message(ctx: &mut EventCtx, msg: WtMessageS2C) -> Result<()> {
    match msg {
        WtMessageS2C::ProduceStarted { id }
        | WtMessageS2C::ProduceStopped { id }
        | WtMessageS2C::ConsumeStarted { id }
        | WtMessageS2C::ConsumeStopped { id } => resolve_pending(&ctx.pending_requests, &id),

        WtMessageS2C::TrackAvailable { track } => {
            let _ = ctx.event_tx.send(PulseEvent::TrackAvailable(track));
        }
        WtMessageS2C::TrackUnavailable { id } => {
            let _ = ctx.event_tx.send(PulseEvent::TrackUnavailable(id));
        }
        WtMessageS2C::Connected {
            id,
            available_tracks,
        } => {
            let _ = ctx.event_tx.send(PulseEvent::Connected {
                id,
                available_tracks,
            });
        }
        WtMessageS2C::Disconnected { reconnect } => {
            let _ = ctx.event_tx.send(PulseEvent::Disconnected { reconnect });
        }
        WtMessageS2C::InitializeGroup {
            external_sender_credential,
            external_sender_signature_key,
        } => {
            let mut mls = ctx.mls.lock().await;
            if let Err(e) =
                mls.initialize_group(&external_sender_credential, &external_sender_signature_key)
            {
                tracing::error!("Failed to initialize MLS group: {e:#}");
            }
        }
        WtMessageS2C::MlsProposals { proposals } => {
            let commit = {
                let mut mls = ctx.mls.lock().await;
                mls.create_commit(&proposals)
            };
            match commit {
                Ok((commit_data, epoch, welcome_data)) => {
                    write_ctl_frame(
                        &mut ctx.ctl_group,
                        &WtMessageC2S::MlsCommit {
                            commit_data,
                            epoch,
                            welcome_data,
                        },
                    )?;
                }
                Err(e) => tracing::error!("Failed to create MLS commit: {e:#}"),
            }
        }
        WtMessageS2C::MlsCommit {
            epoch,
            commit_data,
            welcome_data,
        } => {
            let (result, cur_epoch) = {
                let mut mls = ctx.mls.lock().await;
                let result = if !mls.has_group() {
                    if let Some(ref welcome) = welcome_data {
                        mls.join_from_welcome(welcome)
                    } else {
                        Err(anyhow::anyhow!("MlsCommit without welcome and no group"))
                    }
                } else {
                    mls.apply_commit(&commit_data)
                };
                (result, mls.current_epoch())
            };
            match result {
                Ok(()) => {
                    write_ctl_frame(
                        &mut ctx.ctl_group,
                        &WtMessageC2S::CommitAck { epoch: cur_epoch },
                    )?;
                }
                Err(e) => tracing::error!("Failed to apply MLS commit (epoch {epoch}): {e:#}"),
            }
        }
        WtMessageS2C::EpochReady { epoch } => {
            {
                let mut mls = ctx.mls.lock().await;
                mls.on_epoch_ready(epoch);
            }
            let _ = ctx.event_tx.send(PulseEvent::EpochReady(epoch));
        }
        WtMessageS2C::KeyFrameRequested { track_id } => {
            let _ = ctx.event_tx.send(PulseEvent::KeyFrameRequested(track_id));
        }
        WtMessageS2C::ReceiverReport {
            track_id,
            lost,
            received,
            jitter_ms,
        } => {
            tracing::debug!(
                track_id,
                lost,
                received,
                jitter_ms,
                "Received receiver report"
            );
        }
    }
    Ok(())
}

fn write_ctl_frame(group: &mut GroupProducer, message: &WtMessageC2S) -> Result<()> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(message)
        .map_err(|e| anyhow::anyhow!("failed to serialize control message: {e:?}"))?;
    group
        .write_frame(bytes.into_vec())
        .map_err(|e| anyhow::anyhow!("failed to write control frame: {e:?}"))?;
    Ok(())
}

fn resolve_pending(pending: &Arc<DashMap<String, oneshot::Sender<()>>>, id: &str) {
    if let Some((_, sender)) = pending.remove(id) {
        let _ = sender.send(());
    }
}

fn hint_from_track_name(name: &str) -> Option<MediaHint> {
    match name {
        track_names::MICROPHONE => Some(MediaHint::Audio),
        track_names::CAMERA => Some(MediaHint::Video),
        track_names::SCREEN => Some(MediaHint::ScreenVideo),
        track_names::SCREEN_AUDIO => Some(MediaHint::ScreenAudio),
        _ => None,
    }
}
