use std::{num::NonZeroUsize, sync::Arc};

use harmony_api::{Channel, HarmonyClient};
use lru::LruCache;
use tokio::sync::Mutex;

use crate::errors::RenderableResult;

pub struct ChannelManager {
    client: HarmonyClient,
    cache: Mutex<LruCache<String, Channel>>,
}

impl ChannelManager {
    pub fn new(client: HarmonyClient) -> Arc<Self> {
        Arc::new(Self {
            client,
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(100).unwrap())),
        })
    }

    pub async fn get_channel(&self, channel_id: &str) -> RenderableResult<Channel> {
        if let Some(channel) = self.cache.lock().await.get(channel_id).cloned() {
            return Ok(channel);
        }
        let channel = self.client.get_channel(channel_id).await?;
        self.cache
            .lock()
            .await
            .put(channel_id.to_string(), channel.clone());
        Ok(channel)
    }
}
