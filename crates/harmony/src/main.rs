pub mod authentication;
pub mod errors;
pub mod methods;
pub mod services;

use std::sync::OnceLock;

use authentication::authenticate;
use rapid::socket::RpcClients;
use rapid::socket::RpcServer;
use services::database;
use services::rate_limiter::RedisRateLimiter;
use services::redis;
use services::voice;

use tracing::info;

use crate::services::environment::LISTEN_ADDRESS;

static RPC_CLIENTS: OnceLock<RpcClients> = OnceLock::new();

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    common::telemetry::init_telemetry("harmony");

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
        .rate_limiter(RedisRateLimiter)
        // Channels
        .register("GET_CHANNEL", methods::channels::get_channel)
        .register("GET_CHANNELS", methods::channels::get_channels)
        .register("CREATE_CHANNEL", methods::channels::create_channel)
        .register("EDIT_CHANNEL", methods::channels::edit_channel)
        .register("DELETE_CHANNEL", methods::channels::delete_channel)
        .register("LEAVE_CHANNEL", methods::channels::leave_channel)
        // Invites
        .register("CREATE_INVITE", methods::invites::create_invite)
        .register("DELETE_INVITE", methods::invites::delete_invite)
        .register("GET_INVITE", methods::invites::get_invite)
        .register("GET_INVITES", methods::invites::get_invites)
        .register("ACCEPT_INVITE", methods::invites::accept_invite)
        // Messages
        .register("GET_MESSAGES", methods::messages::get_messages)
        .register("SEND_MESSAGE", methods::messages::send_message)
        .register("EDIT_MESSAGE", methods::messages::edit_message)
        .register("DELETE_MESSAGE", methods::messages::delete_message)
        // Users
        .register("GET_CURRENT_USER", methods::users::get_current_user)
        .register("ADD_CONTACT", methods::users::add_contact)
        .register("REMOVE_CONTACT", methods::users::remove_contact)
        .register("GET_CONTACTS", methods::users::get_contacts)
        // Keys
        .register("SET_KEY_PACKAGE", methods::keys::set_key_package)
        .register("GET_USER", methods::keys::get_user)
        // Voice
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
