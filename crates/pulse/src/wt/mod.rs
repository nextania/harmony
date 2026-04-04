pub mod call;

use common::{NodeEvent, NodeEventKind, SessionData};
use dashmap::DashMap;
use lazy_static::lazy_static;
use pulse_types::fragment::FragmentAssembler;
use pulse_types::{AvailableTrack, MediaHint, WtFragmentedTrackData, WtMessageC2S, WtMessageS2C};
use redis::AsyncCommands;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc};
use tokio::time::Instant;
use tokio::{task, time};
use ulid::Ulid;
use wtransport::endpoint::endpoint_side::Server;
use wtransport::{Endpoint, ServerConfig};

use crate::metrics::{CONNECTIONS_ACTIVE, FRAGMENT_ASSEMBLED, FRAGMENT_DROPPED};
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
    pub connection: Arc<wtransport::Connection>,
    pub message_tx: mpsc::UnboundedSender<WtMessageS2C>,
    pub last_activity: Arc<AtomicU64>,
    pub can_listen: Arc<AtomicBool>,
    pub can_speak: Arc<AtomicBool>,
    pub can_video: Arc<AtomicBool>,
    pub can_screen: Arc<AtomicBool>,
    pub producers: Arc<DashMap<String, TrackInfo>>, // track_id -> TrackInfo
    pub seq_counter: Arc<AtomicU32>,
}

impl SessionState {
    fn update_activity(&self) {
        self.last_activity.store(now(), Ordering::SeqCst);
    }
}

lazy_static! {
    pub static ref GLOBAL_CALLS: Arc<DashMap<String, Call>> = Arc::new(DashMap::new());
    pub static ref GLOBAL_SESSIONS: Arc<DashMap<String, SessionState>> = Arc::new(DashMap::new());
    pub static ref GLOBAL_UNIQUE_SESSIONS: Arc<DashMap<String, String>> = Arc::new(DashMap::new());
}

pub async fn listen() -> anyhow::Result<()> {
    // TODO: get proper certificate
    let identity = wtransport::Identity::load_pemfiles("cert.pem", "key.pem")
        .await
        .expect("Certificate files not found. Please provide cert.pem and key.pem");

    let config = ServerConfig::builder()
        .with_bind_default(4433)
        .with_identity(identity)
        .build();

    let server = Endpoint::<Server>::server(config)?;

    loop {
        let incoming = server.accept().await.await;
        match incoming {
            Ok(session_request) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_session(session_request).await {
                        error!("Session error: {:?}", e);
                    }
                });
            }
            Err(e) => {
                error!("Failed to accept incoming connection: {:?}", e);
                continue;
            }
        };
    }
}

async fn handle_session(
    session_request: wtransport::endpoint::SessionRequest,
) -> anyhow::Result<()> {
    let session = session_request.accept().await?;
    info!("New WT session from {}", session.remote_address());
    CONNECTIONS_ACTIVE.add(1, &[]);

    let unique_id = ulid::Ulid::new().to_string();
    let connection = Arc::new(session);

    let message_pair = mpsc::unbounded_channel::<WtMessageS2C>();

    let (mut send, mut recv) = connection.accept_bi().await?;

    let result =
        handle_session_loop(&connection, &mut send, &mut recv, &unique_id, message_pair).await;

    info!("Cleaning up session {}", unique_id);
    CONNECTIONS_ACTIVE.add(-1, &[]);

    let Some(id) = GLOBAL_UNIQUE_SESSIONS.remove(&unique_id).map(|(_, id)| id) else {
        warn!("Session ID not found for unique ID {}", unique_id);
        return result;
    };
    if let Some((_, state)) = GLOBAL_SESSIONS.remove(&id) {
        let state_data = state.clone();
        drop(state);
        GLOBAL_SESSIONS.remove(&state_data.session_id);

        if let Some(call) = GLOBAL_CALLS.get(&state_data.call_id) {
            call.remove_member(&state_data.session_id).await;
            call.stop_consuming_all(&state_data.session_id);
            let producer_global_ids: Vec<String> =
                state_data.producers.iter().map(|t| t.id.clone()).collect();
            for global_id in producer_global_ids {
                call.stop_producing(&state_data.session_id, &global_id);
            }

            broadcast_proposals(&call).await;
        }
        let mut redis_conn = crate::redis::get_connection().await;
        let event = NodeEvent {
            id: INSTANCE_ID.to_string(),
            event: NodeEventKind::UserDisconnect {
                id: state_data.session_id.clone(),
                call_id: state_data.call_id.clone(),
            },
        };
        let _: Result<(), redis::RedisError> = redis_conn
            .xadd::<_, _, _, _, ()>("voice:events:user-lifecycle", "*", &[("data", event)])
            .await;
    }

    result
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
            // if no commits received after 10 seconds,
            // then destroy the call
            // TODO: decide whether we should resend
            let state = state.lock().await;
            if state.pending_commit.is_some() && state.current_epoch == epoch {
                GLOBAL_CALLS.remove(&id);
                info!("Destroyed call {} due to inactivity", id);
            }
        });
    }
}

