//! # Harmony API Client Library
//!
//! A Rust client library for interacting with the Harmony chat server.

pub mod api;
pub mod channel;
pub mod channel_manager;
pub mod client;
pub mod crypto;
pub mod encrypted_client;
pub mod error;
pub mod events;
pub mod keystore;
pub mod models;
pub mod user;
pub mod user_manager;

pub use channel::{Channel, DecryptedMessage};
pub use channel_manager::ChannelManager;
pub use client::{ClientOptions, HarmonyClient};
pub use crypto::{CryptoError, PersistentEncryption};
pub use encrypted_client::{AddContactOutcome, ContactAction, EncryptedClient, EncryptedEvent};
pub use error::{HarmonyError, Result};
pub use events::*;
pub use keystore::{ContactPrivateKey, Keystore};
pub use models::*;
pub use user::User;
pub use user_manager::{AvatarUrl, PublicUser, UserManager};
