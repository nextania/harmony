use std::{
    collections::HashMap,
    sync::{Arc, mpsc},
};

use arc_swap::ArcSwap;
use async_stream::stream;
use iced::{Task, advanced::image::Handle as ImageHandle};
use pulse_api::{
    AvailableTrack, MediaHint, PulseClient, PulseClientOptions, PulseEvent, TrackHandle,
};
use wgpu_capture::CaptureTarget;

use harmony_api::CallMember;

use crate::{
    Message,
    api::ApiClient,
    errors::{RenderableError, RenderableResult},
    media::screen_capture::{ScreenCaptureConfig, ScreenCaptureSession},
    media::video::{self, Frame as VideoFrame},
    media::{audio::AudioPipeline, codec},
    views::main::{MainMessage, fetch_profiles_task},
    widgets::remote_screen::RemoteScreenFrame,
};

#[derive(Debug, Clone)]
pub struct CallTrackState {
    pub audio: bool,
    pub video: bool,
    pub screen: bool,
}

#[derive(Debug, Clone)]
pub struct CallParticipant {
    pub user_id: String,
    pub session_id: String,
    pub tracks: CallTrackState,
}

impl From<CallMember> for CallParticipant {
    fn from(m: CallMember) -> Self {
        CallParticipant {
            user_id: m.user_id,
            session_id: m.session_id,
            tracks: CallTrackState {
                audio: !m.muted,
                video: false,
                screen: false,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct CallState {
    pub participants: Vec<CallParticipant>,
}

#[derive(Clone)]
pub enum CallMessage {
    Join,
    Start,
    Leave,
    ToggleMic,
    ToggleCamera,
    ToggleScreenShare,
    StartScreenCapture(CaptureTarget, ScreenCaptureConfig),
    ScreenCapturePacket(Vec<u8>),
    ScreenCaptureStopped,
    ScreenCaptureError(String),
    ScreenTrackStarted(TrackHandle),
    CameraTrackStarted(TrackHandle),
    MicEnabled(TrackHandle),
    StateLoaded(String, Option<CallState>),
    PulseConnected(Arc<PulseClient>, String),
    PulseDisconnected,
    PulseEvent(PulseEvent),
    AudioTrackSubscribed(String),
    AudioPacket(String, Vec<u8>),
    VideoTrackSubscribed(String),
    VideoPacket(String, Vec<u8>),
    VideoFrameDecoded(String, Result<(u32, u32, Vec<u8>), String>),
    ConsumeScreenTrack(String),
    StopViewingScreenTrack,
    ToggleScreenshareFullscreen,
    RequestScreenKeyframe,
}

fn msg(m: CallMessage) -> Message {
    Message::Main(MainMessage::Call(m))
}

fn err(e: RenderableError) -> Message {
    Message::Main(MainMessage::ApiError(e))
}

pub struct CallContext<'a> {
    pub api: &'a Arc<ApiClient>,
    pub current_conversation: Option<&'a str>,
    pub self_user_id: &'a str,
}

pub struct CallSession {
    pub channel_id: Option<String>,
    pub call_id: Option<String>,
    pub state: Option<CallState>,
    pub pulse_client: Option<Arc<PulseClient>>,
    pub mic_track: Option<TrackHandle>,
    pub camera_track: Option<TrackHandle>,
    pub screen_track: Option<TrackHandle>,
    pub audio: AudioPipeline,
    pub screen_capture_session: Option<ScreenCaptureSession>,
    pub screen_capture_preview: Option<Arc<ArcSwap<Option<wgpu_capture::CaptureFrame>>>>,
    pub video_frames: HashMap<String, VideoFrame>,
    pub video_handles: HashMap<String, ImageHandle>,
    pub video_decode_tx: HashMap<String, mpsc::Sender<Vec<u8>>>,
    pub screen_view_track_id: Option<String>,
    pub screen_keyframe_request: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub available_screen_tracks: Vec<AvailableTrack>,
    pub screenshare_fullscreen: bool,
    pub screen_track_codec: Option<u8>,
    pub remote_screen_frame: Option<Arc<ArcSwap<Option<RemoteScreenFrame>>>>,
}

impl CallSession {
    pub fn new() -> Self {
        Self {
            channel_id: None,
            call_id: None,
            state: None,
            pulse_client: None,
            mic_track: None,
            camera_track: None,
            screen_track: None,
            audio: AudioPipeline::new().expect("audio pipeline init"),
            screen_capture_session: None,
            screen_capture_preview: None,
            video_frames: HashMap::new(),
            video_handles: HashMap::new(),
            video_decode_tx: HashMap::new(),
            screen_view_track_id: None,
            screen_keyframe_request: None,
            available_screen_tracks: Vec::new(),
            screenshare_fullscreen: false,
            screen_track_codec: None,
            remote_screen_frame: None,
        }
    }

    pub fn update(&mut self, message: CallMessage, ctx: CallContext) -> Task<Message> {
        match message {
            CallMessage::Join => {
                if let Some(conv_id) = ctx.current_conversation {
                    return connect_call_task(ctx.api.clone(), conv_id.to_string(), false);
                }
            }
            CallMessage::Start => {
                if let Some(conv_id) = ctx.current_conversation {
                    return connect_call_task(ctx.api.clone(), conv_id.to_string(), true);
                }
            }
            CallMessage::Leave => {
                if let Some(ref pulse) = self.pulse_client {
                    pulse.disconnect();
                }
                self.teardown_media();
                self.remove_self_participant(ctx.self_user_id);
                self.channel_id = None;
                self.call_id = None;
            }
            CallMessage::ToggleMic => {
                if let Some(p) = self.self_participant_mut(ctx.self_user_id) {
                    let new_audio = !p.tracks.audio;
                    p.tracks.audio = new_audio;
                    if !new_audio {
                        self.audio.stop_capture();
                    }
                    if let Some(conv_id) = self.channel_id.clone() {
                        let client = ctx.api.clone();
                        let pulse = self.pulse_client.clone();
                        let muted = !new_audio;
                        let mic_track = self.mic_track.take();
                        return Task::perform(
                            async move {
                                client
                                    .client()
                                    .update_voice_state(&conv_id, Some(muted), None)
                                    .await
                                    .map_err(RenderableError::from)?;
                                if let Some(pulse) = pulse {
                                    if new_audio {
                                        let handle = pulse
                                            .produce_track(MediaHint::Audio)
                                            .await
                                            .map_err(|e| {
                                                RenderableError::UnknownError(format!(
                                                    "Failed to produce audio track: {e}"
                                                ))
                                            })?;
                                        return Ok(Some(handle));
                                    } else if let Some(handle) = mic_track {
                                        let _ = pulse.stop_producing(handle).await;
                                    }
                                }
                                Ok::<Option<TrackHandle>, RenderableError>(None)
                            },
                            |result| match result {
                                Ok(Some(handle)) => msg(CallMessage::MicEnabled(handle)),
                                Ok(None) => Message::Main(MainMessage::DismissError),
                                Err(e) => err(e),
                            },
                        );
                    }
                }
            }
            CallMessage::ToggleCamera => {
                if let Some(p) = self.self_participant_mut(ctx.self_user_id) {
                    let new_video = !p.tracks.video;
                    p.tracks.video = new_video;
                    if let Some(pulse) = self.pulse_client.clone() {
                        let camera_track = self.camera_track.take();
                        return Task::perform(
                            async move {
                                if new_video {
                                    pulse
                                        .produce_track(pulse_api::MediaHint::Video)
                                        .await
                                        .map(Some)
                                } else {
                                    match camera_track {
                                        Some(handle) => {
                                            pulse.stop_producing(handle).await.map(|_| None)
                                        }
                                        None => Ok(None),
                                    }
                                }
                            },
                            |result| match result {
                                Ok(Some(handle)) => msg(CallMessage::CameraTrackStarted(handle)),
                                Ok(None) => Message::Main(MainMessage::DismissError),
                                Err(e) => err(RenderableError::UnknownError(format!(
                                    "Video track error: {e}"
                                ))),
                            },
                        );
                    }
                }
            }
            CallMessage::ToggleScreenShare => {
                if let Some(session) = self.screen_capture_session.take() {
                    session.stop();
                    self.screen_capture_preview = None;
                    self.screen_keyframe_request = None;
                    if let Some(p) = self.self_participant_mut(ctx.self_user_id) {
                        p.tracks.screen = false;
                    }
                    if let Some(pulse) = self.pulse_client.clone()
                        && let Some(handle) = self.screen_track.take()
                    {
                        return stop_producing_task(pulse, handle, "Screen share stop error");
                    }
                } else if self.pulse_client.is_some() {
                    return Task::done(Message::OpenScreenCapture);
                }
            }
            CallMessage::StartScreenCapture(target, config) => {
                let (session, rx, frame_ref, tick_rx, keyframe_flag) =
                    match crate::media::screen_capture::start_screen_capture(target, config) {
                        Ok(result) => result,
                        Err(e) => {
                            return Task::done(err(RenderableError::UnknownError(format!(
                                "Screen capture failed: {e:#}"
                            ))));
                        }
                    };
                self.screen_capture_session = Some(session);
                self.screen_capture_preview = Some(frame_ref);
                self.screen_keyframe_request = Some(keyframe_flag.clone());

                if let Some(p) = self.self_participant_mut(ctx.self_user_id) {
                    p.tracks.screen = true;
                }

                let pulse = self.pulse_client.clone();
                return Task::stream(stream! {
                    let Some(pulse) = pulse else {
                        yield msg(CallMessage::ScreenCaptureStopped);
                        return;
                    };
                    let screen_track = match pulse
                        .produce_track(pulse_api::MediaHint::ScreenVideo)
                        .await
                    {
                        Ok(handle) => handle,
                        Err(e) => {
                            yield err(RenderableError::UnknownError(format!(
                                "Failed to produce screen track: {e}"
                            )));
                            yield msg(CallMessage::ScreenCaptureStopped);
                            return;
                        }
                    };
                    yield msg(CallMessage::ScreenTrackStarted(screen_track.clone()));
                    let mut rx = rx;
                    let mut tick_rx = tick_rx;
                    loop {
                        tokio::select! {
                            packet = rx.recv() => {
                                match packet {
                                    Some(p) => {
                                        let payload = codec::prepend_codec_byte(p.codec, &p.data);
                                        if let Err(e) = pulse.send_media(
                                            &screen_track,
                                            p.capture_ts_us,
                                            p.keyframe,
                                            &payload,
                                        ) {
                                            tracing::warn!("screen send_media: {e:#}");
                                            break;
                                        }
                                    }
                                    None => break,
                                }
                            }
                            Some(()) = tick_rx.recv() => {
                                yield msg(CallMessage::ScreenCapturePacket(Vec::new()));
                            }
                        }
                    }
                    yield msg(CallMessage::ScreenCaptureStopped);
                });
            }
            CallMessage::ScreenCapturePacket(_) => {}
            CallMessage::ScreenCaptureStopped => {
                let was_sharing = self.screen_capture_session.is_some();
                if let Some(session) = self.screen_capture_session.take() {
                    session.stop();
                }
                self.screen_capture_preview = None;
                self.screen_keyframe_request = None;
                let mut had_track_flag = false;
                if let Some(p) = self.self_participant_mut(ctx.self_user_id) {
                    had_track_flag = p.tracks.screen;
                    p.tracks.screen = false;
                }
                if was_sharing || had_track_flag {
                    if let Some(pulse) = self.pulse_client.clone()
                        && let Some(handle) = self.screen_track.take()
                    {
                        return stop_producing_task(pulse, handle, "Screen share cleanup error");
                    }
                }
            }
            CallMessage::ScreenCaptureError(m) => {
                return Task::done(err(RenderableError::UnknownError(m)));
            }
            CallMessage::StateLoaded(channel_id, state) => {
                // Only update call state if not in an active call, or if this
                // is for the active call's channel
                if self.channel_id.is_none() || self.channel_id.as_deref() == Some(&channel_id) {
                    self.state = state;
                }
            }
            CallMessage::PulseConnected(pulse_client, call_id) => {
                self.pulse_client = Some(pulse_client);
                self.channel_id = ctx.current_conversation.map(str::to_string);
                self.call_id = Some(call_id);
            }
            CallMessage::PulseDisconnected => {
                self.teardown_media();
                if self.channel_id.is_some() {
                    self.remove_self_participant(ctx.self_user_id);
                    self.channel_id = None;
                    self.call_id = None;
                }
            }
            CallMessage::PulseEvent(event) => return self.handle_pulse_event(event),
            CallMessage::AudioTrackSubscribed(track_id) => {
                if let Err(e) = self.audio.add_track(track_id) {
                    tracing::warn!("audio add_track: {e:#}");
                }
                if let Err(e) = self.audio.start_playback() {
                    tracing::warn!("audio start_playback: {e:#}");
                }
            }
            CallMessage::AudioPacket(track_id, data) => {
                if let Err(e) = self.audio.feed_packet(&track_id, &data) {
                    tracing::warn!("audio feed_packet ({track_id}): {e:#}");
                }
            }
            CallMessage::VideoTrackSubscribed(track_id) => {
                let (data_tx, data_rx) = mpsc::channel::<Vec<u8>>();
                let (frame_tx, mut frame_rx) = tokio::sync::mpsc::unbounded_channel();
                let tid_thread = track_id.clone();
                let tid_stream = track_id.clone();

                let spawn_result = std::thread::Builder::new()
                    .name(format!("video-decode-{tid_thread}"))
                    .spawn(move || {
                        let mut decoder = match video::create_video_decoder(codec::VIDEO_H264) {
                            Ok(d) => d,
                            Err(e) => {
                                tracing::warn!(
                                    "failed to create video decoder for {tid_thread}: {e:#}"
                                );
                                return;
                            }
                        };
                        while let Ok(data) = data_rx.recv() {
                            match decoder.decode(&data) {
                                Ok(frames) => {
                                    if let Some(f) = frames.into_iter().last() {
                                        let _ =
                                            frame_tx.send(Ok((f.width, f.height, f.rgba.to_vec())));
                                    }
                                }
                                Err(e) => {
                                    tracing::debug!("video decode ({tid_thread}): {e:#}");
                                }
                            }
                        }
                    });
                if let Err(e) = spawn_result {
                    tracing::warn!("failed to spawn video decode thread: {e:#}");
                    return Task::none();
                }

                self.video_decode_tx.insert(track_id.clone(), data_tx);

                return Task::stream(stream! {
                    while let Some(result) = frame_rx.recv().await {
                        yield msg(CallMessage::VideoFrameDecoded(tid_stream.clone(), result));
                    }
                });
            }
            CallMessage::VideoPacket(track_id, data) => {
                if let Some(tx) = self.video_decode_tx.get(&track_id) {
                    let _ = tx.send(data);
                }
            }
            CallMessage::VideoFrameDecoded(track_id, result) => match result {
                Ok((width, height, rgba)) => {
                    let handle = ImageHandle::from_rgba(width, height, rgba.clone());
                    self.video_handles.insert(track_id.clone(), handle);

                    let frame = RemoteScreenFrame {
                        width,
                        height,
                        rgba,
                    };
                    let swap = self
                        .remote_screen_frame
                        .get_or_insert_with(|| Arc::new(ArcSwap::from_pointee(None)));
                    swap.store(Arc::new(Some(frame)));
                }
                Err(e) => {
                    tracing::debug!("video decode ({track_id}): {e}");
                }
            },
            CallMessage::MicEnabled(handle) => {
                self.mic_track = Some(handle.clone());
                if self.pulse_client.is_some() {
                    match self.audio.start_capture() {
                        Ok(Some(rx)) => {
                            let pulse = self.pulse_client.clone();
                            return Task::stream(stream! {
                                let Some(pulse) = pulse else { return; };
                                let mut rx = rx;
                                while let Some(packet) = rx.recv().await {
                                    if let Err(e) = pulse.send_media(
                                        &handle,
                                        codec::now_micros(),
                                        true,
                                        &packet,
                                    ) {
                                        tracing::warn!("mic send_media: {e:#}");
                                    }
                                }
                                yield Message::Main(MainMessage::DismissError);
                            });
                        }
                        Ok(None) => {} // already capturing
                        Err(e) => {
                            return Task::done(err(RenderableError::UnknownError(format!(
                                "Microphone error: {e}"
                            ))));
                        }
                    }
                }
            }
            CallMessage::CameraTrackStarted(handle) => {
                self.camera_track = Some(handle);
            }
            CallMessage::ScreenTrackStarted(handle) => {
                self.screen_track = Some(handle);
            }
            CallMessage::ConsumeScreenTrack(track_id) => {
                if self.screen_view_track_id.is_some() {
                    return Task::none();
                }
                if let Some(pulse) = self.pulse_client.clone() {
                    if let Some(track) = self
                        .available_screen_tracks
                        .iter()
                        .find(|t| t.id == track_id)
                        .cloned()
                    {
                        self.screen_view_track_id = Some(track.id.clone());

                        let drain_tid = track_id.clone();
                        return Task::stream(stream! {
                            let mut rx = match pulse.consume_track(&track).await {
                                Ok(rx) => rx,
                                Err(e) => {
                                    yield err(RenderableError::UnknownError(format!(
                                        "Failed to consume screen track: {e}"
                                    )));
                                    yield msg(CallMessage::StopViewingScreenTrack);
                                    return;
                                }
                            };
                            yield msg(CallMessage::VideoTrackSubscribed(drain_tid.clone()));

                            while let Some(frame) = rx.recv().await {
                                if let Some((_codec, data)) = codec::strip_codec_byte(&frame.data) {
                                    yield msg(CallMessage::VideoPacket(
                                        drain_tid.clone(),
                                        data.to_vec(),
                                    ));
                                }
                            }
                        });
                    }
                }
            }
            CallMessage::StopViewingScreenTrack => {
                if let Some(track_id) = self.clear_screen_view_state() {
                    if let Some(pulse) = self.pulse_client.clone() {
                        if let Err(e) = pulse.stop_consuming(track_id) {
                            tracing::warn!("stop_consuming screen track: {e:#}");
                        }
                    }
                }
            }
            CallMessage::ToggleScreenshareFullscreen => {
                self.screenshare_fullscreen = !self.screenshare_fullscreen;
            }
            CallMessage::RequestScreenKeyframe => {
                if let Some(ref flag) = self.screen_keyframe_request {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        Task::none()
    }

    fn handle_pulse_event(&mut self, event: PulseEvent) -> Task<Message> {
        tracing::info!("Received Pulse event: {:?}", event);
        match event {
            PulseEvent::Disconnected { reason } => {
                tracing::warn!("Voice disconnected: {reason}");
                self.teardown_media();
                self.channel_id = None;
                self.call_id = None;
            }
            PulseEvent::TrackAvailable(track) => {
                if let Some(ref pulse) = self.pulse_client {
                    let pulse = pulse.clone();
                    let is_audio =
                        matches!(&track.media_hint, MediaHint::Audio | MediaHint::ScreenAudio);
                    if is_audio {
                        return consume_track_task(pulse, track, true);
                    } else if matches!(track.media_hint, MediaHint::ScreenVideo) {
                        if let Some(ref mut call) = self.state {
                            if let Some(p) = call
                                .participants
                                .iter_mut()
                                .find(|p| p.session_id == track.session_id)
                            {
                                p.tracks.screen = true;
                            }
                        }
                        self.available_screen_tracks.push(track);
                    } else {
                        return consume_track_task(pulse, track, false);
                    }
                }
            }
            PulseEvent::TrackUnavailable(id) => {
                if let Some(track) = self.available_screen_tracks.iter().find(|t| t.id == id) {
                    let session_id = track.session_id.clone();
                    if let Some(ref mut call) = self.state {
                        if let Some(p) = call
                            .participants
                            .iter_mut()
                            .find(|p| p.session_id == session_id)
                        {
                            p.tracks.screen = false;
                        }
                    }
                }
                self.audio.remove_track(&id);
                self.video_frames.remove(&id);
                self.video_handles.remove(&id);
                self.video_decode_tx.remove(&id);
                self.available_screen_tracks.retain(|t| t.id != id);
                if self.screen_view_track_id.as_deref() == Some(id.as_str()) {
                    self.clear_screen_view_state();
                }
                tracing::info!("Track unavailable: {id}");
            }
            PulseEvent::KeyFrameRequested(media_hint) => {
                // we need to send an IDR
                if matches!(media_hint, MediaHint::ScreenVideo)
                    && let Some(ref flag) = self.screen_keyframe_request
                {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            PulseEvent::Error(e) => {
                tracing::warn!("Voice client error: {e}");
            }
            PulseEvent::Reconnecting { attempt } => {
                tracing::info!("Voice connection lost, reconnecting (attempt {attempt})");
            }
            _ => {}
        }
        Task::none()
    }

    pub fn on_user_joined(
        &mut self,
        call_id: &str,
        user_id: String,
        session_id: String,
        muted: bool,
        api: &Arc<ApiClient>,
    ) -> Task<Message> {
        if self.call_id.as_deref() != Some(call_id) {
            return Task::none();
        }
        let sharing_screen = self
            .available_screen_tracks
            .iter()
            .any(|t| t.session_id == session_id);
        let participant = CallParticipant {
            user_id: user_id.clone(),
            session_id,
            tracks: CallTrackState {
                audio: !muted,
                video: false,
                screen: sharing_screen,
            },
        };
        if let Some(ref mut call) = self.state {
            if !call
                .participants
                .iter()
                .any(|p| p.user_id == participant.user_id)
            {
                call.participants.push(participant);
            }
        } else {
            self.state = Some(CallState {
                participants: vec![participant],
            });
        }
        fetch_profiles_task(api.clone(), vec![user_id])
    }

    pub fn on_user_left(&mut self, call_id: &str, session_id: &str) {
        if self.call_id.as_deref() != Some(call_id) {
            return;
        }
        if let Some(ref mut call) = self.state {
            call.participants.retain(|p| p.session_id != session_id);
            if call.participants.is_empty() {
                self.state = None;
            }
        }
        let viewed_left = self.available_screen_tracks.iter().any(|t| {
            t.session_id == session_id
                && self.screen_view_track_id.as_deref() == Some(t.id.as_str())
        });
        self.available_screen_tracks
            .retain(|t| t.session_id != session_id);
        if viewed_left {
            self.clear_screen_view_state();
        }
    }

    pub fn on_voice_state_changed(&mut self, call_id: &str, session_id: &str, muted: bool) {
        if self.call_id.as_deref() != Some(call_id) {
            return;
        }
        if let Some(ref mut call) = self.state {
            if let Some(p) = call
                .participants
                .iter_mut()
                .find(|p| p.session_id == session_id)
            {
                p.tracks.audio = !muted;
            }
        }
    }

    pub fn on_call_migrated(&mut self, call_id: &str, api: &Arc<ApiClient>) -> Task<Message> {
        // reconnect to new server
        if self.call_id.as_deref() != Some(call_id) {
            return Task::none();
        }
        let Some(conv_id) = self.channel_id.clone() else {
            return Task::none();
        };
        if let Some(ref pulse) = self.pulse_client {
            pulse.disconnect();
        }
        self.pulse_client = None;
        connect_call_task(api.clone(), conv_id, false)
    }

    fn teardown_media(&mut self) {
        self.pulse_client = None;
        self.mic_track = None;
        self.camera_track = None;
        self.screen_track = None;
        self.audio.stop_playback();
        self.audio.stop_capture();
        if let Some(session) = self.screen_capture_session.take() {
            session.stop();
        }
        self.screen_capture_preview = None;
        self.screen_keyframe_request = None;
        self.clear_screen_view_state();
        self.available_screen_tracks.clear();
        self.video_frames.clear();
        self.video_handles.clear();
        self.video_decode_tx.clear();
    }

    fn remove_self_participant(&mut self, self_user_id: &str) {
        if let Some(ref mut call) = self.state {
            call.participants.retain(|p| p.user_id != self_user_id);
            if call.participants.is_empty() {
                self.state = None;
            }
        }
    }

    // FIXME: use session_id
    fn self_participant_mut(&mut self, self_user_id: &str) -> Option<&mut CallParticipant> {
        self.state
            .as_mut()?
            .participants
            .iter_mut()
            .find(|p| p.user_id == self_user_id)
    }

    fn clear_screen_view_state(&mut self) -> Option<String> {
        let track_id = self.screen_view_track_id.take();
        self.remote_screen_frame = None;
        self.screenshare_fullscreen = false;
        if let Some(ref id) = track_id {
            self.video_decode_tx.remove(id);
            self.video_frames.remove(id);
            self.video_handles.remove(id);
        }
        track_id
    }

    pub fn is_local_screensharing(&self) -> bool {
        self.screen_capture_session.is_some()
    }

    pub fn is_consuming_remote_screenshare(&self) -> bool {
        self.screen_view_track_id.is_some()
    }

    pub fn remote_screenshare_available(&self, self_user_id: &str) -> Option<&CallParticipant> {
        let call = self.state.as_ref()?;
        self.available_screen_tracks.iter().find_map(|t| {
            call.participants
                .iter()
                .find(|p| p.session_id == t.session_id && p.user_id != self_user_id)
        })
    }

    pub fn pending_screen_track_id(&self) -> Option<&str> {
        self.available_screen_tracks.first().map(|t| t.id.as_str())
    }

    pub fn has_active_screenshare(&self) -> bool {
        self.is_local_screensharing() || !self.available_screen_tracks.is_empty()
    }
}

async fn fetch_call_state(
    api: &ApiClient,
    conv_id: &str,
) -> RenderableResult<(CallState, Vec<crate::api::UserProfile>)> {
    let members = api.client().get_call_members(conv_id).await?;
    let ids: Vec<String> = members.iter().map(|m| m.user_id.clone()).collect();
    let profiles = api.get_profiles(ids).await.unwrap_or_default();
    let state = CallState {
        participants: members.into_iter().map(CallParticipant::from).collect(),
    };
    Ok((state, profiles))
}

pub fn load_call_state_task(api: Arc<ApiClient>, conv_id: String) -> Task<Message> {
    Task::stream(stream! {
        match fetch_call_state(&api, &conv_id).await {
            Ok((state, profiles)) => {
                yield Message::Main(MainMessage::ProfilesLoaded(profiles));
                yield msg(CallMessage::StateLoaded(conv_id, Some(state)));
            }
            Err(e) => yield err(e),
        }
    })
}

fn connect_call_task(client: Arc<ApiClient>, conv_id: String, start_first: bool) -> Task<Message> {
    Task::stream(stream! {
        if start_first {
            if let Err(e) = client.client().start_call(&conv_id, None).await {
                yield err(e.into());
                return;
            }
        }
        let token_info = match client.client().create_call_token(&conv_id, true, false).await {
            Ok(info) => info,
            Err(e) => {
                yield err(e.into());
                return;
            }
        };
        let (pulse_client, mut event_rx) = match PulseClient::connect(PulseClientOptions {
            server_url: token_info.server_address,
            session_id: token_info.id,
            session_token: token_info.token,
            call_id: token_info.call_id.clone(),
            identity: client.call_identity().await,
        })
        .await
        {
            Ok(result) => result,
            Err(e) => {
                yield err(RenderableError::UnknownError(format!(
                    "Failed to connect to voice server: {e}"
                )));
                return;
            }
        };
        yield msg(CallMessage::PulseConnected(
            Arc::new(pulse_client),
            token_info.call_id,
        ));
        match fetch_call_state(&client, &conv_id).await {
            Ok((state, profiles)) => {
                yield Message::Main(MainMessage::ProfilesLoaded(profiles));
                yield msg(CallMessage::StateLoaded(conv_id, Some(state)));
            }
            Err(_) => yield msg(CallMessage::StateLoaded(conv_id, None)),
        }
        while let Some(event) = event_rx.recv().await {
            yield msg(CallMessage::PulseEvent(event));
        }
        yield msg(CallMessage::PulseDisconnected);
    })
}

fn consume_track_task(
    pulse: Arc<PulseClient>,
    track: AvailableTrack,
    is_audio: bool,
) -> Task<Message> {
    Task::stream(stream! {
        let track_id = track.id.clone();
        let kind = if is_audio { "audio" } else { "video" };
        let mut rx = match pulse.consume_track(&track).await {
            Ok(rx) => rx,
            Err(e) => {
                yield err(RenderableError::UnknownError(format!(
                    "Failed to consume {kind} track: {e}"
                )));
                return;
            }
        };
        yield msg(if is_audio {
            CallMessage::AudioTrackSubscribed(track_id.clone())
        } else {
            CallMessage::VideoTrackSubscribed(track_id.clone())
        });
        while let Some(frame) = rx.recv().await {
            if let Some((_codec, data)) = codec::strip_codec_byte(&frame.data) {
                yield msg(if is_audio {
                    CallMessage::AudioPacket(track_id.clone(), data.to_vec())
                } else {
                    CallMessage::VideoPacket(track_id.clone(), data.to_vec())
                });
            }
        }
    })
}

fn stop_producing_task(
    pulse: Arc<PulseClient>,
    handle: TrackHandle,
    error_context: &'static str,
) -> Task<Message> {
    Task::perform(
        async move { pulse.stop_producing(handle).await },
        move |result| match result {
            Ok(()) => Message::Main(MainMessage::DismissError),
            Err(e) => err(RenderableError::UnknownError(format!(
                "{error_context}: {e}"
            ))),
        },
    )
}
