use futures_util::StreamExt;
use mongodb::{
    bson::{Binary, doc, spec::BinarySubtype},
    options::FindOptions,
};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::errors::{Error, Result};

use super::{invites::Invite, messages::Message};

pub use harmony_types::channels::{ChannelMember, ChannelMemberRole, EncryptionHint};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Channel {
    PrivateChannel {
        id: String,
        initiator_id: String,
        target_id: String,
        last_key_id: String,
    },
    GroupChannel {
        id: String,
        metadata: Vec<u8>, // encrypted with group key, contains name, description, etc.
        members: Vec<ChannelMember>,
        pending_members: Vec<String>,
        blacklist: Vec<String>,
        encryption_hint: EncryptionHint,
    },
}

impl Channel {
    pub async fn get(id: &str) -> Result<Channel> {
        let database = super::get_database();
        let channel = database
            .collection::<Channel>("channels")
            .find_one(doc! {
                "id": id,
            })
            .await?;
        match channel {
            Some(channel) => Ok(channel),
            None => Err(Error::NotFound),
        }
    }
    pub async fn get_messages(
        &self,
        limit: Option<i64>,
        latest: Option<bool>,
        before: Option<String>,
        after: Option<String>,
    ) -> Result<Vec<Message>> {
        match self {
            Channel::PrivateChannel { id, .. } | Channel::GroupChannel { id, .. } => {
                let database = super::get_database();
                let limit = limit.unwrap_or(50);
                let mut query = doc! { "channelId": id };
                if let Some(before) = before {
                    query.insert("id", doc! { "$lt": before });
                }
                if let Some(after) = after {
                    query.insert("id", doc! { "$gt": after });
                }
                let options = FindOptions::builder()
                    .sort(doc! {
                        "id": if latest.unwrap_or(false) { -1 } else { 1 }
                    })
                    .limit(limit)
                    .build();
                let messages: Vec<_> = database
                    .collection::<Message>("messages")
                    .find(query)
                    .with_options(options)
                    .await?
                    .collect()
                    .await;
                let messages = messages
                    .into_iter()
                    .map(|m| m.map_err(|e| e.into()))
                    .collect::<Result<Vec<_>>>()?;

                Ok(messages)
            }
        }
    }

    pub async fn get_invites(&self) -> Result<Vec<Invite>> {
        match self {
            Channel::GroupChannel { id, .. } => {
                let database = super::get_database();
                let query = doc! {
                    "channel_id": &id,
                };
                let invites: std::result::Result<Vec<Invite>, _> = database
                    .collection::<Invite>("invites")
                    .find(query)
                    .await?
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .collect();
                Ok(invites?)
            }
            _ => Err(Error::NotFound),
        }
    }
    pub fn is_manager(&self, user_id: &str) -> bool {
        match self {
            Channel::GroupChannel { members, .. } => members
                .iter()
                .any(|m| m.id == *user_id && m.role == ChannelMemberRole::Manager),
            _ => false,
        }
    }
    pub async fn create_private(
        initiator_id: String,
        target_id: String,
        key_id: String,
    ) -> Result<Channel> {
        let database = super::get_database();
        let channel = Channel::PrivateChannel {
            id: Ulid::new().to_string(),
            initiator_id,
            target_id,
            last_key_id: key_id,
        };
        database
            .collection::<Channel>("channels")
            .insert_one(&channel)
            .await?;
        Ok(channel)
    }

    pub async fn create_group(
        initiator_id: String,
        metadata: Vec<u8>,
        encryption_hint: EncryptionHint,
    ) -> Result<Channel> {
        let database = super::get_database();
        let channel = Channel::GroupChannel {
            id: Ulid::new().to_string(),
            metadata,
            members: vec![ChannelMember {
                id: initiator_id,
                role: ChannelMemberRole::Manager,
            }],
            pending_members: vec![],
            blacklist: vec![],
            encryption_hint,
        };
        database
            .collection::<Channel>("channels")
            .insert_one(&channel)
            .await?;
        Ok(channel)
    }

    pub async fn get_between(user1: &str, user2: &str) -> Result<Option<Channel>> {
        let database = super::get_database();
        let query = doc! {
            "type": "PrivateChannel",
            "$or": [
                { "initiator_id": user1, "target_id": user2 },
                { "initiator_id": user2, "target_id": user1 },
            ]
        };
        let channel = database
            .collection::<Channel>("channels")
            .find_one(query)
            .await?;
        Ok(channel)
    }

    pub async fn update_key_id(&self, key_id: &str) -> Result<()> {
        let id = self.id();
        let database = super::get_database();
        database
            .collection::<Channel>("channels")
            .update_one(
                doc! { "id": id },
                doc! {
                    "$set": { "last_key_id": key_id }
                },
            )
            .await?;
        Ok(())
    }

