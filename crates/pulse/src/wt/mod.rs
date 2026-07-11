pub mod call;

use common::{NodeEvent, NodeEventKind, SessionData};
use dashmap::DashMap;
use lazy_static::lazy_static;
use moq_native::moq_net::{self, BroadcastProducer, Origin, OriginProducer, Track};
use pulse_types::{MediaHint, WtMessageC2S, WtMessageS2C, track_names};
use redis::AsyncCommands;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use tokio::{task, time};
use ulid::Ulid;

use crate::metrics::CONNECTIONS_ACTIVE;
use crate::redis::INSTANCE_ID;
use crate::wt::call::{Call, MlsState, PendingProposal};

#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub id: String,              // global unique track ID
    pub client_track_id: String, // client-provided track ID
    pub media_hint: MediaHint,
    pub session_id: String,
    pub producer_session: SessionState,
}

#[derive(Clone, Debug)]
pub struct SessionState {
    pub id: String,
    pub session_id: String,
    pub call_id: String,
    pub session_token: String,

    pub message_tx: mpsc::UnboundedSender<WtMessageS2C>,

    pub close_tx: mpsc::UnboundedSender<()>,
    pub can_listen: Arc<AtomicBool>,
    pub can_speak: Arc<AtomicBool>,
    pub can_video: Arc<AtomicBool>,
    pub can_screen: Arc<AtomicBool>,
    pub producers: Arc<DashMap<String, TrackInfo>>, // track_id -> TrackInfo
}

impl SessionState {
    pub fn close(&self, _reason: &str) {
        let _ = self.close_tx.send(());
    }
}

lazy_static! {
    pub static ref GLOBAL_ORIGIN: OriginProducer = Origin::random().produce();
    pub static ref GLOBAL_CALLS: Arc<DashMap<String, Call>> = Arc::new(DashMap::new());
    pub static ref GLOBAL_SESSIONS: Arc<DashMap<String, SessionState>> = Arc::new(DashMap::new());
    pub static ref GLOBAL_UNIQUE_SESSIONS: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
}

fn broadcast_path(call_id: &str, session_id: &str, track: &str) -> String {
    format!("calls/{call_id}/{session_id}/{track}")
}

pub async fn listen() -> anyhow::Result<()> {
    let mut config = moq_native::ServerConfig::default();
    config.bind = Some("[::]:4433".to_string());
    // TODO: get proper certificate
    config.tls.cert = vec!["cert.pem".into()];
    config.tls.key = vec!["key.pem".into()];

    let mut server = config.init()?;
    info!("Pulse MoQ endpoint listening on [::]:4433");

    loop {
        let Some(request) = server.accept().await else {
            continue;
        };
        task::spawn(async move {
            if let Err(e) = handle_request(request).await {
                error!("Session error: {:?}", e);
            }
        });
    }
}

async fn authorize(request: &moq_native::Request) -> anyhow::Result<(String, SessionData)> {
    let url = request
        .url()
        .ok_or_else(|| anyhow::anyhow!("connect URL missing (unsupported transport)"))?;
    let token = url
        .query_pairs()
        .find(|(k, _)| k == "token")
        .map(|(_, v)| v.to_string())
        .ok_or_else(|| anyhow::anyhow!("missing token in connect URL"))?;

    let mut redis_conn = crate::redis::get_connection().await;
    let session_data: Option<SessionData> = redis_conn.get(format!("session:{token}")).await?;
    let session_data = session_data.ok_or_else(|| anyhow::anyhow!("invalid session token"))?;

    if session_data.assigned_server != *INSTANCE_ID {
        return Err(anyhow::anyhow!(
            "session token assigned to a different server: {}",
            session_data.assigned_server
        ));
    }

    Ok((token, session_data))
}

