use harmony_types::channels::{
    ChannelInformation, CreateChannelMethod,
    CreateChannelResponse, DeleteChannelMethod, DeleteChannelResponse, EditChannelMethod,
    EditChannelResponse, GetChannelMethod, GetChannelResponse, GetChannelsMethod,
    GetChannelsResponse, LeaveChannelMethod, LeaveChannelResponse,
};
use harmony_types::invites::{
    AcceptInviteMethod, AcceptInviteResponse, CreateInviteMethod, CreateInviteResponse, DeleteInviteMethod, DeleteInviteResponse, GetInviteMethod, GetInviteResponse, GetInvitesMethod, GetInvitesResponse
};
use harmony_types::messages::{
    DeleteMessageMethod, DeleteMessageResponse, EditMessageMethod, EditMessageResponse,
    GetMessagesMethod, GetMessagesResponse, SendMessageMethod, SendMessageResponse,
};
use harmony_types::users::{
    AddContactMethod, AddContactResponse, AddContactUsernameMethod, AddContactUsernameResponse, ContactExtended, CurrentUserResponse, GetContactsMethod, GetContactsResponse, GetCurrentUserMethod, GetUserMethod, GetUserResponse, RemoveContactMethod, RemoveContactResponse, SetKeyPackageMethod, SetKeyPackageResponse
};
use harmony_types::voice::{
    CreateCallTokenMethod, CreateCallTokenResponse, EndCallMethod, EndCallResponse,
    GetCallMembersMethod, GetCallMembersResponse, StartCallMethod, StartCallResponse,
    UpdateVoiceStateMethod, UpdateVoiceStateResponse,
};
use pulse_types::Region;

use crate::error::Result;
use crate::{Channel, EncryptionHint, HarmonyClient, Message};

