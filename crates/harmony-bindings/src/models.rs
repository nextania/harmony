#[derive(Clone, Debug, uniffi::Record)]
pub struct User {
    pub id: String,
    pub profile_banner: Option<String>,
    pub profile_description: String,
    pub username: String,
    pub discriminator: String,
    pub profile_picture: Option<String>,
    pub presence: Option<Presence>,
    pub contacts: Vec<Contact>,
}

impl From<harmony_api::User> for User {
    fn from(user: harmony_api::User) -> Self {
        Self {
            id: user.id,
            profile_banner: user.profile_banner,
            profile_description: user.profile_description,
            username: user.username,
            discriminator: user.discriminator,
            profile_picture: user.profile_picture,
            presence: user.presence.map(Into::into),
            contacts: user.contacts.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Status {
    Online,
    Idle,
    Busy,
    BusyNotify,
    Invisible,
}

impl From<harmony_api::Status> for Status {
    fn from(status: harmony_api::Status) -> Self {
        match status {
            harmony_api::Status::Online => Status::Online,
            harmony_api::Status::Idle => Status::Idle,
            harmony_api::Status::Busy => Status::Busy,
            harmony_api::Status::BusyNotify => Status::BusyNotify,
            harmony_api::Status::Invisible => Status::Invisible,
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

#[derive(Clone, Debug, uniffi::Enum)]
pub enum Relationship {
    Established,
    Blocked,
    Requested,
    Pending,
}

impl From<harmony_api::Relationship> for Relationship {
    fn from(relationship: harmony_api::Relationship) -> Self {
        match relationship {
            harmony_api::Relationship::Established => Relationship::Established,
            harmony_api::Relationship::Blocked => Relationship::Blocked,
            harmony_api::Relationship::Requested => Relationship::Requested,
            harmony_api::Relationship::Pending => Relationship::Pending,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Contact {
    pub id: String,
    pub relationship: Relationship,
}

impl From<harmony_api::Contact> for Contact {
    fn from(contact: harmony_api::Contact) -> Self {
        Self {
            id: contact.id,
            relationship: contact.relationship.into(),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ContactExtended {
    pub id: String,
    pub relationship: Relationship,
    pub user: User,
}

impl From<harmony_api::ContactExtended> for ContactExtended {
    fn from(contact: harmony_api::ContactExtended) -> Self {
        Self {
            id: contact.id,
            relationship: contact.relationship.into(),
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
pub enum Channel {
    PrivateChannel {
        id: String,
        initiator_id: String,
        target_id: String,
    },
    GroupChannel {
        id: String,
        name: String,
        description: String,
        members: Vec<ChannelMember>,
        blacklist: Vec<String>,
    },
}

impl From<harmony_api::Channel> for Channel {
    fn from(channel: harmony_api::Channel) -> Self {
        match channel {
            harmony_api::Channel::PrivateChannel {
                id,
                initiator_id,
                target_id,
            } => Channel::PrivateChannel {
                id,
                initiator_id,
                target_id,
            },
            harmony_api::Channel::GroupChannel {
                id,
                name,
                description,
                members,
                blacklist,
            } => Channel::GroupChannel {
                id,
                name,
                description,
                members: members.into_iter().map(Into::into).collect(),
                blacklist,
            },
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Message {
    pub id: String,
    pub content: String,
    pub author_id: String,
    pub created_at: i64,
    pub edited: bool,
    pub edited_at: Option<i64>,
    pub channel_id: String,
}

impl From<harmony_api::Message> for Message {
    fn from(message: harmony_api::Message) -> Self {
        Self {
            id: message.id,
            content: message.content,
            author_id: message.author_id,
            created_at: message.created_at,
            edited: message.edited,
            edited_at: message.edited_at,
            channel_id: message.channel_id,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct Invite {
    pub id: String,
    pub code: String,
    pub channel_id: String,
    pub creator_id: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
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
            creator_id: invite.creator_id,
            created_at: invite.created_at,
            expires_at: invite.expires_at,
            max_uses: invite.max_uses,
            uses: invite.uses,
            authorized_users: invite.authorized_users,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ActiveCall {
    pub id: String,
    pub channel_id: String,
    pub participants: Vec<String>,
    pub started_at: i64,
}

impl From<harmony_api::ActiveCall> for ActiveCall {
    fn from(call: harmony_api::ActiveCall) -> Self {
        Self {
            id: call.id,
            channel_id: call.channel_id,
            participants: call.participants,
            started_at: call.started_at,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RtcAuthorization {
    pub channel_id: String,
    pub user_id: String,
}

impl From<harmony_api::RtcAuthorization> for RtcAuthorization {
    fn from(auth: harmony_api::RtcAuthorization) -> Self {
        Self {
            channel_id: auth.channel_id,
            user_id: auth.user_id,
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
