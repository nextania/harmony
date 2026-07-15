use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::sync::broadcast::{self, error::RecvError};

use crate::error::HarmonyResult;
use crate::models::*;
use crate::session::Session;

#[derive(uniffi::Object)]
pub struct HarmonyClient {
    inner: Arc<harmony_api::HarmonyClient>,
    recv: Arc<Mutex<broadcast::Receiver<harmony_api::ClientEvent>>>,
}

#[uniffi::export]
impl HarmonyClient {
    #[uniffi::constructor]
    pub async fn new(session: Arc<Session>, config: ClientOptions) -> HarmonyResult<Arc<Self>> {
        let (inner, receiver) =
            harmony_api::HarmonyClient::new(session.inner.clone(), config.into()).await?;

        Ok(Arc::new(Self {
            inner: Arc::new(inner),
            recv: Arc::new(Mutex::new(receiver)),
        }))
    }

    pub async fn next_event(&self) -> HarmonyResult<Event> {
        let mut recv = self.recv.lock().await;
        loop {
            match recv.recv().await {
                Ok(event) => return Ok(event.into()),
                Err(RecvError::Lagged(missed)) => {
                    tracing::warn!("event receiver lagged; {missed} events dropped");
                }
                Err(RecvError::Closed) => {
                    return Err(crate::HarmonyBindingError::NotConnected);
                }
            }
        }
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

    pub async fn create_private_channel(&self, target_id: String) -> HarmonyResult<Channel> {
        Ok(self.inner.create_private_channel(&target_id).await?.into())
    }

    pub async fn create_group_channel(
        &self,
        metadata: Vec<u8>,
        encryption_hint: EncryptionHint,
    ) -> HarmonyResult<Channel> {
        Ok(self
            .inner
            .create_group_channel(metadata, encryption_hint.into())
            .await?
            .into())
    }

    pub async fn edit_channel(
        &self,
        channel_id: String,
        metadata: Vec<u8>,
    ) -> HarmonyResult<Channel> {
        Ok(self.inner.edit_channel(&channel_id, metadata).await?.into())
    }

    pub async fn delete_channel(&self, channel_id: String) -> HarmonyResult<()> {
        self.inner.delete_channel(&channel_id).await?;
        Ok(())
    }

    pub async fn leave_channel(&self, channel_id: String) -> HarmonyResult<()> {
        self.inner.leave_channel(&channel_id).await?;
        Ok(())
    }

    pub async fn edit_message(
        &self,
        message_id: String,
        content: String,
    ) -> HarmonyResult<Message> {
        Ok(self
            .inner
            .edit_message(&message_id, content.into_bytes())
            .await?
            .into())
    }

    pub async fn delete_message(&self, message_id: String) -> HarmonyResult<()> {
        self.inner.delete_message(&message_id).await?;
        Ok(())
    }

    pub async fn accept_invite(&self, code: String) -> HarmonyResult<AcceptInviteResult> {
        let (pending, channel_id) = self.inner.accept_invite(&code).await?;
        Ok(AcceptInviteResult {
            pending,
            channel_id,
        })
    }

    pub async fn set_key_package(
        &self,
        encrypted_keys: Vec<u8>,
        expected_generation: u64,
    ) -> HarmonyResult<u64> {
        Ok(self
            .inner
            .set_key_package(encrypted_keys, expected_generation)
            .await?)
    }

    pub async fn get_user(&self, user_id: String) -> HarmonyResult<UserProfile> {
        Ok(self.inner.get_user(&user_id).await?.into())
    }

    pub async fn get_current_user(&self) -> HarmonyResult<CurrentUserResponse> {
        Ok(self.inner.get_current_user().await?.into())
    }

    pub async fn add_contact(&self, stage: AddContactStage) -> HarmonyResult<AddContactResponse> {
        Ok(self.inner.add_contact(stage.into()).await?.into())
    }

    pub async fn remove_contact(&self, user_id: String) -> HarmonyResult<()> {
        self.inner.remove_contact(&user_id).await?;
        Ok(())
    }

    pub async fn get_contacts(&self) -> HarmonyResult<Vec<ContactExtended>> {
        let contacts = self
            .inner
            .get_contacts()
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(contacts)
    }

    pub async fn block_contact(&self, user_id: String) -> HarmonyResult<()> {
        self.inner.block_contact(&user_id).await?;
        Ok(())
    }

    pub async fn unblock_contact(&self, user_id: String) -> HarmonyResult<ContactExtended> {
        Ok(self.inner.unblock_contact(&user_id).await?.into())
    }
}
