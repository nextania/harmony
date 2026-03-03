use harmony_types::users::{
    GetUserMethod, GetUserResponse, RelationshipState, SetKeyPackageMethod, SetKeyPackageResponse,
    UserProfile,
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
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    user.set_key_package(data.encrypted_keys).await?;
    Ok::<_, Error>(RpcValue(SetKeyPackageResponse {}))
}

pub async fn get_user(state: RpcState, data: RpcValue<GetUserMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
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
