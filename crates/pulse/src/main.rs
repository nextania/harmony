#[macro_use]
extern crate tracing;

pub mod environment;
pub mod errors;
pub mod metrics;
pub mod mls;
pub mod redis;
pub mod wt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    common::telemetry::init_telemetry("pulse");

    redis::connect();
    redis::get_connection().await;
    info!("Connected to Redis");

    redis::listen();

    wt::listen().await
}