async fn handle_request(request: moq_native::Request) -> anyhow::Result<()> {
    let (token, session_data) = match authorize(&request).await {
        Ok(v) => v,
        Err(e) => {
            warn!("Rejecting connection: {:?}", e);
            request.close(0).await.ok();
            return Ok(());
        }
    };

    let call_id = session_data.call_id.clone();
    let session_id = session_data.session_id.clone();
    let unique_id = Ulid::new().to_string();

    let call_prefix: moq_net::Path = format!("calls/{call_id}").into();
    let session_prefix: moq_net::Path = format!("calls/{call_id}/{session_id}").into();

    let publish_gate = GLOBAL_ORIGIN
        .consume()
        .scope(&[call_prefix])
        .ok_or_else(|| anyhow::anyhow!("failed to scope publish origin"))?;
    let consume_gate = GLOBAL_ORIGIN
        .scope(&[session_prefix])
        .ok_or_else(|| anyhow::anyhow!("failed to scope consume origin"))?;

    let mut session = request
        .with_publish(publish_gate)
        .with_consume(consume_gate)
        .ok()
        .await?;

    info!(
        "New MoQ session for user {} in call {}",
        session_id, call_id
    );
    CONNECTIONS_ACTIVE.add(1, &[]);

    // persistent s2c track
    let (message_tx, message_rx) = mpsc::unbounded_channel::<WtMessageS2C>();
    let (close_tx, mut close_rx) = mpsc::unbounded_channel::<()>();
    let s2c_path = broadcast_path(&call_id, &session_id, track_names::CTL_S2C);
    spawn_s2c_writer(s2c_path, message_rx);

    let ttl_task = spawn_ttl_refresh(token.clone());

    // persistent c2s track
    let c2s_path = broadcast_path(&call_id, &session_id, track_names::CTL_C2S);
    let ctl_result = tokio::select! {
        r = run_control(&c2s_path, &unique_id, &token, &session_data, message_tx.clone(), close_tx.clone()) => r,
        _ = session.closed() => Ok(()),
        _ = close_rx.recv() => Ok(()),
    };
    ttl_task.abort();
    if let Err(e) = &ctl_result {
        warn!("Control loop ended with error: {:?}", e);
    }

    session.close(moq_net::Error::Cancel);
    CONNECTIONS_ACTIVE.add(-1, &[]);
    cleanup_session(&unique_id).await;
    Ok(())
}

fn spawn_s2c_writer(path: String, mut rx: mpsc::UnboundedReceiver<WtMessageS2C>) {
    task::spawn(async move {
        let mut broadcast: BroadcastProducer = match GLOBAL_ORIGIN.create_broadcast(path.as_str()) {
            Some(b) => b,
            None => {
                error!("Failed to create s2c control broadcast {}", path);
                return;
            }
        };
        let mut track = match broadcast.create_track(Track::new(track_names::CTL_S2C)) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to create s2c control track: {:?}", e);
                return;
            }
        };
        let mut group = match track.append_group() {
            Ok(g) => g,
            Err(e) => {
                error!("Failed to open s2c control group: {:?}", e);
                return;
            }
        };

        while let Some(message) = rx.recv().await {
            match rkyv::to_bytes::<rkyv::rancor::Error>(&message) {
                Ok(bytes) => {
                    if let Err(e) = group.write_frame(bytes.into_vec()) {
                        warn!("Failed to write s2c control frame: {:?}", e);
                        break;
                    }
                }
                Err(e) => warn!("Failed to serialize s2c control message: {:?}", e),
            }
        }
    });
}

async fn run_control(
    c2s_path: &str,
    unique_id: &str,
    token: &str,
    session_data: &SessionData,
    message_tx: mpsc::UnboundedSender<WtMessageS2C>,
    close_tx: mpsc::UnboundedSender<()>,
) -> anyhow::Result<()> {
    let origin = GLOBAL_ORIGIN.consume();
    let ctl_bc = origin
        .announced_broadcast(c2s_path)
        .await
        .ok_or_else(|| anyhow::anyhow!("client control track never announced"))?;
    let mut ctl = ctl_bc.subscribe_track(&Track::new(track_names::CTL_C2S))?;

    let mut joined = false;
    while let Some(mut group) = ctl.next_group().await? {
        while let Some(frame) = group.read_frame().await? {
            let message: WtMessageC2S =
                match rkyv::api::high::from_bytes::<_, rkyv::rancor::Error>(&frame) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("Failed to decode control frame: {:?}", e);
                        continue;
                    }
                };

            if !joined {
                // the first message must be Join
                let WtMessageC2S::Join { key_package } = message else {
                    return Err(anyhow::anyhow!("first control frame was not Join"));
                };
                handle_join(
                    unique_id,
                    token,
                    session_data,
                    key_package,
                    message_tx.clone(),
                    close_tx.clone(),
                )
                .await?;
                joined = true;
                continue;
            }

            dispatch_message(unique_id, message).await?;
        }
    }
    Ok(())
}

