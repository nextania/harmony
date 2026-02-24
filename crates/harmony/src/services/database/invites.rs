use mongodb::bson::doc;
use rand::distr::{Alphanumeric, SampleString};
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
    pub expires_at: Option<i64>,
    pub max_uses: Option<i32>,
    pub uses: Vec<String>,
    pub authorized_users: Option<Vec<String>>,
}

impl Invite {
    pub async fn create(
        channel_id: String,
        creator: String,
        expires_at: Option<i64>,
        max_uses: Option<i32>,
        authorized_users: Option<Vec<String>>,
    ) -> Result<Invite> {
        let invite = Invite {
            id: Ulid::new().to_string(),
            code: generate_code(),
            channel_id,
            creator,
            expires_at,
            max_uses,
            uses: Vec::new(),
            authorized_users,
        };
        let database = super::get_database();
        database
            .collection::<Invite>("invites")
            .insert_one(invite.clone())
            .await?;
        Ok(invite)
    }

    pub async fn get(id: &String) -> Result<Invite> {
        let database = super::get_database();
        let invite = database
            .collection::<Invite>("invites")
            .find_one(doc! {
                "id": id,
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

    pub async fn increment_uses(&self, user_id: &str) -> Result<()> {
        let database = super::get_database();
        database
            .collection::<Invite>("invites")
            .update_one(
                doc! { "id": &self.id },
                doc! {
                    "$push": { "uses": user_id }
                },
            )
            .await?;
        if let Some(max_uses) = self.max_uses {
            if (self.uses.len() as i32 + 1) >= max_uses {
                self.delete().await?;
            }
        }
        Ok(())
    }
}

pub fn generate_code() -> String {
    Alphanumeric.sample_string(&mut rand::rng(), 7)
}

impl From<Invite> for harmony_types::invites::Invite {
    fn from(i: Invite) -> Self {
        harmony_types::invites::Invite {
            id: i.id,
            code: i.code,
            channel_id: i.channel_id,
            creator: i.creator,
            expires_at: i.expires_at,
            max_uses: i.max_uses,
            uses: i.uses,
            authorized_users: i.authorized_users,
        }
    }
}