impl HarmonyClient {
    /// Get a specific channel by ID
    pub async fn get_channel(&self, channel_id: &str) -> Result<Channel> {
        let response: GetChannelResponse = self
            .send_request(
                "GET_CHANNEL",
                GetChannelMethod {
                    id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(response.channel)
    }

    /// Get all channels the user has access to
    pub async fn get_channels(&self) -> Result<Vec<Channel>> {
        let response: GetChannelsResponse = self
            .send_request("GET_CHANNELS", GetChannelsMethod {})
            .await?;
        Ok(response.channels)
    }

    /// Get messages from a channel
    pub async fn get_messages(
        &self,
        channel_id: &str,
        limit: Option<i64>,
        latest: Option<bool>,
        before: Option<String>,
        after: Option<String>,
    ) -> Result<Vec<Message>> {
        let response: GetMessagesResponse = self
            .send_request(
                "GET_MESSAGES",
                GetMessagesMethod {
                    channel_id: channel_id.to_string(),
                    limit,
                    latest,
                    before,
                    after,
                },
            )
            .await?;

        Ok(response.messages)
    }

    /// Send a message to a channel
    pub async fn send_message(&self, channel_id: &str, content: Vec<u8>) -> Result<Message> {
        let response: SendMessageResponse = self
            .send_request(
                "SEND_MESSAGE",
                SendMessageMethod {
                    channel_id: channel_id.to_string(),
                    content,
                },
            )
            .await?;

        Ok(response.message)
    }

    /// Create an invite for a channel
    pub async fn create_invite(
        &self,
        channel_id: &str,
        max_uses: Option<i32>,
        expires_at: Option<i64>,
        authorized_users: Option<Vec<String>>,
    ) -> Result<crate::Invite> {
        let response: CreateInviteResponse = self
            .send_request(
                "CREATE_INVITE",
                CreateInviteMethod {
                    channel_id: channel_id.to_string(),
                    max_uses,
                    expires_at,
                    authorized_users,
                },
            )
            .await?;

        Ok(response.invite)
    }

    /// Delete an invite
    pub async fn delete_invite(&self, invite_id: &str) -> Result<()> {
        let _: DeleteInviteResponse = self
            .send_request(
                "DELETE_INVITE",
                DeleteInviteMethod {
                    id: invite_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Get a specific invite by code
    pub async fn get_invite(&self, code: &str) -> Result<GetInviteResponse> {
        let response: GetInviteResponse = self
            .send_request(
                "GET_INVITE",
                GetInviteMethod {
                    code: code.to_string(),
                },
            )
            .await?;

        Ok(response)
    }

    /// Get all invites for channels the user manages
    pub async fn get_invites(&self, channel_id: String) -> Result<Vec<crate::Invite>> {
        let response: GetInvitesResponse = self
            .send_request("GET_INVITES", GetInvitesMethod { channel_id })
            .await?;
        Ok(response.invites)
    }

    /// Start a call in a channel
    pub async fn start_call(
        &self,
        channel_id: &str,
        preferred_region: Option<Region>,
    ) -> Result<StartCallResponse> {
        let response: StartCallResponse = self
            .send_request(
                "START_CALL",
                StartCallMethod {
                    id: channel_id.to_string(),
                    preferred_region,
                },
            )
            .await?;

        Ok(response)
    }

    /// Create a call token (session ID + token) for joining a call via Pulse
    pub async fn create_call_token(
        &self,
        channel_id: &str,
        initial_muted: bool,
        initial_deafened: bool,
    ) -> Result<CreateCallTokenResponse> {
        let response: CreateCallTokenResponse = self
            .send_request(
                "CREATE_CALL_TOKEN",
                CreateCallTokenMethod {
                    id: channel_id.to_string(),
                    initial_muted,
                    initial_deafened,
                },
            )
            .await?;

        Ok(response)
    }

    /// End a call in a channel (requires manager permission)
    pub async fn end_call(&self, channel_id: &str) -> Result<()> {
        let _: EndCallResponse = self
            .send_request(
                "END_CALL",
                EndCallMethod {
                    id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Update voice state (muted/deafened) for the current user in a call
    pub async fn update_voice_state(
        &self,
        channel_id: &str,
        muted: Option<bool>,
        deafened: Option<bool>,
    ) -> Result<UpdateVoiceStateResponse> {
        let response: UpdateVoiceStateResponse = self
            .send_request(
                "UPDATE_VOICE_STATE",
                UpdateVoiceStateMethod {
                    id: channel_id.to_string(),
                    muted,
                    deafened,
                },
            )
            .await?;

        Ok(response)
    }

    /// Get all members currently in a call
    pub async fn get_call_members(&self, channel_id: &str) -> Result<Vec<crate::CallMember>> {
        let response: GetCallMembersResponse = self
            .send_request(
                "GET_CALL_MEMBERS",
                GetCallMembersMethod {
                    id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(response.members)
    }

    /// Create a new private channel with another user
    pub async fn create_private_channel(&self, target_id: &str) -> Result<Channel> {
        let response: CreateChannelResponse = self
            .send_request(
                "CREATE_CHANNEL",
                CreateChannelMethod {
                    channel: ChannelInformation::PrivateChannel {
                        target_id: target_id.to_string(),
                    },
                },
            )
            .await?;

        Ok(response.channel)
    }

    /// Create a new group channel with encrypted metadata
    pub async fn create_group_channel(
        &self,
        metadata: Vec<u8>,
        encryption_hint: EncryptionHint,
    ) -> Result<Channel> {
        let response: CreateChannelResponse = self
            .send_request(
                "CREATE_CHANNEL",
                CreateChannelMethod {
                    channel: ChannelInformation::GroupChannel {
                        metadata,
                        encryption_hint,
                    },
                },
            )
            .await?;

        Ok(response.channel)
    }

    /// Edit group channel metadata (manager only)
    pub async fn edit_channel(&self, channel_id: &str, metadata: Vec<u8>) -> Result<Channel> {
        let response: EditChannelResponse = self
            .send_request(
                "EDIT_CHANNEL",
                EditChannelMethod {
                    channel_id: channel_id.to_string(),
                    metadata,
                },
            )
            .await?;

        Ok(response.channel)
    }

    /// Delete a channel (manager only)
    pub async fn delete_channel(&self, channel_id: &str) -> Result<()> {
        let _: DeleteChannelResponse = self
            .send_request(
                "DELETE_CHANNEL",
                DeleteChannelMethod {
                    channel_id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Leave a group channel
    pub async fn leave_channel(&self, channel_id: &str) -> Result<()> {
        let _: LeaveChannelResponse = self
            .send_request(
                "LEAVE_CHANNEL",
                LeaveChannelMethod {
                    channel_id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Edit a message (author only)
    pub async fn edit_message(&self, message_id: &str, content: Vec<u8>) -> Result<Message> {
        let response: EditMessageResponse = self
            .send_request(
                "EDIT_MESSAGE",
                EditMessageMethod {
                    message_id: message_id.to_string(),
                    content,
                },
            )
            .await?;

        Ok(response.message)
    }

    /// Delete a message (author or channel manager)
    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        let _: DeleteMessageResponse = self
            .send_request(
                "DELETE_MESSAGE",
                DeleteMessageMethod {
                    message_id: message_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Accept an invite by code
    pub async fn accept_invite(&self, code: &str) -> Result<bool> {
        let response: AcceptInviteResponse = self
            .send_request(
                "ACCEPT_INVITE",
                AcceptInviteMethod {
                    code: code.to_string(),
                },
            )
            .await?;

        Ok(response.pending)
    }

    /// Upload key material (x25519 public key, encrypted private keys)
    pub async fn set_key_package(
        &self,
        public_key: Vec<u8>,
        encrypted_keys: Vec<u8>,
    ) -> Result<()> {
        let _: SetKeyPackageResponse = self
            .send_request(
                "SET_KEY_PACKAGE",
                SetKeyPackageMethod {
                    public_key,
                    encrypted_keys,
                },
            )
            .await?;

        Ok(())
    }

    /// Get a user's public profile (x25519 public key)
    pub async fn get_user(&self, user_id: &str) -> Result<crate::UserProfile> {
        let response: GetUserResponse = self
            .send_request(
                "GET_USER",
                GetUserMethod {
                    user_id: user_id.to_string(),
                },
            )
            .await?;

        Ok(response.user)
    }

    /// Get the current authenticated user's data and keys
    pub async fn get_current_user(&self) -> Result<CurrentUserResponse> {
        let response: CurrentUserResponse = self
            .send_request("GET_CURRENT_USER", GetCurrentUserMethod {})
            .await?;

        Ok(response)
    }

    /// Add a contact by user ID
    pub async fn add_contact(&self, user_id: &str) -> Result<()> {
        let _: AddContactResponse = self
            .send_request(
                "ADD_CONTACT",
                AddContactMethod {
                    id: user_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Add a contact by username
    pub async fn add_contact_username(&self, username: &str) -> Result<()> {
        let _: AddContactUsernameResponse = self
            .send_request(
                "ADD_CONTACT_USERNAME",
                AddContactUsernameMethod {
                    username: username.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Remove a contact by user ID
    pub async fn remove_contact(&self, user_id: &str) -> Result<()> {
        let _: RemoveContactResponse = self // server returns AddContactResponse for REMOVE_CONTACT
            .send_request(
                "REMOVE_CONTACT",
                RemoveContactMethod {
                    id: user_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Get the current user's contacts list
    pub async fn get_contacts(&self) -> Result<Vec<ContactExtended>> {
        let response: GetContactsResponse = self
            .send_request("GET_CONTACTS", GetContactsMethod {})
            .await?;

        Ok(response.contacts)
    }
}
