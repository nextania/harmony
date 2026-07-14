use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use moq_native::moq_net::{
    self, BroadcastProducer, GroupProducer, Origin, OriginProducer, Track, TrackProducer,
};
use pulse_types::{
    AvailableTrack, ControlC2S, ControlS2C, MediaHint, priority_for_hint, track_name_for_hint,
    track_names,
};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::error::PulseError;
use crate::events::PulseEvent;
use crate::mls::{MlsClient, MlsError, MlsIdentity};

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
    /// Account identity for authenticated group membership: signs our MLS
    /// credential and verifies other members against pinned identity keys.
    pub identity: MlsIdentity,
}

#[derive(Clone, Debug)]
pub struct MediaFrame {
    pub capture_ts_us: u64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct TrackHandle {
    media_hint: MediaHint,
}

impl TrackHandle {
    pub fn media_hint(&self) -> &MediaHint {
        &self.media_hint
    }
}

const MAX_GROUP_BACKLOG: u64 = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const ERROR_EVENT_INTERVAL: Duration = Duration::from_secs(5);

/// Handle to a connected Pulse session.
#[derive(Clone)]
pub struct PulseClient {
    command_tx: mpsc::UnboundedSender<ClientCommand>,
    call_id: String,
    session_id: String,
}

enum ClientCommand {
    SendCtl(ControlC2S),
    StartProduce {
        media_hint: MediaHint,
        reply: oneshot::Sender<Result<TrackHandle, PulseError>>,
    },
    StopProduce {
        media_hint: MediaHint,
        reply: oneshot::Sender<Result<(), PulseError>>,
    },
    WriteMedia {
        media_hint: MediaHint,
        capture_ts_us: u64,
        keyframe: bool,
        data: Vec<u8>,
    },
    StartConsume {
        track: AvailableTrack,
        sink: mpsc::UnboundedSender<MediaFrame>,
    },
    StopConsume {
        id: String,
    },
    Shutdown,
}

impl PulseClient {
    /// Connect to a Pulse server and start the background event loop.
    pub async fn connect(
        options: PulseClientOptions,
    ) -> Result<(Self, mpsc::UnboundedReceiver<PulseEvent>), PulseError> {
        let mls = MlsClient::new(
            &options.session_id,
            &options.call_id,
            options.identity.clone(),
        )?;
        let mls = Arc::new(Mutex::new(mls));

        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = oneshot::channel();

        let call_id = options.call_id.clone();
        let session_id = options.session_id.clone();
        tokio::spawn(supervisor(options, mls, command_rx, event_tx, ready_tx));

        ready_rx.await.map_err(|_| PulseError::Disconnected)??;

        Ok((
            Self {
                command_tx,
                call_id,
                session_id,
            },
            event_rx,
        ))
    }

    /// Start producing a track of the given kind. Creates the MoQ media track,
    /// announces it, and waits for the server to confirm.
    ///
    /// At most one track per [`MediaHint`] can be produced per session.
    pub async fn produce_track(&self, media_hint: MediaHint) -> Result<TrackHandle, PulseError> {
        let (reply, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::StartProduce { media_hint, reply })
            .map_err(|_| PulseError::Disconnected)?;
        tokio::time::timeout(REQUEST_TIMEOUT, reply_rx)
            .await
            .map_err(|_| PulseError::Timeout(REQUEST_TIMEOUT, "ProduceStarted"))?
            .map_err(|_| PulseError::Disconnected)?
    }

    /// Stop producing the track behind `handle` and wait for the server to
    /// confirm.
    pub async fn stop_producing(&self, handle: TrackHandle) -> Result<(), PulseError> {
        let (reply, reply_rx) = oneshot::channel();
        self.command_tx
            .send(ClientCommand::StopProduce {
                media_hint: handle.media_hint,
                reply,
            })
            .map_err(|_| PulseError::Disconnected)?;
        tokio::time::timeout(REQUEST_TIMEOUT, reply_rx)
            .await
            .map_err(|_| PulseError::Timeout(REQUEST_TIMEOUT, "ProduceStopped"))?
            .map_err(|_| PulseError::Disconnected)?
    }

