use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use quick_cache::sync::Cache;

use crate::{
    Result,
    encrypted_client::Core,
    models::{ChannelData, Message},
};

const MESSAGE_CACHE_CAPACITY: usize = 1000;

#[derive(Clone, Debug)]
pub struct DecryptedMessage {
    pub message: Message,
    pub content: Vec<u8>,
}

struct MessageStore {
    loaded: AtomicBool,
    messages: Cache<String, DecryptedMessage>,
}

impl MessageStore {
    fn new() -> Self {
        Self {
            loaded: AtomicBool::new(false),
            messages: Cache::new(MESSAGE_CACHE_CAPACITY),
        }
    }

    fn is_loaded(&self) -> bool {
        self.loaded.load(Ordering::Acquire)
    }

    fn snapshot(&self) -> Vec<DecryptedMessage> {
        let mut messages: Vec<DecryptedMessage> =
            self.messages.iter().map(|(_, value)| value).collect();
        messages.sort_by(|a, b| a.message.id.cmp(&b.message.id));
        messages
    }

    fn store_history(&self, history: &[DecryptedMessage]) {
        for message in history {
            self.messages
                .insert(message.message.id.clone(), message.clone());
        }
        self.loaded.store(true, Ordering::Release);
    }

    fn upsert(&self, message: DecryptedMessage) {
        if self.is_loaded() {
            self.messages.insert(message.message.id.clone(), message);
        }
    }

    fn remove(&self, message_id: &str) {
        self.messages.remove(message_id);
    }
}

#[derive(Clone)]
pub struct Channel {
    data: ChannelData,
    core: Arc<Core>,
    messages: Arc<MessageStore>,
}

impl std::fmt::Debug for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Channel").field("id", &self.id()).finish()
    }
}

impl Channel {
    pub(crate) fn new(data: ChannelData, core: Arc<Core>) -> Self {
        Self {
            data,
            core,
            messages: Arc::new(MessageStore::new()),
        }
    }

    pub(crate) fn with_data(&self, data: ChannelData) -> Self {
        Self {
            data,
            core: self.core.clone(),
            messages: self.messages.clone(),
        }
    }

    pub fn data(&self) -> &ChannelData {
        &self.data
    }

    pub fn id(&self) -> &str {
        self.data.id()
    }

    pub async fn messages(&self) -> Result<Vec<DecryptedMessage>> {
        if self.messages.is_loaded() {
            return Ok(self.messages.snapshot());
        }

        let messages = self
            .core
            .client
            .get_messages(self.id(), Some(50), None, None, None)
            .await?;

        let mut result = Vec::with_capacity(messages.len());
        for message in messages {
            let content = self.core.decrypt_content(&self.data, &message).await?;
            result.push(DecryptedMessage { message, content });
        }

        self.messages.store_history(&result);
        Ok(self.messages.snapshot())
    }

    fn cache_message(&self, message: &Message, content: &[u8]) {
        self.messages.upsert(DecryptedMessage {
            message: message.clone(),
            content: content.to_vec(),
        });
    }

    pub async fn send_message(&self, content: &[u8]) -> Result<Message> {
        let encrypted = self.core.encrypt_content(&self.data, content).await?;
        let message = self.core.client.send_message(self.id(), encrypted).await?;
        self.cache_message(&message, content);
        Ok(message)
    }

    pub async fn edit_message(&self, message_id: &str, content: &[u8]) -> Result<Message> {
        let encrypted = self.core.encrypt_content(&self.data, content).await?;
        let message = self.core.client.edit_message(message_id, encrypted).await?;
        self.cache_message(&message, content);
        Ok(message)
    }

    pub async fn delete_message(&self, message_id: &str) -> Result<()> {
        self.core.client.delete_message(message_id).await?;
        self.remove_cached(message_id);
        Ok(())
    }

    pub(crate) async fn receive_message(&self, msg: &Message) -> Result<Vec<u8>> {
        let content = self.decrypt_message(msg).await?;
        self.cache_message(msg, &content);
        Ok(content)
    }

    pub(crate) fn remove_cached(&self, message_id: &str) {
        self.messages.remove(message_id);
    }

    pub async fn decrypt_message(&self, msg: &Message) -> Result<Vec<u8>> {
        self.core.decrypt_content(&self.data, msg).await
    }
}
