use futures_util::StreamExt;
use mongodb::{bson::doc, options::FindOptions};
use serde::{Deserialize, Serialize};

use crate::{
    errors::{Error, Result},
    services::permissions::PermissionSet,
};

use super::{invites::Invite, messages::Message};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "type")]
pub enum Channel {
    PrivateChannel {
        id: String,
        initiator_id: String,
        target_id: String,
        scope_id: String, // scope: "global" or id
    },
    GroupChannel {
        id: String,
        name: String,
        description: String,
        owner_id: String,
        members: Vec<String>,
        scope_id: String,
    },
    InformationChannel {
        id: String,
        name: String,
        space_id: String,
        scope_id: String,
        permissions: Vec<PermissionOverride>,
    },
    AnnouncementChannel {
        id: String,
        name: String,
        space_id: String,
        scope_id: String,
        permissions: Vec<PermissionOverride>,
    },
    StandardChannel {
        id: String,
        name: String,
        description: String,
        space_id: String,
        scope_id: String,
        // TODO: permission checks
        permissions: Vec<PermissionOverride>,
    },
    // ForumChannel {
    //     id: String,
    //     name: String,
    //     description: String,
    //     space_id: String,
    //     scope_id: String,
    //     permissions: Vec<PermissionOverride>,
    // },
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
            Channel::AnnouncementChannel { id, .. } | Channel::StandardChannel { id, .. } => {
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
            _ => Err(Error::NotFound),
        }
    }

    
    pub async fn get_invites(&self) -> Result<Vec<Invite>> {
        match self {
            Channel::AnnouncementChannel { id, space_id,  .. } | Channel::StandardChannel { id, space_id, .. } => {
                let database = super::get_database();
                let query = doc! {
                    "channel_id": &id,
                    "space_id": space_id,
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
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOverride {
    pub id: String,
    pub allow: PermissionSet,
    pub deny: PermissionSet,
    pub entity_type: EntityType,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EntityType {
    Role,
    Member,
}

// pub async fn create_channel(
//     channel_type: String,
//     name: Option<String>,
//     description: Option<String>,
//     space_id: Option<String>,
//     members: Option<Vec<String>>,
//     initiator_id: Option<String>,
//     peer_id: Option<String>,
// ) -> Result<Channel> {
//     let database = super::get_database();
//     let channel = match channel_type.as_str() {
//         "PRIVATE" => Channel::PrivateChannel {
//             id: super::generate_ulid(),
//             initiator_id: initiator_id.unwrap(),
//             peer_id: peer_id.unwrap(),
//             scope_id: "global".to_owned(),
//         },
//         "GROUP" => Channel::GroupChannel {
//             id: super::generate_ulid(),
//             name: name.unwrap(),
//             description: description.unwrap_or("".to_string()),
//             owner_id: initiator_id.unwrap(),
//             members: members.unwrap(),
//             scope_id: "global".to_owned(),
//         },
//         "INFORMATION" => Channel::InformationChannel {
//             id: super::generate_ulid(),
//             name: name.unwrap(),
//             space_id: space_id.unwrap(),
//             scope_id: "global".to_owned(),
//         },
//         "ANNOUNCEMENT" => Channel::AnnouncementChannel {
//             id: super::generate_ulid(),
//             name: name.unwrap(),
//             space_id: space_id.unwrap(),
//             scope_id: "global".to_owned(),
//         },
//         "CHAT" => Channel::ChatChannel {
//             id: super::generate_ulid(),
//             name: name.unwrap(),
//             description: description.unwrap_or("".to_string()),
//             space_id: space_id.unwrap(),
//             scope_id: "global".to_owned(),
//         },
//         _ => return Err(Error::BadRequest),
//     };
//     database
//         .collection::<Channel>("channels")
//         .insert_one(
//             channel.clone(),
//             None,
//         )
//         .await?;
//     Ok(channel)
// }
