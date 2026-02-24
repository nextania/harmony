use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::{
    CallMember, Channel, Contact, CreateCallTokenResponse, CurrentUserResponse,
    EncryptionHint, GetCallMembersResponse, HarmonyClient, Invite, Message,
    StartCallResponse, UpdateVoiceStateResponse, UserProfile,
};

impl HarmonyClient {
    /// Get a specific channel by ID
    pub async fn get_channel(&self, channel_id: &str) -> Result<Channel> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            channel: Channel,
        }

        let response: Response = self
            .send_request(
                "GET_CHANNEL",
                Params {
                    id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(response.channel)
    }

    /// Get all channels the user has access to
    pub async fn get_channels(&self) -> Result<Vec<Channel>> {
        #[derive(Serialize)]
        struct Params {}

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            channels: Vec<Channel>,
        }

        let response: Response = self.send_request("GET_CHANNELS", Params {}).await?;
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
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            limit: Option<i64>,
            latest: Option<bool>,
            before: Option<String>,
            after: Option<String>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            messages: Vec<Message>,
        }

        let response: Response = self
            .send_request(
                "GET_MESSAGES",
                Params {
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
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            content: Vec<u8>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            message: Message,
        }

        let response: Response = self
            .send_request(
                "SEND_MESSAGE",
                Params {
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
        expires_at: Option<u64>,
        authorized_users: Option<Vec<String>>,
    ) -> Result<Invite> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            max_uses: Option<i32>,
            expires_at: Option<u64>,
            authorized_users: Option<Vec<String>>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            invite: Invite,
        }

        let response: Response = self
            .send_request(
                "CREATE_INVITE",
                Params {
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
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        struct Response {}

        let _: Response = self
            .send_request(
                "DELETE_INVITE",
                Params {
                    id: invite_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Get a specific invite by ID
    pub async fn get_invite(&self, invite_id: &str) -> Result<Invite> {
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            invite: Invite,
        }

        let response: Response = self
            .send_request(
                "GET_INVITE",
                Params {
                    id: invite_id.to_string(),
                },
            )
            .await?;

        Ok(response.invite)
    }

    /// Get all invites for channels the user manages
    pub async fn get_invites(&self, channel_id: String) -> Result<Vec<Invite>> {
        #[derive(Serialize)]
        struct Params {
            channel_id: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            invites: Vec<Invite>,
        }

        let response: Response = self
            .send_request("GET_INVITES", Params { channel_id })
            .await?;
        Ok(response.invites)
    }

    /// Start a call in a channel
    pub async fn start_call(
        &self,
        channel_id: &str,
        preferred_region: Option<&str>,
    ) -> Result<StartCallResponse> {
        #[derive(Serialize)]
        struct Params {
            id: String,
            preferred_region: Option<String>,
        }

        let response: StartCallResponse = self
            .send_request(
                "START_CALL",
                Params {
                    id: channel_id.to_string(),
                    preferred_region: preferred_region.map(|s| s.to_string()),
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
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            id: String,
            initial_muted: bool,
            initial_deafened: bool,
        }

        let response: CreateCallTokenResponse = self
            .send_request(
                "CREATE_CALL_TOKEN",
                Params {
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
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        struct Response {}

        let _: Response = self
            .send_request(
                "END_CALL",
                Params {
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
        #[derive(Serialize)]
        struct Params {
            id: String,
            muted: Option<bool>,
            deafened: Option<bool>,
        }

        let response: UpdateVoiceStateResponse = self
            .send_request(
                "UPDATE_VOICE_STATE",
                Params {
                    id: channel_id.to_string(),
                    muted,
                    deafened,
                },
            )
            .await?;

        Ok(response)
    }

    /// Get all members currently in a call
    pub async fn get_call_members(&self, channel_id: &str) -> Result<Vec<CallMember>> {
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        let response: GetCallMembersResponse = self
            .send_request(
                "GET_CALL_MEMBERS",
                Params {
                    id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(response.members)
    }

    /// Create a new private channel with another user
    pub async fn create_private_channel(&self, target_id: &str) -> Result<Channel> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ChannelInfo {
            #[serde(rename = "type")]
            channel_type: String,
            target_id: String,
        }

        #[derive(Serialize)]
        struct Params {
            channel: ChannelInfo,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            channel: Channel,
        }

        let response: Response = self
            .send_request(
                "CREATE_CHANNEL",
                Params {
                    channel: ChannelInfo {
                        channel_type: "PRIVATE_CHANNEL".to_string(),
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
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct ChannelInfo {
            #[serde(rename = "type")]
            channel_type: String,
            metadata: Vec<u8>,
            encryption_hint: EncryptionHint,
        }

        #[derive(Serialize)]
        struct Params {
            channel: ChannelInfo,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            channel: Channel,
        }

        let response: Response = self
            .send_request(
                "CREATE_CHANNEL",
                Params {
                    channel: ChannelInfo {
                        channel_type: "GROUP_CHANNEL".to_string(),
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
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            metadata: Vec<u8>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            channel: Channel,
        }

        let response: Response = self
            .send_request(
                "EDIT_CHANNEL",
                Params {
                    channel_id: channel_id.to_string(),
                    metadata,
                },
            )
            .await?;

        Ok(response.channel)
    }

    /// Delete a channel (manager only)
    pub async fn delete_channel(&self, channel_id: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "DELETE_CHANNEL",
                Params {
                    channel_id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Leave a group channel
    pub async fn leave_channel(&self, channel_id: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "LEAVE_CHANNEL",
                Params {
                    channel_id: channel_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Add a user to a group channel (manager only)
    pub async fn add_user_to_channel(&self, channel_id: &str, user_id: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            user_id: String,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "ADD_USER_TO_CHANNEL",
                Params {
                    channel_id: channel_id.to_string(),
                    user_id: user_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Edit a message (author only)
    pub async fn edit_message(&self, message_id: &str, content: Vec<u8>) -> Result<Message> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            message_id: String,
            content: Vec<u8>,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            message: Message,
        }

        let response: Response = self
            .send_request(
                "EDIT_MESSAGE",
                Params {
                    message_id: message_id.to_string(),
                    content,
                },
            )
            .await?;

        Ok(response.message)
    }

    /// Delete a message (author or channel manager)
    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            message_id: String,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "DELETE_MESSAGE",
                Params {
                    message_id: message_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Accept an invite by code
    pub async fn accept_invite(&self, code: &str) -> Result<bool> {
        #[derive(Serialize)]
        struct Params {
            code: String,
        }

        #[derive(Deserialize)]
        struct Response {
            pending: bool,
        }

        let response: Response = self
            .send_request(
                "ACCEPT_INVITE",
                Params {
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
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            public_key: Vec<u8>,
            encrypted_keys: Vec<u8>,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "SET_KEY_PACKAGE",
                Params {
                    public_key,
                    encrypted_keys,
                },
            )
            .await?;

        Ok(())
    }

    /// Get a user's public profile (x25519 public key)
    pub async fn get_user(&self, user_id: &str) -> Result<UserProfile> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            user_id: String,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Response {
            user: UserProfile,
        }

        let response: Response = self
            .send_request(
                "GET_USER",
                Params {
                    user_id: user_id.to_string(),
                },
            )
            .await?;

        Ok(response.user)
    }

    /// Get the current authenticated user's data and keys
    pub async fn get_current_user(&self) -> Result<CurrentUserResponse> {
        #[derive(Serialize)]
        struct Params {}

        let response: CurrentUserResponse = self
            .send_request("GET_CURRENT_USER", Params {})
            .await?;

        Ok(response)
    }

    /// Add a friend by user ID
    pub async fn add_friend(&self, user_id: &str) -> Result<()> {
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "ADD_FRIEND",
                Params {
                    id: user_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Add a friend by username
    pub async fn add_friend_username(&self, username: &str) -> Result<()> {
        #[derive(Serialize)]
        struct Params {
            username: String,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "ADD_FRIEND_USERNAME",
                Params {
                    username: username.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Remove a friend by user ID
    pub async fn remove_friend(&self, user_id: &str) -> Result<()> {
        #[derive(Serialize)]
        struct Params {
            id: String,
        }

        #[derive(Deserialize)]
        struct Resp {}

        let _: Resp = self
            .send_request(
                "REMOVE_FRIEND",
                Params {
                    id: user_id.to_string(),
                },
            )
            .await?;

        Ok(())
    }

    /// Get the current user's contacts/friends list
    pub async fn get_friends(&self) -> Result<Vec<Contact>> {
        #[derive(Serialize)]
        struct Params {}

        let response: Vec<Contact> = self
            .send_request("GET_FRIENDS", Params {})
            .await?;

        Ok(response)
    }

}
