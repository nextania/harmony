use std::{
    collections::HashMap,
    num::NonZero,
    sync::{Arc, mpsc},
    time::UNIX_EPOCH,
};

use async_stream::stream;
use harmony_api::{ClientEvent, Event, LifecycleEvent};
use iced::{
    Element, Length, Task,
    widget::{Space, button, column, container, row, text},
};
use lru::LruCache;
use pulse_api::{
    AvailableTrack, MediaHint, PulseClient, PulseClientOptions, PulseEvent, TrackHandle,
};
use ulid::Ulid;

use crate::media::screen_capture::{ScreenCaptureConfig, ScreenCaptureSession};
use crate::media::video::{self, Frame as VideoFrame};
use crate::media::{audio::AudioPipeline, codec};
use crate::{
    ChatMessage, Message, MessageContent,
    api::{
        ApiClient, ApiMessageContent, CallParticipant, CallState, CallTrackState, Contact,
        ContactAction, ContactStatus, placeholder_profile,
    },
    errors::RenderableError,
    format_message_time,
    icons::{FLUENT_ICONS, Icon},
    theme::{BG_APP, DM_SANS, TEXT_MUTED},
    views::main::{
        chat_area::chat_area, chat_list::chat_list, people_list::people_list, sidebar::sidebar,
    },
};
use arc_swap::ArcSwap;
use iced::advanced::image::Handle as ImageHandle;
use wgpu_capture::CaptureTarget;

pub mod chat_area;
pub mod chat_list;
pub mod people_list;
pub mod sidebar;

#[derive(Debug, Clone)]
pub enum AvatarAction {
    Profile,
    Settings,
    Logout,
}

#[derive(Debug, Clone)]
pub enum SidebarTab {
    Messages,
    Spaces,
    People,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatMode {
    Text,
    Voice,
}

#[derive(Clone)]
pub enum MainMessage {
    TabSelected(SidebarTab),
    ChatModeSelected(ChatMode),
    ChatSelected(String),
    ChatInputChanged(String),
    SearchInputChanged(String),
    SendMessage,
    MessageSent(ChatMessage),
    EditMessage(String, String),
    MessageEdited(String, ChatMessage),
    DeleteMessage(String),
    MessageDeleted(String, String),
    ServerEvent(harmony_api::ClientEvent),
    Ignore,
    JoinCall,
    StartCall,
    LeaveCall,
    ToggleMic,
    ToggleCamera,
    ToggleScreenShare,
    StartScreenCapture(CaptureTarget, ScreenCaptureConfig),
    ScreenCapturePacket(Vec<u8>),
    ScreenCaptureStopped,
    VideoTrackSubscribed(String),
    VideoPacket(String, Vec<u8>),
    CallStateLoaded(String, Option<CallState>),
    CallParticipantJoined(CallParticipant),
    PulseConnected(Arc<PulseClient>, String),
    PulseDisconnected,
    PulseEvent(PulseEvent),
    AudioTrackSubscribed(String),
    AudioPacket(String, Vec<u8>),
    MicEnabled(TrackHandle),
    CameraTrackStarted(TrackHandle),
    ScreenTrackStarted(TrackHandle),
    ToggleChatList,
    ToggleAvatarMenu,
    AvatarMenuDismiss,
    AvatarMenuAction(AvatarAction),
    OpenSettings,
    MessagesLoaded(String, Vec<ChatMessage>),
    NewMessageDecrypted(String, ChatMessage),
    ApiError(RenderableError),
    DismissError,
    ToggleEmojiPicker,
    EmojiPickerDismiss,
    EmojiSelected(String),
    EmojiCategorySelected(emojis::Group),
    EmojiSearchChanged(String),
    ContactsLoaded(Vec<Contact>),
    AddContactInputChanged(String),
    AddContactSubmit,
    ContactAdded(Contact),
    RemoveContact(String),
    ContactRemoved(String),
    AcceptContact(String),
    ContactAccepted(Contact),
    BlockContact(String),
    ContactBlocked(String),
    UnblockContact(String),
    ContactUnblocked(Contact),
    OpenPrivateChannel(String),
    PrivateChannelOpened(crate::errors::RenderableResult<crate::api::Channel>),

    ConsumeScreenTrack(String),
    StopViewingScreenTrack,
    ToggleScreenshareFullscreen,
    RequestScreenKeyframe,
    ScreenCaptureError(String),
    VideoFrameDecoded(String, Result<(u32, u32, Vec<u8>), String>),
}

pub struct MainView {
    active_tab: SidebarTab,
    chat_mode: ChatMode,
    api: Arc<ApiClient>,
    pub chat_input: String,
    pub search_input: String,
    pub conversations: HashMap<String, crate::api::Channel>,
    pub current_conversation: Option<String>,
    pub conversation_messages: LruCache<String, Vec<ChatMessage>>,
    pub current_user: crate::api::CurrentUser,
    pub chat_list_visible: bool,
    pub avatar_menu_open: bool,
    pub current_call: Option<String>,
    pub current_call_id: Option<String>,
    pub current_call_state: Option<CallState>,
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

    pub current_conversation_messages: Vec<ChatMessage>,

    pub emoji_picker_open: bool,
    pub emoji_picker_category: emojis::Group,
    pub emoji_search: String,

    pub contacts: Vec<Contact>,
    pub contacts_loaded: bool,
    pub add_contact_input: String,

    pub error: Option<RenderableError>,

