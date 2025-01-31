
use std::sync::Arc;

use dashmap::{mapref::multiple::RefMulti, DashMap};
use rapid::socket::{emit_one, RpcClient};
use serde::{Deserialize, Serialize};

use crate::services::database::{messages::Message, users::User};

pub mod channels;
pub mod events;
pub mod invites;
pub mod messages;
pub mod roles;
pub mod spaces;
pub mod users;
pub mod webrtc;

// #[derive(Clone, Debug, Deserialize, Serialize)]
// #[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
// #[repr(i8)]
// pub enum Method {
//     Identify(IdentifyMethod) = 1,
//     Heartbeat(HeartbeatMethod) = 2,
//     GetId(GetIdMethod) = 3,

//     // WebRTC: 10-19
//     StartCall(StartCallMethod) = 10,
//     JoinCall(JoinCallMethod) = 11,
//     LeaveCall(LeaveCallMethod) = 12,
//     EndCall(EndCallMethod) = 13,

//     GetMessages(GetMessagesMethod) = 20,
//     SendMessage(SendMessageMethod) = 22,

//     GetChannel(GetChannelMethod) = 30,
//     GetChannels(GetChannelsMethod) = 31,
//     // CreateChannel(CreateChannelMethod) = 32,
//     // EditChannel(EditChannelMethod) = 33,
//     // DeleteChannel(DeleteChannelMethod) = 34,
//     GetSpace(GetSpaceMethod) = 40,
//     CreateSpace(CreateSpaceMethod) = 41,
//     EditSpace(EditSpaceMethod) = 42,
//     DeleteSpace(DeleteSpaceMethod) = 43,

//     // AddFriend(AddFriendMethod) = 50,
//     // RemoveFriend(RemoveFriendMethod) = 51,
//     // GetFriends(GetFriendsMethod) = 52,
//     // GetFriendRequests(GetFriendRequestsMethod) = 53,
//     // AcknowledgeFriendRequest(AcknowledgeFriendRequestMethod) = 55,
//     CreateInvite(CreateInviteMethod) = 60,
//     DeleteInvite(DeleteInviteMethod) = 61,
//     GetInvite(GetInviteMethod) = 62,
//     GetInvites(GetInvitesMethod) = 63,

//     CreateRole(CreateRoleMethod) = 70,
//     EditRole(EditRoleMethod) = 71,
//     DeleteRole(DeleteRoleMethod) = 72,
//     // GetRoles(GetRolesMethod) = 73,
// }


// #[derive(Debug, Deserialize, Serialize)]
// pub struct RpcApiMethod {
//     pub(crate) id: Option<String>,
//     #[serde(flatten)]
//     pub(crate) method: Method,
// }

// #[derive(Clone, Debug, Deserialize, Serialize)]
// pub struct RpcApiResponse {
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub(crate) id: Option<String>,
//     #[serde(flatten)]
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub(crate) response: Option<Response>,
//     #[serde(flatten)]
//     #[serde(skip_serializing_if = "Option::is_none")]
//     pub(crate) error: Option<Error>,
// }

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Event {
    // WebRTC: 10-19
    NewMessage(NewMessageEvent),
    RemoveFriend(String),
    AddFriend(String),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RpcApiEvent {
    #[serde(flatten)]
    pub(crate) event: Event,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloEvent {
    pub(crate) public_key: Vec<u8>,
    pub(crate) request_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMessageEvent {
    message: Message,
    channel_id: String,
}

pub enum CreateChannelType {
    PrivateChannel {
        peer_id: String,
        scope_id: String,
    },
    GroupChannel {
        name: String,
        description: String,
        members: Vec<String>,
        scope_id: String,
    },
    InformationChannel {
        name: String,
        description: String,
        nexus_id: String,
        scope_id: String,
    },
    TextChannel {
        name: String,
        description: String,
        nexus_id: String,
        scope_id: String,
    },
}

pub fn emit_to_id(clients: Arc<DashMap<String, RpcClient>>, user_id: &str, event: Event) {
    let client: Vec<RefMulti<'_, String, RpcClient>> = clients.iter().filter(|client| {
        let i = client.get_user::<User>().map(|u| u.id.clone());
        i == Some(user_id.to_owned())
    }).collect();
    for client in client {
        emit_one(client.value(), RpcApiEvent { event: event.clone() });
    }
}
