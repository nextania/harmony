use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub content: Vec<u8>,
    pub author_id: String,
    pub edited_at: Option<i64>,
    pub channel_id: String,
    // Private messages will have a key ID,
    // since a new key is generated each time a user establishes
    // a relationship with another user
    pub key_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMessagesMethod {
    pub channel_id: String,
    pub limit: Option<i64>,
    pub latest: Option<bool>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GetMessagesResponse {
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageMethod {
    pub channel_id: String,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendMessageResponse {
    pub message: Message,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMessageMethod {
    pub message_id: String,
    pub content: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditMessageResponse {
    pub message: Message,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMessageMethod {
    pub message_id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteMessageResponse {}
