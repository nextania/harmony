use std::{collections::HashMap, sync::Arc};

use harmony_api::{AddContactOutcome, Channel, EncryptedClient, EncryptedEvent, LifecycleEvent};
use iced::{
    Element, Length, Task,
    widget::{Space, button, column, container, row, text},
};

use crate::{
    ChatMessage, Message,
    api::{UserProfile, placeholder_profile},
    errors::RenderableError,
    icons::{FLUENT_ICONS, Icon},
    theme::{BG_APP, DM_SANS, TEXT_MUTED},
    views::main::{
        call::{CallContext, CallMessage, CallParticipant, CallSession},
        chat_area::chat_area,
        chat_list::chat_list,
        contacts::{ContactsMessage, ContactsState},
        people_list::people_list,
        sidebar::sidebar,
    },
};

pub mod call;
pub mod chat_area;
pub mod chat_list;
pub mod contacts;
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
    ServerEvent(harmony_api::EncryptedEvent),
    Ignore,
    Call(CallMessage),
    Contacts(ContactsMessage),
    ToggleChatList,
    ToggleAvatarMenu,
    AvatarMenuDismiss,
    AvatarMenuAction(AvatarAction),
    OpenSettings,
    MessagesLoaded(String, Vec<ChatMessage>),
    ApiError(RenderableError),
    DismissError,
    ToggleEmojiPicker,
    EmojiPickerDismiss,
    EmojiSelected(String),
    EmojiCategorySelected(emojis::Group),
    EmojiSearchChanged(String),
    OpenPrivateChannel(String),
    PrivateChannelOpened(crate::errors::RenderableResult<Channel>),
    UsersFetched,
    ContactOutcome(AddContactOutcome),
}

fn decode_content(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).into_owned()
}

pub struct MainView {
    active_tab: SidebarTab,
    chat_mode: ChatMode,
    api: Arc<EncryptedClient>,
    pub chat_input: String,
    pub search_input: String,
    pub current_channels: HashMap<String, Channel>,
    pub current_conversation: Option<String>,
    pub current_user_id: String,
    pub chat_list_visible: bool,
    pub avatar_menu_open: bool,

    pub current_conversation_messages: Vec<ChatMessage>,

    pub emoji_picker_open: bool,
    pub emoji_picker_category: emojis::Group,
    pub emoji_search: String,

    // TODO:
    pub call: CallSession,
    pub contacts: ContactsState,

    pub error: Option<RenderableError>,
}

impl MainView {
    pub fn new(api: Arc<EncryptedClient>, channels: HashMap<String, Channel>) -> Self {
        let current_user_id = api.user_id().to_string();
        Self {
            active_tab: SidebarTab::Messages,
            chat_mode: ChatMode::Text,
            api,
            chat_input: String::new(),
            search_input: String::new(),
            current_channels: channels,
            current_conversation: None,
            current_user_id,
            chat_list_visible: true,
            avatar_menu_open: false,
            current_conversation_messages: Vec::new(),
            emoji_picker_open: false,
            emoji_picker_category: emojis::Group::SmileysAndEmotion,
            emoji_search: String::new(),
            call: CallSession::new(),
            contacts: ContactsState::default(),
            error: None,
        }
    }

