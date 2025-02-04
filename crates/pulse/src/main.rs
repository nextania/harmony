#[macro_use]
extern crate log;

use std::env;

use pretty_env_logger::formatted_builder;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod environment;
pub mod errors;
pub mod redis;
pub mod rtc;
pub mod socket;

use crate::errors::Result;

#[async_std::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    // let mut builder = formatted_builder();
    // builder.parse_filters("debug");
    // builder.try_init().unwrap();
    if env::var("RUST_LOG").is_err() {
        env::set_var("RUST_LOG", "pulse=debug,str0m=debug");
    }
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    redis::connect().await;
    redis::listen().await;
    Ok(())
}
