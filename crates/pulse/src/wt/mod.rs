pub mod call;

use dashmap::DashMap;
use lazy_static::lazy_static;
use pulse_api::{
    AvailableTrack, MediaHint, NodeEvent, NodeEventKind, SessionData, WtMessageC2S, WtMessageS2C,
    WtTrackData,
};
use redis::AsyncCommands;
use tokio::{task, time};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};
use tokio::time::Instant;
use ulid::Ulid;
use wtransport::endpoint::endpoint_side::Server;
use wtransport::{Endpoint, ServerConfig};

use crate::redis::INSTANCE_ID;
use crate::wt::call::{Call, MlsState, PendingProposal};

#[derive(Clone, Debug)]
pub struct SessionInner {
    // TODO:?
    pub can_listen: bool,
    pub can_speak: bool,
    pub can_video: bool,
    pub can_screen: bool,
    pub producers: HashMap<String, TrackInfo>, // track_id -> TrackInfo
    pub last_activity: Instant,
}

#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub id: String, // global unique track ID
    #[allow(dead_code)]
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
    pub session_data: Arc<RwLock<SessionInner>>,
    pub connection: Arc<wtransport::Connection>,
    pub message_tx: mpsc::UnboundedSender<WtMessageS2C>,
}

impl SessionState {
    async fn update_activity(&self) {
        self.session_data.write().await.last_activity = Instant::now();
    }
}

lazy_static! {
    pub static ref GLOBAL_CALLS: Arc<DashMap<String, Call>> = Arc::new(DashMap::new());
    pub static ref GLOBAL_SESSIONS: Arc<DashMap<String, SessionState>> = Arc::new(DashMap::new());
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
        let incoming = server.accept().await;
        let session_request = incoming.await?;

        tokio::spawn(async move {
            if let Err(e) = handle_session(session_request).await {
                error!("Session error: {:?}", e);
            }
        });
    }
}