fn spawn_ttl_refresh(token: String) -> task::JoinHandle<()> {
    task::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let mut redis_conn = crate::redis::get_connection().await;
            if let Err(e) = redis_conn
                .expire::<_, ()>(&format!("session:{token}"), 60)
                .await
            {
                warn!("Failed to refresh session TTL: {:?}", e);
            }
        }
    })
}

async fn handle_join(
    unique_id: &str,
    token: &str,
    session_data: &SessionData,
    key_package: Vec<u8>,
    message_tx: mpsc::UnboundedSender<WtMessageS2C>,
    close_tx: mpsc::UnboundedSender<()>,
) -> anyhow::Result<()> {
    // reconnection
    if let Some((_, old)) = GLOBAL_SESSIONS.remove(&session_data.session_id) {
        GLOBAL_UNIQUE_SESSIONS.remove(&old.id);
        let _ = old
            .message_tx
            .send(WtMessageS2C::Disconnected { reconnect: None });
        old.close("replaced by reconnection");
    }

    let state = SessionState {
        id: unique_id.to_string(),
        session_id: session_data.session_id.clone(),
        call_id: session_data.call_id.clone(),
        session_token: token.to_string(),
        message_tx: message_tx.clone(),
        close_tx,
        can_listen: Arc::new(AtomicBool::new(session_data.can_listen)),
        can_speak: Arc::new(AtomicBool::new(session_data.can_speak)),
        can_video: Arc::new(AtomicBool::new(session_data.can_video)),
        can_screen: Arc::new(AtomicBool::new(session_data.can_screen)),
        producers: Arc::new(DashMap::new()),
    };
    GLOBAL_SESSIONS.insert(state.session_id.clone(), state.clone());
    GLOBAL_UNIQUE_SESSIONS.insert(state.id.clone(), state.session_id.clone());

    let call = GLOBAL_CALLS
        .entry(session_data.call_id.clone())
        .or_insert_with(|| Call {
            id: session_data.call_id.clone(),
            tracks: DashMap::new(),
            members: DashMap::new(),
            mls_state: Arc::new(Mutex::new(MlsState {
                current_epoch: 0,
                pending_proposals: Vec::new(),
                pending_commit: None,
                pending_acks: HashSet::new(),
                full_members: Vec::new(),
                pending_epoch_change: false,
                pending_members: Vec::new(),
            })),
        });

    let is_first_member = call.members.is_empty();
    call.add_member(state.session_id.clone(), key_package).await;
    broadcast_proposals(&call).await;
    let available_tracks = call.get_available_tracks(&state.session_id);
    drop(call);

    let _ = message_tx.send(WtMessageS2C::Connected {
        id: state.session_id.clone(),
        available_tracks,
    });

    if is_first_member {
        let external_sender_credential = crate::environment::EXTERNAL_SENDER
            .serialize_credential()
            .unwrap_or_default();
        let external_sender_signature_key = crate::environment::EXTERNAL_SENDER
            .signature_public_key()
            .clone();
        let _ = message_tx.send(WtMessageS2C::InitializeGroup {
            external_sender_credential,
            external_sender_signature_key,
        });
        info!(
            "Sent InitializeGroup to first member in call {}",
            state.call_id
        );
    }

    let mut redis_conn = crate::redis::get_connection().await;
    let event = NodeEvent {
        id: INSTANCE_ID.to_string(),
        event: NodeEventKind::UserConnect {
            id: session_data.session_id.clone(),
            call_id: session_data.call_id.clone(),
        },
    };
    let _: Result<(), redis::RedisError> = redis_conn
        .xadd::<_, _, _, _, ()>("voice:events:user-lifecycle", "*", &[("data", event)])
        .await;
    redis_conn
        .expire::<_, ()>(&format!("session:{token}"), 60)
        .await?;

    info!(
        "Session {} authenticated for user {}",
        unique_id, state.session_id
    );
    Ok(())
}

