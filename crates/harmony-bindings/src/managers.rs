use std::sync::Arc;

use crate::client::HarmonyClient;
use crate::error::HarmonyResult;
use crate::models::*;

#[derive(uniffi::Object)]
pub struct ChannelManager {
    inner: harmony_api::ChannelManager,
}

#[uniffi::export]
impl ChannelManager {
    #[uniffi::constructor]
    pub fn new(client: Arc<HarmonyClient>) -> Arc<Self> {
        Self {
            inner: harmony_api::ChannelManager::new((*client.inner()).clone()),
        }
        .into()
    }

    pub async fn get_channel(&self, channel_id: String) -> HarmonyResult<Channel> {
        Ok(self.inner.get_channel(&channel_id).await?.into())
    }
}

#[derive(uniffi::Object)]
pub struct UserManager {
    inner: harmony_api::UserManager,
}

#[uniffi::export]
impl UserManager {
    #[uniffi::constructor]
    pub fn new(base_url: String, token: String) -> Arc<Self> {
        Self {
            inner: harmony_api::UserManager::new(reqwest::Client::new(), base_url, token),
        }
        .into()
    }

    pub async fn get_user(&self, user_id: String) -> HarmonyResult<PublicUser> {
        Ok(self.inner.get_user(&user_id).await?.into())
    }

    pub async fn get_user_by_username(&self, username: String) -> HarmonyResult<PublicUser> {
        Ok(self.inner.get_user_by_username(&username).await?.into())
    }

    pub async fn get_users(&self, user_ids: Vec<String>) -> HarmonyResult<Vec<PublicUser>> {
        let users = self
            .inner
            .get_users(user_ids)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(users)
    }
}