async fn handle_session(
    session_request: wtransport::endpoint::SessionRequest,
) -> anyhow::Result<()> {
    let session = session_request.accept().await?;
    info!("New WT session from {}", session.remote_address());

    let session_id = ulid::Ulid::new().to_string();
    let connection = Arc::new(session);

    let message_pair = mpsc::unbounded_channel::<WtMessageS2C>();

    let (mut send, mut recv) = connection.accept_bi().await?;

    let result =
        handle_session_loop(&connection, &mut send, &mut recv, &session_id, message_pair).await;

    info!("Cleaning up session {}", session_id);

    if let Some((_, state)) = GLOBAL_SESSIONS.remove(&session_id) {
        let session = state.session_data.read().await;

        if let Some(call) = GLOBAL_CALLS.get(&state.call_id) {
            call.remove_member(&session_id).await;
            call.stop_consuming_all(&session_id);
            let producer_global_ids: Vec<String> =
                session.producers.values().map(|t| t.id.clone()).collect();
            for global_id in producer_global_ids {
                call.stop_producing(&state.id, &global_id);
            }
            
            broadcast_proposals(&call).await;
        }
        let mut redis_conn = crate::redis::get_connection().await;
        let event = NodeEvent {
            id: session_id.to_string(),
            event: NodeEventKind::UserDisconnect {
                id: state.session_id.clone(),
                call_id: state.call_id.clone(),
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
                let _ = session
                    .message_tx
                    .send(WtMessageS2C::MlsProposals { proposals: proposals.clone() });
                
            }
        }
        let state = call.mls_state.clone();
        let id = call.id.clone();
        task::spawn(async move {
            time::sleep(Duration::from_secs(10)).await;
            // if no commits received after 10 seconds,
            // then destroy the call
            let state = state.lock().await;
            if state.pending_commit.is_some()  && state.current_epoch == epoch {
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

    loop {
        let timeout_duration = if let Some(session) = GLOBAL_SESSIONS.get(session_id) {
            let time_since_activity = session.session_data.read().await.last_activity.elapsed();
            if time_since_activity > Duration::from_secs(60) {
                warn!("Session {} timed out due to inactivity", session.id);
                send_message(send, WtMessageS2C::Disconnected { reconnect: None }).await?;
                return Ok(());
            }
            Duration::from_secs(60) - time_since_activity
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
                        if let Some(session) = GLOBAL_SESSIONS.get(session_id) {
                            session.update_activity().await;
                            handle_datagram(&dg.payload()[..], &session).await?;
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
                        if let Some(session) = GLOBAL_SESSIONS.get(session_id) {
                            session.update_activity().await;
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

async fn handle_datagram(payload: &[u8], session: &SessionState) -> anyhow::Result<()> {
    let message: WtTrackData = match rkyv::api::high::from_bytes::<_, rkyv::rancor::Error>(payload)
    {
        Ok(msg) => msg,
        Err(e) => {
            warn!("Failed to deserialize track data: {:?}", e);
            return Ok(());
        }
    };
    let Some(call) = GLOBAL_CALLS.get(&session.call_id) else {
        warn!(
            "Call {} not found for session {}",
            session.call_id, session.id
        );
        return Ok(());
    };
    let Some(track_id) = call.get_mapped_track_id(&message.id, &session.id) else {
        warn!(
            "Received data for track {} not produced by this session",
            message.id
        );
        return Ok(());
    };

    let session_inner = session.session_data.read().await;
    let track_info = session_inner.producers.get(&track_id).unwrap();
    if matches!(track_info.media_hint, MediaHint::Audio) {
        if !session_inner.can_speak {
            // drop muted audio packets
            return Ok(());
        }
    }

    call.dispatch(&track_id, &message.data).await;

    drop(session_inner);

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
    let Some(state) = GLOBAL_SESSIONS.get(session_id) else {
        let WtMessageC2S::Connect { session_token, key_package } = message else {
            warn!("Received message before authentication");
            return Ok(());
        };
        handle_connect(session_token, key_package, send, session_id, connection, message_tx).await?;
        return Ok(());
    };
    let state = state.value();
    match message {
        WtMessageC2S::Disconnect {} => {
            handle_disconnect(send, connection).await?;
            return Err(anyhow::anyhow!("Client disconnected"));
        }
        WtMessageC2S::StartProduce { id, media_hint } => {
            handle_start_produce(id, media_hint, send, state).await?;
        }
        WtMessageC2S::StopProduce { id } => {
            handle_stop_produce(id, send, state).await?;
        }
        WtMessageC2S::StartConsume { id } => {
            handle_start_consume(id, send, state).await?;
        }
        WtMessageC2S::StopConsume { id } => {
            handle_stop_consume(id, send, state).await?;
        }
        WtMessageC2S::Heartbeat {} => {
            handle_heartbeat(send, state).await?;
        }
        WtMessageC2S::MlsCommit { commit_data, epoch, welcome_data } => {
            handle_mls_commit(commit_data, epoch, welcome_data, send, state).await?;
        }
        WtMessageC2S::CommitAck { epoch } => {
            handle_commit_ack(epoch, state).await?;
        }
        _ => {
            warn!("Unhandled message type");
        }
    }

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

    let session_data: Option<String> = redis_conn
        .get(&format!("session:{}", session_token))
        .await?;

    let session_data = match session_data {
        Some(data) => {
            let parsed: SessionData = pulse_api::deserialize(data.as_bytes())
                .map_err(|e| anyhow::anyhow!("Failed to parse session data: {:?}", e))?;
            parsed
        }
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

    let old_session = GLOBAL_SESSIONS
        .iter()
        .find(|entry| entry.value().session_token == session_token)
        .map(|entry| entry.value().clone());

    if let Some(old_session) = old_session {
        let _ = old_session
            .message_tx
            .send(WtMessageS2C::Disconnected { reconnect: None });
        old_session
            .connection
            .close(0u32.into(), b"Session replaced by reconnection");
        // IMPORTANT: we remove the session here
        // so that we don't try to destroy the session later
        GLOBAL_SESSIONS.remove(&old_session.id);
    }

    let state = SessionState {
        id: session_id.to_string(),
        session_id: session_data.session_id.clone(),
        call_id: session_data.call_id.clone(),
        session_token: session_token.clone(),
        session_data: Arc::new(RwLock::new(SessionInner {
            can_listen: session_data.can_listen,
            can_speak: session_data.can_speak,
            can_video: session_data.can_video,
            can_screen: session_data.can_screen,
            producers: HashMap::new(), // TODO: copy producers from previous session, if any
            last_activity: Instant::now(),
        })),
        connection: Arc::new(connection.clone()),
        message_tx: message_tx.clone(),
    };
    GLOBAL_SESSIONS.insert(state.id.clone(), state.clone());

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
    
    call.add_member(state.id.clone(), key_package).await;
    broadcast_proposals(&call).await;

    let available_tracks: Vec<AvailableTrack> = GLOBAL_CALLS
        .get(&session_data.call_id)
        .map_or(Vec::new(), |call| {
            call.value().get_available_tracks(&state.id)
        });

    send_message(
        send,
        WtMessageS2C::Connected {
            id: state.id.clone(),
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
        
        info!("Sent InitializeGroup to first member in call {}", session_data.call_id);
    }

    let mut redis_conn = crate::redis::get_connection().await;
    let event = NodeEvent {
        id: state.id.clone(),
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
    if state.session_data.read().await.can_listen {
        warn!("Cannot consume track while deafened");
        return Ok(());
    }
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };
    call.start_consuming(&state.id, &track_id);

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
    call.stop_consuming(&state.id, &track_id);

    send_message(send, WtMessageS2C::ConsumeStopped { id: track_id }).await?;

    Ok(())
}

async fn handle_start_produce(
    track_id: String,
    media_hint: MediaHint,
    send: &mut wtransport::stream::SendStream,
    state: &SessionState,
) -> anyhow::Result<()> {
    let session_data = state.session_data.read().await;
    let allowed = match media_hint {
        MediaHint::Audio => session_data.can_speak,
        MediaHint::Video => session_data.can_video,
        MediaHint::ScreenAudio | MediaHint::ScreenVideo => session_data.can_screen,
    };

    if !allowed {
        warn!("User does not have permission to produce {:?}", media_hint);
        return Ok(());
    }

    for track in state.session_data.read().await.producers.values() {
        if std::mem::discriminant(&track.media_hint) == std::mem::discriminant(&media_hint) {
            warn!("Already producing track of type {:?}", media_hint);
            return Ok(());
        }
    }

    let current_session_id = state.id.clone();

    let global_track_id = Ulid::new().to_string();

    let track_info = TrackInfo {
        id: global_track_id.clone(),
        client_track_id: track_id.clone(),
        media_hint: media_hint.clone(),
        session_id: current_session_id.clone(),
        producer_session: state.clone(),
    };

    state
        .session_data
        .write()
        .await
        .producers
        .insert(track_id.clone(), track_info.clone());

    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };
    call.start_producing(&state.id, track_info).await;

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
    let global_track_id = call.get_mapped_track_id(&track_id, &state.id);
    let Some(global_track_id) = global_track_id else {
        warn!("Track {} not found for session {}", track_id, state.id);
        return Ok(());
    };
    call.stop_producing(&state.id, &global_track_id);
    let mut session = state.session_data.write().await;
    session.producers.remove(&track_id);

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
        warn!("Received commit for epoch {}, but current epoch is {}", epoch, mls_state.current_epoch);
        return Ok(());
    }
    let new_members = pending_commit.proposals.iter().filter_map(|p| {
        if let PendingProposal::Add { session_id, .. } = p {
            Some(session_id.clone())
        } else {
            None
        }
    }).collect::<Vec<_>>();

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
    info!("Forwarded MLS commit to all members of call {}", state.call_id);
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
                        let _ = session.message_tx.send(WtMessageS2C::EpochReady { epoch: new_epoch });
                    }
                }
                info!("Advanced to epoch {} for call {}", new_epoch, call_id);
            } else {
                info!("Epoch already advanced for call {}, current epoch is {}", call_id, call.mls_state.lock().await.current_epoch);
                return;
            }
        }
    });
    Ok(())
}

async fn handle_commit_ack(
    epoch: u64,
    state: &SessionState,
) -> anyhow::Result<()> {
    let Some(call) = GLOBAL_CALLS.get(&state.call_id) else {
        warn!("Call {} not found for session {}", state.call_id, state.id);
        return Ok(());
    };
    
    let all_acked = call.record_commit_ack(&state.id, epoch).await;
    
    if all_acked {
        // broadcast epoch ready if this was the last ack needed
        let new_epoch = call.increment_epoch().await;
        if let Some(new_epoch) = new_epoch {
            for recipient in call.mls_state.lock().await.full_members.iter() {
                if let Some(session) = GLOBAL_SESSIONS.get(recipient) {
                    let _ = session.message_tx.send(WtMessageS2C::EpochReady { epoch: new_epoch });
                } 
            }
        }
    }
    
    Ok(())
}