async fn handle_session_loop(
    connection: &wtransport::Connection,
    send: &mut wtransport::stream::SendStream,
    recv: &mut wtransport::stream::RecvStream,
    session_id: &str,
    (message_tx, mut message_rx): (
        mpsc::UnboundedSender<WtMessageS2C>,
        mpsc::UnboundedReceiver<WtMessageS2C>,
    ),
) -> anyhow::Result<()> {
    let mut bytes = vec![0u8; 65536];
    let mut buffer = Vec::new();
    let connected = Instant::now();
    let mut assembler = FragmentAssembler::new(Duration::from_secs(1));

    loop {
        let current_session = GLOBAL_UNIQUE_SESSIONS
            .get(session_id)
            .map(|s| s.value().clone())
            .and_then(|id| GLOBAL_SESSIONS.get(&id).map(|s| s.clone()));

        let timeout_duration = if let Some(ref session) = current_session {
            let time_since_activity = now() - session.last_activity.load(Ordering::SeqCst);
            if time_since_activity > 60 {
                warn!("Session {} timed out due to inactivity", session.id);
                send_message(send, WtMessageS2C::Disconnected { reconnect: None }).await?;
                return Ok(());
            }
            Duration::from_secs(60 - time_since_activity)
        } else {
            if connected.elapsed() > Duration::from_secs(30) {
                warn!("Session timed out waiting for authentication");
                // disconnect
                return Ok(());
            }
            Duration::from_secs(30) - connected.elapsed()
        };

        tokio::select! {
            dg_result = connection.receive_datagram() => {
                match dg_result {
                    Ok(dg) => {
                        if let Some(ref session) = current_session {
                            session.update_activity();
                            let fragment: WtFragmentedTrackData = match rkyv::api::high::from_bytes::<_, rkyv::rancor::Error>(&dg.payload()[..]) {
                                Ok(f) => f,
                                Err(e) => {
                                    warn!("Failed to deserialize fragment: {:?}", e);
                                    FRAGMENT_DROPPED.add(1, &[]);
                                    continue;
                                }
                            };
                            if let Some(reassembled) = assembler.insert(fragment) {
                                FRAGMENT_ASSEMBLED.add(1, &[]);
                                handle_datagram(&reassembled.id, &reassembled.data, &session).await?;
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Error receiving datagram: {:?}", e);
                    }
                }
            }

            read_result = recv.read(&mut bytes) => {
                match read_result {
                    Ok(Some(len)) => {
                        if let Some(ref session) = current_session {
                            session.update_activity();
                        }
                        buffer.extend_from_slice(&bytes[..len]);

                        while let Some((message, consumed)) = try_parse_message(&buffer)? {
                            handle_message(
                                message,
                                send,
                                session_id,
                                connection,
                                message_tx.clone(),
                            ).await?;
                            buffer.drain(..consumed);
                        }
                    }
                    Ok(None) => {
                        info!("Stream closed by client");
                        return Ok(());
                    }
                    Err(e) => {
                        error!("Error reading from stream: {:?}", e);
                        return Err(anyhow::anyhow!("Read error: {:?}", e));
                    }
                }
            }

            Some(message) = message_rx.recv() => {
                if let Err(e) = send_message(send, message).await {
                    error!("Failed to send notification: {:?}", e);
                    return Err(e);
                }
            }

            _ = tokio::time::sleep(timeout_duration) => {
                continue;
            }
        }
    }
}

async fn handle_datagram(
    client_track_id: &str,
    data: &[u8],
    session: &SessionState,
) -> anyhow::Result<()> {
    let Some(call) = GLOBAL_CALLS.get(&session.call_id) else {
        warn!(
            "Call {} not found for session {}",
            session.call_id, session.session_id
        );
        return Ok(());
    };
    let Some(track_id) = call.get_mapped_track_id(client_track_id, &session.session_id) else {
        warn!(
            "Received data for track {} not produced by this session",
            client_track_id
        );
        return Ok(());
    };
    if data.is_empty() {
        warn!("Received empty datagram for track {}", track_id);
        return Ok(());
    }

    let track_info = session.producers.get(&track_id).unwrap();
    if matches!(track_info.media_hint, MediaHint::Audio)
        && !session.can_speak.load(Ordering::SeqCst)
    {
        // drop muted audio packets
        return Ok(());
    }

    call.dispatch(&track_id, data).await;

    Ok(())
}

fn try_parse_message(buffer: &[u8]) -> anyhow::Result<Option<(WtMessageC2S, usize)>> {
    // message is length-prefixed with u32 BE (4 bytes)
    if buffer.len() < 4 {
        return Ok(None);
    }
    let len = u32::from_be_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
    if buffer.len() < 4 + len {
        return Ok(None);
    }
    let message: WtMessageC2S =
        match rkyv::api::high::from_bytes::<_, rkyv::rancor::Error>(&buffer[4..4 + len]) {
            Ok(msg) => msg,
            Err(e) => return Err(anyhow::anyhow!("Failed to deserialize message: {:?}", e)),
        };

    Ok(Some((message, 4 + len)))
}

async fn handle_message(
    message: WtMessageC2S,
    send: &mut wtransport::stream::SendStream,
    session_id: &str,
    connection: &wtransport::Connection,
    message_tx: mpsc::UnboundedSender<WtMessageS2C>,
) -> anyhow::Result<()> {
    dbg!(&message);
    if let Some(id) = GLOBAL_UNIQUE_SESSIONS
        .get(session_id)
        .map(|s| s.value().clone())
        && let Some(state) = GLOBAL_SESSIONS.get(&id)
    {
        match message {
            WtMessageC2S::Disconnect {} => {
                handle_disconnect(send, connection).await?;
                return Err(anyhow::anyhow!("Client disconnected"));
            }
            WtMessageC2S::StartProduce { id, media_hint } => {
                handle_start_produce(id, media_hint, send, &state).await?;
            }
            WtMessageC2S::StopProduce { id } => {
                handle_stop_produce(id, send, &state).await?;
            }
            WtMessageC2S::StartConsume { id } => {
                handle_start_consume(id, send, &state).await?;
            }
            WtMessageC2S::StopConsume { id } => {
                handle_stop_consume(id, send, &state).await?;
            }
            WtMessageC2S::Heartbeat {} => {
                handle_heartbeat(send, &state).await?;
            }
            WtMessageC2S::MlsCommit {
                commit_data,
                epoch,
                welcome_data,
            } => {
                handle_mls_commit(commit_data, epoch, welcome_data, send, &state).await?;
            }
            WtMessageC2S::CommitAck { epoch } => {
                handle_commit_ack(epoch, &state).await?;
            }
            _ => {
                warn!("Unhandled message type");
            }
        }
    } else {
        let WtMessageC2S::Connect {
            session_token,
            key_package,
        } = message
        else {
            warn!("Received message before authentication");
            return Ok(());
        };
        handle_connect(
            session_token,
            key_package,
            send,
            session_id,
            connection,
            message_tx,
        )
        .await?;
        return Ok(());
    };

    Ok(())
}

async fn handle_connect(
    session_token: String,
    key_package: Vec<u8>,
    send: &mut wtransport::stream::SendStream,
    session_id: &str,
    connection: &wtransport::Connection,
    message_tx: mpsc::UnboundedSender<WtMessageS2C>,
) -> anyhow::Result<()> {
    let mut redis_conn = crate::redis::get_connection().await;

    let session_data: Option<SessionData> =
        redis_conn.get(format!("session:{}", session_token)).await?;

    let session_data = match session_data {
        Some(data) => data,
        None => {
            warn!("Invalid session token: {}", session_token);
            // disconnect
            return Err(anyhow::anyhow!("Invalid session token"));
        }
    };

    if session_data.assigned_server != *INSTANCE_ID {
        warn!(
            "Session token assigned to different server: {}",
            session_data.assigned_server
        );
        return Err(anyhow::anyhow!(
            "Session token assigned to different server"
        ));
    }

    // IMPORTANT: we remove the session here
    // so that we don't try to destroy the session later
    let old_session = GLOBAL_SESSIONS
        .remove(&session_data.session_id)
        .map(|(_, s)| s);

    if let Some(old_session) = old_session {
        GLOBAL_UNIQUE_SESSIONS.remove(&old_session.id);
        let _ = old_session
            .message_tx
            .send(WtMessageS2C::Disconnected { reconnect: None });
        old_session
            .connection
            .close(0u32.into(), b"Session replaced by reconnection");
    }

    let state = SessionState {
        id: session_id.to_string(),
        session_id: session_data.session_id.clone(),
        call_id: session_data.call_id.clone(),
        session_token: session_token.clone(),
        can_listen: Arc::new(AtomicBool::new(session_data.can_listen)),
        can_speak: Arc::new(AtomicBool::new(session_data.can_speak)),
        can_video: Arc::new(AtomicBool::new(session_data.can_video)),
        can_screen: Arc::new(AtomicBool::new(session_data.can_screen)),
        producers: Arc::new(DashMap::new()), // TODO: copy producers from previous session, if any
        connection: Arc::new(connection.clone()),
        message_tx: message_tx.clone(),
        last_activity: Arc::new(AtomicU64::new(now())),
        seq_counter: Arc::new(AtomicU32::new(0)),
    };
    GLOBAL_SESSIONS.insert(state.session_id.clone(), state.clone());
    GLOBAL_UNIQUE_SESSIONS.insert(state.id.clone(), state.session_id.clone());

    let call = GLOBAL_CALLS
        .entry(session_data.call_id.clone())
        .or_insert_with(|| Call {
            id: session_data.call_id.clone(),
            tracks: DashMap::new(),
            consumers: DashMap::new(),
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

    let available_tracks: Vec<AvailableTrack> = call.get_available_tracks(&state.session_id);

    send_message(
        send,
        WtMessageS2C::Connected {
            id: state.session_id.clone(),
            available_tracks,
        },
    )
    .await?;

    // if this is the first member, send external sender credential for group initialization
    // TODO: initialize external sender keys per call
    if is_first_member {
        let external_sender_credential = crate::environment::EXTERNAL_SENDER
            .serialize_credential()
            .unwrap_or_default();
        let external_sender_signature_key = crate::environment::EXTERNAL_SENDER
            .signature_public_key()
            .clone();

        send_message(
            send,
            WtMessageS2C::InitializeGroup {
                external_sender_credential,
                external_sender_signature_key,
            },
        )
        .await?;

        info!(
            "Sent InitializeGroup to first member in call {}",
            session_data.call_id
        );
    }

    let mut redis_conn = crate::redis::get_connection().await;
    let event = NodeEvent {
        id: INSTANCE_ID.to_string(),
        event: NodeEventKind::UserConnect {
            // Note: session_id here refers to the instance of the user,
            // as opposed to the specific connection
            id: session_data.session_id.clone(),
            call_id: session_data.call_id.clone(),
        },
    };
    let _: Result<(), redis::RedisError> = redis_conn
        .xadd::<_, _, _, _, ()>("voice:events:user-lifecycle", "*", &[("data", event)])
        .await;

    redis_conn
        .expire::<_, ()>(&format!("session:{}", session_token), 60)
        .await?;

    info!(
        "Session {} authenticated for user {}",
        state.id, session_data.session_id
    );

    Ok(())
}

async fn handle_start_consume(
    track_id: String,
    send: &mut wtransport::stream::SendStream,
    state: &SessionState,
) -> anyhow::Result<()> {
    if !state.can_listen.load(Ordering::SeqCst) {
        warn!("Cannot consume track while deafened");
        return Ok(());
    }
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };
    call.start_consuming(&state.session_id, &track_id);
    drop(call);

    send_message(send, WtMessageS2C::ConsumeStarted { id: track_id }).await?;

    Ok(())
}

async fn handle_stop_consume(
    track_id: String,
    send: &mut wtransport::stream::SendStream,
    state: &SessionState,
) -> anyhow::Result<()> {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };
    call.stop_consuming(&state.session_id, &track_id);
    drop(call);

    send_message(send, WtMessageS2C::ConsumeStopped { id: track_id }).await?;

    Ok(())
}

async fn handle_start_produce(
    track_id: String,
    media_hint: MediaHint,
    send: &mut wtransport::stream::SendStream,
    state: &SessionState,
) -> anyhow::Result<()> {
    let allowed = match media_hint {
        MediaHint::Audio => state.can_speak.load(Ordering::SeqCst),
        MediaHint::Video => state.can_video.load(Ordering::SeqCst),
        MediaHint::ScreenAudio | MediaHint::ScreenVideo => state.can_screen.load(Ordering::SeqCst),
    };

    if !allowed {
        warn!("User does not have permission to produce {:?}", media_hint);
        return Ok(());
    }

    for track in state.producers.iter() {
        if std::mem::discriminant(&track.media_hint) == std::mem::discriminant(&media_hint) {
            warn!("Already producing track of type {:?}", media_hint);
            return Ok(());
        }
    }

    let current_session_id = state.session_id.clone();

    let global_track_id = Ulid::new().to_string();

    let track_info = TrackInfo {
        id: global_track_id.clone(),
        client_track_id: track_id.clone(),
        media_hint: media_hint.clone(),
        session_id: current_session_id.clone(),
        producer_session: state.clone(),
    };

    state
        .producers
        .insert(global_track_id.clone(), track_info.clone());

    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };
    call.start_producing(&state.session_id, track_info).await;
    drop(call);

    send_message(
        send,
        WtMessageS2C::ProduceStarted {
            id: track_id.clone(),
        },
    )
    .await?;

    Ok(())
}

async fn handle_stop_produce(
    track_id: String,
    send: &mut wtransport::stream::SendStream,
    state: &SessionState,
) -> anyhow::Result<()> {
    let call = match GLOBAL_CALLS.get(&state.call_id) {
        Some(c) => c,
        None => {
            warn!("Call {} not found for session {}", state.call_id, state.id);
            return Ok(());
        }
    };
    let global_track_id = call.get_mapped_track_id(&track_id, &state.session_id);
    let Some(global_track_id) = global_track_id else {
        warn!("Track {} not found for session {}", track_id, state.id);
        return Ok(());
    };
    call.stop_producing(&state.session_id, &global_track_id);
    drop(call);
    state.producers.remove(&global_track_id);
    send_message(
        send,
        WtMessageS2C::ProduceStopped {
            id: track_id.clone(),
        },
    )
    .await?;

    Ok(())
}

async fn handle_disconnect(
    send: &mut wtransport::stream::SendStream,
    connection: &wtransport::Connection,
) -> anyhow::Result<()> {
    send_message(send, WtMessageS2C::Disconnected { reconnect: None }).await?;
    connection.close(0u32.into(), b"Client disconnected");

    Ok(())
}

async fn handle_heartbeat(
    send: &mut wtransport::stream::SendStream,
    state: &SessionState,
) -> anyhow::Result<()> {
    let session_token = state.session_token.clone();
    let mut redis_conn = crate::redis::get_connection().await;
    if let Err(e) = redis_conn
        .expire::<_, ()>(&format!("session:{}", session_token), 60)
        .await
    {
        warn!("Failed to update session TTL: {:?}", e);
    }

    send_message(send, WtMessageS2C::Heartbeat {}).await?;

    Ok(())
}

async fn send_message(
    send: &mut wtransport::stream::SendStream,
    message: WtMessageS2C,
) -> anyhow::Result<()> {
    let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&message)
        .map_err(|e| anyhow::anyhow!("Failed to serialize message: {:?}", e))?;
    let len = bytes.len() as u32;
    send.write_all(&len.to_be_bytes()).await?;
    send.write_all(&bytes).await?;

    Ok(())
}

