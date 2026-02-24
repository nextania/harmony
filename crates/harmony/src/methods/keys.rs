use harmony_types::users::{
    GetUserMethod, GetUserResponse, Relationship, SetKeyPackageMethod, SetKeyPackageResponse, UserProfile
};
use rapid::socket::{RpcResponder, RpcState, RpcValue};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    services::database::users::User,
};


pub async fn set_key_package(
    state: RpcState,
    data: RpcValue<SetKeyPackageMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    user.set_key_package(data.public_key, data.encrypted_keys)
        .await?;
    Ok::<_, Error>(RpcValue(SetKeyPackageResponse {}))
}


pub async fn get_user(state: RpcState, data: RpcValue<GetUserMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let target = User::get(&data.user_id).await?;
    let public_key = target.key_package.as_ref().map(|kp| kp.public_key.clone());
    let show_presence = match match user.relationship_with(&target.id).await? {
        Some(rel) => rel == Relationship::Established,
        None => false,
    } {
        true => Some(target.presence.clone()),
        false => None,
    };
    Ok::<_, Error>(RpcValue(GetUserResponse {
        user: UserProfile {
            id: target.id,
            public_key,
            presence: show_presence,
        },
    }))
}
