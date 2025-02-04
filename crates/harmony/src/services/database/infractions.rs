use futures_util::StreamExt;
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use crate::errors::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Infraction {
    pub id: String,
    pub infraction_type: InfractionType,
    pub space_id: String,
    pub member_id: String,
    pub reason: String,
    pub expires_at: Option<i64>,
    pub created_at: i64,
    pub created_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InfractionType {
    Suspend,
    Kick,
    Ban,
}

pub async fn create_ban(
    space_id: String,
    member_id: String,
    reason: String,
    expires_at: Option<i64>,
    created_by: String,
) -> Result<()> {
    let database = super::get_database();
    let ban = Infraction {
        id: Ulid::new().to_string(),
        infraction_type: InfractionType::Ban,
        space_id,
        member_id,
        reason,
        expires_at,
        created_at: chrono::Utc::now().timestamp_millis(),
        created_by,
    };
    database
        .collection::<Infraction>("infraction")
        .insert_one(ban)
        .await?;
    Ok(())
}

pub async fn is_banned(user_id: String, space_id: String) -> Result<bool> {
    let database = super::get_database();
    let bans = database
        .collection::<Infraction>("infractions")
        .find(doc! {
            "member_id": user_id,
            "space_id": space_id,
            "expires_at": {
                "$gt": chrono::Utc::now().timestamp_millis()
            },
            "infraction_type": "BAN"
        })
        .await?;
    let is_banned = bans.count().await > 0;
    Ok(is_banned)
}

pub async fn revoke_ban(ban_id: String) -> Result<()> {
    let database = super::get_database();
    database
        .collection::<Infraction>("infractions")
        .delete_one(doc! {
            "id": ban_id,
            "infraction_type": "BAN"
        })
        .await?;
    Ok(())
}
