use std::{collections::HashMap, num::NonZero, sync::Arc};

use harmony_api::{ClientEvent, Event, LifecycleEvent};
use iced::{
    Element, Length, Task,
    widget::{Space, button, column, container, row, text},
};
use lru::LruCache;

use crate::{
    ChatMessage, Message,
    api::{ApiClient, UserProfile, placeholder_profile},
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
    ServerEvent(harmony_api::ClientEvent),
    Ignore,
    Call(CallMessage),
    Contacts(ContactsMessage),
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
    OpenPrivateChannel(String),
    PrivateChannelOpened(crate::errors::RenderableResult<(harmony_api::Channel, Vec<UserProfile>)>),
    ProfilesLoaded(Vec<UserProfile>),
}

pub struct MainView {
    active_tab: SidebarTab,
    chat_mode: ChatMode,
    api: Arc<ApiClient>,
    pub chat_input: String,
    pub search_input: String,
    pub conversations: HashMap<String, harmony_api::Channel>,
    pub current_conversation: Option<String>,
    pub conversation_messages: LruCache<String, Vec<ChatMessage>>,
    pub current_user_id: String,
    pub profiles: HashMap<String, UserProfile>,
    pub chat_list_visible: bool,
    pub avatar_menu_open: bool,

    pub current_conversation_messages: Vec<ChatMessage>,

    pub emoji_picker_open: bool,
    pub emoji_picker_category: emojis::Group,
    pub emoji_search: String,

    pub call: CallSession,
    pub contacts: ContactsState,

    pub error: Option<RenderableError>,
}