    pub fn update(&mut self, message: MainMessage) -> Task<Message> {
        match message {
            MainMessage::Call(m) => {
                return self.call.update(
                    m,
                    CallContext {
                        api: &self.api,
                        current_conversation: self.current_conversation.as_deref(),
                        self_user_id: &self.current_user_id,
                    },
                );
            }
            MainMessage::Contacts(m) => {
                return self.contacts.update(m, &self.api);
            }
            MainMessage::TabSelected(tab) => {
                self.active_tab = tab;
                if matches!(self.active_tab, SidebarTab::People) && !self.contacts.loaded {
                    return ContactsState::load_task(&self.api);
                }
            }
            MainMessage::ChatModeSelected(mode) => {
                if self.chat_mode == mode {
                    return Task::none();
                }
                self.chat_mode = mode;
                if matches!(self.chat_mode, ChatMode::Voice)
                    && let Some(conv_id) = self.current_conversation.clone()
                {
                    return call::load_call_state_task(self.api.clone(), conv_id);
                }
            }
            MainMessage::ChatSelected(i) => {
                if self.current_conversation.as_ref() == Some(&i) {
                    return Task::none();
                }
                self.current_conversation = Some(i.clone());
                self.current_conversation_messages = vec![];
                if self.call.channel_id.is_none() {
                    self.call.state = None;
                }

                let call_task = call::load_call_state_task(self.api.clone(), i.clone());

                let client = self.api.clone();
                let msg_task = Task::perform(
                    async move {
                        let channel = client.channels().fetch(&i).await?;
                        let messages = channel
                            .messages()
                            .await?
                            .into_iter()
                            .map(|m| ChatMessage::new(&m.message, decode_content(m.content)))
                            .collect();
                        Ok((i, messages))
                    },
                    |result| match result {
                        Ok((conv_id, messages)) => {
                            Message::Main(MainMessage::MessagesLoaded(conv_id, messages))
                        }
                        Err(e) => Message::Main(MainMessage::ApiError(e)),
                    },
                );
                return Task::batch([msg_task, call_task]);
            }
            MainMessage::ChatInputChanged(s) => self.chat_input = s,
            MainMessage::SearchInputChanged(s) => self.search_input = s,
            MainMessage::SendMessage => {
                if !self.chat_input.is_empty()
                    && let Some(conv_id) = &self.current_conversation
                {
                    let client = self.api.clone();
                    let channel_id = conv_id.clone();
                    let content = self.chat_input.clone();
                    self.chat_input.clear();
                    return Task::perform(
                        async move {
                            let channel = client.channels().fetch(&channel_id).await?;
                            let msg = channel.send_message(content.as_bytes()).await?;
                            Ok(ChatMessage::new(&msg, content))
                        },
                        move |result: crate::errors::RenderableResult<_>| match result {
                            Ok(chat_msg) => Message::Main(MainMessage::MessageSent(chat_msg)),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                }
            }
            MainMessage::MessageSent(msg) => {
                if self.current_conversation.is_some() {
                    self.current_conversation_messages.push(msg);
                }
            }
            MainMessage::EditMessage(message_id, new_content) => {
                if let Some(conv_id) = &self.current_conversation {
                    let client = self.api.clone();
                    let channel_id = conv_id.clone();
                    let mid = message_id.clone();
                    return Task::perform(
                        async move {
                            let channel = client.channels().fetch(&channel_id).await?;
                            let msg = channel.edit_message(&mid, new_content.as_bytes()).await?;
                            Ok(ChatMessage::new(&msg, new_content))
                        },
                        move |result: crate::errors::RenderableResult<_>| match result {
                            Ok(chat_msg) => Message::Main(MainMessage::MessageEdited(
                                message_id.clone(),
                                chat_msg,
                            )),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                }
            }
            MainMessage::MessageEdited(message_id, updated_msg) => {
                if let Some(m) = self
                    .current_conversation_messages
                    .iter_mut()
                    .find(|m| m.id == message_id)
                {
                    *m = updated_msg;
                }
            }
            MainMessage::DeleteMessage(message_id) => {
                if let Some(conv_id) = &self.current_conversation {
                    let client = self.api.clone();
                    let mid = message_id.clone();
                    let cid = conv_id.clone();
                    let channel_id = cid.clone();
                    return Task::perform(
                        async move {
                            let channel = client.channels().fetch(&channel_id).await?;
                            channel.delete_message(&mid).await?;
                            Ok::<(), RenderableError>(())
                        },
                        move |result| match result {
                            Ok(()) => Message::Main(MainMessage::MessageDeleted(
                                message_id.clone(),
                                cid.clone(),
                            )),
                            Err(e) => Message::Main(MainMessage::ApiError(e)),
                        },
                    );
                }
            }
            MainMessage::MessageDeleted(message_id, channel_id) => {
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    self.current_conversation_messages
                        .retain(|m| m.id != message_id);
                }
            }
            MainMessage::ServerEvent(event) => {
                tracing::info!("Received client event: {:?}", event);
                return self.handle_client_event(event);
            }
            MainMessage::ContactOutcome(outcome) => {
                return self.contacts.update(
                    ContactsMessage::Accepted(contacts::Contact::from_outcome(outcome)),
                    &self.api,
                );
            }
            MainMessage::Ignore => {}
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
                let mut missing: Vec<String> = messages
                    .iter()
                    .map(|m| m.author_id.clone())
                    .filter(|a| self.api.users().get(a).is_none())
                    .collect();
                missing.sort();
                missing.dedup();
                if self.current_conversation.as_ref() == Some(&id) {
                    self.current_conversation_messages = messages;
                }
                return fetch_users_task(self.api.clone(), missing);
            }
            MainMessage::UsersFetched => {}
            MainMessage::ApiError(e) => {
                self.error = Some(e);
            }
            MainMessage::DismissError => {
                self.error = None;
            }
            MainMessage::OpenPrivateChannel(user_id) => {
                let existing_id =
                    self.current_channels
                        .iter()
                        .find_map(|(id, ch)| match ch.data() {
                            harmony_api::ChannelData::PrivateChannel {
                                initiator_id,
                                target_id,
                                ..
                            } if *initiator_id == user_id || *target_id == user_id => {
                                Some(id.clone())
                            }
                            _ => None,
                        });
                if let Some(id) = existing_id {
                    self.active_tab = SidebarTab::Messages;
                    return Task::done(Message::Main(MainMessage::ChatSelected(id)));
                } else {
                    let client = self.api.clone();
                    return Task::perform(
                        async move {
                            let channel =
                                client.channels().create_private_channel(&user_id).await?;
                            let _ = client.users().fetch_bulk(vec![user_id]).await;
                            Ok(channel)
                        },
                        |result| Message::Main(MainMessage::PrivateChannelOpened(result)),
                    );
                }
            }
            MainMessage::PrivateChannelOpened(result) => match result {
                Ok(channel) => {
                    let id = channel.id().to_string();
                    self.current_channels.insert(id.clone(), channel);
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

    fn handle_client_event(&mut self, event: EncryptedEvent) -> Task<Message> {
        match event {
            EncryptedEvent::Lifecycle(l) => return self.handle_lifecycle_event(l),
            EncryptedEvent::NewMessage {
                channel_id,
                message,
            } => {
                let chat_msg = ChatMessage::new(&message.message, decode_content(message.content));
                let author_id = chat_msg.author_id.clone();
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    self.current_conversation_messages.push(chat_msg);
                }
                if self.api.users().get(&author_id).is_none() {
                    return fetch_users_task(self.api.clone(), vec![author_id]);
                }
            }
            EncryptedEvent::MessageEdited {
                channel_id,
                message,
            } => {
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    let chat_msg =
                        ChatMessage::new(&message.message, decode_content(message.content));
                    if let Some(m) = self
                        .current_conversation_messages
                        .iter_mut()
                        .find(|m| m.id == chat_msg.id)
                    {
                        *m = chat_msg;
                    }
                }
            }
            EncryptedEvent::MessageDeleted {
                channel_id,
                message_id,
            } => {
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    self.current_conversation_messages
                        .retain(|m| m.id != message_id);
                }
            }
            EncryptedEvent::ChannelUpdated { channel } => {
                let member_ids: Vec<String> = match channel.data() {
                    harmony_api::ChannelData::PrivateChannel {
                        initiator_id,
                        target_id,
                        ..
                    } => vec![initiator_id.clone(), target_id.clone()],
                    harmony_api::ChannelData::GroupChannel { members, .. } => {
                        members.iter().map(|m| m.id.clone()).collect()
                    }
                };
                self.current_channels
                    .insert(channel.id().to_string(), channel);
                let missing: Vec<String> = member_ids
                    .into_iter()
                    .filter(|id| self.api.users().get(id).is_none())
                    .collect();
                return fetch_users_task(self.api.clone(), missing);
            }
            EncryptedEvent::ChannelDeleted { channel_id } => {
                self.current_channels.remove(&channel_id);
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    self.current_conversation = None;
                    self.current_conversation_messages.clear();
                }
            }
            EncryptedEvent::MemberJoined { .. } => {
                // TODO: update group channel membership
            }
            EncryptedEvent::MemberLeft { .. } => {
                // TODO: update group channel membership
            }
            EncryptedEvent::UserJoinedCall(e) => {
                return self.call.on_user_joined(
                    &e.call_id,
                    e.user_id,
                    e.session_id,
                    e.muted,
                    &self.api,
                );
            }
            EncryptedEvent::UserLeftCall(e) => {
                self.call.on_user_left(&e.call_id, &e.session_id);
            }
            EncryptedEvent::UserVoiceStateChanged(e) => {
                self.call
                    .on_voice_state_changed(&e.call_id, &e.session_id, e.muted);
            }
            EncryptedEvent::CallMigrated(e) => {
                return self.call.on_call_migrated(&e.call_id, &self.api);
            }
            EncryptedEvent::ContactStateChanged { user_id, state } => {
                return self.contacts.on_state_changed(user_id, &state, &self.api);
            }
        }
        Task::none()
    }

    pub fn is_local_screensharing(&self) -> bool {
        self.call.is_local_screensharing()
    }

    pub fn is_consuming_remote_screenshare(&self) -> bool {
        self.call.is_consuming_remote_screenshare()
    }

    pub fn remote_screenshare_available(&self) -> Option<&CallParticipant> {
        self.call
            .remote_screenshare_available(&self.current_user_id)
    }

    pub fn profile(&self, user_id: &str) -> UserProfile {
        self.api
            .users()
            .get(user_id)
            .map(|u| UserProfile::from(&u))
            .unwrap_or_else(|| placeholder_profile(user_id))
    }

    pub fn pending_screen_track_id(&self) -> Option<&str> {
        self.call.pending_screen_track_id()
    }

    pub fn has_active_screenshare(&self) -> bool {
        self.call.has_active_screenshare()
    }

    pub fn view(&self) -> Element<MainMessage> {
        let sidebar = sidebar(self);
        let mut main_row = row![sidebar];
        let error_banner: Option<Element<MainMessage>> = self.error.as_ref().map(|e| {
            container(
                row![
                    text(e.friendly())
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
            let chat_area = chat_area(self);
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

pub fn fetch_users_task(api: Arc<EncryptedClient>, user_ids: Vec<String>) -> Task<Message> {
    if user_ids.is_empty() {
        return Task::none();
    }
    Task::perform(
        async move { api.users().fetch_bulk(user_ids).await },
        |result| match result {
            Ok(_) => Message::Main(MainMessage::UsersFetched),
            Err(_) => Message::Main(MainMessage::Ignore),
        },
    )
}
