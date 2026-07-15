use std::{collections::HashMap, sync::Arc};

use chacha20poly1305::{Key, aead::Generate};
use quick_cache::sync::Cache;

use crate::{
    Result,
    channel::Channel,
    crypto::{GROUP_METADATA_AAD, PersistentEncryption},
    encrypted_client::Core,
    error::HarmonyError,
    models::{ChannelData, EncryptionHint},
    user_manager::UserManager,
};

pub struct ChannelManager {
    core: Arc<Core>,
    users: Arc<UserManager>,
    cache: Cache<String, Channel>,
}

impl ChannelManager {
    pub(crate) fn new(core: Arc<Core>, users: Arc<UserManager>) -> Self {
        Self {
            core,
            users,
            cache: Cache::new(100),
        }
    }

    fn wrap(&self, data: ChannelData) -> Channel {
        match self.cache.get(data.id()) {
            Some(existing) => existing.with_data(data),
            None => Channel::new(data, self.core.clone()),
        }
    }

    /// Gets a channel if cached.
    pub fn get(&self, id: &str) -> Option<Channel> {
        self.cache.get(id)
    }

    pub async fn fetch_personal(&self) -> Result<HashMap<String, Channel>> {
        let channels = self.core.client.get_channels().await?;
        let mut user_ids = vec![self.core.user_id.to_string()];
        for ch in &channels {
            match ch {
                ChannelData::PrivateChannel {
                    initiator_id,
                    target_id,
                    ..
                } => {
                    user_ids.push(initiator_id.clone());
                    user_ids.push(target_id.clone());
                }
                ChannelData::GroupChannel { members, .. } => {
                    user_ids.extend(members.iter().map(|m| m.id.clone()));
                }
            }
        }
        user_ids.sort();
        user_ids.dedup();
        if let Err(e) = self.users.fetch_bulk(user_ids).await {
            tracing::warn!("failed to prefetch channel member profiles: {e}");
        }
        let channels = channels
            .into_iter()
            .map(|c| (c.id().to_string(), self.update(c)))
            .collect::<HashMap<_, _>>();
        Ok(channels)
    }

    pub async fn fetch(&self, channel_id: &str) -> Result<Channel> {
        if let Some(channel) = self.cache.get(channel_id) {
            return Ok(channel);
        }
        let channel = self.core.client.get_channel(channel_id).await?;
        Ok(self.update(channel))
    }

    pub async fn create_private_channel(&self, target_id: &str) -> Result<Channel> {
        let channel = self.core.client.create_private_channel(target_id).await?;
        Ok(self.update(channel))
    }

    pub async fn create_group_channel(&self, metadata_plaintext: &[u8]) -> Result<Channel> {
        let gen_key = Key::generate();
        let mut key = [0u8; 32];
        key.copy_from_slice(&gen_key);
        let encrypted_metadata =
            PersistentEncryption::encrypt_with_key(&key, metadata_plaintext, GROUP_METADATA_AAD);
        let channel = self
            .core
            .client
            .create_group_channel(encrypted_metadata, EncryptionHint::Persistent)
            .await?;
        let channel_id = channel.id().to_string();
        {
            let mut ks = self.core.keystore.lock().await;
            ks.store_group_key(&channel_id, &key);
        }
        self.core.sync_keystore().await?;
        Ok(self.update(channel))
    }

    pub async fn create_group_invite(&self, channel_id: &str) -> Result<String> {
        let invite = self
            .core
            .client
            .create_invite(channel_id, Some(1), None, None)
            .await?;
        Ok(invite.code)
    }

    pub async fn join_group(&self, invite_code: &str, group_key: &[u8]) -> Result<Channel> {
        let (_pending, channel_id) = self.core.client.accept_invite(invite_code).await?;
        if group_key.len() != 32 {
            return Err(HarmonyError::InvalidGroupKeyLength(group_key.len()));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(group_key);
        {
            let mut ks = self.core.keystore.lock().await;
            ks.store_group_key(&channel_id, &key);
        }
        self.core.sync_keystore().await?;
        self.fetch(&channel_id).await
    }

    pub async fn get_group_key(&self, channel_id: &str) -> Option<Vec<u8>> {
        let ks = self.core.keystore.lock().await;
        ks.get_group_key(channel_id).map(|k| k.to_vec())
    }

    pub(crate) fn update(&self, channel: ChannelData) -> Channel {
        let channel = self.wrap(channel);
        self.cache.insert(channel.id().to_string(), channel.clone());
        channel
    }

    pub(crate) fn invalidate(&self, channel_id: &str) {
        self.cache.remove(channel_id);
    }
}
