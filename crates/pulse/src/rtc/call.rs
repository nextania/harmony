use std::sync::Arc;

use async_std::{
    channel::{self, Receiver, Sender},
    sync::Mutex,
};
use dashmap::DashMap;
use lazy_static::lazy_static;

use crate::socket::events::{MediaType, RemoteTrack};

use super::{
    peer::{Client, Track},
    udp::Propagated,
};

#[derive(Clone, Debug)]
pub enum CallEvent {
    CreateTrack(RemoteTrack),
    RemoveTrack { removed_tracks: Vec<String> },

    // Origin client ID
    Propagate(Propagated),
}

#[derive(Debug)]
pub struct Call {
    id: String,
    pub senders: Arc<Mutex<Vec<Sender<CallEvent>>>>,
    pub tracks: DashMap<String, Arc<Track>>,
    pub clients: DashMap<String, Client>,
}

lazy_static! {
    pub static ref CALLS: DashMap<String, Arc<Call>> = DashMap::new();
}

impl Call {
    fn new(id: String) -> Self {
        Call {
            id,
            senders: Default::default(),
            // user_tracks: Default::default(),
            tracks: Default::default(),
            clients: Default::default(),
        }
    }

    pub fn destroy(&self) {
        // for client in self.clients.iter() {
        //     self.remove_user(&client.id);
        // }
        CALLS.remove(&self.id);
    }

    pub fn get(id: &str) -> Arc<Call> {
        if let Some(call) = CALLS.get(id) {
            call.clone()
        } else {
            let call: Arc<Call> = Arc::new(Call::new(id.to_string()));
            CALLS.insert(id.to_string(), call.clone());

            call
        }
    }

    pub async fn publish(&self, event: CallEvent) {
        // self.sender.clone().try_send(event).ok();
        for sender in self.senders.lock().await.iter() {
            sender.try_send(event.clone()).ok();
        }
    }

    pub async fn listener(&self) -> Receiver<CallEvent> {
        let (sender, receiver) = channel::unbounded();
        self.senders.lock().await.push(sender);
        receiver
    }

    pub async fn get_available_tracks(&self) -> Vec<RemoteTrack> {
        let mut tracks = vec![];

        for item in &self.tracks {
            let id = item.key();
            let track = item.value();

            // TODO: more detailed track info (structure: [{ user: id, tracks: [{ id, type }] }])
            tracks.push(RemoteTrack {
                id: id.to_owned(),
                media_type: MediaType::Video, // FIXME: get media type from track
                user_id: track.producer.to_owned(),
            });
        }

        tracks
    }

    pub fn get_user_ids(&self) -> Vec<String> {
        self.clients.iter().map(|c| c.key().to_owned()).collect()
    }

    pub async fn remove_user(&self, id: &str) {
        let Some((_, mut client)) = self.clients.remove(id) else {
            return;
        };
        // if let Some (client) = client.upgrade() {
        client.close();
        // }

        // TODO: emit event to redis
        
        if self.clients.is_empty() {
            self.destroy();
        }
    }
}
