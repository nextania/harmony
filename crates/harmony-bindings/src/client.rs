use std::sync::Arc;
use tokio::runtime::Runtime;

use crate::error::{HarmonyBindingError, HarmonyResult};
use crate::models::*;

#[derive(uniffi::Object)]
pub struct HarmonyClient {
    inner: Arc<harmony_api::HarmonyClient>,
    runtime: Arc<Runtime>,
}

#[uniffi::export]
impl HarmonyClient {
    #[uniffi::constructor]
    pub fn new(config: ClientOptions) -> HarmonyResult<Arc<Self>> {
        let runtime = Arc::new(Runtime::new().map_err(|e| HarmonyBindingError::Internal {
            reason: e.to_string(),
        })?);

        let inner =
            runtime.block_on(async { harmony_api::HarmonyClient::new(config.into()).await })?;

        Ok(Arc::new(Self {
            inner: Arc::new(inner),
            runtime,
        }))
    }

    pub fn get_channels(&self) -> HarmonyResult<Vec<Channel>> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let channels = inner.get_channels().await?;
                Ok::<Vec<Channel>, harmony_api::HarmonyError>(
                    channels.into_iter().map(Into::into).collect(),
                )
            })
            .map_err(Into::into)
    }

    pub fn get_channel(&self, channel_id: String) -> HarmonyResult<Channel> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let channel = inner.get_channel(&channel_id).await?;
                Ok::<Channel, harmony_api::HarmonyError>(channel.into())
            })
            .map_err(Into::into)
    }

    pub fn get_messages(
        &self,
        channel_id: String,
        limit: Option<i64>,
        latest: Option<bool>,
        before: Option<String>,
        after: Option<String>,
    ) -> HarmonyResult<Vec<Message>> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let messages = inner
                    .get_messages(&channel_id, limit, latest, before, after)
                    .await?;
                Ok::<Vec<Message>, harmony_api::HarmonyError>(
                    messages.into_iter().map(Into::into).collect(),
                )
            })
            .map_err(Into::into)
    }

    pub fn send_message(&self, channel_id: String, content: String) -> HarmonyResult<Message> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let message = inner.send_message(&channel_id, &content).await?;
                Ok::<Message, harmony_api::HarmonyError>(message.into())
            })
            .map_err(Into::into)
    }

    pub fn create_invite(
        &self,
        channel_id: String,
        max_uses: Option<i32>,
        expires_at: Option<u64>,
        authorized_users: Option<Vec<String>>,
    ) -> HarmonyResult<Invite> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let invite = inner
                    .create_invite(&channel_id, max_uses, expires_at, authorized_users)
                    .await?;
                Ok::<Invite, harmony_api::HarmonyError>(invite.into())
            })
            .map_err(Into::into)
    }

    pub fn get_invite(&self, invite_id: String) -> HarmonyResult<Invite> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let invite = inner.get_invite(&invite_id).await?;
                Ok::<Invite, harmony_api::HarmonyError>(invite.into())
            })
            .map_err(Into::into)
    }

    pub fn get_invites(&self, channel_id: String) -> HarmonyResult<Vec<Invite>> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let invites = inner.get_invites(channel_id).await?;
                Ok::<Vec<Invite>, harmony_api::HarmonyError>(
                    invites.into_iter().map(Into::into).collect(),
                )
            })
            .map_err(Into::into)
    }

    pub fn delete_invite(&self, invite_id: String) -> HarmonyResult<()> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                inner.delete_invite(&invite_id).await?;
                Ok::<(), harmony_api::HarmonyError>(())
            })
            .map_err(Into::into)
    }

    pub fn start_call(&self, channel_id: String) -> HarmonyResult<()> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                inner.start_call(&channel_id).await?;
                Ok::<(), harmony_api::HarmonyError>(())
            })
            .map_err(Into::into)
    }

    pub fn join_call(&self, channel_id: String, sdp: String) -> HarmonyResult<String> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                let response = inner.join_call(&channel_id, &sdp).await?;
                Ok::<String, harmony_api::HarmonyError>(response)
            })
            .map_err(Into::into)
    }

    pub fn leave_call(&self, channel_id: String) -> HarmonyResult<()> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                inner.leave_call(&channel_id).await?;
                Ok::<(), harmony_api::HarmonyError>(())
            })
            .map_err(Into::into)
    }

    pub fn end_call(&self, channel_id: String) -> HarmonyResult<()> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                inner.end_call(&channel_id).await?;
                Ok::<(), harmony_api::HarmonyError>(())
            })
            .map_err(Into::into)
    }

    pub fn is_connected(&self) -> bool {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move { inner.is_connected().await })
    }

    pub fn is_reconnecting(&self) -> bool {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move { inner.is_reconnecting().await })
    }

    pub fn reconnect_attempts(&self) -> u32 {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move { inner.reconnect_attempts().await })
    }

    pub fn reconnect(&self) -> HarmonyResult<()> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                inner.reconnect().await?;
                Ok::<(), harmony_api::HarmonyError>(())
            })
            .map_err(Into::into)
    }

    pub fn disconnect(&self) -> HarmonyResult<()> {
        let inner = self.inner.clone();
        self.runtime
            .block_on(async move {
                inner.disconnect().await?;
                Ok::<(), harmony_api::HarmonyError>(())
            })
            .map_err(Into::into)
    }
}
