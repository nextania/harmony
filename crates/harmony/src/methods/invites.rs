use std::sync::Arc;

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated, errors::{Error, Result}, services::{
        database::{
            channels::Channel,
            infractions::is_banned,
            invites::Invite,
            members::Member,
            spaces::Space,
        },
        permissions::Permission,
    }
};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateInviteMethod {
    channel_id: String,
    max_uses: Option<i32>,
    expires_at: Option<u64>,
    authorized_users: Option<Vec<String>>,
    space_id: Option<String>,
    scope_id: Option<String>,
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
        data.space_id.clone(),
        data.scope_id.clone(),
    )
    .await?;
    Ok::<_, Error>(RpcValue(CreateInviteResponse { invite }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateInviteResponse {
    invite: Invite,
}

// pub struct UpdateInviteMethod {
//     code: String,
//     max_uses: Option<i32>,
//     expires_at: Option<u64>,
// }

// #[async_trait]
// impl Respond for UpdateInviteMethod {
//     async fn respond(&self, clients: Arc<DashMap<String, RpcClient>>, id: String) -> Response {
//         let client = clients.get(&id.clone()).unwrap();
//         let member = get_member(id).await;
//         match member {
//             Ok(member) => {
//                 let permissions = permissions_for(member).await;
//                 if !permissions.has_permission(Permission::ManageInvites) {
//                     return Response::Error(ErrorResponse { error: "You do not have permission to manage invites".to_string() });
//                 } else {

//                 }
//             },
//             Err(error) => Response::Error(ErrorResponse { error })
//         }
//     }
// }

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DeleteInviteMethod {
    id: String,
    space_id: String,
}

pub async fn delete_invite(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<DeleteInviteMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(clients, &id)?;
    let member = Member::get(&user.id, &data.space_id).await?;
    let permissions = member.get_permissions().await?;
    if !permissions.has_permission(Permission::ManageInvites) {
        return Err(Error::MissingPermission {
            permission: Permission::ManageInvites,
        });
    } else {
        let invite = Invite::get(&data.id).await?;
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
    if let Some(space_id) = invite.space_id {
        let space = Space::get(&space_id).await?;
        let banned = is_banned(user.id.clone(), space.id).await?;
        Ok(RpcValue(GetInviteResponse {
            invite: InviteInformation::Space {
                name: space.name,
                description: space.description,
                inviter_id: invite.creator,
                banned,
                authorized: invite
                    .authorized_users
                    .unwrap_or_else(|| vec![user.id.clone()])
                    .contains(&user.id),
                member_count: space.members.len() as i32,
            },
        }))
    } else {
        let channel = Channel::get(&invite.channel_id).await?;
        if let Channel::GroupChannel {
            name,
            description,
            members,
            ..
        } = channel
        {
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
        } else {
            Err(Error::InvalidInvite)
        }
    }
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
    space_id: Option<String>,
    scope_id: Option<String>,
}

pub async fn get_invites(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<GetInvitesMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(clients, &id)?;
    if let Some(space_id) = &data.space_id {
        let member = Member::get(&user.id, space_id).await?;
        let permissions = member.get_permissions().await?;
        if !permissions.has_permission(Permission::ManageInvites) {
            return Err(Error::MissingPermission {
                permission: Permission::ManageInvites,
            });
        }
    }
    let channel = Channel::get(&data.channel_id).await?;
    let invites = channel.get_invites().await?;
    Ok(RpcValue(GetInvitesResponse { invites }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetInvitesResponse {
    invites: Vec<Invite>,
}

// TODO: Invite manager built-in
