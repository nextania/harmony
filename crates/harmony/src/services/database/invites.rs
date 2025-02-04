use mongodb::bson::doc;
use rand::distributions::{Alphanumeric, DistString};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::errors::{Error, Result};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Invite {
    pub id: String,
    pub code: String,
    pub channel_id: String,
    pub creator: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub max_uses: Option<i32>,
    pub uses: Vec<String>,
    pub authorized_users: Option<Vec<String>>,
    pub space_id: Option<String>,
    pub scope_id: String,
}

impl Invite {
    pub async fn create(
        channel_id: String,
        creator: String,
        expires_at: Option<u64>,
        max_uses: Option<i32>,
        authorized_users: Option<Vec<String>>,
        space_id: Option<String>,
        scope_id: Option<String>,
    ) -> Result<Invite> {
        let invite = Invite {
            id: Ulid::new().to_string(),
            code: generate_code(),
            channel_id,
            creator,
            created_at: chrono::Utc::now().timestamp_millis() as u64,
            expires_at,
            max_uses,
            uses: Vec::new(),
            authorized_users,
            space_id,
            scope_id: scope_id.unwrap_or_else(|| "global".to_owned()),
        };
        let database = super::get_database();
        database
            .collection::<Invite>("invites")
            .insert_one(invite.clone())
            .await?;
        Ok(invite)
    }

    pub async fn get(code: &String) -> Result<Invite> {
        let database = super::get_database();
        let invite = database
            .collection::<Invite>("invites")
            .find_one(doc! {
                "id": code,
            })
            .await?;
        match invite {
            Some(invite) => Ok(invite),
            None => Err(Error::NotFound),
        }
    }
    pub async fn delete(&self) -> Result<bool> {
        let database = super::get_database();
        let result = database
            .collection::<Invite>("invites")
            .delete_one(doc! {
                "id": &self.id,
            })
            .await?
            .deleted_count
            > 0;
        Ok(result)
    }
}

pub fn generate_code() -> String {
    Alphanumeric.sample_string(&mut rand::thread_rng(), 7)
}

// pub async fn update_invite(code: String) -> Option<Invite> {

// }

