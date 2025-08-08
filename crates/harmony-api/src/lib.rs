//! # Harmony API Client Library
//!
//! A Rust client library for interacting with the Harmony chat server.
//! This library provides a high-level interface for authentication, messaging,
//! channel management, invites, and WebRTC calls.
//!
//! ## Example
//!
//! ```rust,no_run
//! use harmony_api::{HarmonyClient, ClientConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = ClientConfig::new("ws://localhost:8080", "your-jwt-token");
//!     let mut client = HarmonyClient::new(config).await?;
//!     
//!     // Get channels
//!     let channels = client.get_channels().await?;
//!     println!("Found {} channels", channels.len());
//!     
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod models;
pub mod error;
pub mod events;

pub use client::{HarmonyClient, ClientConfig};
pub use error::{HarmonyError, Result};
pub use models::*;
pub use events::*;