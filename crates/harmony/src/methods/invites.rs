use harmony_types::invites::{
    AcceptInviteMethod, AcceptInviteResponse, CreateInviteMethod, CreateInviteResponse,
    DeleteInviteMethod, DeleteInviteResponse, GetInviteMethod, GetInviteResponse, GetInvitesMethod,
    GetInvitesResponse, InviteInformation,
};
use rapid::socket::{RpcResponder, RpcState, RpcValue};

use crate::{
    authentication::check_authenticated,
    errors::Error,
    methods::{Event, MemberJoinedEvent, emit_to_ids},
    services::database::{
        channels::{Channel, EncryptionHint},
        invites::Invite,
    },
};

pub async fn create_invite(
    state: RpcState,
    data: RpcValue<CreateInviteMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(&state)?;
    let invite = Invite::create(
        data.channel_id.clone(),
        user.id.clone(),
        data.expires_at,
        data.max_uses,
        data.authorized_users.clone(),
    )
    .await?;
    Ok::<_, Error>(RpcValue(CreateInviteResponse {
        invite: invite.into(),
    }))
}

pub async fn delete_invite(
    state: RpcState,
    data: RpcValue<DeleteInviteMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(&state)?;
    let invite = Invite::get(&data.id).await?;
    let channel = Channel::get(&invite.channel_id).await?;
    if !channel.is_manager(&user.id) {
        Err(Error::MissingPermission)
    } else {
        invite.delete().await?;
        Ok(RpcValue(DeleteInviteResponse {}))
    }
}

pub async fn get_invite(state: RpcState, data: RpcValue<GetInviteMethod>) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(&state)?;
    let invite = Invite::get(&data.code).await?;
    let channel = Channel::get(&invite.channel_id).await?;
    //ban?
    let Channel::GroupChannel {
        id: channel_id, metadata, members, ..
    } = channel
    else {
        return Err(Error::InvalidInvite);
    };
    Ok(RpcValue(GetInviteResponse {
        invite: InviteInformation::Group {
            channel_id,
            metadata: metadata.clone(),
            inviter_id: invite.creator,
            authorized: invite
                .authorized_users
                .unwrap_or_else(|| vec![user.id.clone()])
                .contains(&user.id),
            member_count: members.len() as i32,
        },
    }))
}

pub async fn get_invites(state: RpcState, data: RpcValue<GetInvitesMethod>) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(&state)?;
    let channel = Channel::get(&data.channel_id).await?;
    if !channel.is_manager(&user.id) {
        return Err(Error::MissingPermission);
    }
    let invites = channel.get_invites().await?;
    Ok(RpcValue(GetInvitesResponse {
        invites: invites.into_iter().map(|i| i.into()).collect(),
    }))
}

pub async fn accept_invite(
    state: RpcState,
    data: RpcValue<AcceptInviteMethod>,
) -> impl RpcResponder {
    let data = data.into_inner();
    let user = check_authenticated(&state)?;
    let invite = Invite::get(&data.code).await?;
    if invite
        .authorized_users
        .as_ref()
        .unwrap_or(&vec![user.id.clone()])
        .contains(&user.id)
    {
        let channel = Channel::get(&invite.channel_id).await?;
        let pending = if let Channel::GroupChannel {
            encryption_hint: EncryptionHint::Mls,
            ..
        } = channel
        {
            // in this case, we want to add a pending external proposal for this user
            // it should now show as pending for this user, until a manager in the group
            // makes a commit, at which point the user will be added as a regular member
            channel.add_pending_member(&user.id).await?;
            true
        } else {
            channel.add_member(&user.id).await?;
            false
        };
        invite.increment_uses(&user.id).await?;
        // broadcast event
        emit_to_ids(
            state.clients(),
            &channel.member_ids(),
            Event::MemberJoined(MemberJoinedEvent {
                channel_id: channel.id().to_string(),
                user_id: user.id.clone(),
            }),
        );
        Ok(RpcValue(AcceptInviteResponse { pending, channel_id: channel.id().to_string() }))
    } else {
        Err(Error::InvalidInvite)
    }
}
