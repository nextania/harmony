use std::sync::Arc;

use dashmap::DashMap;
use rapid::socket::{RpcClient, RpcResponder, RpcValue};
use serde::{Deserialize, Serialize};

use crate::{
    authentication::check_authenticated, errors::Error, services::database::spaces::Space
};


#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSpaceMethod {
    id: String,
}

pub async fn get_space(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<GetSpaceMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let space = Space::get(&data.id).await?;
    let user_in_space = user.in_space(&data.id).await?;
    if !user_in_space {
        return Err(Error::NotFound);
    }
    Ok(RpcValue(GetSpaceResponse { space }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSpaceResponse {
    space: Space,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSpaceMethod {
    // space: Space,
    name: String,
    description: Option<String>,
    scope: Option<String>,
}

pub async fn create_space(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<CreateSpaceMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let trimmed = data.name.trim();
    if trimmed.len() > 32 {
        return Err(Error::NameTooLong);
    }
    if trimmed.is_empty() {
        return Err(Error::NameEmpty);
    }
    let space = Space::create(
        data.name.clone(),
        data.description.clone(),
        user.id.clone(),
        data.scope.clone(),
    )
    .await?;
    Ok(RpcValue(CreateSpaceResponse { space }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSpaceResponse {
    space: Space,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinSpaceMethod {
    code: String,
}

pub async fn join_space(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<JoinSpaceMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let space = user.accept_invite(&data.code).await?;
    space.add_member(&id).await?;
    Ok::<_, Error>(RpcValue(JoinSpaceResponse { space }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinSpaceResponse {
    space: Space,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveSpaceMethod {
    space_id: String,
}

pub async fn leave_space(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<LeaveSpaceMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let user_in_space = user.in_space(&data.space_id).await?;
    if !user_in_space {
        return Err(Error::NotFound);
    }
    let space = Space::get(&data.space_id).await?;
    space.remove_member(&id).await?;
    Ok(RpcValue(LeaveSpaceResponse {
        space_id: data.space_id.clone(),
    }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeaveSpaceResponse {
    space_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSpacesMethod {}

pub async fn get_spaces(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    _: GetSpacesMethod,
) -> impl RpcResponder {
    let user = check_authenticated(clients, &id)?;
    let spaces = user.get_spaces().await?;
    Ok::<_, Error>(RpcValue(GetSpacesResponse { spaces }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetSpacesResponse {
    spaces: Vec<Space>,
}

// FIXME: permissions

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSpaceMethod {
    space_id: String,
    name: Option<String>,
    description: Option<String>,
    base_permissions: Option<i32>,
}
// TODO: logger
pub async fn edit_space(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<EditSpaceMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let space = Space::get(&data.space_id).await?;
    let space = space
        .update(
            data.name.clone(),
            data.description.clone(),
            data.base_permissions,
        )
        .await?;
    Ok::<_, Error>(RpcValue(EditSpaceResponse { space }))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditSpaceResponse {
    space: Space,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSpaceMethod {
    space_id: String,
}

pub async fn delete_space(
    clients: Arc<DashMap<String, RpcClient>>,
    id: String,
    data: RpcValue<DeleteSpaceMethod>,
) -> impl RpcResponder {
    check_authenticated(clients, &id)?;
    let data = data.into_inner();
    let space = Space::get(&data.space_id).await?;
    space.delete().await?;
    Ok::<_, Error>(RpcValue(DeleteSpaceResponse {
        id: data.space_id.clone(),
    }))
}

// FIXME: sudo mode

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteSpaceResponse {
    id: String,
}
