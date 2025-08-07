use std::sync::Arc;

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated,
    errors::{Error, Result},
    services::database::{channels::Channel, invites::Invite},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateInviteMethod {
    channel_id: String,
    max_uses: Option<i32>,
    expires_at: Option<u64>,
    authorized_users: Option<Vec<String>>,
}

pub async fn create_invite(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<CreateInviteMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(clients, &id)?;
    let invite = Invite::create(
        data.channel_id.clone(),
        user.id.clone(),
        data.expires_at,
        data.max_uses,
        data.authorized_users.clone(),
    )
    .await?;
    Ok::<_, Error>(RpcValue(CreateInviteResponse { invite }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateInviteResponse {
    invite: Invite,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeleteInviteMethod {
    id: String,
}

pub async fn delete_invite(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<DeleteInviteMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(clients, &id)?;
    let invite = Invite::get(&data.id).await?;
    let channel = Channel::get(&invite.channel_id).await?;
    if !channel.is_manager(&user.id) {
        return Err(Error::MissingPermission);
    } else {
        invite.delete().await?;
        Ok(RpcValue(DeleteInviteResponse {}))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteInviteResponse {}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInviteMethod {
    code: String,
}

pub async fn get_invite(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<GetInviteMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(clients, &id)?;
    let invite = Invite::get(&data.code).await?;
    let channel = Channel::get(&invite.channel_id).await?;
    //ban?
    let Channel::GroupChannel {
        name,
        description,
        members,
        ..
    } = channel
    else {
        return Err(Error::InvalidInvite);
    };
    Ok(RpcValue(GetInviteResponse {
        invite: InviteInformation::Group {
            name,
            description,
            inviter_id: invite.creator,
            authorized: invite
                .authorized_users
                .unwrap_or_else(|| vec![user.id.clone()])
                .contains(&user.id),
            member_count: members.len() as i32,
        },
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InviteInformation {
    #[serde(rename_all = "camelCase")]
    Group {
        name: String,
        description: String,
        inviter_id: String,
        authorized: bool,
        member_count: i32,
    },
    #[serde(rename_all = "camelCase")]
    Space {
        name: String,
        description: String,
        inviter_id: String,
        banned: bool,
        authorized: bool,
        member_count: i32,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInviteResponse {
    #[serde(flatten)]
    invite: InviteInformation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInvitesMethod {
    channel_id: String,
}

pub async fn get_invites(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<GetInvitesMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(clients, &id)?;
    let channel = Channel::get(&data.channel_id).await?;
    if !channel.is_manager(&user.id) {
        return Err(Error::MissingPermission);
    }
    let invites = channel.get_invites().await?;
    Ok(RpcValue(GetInvitesResponse { invites }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInvitesResponse {
    invites: Vec<Invite>,
}

// TODO: Invite manager built-in
