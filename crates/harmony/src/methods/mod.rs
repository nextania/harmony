use std::collections::HashSet;

pub use harmony_types::events::{
    CallMigratedEvent, ChannelDeletedEvent, ChannelUpdatedEvent, Event, MemberJoinedEvent,
    MemberLeftEvent, MessageDeletedEvent, MessageEditedEvent, NewMessageEvent, UserJoinedCallEvent,
    UserLeftCallEvent, UserVoiceStateChangedEvent,
};
use rapid::socket::RpcClients;

pub mod channels;
pub mod invites;
pub mod keys;
pub mod messages;
pub mod users;
pub mod voice;

pub fn emit_to_ids(clients: RpcClients, user_ids: &[String], event: Event) {
    let id_set: HashSet<&str> = user_ids.iter().map(|s| s.as_str()).collect();
    clients.emit_by(event, |client| {
        client.user_id().is_some_and(|uid| id_set.contains(uid))
    });
}
