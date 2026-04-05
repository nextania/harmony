use std::{collections::HashMap, num::NonZero, sync::Arc, time::UNIX_EPOCH};

use async_stream::stream;
use harmony_api::Event;
use iced::{
    Element, Length, Task,
    widget::{Space, button, column, container, row, text},
};
use lru::LruCache;
use pulse_api::{MediaHint, PulseClient, PulseClientOptions, PulseEvent};
use ulid::Ulid;

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
use crate::media::{audio::AudioPipeline, codec};

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
    ServerEvent(harmony_api::Event),
    JoinCall,
    StartCall,
    LeaveCall,
    ToggleMic,
    ToggleCamera,
    ToggleScreenShare,
    CallStateLoaded(String, Option<CallState>),
    CallParticipantJoined(CallParticipant),
    PulseConnected(Arc<PulseClient>, String),
    PulseDisconnected,
    PulseEvent(PulseEvent),
    AudioTrackSubscribed(String),
    AudioPacket(String, Vec<u8>),
    MicEnabled,
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
    pub audio: AudioPipeline,

    pub current_conversation_messages: Vec<ChatMessage>,

    pub emoji_picker_open: bool,
    pub emoji_picker_category: emojis::Group,
    pub emoji_search: String,

    pub contacts: Vec<Contact>,
    pub contacts_loaded: bool,
    pub add_contact_input: String,

    pub error: Option<RenderableError>,
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
            audio: AudioPipeline::new().expect("audio pipeline init"),
            current_conversation_messages: Vec::new(),
            emoji_picker_open: false,
            emoji_picker_category: emojis::Group::SmileysAndEmotion,
            emoji_search: String::new(),
            contacts: Vec::new(),
            contacts_loaded: false,
            add_contact_input: String::new(),
            error: None,
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
                tracing::info!("Received server event: {:?}", event);
                return self.handle_server_event(event);
            }
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
                self.audio.stop_playback();
                self.audio.stop_capture();
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
                            return Task::perform(
                                async move {
                                    client
                                        .update_voice_state(&conv_id, Some(muted), None)
                                        .await?;
                                    if let Some(pulse) = pulse {
                                        if new_audio {
                                            pulse
                                                .produce_track(
                                                    "audio".to_string(),
                                                    MediaHint::Audio,
                                                )
                                                .await
                                                .map_err(|e| {
                                                    RenderableError::UnknownError(format!(
                                                        "Failed to produce audio track: {e}"
                                                    ))
                                                })?;
                                        } else {
                                            let _ = pulse.stop_producing("audio".to_string()).await;
                                        }
                                    }
                                    Ok::<bool, RenderableError>(new_audio)
                                },
                                |result| match result {
                                    Ok(true) => Message::Main(MainMessage::MicEnabled),
                                    Ok(false) => Message::Main(MainMessage::DismissError),
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
                            return Task::perform(
                                async move {
                                    if new_video {
                                        pulse
                                            .produce_track(
                                                "video".to_string(),
                                                pulse_api::MediaHint::Video,
                                            )
                                            .await
                                    } else {
                                        pulse.stop_producing("video".to_string()).await
                                    }
                                },
                                |result| match result {
                                    Ok(()) => Message::Main(MainMessage::DismissError),
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
                if let Some(ref mut call) = self.current_call_state {
                    if let Some(p) = call
                        .participants
                        .iter_mut()
                        .find(|p| p.profile.id == self.current_user.profile.id)
                    {
                        let new_screen = !p.tracks.screen;
                        p.tracks.screen = new_screen;
                        if let Some(pulse) = self.pulse_client.clone() {
                            return Task::perform(
                                async move {
                                    if new_screen {
                                        pulse
                                            .produce_track(
                                                "screen".to_string(),
                                                pulse_api::MediaHint::ScreenVideo,
                                            )
                                            .await
                                    } else {
                                        pulse.stop_producing("screen".to_string()).await
                                    }
                                },
                                |result| match result {
                                    Ok(()) => Message::Main(MainMessage::DismissError),
                                    Err(e) => Message::Main(MainMessage::ApiError(
                                        RenderableError::UnknownError(format!(
                                            "Screen share error: {e}"
                                        )),
                                    )),
                                },
                            );
                        }
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
            MainMessage::CallParticipantJoined(participant) => {
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
                self.audio.stop_playback();
                self.audio.stop_capture();
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
                    PulseEvent::Disconnected { reconnect } => {
                        if reconnect.is_none() {
                            self.pulse_client = None;
                            self.audio.stop_playback();
                            self.audio.stop_capture();
                            self.current_call = None;
                            self.current_call_id = None;
                        }
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
                                    while let Some(packet) = rx.recv().await {
                                        if let Some((_codec, data)) =
                                            codec::strip_codec_byte(&packet)
                                        {
                                            yield Message::Main(MainMessage::AudioPacket(
                                                track_id.clone(),
                                                data.to_vec(),
                                            ));
                                        }
                                    }
                                });
                            } else {
                                return Task::perform(
                                    async move { pulse.consume_track(&track).await },
                                    |result| match result {
                                        Ok(_rx) => {
                                            // TODO: feed into video pipeline
                                            Message::Main(MainMessage::DismissError)
                                        }
                                        Err(e) => Message::Main(MainMessage::ApiError(
                                            RenderableError::UnknownError(format!(
                                                "Failed to consume track: {e}"
                                            )),
                                        )),
                                    },
                                );
                            }
                        }
                    }
                    PulseEvent::TrackUnavailable(id) => {
                        self.audio.remove_track(&id);
                        tracing::info!("Track unavailable: {id}");
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
            MainMessage::MicEnabled => {
                if self.pulse_client.is_some() {
                    match self.audio.start_capture() {
                        Ok(Some(rx)) => {
                            let pulse = self.pulse_client.clone();
                            return Task::stream(stream! {
                                let Some(pulse) = pulse else { return; };
                                let mut rx = rx;
                                while let Some(packet) = rx.recv().await {
                                    if let Err(e) = pulse.send_media("audio", &packet) {
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
            Event::Disconnected => {
                self.error = Some(RenderableError::NetworkError);
            }
            Event::Reconnected => {
                self.error = None;
            }
            Event::ReconnectionFailed { .. } => {
                self.error = Some(RenderableError::NetworkError);
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
            Event::ContactStateChanged { user_id, state } => {
                if matches!(state, harmony_api::RelationshipState::None) {
                    self.contacts.retain(|c| c.profile.id != user_id);
                } else if let harmony_api::RelationshipState::PendingKeyExchange {
                    public_key: Some(public_key),
                    encapsulated: Some(encapsulated),
                } = state
                {
                    let client = self.api.clone();
                    let uid = user_id.clone();
                    return Task::perform(
                        async move {
                            client
                                .add_contact(ContactAction::Finalize {
                                    user_id: uid,
                                    public_key,
                                    encapsulated,
                                })
                                .await
                        },
                        |result| match result {
                            Ok(contact) => Message::Main(MainMessage::ContactAccepted(contact)),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                } else if let harmony_api::RelationshipState::Established {
                    public_key,
                    encapsulated,
                    key_id,
                } = state
                {
                    let client = self.api.clone();
                    let uid = user_id.clone();
                    return Task::perform(
                        async move {
                            client
                                .add_contact(ContactAction::HandleEstablished {
                                    user_id: uid,
                                    public_key,
                                    encapsulated,
                                    key_id,
                                })
                                .await
                        },
                        |result| match result {
                            Ok(contact) => Message::Main(MainMessage::ContactAccepted(contact)),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                } else {
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
            _ => {}
        }
        Task::none()
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
