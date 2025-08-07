use futures_util::StreamExt;
use mongodb::{bson::doc, options::FindOptions};
use serde::{Deserialize, Serialize};

use crate::errors::{Error, Result};

use super::{invites::Invite, messages::Message};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMember {
    pub id: String,
    pub role: ChannelMemberRole,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelMemberRole {
    Member,
    Manager,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
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

impl Channel {
    pub async fn get(id: &String) -> Result<Channel> {
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
    pub fn is_manager(&self, user_id: &String) -> bool {
        match self {
            Channel::GroupChannel { members, .. } => members
                .iter()
                .any(|m| m.id == *user_id && m.role == ChannelMemberRole::Manager),
            _ => false,
        }
    }
    // TODO: pub async fn create
}
