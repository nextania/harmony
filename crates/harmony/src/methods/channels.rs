use std::sync::Arc;

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated, errors::{Error, Result}, services::database::channels::Channel
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelMethod {
    id: String,
    // TODO: scopes
    scope_id: Option<String>,
    space_id: Option<String>,
}

pub async fn get_channel(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<GetChannelMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(clients, &id)?;
    let channel = Channel::get(&data.id).await?;
    match channel {
        Channel::PrivateChannel { .. } | Channel::GroupChannel { .. } => {
            if data.space_id.is_some() {
                return Err(Error::NotFound);
            }
            let in_channel = user.in_channel(&channel).await?;
            if !in_channel {
                return Err(Error::NotFound);
            }
            Ok(RpcValue(GetChannelResponse { channel }))
        }
        Channel::InformationChannel { ref space_id, .. }
        | Channel::AnnouncementChannel { ref space_id, .. }
        | Channel::StandardChannel { ref space_id, .. } => {
            if let Some(request_space_id) = &data.space_id {
                if request_space_id != space_id {
                    return Err(Error::NotFound);
                }
                let user_in_space = user.in_space(space_id).await?;
                if !user_in_space {
                    return Err(Error::NotFound);
                }
                Ok(RpcValue(GetChannelResponse { channel }))
            } else {
                Err(Error::NotFound)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelResponse {
    channel: Channel,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelsMethod {
    // TODO: work out how scopes work with private channels
    scope_id: Option<String>,
}

pub async fn get_channels(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    _: RpcValue<GetChannelsMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(clients, &id)?;
    let channels = user.get_channels().await?;
    Ok::<_, Error>(RpcValue(GetChannelsResponse { channels }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelsResponse {
    channels: Vec<Channel>,
}
// TODO: Partial structs

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateChannelMethod {
    channel: ChannelInformation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum ChannelInformation {
    PrivateChannel {
        target_id: String,
        scope_id: Option<String>,
    },
    GroupChannel {
        scope_id: String,
        name: String,
        description: Option<String>,
    },
    InformationChannel {
        space_id: String,
        scope_id: Option<String>,
        name: String,
        description: Option<String>,
    },
    AnnouncementChannel {
        space_id: String,
        scope_id: Option<String>,
        name: String,
        description: Option<String>,
    },
    ChatChannel {
        space_id: String,
        scope_id: Option<String>,
        name: String,
        description: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditChannelMethod {
    channel_id: String,
    name: Option<String>,
    description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteChannelMethod {
    channel_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddUserToChannelMethod {
    channel_id: String,
    user_id: String,
}