    /// Subscribe to a remote track. Returns a stream of decrypted media frames.
    ///
    /// The stream ends when the producer stops, the track becomes unavailable,
    /// or the connection is lost; after a reconnect, re-subscribe from the
    /// fresh [`PulseEvent::TrackAvailable`] events.
    pub async fn consume_track(
        &self,
        track: &AvailableTrack,
    ) -> Result<mpsc::UnboundedReceiver<MediaFrame>, PulseError> {
        let (sink, rx) = mpsc::unbounded_channel();
        self.command_tx
            .send(ClientCommand::StartConsume {
                track: track.clone(),
                sink,
            })
            .map_err(|_| PulseError::Disconnected)?;

        if matches!(track.media_hint, MediaHint::Video | MediaHint::ScreenVideo) {
            let _ = self.request_keyframe(&track.id);
        }
        Ok(rx)
    }

    /// Stop consuming a remote track.
    pub fn stop_consuming(&self, id: String) -> Result<(), PulseError> {
        self.command_tx
            .send(ClientCommand::StopConsume { id })
            .map_err(|_| PulseError::Disconnected)
    }

    /// Write an encoded access unit for a track. `keyframe` rolls a new MoQ group.
    pub fn send_media(
        &self,
        handle: &TrackHandle,
        capture_ts_us: u64,
        keyframe: bool,
        data: &[u8],
    ) -> Result<(), PulseError> {
        self.command_tx
            .send(ClientCommand::WriteMedia {
                media_hint: handle.media_hint.clone(),
                capture_ts_us,
                keyframe,
                data: data.to_vec(),
            })
            .map_err(|_| PulseError::Disconnected)
    }

    /// Ask the producer of `track_id` to emit a keyframe (PLI). Fire-and-forget.
    pub fn request_keyframe(&self, track_id: &str) -> Result<(), PulseError> {
        self.command_tx
            .send(ClientCommand::SendCtl(ControlC2S::RequestKeyFrame {
                track_id: track_id.to_string(),
            }))
            .map_err(|_| PulseError::Disconnected)
    }

    /// Send a periodic receiver report so the producer can adapt. Fire-and-forget.
    pub fn send_receiver_report(
        &self,
        track_id: &str,
        lost: u32,
        received: u32,
        jitter_ms: u32,
    ) -> Result<(), PulseError> {
        self.command_tx
            .send(ClientCommand::SendCtl(ControlC2S::ReceiverReport {
                track_id: track_id.to_string(),
                lost,
                received,
                jitter_ms,
            }))
            .map_err(|_| PulseError::Disconnected)
    }

    /// Gracefully disconnect (closes the MoQ session, no reconnect).
    pub fn disconnect(&self) {
        let _ = self.command_tx.send(ClientCommand::Shutdown);
    }