    // Screenshare state
    /// Global track id of the screen track this client is currently viewing.
    /// Used to route decoded frames to the correct decoder and to target
    /// keyframe requests at the right producer.
    pub screen_view_track_id: Option<String>,
    pub screen_keyframe_request: Option<Arc<std::sync::atomic::AtomicBool>>,
    pub available_screen_tracks: Vec<AvailableTrack>,
    pub screenshare_fullscreen: bool,
    pub screen_track_codec: Option<u8>,
    pub remote_screen_frame:
        Option<Arc<ArcSwap<Option<crate::widgets::remote_screen::RemoteScreenFrame>>>>,
}

impl MainView {
    pub fn new(
        api: Arc<ApiClient>,
        current_user: crate::api::CurrentUser,
        conversations: HashMap<String, crate::api::Channel>,
    ) -> Self {
        Self {
            active_tab: SidebarTab::Messages,
            chat_mode: ChatMode::Text,
            api,
            chat_input: String::new(),
            search_input: String::new(),
            conversations,
            current_conversation: None,
            conversation_messages: LruCache::new(NonZero::new(100).unwrap()),
            current_user,
            chat_list_visible: true,
            avatar_menu_open: false,
            current_call: None,
            current_call_id: None,
            current_call_state: None,
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
            current_conversation_messages: Vec::new(),
            emoji_picker_open: false,
            emoji_picker_category: emojis::Group::SmileysAndEmotion,
            emoji_search: String::new(),
            contacts: Vec::new(),
            contacts_loaded: false,
            add_contact_input: String::new(),
            error: None,
            screen_view_track_id: None,
            screen_keyframe_request: None,
            available_screen_tracks: Vec::new(),
            screenshare_fullscreen: false,
            screen_track_codec: None,
            remote_screen_frame: None,
        }
    }