    pub fn id(&self) -> &str {
        match self {
            Channel::PrivateChannel { id, .. } | Channel::GroupChannel { id, .. } => id,
        }
    }

    pub fn is_member(&self, user_id: &str) -> bool {
        match self {
            Channel::PrivateChannel {
                initiator_id,
                target_id,
                ..
            } => initiator_id == user_id || target_id == user_id,
            Channel::GroupChannel { members, .. } => members.iter().any(|m| m.id == user_id),
        }
    }

    pub fn member_ids(&self) -> Vec<String> {
        match self {
            Channel::PrivateChannel {
                initiator_id,
                target_id,
                ..
            } => vec![initiator_id.clone(), target_id.clone()],
            Channel::GroupChannel { members, .. } => members.iter().map(|m| m.id.clone()).collect(),
        }
    }

    pub async fn add_member(&self, user_id: &str) -> Result<()> {
        let Channel::GroupChannel { id, .. } = self else {
            return Err(Error::MissingPermission);
        };
        let database = super::get_database();
        database
            .collection::<Channel>("channels")
            .update_one(
                doc! { "id": id },
                doc! {
                    "$push": {
                        "members": {
                            "id": user_id,
                            "role": "MEMBER"
                        }
                    }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn add_pending_member(&self, user_id: &str) -> Result<()> {
        let Channel::GroupChannel { id, .. } = self else {
            return Err(Error::MissingPermission);
        };
        let database = super::get_database();
        database
            .collection::<Channel>("channels")
            .update_one(
                doc! { "id": id },
                doc! {
                    "$addToSet": {
                        "pendingMembers": user_id
                    }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn promote_pending_member(&self, user_id: &str) -> Result<()> {
        let Channel::GroupChannel { id, .. } = self else {
            return Err(Error::MissingPermission);
        };
        let database = super::get_database();
        database
            .collection::<Channel>("channels")
            .update_one(
                doc! { "id": id },
                doc! {
                    "$pull": { "pendingMembers": user_id },
                    "$push": {
                        "members": {
                            "id": user_id,
                            "role": "MEMBER"
                        }
                    }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn remove_member(&self, user_id: &str) -> Result<()> {
        let Channel::GroupChannel { id, .. } = self else {
            return Err(Error::MissingPermission);
        };
        let database = super::get_database();
        database
            .collection::<Channel>("channels")
            .update_one(
                doc! { "id": id },
                doc! {
                    "$pull": {
                        "members": { "id": user_id }
                    }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn update_metadata(&self, metadata: Vec<u8>) -> Result<()> {
        let Channel::GroupChannel { id, .. } = self else {
            return Err(Error::MissingPermission);
        };
        let database = super::get_database();
        database
            .collection::<Channel>("channels")
            .update_one(
                doc! { "id": id },
                doc! {
                    "$set": { "metadata": Binary { subtype: BinarySubtype::Generic, bytes: metadata } }
                },
            )
            .await?;
        Ok(())
    }

    pub async fn delete(&self) -> Result<()> {
        let id = self.id();
        let database = super::get_database();
        database
            .collection::<Channel>("channels")
            .delete_one(doc! { "id": id })
            .await?;
        database
            .collection::<Message>("messages")
            .delete_many(doc! { "channelId": id })
            .await?;
        database
            .collection::<Invite>("invites")
            .delete_many(doc! { "channelId": id })
            .await?;
        Ok(())
    }

    /// Count how many managers remain in a group channel.
    pub fn manager_count(&self) -> usize {
        match self {
            Channel::GroupChannel { members, .. } => members
                .iter()
                .filter(|m| m.role == ChannelMemberRole::Manager)
                .count(),
            _ => 0,
        }
    }
}

impl From<Channel> for harmony_types::channels::Channel {
    fn from(c: Channel) -> Self {
        match c {
            Channel::PrivateChannel {
                id,
                initiator_id,
                target_id,
                last_key_id,
            } => harmony_types::channels::Channel::PrivateChannel {
                id,
                initiator_id,
                target_id,
                last_key_id,
            },
            Channel::GroupChannel {
                id,
                metadata,
                members,
                pending_members,
                blacklist,
                encryption_hint,
            } => harmony_types::channels::Channel::GroupChannel {
                id,
                metadata,
                // ChannelMember and EncryptionHint are re-exported from harmony-types
                // so they are the same type – no per-field conversion needed.
                members,
                pending_members,
                blacklist,
                encryption_hint,
            },
        }
    }
}