    pub fn call_id(&self) -> &str {
        &self.call_id
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}

struct MediaProducer {
    _broadcast: BroadcastProducer,
    track: TrackProducer,
    group: Option<GroupProducer>,
    next_group_seq: u64,
    media_hint: MediaHint,
    server_track_id: Option<String>,
}

enum PendingKind {
    StartProduce {
        media_hint: MediaHint,
        reply: Option<oneshot::Sender<Result<TrackHandle, PulseError>>>,
    },
    StopProduce {
        reply: oneshot::Sender<Result<(), PulseError>>,
    },
}

struct PendingRequest {
    kind: PendingKind,
    deadline: Instant,
}

struct SessionCtx {
    origin: OriginProducer,
    session: moq_net::Session,
    ctl_track: TrackProducer,
    _ctl_broadcast: BroadcastProducer,
    s2c_rx: mpsc::UnboundedReceiver<ControlS2C>,
    buffered: Vec<ControlS2C>,
    producers: HashMap<&'static str, MediaProducer>, // track name -> producer
    consumers: HashMap<String, tokio::task::JoinHandle<()>>, // global track id -> task
    pending: HashMap<u64, PendingRequest>,
    last_crypto_error: Option<Instant>,
}

/// State that survives reconnects.
struct Shared {
    options: PulseClientOptions,
    mls: Arc<Mutex<MlsClient>>,
    event_tx: mpsc::UnboundedSender<PulseEvent>,
    active_hints: Vec<MediaHint>,
    next_request_id: u64,
}

enum SessionEnd {
    /// `disconnect()` was called or every `PulseClient` handle was dropped.
    Shutdown,
    /// The server deliberately terminated this session (kick or replacement);
    /// do not reconnect.
    Kicked,
    /// The connection was lost, optionally with a migration target
    /// `(server_url, token)` to reconnect to.
    Lost { redirect: Option<(String, String)> },
}

async fn supervisor(
    options: PulseClientOptions,
    mls: Arc<Mutex<MlsClient>>,
    mut command_rx: mpsc::UnboundedReceiver<ClientCommand>,
    event_tx: mpsc::UnboundedSender<PulseEvent>,
    ready_tx: oneshot::Sender<Result<(), PulseError>>,
) {
    let mut shared = Shared {
        options,
        mls,
        event_tx,
        active_hints: Vec::new(),
        next_request_id: 0,
    };

    let mut ctx = match establish(&shared.options, &shared.mls).await {
        Ok((ctx, id, tracks)) => {
            let _ = ready_tx.send(Ok(()));
            announce_connected(&shared.event_tx, id, tracks);
            ctx
        }
        Err(e) => {
            let _ = ready_tx.send(Err(e));
            return;
        }
    };

    loop {
        let end = run_session(&mut ctx, &mut command_rx, &mut shared).await;
        teardown(&mut ctx);

        match end {
            SessionEnd::Shutdown => return,
            SessionEnd::Kicked => {
                let _ = shared.event_tx.send(PulseEvent::Disconnected {
                    reason: "disconnected by server".to_string(),
                });
                return;
            }
            SessionEnd::Lost { redirect } => {
                if let Some((server_url, token)) = redirect {
                    shared.options.server_url = server_url;
                    shared.options.session_token = token;
                }
                let Some(new_ctx) = reconnect(&shared).await else {
                    let _ = shared.event_tx.send(PulseEvent::Disconnected {
                        reason: "reconnect attempts exhausted".to_string(),
                    });
                    return;
                };
                ctx = new_ctx;
                for hint in shared.active_hints.clone() {
                    if let Err(e) = start_producer(&mut ctx, &mut shared, hint, None) {
                        let _ = shared.event_tx.send(PulseEvent::Error(e));
                    }
                }
            }
        }
    }
}

async fn reconnect(shared: &Shared) -> Option<SessionCtx> {
    for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
        let _ = shared.event_tx.send(PulseEvent::Reconnecting { attempt });
        match establish(&shared.options, &shared.mls).await {
            Ok((ctx, id, tracks)) => {
                announce_connected(&shared.event_tx, id, tracks);
                return Some(ctx);
            }
            Err(e) => {
                tracing::warn!("Pulse reconnect attempt {attempt} failed: {e}");
                let _ = shared.event_tx.send(PulseEvent::Error(e));
                let backoff = Duration::from_millis(500)
                    .saturating_mul(1 << attempt.min(5))
                    .min(Duration::from_secs(10));
                tokio::time::sleep(backoff).await;
            }
        }
    }
    None
}

fn announce_connected(
    event_tx: &mpsc::UnboundedSender<PulseEvent>,
    id: String,
    available_tracks: Vec<AvailableTrack>,
) {
    let _ = event_tx.send(PulseEvent::Connected {
        id,
        available_tracks: available_tracks.clone(),
    });
    for track in available_tracks {
        let _ = event_tx.send(PulseEvent::TrackAvailable(track));
    }
}

fn teardown(ctx: &mut SessionCtx) {
    for (_, handle) in ctx.consumers.drain() {
        handle.abort();
    }
    for (_, pending) in ctx.pending.drain() {
        match pending.kind {
            PendingKind::StartProduce { reply, .. } => {
                if let Some(reply) = reply {
                    let _ = reply.send(Err(PulseError::Disconnected));
                }
            }
            PendingKind::StopProduce { reply } => {
                let _ = reply.send(Err(PulseError::Disconnected));
            }
        }
    }
    ctx.session.close(moq_net::Error::Cancel);
}

async fn establish(
    options: &PulseClientOptions,
    mls: &Arc<Mutex<MlsClient>>,
) -> Result<(SessionCtx, String, Vec<AvailableTrack>), PulseError> {
    let key_package = mls.lock().await.serialized_key_package()?;

    let origin = Origin::random().produce();

    let mut client_config = moq_native::ClientConfig::default();
    client_config.tls.disable_verify = Some(true); // TODO: proper cert validation
    let client = client_config
        .init()
        .map_err(|e| PulseError::Transport(Arc::new(e)))?;

    let mut url: url::Url = options
        .server_url
        .parse()
        .map_err(|_| PulseError::InvalidUrl(options.server_url.clone()))?;
    url.set_path("/call");
    url.query_pairs_mut()
        .clear()
        .append_pair("token", &options.session_token);

    let session = client
        .with_publish(origin.consume())
        .with_consume(origin.clone())
        .connect(url)
        .await
        .map_err(|e| PulseError::Transport(Arc::new(e)))?;

    let c2s_path = format!(
        "calls/{}/{}/{}",
        options.call_id,
        options.session_id,
        track_names::CTL_C2S
    );
    let mut ctl_broadcast = origin
        .create_broadcast(&c2s_path)
        .ok_or_else(|| PulseError::BroadcastCreation(c2s_path.clone()))?;
    let mut ctl_track = ctl_broadcast
        .create_track(Track::new(track_names::CTL_C2S))
        .map_err(|e| PulseError::Transport(Arc::new(e)))?;
    write_ctl_frame(&mut ctl_track, &ControlC2S::Join { key_package })?;

    let (s2c_tx, mut s2c_rx) = mpsc::unbounded_channel();
    tokio::spawn(read_s2c(
        origin.clone(),
        options.call_id.clone(),
        options.session_id.clone(),
        s2c_tx,
    ));

    // NOTE: dropping ANY cloned instance of moq_net::Session closes the
    // connection.
    let mut buffered = Vec::new();
    let connected = tokio::time::timeout(CONNECT_TIMEOUT, async {
        loop {
            tokio::select! {
                msg = s2c_rx.recv() => match msg {
                    Some(ControlS2C::Connected { id, available_tracks }) => {
                        return Ok((id, available_tracks));
                    }
                    Some(ControlS2C::Disconnected { .. }) | None => {
                        return Err(PulseError::ConnectRejected);
                    }
                    Some(other) => buffered.push(other),
                },
                _ = session.closed() => return Err(PulseError::ConnectRejected),
            }
        }
    })
    .await
    .map_err(|_| PulseError::Timeout(CONNECT_TIMEOUT, "Connected"))??;

    let (id, available_tracks) = connected;
    Ok((
        SessionCtx {
            origin,
            session,
            ctl_track,
            _ctl_broadcast: ctl_broadcast,
            s2c_rx,
            buffered,
            producers: HashMap::new(),
            consumers: HashMap::new(),
            pending: HashMap::new(),
            last_crypto_error: None,
        },
        id,
        available_tracks,
    ))
}

async fn run_session(
    ctx: &mut SessionCtx,
    command_rx: &mut mpsc::UnboundedReceiver<ClientCommand>,
    shared: &mut Shared,
) -> SessionEnd {
    for msg in std::mem::take(&mut ctx.buffered) {
        if let Some(end) = handle_server_message(ctx, shared, msg).await {
            return end;
        }
    }

    let mut sweep = tokio::time::interval(Duration::from_secs(1));
    sweep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = sweep.tick() => {
                sweep_expired_requests(ctx, shared);
            }

            msg = ctx.s2c_rx.recv() => {
                let Some(msg) = msg else {
                    tracing::warn!("s2c control reader ended; treating as connection loss");
                    return SessionEnd::Lost { redirect: None };
                };
                if let Some(end) = handle_server_message(ctx, shared, msg).await {
                    return end;
                }
            }

            command = command_rx.recv() => {
                let Some(command) = command else {
                    tracing::info!("All PulseClient handles dropped; shutting down Pulse session");
                    return SessionEnd::Shutdown;
                };
                match command {
                    ClientCommand::SendCtl(msg) => {
                        if let Err(e) = write_ctl_frame(&mut ctx.ctl_track, &msg) {
                            tracing::warn!("Failed to write control frame: {e}");
                        }
                    }
                    ClientCommand::StartProduce { media_hint, reply } => {
                        if let Err(e) = start_producer(ctx, shared, media_hint, Some(reply)) {
                            let _ = shared.event_tx.send(PulseEvent::Error(e));
                        }
                    }
                    ClientCommand::StopProduce { media_hint, reply } => {
                        stop_producer(ctx, shared, media_hint, reply);
                    }
                    ClientCommand::WriteMedia { media_hint, capture_ts_us, keyframe, data } => {
                        write_media(ctx, shared, media_hint, capture_ts_us, keyframe, data).await;
                    }
                    ClientCommand::StartConsume { track, sink } => {
                        let id = track.id.clone();
                        let handle = spawn_consumer(ctx, shared, track, sink);
                        if let Some(prev) = ctx.consumers.insert(id, handle) {
                            prev.abort();
                        }
                    }
                    ClientCommand::StopConsume { id } => {
                        if let Some(handle) = ctx.consumers.remove(&id) {
                            handle.abort();
                        }
                    }
                    ClientCommand::Shutdown => {
                        return SessionEnd::Shutdown;
                    }
                }
            }

            _ = ctx.session.closed() => {
                return SessionEnd::Lost { redirect: None };
            }
        }
    }
}

