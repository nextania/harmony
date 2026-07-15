use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::sync::broadcast::{self, error::RecvError};

use crate::error::HarmonyResult;
use crate::managers::{ChannelManager, UserManager};
use crate::models::*;
use crate::session::Session;

#[derive(uniffi::Object)]
pub struct EncryptedClient {
    inner: Arc<harmony_api::EncryptedClient>,
    recv: Arc<Mutex<broadcast::Receiver<harmony_api::EncryptedEvent>>>,
}

#[uniffi::export]
impl EncryptedClient {
    #[uniffi::constructor]
    pub async fn connect(session: Arc<Session>, options: ClientOptions) -> HarmonyResult<Arc<Self>> {
        let (inner, receiver) =
            harmony_api::EncryptedClient::connect(session.inner.clone(), options.into()).await?;
        Ok(Arc::new(Self {
            inner,
            recv: Arc::new(Mutex::new(receiver)),
        }))
    }

    pub async fn next_event(&self) -> HarmonyResult<Event> {
        let mut recv = self.recv.lock().await;
        loop {
            match recv.recv().await {
                Ok(harmony_api::EncryptedEvent::Lifecycle(_)) => {}
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

    pub fn user_id(&self) -> String {
        self.inner.user_id().to_string()
    }

    pub fn channels(&self) -> Arc<ChannelManager> {
        ChannelManager::new(self.inner.channels().clone())
    }

    pub fn users(&self) -> Arc<UserManager> {
        UserManager::from_inner(self.inner.users().clone())
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
}
