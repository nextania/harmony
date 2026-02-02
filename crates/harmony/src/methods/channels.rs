use rapid::socket::{RpcResponder, RpcState, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    services::database::channels::Channel,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetChannelMethod {
    id: String,
}

pub async fn get_channel(
    state: RpcState,
    data: RpcValue<GetChannelMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(&state)?;
    let channel = Channel::get(&data.id).await?;
    match channel {
        Channel::PrivateChannel { .. } | Channel::GroupChannel { .. } => {
            let in_channel = user.in_channel(&channel).await?;
            if !in_channel {
                return Err(Error::NotFound);
            }
            Ok(RpcValue(GetChannelResponse { channel }))
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
pub struct GetChannelsMethod {}

pub async fn get_channels(
    state: RpcState,
    _: RpcValue<GetChannelsMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
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
    },
    GroupChannel {
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
