use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::error::HarmonyResult;
use crate::models::*;

#[derive(uniffi::Object)]
pub struct HarmonyClient {
    inner: Arc<harmony_api::HarmonyClient>,
    recv: Arc<Mutex<UnboundedReceiver<harmony_api::Event>>>,
}

#[uniffi::export]
impl HarmonyClient {
    #[uniffi::constructor]
    pub async fn new(config: ClientOptions) -> HarmonyResult<Arc<Self>> {
        let (inner, receiver) = harmony_api::HarmonyClient::new(config.into()).await?;

        Ok(Arc::new(Self {
            inner: Arc::new(inner),
            recv: Arc::new(Mutex::new(receiver)),
        }))
    }

    pub async fn next_event(&self) -> HarmonyResult<Event> {
        let event = self.recv.lock()
            .await
            .recv()
            .await
            .ok_or(crate::HarmonyBindingError::NotConnected)?;
        Ok(event.into())
    }

    pub async fn get_channels(&self) -> HarmonyResult<Vec<Channel>> {
        let channels = self
            .inner
            .get_channels()
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(channels)
    }

    pub async fn get_channel(&self, channel_id: String) -> HarmonyResult<Channel> {
        let channel = self.inner.get_channel(&channel_id).await?.into();
        Ok(channel)
    }

    pub async fn get_messages(
        &self,
        channel_id: String,
        limit: Option<i64>,
        latest: Option<bool>,
        before: Option<String>,
        after: Option<String>,
    ) -> HarmonyResult<Vec<Message>> {
        let messages = self
            .inner
            .get_messages(&channel_id, limit, latest, before, after)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(messages)
    }

    pub async fn send_message(
        &self,
        channel_id: String,
        content: String,
    ) -> HarmonyResult<Message> {
        let message: Message = self
            .inner
            .send_message(&channel_id, content.into_bytes())
            .await?
            .into();
        Ok(message)
    }

    pub async fn create_invite(
        &self,
        channel_id: String,
        max_uses: Option<i32>,
        expires_at: Option<i64>,
        authorized_users: Option<Vec<String>>,
    ) -> HarmonyResult<Invite> {
        let invite = self
            .inner
            .create_invite(&channel_id, max_uses, expires_at, authorized_users)
            .await?
            .into();
        Ok(invite)
    }

    pub async fn get_invite(&self, invite_id: String) -> HarmonyResult<InviteInformation> {
        let invite = self.inner.get_invite(&invite_id).await?.invite.into();
        Ok(invite)
    }

    pub async fn get_invites(&self, channel_id: String) -> HarmonyResult<Vec<Invite>> {
        let invites: Vec<Invite> = self
            .inner
            .get_invites(channel_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(invites)
    }

    pub async fn delete_invite(&self, invite_id: String) -> HarmonyResult<()> {
        Ok(self.inner.delete_invite(&invite_id).await?)
    }

    pub async fn start_call(
        &self,
        channel_id: String,
        preferred_region: Option<Region>,
    ) -> HarmonyResult<StartCallResponse> {
        let response: StartCallResponse = self
            .inner
            .start_call(&channel_id, preferred_region.map(Into::into))
            .await?
            .into();
        Ok(response)
    }

    pub async fn create_call_token(
        &self,
        channel_id: String,
        initial_muted: bool,
        initial_deafened: bool,
    ) -> HarmonyResult<CreateCallTokenResponse> {
        let response = self
            .inner
            .create_call_token(&channel_id, initial_muted, initial_deafened)
            .await?
            .into();
        Ok(response)
    }

    pub async fn end_call(&self, channel_id: String) -> HarmonyResult<()> {
        self.inner.end_call(&channel_id).await?;
        Ok(())
    }

    pub async fn update_voice_state(
        &self,
        channel_id: String,
        muted: Option<bool>,
        deafened: Option<bool>,
    ) -> HarmonyResult<UpdateVoiceStateResponse> {
        let response = self
            .inner
            .update_voice_state(&channel_id, muted, deafened)
            .await?
            .into();
        Ok(response)
    }

    pub async fn get_call_members(&self, channel_id: String) -> HarmonyResult<Vec<CallMember>> {
        let members: Vec<CallMember> = self
            .inner
            .get_call_members(&channel_id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(members)
    }

    pub fn is_connected(&self) -> bool {
        self.inner.is_connected()
    }

    pub fn is_reconnecting(&self) -> bool {
        self.inner.is_reconnecting()
    }

    pub fn reconnect_attempts(&self) -> u32 {
        self.inner.reconnect_attempts()
    }

    pub fn disconnect(&self) -> HarmonyResult<()> {
        Ok(self.inner.disconnect()?)
    }
}
