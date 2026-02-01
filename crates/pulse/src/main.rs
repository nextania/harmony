#[macro_use]
extern crate tracing;

use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub mod environment;
pub mod errors;
pub mod redis;
pub mod wt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    redis::connect();
    redis::get_connection().await;
    info!("Connected to Redis");
    
    wt::listen().await
}
