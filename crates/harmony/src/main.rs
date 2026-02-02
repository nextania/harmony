pub mod authentication;
pub mod errors;
pub mod methods;
pub mod request;
pub mod services;

use authentication::authenticate;
use once_cell::sync::OnceCell;
use rapid::socket::RpcClients;
use rapid::socket::RpcServer;
use services::database;
use services::redis;
use services::voice;

use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::services::environment::LISTEN_ADDRESS;

static RPC_CLIENTS: OnceCell<RpcClients> = OnceCell::new();

#[tokio::main]
async fn main() {
    // TODO: environment, negotiate encryption

    dotenvy::dotenv().ok();
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    database::connect().await;
    info!("Connected to database");

    redis::connect();
    redis::get_connection().await;
    info!("Connected to Redis");

    redis::create_streams()
        .await
        .expect("Failed to initialize Redis streams");
    info!("Initialized Redis streams");

    let listen_address = LISTEN_ADDRESS.to_owned();
    info!("Starting server at {listen_address}");
    let server = RpcServer::new(Box::new(|token| Box::pin(authenticate(token))))
        .register("GET_CHANNEL", methods::channels::get_channel)
        .register("GET_CHANNELS", methods::channels::get_channels)
        .register("CREATE_INVITE", methods::invites::create_invite)
        .register("DELETE_INVITE", methods::invites::delete_invite)
        .register("GET_INVITE", methods::invites::get_invite)
        .register("GET_INVITES", methods::invites::get_invites)
        .register("GET_MESSAGES", methods::messages::get_messages)
        .register("SEND_MESSAGE", methods::messages::send_message)
        .register("CREATE_CALL_TOKEN", methods::voice::create_call_token)
        .register("START_CALL", methods::voice::start_call)
        .register("END_CALL", methods::voice::end_call)
        .register("UPDATE_VOICE_STATE", methods::voice::update_voice_state)
        .register("GET_CALL_MEMBERS", methods::voice::get_call_members);

    RPC_CLIENTS
        .set(server.clients())
        .expect("Failed to set RPC clients");
    voice::spawn_voice_events();

    server.start(listen_address).await;
}
