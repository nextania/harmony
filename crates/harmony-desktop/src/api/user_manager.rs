use std::num::NonZeroUsize;
use std::sync::Arc;

use iced::Color;
use lru::LruCache;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{api::UserProfile, errors::RenderableError};

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

impl From<PublicUser> for UserProfile {
    fn from(user: PublicUser) -> Self {
        // FIXME: placeholder
        let hash = Sha256::digest(user.id.as_bytes());
        let r = hash[0] as f32 / 255.0;
        let g = hash[1] as f32 / 255.0;
        let b = hash[2] as f32 / 255.0;
        let r2 = hash[3] as f32 / 255.0;
        let g2 = hash[4] as f32 / 255.0;
        let b2 = hash[5] as f32 / 255.0;
        UserProfile {
            id: user.id,
            display_name: user.display_name,
            username: user.username,
            avatar_color_start: Color::from_rgb(r, g, b),
            avatar_color_end: Color::from_rgb(r2, g2, b2),
        }
    }
}

const CACHE_CAPACITY: usize = 512;

pub struct UserManager {
    cache: Mutex<LruCache<String, UserProfile>>,
    http: Client,
    base_url: String,
    token: String,
}

impl UserManager {
    pub fn new(http: Client, base_url: impl Into<String>, token: impl Into<String>) -> Arc<Self> {
        Arc::new(Self {
            cache: Mutex::new(LruCache::new(NonZeroUsize::new(CACHE_CAPACITY).unwrap())),
            http,
            base_url: base_url.into(),
            token: token.into(),
        })
    }

    pub async fn get_user(&self, user_id: &str) -> Result<UserProfile, RenderableError> {
        {
            let mut cache = self.cache.lock().await;
            if let Some(profile) = cache.get(user_id) {
                return Ok(profile.clone());
            }
        }

        let resp = self
            .http
            .get(format!("{}/api/user/{}", self.base_url, user_id))
            .header("Authorization", self.token.clone())
            .send()
            .await
            .map_err(|_| RenderableError::NetworkError)?;

        let public_user: PublicUser = resp
            .json()
            .await
            .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

        let profile = UserProfile::from(public_user);

        {
            let mut cache = self.cache.lock().await;
            cache.put(profile.id.clone(), profile.clone());
        }

        Ok(profile)
    }
    pub async fn get_user_by_username(&self, username: &str) -> Result<UserProfile, RenderableError> {
        let resp = self
            .http
            .get(format!(
                "{}/api/user/username/{}",
                self.base_url, username
            ))
            .header("Authorization", self.token.clone())
            .send()
            .await
            .map_err(|_| RenderableError::NetworkError)?;

        let public_user: PublicUser = resp
            .json()
            .await
            .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

        let profile = UserProfile::from(public_user);

        {
            let mut cache = self.cache.lock().await;
            cache.put(profile.id.clone(), profile.clone());
        }

        Ok(profile)
    }

    pub async fn get_users(
        &self,
        user_ids: Vec<String>,
    ) -> Result<Vec<UserProfile>, RenderableError> {
        let mut fetched: Vec<(String, UserProfile)> = Vec::new();
        let mut missing: Vec<String> = Vec::new();

        {
            let mut cache = self.cache.lock().await;
            for id in &user_ids {
                if let Some(profile) = cache.get(id) {
                    fetched.push((id.clone(), profile.clone()));
                } else {
                    missing.push(id.clone());
                }
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
                .map_err(|_| RenderableError::NetworkError)?;

            let users: Vec<PublicUser> = resp
                .json()
                .await
                .map_err(|e| RenderableError::UnknownError(e.to_string()))?;

            let mut cache = self.cache.lock().await;
            for user in users {
                let profile = UserProfile::from(user);
                cache.put(profile.id.clone(), profile.clone());
                fetched.push((profile.id.clone(), profile));
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
