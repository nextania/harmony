use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::errors::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub(crate) id: String,
    pub(crate) content: String,
    pub(crate) author_id: String,
    pub(crate) created_at: i64,
    pub(crate) edited: bool,
    pub(crate) edited_at: Option<i64>,
    pub(crate) channel_id: String,
}

impl Message {
    pub async fn create(channel_id: String, author_id: String, content: String) -> Result<Message> {
        let message = Message {
            id: Ulid::new().to_string(),
            content,
            author_id,
            created_at: chrono::Utc::now().timestamp_millis(),
            edited: false,
            edited_at: None,
            channel_id,
        };
        let database = super::get_database();
        database
            .collection::<Message>("messages")
            .insert_one(message.clone())
            .await?;
        Ok(message)
    }
    pub async fn edit(&self, content: String) -> Result<Message> {
        let database = super::get_database();
        let message = database
            .collection::<Message>("messages")
            .find_one_and_update(
                doc! { "id": &self.id },
                doc! { "$set": {
                    "content": content,
                    "edited": true,
                    "editedAt": chrono::Utc::now().timestamp_millis(),
                } },
            )
            .await?;
        match message {
            Some(message) => Ok(message),
            None => Err(Error::NotFound),
        }
    }

    pub async fn delete(&self) -> Result<Message> {
        let database = super::get_database();
        let message = database
            .collection::<Message>("messages")
            .find_one_and_delete(doc! { "id": &self.id })
            .await?;
        match message {
            Some(message) => Ok(message),
            None => Err(Error::NotFound),
        }
    }
}
