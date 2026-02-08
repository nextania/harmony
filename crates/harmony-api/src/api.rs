use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::{
    CallMember, Channel, CreateCallTokenResponse, GetCallMembersResponse, HarmonyClient, Invite,
    Message, StartCallResponse, UpdateVoiceStateResponse,
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
    pub async fn send_message(&self, channel_id: &str, content: &str) -> Result<Message> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params {
            channel_id: String,
            content: String,
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
                    content: content.to_string(),
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
}
