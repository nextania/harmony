use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use ulid::Ulid;

use super::users::User;
use crate::errors::Result;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Scope {
    id: String,
    name: String,
    disabled: bool,
    nexuses: Vec<String>,
    channels: Vec<String>,
    users: Vec<String>,
}

impl Scope {
    // Must be platform_administrator
    pub async fn create(name: String) -> Result<()> {
        let scopes = super::get_database().collection::<Scope>("scopes");
        scopes
            .insert_one(Scope {
                id: Ulid::new().to_string(),
                name,
                disabled: false,
                nexuses: Vec::new(),
                channels: Vec::new(),
                users: Vec::new(),
            })
            .await?;
        Ok(())
    }
    pub async fn update(
        &self,
        name: Option<String>,
        disabled: Option<bool>,
        add_users: Vec<String>,
        remove_users: Vec<String>,
    ) -> Result<bool> {
        let scopes = super::get_database().collection::<Scope>("scopes");
        let scope = scopes
            .find_one(doc! {
                "id": &self.id
            })
            .await?;
        match scope {
            Some(mut s) => {
                if let Some(n) = name {
                    s.name = n;
                }
                if let Some(d) = disabled {
                    s.disabled = d;
                }
                for ru in remove_users {
                    let index = s.users.iter().position(|x| *x == ru);
                    if let Some(i) = index {
                        s.users.remove(i);
                    }
                }
                for au in add_users {
                    User::get(&au).await?;
                    s.users.push(au);
                }
                Ok(true)
            }
            None => Ok(false),
        }
    }
    // TODO: warn user that this is a destructive action
    pub async fn delete_scope(&self) -> Result<bool> {
        if &self.id == "global" {
            Ok(false) // The global scope may not be deleted
        } else {
            let scopes = super::get_database().collection::<Scope>("scopes");
            scopes
                .delete_one(doc! {
                    "id": &self.id
                })
                .await?;
            Ok(true)
        }
    }
}
