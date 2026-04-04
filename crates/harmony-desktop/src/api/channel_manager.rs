use std::sync::Arc;

use harmony_api::{Channel, HarmonyClient};
use quick_cache::sync::Cache;

use crate::errors::RenderableResult;

pub struct ChannelManager {
    client: HarmonyClient,
    cache: Cache<String, Channel>,
}

impl ChannelManager {
    pub fn new(client: HarmonyClient) -> Arc<Self> {
        Arc::new(Self {
            client,
            cache: Cache::new(100),
        })
    }

    pub async fn get_channel(&self, channel_id: &str) -> RenderableResult<Channel> {
        if let Some(channel) = self.cache.get(channel_id) {
            return Ok(channel);
        }
        let channel = self.client.get_channel(channel_id).await?;
        self.cache.insert(channel_id.to_string(), channel.clone());
        Ok(channel)
    }
}
