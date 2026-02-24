use mongodb::bson::doc;
use serde::{Deserialize, Serialize};

use crate::errors::Result;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Call {
    pub id: String,
    pub name: Option<String>,
    pub channel_id: String,
    pub joined_members: Vec<String>,
    pub ended_at: i64, // last check: this will be useful if the server goes down
    pub initiator: String, // user id of who started the call
}

impl Call {
    pub async fn create(&self) -> Result<()> {
        let database = super::get_database();
        database
            .collection::<Call>("calls")
            .insert_one(self.clone())
            .await?;
        Ok(())
    }

    pub async fn update(id: &String, members: Vec<String>) -> Result<()> {
        let database = super::get_database();
        database
            .collection::<Call>("calls")
            .update_one(
                doc! {
                    "id": id,
                },
                doc! {
                    "$addToSet": {
                        "joined_members": {
                            "$each": members
                        }
                    },
                    "$set": {
                        "ended_at": chrono::Utc::now().timestamp_millis(),
                    },
                },
            )
            .await?;
        Ok(())
    }
}
