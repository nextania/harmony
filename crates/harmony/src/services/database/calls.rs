use mongodb::bson::doc;
use serde::{Deserialize, Serialize};

use crate::errors::Result;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Call {
    pub id: String,
    pub name: Option<String>,
    pub channel_id: String,
    // TODO: ALL members who ever joined the call at any point
    pub joined_members: Vec<String>,
    // also stores last ping
    pub ended_at: i64, // can calculate duration
}

impl Call {
    // save in mongodb
    pub async fn create(&self) -> Result<()> {
        let database = super::get_database();
        database
            .collection::<Call>("calls")
            .insert_one(self.clone())
            .await?;
        Ok(())
    }
    // updating it periodically creates persistence
    pub async fn update(id: &String, members: Vec<String>) -> Result<()> {
        let database = super::get_database();
        database
            .collection::<Call>("calls")
            .update_one(
                doc! {
                    "id": id,
                },
                doc! {
                    "$set": {
                        "joined_members": members,
                        "ended_at": chrono::Utc::now().timestamp_millis(),
                    },
                },
            )
            .await?;
        Ok(())
    }
}
