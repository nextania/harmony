use harmony_types::users::{
    AddContactMethod, AddContactResponse, CurrentUserResponse, GetContactsMethod,
    GetContactsResponse, GetCurrentUserMethod, RemoveContactMethod, RemoveContactResponse,
};
use rapid::socket::{RpcResponder, RpcState, RpcValue};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    methods::{Event, emit_to_id},
    services::database::users::{RelationshipState, User},
};

pub async fn add_contact(state: RpcState, data: RpcValue<AddContactMethod>) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let result = user.add_contact(data.stage).await?;
    emit_to_id(
        state.clients(),
        &result.other_id,
        Event::ContactStateChanged {
            user_id: user.id.clone(),
            state: result.other_state,
        },
    );
    Ok::<_, Error>(RpcValue(AddContactResponse {
        profile: result.profile,
        state: result.self_state,
    }))
}

pub async fn remove_contact(
    state: RpcState,
    data: RpcValue<RemoveContactMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let data = data.into_inner();
    let friend = User::get(&data.id).await?;
    user.remove_contact(&friend.id).await?;
    emit_to_id(
        state.clients(),
        &friend.id,
        Event::ContactStateChanged {
            user_id: user.id.clone(),
            state: RelationshipState::None,
        },
    );
    Ok::<_, Error>(RpcValue(RemoveContactResponse {}))
}

pub async fn get_contacts(
    state: RpcState,
    _data: RpcValue<GetContactsMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    let contacts = user.get_contacts().await?;
    Ok::<_, Error>(RpcValue(GetContactsResponse { contacts }))
}

/// Get the current authenticated user data and keys
/// This method should be used immediately after authentication
pub async fn get_current_user(
    state: RpcState,
    _data: RpcValue<GetCurrentUserMethod>,
) -> impl RpcResponder {
    let user = check_authenticated(&state)?;
    Ok::<_, Error>(RpcValue(CurrentUserResponse {
        id: user.id.clone(),
        encrypted_keys: user
            .key_package
            .as_ref()
            .map(|kp| kp.encrypted_keys.clone()),
        presence: user.presence.clone(),
    }))
}
