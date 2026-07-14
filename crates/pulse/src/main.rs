#[macro_use]
extern crate tracing;

pub mod environment;
pub mod errors;
pub mod metrics;
pub mod mls;
pub mod nats;
pub mod redis;
pub mod wt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    common::telemetry::init_telemetry("pulse");

    redis::connect();
    redis::get_connection().await;
    info!("Connected to Redis");

    nats::connect().await;
    info!("Connected to NATS and created streams");

    nats::listen();

    wt::listen().await
}
