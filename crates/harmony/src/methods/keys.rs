use harmony_types::users::{
    GetUserMethod, GetUserResponse, GetUsersMethod, GetUsersResponse, RelationshipState,
    SetKeyPackageMethod, SetKeyPackageResponse, UserProfile,
};
use rapid::socket::{RpcResponder, RpcState, RpcValue};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    services::database::users::{User, get_presentable_presence},
};

pub async fn set_key_package(
    state: RpcState,
    data: RpcValue<SetKeyPackageMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state).await?;
    let data = data.into_inner();
    let generation = user
        .set_key_package(data.encrypted_keys, data.expected_generation)
        .await?;
    Ok::<_, Error>(RpcValue(SetKeyPackageResponse { generation }))
}

pub async fn get_user(state: RpcState, data: RpcValue<GetUserMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state).await?;
    let data = data.into_inner();
    let target = User::get(&data.user_id).await?;
    let show_presence = match match user.relationship_with(&target.id).await? {
        Some(rel) => matches!(rel, RelationshipState::Established { .. }),
        None => false,
    } {
        true => Some(get_presentable_presence(&target).await?),
        false => None,
    };
    Ok::<_, Error>(RpcValue(GetUserResponse {
        user: UserProfile {
            id: target.id,
            presence: show_presence,
        },
    }))
}

const GET_USERS_MAX_BATCH: usize = 50;

pub async fn get_users(state: RpcState, data: RpcValue<GetUsersMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state).await?;
    let data = data.into_inner();
    if data.user_ids.len() > GET_USERS_MAX_BATCH {
        return Err(Error::InvalidMethod);
    }
    let mut users = Vec::with_capacity(data.user_ids.len());
    for user_id in &data.user_ids {
        let target = match User::get(user_id).await {
            Ok(target) => target,
            Err(Error::NotFound) => continue,
            Err(e) => return Err(e),
        };
        let show_presence = match user.relationship_with(&target.id).await? {
            Some(RelationshipState::Established { .. }) => {
                Some(get_presentable_presence(&target).await?)
            }
            _ => None,
        };
        users.push(UserProfile {
            id: target.id,
            presence: show_presence,
        });
    }
    Ok::<_, Error>(RpcValue(GetUsersResponse { users }))
}
