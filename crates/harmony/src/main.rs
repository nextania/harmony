#![allow(dead_code)]

pub mod authentication;
pub mod errors;
pub mod methods;
pub mod request;
pub mod services;

use authentication::authenticate;
use rapid::socket::RpcServer;
use services::database;
use services::redis;
// use services::webrtc;

use log::info;
use services::voice;

use crate::services::environment::LISTEN_ADDRESS;

#[tokio::main]
async fn main() {
    // TODO: environment, negotiate encryption

    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    database::connect().await;
    info!("Connected to database");

    // run DB migrations as necessary

    redis::connect().await;
    info!("Connected to Redis");
    voice::spawn_check_available_nodes();

    let listen_address = LISTEN_ADDRESS.to_owned();
    info!("Starting server at {listen_address}");
    RpcServer::new(Box::new(|token| Box::pin(authenticate(token))))
        .register("GET_CHANNEL", methods::channels::get_channel)
        .register("GET_CHANNELS", methods::channels::get_channels)
        .register("CREATE_INVITE", methods::invites::create_invite)
        .register("DELETE_INVITE", methods::invites::delete_invite)
        .register("GET_INVITE", methods::invites::get_invite)
        .register("GET_INVITES", methods::invites::get_invites)
        .register("GET_MESSAGES", methods::messages::get_messages)
        .register("SEND_MESSAGE", methods::messages::send_message)
        .register("JOIN_CALL", methods::voice::join_call)
        .register("LEAVE_CALL", methods::voice::leave_call)
        .register("START_CALL", methods::voice::start_call)
        .register("END_CALL", methods::voice::end_call)
        .start(listen_address)
        .await;
}
