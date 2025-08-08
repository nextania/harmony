//! # Harmony API Client Library
//!
//! A Rust client library for interacting with the Harmony chat server.

pub mod api;
pub mod client;
pub mod error;
pub mod events;
pub mod models;

pub use client::{ClientOptions, HarmonyClient};
pub use error::{HarmonyError, Result};
pub use events::*;
pub use models::*;