async fn handle_mls_commit(
    commit_data: Vec<u8>,
    epoch: u64,
    welcome_data: Option<Vec<u8>>,
    _send: &mut wtransport::stream::SendStream,
    state: &SessionState,
) -> anyhow::Result<()> {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };

    // only choose the first commit for a given epoch even though everyone should be sending commits
    let mut mls_state = call.mls_state.lock().await;
    let Some(pending_commit) = mls_state.pending_commit.take() else {
        // already claimed by another commit, ignore
        return Ok(());
    };
    if mls_state.current_epoch != epoch {
        warn!(
            "Received commit for epoch {}, but current epoch is {}",
            epoch, mls_state.current_epoch
        );
        return Ok(());
    }
    let new_members = pending_commit
        .proposals
        .iter()
        .filter_map(|p| {
            if let PendingProposal::Add { session_id, .. } = p {
                Some(session_id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    // broadcast commit to all members (clients should ONLY apply the commit broadcast
    // by the server, since the chosen one is not necessarily the client's own commit)
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
        // add new member to full members
        mls_state.full_members.push(new_member.clone());
    }
    mls_state.pending_epoch_change = true;
    info!(
        "Forwarded MLS commit to all members of call {}",
        state.call_id
    );
    // when all members have acknowledged OR when task times out, increment epoch and broadcast epoch ready
    let call_id = state.call_id.clone();
    task::spawn(async move {
        let ack_timeout = Duration::from_secs(10);
        let start = Instant::now();
        time::sleep_until(start + ack_timeout).await;

        // advance epoch
        if let Some(call) = GLOBAL_CALLS.get(&call_id) {
            let new_epoch = call.increment_epoch().await;
            if let Some(new_epoch) = new_epoch {
                // broadcast epoch ready
                for recipient in call.mls_state.lock().await.full_members.iter() {
                    if let Some(session) = GLOBAL_SESSIONS.get(recipient) {
                        let _ = session
                            .message_tx
                            .send(WtMessageS2C::EpochReady { epoch: new_epoch });
                    }
                }
                info!("Advanced to epoch {} for call {}", new_epoch, call_id);
            } else {
                info!(
                    "Epoch already advanced for call {}, current epoch is {}",
                    call_id,
                    call.mls_state.lock().await.current_epoch
                );
            }
        }
    });
    Ok(())
}

async fn handle_commit_ack(epoch: u64, state: &SessionState) -> anyhow::Result<()> {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };
    // TODO: look into this
    let all_acked = call.record_commit_ack(&state.session_id, epoch).await;

    if all_acked {
        // broadcast epoch ready if this was the last ack needed
        let new_epoch = call.increment_epoch().await;
        if let Some(new_epoch) = new_epoch {
            for recipient in call.mls_state.lock().await.full_members.iter() {
                if let Some(session) = GLOBAL_SESSIONS.get(recipient) {
                    let _ = session
                        .message_tx
                        .send(WtMessageS2C::EpochReady { epoch: new_epoch });
                }
            }
        }
    }

    Ok(())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}