async fn dispatch_message(unique_id: &str, message: WtMessageC2S) -> anyhow::Result<()> {
    let Some(state) = GLOBAL_UNIQUE_SESSIONS
        .get(unique_id)
        .map(|s| s.value().clone())
        .and_then(|id| GLOBAL_SESSIONS.get(&id).map(|s| s.clone()))
    else {
        warn!("Control message for unknown session {}", unique_id);
        return Ok(());
    };

    match message {
        WtMessageC2S::Join { .. } => warn!("Duplicate Join ignored"),
        WtMessageC2S::StartProduce { id, media_hint } => {
            handle_start_produce(id, media_hint, &state).await;
        }
        WtMessageC2S::StopProduce { id } => handle_stop_produce(id, &state),
        WtMessageC2S::MlsCommit {
            commit_data,
            epoch,
            welcome_data,
        } => handle_mls_commit(commit_data, epoch, welcome_data, &state).await,
        WtMessageC2S::CommitAck { epoch } => handle_commit_ack(epoch, &state).await,
        WtMessageC2S::RequestKeyFrame { track_id } => handle_request_keyframe(track_id, &state),
        WtMessageC2S::ReceiverReport {
            track_id,
            lost,
            received,
            jitter_ms,
        } => handle_receiver_report(track_id, lost, received, jitter_ms, &state),
    }
    Ok(())
}

async fn handle_start_produce(
    client_track_id: String,
    media_hint: MediaHint,
    state: &SessionState,
) {
    let allowed = match media_hint {
        MediaHint::Audio => state.can_speak.load(Ordering::SeqCst),
        MediaHint::Video => state.can_video.load(Ordering::SeqCst),
        MediaHint::ScreenAudio | MediaHint::ScreenVideo => state.can_screen.load(Ordering::SeqCst),
    };
    if !allowed {
        warn!("User lacks permission to produce {:?}", media_hint);
        return;
    }
    for track in state.producers.iter() {
        if std::mem::discriminant(&track.media_hint) == std::mem::discriminant(&media_hint) {
            warn!("Already producing track of type {:?}", media_hint);
            return;
        }
    }

    let global_track_id = Ulid::new().to_string();
    let track_info = TrackInfo {
        id: global_track_id.clone(),
        client_track_id: client_track_id.clone(),
        media_hint: media_hint.clone(),
        session_id: state.session_id.clone(),
        producer_session: state.clone(),
    };
    state
        .producers
        .insert(global_track_id.clone(), track_info.clone());

    if let Some(call) = GLOBAL_CALLS.get(&state.call_id) {
        call.start_producing(&state.session_id, track_info).await;
    }
    let _ = state.message_tx.send(WtMessageS2C::ProduceStarted {
        id: client_track_id,
    });
}

fn handle_stop_produce(client_track_id: String, state: &SessionState) {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        return;
    };
    let Some(global_track_id) = call.get_mapped_track_id(&client_track_id, &state.session_id)
    else {
        warn!("StopProduce for unknown track {}", client_track_id);
        return;
    };
    call.stop_producing(&state.session_id, &global_track_id);
    drop(call);
    state.producers.remove(&global_track_id);
    let _ = state.message_tx.send(WtMessageS2C::ProduceStopped {
        id: client_track_id,
    });
}

fn handle_request_keyframe(track_id: String, state: &SessionState) {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        return;
    };
    if let Some(track) = call.tracks.get(&track_id) {
        let _ = track
            .producer_session
            .message_tx
            .send(WtMessageS2C::KeyFrameRequested {
                track_id: track.client_track_id.clone(),
            });
    }
}

fn handle_receiver_report(
    track_id: String,
    lost: u32,
    received: u32,
    jitter_ms: u32,
    state: &SessionState,
) {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        return;
    };
    if let Some(track) = call.tracks.get(&track_id) {
        let _ = track
            .producer_session
            .message_tx
            .send(WtMessageS2C::ReceiverReport {
                track_id: track.client_track_id.clone(),
                lost,
                received,
                jitter_ms,
            });
    }
}

async fn broadcast_proposals(call: &Call) {
    let proposals = call.flush_proposals().await;
    if let Some((proposals, recipients, epoch)) = proposals {
        for recipient in recipients {
            if let Some(session) = GLOBAL_SESSIONS.get(&recipient) {
                let _ = session.message_tx.send(WtMessageS2C::MlsProposals {
                    proposals: proposals.clone(),
                });
            }
        }
        let state = call.mls_state.clone();
        let id = call.id.clone();
        task::spawn(async move {
            time::sleep(Duration::from_secs(10)).await;
            let state = state.lock().await;
            if state.pending_commit.is_some() && state.current_epoch == epoch {
                GLOBAL_CALLS.remove(&id);
                info!("Destroyed call {} due to inactivity", id);
            }
        });
    }
}

