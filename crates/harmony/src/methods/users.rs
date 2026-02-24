use harmony_types::users::{
    AddContactMethod, AddContactResponse, AddContactUsernameMethod, CurrentUserResponse, GetCurrentUserMethod, GetContactsMethod, GetContactsResponse, RemoveContactMethod, RemoveContactResponse
};
use rapid::socket::{RpcResponder, RpcState, RpcValue};

use crate::{authentication::check_authenticated, errors::Error, services::database::users::User};

pub async fn add_contact(state: RpcState, data: RpcValue<AddContactMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let friend = User::get(&data.id).await?;
    User::add_contact(&user, &friend.id).await?;
    Ok::<_, Error>(RpcValue(AddContactResponse {}))
}

pub async fn add_contact_username(
    state: RpcState,
    data: RpcValue<AddContactUsernameMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let friend = User::get_by_username(&data.username).await?;
    User::add_contact(&user, &friend.id).await?;
    Ok::<_, Error>(RpcValue(AddContactResponse {}))
}

pub async fn remove_contact(state: RpcState, data: RpcValue<RemoveContactMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let friend = User::get(&data.id).await?;
    user.remove_contact(&friend.id).await?;
    Ok::<_, Error>(RpcValue(RemoveContactResponse {}))
}

pub async fn get_contacts(state: RpcState, _data: RpcValue<GetContactsMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let contacts = user.get_contacts().await?;
    Ok::<_, Error>(RpcValue(GetContactsResponse { contacts }))
}

/// Get the current authenticated user data and keys
/// This method should be used immediately after authentication
pub async fn get_current_user(state: RpcState, _data: RpcValue<GetCurrentUserMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    Ok::<_, Error>(RpcValue(CurrentUserResponse {
        id: user.id.clone(),
        public_key: user.key_package.as_ref().map(|kp| kp.public_key.clone()),
        encrypted_keys: user.key_package.as_ref().map(|kp| kp.encrypted_keys.clone()),
        presence: user.presence.clone(),
    }))
}
