use async_std::{channel::Receiver, net::UdpSocket};
use pulse_api::NodeEvent;
use ulid::Ulid;

use crate::{environment, errors::Result, rtc::{call::Call, peer::ClientApi}};

use super::client::CallUser;

#[derive(Default, Debug)]
pub struct UserCapabilities {
    pub audio: bool,
    pub video: bool,
    pub screenshare: bool,
}

#[derive(Debug)]
pub struct UserInformation {
    pub id: String,
    pub capabilities: UserCapabilities,
}

pub async fn create_new_user(user: UserInformation, call_id: String, recv: Receiver<NodeEvent>) -> Result<ClientApi> {
    info!("User {} joined {call_id}", user.id);
    let call = Call::get(&call_id);

    // let udp_server = UdpSocket::bind(format!("{}:0", *environment::SOCKET_ADDRESS)).await?;
    //bind to any port in 10000-11000 range
    let udp_server = UdpSocket::bind(format!(
        "{}:{}",
        *environment::SOCKET_ADDRESS,
        10000 + rand::random::<u16>() % 1000
    ))
    .await?;
    let local_addr = udp_server.local_addr()?;

    let session_id = Ulid::new().to_string();

    let client = ClientApi::new(session_id.clone(), udp_server, local_addr, call.clone());
    // let client = CallUser::create(user, call_id).await?;
    // client.run(recv).await
    Ok(client)
}