    pub fn update(&mut self, message: MainMessage) -> Task<Message> {
        match message {
            MainMessage::TabSelected(tab) => {
                self.active_tab = tab;
                if matches!(self.active_tab, SidebarTab::People) && !self.contacts_loaded {
                    let client = self.api.clone();
                    return Task::perform(async move { client.get_contacts().await }, |result| {
                        match result {
                            Ok(contacts) => Message::Main(MainMessage::ContactsLoaded(contacts)),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        }
                    });
                }
            }
            MainMessage::ChatModeSelected(mode) => {
                if self.chat_mode == mode {
                    return Task::none();
                }
                self.chat_mode = mode;
                if matches!(self.chat_mode, ChatMode::Voice) {
                    if let Some(conv_id) = self.current_conversation.clone() {
                        let client = self.api.clone();
                        return Task::perform(
                            async move {
                                client
                                    .get_call(&conv_id)
                                    .await
                                    .map(|state| (conv_id, state))
                            },
                            |result| match result {
                                Ok((channel_id, state)) => {
                                    Message::Main(MainMessage::CallStateLoaded(channel_id, state))
                                }
                                Err(e) => Message::Main(MainMessage::ApiError(e)),
                            },
                        );
                    }
                }
            }
            MainMessage::ChatSelected(i) => {
                if self.current_conversation.as_ref() == Some(&i) {
                    return Task::none();
                }
                self.current_conversation = Some(i.clone());
                self.current_conversation_messages = vec![];
                if self.current_call.is_none() {
                    self.current_call_state = None;
                }

                let call_client = self.api.clone();
                let call_channel_id = i.clone();
                let call_task = Task::perform(
                    async move {
                        call_client
                            .get_call(&call_channel_id)
                            .await
                            .map(|state| (call_channel_id, state))
                    },
                    |result| match result {
                        Ok((channel_id, state)) => {
                            Message::Main(MainMessage::CallStateLoaded(channel_id, state))
                        }
                        Err(e) => Message::Main(MainMessage::ApiError(e)),
                    },
                );

                // return a task to load messages for this conversation if not already cached
                let msg_task = if !self.conversation_messages.contains(&i) {
                    let client = self.api.clone();
                    Task::perform(
                        async move {
                            let raw = client.get_messages(&i).await?;
                            let messages = raw
                                .into_iter()
                                .map(|api_msg| {
                                    let time = Ulid::from_string(&api_msg.id)
                                        .expect("Invalid ULID")
                                        .datetime()
                                        .duration_since(UNIX_EPOCH)
                                        .expect("Time went backwards")
                                        .as_millis()
                                        as i64;
                                    ChatMessage {
                                        id: api_msg.id.clone(),
                                        user: api_msg.author.clone(),
                                        time,
                                        formatted_time: format_message_time(time),
                                        content: match api_msg.content {
                                            ApiMessageContent::Text(text) => {
                                                MessageContent::Text(text)
                                            }
                                            ApiMessageContent::CallCard { channel, duration } => {
                                                MessageContent::CallCard { channel, duration }
                                            }
                                        },
                                    }
                                })
                                .collect();
                            Ok((i, messages))
                        },
                        |result| match result {
                            Ok((conv_id, messages)) => {
                                Message::Main(MainMessage::MessagesLoaded(conv_id, messages))
                            }
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    )
                } else {
                    Task::done(Message::Main(MainMessage::MessagesLoaded(
                        i.clone(),
                        self.conversation_messages
                            .get(&i)
                            .cloned()
                            .unwrap_or_default(),
                    )))
                };
                return Task::batch([msg_task, call_task]);
            }
            MainMessage::ChatInputChanged(s) => self.chat_input = s,
            MainMessage::SearchInputChanged(s) => self.search_input = s,
            MainMessage::SendMessage => {
                if !self.chat_input.is_empty() {
                    if let Some(conv_id) = &self.current_conversation {
                        let client = self.api.clone();
                        let channel_id = conv_id.clone();
                        let content = self.chat_input.clone();
                        self.chat_input.clear();
                        let _current_user = self.current_user.clone();
                        return Task::perform(
                            async move { client.send_message(&channel_id, &content).await },
                            move |result| match result {
                                Ok(api_msg) => {
                                    let time = Ulid::from_string(&api_msg.id)
                                        .map(|u| {
                                            u.datetime()
                                                .duration_since(UNIX_EPOCH)
                                                .unwrap_or_default()
                                                .as_millis()
                                                as i64
                                        })
                                        .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis());
                                    let chat_msg = ChatMessage {
                                        id: api_msg.id.clone(),
                                        user: api_msg.author.clone(),
                                        time,
                                        formatted_time: format_message_time(time),
                                        content: match api_msg.content {
                                            ApiMessageContent::Text(t) => MessageContent::Text(t),
                                            ApiMessageContent::CallCard { channel, duration } => {
                                                MessageContent::CallCard { channel, duration }
                                            }
                                        },
                                    };
                                    Message::Main(MainMessage::MessageSent(chat_msg))
                                }
                                Err(e) => Message::Main(MainMessage::ApiError(e)),
                            },
                        );
                    }
                }
            }
            MainMessage::MessageSent(msg) => {
                if let Some(conv_id) = &self.current_conversation {
                    if let Some(msgs) = self.conversation_messages.get_mut(conv_id) {
                        msgs.push(msg.clone());
                    } else {
                        self.conversation_messages
                            .put(conv_id.clone(), vec![msg.clone()]);
                    }
                    self.current_conversation_messages.push(msg);
                }
            }
            MainMessage::NewMessageDecrypted(channel_id, chat_msg) => {
                if let Some(msgs) = self.conversation_messages.get_mut(&channel_id) {
                    msgs.push(chat_msg.clone());
                }
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    self.current_conversation_messages.push(chat_msg);
                }
            }
            MainMessage::EditMessage(message_id, new_content) => {
                if let Some(conv_id) = &self.current_conversation {
                    let client = self.api.clone();
                    let channel_id = conv_id.clone();
                    let mid = message_id.clone();
                    return Task::perform(
                        async move { client.edit_message(&mid, &channel_id, &new_content).await },
                        move |result| match result {
                            Ok(api_msg) => {
                                let time = Ulid::from_string(&api_msg.id)
                                    .map(|u| {
                                        u.datetime()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis()
                                            as i64
                                    })
                                    .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis());
                                let chat_msg = ChatMessage {
                                    id: api_msg.id.clone(),
                                    user: api_msg.author.clone(),
                                    time,
                                    formatted_time: format_message_time(time),
                                    content: match api_msg.content {
                                        ApiMessageContent::Text(t) => MessageContent::Text(t),
                                        ApiMessageContent::CallCard { channel, duration } => {
                                            MessageContent::CallCard { channel, duration }
                                        }
                                    },
                                };
                                Message::Main(MainMessage::MessageEdited(message_id, chat_msg))
                            }
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                }
            }
            MainMessage::MessageEdited(message_id, updated_msg) => {
                if let Some(conv_id) = &self.current_conversation {
                    if let Some(msgs) = self.conversation_messages.get_mut(conv_id) {
                        if let Some(m) = msgs.iter_mut().find(|m| m.id == message_id) {
                            *m = updated_msg.clone();
                        }
                    }
                    if let Some(m) = self
                        .current_conversation_messages
                        .iter_mut()
                        .find(|m| m.id == message_id)
                    {
                        *m = updated_msg;
                    }
                }
            }
            MainMessage::DeleteMessage(message_id) => {
                if let Some(conv_id) = &self.current_conversation {
                    let client = self.api.clone();
                    let mid = message_id.clone();
                    let cid = conv_id.clone();
                    return Task::perform(
                        async move { client.delete_message(&mid).await },
                        move |result| match result {
                            Ok(()) => Message::Main(MainMessage::MessageDeleted(message_id, cid)),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                }
            }
            MainMessage::MessageDeleted(message_id, channel_id) => {
                if let Some(msgs) = self.conversation_messages.get_mut(&channel_id) {
                    msgs.retain(|m| m.id != message_id);
                }
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    self.current_conversation_messages
                        .retain(|m| m.id != message_id);
                }
            }
            MainMessage::ServerEvent(event) => {
                tracing::info!("Received client event: {:?}", event);
                match event {
                    ClientEvent::Lifecycle(l) => return self.handle_lifecycle_event(l),
                    ClientEvent::Event(e) => {
                        let crypto_task = {
                            let api = self.api.clone();
                            let ev = e.clone();
                            Task::perform(async move { api.handle_event(&ev).await }, |result| {
                                match result {
                                    Ok(Some(contact)) => {
                                        Message::Main(MainMessage::ContactAccepted(contact))
                                    }
                                    Ok(None) => Message::Main(MainMessage::Ignore),
                                    Err(err) => Message::Main(MainMessage::ApiError(err)),
                                }
                            })
                        };
                        let ui_task = self.handle_server_event(e);
                        return Task::batch([crypto_task, ui_task]);
                    }
                }
            }
            MainMessage::Ignore => {}
            MainMessage::JoinCall => {
                if let Some(conv_id) = self.current_conversation.clone() {
                    let client = self.api.clone();
                    return Task::stream(stream! {
                        let token_info = match client.create_call_token(&conv_id).await {
                            Ok(info) => info,
                            Err(e) => {
                                yield Message::Main(MainMessage::ApiError(e));
                                return;
                            }
                        };
                        let (pulse_client, mut event_rx) = match PulseClient::connect(
                            PulseClientOptions {
                                server_url: token_info.server_address,
                                session_id: token_info.session_id,
                                session_token: token_info.token,
                                call_id: token_info.call_id.clone(),
                                identity: client.call_identity().await,
                            },
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                yield Message::Main(MainMessage::ApiError(
                                    RenderableError::UnknownError(format!(
                                        "Failed to connect to voice server: {e}"
                                    )),
                                ));
                                return;
                            }
                        };
                        let pulse_call_id = token_info.call_id.clone();
                        yield Message::Main(MainMessage::PulseConnected(Arc::new(pulse_client), pulse_call_id));
                        let call_state = client.get_call(&conv_id).await.ok().flatten();
                        yield Message::Main(MainMessage::CallStateLoaded(conv_id, call_state));
                        while let Some(event) = event_rx.recv().await {
                            yield Message::Main(MainMessage::PulseEvent(event));
                        }
                        yield Message::Main(MainMessage::PulseDisconnected);
                    });
                }
            }
            MainMessage::StartCall => {
                if let Some(conv_id) = self.current_conversation.clone() {
                    let client = self.api.clone();
                    return Task::stream(stream! {
                        if let Err(e) = client.start_call(&conv_id).await {
                            yield Message::Main(MainMessage::ApiError(e));
                            return;
                        }
                        let token_info = match client.create_call_token(&conv_id).await {
                            Ok(info) => info,
                            Err(e) => {
                                yield Message::Main(MainMessage::ApiError(e));
                                return;
                            }
                        };
                        tracing::info!("Token info: {:?}", token_info);
                        let (pulse_client, mut event_rx) = match PulseClient::connect(
                            PulseClientOptions {
                                server_url: token_info.server_address,
                                session_id: token_info.session_id,
                                session_token: token_info.token,
                                call_id: token_info.call_id.clone(),
                                identity: client.call_identity().await,
                            },
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                yield Message::Main(MainMessage::ApiError(
                                    RenderableError::UnknownError(format!(
                                        "Failed to connect to voice server: {e}"
                                    )),
                                ));
                                return;
                            }
                        };
                        let pulse_call_id = token_info.call_id.clone();
                        yield Message::Main(MainMessage::PulseConnected(Arc::new(pulse_client), pulse_call_id));
                        let call_state = client.get_call(&conv_id).await.ok().flatten();
                        tracing::info!("Loaded call state: {:?}", call_state);
                        yield Message::Main(MainMessage::CallStateLoaded(conv_id, call_state));
                        while let Some(event) = event_rx.recv().await {
                            yield Message::Main(MainMessage::PulseEvent(event));
                        }
                        yield Message::Main(MainMessage::PulseDisconnected);
                    });
                }
            }
            MainMessage::LeaveCall => {
                if let Some(ref pulse) = self.pulse_client {
                    pulse.disconnect();
                }
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
                if let Some(ref mut call) = self.current_call_state {
                    call.participants
                        .retain(|p| p.profile.id != self.current_user.profile.id);
                    if call.participants.is_empty() {
                        self.current_call_state = None;
                    }
                }
                self.current_call = None;
                self.current_call_id = None;
            }
            MainMessage::ToggleMic => {
                if let Some(ref mut call) = self.current_call_state {
                    if let Some(p) = call
                        .participants
                        .iter_mut()
                        // FIXME: use session_id
                        .find(|p| p.profile.id == self.current_user.profile.id)
                    {
                        let new_audio = !p.tracks.audio;
                        p.tracks.audio = new_audio;
                        if !new_audio {
                            self.audio.stop_capture();
                        }
                        if let Some(conv_id) = self.current_call.clone() {
                            let client = self.api.clone();
                            let pulse = self.pulse_client.clone();
                            let muted = !new_audio;
                            let mic_track = self.mic_track.take();
                            return Task::perform(
                                async move {
                                    client
                                        .update_voice_state(&conv_id, Some(muted), None)
                                        .await?;
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
                                    Ok(Some(handle)) => {
                                        Message::Main(MainMessage::MicEnabled(handle))
                                    }
                                    Ok(None) => Message::Main(MainMessage::DismissError),
                                    Err(e) => Message::Main(MainMessage::ApiError(e)),
                                },
                            );
                        }
                    }
                }
            }
            MainMessage::ToggleCamera => {
                if let Some(ref mut call) = self.current_call_state {
                    if let Some(p) = call
                        .participants
                        .iter_mut()
                        .find(|p| p.profile.id == self.current_user.profile.id)
                    {
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
                                    Ok(Some(handle)) => {
                                        Message::Main(MainMessage::CameraTrackStarted(handle))
                                    }
                                    Ok(None) => Message::Main(MainMessage::DismissError),
                                    Err(e) => Message::Main(MainMessage::ApiError(
                                        RenderableError::UnknownError(format!(
                                            "Video track error: {e}"
                                        )),
                                    )),
                                },
                            );
                        }
                    }
                }
            }
            MainMessage::ToggleScreenShare => {
                if let Some(session) = self.screen_capture_session.take() {
                    session.stop();
                    self.screen_capture_preview = None;
                    self.screen_keyframe_request = None;
                    if let Some(ref mut call) = self.current_call_state {
                        if let Some(p) = call
                            .participants
                            .iter_mut()
                            .find(|p| p.profile.id == self.current_user.profile.id)
                        {
                            p.tracks.screen = false;
                        }
                    }
                    if let Some(pulse) = self.pulse_client.clone()
                        && let Some(handle) = self.screen_track.take()
                    {
                        return Task::perform(
                            async move { pulse.stop_producing(handle).await },
                            |result| match result {
                                Ok(()) => Message::Main(MainMessage::DismissError),
                                Err(e) => Message::Main(MainMessage::ApiError(
                                    RenderableError::UnknownError(format!(
                                        "Screen share stop error: {e}"
                                    )),
                                )),
                            },
                        );
                    }
                } else if self.pulse_client.is_some() {
                    return Task::done(Message::OpenScreenCapture);
                }
            }
            MainMessage::StartScreenCapture(target, config) => {
                let (session, rx, frame_ref, tick_rx, keyframe_flag) =
                    match crate::media::screen_capture::start_screen_capture(target, config) {
                        Ok(result) => result,
                        Err(e) => {
                            self.error = Some(RenderableError::UnknownError(format!(
                                "Screen capture failed: {e:#}"
                            )));
                            return Task::none();
                        }
                    };
                self.screen_capture_session = Some(session);
                self.screen_capture_preview = Some(frame_ref);
                self.screen_keyframe_request = Some(keyframe_flag.clone());

                if let Some(ref mut call) = self.current_call_state {
                    if let Some(p) = call
                        .participants
                        .iter_mut()
                        .find(|p| p.profile.id == self.current_user.profile.id)
                    {
                        p.tracks.screen = true;
                    }
                }

                let pulse = self.pulse_client.clone();
                return Task::stream(stream! {
                    let Some(pulse) = pulse else {
                        yield Message::Main(MainMessage::ScreenCaptureStopped);
                        return;
                    };
                    let screen_track = match pulse
                        .produce_track(pulse_api::MediaHint::ScreenVideo)
                        .await
                    {
                        Ok(handle) => handle,
                        Err(e) => {
                            yield Message::Main(MainMessage::ApiError(
                                RenderableError::UnknownError(format!(
                                    "Failed to produce screen track: {e}"
                                )),
                            ));
                            yield Message::Main(MainMessage::ScreenCaptureStopped);
                            return;
                        }
                    };
                    yield Message::Main(MainMessage::ScreenTrackStarted(screen_track.clone()));
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
                                yield Message::Main(MainMessage::ScreenCapturePacket(Vec::new()));
                            }
                        }
                    }
                    yield Message::Main(MainMessage::ScreenCaptureStopped);
                });
            }
            MainMessage::ScreenCapturePacket(_) => {}
            MainMessage::ScreenCaptureStopped => {
                let was_sharing = self.screen_capture_session.is_some();
                if let Some(session) = self.screen_capture_session.take() {
                    session.stop();
                }
                self.screen_capture_preview = None;
                self.screen_keyframe_request = None;
                let mut had_track_flag = false;
                if let Some(ref mut call) = self.current_call_state {
                    if let Some(p) = call
                        .participants
                        .iter_mut()
                        .find(|p| p.profile.id == self.current_user.profile.id)
                    {
                        had_track_flag = p.tracks.screen;
                        p.tracks.screen = false;
                    }
                }
                if was_sharing || had_track_flag {
                    if let Some(pulse) = self.pulse_client.clone()
                        && let Some(handle) = self.screen_track.take()
                    {
                        return Task::perform(
                            async move { pulse.stop_producing(handle).await },
                            |result| match result {
                                Ok(()) => Message::Main(MainMessage::DismissError),
                                Err(e) => Message::Main(MainMessage::ApiError(
                                    RenderableError::UnknownError(format!(
                                        "Screen share cleanup error: {e}"
                                    )),
                                )),
                            },
                        );
                    }
                }
            }
            MainMessage::CallStateLoaded(channel_id, state) => {
                // Only update call state if not in an active call, or if this is for the active call's channel
                if self.current_call.is_none() || self.current_call.as_deref() == Some(&channel_id)
                {
                    self.current_call_state = state;
                }
            }
            MainMessage::CallParticipantJoined(mut participant) => {
                if self
                    .available_screen_tracks
                    .iter()
                    .any(|t| t.session_id == participant.session_id)
                {
                    participant.tracks.screen = true;
                }
                if let Some(ref mut call) = self.current_call_state {
                    if !call
                        .participants
                        .iter()
                        .any(|p| p.profile.id == participant.profile.id)
                    {
                        call.participants.push(participant);
                    }
                } else {
                    self.current_call_state = Some(CallState {
                        participants: vec![participant],
                    });
                }
            }
            MainMessage::PulseConnected(pulse_client, call_id) => {
                self.pulse_client = Some(pulse_client);
                self.current_call = self.current_conversation.clone();
                self.current_call_id = Some(call_id);
            }
            MainMessage::PulseDisconnected => {
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
                if self.current_call.is_some() {
                    if let Some(ref mut call) = self.current_call_state {
                        call.participants
                            .retain(|p| p.profile.id != self.current_user.profile.id);
                        if call.participants.is_empty() {
                            self.current_call_state = None;
                        }
                    }
                    self.current_call = None;
                    self.current_call_id = None;
                }
            }
            MainMessage::PulseEvent(event) => {
                tracing::info!("Received Pulse event: {:?}", event);
                match event {
                    PulseEvent::Disconnected { reason } => {
                        tracing::warn!("Voice disconnected: {reason}");
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
                        self.current_call = None;
                        self.current_call_id = None;
                    }
                    PulseEvent::TrackAvailable(track) => {
                        if let Some(ref pulse) = self.pulse_client {
                            let pulse = pulse.clone();
                            let is_audio = matches!(
                                &track.media_hint,
                                MediaHint::Audio | MediaHint::ScreenAudio
                            );
                            if is_audio {
                                return Task::stream(stream! {
                                    let track_id = track.id.clone();
                                    let mut rx = match pulse.consume_track(&track).await {
                                        Ok(rx) => rx,
                                        Err(e) => {
                                            yield Message::Main(MainMessage::ApiError(
                                                RenderableError::UnknownError(format!(
                                                    "Failed to consume audio track: {e}"
                                                )),
                                            ));
                                            return;
                                        }
                                    };
                                    yield Message::Main(MainMessage::AudioTrackSubscribed(
                                        track_id.clone(),
                                    ));
                                    while let Some(frame) = rx.recv().await {
                                        if let Some((_codec, data)) =
                                            codec::strip_codec_byte(&frame.data)
                                        {
                                            yield Message::Main(MainMessage::AudioPacket(
                                                track_id.clone(),
                                                data.to_vec(),
                                            ));
                                        }
                                    }
                                });
                            } else if matches!(track.media_hint, MediaHint::ScreenVideo) {
                                if let Some(ref mut call) = self.current_call_state {
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
                                return Task::stream(stream! {
                                    let track_id = track.id.clone();
                                    let mut rx = match pulse.consume_track(&track).await {
                                        Ok(rx) => rx,
                                        Err(e) => {
                                            yield Message::Main(MainMessage::ApiError(
                                                RenderableError::UnknownError(format!(
                                                    "Failed to consume video track: {e}"
                                                )),
                                            ));
                                            return;
                                        }
                                    };
                                    yield Message::Main(MainMessage::VideoTrackSubscribed(
                                        track_id.clone(),
                                    ));
                                    while let Some(frame) = rx.recv().await {
                                        if let Some((_codec, data)) =
                                            codec::strip_codec_byte(&frame.data)
                                        {
                                            yield Message::Main(MainMessage::VideoPacket(
                                                track_id.clone(),
                                                data.to_vec(),
                                            ));
                                        }
                                    }
                                });
                            }
                        }
                    }
                    PulseEvent::TrackUnavailable(id) => {
                        if let Some(track) =
                            self.available_screen_tracks.iter().find(|t| t.id == id)
                        {
                            let session_id = track.session_id.clone();
                            if let Some(ref mut call) = self.current_call_state {
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
            }
            MainMessage::AudioTrackSubscribed(track_id) => {
                if let Err(e) = self.audio.add_track(track_id) {
                    tracing::warn!("audio add_track: {e:#}");
                }
                if let Err(e) = self.audio.start_playback() {
                    tracing::warn!("audio start_playback: {e:#}");
                }
            }
            MainMessage::AudioPacket(track_id, data) => {
                if let Err(e) = self.audio.feed_packet(&track_id, &data) {
                    tracing::warn!("audio feed_packet ({track_id}): {e:#}");
                }
            }
            MainMessage::VideoTrackSubscribed(track_id) => {
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
                        yield Message::Main(MainMessage::VideoFrameDecoded(
                            tid_stream.clone(),
                            result,
                        ));
                    }
                });
            }
            MainMessage::VideoPacket(track_id, data) => {
                if let Some(tx) = self.video_decode_tx.get(&track_id) {
                    let _ = tx.send(data);
                }
            }
            MainMessage::VideoFrameDecoded(track_id, result) => match result {
                Ok((width, height, rgba)) => {
                    let handle = ImageHandle::from_rgba(width, height, rgba.clone());
                    self.video_handles.insert(track_id.clone(), handle);

                    let frame = crate::widgets::remote_screen::RemoteScreenFrame {
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
            MainMessage::MicEnabled(handle) => {
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
                            self.error = Some(RenderableError::UnknownError(format!(
                                "Microphone error: {e}"
                            )));
                        }
                    }
                }
            }
            MainMessage::CameraTrackStarted(handle) => {
                self.camera_track = Some(handle);
            }
            MainMessage::ScreenTrackStarted(handle) => {
                self.screen_track = Some(handle);
            }
            MainMessage::ToggleEmojiPicker => self.emoji_picker_open = !self.emoji_picker_open,
            MainMessage::EmojiPickerDismiss => self.emoji_picker_open = false,
            MainMessage::EmojiSelected(emoji) => {
                self.chat_input.push_str(&emoji);
            }
            MainMessage::EmojiCategorySelected(group) => {
                self.emoji_picker_category = group;
                self.emoji_search.clear();
            }
            MainMessage::EmojiSearchChanged(s) => self.emoji_search = s,
            MainMessage::ToggleChatList => self.chat_list_visible = !self.chat_list_visible,
            MainMessage::ToggleAvatarMenu => self.avatar_menu_open = !self.avatar_menu_open,
            MainMessage::AvatarMenuDismiss => self.avatar_menu_open = false,
            MainMessage::AvatarMenuAction(action) => {
                self.avatar_menu_open = false;
                match action {
                    AvatarAction::Settings => return Task::done(Message::OpenSettings),
                    AvatarAction::Logout => return Task::done(Message::Logout),
                    _ => {}
                }
            }
            MainMessage::OpenSettings => return Task::done(Message::OpenSettings),
            MainMessage::MessagesLoaded(id, messages) => {
                self.conversation_messages.put(id.clone(), messages.clone());
                if self.current_conversation.as_ref() == Some(&id) {
                    self.current_conversation_messages = messages;
                }
            }
            MainMessage::ApiError(e) => {
                self.error = Some(e);
            }
            MainMessage::DismissError => {
                self.error = None;
            }
            MainMessage::ContactsLoaded(contacts) => {
                self.contacts = contacts;
                self.contacts_loaded = true;
            }
            MainMessage::AddContactInputChanged(s) => self.add_contact_input = s,
            MainMessage::AddContactSubmit => {
                let username = self.add_contact_input.trim().to_string();
                if !username.is_empty() {
                    self.add_contact_input.clear();
                    let client = self.api.clone();
                    return Task::perform(
                        async move {
                            let user = client.get_user_profile_by_username(&username).await?;
                            client
                                .add_contact(ContactAction::Request { user_id: user.id })
                                .await
                        },
                        |result| match result {
                            Ok(contact) => Message::Main(MainMessage::ContactAdded(contact)),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                }
            }
            MainMessage::ContactAdded(contact) => {
                if !self
                    .contacts
                    .iter()
                    .any(|c| c.profile.id == contact.profile.id)
                {
                    self.contacts.push(contact);
                }
            }
            MainMessage::RemoveContact(user_id) => {
                let client = self.api.clone();
                let uid = user_id.clone();
                return Task::perform(
                    async move { client.remove_contact(&uid).await },
                    move |result| match result {
                        Ok(()) => Message::Main(MainMessage::ContactRemoved(user_id)),
                        Err(e) => Message::Main(MainMessage::ApiError(e)),
                    },
                );
            }
            MainMessage::ContactRemoved(user_id) => {
                self.contacts.retain(|c| c.profile.id != user_id);
            }
            MainMessage::AcceptContact(user_id) => {
                let client = self.api.clone();
                let uid = user_id.clone();
                return Task::perform(
                    async move {
                        client
                            .add_contact(ContactAction::Accept { user_id: uid })
                            .await
                    },
                    move |result| match result {
                        Ok(contact) => Message::Main(MainMessage::ContactAccepted(contact)),
                        Err(e) => Message::Main(MainMessage::ApiError(e)),
                    },
                );
            }
            MainMessage::ContactAccepted(contact) => {
                if let Some(c) = self
                    .contacts
                    .iter_mut()
                    .find(|c| c.profile.id == contact.profile.id)
                {
                    c.status = contact.status;
                }
            }
            MainMessage::BlockContact(user_id) => {
                let client = self.api.clone();
                let uid = user_id.clone();
                return Task::perform(
                    async move { client.block_contact(&uid).await },
                    move |result| match result {
                        Ok(()) => Message::Main(MainMessage::ContactBlocked(user_id)),
                        Err(e) => Message::Main(MainMessage::ApiError(e)),
                    },
                );
            }
            MainMessage::ContactBlocked(user_id) => {
                if let Some(c) = self.contacts.iter_mut().find(|c| c.profile.id == user_id) {
                    c.status = ContactStatus::Blocked;
                }
            }
            MainMessage::UnblockContact(user_id) => {
                let client = self.api.clone();
                let uid = user_id.clone();
                return Task::perform(
                    async move { client.unblock_contact(&uid).await },
                    move |result| match result {
                        Ok(contact) => Message::Main(MainMessage::ContactUnblocked(contact)),
                        Err(e) => Message::Main(MainMessage::ApiError(e)),
                    },
                );
            }
            MainMessage::ContactUnblocked(contact) => {
                if let Some(c) = self
                    .contacts
                    .iter_mut()
                    .find(|c| c.profile.id == contact.profile.id)
                {
                    c.status = ContactStatus::Established;
                }
            }
            MainMessage::OpenPrivateChannel(user_id) => {
                let existing_id = self.conversations.iter().find_map(|(id, ch)| {
                    if let crate::api::Channel::Private { other, .. } = ch {
                        if other.id == user_id {
                            return Some(id.clone());
                        }
                    }
                    None
                });
                if let Some(id) = existing_id {
                    self.active_tab = SidebarTab::Messages;
                    return Task::done(Message::Main(MainMessage::ChatSelected(id)));
                } else {
                    let client = self.api.clone();
                    return Task::perform(
                        async move { client.create_private_channel(&user_id).await },
                        |result| Message::Main(MainMessage::PrivateChannelOpened(result)),
                    );
                }
            }
            MainMessage::PrivateChannelOpened(result) => match result {
                Ok(channel) => {
                    let id = channel.id();
                    self.conversations.insert(id.clone(), channel);
                    self.active_tab = SidebarTab::Messages;
                    return Task::done(Message::Main(MainMessage::ChatSelected(id)));
                }
                Err(e) => {
                    return Task::done(Message::Main(MainMessage::ApiError(e)));
                }
            },
            MainMessage::ConsumeScreenTrack(track_id) => {
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
                                    yield Message::Main(MainMessage::ApiError(
                                        RenderableError::UnknownError(format!(
                                            "Failed to consume screen track: {e}"
                                        )),
                                    ));
                                    yield Message::Main(MainMessage::StopViewingScreenTrack);
                                    return;
                                }
                            };
                            yield Message::Main(MainMessage::VideoTrackSubscribed(
                                drain_tid.clone(),
                            ));

                            while let Some(frame) = rx.recv().await {
                                if let Some((_codec, data)) = codec::strip_codec_byte(&frame.data) {
                                    yield Message::Main(MainMessage::VideoPacket(
                                        drain_tid.clone(),
                                        data.to_vec(),
                                    ));
                                }
                            }
                        });
                    }
                }
            }
            MainMessage::StopViewingScreenTrack => {
                if let Some(track_id) = self.clear_screen_view_state() {
                    if let Some(pulse) = self.pulse_client.clone() {
                        if let Err(e) = pulse.stop_consuming(track_id) {
                            tracing::warn!("stop_consuming screen track: {e:#}");
                        }
                    }
                }
            }
            MainMessage::ToggleScreenshareFullscreen => {
                self.screenshare_fullscreen = !self.screenshare_fullscreen;
            }
            MainMessage::RequestScreenKeyframe => {
                if let Some(ref flag) = self.screen_keyframe_request {
                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            MainMessage::ScreenCaptureError(msg) => {
                self.error = Some(RenderableError::UnknownError(msg));
            }
        }
        Task::none()
    }

    fn handle_lifecycle_event(&mut self, event: LifecycleEvent) -> Task<Message> {
        match event {
            LifecycleEvent::Disconnected => {
                self.error = Some(RenderableError::NetworkError);
            }
            LifecycleEvent::Reconnected => {
                self.error = None;
            }
            LifecycleEvent::ReconnectionFailed { .. } => {
                self.error = Some(RenderableError::NetworkError);
            }
            LifecycleEvent::Connected | LifecycleEvent::Reconnecting { .. } => {}
        }
        Task::none()
    }

    fn handle_server_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::NewMessage(e) => {
                let channel_id = e.channel_id.clone();
                let msg = e.message;
                let api = self.api.clone();
                return Task::perform(
                    async move {
                        let api_msg = api.map_message(&msg).await?;
                        let time = Ulid::from_string(&api_msg.id)
                            .map(|u| {
                                u.datetime()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_millis() as i64
                            })
                            .unwrap_or_else(|_| chrono::Utc::now().timestamp_millis());
                        Ok((
                            channel_id,
                            ChatMessage {
                                id: api_msg.id.clone(),
                                user: api_msg.author,
                                time,
                                formatted_time: format_message_time(time),
                                content: match api_msg.content {
                                    ApiMessageContent::Text(t) => MessageContent::Text(t),
                                    ApiMessageContent::CallCard { channel, duration } => {
                                        MessageContent::CallCard { channel, duration }
                                    }
                                },
                            },
                        ))
                    },
                    |result: crate::errors::RenderableResult<_>| match result {
                        Ok((channel_id, chat_msg)) => {
                            Message::Main(MainMessage::NewMessageDecrypted(channel_id, chat_msg))
                        }
                        Err(e) => Message::Main(MainMessage::ApiError(e)),
                    },
                );
            }
            Event::MessageEdited(_e) => {
                // TODO:
            }
            Event::MessageDeleted(_e) => {
                // TODO:
            }
            Event::ChannelUpdated(e) => {
                let ch = &e.channel;
                let id = ch.id().to_string();
                match ch {
                    harmony_api::Channel::PrivateChannel {
                        initiator_id,
                        target_id,
                        ..
                    } => {
                        let other_id = if *initiator_id == self.current_user.profile.id {
                            target_id
                        } else {
                            initiator_id
                        };
                        let profile = placeholder_profile(&other_id);
                        self.conversations.insert(
                            id.clone(),
                            crate::api::Channel::Private { id, other: profile },
                        );
                    }
                    harmony_api::Channel::GroupChannel { members, .. } => {
                        let profiles = members.iter().map(|m| placeholder_profile(&m.id)).collect();
                        self.conversations.insert(
                            id.clone(),
                            crate::api::Channel::Group {
                                id,
                                name: None,
                                participants: profiles,
                            },
                        );
                    }
                }
            }
            Event::ChannelDeleted(e) => {
                self.conversations.remove(&e.channel_id);
                if self.current_conversation.as_ref() == Some(&e.channel_id) {
                    self.current_conversation = None;
                    self.current_conversation_messages.clear();
                }
            }
            Event::MemberJoined(_e) => {
                // TODO: update group channel membership
            }
            Event::MemberLeft(_e) => {
                // TODO: update group channel membership
            }
            Event::UserJoinedCall {
                call_id,
                user_id,
                session_id,
                muted,
                deafened: _,
            } => {
                if self.current_call_id.as_deref() == Some(&call_id) {
                    let client = self.api.clone();
                    let uid = user_id.clone();
                    return Task::perform(
                        async move {
                            let profile = client.get_user_profile(&uid).await?;
                            Ok::<CallParticipant, RenderableError>(CallParticipant {
                                profile,
                                session_id,
                                tracks: CallTrackState {
                                    audio: !muted,
                                    video: false,
                                    screen: false,
                                },
                            })
                        },
                        |result| match result {
                            Ok(participant) => {
                                Message::Main(MainMessage::CallParticipantJoined(participant))
                            }
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                }
            }
            Event::UserLeftCall {
                call_id,
                session_id,
            } => {
                if self.current_call_id.as_deref() == Some(&call_id) {
                    if let Some(ref mut call) = self.current_call_state {
                        call.participants.retain(|p| p.session_id != session_id);
                        if call.participants.is_empty() {
                            self.current_call_state = None;
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
            }
            Event::UserVoiceStateChanged {
                call_id,
                session_id,
                muted,
                deafened: _,
            } => {
                if self.current_call_id.as_deref() == Some(&call_id) {
                    if let Some(ref mut call) = self.current_call_state {
                        if let Some(p) = call
                            .participants
                            .iter_mut()
                            .find(|p| p.session_id == session_id)
                        {
                            p.tracks.audio = !muted;
                        }
                    }
                }
            }
            Event::CallMigrated {
                call_id,
                server_address: _,
            } => {
                // reconnect to new server
                if self.current_call_id.as_deref() == Some(&call_id)
                    && let Some(conv_id) = self.current_call.clone()
                {
                    if let Some(ref pulse) = self.pulse_client {
                        pulse.disconnect();
                    }
                    self.pulse_client = None;
                    let client = self.api.clone();
                    return Task::stream(stream! {
                        let token_info = match client.create_call_token(&conv_id).await {
                            Ok(info) => info,
                            Err(e) => {
                                yield Message::Main(MainMessage::ApiError(e));
                                return;
                            }
                        };
                        let (pulse_client, mut event_rx) = match PulseClient::connect(
                            PulseClientOptions {
                                server_url: token_info.server_address,
                                session_id: token_info.session_id,
                                session_token: token_info.token,
                                call_id: token_info.call_id.clone(),
                                identity: client.call_identity().await,
                            },
                        )
                        .await
                        {
                            Ok(result) => result,
                            Err(e) => {
                                yield Message::Main(MainMessage::ApiError(
                                    RenderableError::UnknownError(format!(
                                        "Failed to reconnect to voice server: {e}"
                                    )),
                                ));
                                return;
                            }
                        };
                        let pulse_call_id = token_info.call_id.clone();
                        yield Message::Main(MainMessage::PulseConnected(
                            Arc::new(pulse_client),
                            pulse_call_id,
                        ));
                        let call_state = client.get_call(&conv_id).await.ok().flatten();
                        yield Message::Main(MainMessage::CallStateLoaded(conv_id, call_state));
                        while let Some(event) = event_rx.recv().await {
                            yield Message::Main(MainMessage::PulseEvent(event));
                        }
                        yield Message::Main(MainMessage::PulseDisconnected);
                    });
                }
            }
            Event::ContactStateChanged { user_id, state } => {
                if matches!(state, harmony_api::RelationshipState::None) {
                    self.contacts.retain(|c| c.profile.id != user_id);
                } else if !matches!(
                    state,
                    harmony_api::RelationshipState::PendingKeyExchange { .. }
                        | harmony_api::RelationshipState::Established { .. }
                ) {
                    let new_status = crate::api::map_relationship(&state);
                    if let Some(c) = self.contacts.iter_mut().find(|c| c.profile.id == user_id) {
                        c.status = new_status;
                    } else {
                        let client = self.api.clone();
                        let uid = user_id.clone();
                        return Task::perform(
                            async move { client.get_user_profile(&uid).await },
                            move |result| match result {
                                Ok(profile) => Message::Main(MainMessage::ContactAdded(Contact {
                                    profile,
                                    status: new_status,
                                })),
                                Err(e) => Message::Main(MainMessage::ApiError(e)),
                            },
                        );
                    }
                }
            }
        }
        Task::none()
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

    pub fn remote_screenshare_available(&self) -> Option<&CallParticipant> {
        let call = self.current_call_state.as_ref()?;
        self.available_screen_tracks.iter().find_map(|t| {
            call.participants.iter().find(|p| {
                p.session_id == t.session_id && p.profile.id != self.current_user.profile.id
            })
        })
    }

    pub fn pending_screen_track_id(&self) -> Option<&str> {
        self.available_screen_tracks.first().map(|t| t.id.as_str())
    }

    pub fn has_active_screenshare(&self) -> bool {
        self.is_local_screensharing() || !self.available_screen_tracks.is_empty()
    }

    pub fn view(&self) -> Element<MainMessage> {
        let sidebar = sidebar(&self);
        let mut main_row = row![sidebar];
        let error_banner: Option<Element<MainMessage>> = self.error.as_ref().map(|e| {
            container(
                row![
                    text(e.to_string())
                        .size(14)
                        .font(DM_SANS)
                        .color(iced::Color::WHITE),
                    Space::new().width(Length::Fill),
                    button(
                        text(Icon::DismissFilled.unicode())
                            .size(12)
                            .font(FLUENT_ICONS)
                    )
                    .on_press(MainMessage::DismissError)
                    .style(|_theme, _status| button::Style {
                        background: None,
                        text_color: iced::Color::WHITE,
                        ..Default::default()
                    }),
                ]
                .align_y(iced::Alignment::Center)
                .spacing(8),
            )
            .padding(iced::Padding::from([6, 12]))
            .width(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.8, 0.15, 0.15,
                ))),
                ..Default::default()
            })
            .into()
        });
        match self.active_tab {
            SidebarTab::People => {
                main_row = main_row.push(people_list(self));
                let content: Element<MainMessage> = main_row
                    .push(
                        container(
                            text("Select a contact to view their profile")
                                .size(18)
                                .color(TEXT_MUTED)
                                .font(DM_SANS),
                        )
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                        .style(|_theme| container::Style {
                            background: Some(iced::Background::Color(BG_APP)),
                            ..Default::default()
                        }),
                    )
                    .height(Length::Fill)
                    .width(Length::Fill)
                    .into();

                if let Some(banner) = error_banner {
                    return column![banner, content]
                        .height(Length::Fill)
                        .width(Length::Fill)
                        .into();
                } else {
                    return content;
                }
            }
            _ => {
                if self.chat_list_visible {
                    main_row = main_row.push(chat_list(self));
                }
            }
        }

        let content: Element<MainMessage> = if self.current_conversation.is_some() {
            let chat_area = chat_area(&self);
            main_row
                .push(chat_area)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        } else {
            main_row
                .push(
                    container(
                        text("Select a conversation to start chatting")
                            .size(18)
                            .color(TEXT_MUTED)
                            .font(DM_SANS),
                    )
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .style(|_theme| container::Style {
                        background: Some(iced::Background::Color(BG_APP)),
                        ..Default::default()
                    }),
                )
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        };

        if let Some(banner) = error_banner {
            column![banner, content]
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        } else {
            content
        }
    }
}
