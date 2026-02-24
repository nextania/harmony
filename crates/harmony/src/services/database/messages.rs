use mongodb::bson::{doc, Binary, spec::BinarySubtype};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::errors::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reaction {
    pub user_id: String,
    pub data: Vec<u8>, 
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub(crate) id: String,
    pub(crate) content: Vec<u8>, // encrypted content (including media IDs)
    pub(crate) reactions: Vec<Reaction>,
    pub(crate) author_id: String,
    pub(crate) edited_at: Option<i64>,
    pub(crate) channel_id: String,
}

impl Message {
    pub async fn get(id: &str) -> Result<Message> {
        let database = super::get_database();
        let message = database
            .collection::<Message>("messages")
            .find_one(doc! { "id": id })
            .await?;
        match message {
            Some(message) => Ok(message),
            None => Err(Error::NotFound),
        }
    }

    pub async fn ephemeral(_channel_id: &str, _author_id: &str, _content: &[u8]) -> Result<Message> {
        //  TODO: MLS messages are ephemeral, but they should still be stored
        // for any offline users for a short period of time
        todo!()
    }

    pub async fn create(channel_id: &str, author_id: &str, content: &[u8]) -> Result<Message> {
        let message = Message {
            id: Ulid::new().to_string(),
            content: content.to_vec(),
            author_id: author_id.to_string(),
            edited_at: None,
            channel_id: channel_id.to_string(),
            reactions: Vec::new(),
        };
        let database = super::get_database();
        database
            .collection::<Message>("messages")
            .insert_one(message.clone())
            .await?;
        Ok(message)
    }
    pub async fn edit(&self, content: Vec<u8>) -> Result<Message> {
        let database = super::get_database();
        let message = database
            .collection::<Message>("messages")
            .find_one_and_update(
                doc! { "id": &self.id },
                doc! { "$set": {
                    "content": Binary { subtype: BinarySubtype::Generic, bytes: content },
                    "edited_at": chrono::Utc::now().timestamp_millis(),
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

    pub async fn delete_in(channel_id: &str) -> Result<()> {
        let database = super::get_database();
        database
            .collection::<Message>("messages")
            .delete_many(doc! { "channel_id": channel_id })
            .await?;
        Ok(())
    }
}