impl MainView {
    pub fn new(
        api: Arc<ApiClient>,
        conversations: HashMap<String, harmony_api::Channel>,
        profiles: HashMap<String, UserProfile>,
    ) -> Self {
        let current_user_id = api.user_id().to_string();
        Self {
            active_tab: SidebarTab::Messages,
            chat_mode: ChatMode::Text,
            api,
            chat_input: String::new(),
            search_input: String::new(),
            conversations,
            current_conversation: None,
            conversation_messages: LruCache::new(NonZero::new(100).unwrap()),
            current_user_id,
            profiles,
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
                return self.contacts.update(m, &self.api, &mut self.profiles);
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

                // return a task to load messages for this conversation if not already cached
                let msg_task = if !self.conversation_messages.contains(&i) {
                    let client = self.api.clone();
                    Task::perform(
                        async move {
                            let raw = client.get_messages(&i).await?;
                            let messages = raw
                                .into_iter()
                                .map(|(m, text)| ChatMessage::new(&m, text))
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
                if !self.chat_input.is_empty()
                    && let Some(conv_id) = &self.current_conversation
                {
                    let client = self.api.clone();
                    let channel_id = conv_id.clone();
                    let content = self.chat_input.clone();
                    self.chat_input.clear();
                    return Task::perform(
                        async move {
                            let msg = client.send_message(&channel_id, &content).await?;
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
                let author_id = chat_msg.author_id.clone();
                if let Some(msgs) = self.conversation_messages.get_mut(&channel_id) {
                    msgs.push(chat_msg.clone());
                }
                if self.current_conversation.as_ref() == Some(&channel_id) {
                    self.current_conversation_messages.push(chat_msg);
                }
                if !self.profiles.contains_key(&author_id) {
                    return fetch_profiles_task(self.api.clone(), vec![author_id]);
                }
            }
            MainMessage::EditMessage(message_id, new_content) => {
                if let Some(conv_id) = &self.current_conversation {
                    let client = self.api.clone();
                    let channel_id = conv_id.clone();
                    let mid = message_id.clone();
                    return Task::perform(
                        async move {
                            let msg = client.edit_message(&mid, &channel_id, &new_content).await?;
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
                if let Some(conv_id) = &self.current_conversation {
                    if let Some(msgs) = self.conversation_messages.get_mut(conv_id)
                        && let Some(m) = msgs.iter_mut().find(|m| m.id == message_id)
                    {
                        *m = updated_msg.clone();
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
                        async move {
                            client.client().delete_message(&mid).await?;
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
                                    Ok(Some(outcome)) => Message::Main(MainMessage::Contacts(
                                        ContactsMessage::Accepted(contacts::Contact::from_outcome(
                                            outcome,
                                        )),
                                    )),
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
                    .filter(|a| !self.profiles.contains_key(a))
                    .collect();
                missing.sort();
                missing.dedup();
                self.conversation_messages.put(id.clone(), messages.clone());
                if self.current_conversation.as_ref() == Some(&id) {
                    self.current_conversation_messages = messages;
                }
                return fetch_profiles_task(self.api.clone(), missing);
            }
            MainMessage::ProfilesLoaded(profiles) => {
                self.profiles
                    .extend(profiles.into_iter().map(|p| (p.id.clone(), p)));
            }
            MainMessage::ApiError(e) => {
                self.error = Some(e);
            }
            MainMessage::DismissError => {
                self.error = None;
            }
            MainMessage::OpenPrivateChannel(user_id) => {
                let existing_id = self.conversations.iter().find_map(|(id, ch)| match ch {
                    harmony_api::Channel::PrivateChannel {
                        initiator_id,
                        target_id,
                        ..
                    } if *initiator_id == user_id || *target_id == user_id => Some(id.clone()),
                    _ => None,
                });
                if let Some(id) = existing_id {
                    self.active_tab = SidebarTab::Messages;
                    return Task::done(Message::Main(MainMessage::ChatSelected(id)));
                } else {
                    let client = self.api.clone();
                    return Task::perform(
                        async move {
                            let channel = client.client().create_private_channel(&user_id).await?;
                            let profiles =
                                client.get_profiles(vec![user_id]).await.unwrap_or_default();
                            Ok((channel, profiles))
                        },
                        |result| Message::Main(MainMessage::PrivateChannelOpened(result)),
                    );
                }
            }
            MainMessage::PrivateChannelOpened(result) => match result {
                Ok((channel, profiles)) => {
                    self.profiles
                        .extend(profiles.into_iter().map(|p| (p.id.clone(), p)));
                    let id = channel.id().to_string();
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
                        let text = api.decrypt_message(&msg).await?;
                        Ok((channel_id, ChatMessage::new(&msg, text)))
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
                let ch = e.channel;
                let member_ids: Vec<String> = match &ch {
                    harmony_api::Channel::PrivateChannel {
                        initiator_id,
                        target_id,
                        ..
                    } => vec![initiator_id.clone(), target_id.clone()],
                    harmony_api::Channel::GroupChannel { members, .. } => {
                        members.iter().map(|m| m.id.clone()).collect()
                    }
                };
                self.conversations.insert(ch.id().to_string(), ch);
                let missing: Vec<String> = member_ids
                    .into_iter()
                    .filter(|id| !self.profiles.contains_key(id))
                    .collect();
                return fetch_profiles_task(self.api.clone(), missing);
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
                return self
                    .call
                    .on_user_joined(&call_id, user_id, session_id, muted, &self.api);
            }
            Event::UserLeftCall {
                call_id,
                session_id,
            } => {
                self.call.on_user_left(&call_id, &session_id);
            }
            Event::UserVoiceStateChanged {
                call_id,
                session_id,
                muted,
                deafened: _,
            } => {
                self.call
                    .on_voice_state_changed(&call_id, &session_id, muted);
            }
            Event::CallMigrated {
                call_id,
                server_address: _,
            } => {
                return self.call.on_call_migrated(&call_id, &self.api);
            }
            Event::ContactStateChanged { user_id, state } => {
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
        self.profiles
            .get(user_id)
            .cloned()
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

pub fn fetch_profiles_task(api: Arc<ApiClient>, user_ids: Vec<String>) -> Task<Message> {
    if user_ids.is_empty() {
        return Task::none();
    }
    Task::perform(
        async move { api.get_profiles(user_ids).await },
        |result| match result {
            Ok(profiles) => Message::Main(MainMessage::ProfilesLoaded(profiles)),
            Err(_) => Message::Main(MainMessage::Ignore),
        },
    )
}
