use std::sync::Arc;

use crate::error::HarmonyResult;
use crate::models::*;

#[derive(uniffi::Object)]
pub struct ChannelManager {
    inner: Arc<harmony_api::ChannelManager>,
}

impl ChannelManager {
    pub(crate) fn new(inner: Arc<harmony_api::ChannelManager>) -> Arc<Self> {
        Self { inner }.into()
    }
}

#[uniffi::export]
impl ChannelManager {
    pub async fn get_channel(&self, channel_id: String) -> HarmonyResult<Channel> {
        Ok(self.inner.fetch(&channel_id).await?.data().clone().into())
    }

    pub async fn create_private_channel(&self, target_id: String) -> HarmonyResult<Channel> {
        Ok(self
            .inner
            .create_private_channel(&target_id)
            .await?
            .data()
            .clone()
            .into())
    }

    pub async fn create_group_channel(&self, metadata: Vec<u8>) -> HarmonyResult<Channel> {
        Ok(self
            .inner
            .create_group_channel(&metadata)
            .await?
            .data()
            .clone()
            .into())
    }

    pub async fn create_group_invite(&self, channel_id: String) -> HarmonyResult<String> {
        Ok(self.inner.create_group_invite(&channel_id).await?)
    }

    pub async fn join_group(
        &self,
        invite_code: String,
        group_key: Vec<u8>,
    ) -> HarmonyResult<String> {
        Ok(self
            .inner
            .join_group(&invite_code, &group_key)
            .await?
            .id()
            .to_string())
    }

    pub async fn get_group_key(&self, channel_id: String) -> Option<Vec<u8>> {
        self.inner.get_group_key(&channel_id).await
    }
}

#[derive(uniffi::Object)]
pub struct UserManager {
    inner: Arc<harmony_api::UserManager>,
}

impl UserManager {
    pub(crate) fn from_inner(inner: Arc<harmony_api::UserManager>) -> Arc<Self> {
        Self { inner }.into()
    }
}

#[uniffi::export]
impl UserManager {
    pub async fn get_user(&self, user_id: String) -> HarmonyResult<PublicUser> {
        Ok(self.inner.fetch(&user_id).await?.into())
    }

    pub async fn get_user_by_username(&self, username: String) -> HarmonyResult<PublicUser> {
        Ok(self.inner.fetch_by_username(&username).await?.into())
    }

    pub async fn get_users(&self, user_ids: Vec<String>) -> HarmonyResult<Vec<PublicUser>> {
        let users = self
            .inner
            .fetch_bulk(user_ids)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(users)
    }
}
