use std::{collections::HashMap, sync::Arc};

use core_api::Session;
use quick_cache::sync::Cache;

use crate::{Result, encrypted_client::Core, user::User};

pub use core_api::{AvatarUrl, PublicUser};

const CACHE_CAPACITY: usize = 512;
const FETCH_CHUNK_SIZE: usize = 50;

pub struct UserManager {
    cache: Cache<String, User>,
    session: Arc<Session>,
    core: Arc<Core>,
}

impl UserManager {
    pub(crate) fn new(core: Arc<Core>, session: Arc<Session>) -> Self {
        Self {
            cache: Cache::new(CACHE_CAPACITY),
            session,
            core,
        }
    }

    /// Gets a user if cached.
    pub fn get(&self, id: &str) -> Option<User> {
        self.cache.get(id)
    }

    async fn fetch_merged(&self, base: PublicUser) -> Result<User> {
        let profile = self.core.client.get_user(&base.id).await?;
        let user = User::new(base, profile);
        self.cache.insert(user.id().to_string(), user.clone());
        Ok(user)
    }

    pub async fn fetch(&self, user_id: &str) -> Result<User> {
        if let Some(user) = self.cache.get(user_id) {
            return Ok(user);
        }
        let base = self.session.get_user(user_id).await?;
        self.fetch_merged(base).await
    }

    pub async fn fetch_by_username(&self, username: &str) -> Result<User> {
        let base = self.session.get_user_by_username(username).await?;
        if let Some(user) = self.cache.get(&base.id) {
            return Ok(user);
        }
        self.fetch_merged(base).await
    }

    pub async fn fetch_bulk(&self, user_ids: Vec<String>) -> Result<Vec<User>> {
        let mut users: HashMap<String, User> = HashMap::new();
        let mut missing: Vec<String> = Vec::new();

        for id in &user_ids {
            if let Some(user) = self.cache.get(id) {
                users.insert(id.clone(), user);
            } else {
                missing.push(id.clone());
            }
        }

        for chunk in missing.chunks(FETCH_CHUNK_SIZE) {
            let mut profiles: HashMap<String, _> = self
                .core
                .client
                .get_users(chunk)
                .await?
                .into_iter()
                .map(|p| (p.id.clone(), p))
                .collect();

            // users unknown to either server are skipped
            for base in self.session.get_users(chunk).await? {
                let Some(profile) = profiles.remove(&base.id) else {
                    continue;
                };
                let user = User::new(base, profile);
                self.cache.insert(user.id().to_string(), user.clone());
                users.insert(user.id().to_string(), user);
            }
        }

        Ok(user_ids
            .iter()
            .filter_map(|id| users.get(id).cloned())
            .collect())
    }
}
