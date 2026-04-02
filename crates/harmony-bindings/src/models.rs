#[derive(Clone, Debug, uniffi::Record)]
pub struct UserProfile {
    pub id: String,
    pub presence: Option<Presence>,
}

impl From<harmony_api::UserProfile> for UserProfile {
    fn from(user: harmony_api::UserProfile) -> Self {
        Self {
            id: user.id,
            presence: user.presence.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CurrentUserResponse {
    pub id: String,
    pub encrypted_keys: Option<Vec<u8>>,
    pub presence: Presence,
}

impl From<harmony_api::CurrentUserResponse> for CurrentUserResponse {
    fn from(user: harmony_api::CurrentUserResponse) -> Self {
        Self {
            id: user.id,
            encrypted_keys: user.encrypted_keys,
            presence: user.presence.into(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Status {
    Online,
    Idle,
    Busy,
    BusyNotify,
    Offline,
}

impl From<harmony_api::Status> for Status {
    fn from(status: harmony_api::Status) -> Self {
        match status {
            harmony_api::Status::Online => Status::Online,
            harmony_api::Status::Idle => Status::Idle,
            harmony_api::Status::Busy => Status::Busy,
            harmony_api::Status::BusyNotify => Status::BusyNotify,
            harmony_api::Status::Offline => Status::Offline,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Presence {
    pub status: Status,
    pub message: String,
}

impl From<harmony_api::Presence> for Presence {
    fn from(presence: harmony_api::Presence) -> Self {
        Self {
            status: presence.status.into(),
            message: presence.message,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct UnifiedPublicKey {
    pub x25519: Vec<u8>,
    pub mlkem: Vec<u8>,
}

impl From<harmony_api::UnifiedPublicKey> for UnifiedPublicKey {
    fn from(pk: harmony_api::UnifiedPublicKey) -> Self {
        Self {
            x25519: pk.x25519.to_vec(),
            mlkem: pk.mlkem,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum RelationshipState {
    None,
    Requested {
        public_key: Option<UnifiedPublicKey>,
    },
    PendingKeyExchange {
        public_key: Option<UnifiedPublicKey>,
        encapsulated: Option<Vec<u8>>,
    },
    Established {
        public_key: UnifiedPublicKey,
        encapsulated: Vec<u8>,
        key_id: String,
    },
    Blocked,
}

impl From<harmony_api::RelationshipState> for RelationshipState {
    fn from(state: harmony_api::RelationshipState) -> Self {
        match state {
            harmony_api::RelationshipState::None => RelationshipState::None,
            harmony_api::RelationshipState::Requested { public_key } => {
                RelationshipState::Requested {
                    public_key: public_key.map(Into::into),
                }
            }
            harmony_api::RelationshipState::PendingKeyExchange {
                public_key,
                encapsulated,
            } => RelationshipState::PendingKeyExchange {
                public_key: public_key.map(Into::into),
                encapsulated,
            },
            harmony_api::RelationshipState::Established {
                public_key,
                encapsulated,
                key_id,
            } => RelationshipState::Established {
                public_key: public_key.into(),
                encapsulated,
                key_id,
            },
            harmony_api::RelationshipState::Blocked => RelationshipState::Blocked,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Contact {
    pub id: String,
    pub state: RelationshipState,
}

impl From<harmony_api::Contact> for Contact {
    fn from(contact: harmony_api::Contact) -> Self {
        Self {
            id: contact.id,
            state: contact.state.into(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ContactExtended {
    pub id: String,
    pub state: RelationshipState,
    pub user: UserProfile,
}

impl From<harmony_api::ContactExtended> for ContactExtended {
    fn from(contact: harmony_api::ContactExtended) -> Self {
        Self {
            id: contact.id,
            state: contact.state.into(),
            user: contact.user.into(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ChannelMember {
    pub id: String,
    pub role: ChannelMemberRole,
}

impl From<harmony_api::ChannelMember> for ChannelMember {
    fn from(member: harmony_api::ChannelMember) -> Self {
        Self {
            id: member.id,
            role: member.role.into(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum ChannelMemberRole {
    Member,
    Manager,
}

impl From<harmony_api::ChannelMemberRole> for ChannelMemberRole {
    fn from(role: harmony_api::ChannelMemberRole) -> Self {
        match role {
            harmony_api::ChannelMemberRole::Member => ChannelMemberRole::Member,
            harmony_api::ChannelMemberRole::Manager => ChannelMemberRole::Manager,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum EncryptionHint {
    Mls,
    Persistent,
}

impl From<harmony_api::EncryptionHint> for EncryptionHint {
    fn from(hint: harmony_api::EncryptionHint) -> Self {
        match hint {
            harmony_api::EncryptionHint::Mls => EncryptionHint::Mls,
            harmony_api::EncryptionHint::Persistent => EncryptionHint::Persistent,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Channel {
    PrivateChannel {
        id: String,
        initiator_id: String,
        target_id: String,
        last_key_id: String,
    },
    GroupChannel {
        id: String,
        metadata: Vec<u8>,
        members: Vec<ChannelMember>,
        pending_members: Vec<String>,
        blacklist: Vec<String>,
        encryption_hint: EncryptionHint,
    },
}

impl From<harmony_api::Channel> for Channel {
    fn from(channel: harmony_api::Channel) -> Self {
        match channel {
            harmony_api::Channel::PrivateChannel {
                id,
                initiator_id,
                target_id,
                last_key_id,
            } => Channel::PrivateChannel {
                id,
                initiator_id,
                target_id,
                last_key_id,
            },
            harmony_api::Channel::GroupChannel {
                id,
                metadata,
                members,
                pending_members,
                blacklist,
                encryption_hint,
            } => Channel::GroupChannel {
                id,
                metadata,
                members: members.into_iter().map(Into::into).collect(),
                pending_members,
                blacklist,
                encryption_hint: encryption_hint.into(),
            },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Message {
    pub id: String,
    pub content: Vec<u8>,
    pub author_id: String,
    pub edited_at: Option<i64>,
    pub channel_id: String,
    pub key_id: Option<String>,
}

impl From<harmony_api::Message> for Message {
    fn from(message: harmony_api::Message) -> Self {
        Self {
            id: message.id,
            content: message.content,
            author_id: message.author_id,
            edited_at: message.edited_at,
            channel_id: message.channel_id,
            key_id: message.key_id,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Invite {
    pub id: String,
    pub code: String,
    pub channel_id: String,
    pub creator: String,
    pub expires_at: Option<i64>,
    pub max_uses: Option<i32>,
    pub uses: Vec<String>,
    pub authorized_users: Option<Vec<String>>,
}

impl From<harmony_api::Invite> for Invite {
    fn from(invite: harmony_api::Invite) -> Self {
        Self {
            id: invite.id,
            code: invite.code,
            channel_id: invite.channel_id,
            creator: invite.creator,
            expires_at: invite.expires_at,
            max_uses: invite.max_uses,
            uses: invite.uses,
            authorized_users: invite.authorized_users,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum InviteInformation {
    Group {
        channel_id: String,
        metadata: Vec<u8>,
        inviter_id: String,
        authorized: bool,
        member_count: i32,
    },
    Space {
        name: String,
        description: String,
        inviter_id: String,
        banned: bool,
        authorized: bool,
        member_count: i32,
    },
}

impl From<harmony_api::InviteInformation> for InviteInformation {
    fn from(info: harmony_api::InviteInformation) -> Self {
        match info {
            harmony_api::InviteInformation::Group {
                channel_id,
                metadata,
                inviter_id,
                authorized,
                member_count,
            } => InviteInformation::Group {
                channel_id,
                metadata,
                inviter_id,
                authorized,
                member_count,
            },
            harmony_api::InviteInformation::Space {
                name,
                description,
                inviter_id,
                banned,
                authorized,
                member_count,
            } => InviteInformation::Space {
                name,
                description,
                inviter_id,
                banned,
                authorized,
                member_count,
            },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct StartCallResponse {
    pub id: String,
}

impl From<harmony_api::StartCallResponse> for StartCallResponse {
    fn from(response: harmony_api::StartCallResponse) -> Self {
        Self { id: response.id }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CreateCallTokenResponse {
    pub id: String,
    pub token: String,
    pub server_address: String,
    pub call_id: String,
}

impl From<harmony_api::CreateCallTokenResponse> for CreateCallTokenResponse {
    fn from(response: harmony_api::CreateCallTokenResponse) -> Self {
        Self {
            id: response.id,
            token: response.token,
            server_address: response.server_address,
            call_id: response.call_id,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct UpdateVoiceStateResponse {
    pub muted: bool,
    pub deafened: bool,
}

impl From<harmony_api::UpdateVoiceStateResponse> for UpdateVoiceStateResponse {
    fn from(response: harmony_api::UpdateVoiceStateResponse) -> Self {
        Self {
            muted: response.muted,
            deafened: response.deafened,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Region {
    Canada,
    UsCentral,
    UsEast,
    UsWest,
    Europe,
    Asia,
    SouthAmerica,
    Australia,
    Africa,
}

impl From<Region> for harmony_api::Region {
    fn from(region: Region) -> Self {
        match region {
            Region::Canada => harmony_api::Region::Canada,
            Region::UsCentral => harmony_api::Region::UsCentral,
            Region::UsEast => harmony_api::Region::UsEast,
            Region::UsWest => harmony_api::Region::UsWest,
            Region::Europe => harmony_api::Region::Europe,
            Region::Asia => harmony_api::Region::Asia,
            Region::SouthAmerica => harmony_api::Region::SouthAmerica,
            Region::Australia => harmony_api::Region::Australia,
            Region::Africa => harmony_api::Region::Africa,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CallMember {
    pub user_id: String,
    pub session_id: String,
    pub muted: bool,
    pub deafened: bool,
}

impl From<harmony_api::CallMember> for CallMember {
    fn from(member: harmony_api::CallMember) -> Self {
        Self {
            user_id: member.user_id,
            session_id: member.session_id,
            muted: member.muted,
            deafened: member.deafened,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Event {
    NewMessage {
        message: Message,
        channel_id: String,
    },
    MessageEdited {
        message: Message,
        channel_id: String,
    },
    MessageDeleted {
        message_id: String,
        channel_id: String,
    },
    RemoveContact {
        user_id: String,
    },
    AddContact {
        user_id: String,
    },
    ChannelUpdated {
        channel: Channel,
    },
    ChannelDeleted {
        channel_id: String,
    },
    MemberJoined {
        channel_id: String,
        user_id: String,
    },
    MemberLeft {
        channel_id: String,
        user_id: String,
    },
    Connected,
    Disconnected,
    Reconnecting {
        attempt: u32,
        max_attempts: u32,
    },
    Reconnected,
    ReconnectionFailed {
        attempts: u32,
    },
    UserJoinedCall {
        call_id: String,
        user_id: String,
        session_id: String,
        muted: bool,
        deafened: bool,
    },
    UserLeftCall {
        call_id: String,
        session_id: String,
    },
    UserVoiceStateChanged {
        call_id: String,
        session_id: String,
        muted: bool,
        deafened: bool,
    },
}

impl From<harmony_api::Event> for Event {
    fn from(event: harmony_api::Event) -> Self {
        match event {
            harmony_api::Event::NewMessage(e) => Event::NewMessage {
                message: e.message.into(),
                channel_id: e.channel_id,
            },
            harmony_api::Event::MessageEdited(e) => Event::MessageEdited {
                message: e.message.into(),
                channel_id: e.channel_id,
            },
            harmony_api::Event::MessageDeleted(e) => Event::MessageDeleted {
                message_id: e.message_id,
                channel_id: e.channel_id,
            },
            harmony_api::Event::RemoveContact(id) => Event::RemoveContact { user_id: id },
            harmony_api::Event::AddContact(id) => Event::AddContact { user_id: id },
            harmony_api::Event::ChannelUpdated(e) => Event::ChannelUpdated {
                channel: e.channel.into(),
            },
            harmony_api::Event::ChannelDeleted(e) => Event::ChannelDeleted {
                channel_id: e.channel_id,
            },
            harmony_api::Event::MemberJoined(e) => Event::MemberJoined {
                channel_id: e.channel_id,
                user_id: e.user_id,
            },
            harmony_api::Event::MemberLeft(e) => Event::MemberLeft {
                channel_id: e.channel_id,
                user_id: e.user_id,
            },
            harmony_api::Event::Connected => Event::Connected,
            harmony_api::Event::Disconnected => Event::Disconnected,
            harmony_api::Event::Reconnecting {
                attempt,
                max_attempts,
            } => Event::Reconnecting {
                attempt,
                max_attempts,
            },
            harmony_api::Event::Reconnected => Event::Reconnected,
            harmony_api::Event::ReconnectionFailed { attempts } => {
                Event::ReconnectionFailed { attempts }
            }
            harmony_api::Event::UserJoinedCall {
                call_id,
                user_id,
                session_id,
                muted,
                deafened,
            } => Event::UserJoinedCall {
                call_id,
                user_id,
                session_id,
                muted,
                deafened,
            },
            harmony_api::Event::UserLeftCall {
                call_id,
                session_id,
            } => Event::UserLeftCall {
                call_id,
                session_id,
            },
            harmony_api::Event::UserVoiceStateChanged {
                call_id,
                session_id,
                muted,
                deafened,
            } => Event::UserVoiceStateChanged {
                call_id,
                session_id,
                muted,
                deafened,
            },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ClientOptions {
    pub server_url: String,
    pub token: String,
    pub timeout_seconds: u64,
    pub auto_reconnect: bool,
    pub max_reconnect_attempts: u32,
}

impl ClientOptions {
    #[uniffi::constructor]
    pub fn new(server_url: String, token: String) -> Self {
        Self {
            server_url,
            token,
            timeout_seconds: 30,
            auto_reconnect: true,
            max_reconnect_attempts: 5,
        }
    }

    #[uniffi::method]
    pub fn with_timeout(&self, timeout_seconds: u64) -> Self {
        let mut options = self.clone();
        options.timeout_seconds = timeout_seconds;
        options
    }

    #[uniffi::method]
    pub fn with_auto_reconnect(&self, enabled: bool) -> Self {
        let mut options = self.clone();
        options.auto_reconnect = enabled;
        options
    }

    #[uniffi::method]
    pub fn with_max_reconnect_attempts(&self, attempts: u32) -> Self {
        let mut options = self.clone();
        options.max_reconnect_attempts = attempts;
        options
    }
}

impl From<ClientOptions> for harmony_api::ClientOptions {
    fn from(options: ClientOptions) -> Self {
        harmony_api::ClientOptions::new(options.server_url, options.token)
            .with_timeout(std::time::Duration::from_secs(options.timeout_seconds))
            .with_auto_reconnect(options.auto_reconnect)
            .with_max_reconnect_attempts(options.max_reconnect_attempts)
    }
}
