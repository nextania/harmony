use quick_cache::sync::Cache;

use crate::{Result, client::HarmonyClient, models::Channel};

pub struct ChannelManager {
    client: HarmonyClient,
    cache: Cache<String, Channel>,
}

impl ChannelManager {
    pub fn new(client: HarmonyClient) -> Self {
        Self {
            client,
            cache: Cache::new(100),
        }
    }

    pub async fn get_channel(&self, channel_id: &str) -> Result<Channel> {
        if let Some(channel) = self.cache.get(channel_id) {
            return Ok(channel);
        }
        let channel = self.client.get_channel(channel_id).await?;
        self.cache.insert(channel_id.to_string(), channel.clone());
        Ok(channel)
    }

    pub(crate) fn update(&self, channel: Channel) {
        self.cache.insert(channel.id().to_string(), channel);
    }

    pub(crate) fn invalidate(&self, channel_id: &str) {
        self.cache.remove(channel_id);
    }
}
