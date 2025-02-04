use std::sync::Arc;

use crate::{
    environment, errors::Result, rtc::peer::ClientApi
};
use async_std::{
    channel::Receiver, net::UdpSocket
};
use pulse_api::NodeEvent;
use ulid::Ulid;

use crate::rtc::call::Call;

use super::
    server::UserInformation
;

#[derive(Debug)]
pub struct CallUser {
    user: UserInformation,
    call: Arc<Call>,
    pub session_id: String, // TODO: set this value
    client: ClientApi,
}

impl CallUser {
    pub async fn create(user: UserInformation, call_id: String) -> Result<Self> {
        let call = Call::get(&call_id);
        info!("Created a new client for {user:?} in call {call_id}.");

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

        Ok(Self {
            user,
            call,
            session_id,
            client,
        })
    }

    pub async fn run(mut self, stream: Receiver<NodeEvent>) -> Result<()> {
        // debug!("Announcing current state to client");
        // write
        //     .send(ServerEvent::Accept {
        //         available_tracks: self.call.get_available_tracks().await,
        //         user_ids: self.call.get_user_ids(),
        //     })
        //     .await?;

        debug!("Listening for events");
        info!("User {} disconnected", self.user.id);
        self.call.remove_user(&self.user.id).await;
        Ok(())
    }
}
