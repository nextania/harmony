#[macro_use]
extern crate log;

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub mod environment;
pub mod errors;
pub mod redis;
pub mod wt;

use crate::errors::Result;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    redis::connect().await;
    info!("Connected to Redis");
    
    wt::listen().await;
    Ok(())
}