async fn handle_mls_commit(
    commit_data: Vec<u8>,
    epoch: u64,
    welcome_data: Option<Vec<u8>>,
    state: &SessionState,
) {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        return;
    };
    let mut mls_state = call.mls_state.lock().await;
    let Some(pending_commit) = mls_state.pending_commit.take() else {
        return;
    };
    if mls_state.current_epoch != epoch {
        warn!(
            "Received commit for epoch {}, but current epoch is {}",
            epoch, mls_state.current_epoch
        );
        return;
    }
    let new_members = pending_commit
        .proposals
        .iter()
        .filter_map(|p| match p {
            PendingProposal::Add { session_id, .. } => Some(session_id.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();

    for recipient in mls_state.full_members.iter() {
        if let Some(session) = GLOBAL_SESSIONS.get(recipient) {
            let _ = session.message_tx.send(WtMessageS2C::MlsCommit {
                commit_data: commit_data.clone(),
                epoch,
                welcome_data: None,
            });
        }
    }
    for new_member in new_members.iter() {
        if let Some(session) = GLOBAL_SESSIONS.get(new_member) {
            let _ = session.message_tx.send(WtMessageS2C::MlsCommit {
                commit_data: commit_data.clone(),
                epoch,
                welcome_data: welcome_data.clone(),
            });
        }
        mls_state.full_members.push(new_member.clone());
    }
    mls_state.pending_epoch_change = true;
    drop(mls_state);
    info!(
        "Forwarded MLS commit to all members of call {}",
        state.call_id
    );

    let call_id = state.call_id.clone();
    task::spawn(async move {
        time::sleep(Duration::from_secs(10)).await;
        if let Some(call) = GLOBAL_CALLS.get(&call_id) {
            if let Some(new_epoch) = call.increment_epoch().await {
                for recipient in call.mls_state.lock().await.full_members.iter() {
                    if let Some(session) = GLOBAL_SESSIONS.get(recipient) {
                        let _ = session
                            .message_tx
                            .send(WtMessageS2C::EpochReady { epoch: new_epoch });
                    }
                }
                info!("Advanced to epoch {} for call {}", new_epoch, call_id);
            }
        }
    });
}

async fn handle_commit_ack(epoch: u64, state: &SessionState) {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        return;
    };
    if call.record_commit_ack(&state.session_id, epoch).await {
        if let Some(new_epoch) = call.increment_epoch().await {
            for recipient in call.mls_state.lock().await.full_members.iter() {
                if let Some(session) = GLOBAL_SESSIONS.get(recipient) {
                    let _ = session
                        .message_tx
                        .send(WtMessageS2C::EpochReady { epoch: new_epoch });
                }
            }
        }
    }
}

async fn cleanup_session(unique_id: &str) {
    info!("Cleaning up session {}", unique_id);
    let Some(id) = GLOBAL_UNIQUE_SESSIONS.remove(unique_id).map(|(_, id)| id) else {
        return;
    };
    let Some((_, state)) = GLOBAL_SESSIONS.remove(&id) else {
        return;
    };

    if let Some(call) = GLOBAL_CALLS.get(&state.call_id) {
        call.remove_member(&state.session_id).await;
        let producer_ids: Vec<String> = state.producers.iter().map(|t| t.id.clone()).collect();
        for global_id in producer_ids {
            call.stop_producing(&state.session_id, &global_id);
        }
        broadcast_proposals(&call).await;
    }

    let mut redis_conn = crate::redis::get_connection().await;
    let event = NodeEvent {
        id: INSTANCE_ID.to_string(),
        event: NodeEventKind::UserDisconnect {
            id: state.session_id.clone(),
            call_id: state.call_id.clone(),
        },
    };
    let _: Result<(), redis::RedisError> = redis_conn
        .xadd::<_, _, _, _, ()>("voice:events:user-lifecycle", "*", &[("data", event)])
        .await;
}