fn start_producer(
    ctx: &mut SessionCtx,
    shared: &mut Shared,
    media_hint: MediaHint,
    reply: Option<oneshot::Sender<Result<TrackHandle, PulseError>>>,
) -> Result<(), PulseError> {
    let track_name = track_name_for_hint(&media_hint);

    let result = (|| {
        if ctx.producers.contains_key(track_name) {
            return Err(PulseError::AlreadyProducing(media_hint.clone()));
        }
        let path = format!(
            "calls/{}/{}/{}",
            shared.options.call_id, shared.options.session_id, track_name
        );
        let mut broadcast = ctx
            .origin
            .create_broadcast(&path)
            .ok_or_else(|| PulseError::BroadcastCreation(path.clone()))?;
        let track = broadcast
            .create_track(Track::new(track_name).with_priority(priority_for_hint(&media_hint)))
            .map_err(|e| PulseError::Transport(Arc::new(e)))?;
        ctx.producers.insert(
            track_name,
            MediaProducer {
                _broadcast: broadcast,
                track,
                group: None,
                next_group_seq: 0,
                media_hint: media_hint.clone(),
                server_track_id: None,
            },
        );

        let request_id = shared.next_request_id;
        shared.next_request_id += 1;
        if let Err(e) = write_ctl_frame(
            &mut ctx.ctl_track,
            &ControlC2S::StartProduce {
                request_id,
                media_hint: media_hint.clone(),
            },
        ) {
            ctx.producers.remove(track_name);
            return Err(e);
        }
        Ok(request_id)
    })();

    match result {
        Ok(request_id) => {
            if !shared.active_hints.contains(&media_hint) {
                shared.active_hints.push(media_hint.clone());
            }
            ctx.pending.insert(
                request_id,
                PendingRequest {
                    kind: PendingKind::StartProduce { media_hint, reply },
                    deadline: Instant::now() + REQUEST_TIMEOUT,
                },
            );
            Ok(())
        }
        Err(e) => {
            if let Some(reply) = reply {
                let _ = reply.send(Err(e));
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

fn stop_producer(
    ctx: &mut SessionCtx,
    shared: &mut Shared,
    media_hint: MediaHint,
    reply: oneshot::Sender<Result<(), PulseError>>,
) {
    let track_name = track_name_for_hint(&media_hint);
    if ctx.producers.remove(track_name).is_none() {
        let _ = reply.send(Err(PulseError::NotProducing(media_hint)));
        return;
    }
    shared.active_hints.retain(|h| h != &media_hint);

    let request_id = shared.next_request_id;
    shared.next_request_id += 1;
    if let Err(e) = write_ctl_frame(
        &mut ctx.ctl_track,
        &ControlC2S::StopProduce {
            request_id,
            media_hint,
        },
    ) {
        let _ = reply.send(Err(e));
        return;
    }
    ctx.pending.insert(
        request_id,
        PendingRequest {
            kind: PendingKind::StopProduce { reply },
            deadline: Instant::now() + REQUEST_TIMEOUT,
        },
    );
}

fn sweep_expired_requests(ctx: &mut SessionCtx, shared: &mut Shared) {
    let now = Instant::now();
    let expired: Vec<u64> = ctx
        .pending
        .iter()
        .filter(|(_, req)| req.deadline <= now)
        .map(|(id, _)| *id)
        .collect();
    for id in expired {
        let Some(req) = ctx.pending.remove(&id) else {
            continue;
        };
        match req.kind {
            PendingKind::StartProduce { media_hint, reply } => {
                ctx.producers.remove(track_name_for_hint(&media_hint));
                shared.active_hints.retain(|h| h != &media_hint);
                if let Some(reply) = reply {
                    let _ = reply.send(Err(PulseError::Timeout(REQUEST_TIMEOUT, "ProduceStarted")));
                }
            }
            PendingKind::StopProduce { reply } => {
                let _ = reply.send(Err(PulseError::Timeout(REQUEST_TIMEOUT, "ProduceStopped")));
            }
        }
    }
}

async fn write_media(
    ctx: &mut SessionCtx,
    shared: &Shared,
    media_hint: MediaHint,
    capture_ts_us: u64,
    keyframe: bool,
    data: Vec<u8>,
) {
    let track_name = track_name_for_hint(&media_hint);
    if !ctx.producers.contains_key(track_name) {
        tracing::warn!("Write to unknown producer {track_name}");
        return;
    }

    let payload = {
        let mut mls = shared.mls.lock().await;
        if !mls.media_ready() {
            drop(mls);
            emit_crypto_error(ctx, shared, PulseError::Crypto(MlsError::NoActiveEpoch));
            return;
        }
        match mls.seal_media(track_name, capture_ts_us, &data) {
            Ok(p) => p,
            Err(e) => {
                drop(mls);
                emit_crypto_error(ctx, shared, PulseError::Crypto(e));
                return;
            }
        }
    };

    let Some(producer) = ctx.producers.get_mut(track_name) else {
        return;
    };

    // new group for each keyframe
    if keyframe || producer.group.is_none() {
        if let Some(mut old) = producer.group.take() {
            let _ = old.finish();
        }
        let seq = producer.next_group_seq;
        producer.next_group_seq += 1;
        match producer.track.create_group(seq.into()) {
            Ok(g) => producer.group = Some(g),
            Err(e) => {
                tracing::warn!("Failed to create media group: {e:?}");
                return;
            }
        }
    }

    if let Some(group) = producer.group.as_mut()
        && let Err(e) = group.write_frame(payload)
    {
        tracing::warn!("Failed to write media frame: {e:?}");
    }
}

fn emit_crypto_error(ctx: &mut SessionCtx, shared: &Shared, error: PulseError) {
    tracing::warn!("{error}");
    let now = Instant::now();
    if ctx
        .last_crypto_error
        .is_none_or(|last| now.duration_since(last) >= ERROR_EVENT_INTERVAL)
    {
        ctx.last_crypto_error = Some(now);
        let _ = shared.event_tx.send(PulseEvent::Error(error));
    }
}

fn spawn_consumer(
    ctx: &SessionCtx,
    shared: &Shared,
    track: AvailableTrack,
    sink: mpsc::UnboundedSender<MediaFrame>,
) -> tokio::task::JoinHandle<()> {
    let origin = ctx.origin.clone();
    let mls = shared.mls.clone();
    let event_tx = shared.event_tx.clone();
    let call_id = shared.options.call_id.clone();
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

        let mut last_group_seq: u64 = 0;
        let mut last_error_emit: Option<Instant> = None;
        loop {
            // if we fall behind, discard and jump ahead
            if let Some(latest) = tc.latest()
                && latest.saturating_sub(last_group_seq) > MAX_GROUP_BACKLOG
            {
                tc.start_at(latest);
            }

            let mut group = match tc.next_group().await {
                Ok(Some(g)) => g,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("Track {path} error: {e:#}");
                    break;
                }
            };
            last_group_seq = group.sequence;
            let mut first = true;
            loop {
                match group.read_frame().await {
                    Ok(Some(frame)) => {
                        let keyframe = first;
                        first = false;
                        let opened = {
                            let mut mls = mls.lock().await;
                            mls.open_media(&producer_session, &track_name, &frame)
                        };
                        match opened {
                            Ok((header, data)) => {
                                if sink
                                    .send(MediaFrame {
                                        capture_ts_us: header.capture_ts_us,
                                        keyframe,
                                        data,
                                    })
                                    .is_err()
                                {
                                    return; // consumer dropped
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Decrypt failed for {path}: {e:#}");
                                let now = Instant::now();
                                if last_error_emit.is_none_or(|last| {
                                    now.duration_since(last) >= ERROR_EVENT_INTERVAL
                                }) {
                                    last_error_emit = Some(now);
                                    let _ = event_tx.send(PulseEvent::Error(PulseError::Crypto(e)));
                                }
                            }
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
    })
}

async fn handle_server_message(
    ctx: &mut SessionCtx,
    shared: &mut Shared,
    msg: ControlS2C,
) -> Option<SessionEnd> {
    match msg {
        ControlS2C::ProduceStarted {
            request_id,
            track_id,
        } => match ctx.pending.remove(&request_id).map(|r| r.kind) {
            Some(PendingKind::StartProduce { media_hint, reply }) => {
                let track_name = track_name_for_hint(&media_hint);
                if let Some(producer) = ctx.producers.get_mut(track_name) {
                    producer.server_track_id = Some(track_id);
                }
                if let Some(reply) = reply {
                    let _ = reply.send(Ok(TrackHandle { media_hint }));
                }
            }
            _ => tracing::warn!("ProduceStarted for unknown request {request_id}"),
        },
        ControlS2C::ProduceStopped { request_id } => {
            match ctx.pending.remove(&request_id).map(|r| r.kind) {
                Some(PendingKind::StopProduce { reply }) => {
                    let _ = reply.send(Ok(()));
                }
                _ => tracing::warn!("ProduceStopped for unknown request {request_id}"),
            }
        }
        ControlS2C::ProduceFailed { request_id, reason } => {
            match ctx.pending.remove(&request_id).map(|r| r.kind) {
                Some(PendingKind::StartProduce { media_hint, reply }) => {
                    ctx.producers.remove(track_name_for_hint(&media_hint));
                    shared.active_hints.retain(|h| h != &media_hint);
                    match reply {
                        Some(reply) => {
                            let _ = reply.send(Err(PulseError::Rejected(reason)));
                        }
                        None => {
                            let _ = shared
                                .event_tx
                                .send(PulseEvent::Error(PulseError::Rejected(reason)));
                        }
                    }
                }
                Some(PendingKind::StopProduce { reply }) => {
                    let _ = reply.send(Err(PulseError::Rejected(reason)));
                }
                None => tracing::warn!("ProduceFailed for unknown request {request_id}: {reason}"),
            }
        }

        ControlS2C::TrackAvailable { track } => {
            let _ = shared.event_tx.send(PulseEvent::TrackAvailable(track));
        }
        ControlS2C::TrackUnavailable { id } => {
            if let Some(handle) = ctx.consumers.remove(&id) {
                handle.abort();
            }
            let _ = shared.event_tx.send(PulseEvent::TrackUnavailable(id));
        }
        ControlS2C::Connected {
            id,
            available_tracks,
        } => {
            announce_connected(&shared.event_tx, id, available_tracks);
        }
        ControlS2C::Disconnected { reconnect } => {
            return Some(match reconnect {
                Some(redirect) => SessionEnd::Lost {
                    redirect: Some(redirect),
                },
                None => SessionEnd::Kicked,
            });
        }
        ControlS2C::InitializeGroup {
            external_sender_credential,
            external_sender_signature_key,
        } => {
            let mut mls = shared.mls.lock().await;
            match mls.initialize_group(&external_sender_credential, &external_sender_signature_key)
            {
                Ok(()) => {
                    let (epoch, members) = (mls.current_epoch(), mls.roster());
                    drop(mls);
                    let _ = shared
                        .event_tx
                        .send(PulseEvent::MembershipChanged { epoch, members });
                }
                Err(e) => {
                    drop(mls);
                    emit_mls_error(shared, e);
                }
            }
        }
        ControlS2C::MlsProposals { proposals } => {
            let commit = {
                let mut mls = shared.mls.lock().await;
                mls.create_commit(&proposals)
            };
            match commit {
                Ok((commit_data, epoch, welcome_data)) => {
                    if let Err(e) = write_ctl_frame(
                        &mut ctx.ctl_track,
                        &ControlC2S::MlsCommit {
                            commit_data,
                            epoch,
                            welcome_data,
                        },
                    ) {
                        emit_error(shared, e);
                    }
                }
                Err(e) => emit_mls_error(shared, e),
            }
        }
        ControlS2C::MlsCommit {
            epoch,
            commit_data,
            welcome_data,
        } => {
            let (result, cur_epoch, roster) = {
                let mut mls = shared.mls.lock().await;
                let result = if !mls.has_group() {
                    if let Some(ref welcome) = welcome_data {
                        mls.join_from_welcome(welcome, epoch)
                    } else {
                        Err(MlsError::CommitWithoutGroup)
                    }
                } else {
                    mls.apply_commit(&commit_data, epoch)
                };
                let roster = result.is_ok().then(|| mls.roster());
                (result, mls.current_epoch(), roster)
            };
            match result {
                Ok(()) => {
                    let _ = shared.event_tx.send(PulseEvent::MembershipChanged {
                        epoch: cur_epoch,
                        members: roster.unwrap_or_default(),
                    });
                    if let Err(e) = write_ctl_frame(
                        &mut ctx.ctl_track,
                        &ControlC2S::CommitAck { epoch: cur_epoch },
                    ) {
                        emit_error(shared, e);
                    }
                }
                Err(e) => emit_mls_error(shared, e),
            }
        }
        ControlS2C::EpochReady { epoch } => {
            {
                let mut mls = shared.mls.lock().await;
                mls.on_epoch_ready(epoch);
            }
            let _ = shared.event_tx.send(PulseEvent::EpochReady(epoch));
        }
        ControlS2C::KeyFrameRequested { track_id } => {
            let hint = ctx
                .producers
                .values()
                .find(|p| p.server_track_id.as_deref() == Some(track_id.as_str()))
                .map(|p| p.media_hint.clone());
            match hint {
                Some(hint) => {
                    let _ = shared.event_tx.send(PulseEvent::KeyFrameRequested(hint));
                }
                None => tracing::warn!("KeyFrameRequested for unknown track {track_id}"),
            }
        }
        ControlS2C::ReceiverReport {
            track_id,
            lost,
            received,
            jitter_ms,
        } => {
            let hint = ctx
                .producers
                .values()
                .find(|p| p.server_track_id.as_deref() == Some(track_id.as_str()))
                .map(|p| p.media_hint.clone());
            match hint {
                Some(media_hint) => {
                    let _ = shared.event_tx.send(PulseEvent::ReceiverReport {
                        media_hint,
                        lost,
                        received,
                        jitter_ms,
                    });
                }
                None => tracing::debug!(track_id, "ReceiverReport for unknown track"),
            }
        }
    }
    None
}

fn emit_mls_error(shared: &Shared, message: MlsError) {
    emit_error(shared, PulseError::Mls(message));
}

fn emit_error(shared: &Shared, error: PulseError) {
    tracing::error!("{error}");
    let _ = shared.event_tx.send(PulseEvent::Error(error));
}

async fn read_s2c(
    origin: OriginProducer,
    call_id: String,
    session_id: String,
    tx: mpsc::UnboundedSender<ControlS2C>,
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
                Ok(Some(frame)) => match serde_cbor_2::from_slice::<ControlS2C>(&frame) {
                    Ok(msg) => {
                        if tx.send(msg).is_err() {
                            return; // event loop gone
                        }
                    }
                    Err(e) => tracing::warn!("Failed to decode s2c control frame: {e:#}"),
                },
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("s2c control frame read error: {e:#}");
                    return;
                }
            }
        }
    }
}

fn write_ctl_frame(track: &mut TrackProducer, message: &ControlC2S) -> Result<(), PulseError> {
    let bytes =
        serde_cbor_2::to_vec(message).map_err(|e| PulseError::ControlSerialization(Arc::new(e)))?;
    track
        .write_frame(bytes)
        .map_err(|e| PulseError::Transport(Arc::new(e)))?;
    Ok(())
}
