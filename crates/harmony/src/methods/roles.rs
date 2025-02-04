use std::sync::Arc;

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated, errors::{Error, Result}, services::{
        database::{
            members::Member,
            roles::{Color, Role},
            spaces::Space,
        },
        permissions::{can_modify_role, Permission},
    }
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleMethod {
    name: String,
    permissions: i64,
    color: Color,
    space_id: String,
}

pub async fn create_role(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<CreateRoleMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let space = Space::get(&data.space_id).await?;
    let role = Role::create(
        &space,
        data.name.clone(),
        data.permissions,
        data.color.clone(),
    )
    .await?;
    Ok::<_, Error>(RpcValue(CreateRoleResponse { role }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRoleResponse {
    role: Role,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditRoleMethod {
    id: String,
    name: String,
    permissions: i64,
    color: Color,
    space_id: String,
    scope_id: Option<String>,
}

pub async fn edit_role(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<EditRoleMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let role = Role::get(&data.id).await?;
    if role.space_id != data.space_id {
        return Err(Error::NotFound);
    }
    if let Some(scope_id) = &data.scope_id {
        if role.scope_id != scope_id.clone() {
            return Err(Error::NotFound);
        }
    }
    let member = Member::get(&id, &data.space_id).await?;
    let can_modify = can_modify_role(&member, &role).await?;
    if !can_modify {
        return Err(Error::MissingPermission {
            permission: Permission::ManageRoles,
        });
    }
    let role = role
        .update(data.name.clone(), data.permissions, data.color.clone())
        .await?;
    Ok(RpcValue(EditRoleResponse { role }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditRoleResponse {
    role: Role,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRoleMethod {
    id: String,
}

pub async fn delete_role(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<DeleteRoleMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let role = Role::get(&data.id).await?;
    role.delete().await?;
    Ok::<_, Error>(RpcValue(DeleteRoleResponse {
        id: data.id.clone(),
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRoleResponse {
    id: String,
}
