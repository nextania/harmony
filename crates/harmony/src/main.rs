#![allow(dead_code)]

pub mod errors;
pub mod methods;
pub mod services;
pub mod authentication;
pub mod request;

use authentication::authenticate;
use rapid::socket::RpcServer;
use services::database;
use services::redis;
// use services::webrtc;

use log::info;
use services::webrtc;

use crate::services::environment::LISTEN_ADDRESS;

#[async_std::main]
async fn main() {
    // TODO: environment, negotiate encryption

    dotenvy::dotenv().ok();
    env_logger::init_from_env(env_logger::Env::default().default_filter_or("info"));

    database::connect().await;
    info!("Connected to database");

    // run DB migrations as necessary

    redis::connect().await;
    info!("Connected to Redis");
    webrtc::spawn_check_available_nodes();

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
        .register("CREATE_ROLE", methods::roles::create_role)
        .register("EDIT_ROLE", methods::roles::edit_role)
        .register("DELETE_ROLE", methods::roles::delete_role)
        .register("GET_SPACE", methods::spaces::get_space)
        .register("CREATE_SPACE", methods::spaces::create_space)
        .register("EDIT_SPACE", methods::spaces::edit_space)
        .register("DELETE_SPACE", methods::spaces::delete_space)
        .register("JOIN_CALL", methods::webrtc::join_call)
        .register("LEAVE_CALL", methods::webrtc::leave_call)
        .register("START_CALL", methods::webrtc::start_call)
        .register("END_CALL", methods::webrtc::end_call)
        .start(listen_address).await;
}
