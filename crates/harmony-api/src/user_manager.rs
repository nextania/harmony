use quick_cache::sync::Cache;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::{Result, error::HarmonyError};

// TODO: separate this into account

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicUser {
    pub id: String,
    pub username: String,
    pub display_name: String,
    pub description: String,
    pub avatar: Option<AvatarUrl>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvatarUrl {
    pub id: String,
    pub signature: String,
    pub timestamp: u64,
}

const CACHE_CAPACITY: usize = 512;

pub struct UserManager {
    cache: Cache<String, PublicUser>,
    http: Client,
    base_url: String,
    token: String,
}

impl UserManager {
    pub fn new(http: Client, base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            cache: Cache::new(CACHE_CAPACITY),
            http,
            base_url: base_url.into(),
            token: token.into(),
        }
    }

    pub async fn get_user(&self, user_id: &str) -> Result<PublicUser> {
        if let Some(profile) = self.cache.get(user_id) {
            return Ok(profile.clone());
        }

        let resp = self
            .http
            .get(format!("{}/api/user/{}", self.base_url, user_id))
            .header("Authorization", self.token.clone())
            .send()
            .await
            .map_err(|_| HarmonyError::NotConnected)?;

        let public_user: PublicUser = resp
            .json()
            .await
            .map_err(|e| HarmonyError::Http(Box::new(e)))?;

        self.cache
            .insert(public_user.id.clone(), public_user.clone());

        Ok(public_user)
    }
    pub async fn get_user_by_username(&self, username: &str) -> Result<PublicUser> {
        let resp = self
            .http
            .get(format!("{}/api/user/username/{}", self.base_url, username))
            .header("Authorization", self.token.clone())
            .send()
            .await
            .map_err(|_| HarmonyError::NotConnected)?;

        let public_user: PublicUser = resp
            .json()
            .await
            .map_err(|e| HarmonyError::Http(Box::new(e)))?;

        self.cache
            .insert(public_user.id.clone(), public_user.clone());

        Ok(public_user)
    }

    pub async fn get_users(&self, user_ids: Vec<String>) -> Result<Vec<PublicUser>> {
        let mut fetched: Vec<(String, PublicUser)> = Vec::new();
        let mut missing: Vec<String> = Vec::new();

        for id in &user_ids {
            if let Some(profile) = self.cache.get(id) {
                fetched.push((id.clone(), profile.clone()));
            } else {
                missing.push(id.clone());
            }
        }

        for chunk in missing.chunks(50) {
            let resp = self
                .http
                .post(format!("{}/api/user/batch", self.base_url))
                .header("Authorization", self.token.clone())
                .json(chunk)
                .send()
                .await
                .map_err(|_| HarmonyError::NotConnected)?;

            let users: Vec<PublicUser> = resp
                .json()
                .await
                .map_err(|e| HarmonyError::Http(Box::new(e)))?;

            for user in users {
                self.cache.insert(user.id.clone(), user.clone());
                fetched.push((user.id.clone(), user));
            }
        }

        let ordered = user_ids
            .iter()
            .filter_map(|id| {
                fetched
                    .iter()
                    .find(|(k, _)| k == id)
                    .map(|(_, v)| v.clone())
            })
            .collect();

        Ok(ordered)
    }
}
