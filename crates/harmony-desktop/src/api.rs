use std::sync::Arc;

use harmony_api::{EncryptedClient, User};
use iced::Color;

#[derive(Debug, Clone)]
pub struct UserProfile {
    pub id: String,
    pub display_name: String,
    pub username: String,
    // FIXME: placeholder
    pub avatar_color_start: Color,
    pub avatar_color_end: Color,
}

impl From<&User> for UserProfile {
    fn from(user: &User) -> Self {
        let (start, end) = avatar_colors(user.id());
        UserProfile {
            id: user.id().to_string(),
            display_name: user.display_name().to_string(),
            username: user.username().to_string(),
            avatar_color_start: start,
            avatar_color_end: end,
        }
    }
}

// FIXME: placeholder
fn avatar_colors(user_id: &str) -> (Color, Color) {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(user_id.as_bytes());
    let c = |i: usize| {
        Color::from_rgb(
            hash[i] as f32 / 255.0,
            hash[i + 1] as f32 / 255.0,
            hash[i + 2] as f32 / 255.0,
        )
    };
    (c(0), c(3))
}

pub fn placeholder_profile(user_id: &str) -> UserProfile {
    let (start, end) = avatar_colors(user_id);
    UserProfile {
        id: user_id.to_string(),
        display_name: "Unknown user".to_string(),
        username: "?".to_string(),
        avatar_color_start: start,
        avatar_color_end: end,
    }
}

// TODO: move more to harmony-api
pub async fn call_identity(client: &EncryptedClient) -> pulse_api::MlsIdentity {
    let seed = client.identity_seed().await;
    let trusted = client.identity_key_snapshot().await;
    pulse_api::MlsIdentity {
        user_id: client.user_id().to_string(),
        signing_seed: *seed,
        trusted_keys: Arc::new(move |user_id: &str| trusted.get(user_id).copied()),
    }
}
