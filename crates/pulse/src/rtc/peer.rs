use std::{
    collections::VecDeque,
    net::SocketAddr,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use async_std::{
    channel::{self, Receiver, Sender},
    future::timeout,
    net::UdpSocket,
    task,
};
use str0m::{
    change::{SdpAnswer, SdpOffer, SdpPendingOffer},
    channel::{ChannelData, ChannelId},
    media::{Direction, KeyframeRequest, KeyframeRequestKind, MediaData, MediaKind, Mid, Rid},
    Candidate, Event, IceConnectionState, Input, Output, Rtc,
};
use ulid::Ulid;

use crate::{
    errors::Error, redis::get_connection, socket::events::{MediaType, RemoteTrack}
};

use super::{
    call::{Call, CallEvent},
    udp::{poll_until_timeout, read_socket_input, Propagated},
};

#[derive(Debug)]
pub struct TrackIn {
    origin: String,
    mid: Mid,
    kind: MediaKind,
}

#[derive(Debug)]
pub struct TrackInEntry {
    pub id: Arc<TrackIn>,
    last_keyframe_request: Option<Instant>,
    pub active: bool,
}

#[derive(Debug)]
pub struct TrackOut {
    track_in: Weak<TrackIn>,
    state: TrackOutState,
}

#[derive(Debug)]
pub struct Track {
    track_in: Weak<TrackIn>,
    consumers: Vec<Weak<TrackOut>>,
    pub producer: String,
    pub id: String,
    media_type: MediaType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrackOutState {
    ToOpen,
    Negotiating(Mid),
    Open(Mid),
}

impl TrackOut {
    fn mid(&self) -> Option<Mid> {
        match self.state {
            TrackOutState::ToOpen => None,
            TrackOutState::Negotiating(m) | TrackOutState::Open(m) => Some(m),
        }
    }
}

pub enum ClientApiOut {
    RemoveUser(String),
    Answer(SdpAnswer),
    // NewTrack(String),
}
pub enum ClientApiIn {
    Offer(SdpOffer),
    NewTrack(String, MediaType),
    AddTrack(String),
    RemoveTrack(String),
    Destroy,
}

#[derive(Debug)]
pub struct ClientApi {
    pub send: Sender<ClientApiIn>,
    pub recv: Receiver<ClientApiOut>,
}

impl ClientApi {
    pub fn new(
        session_id: String,
        socket: UdpSocket,
        local_addr: SocketAddr,
        call: Arc<Call>,
    ) -> Self {
        let (in_send, in_recv) = channel::unbounded();
        let (out_send, out_recv) = channel::unbounded();
        task::spawn(async move {
            let mut client = Client::new(session_id, local_addr, call.clone());
            let mut to_propagate: VecDeque<Propagated> = VecDeque::new();
            let mut buf = vec![0; 2000];
            info!("Client {} started", client.session_id);
            let receiver = call.listener().await;
            let redis = get_connection().await;
            loop {
                if !client.rtc.is_alive() {
                    out_send
                        .send(ClientApiOut::RemoveUser(client.session_id.clone()))
                        .await?;
                    break;
                    // self.call.remove_user(&self.session_id);
                }

                let msg = in_recv.try_recv();
                match msg {
                    Ok(ClientApiIn::Offer(offer)) => {
                        out_send
                            .send(ClientApiOut::Answer(client.negotiate(offer)))
                            .await?;
                    }
                    Ok(ClientApiIn::Destroy) => {
                        client.close();
                        out_send
                            .send(ClientApiOut::RemoveUser(client.session_id.clone()))
                            .await?;
                        break;
                    }
                    Ok(ClientApiIn::NewTrack(track_id, media_type)) => {
                        // check if such track exists on rtc
                        let Some(track) = client
                            .tracks_in
                            .iter_mut()
                            .find(|t| t.id.mid.to_string() == track_id)
                        else {
                            warn!("WARNING: track not found");
                            // TODO: handle error
                            continue;
                        };
                        track.active = true;
                        let call_track = Track {
                            track_in: Arc::downgrade(&track.id.clone()),
                            consumers: vec![],
                            producer: client.session_id.clone(),
                            id: track_id.clone(),
                            media_type: media_type.clone(),
                        };
                        client
                            .call
                            .tracks
                            .insert(track_id.clone(), Arc::new(call_track));
                        debug!("New track: {:?}", track_id);
                        // FIXME:
                        call.publish(CallEvent::CreateTrack(RemoteTrack {
                            id: track_id.clone(),
                            user_id: client.session_id.clone(),
                            media_type,
                        }))
                        .await;
                    }
                    Ok(ClientApiIn::AddTrack(track_id)) => {
                        let Some(track_in) = call.tracks.iter().find(|t| t.id == track_id) else {
                            warn!("WARNING: track not found");
                            continue;
                        };
                        client.handle_track_open(track_in.track_in.clone());
                        drop(track_in);
                    }
                    Ok(ClientApiIn::RemoveTrack(track_id)) => {}
                    Err(e) => {
                        // error!("{}", e);
                    }
                }

                // Receive propagated events from other clients
                if let Ok(event) = receiver.try_recv() {
                    match event {
                        CallEvent::Propagate(propagated) => {
                            let Some(client_id) = propagated.client_id() else {
                                // If the event doesn't have a client id, it can't be propagated,
                                // (it's either a noop or a timeout).
                                continue;
                            };

                            if client.session_id == client_id {
                                // Do not propagate to originating client.
                                continue;
                            }

                            match propagated {
                                //TODO:more detailed description of track (producer id)
                                Propagated::MediaData(_, data) => {
                                    client.handle_media_data_out(client_id.clone(), &data)
                                }
                                Propagated::KeyframeRequest(_, req, origin, mid_in) => {
                                    // Only one origin client handles the keyframe request.
                                    if *origin == client.session_id {
                                        client.handle_keyframe_request(req, mid_in)
                                    }
                                }
                                Propagated::Noop | Propagated::Timeout(_) => {}
                            }
                        },
                        CallEvent::CreateTrack(_) => todo!(),
                        CallEvent::RemoveTrack { removed_tracks } => todo!(),
                        // CallEvent
                    }
                }

                // Poll for output from RTC library
                // Data to be sent out will be sent out
                poll_until_timeout(&mut client, &mut to_propagate, &socket).await;

                // What the RTC library tells us to propagate to other clients
                if let Some(p) = to_propagate.pop_front() {
                    call.publish(CallEvent::Propagate(p)).await;
                    continue;
                }

                if let Ok(Some(input)) =
                    timeout(Duration::from_secs(1), read_socket_input(&socket, &mut buf)).await
                {
                    client.handle_input(input);
                }

                // Drive time forward in all clients.
                let now = Instant::now();
                client.handle_input(Input::Timeout(now));
            }
            Ok::<(), Error>(())
        });
        Self {
            send: in_send,
            recv: out_recv,
        }
    }
}

#[derive(Debug)]
pub struct Client {
    pub session_id: String,
    pub rtc: Rtc,
    pub pending: Option<SdpPendingOffer>,
    pub cid: Option<ChannelId>,
    pub tracks_in: Vec<TrackInEntry>,
    pub tracks_out: Vec<TrackOut>,
    pub chosen_rid: Option<Rid>,
    pub local_addr: SocketAddr,
    pub call: Arc<Call>,
}

impl Client {
    pub fn new(session_id: String, local_addr: SocketAddr, call: Arc<Call>) -> Self {
        let rtc = Rtc::new();
        Self {
            session_id,
            rtc,
            pending: None,
            cid: None,
            tracks_in: vec![],
            tracks_out: vec![],
            chosen_rid: None,
            local_addr,
            call,
        }
    }

    pub fn negotiate(&mut self, offer: SdpOffer) -> SdpAnswer {
        let candidate = Candidate::host(self.local_addr, "udp").expect("host candidate");
        self.rtc.add_local_candidate(candidate);
        let answer = self
            .rtc
            .sdp_api()
            .accept_offer(offer)
            .expect("accept offer");
        answer
    }

    pub fn close(&mut self) {
        self.rtc.disconnect();
    }

    pub fn accepts(&self, input: &Input) -> bool {
        self.rtc.accepts(input)
    }

    pub fn handle_input(&mut self, input: Input) {
        if !self.rtc.is_alive() {
            return;
        }

        if let Err(e) = self.rtc.handle_input(input) {
            warn!("Client ({}) disconnected: {:?}", self.session_id, e);
            self.rtc.disconnect();
        }
    }

    pub async fn poll_output(&mut self, socket: &UdpSocket) -> Propagated {
        if !self.rtc.is_alive() {
            return Propagated::Noop;
        }

        // Incoming tracks from other clients cause new entries in track_out that
        // need SDP negotiation with the remote peer.
        if self.negotiate_if_needed() {
            return Propagated::Noop;
        }

        match self.rtc.poll_output() {
            Ok(output) => self.handle_output(output, socket).await,
            Err(e) => {
                warn!("Client ({}) poll_output failed: {:?}", self.session_id, e);
                self.rtc.disconnect();
                Propagated::Noop
            }
        }
    }

    async fn handle_output(&mut self, output: Output, socket: &UdpSocket) -> Propagated {
        match output {
            Output::Transmit(transmit) => {
                socket
                    .send_to(&transmit.contents, transmit.destination)
                    .await
                    .expect("sending UDP data");
                Propagated::Noop
            }
            Output::Timeout(t) => Propagated::Timeout(t),
            Output::Event(e) => match e {
                Event::IceConnectionStateChange(v) => {
                    if v == IceConnectionState::Disconnected {
                        // Ice disconnect could result in trying to establish a new connection,
                        // but this impl just disconnects directly.
                        self.rtc.disconnect();
                    }
                    Propagated::Noop
                }
                Event::MediaAdded(e) => self.handle_media_added(e.mid, e.kind),
                Event::MediaData(data) => self.handle_media_data_in(data),
                Event::KeyframeRequest(req) => self.handle_incoming_keyframe_req(req),
                Event::ChannelOpen(cid, _) => {
                    info!("Channel open: {:?}", cid);
                    self.cid = Some(cid);
                    Propagated::Noop
                }
                Event::ChannelData(data) => self.handle_channel_data(data),
                Event::StreamPaused(s) => {
                    // This is where we would pause the stream.
                    info!("Stream paused: {:?}", s);
                    // TODO: implement pausing of the stream (ignore for now)
                    // idea is that eventually we would terminate the stream immediately if the stream disconnects
                    // but for now:
                    // - when user leaves, terminate stream
                    // - or when user manually terminates stream
                    Propagated::Noop
                }

                // NB: To see statistics, uncomment set_stats_interval() above.
                Event::MediaIngressStats(data) => {
                    info!("{:?}", data);
                    Propagated::Noop
                }
                Event::MediaEgressStats(data) => {
                    info!("{:?}", data);
                    Propagated::Noop
                }
                Event::PeerStats(data) => {
                    info!("{:?}", data);
                    Propagated::Noop
                }
                _ => Propagated::Noop,
            },
        }
    }

    fn handle_media_added(&mut self, mid: Mid, kind: MediaKind) -> Propagated {
        // let track_id = Ulid::new().to_string();
        let track_in = TrackInEntry {
            id: Arc::new(TrackIn {
                origin: self.session_id.clone(),
                mid: mid.clone(),
                kind,
            }),
            last_keyframe_request: None,
            active: false,
        };

        // The Client instance owns the strong reference to the incoming
        // track, all other clients have a weak reference.
        // let weak = Arc::downgrade(&track_in.id);
        self.tracks_in.push(track_in);
        if let Some(mut channel) = self.cid.and_then(|id| self.rtc.channel(id)) {
            let json = rmp_serde::to_vec(&mid.to_string()).unwrap();
            channel
                .write(true, json.as_slice())
                .expect("to write answer");
        };

        Propagated::Noop
    }

    fn handle_media_data_in(&mut self, data: MediaData) -> Propagated {
        if !data.contiguous {
            self.request_keyframe_throttled(data.mid, data.rid, KeyframeRequestKind::Fir);
        }

        Propagated::MediaData(self.session_id.clone(), Arc::new(data))
    }

    fn request_keyframe_throttled(
        &mut self,
        mid: Mid,
        rid: Option<Rid>,
        kind: KeyframeRequestKind,
    ) {
        let Some(mut writer) = self.rtc.writer(mid) else {
            return;
        };

        let Some(track_entry) = self.tracks_in.iter_mut().find(|t| t.id.mid == mid) else {
            return;
        };

        if track_entry
            .last_keyframe_request
            .map(|t| t.elapsed() < Duration::from_secs(1))
            .unwrap_or(false)
        {
            return;
        }

        _ = writer.request_keyframe(rid, kind);

        track_entry.last_keyframe_request = Some(Instant::now());
    }

    fn handle_incoming_keyframe_req(&self, mut req: KeyframeRequest) -> Propagated {
        // Need to figure out the track_in mid that needs to handle the keyframe request.
        let Some(track_out) = self.tracks_out.iter().find(|t| t.mid() == Some(req.mid)) else {
            return Propagated::Noop;
        };
        let Some(track_in) = track_out.track_in.upgrade() else {
            return Propagated::Noop;
        };

        // This is the rid picked from incoming mediadata, and to which we need to
        // send the keyframe request.
        req.rid = self.chosen_rid;

        Propagated::KeyframeRequest(
            self.session_id.clone(),
            req,
            track_in.origin.clone(),
            track_in.mid,
        )
    }

    fn negotiate_if_needed(&mut self) -> bool {
        if self.cid.is_none() || self.pending.is_some() {
            debug!("no data channel!!!!!!!!!!!!!!!");
            // Don't negotiate if there is no data channel, or if we have pending changes already.
            return false;
        }

        let mut change = self.rtc.sdp_api();

        for track in &mut self.tracks_out {
            if let TrackOutState::ToOpen = track.state {
                if let Some(track_in) = track.track_in.upgrade() {
                    let stream_id = track_in.origin.to_string();
                    let mid =
                        change.add_media(track_in.kind, Direction::SendOnly, Some(stream_id), None);
                    track.state = TrackOutState::Negotiating(mid);
                }
            }
        }

        if !change.has_changes() {
            return false;
        }

        let Some((offer, pending)) = change.apply() else {
            return false;
        };

        let Some(mut channel) = self.cid.and_then(|id| self.rtc.channel(id)) else {
            return false;
        };

        let json = rmp_serde::to_vec_named(&offer).unwrap();
        channel
            .write(true, json.as_slice())
            .expect("to write answer");

        self.pending = Some(pending);

        true
    }

    fn handle_channel_data(&mut self, d: ChannelData) -> Propagated {
        if let Ok(offer) = rmp_serde::from_slice::<'_, SdpOffer>(&d.data) {
            self.handle_offer(offer);
        } else if let Ok(answer) = rmp_serde::from_slice::<'_, SdpAnswer>(&d.data) {
            self.handle_answer(answer);
        }
        println!("we got something");
        Propagated::Noop
    }

    fn handle_offer(&mut self, offer: SdpOffer) {
        let answer = self
            .rtc
            .sdp_api()
            .accept_offer(offer)
            .expect("offer to be accepted");

        // Keep local track state in sync, cancelling any pending negotiation
        // so we can redo it after this offer is handled.
        for track in &mut self.tracks_out {
            if let TrackOutState::Negotiating(_) = track.state {
                track.state = TrackOutState::ToOpen;
            }
        }

        let mut channel = self
            .cid
            .and_then(|id| self.rtc.channel(id))
            .expect("channel to be open");

        let json = rmp_serde::to_vec_named(&answer).unwrap();
        channel
            .write(true, json.as_slice())
            .expect("to write answer");
    }

    fn handle_answer(&mut self, answer: SdpAnswer) {
        if let Some(pending) = self.pending.take() {
            self.rtc
                .sdp_api()
                .accept_answer(pending, answer)
                .expect("answer to be accepted");

            for track in &mut self.tracks_out {
                if let TrackOutState::Negotiating(m) = track.state {
                    track.state = TrackOutState::Open(m);
                }
            }
        }
    }

    pub fn handle_track_open(&mut self, track_in: Weak<TrackIn>) {
        let track_out = TrackOut {
            track_in,
            state: TrackOutState::ToOpen,
        };
        self.tracks_out.push(track_out);
    }

    pub fn handle_media_data_out(&mut self, origin: String, data: &MediaData) {
        // Figure out which outgoing track maps to the incoming media data.
        let Some(mid) = self
            .tracks_out
            .iter()
            .find(|o| {
                o.track_in
                    .upgrade()
                    .filter(|i| i.origin == origin && i.mid == data.mid)
                    .is_some()
            })
            .and_then(|o| o.mid())
        else {
            return;
        };

        if data.rid.is_some() && data.rid != Some("h".into()) {
            // This is where we plug in a selection strategy for simulcast. For
            // now either let rid=None through (which would be no simulcast layers)
            // or "h" if we have simulcast (see commented out code in chat.html).
            return;
        }
        // TODO: option to disable leave sound
        // Remember this value for keyframe requests.
        if self.chosen_rid != data.rid {
            self.chosen_rid = data.rid;
        }

        let Some(writer) = self.rtc.writer(mid) else {
            return;
        };

        // Match outgoing pt to incoming codec.
        let Some(pt) = writer.match_params(data.params) else {
            return;
        };

        if let Err(e) = writer.write(pt, data.network_time, data.time, data.data.clone()) {
            warn!("Client ({}) failed: {:?}", self.session_id, e);
            self.rtc.disconnect();
        }
    }

    pub fn handle_keyframe_request(&mut self, req: KeyframeRequest, mid_in: Mid) {
        let has_incoming_track = self.tracks_in.iter().any(|i| i.id.mid == mid_in);

        // This will be the case for all other client but the one where the track originates.
        if !has_incoming_track {
            return;
        }

        let Some(mut writer) = self.rtc.writer(mid_in) else {
            return;
        };

        if let Err(e) = writer.request_keyframe(req.rid, req.kind) {
            // This can fail if the rid doesn't match any media.
            info!("request_keyframe failed: {:?}", e);
        }
    }
}
