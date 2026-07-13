use std::sync::Arc;

use crate::client::HarmonyClient;
use crate::error::HarmonyResult;
use crate::models::*;

#[derive(uniffi::Object)]
pub struct EncryptedClient {
    inner: Arc<harmony_api::EncryptedClient>,
}

#[uniffi::export]
impl EncryptedClient {
    #[uniffi::constructor]
    pub async fn connect(
        client: Arc<HarmonyClient>,
        encrypted_key: String,
        password: String,
    ) -> HarmonyResult<Arc<Self>> {
        let inner = harmony_api::EncryptedClient::connect(
            (*client.inner()).clone(),
            encrypted_key,
            password,
        )
        .await?;
        Ok(Arc::new(Self { inner }))
    }

    pub fn user_id(&self) -> String {
        self.inner.user_id().to_string()
    }

    pub async fn sync_keystore(&self) -> HarmonyResult<()> {
        self.inner.sync_keystore().await?;
        Ok(())
    }

    pub async fn encrypt_content(
        &self,
        channel_id: String,
        plaintext: Vec<u8>,
    ) -> HarmonyResult<Vec<u8>> {
        Ok(self.inner.encrypt_content(&channel_id, &plaintext).await?)
    }

    pub async fn decrypt_content(&self, message: Message) -> HarmonyResult<Vec<u8>> {
        let msg: harmony_api::Message = message.into();
        Ok(self.inner.decrypt_content(&msg).await?)
    }

    pub async fn add_contact(&self, action: ContactAction) -> HarmonyResult<AddContactOutcome> {
        Ok(self.inner.add_contact(action.into()).await?.into())
    }

    pub async fn create_group_channel(&self, metadata: Vec<u8>) -> HarmonyResult<Channel> {
        Ok(self.inner.create_group_channel(&metadata).await?.into())
    }

    pub async fn create_group_invite(&self, channel_id: String) -> HarmonyResult<String> {
        Ok(self.inner.create_group_invite(&channel_id).await?)
    }

    pub async fn join_group(
        &self,
        invite_code: String,
        group_key: Vec<u8>,
    ) -> HarmonyResult<String> {
        Ok(self.inner.join_group(&invite_code, &group_key).await?)
    }

    pub async fn get_group_key(&self, channel_id: String) -> Option<Vec<u8>> {
        self.inner.get_group_key(&channel_id).await
    }
}
