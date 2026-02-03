use std::{collections::HashSet, sync::Arc};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use pulse_api::{AvailableTrack, WtMessageS2C, WtTrackData};

use crate::wt::{GLOBAL_SESSIONS, TrackInfo};

#[derive(Clone, Debug)]
pub struct Call {
    pub id: String,
    pub tracks: DashMap<String, TrackInfo>,
    pub consumers: DashMap<String, Arc<ArcSwap<HashSet<String>>>>, // track -> set of session ids
    pub members: DashMap<String, ()>,                              // session ids in this call
}

impl Call {
    pub fn start_consuming(&self, session_id: &str, track_id: &str) {
        let track_info = self.tracks.get(track_id).map(|t| t.value().clone());
        let Some(track_info) = track_info else {
            warn!("Track {} does not exist", track_id);
            return;
        };
        if track_info.session_id == session_id {
            warn!("Cannot consume own track");
            return;
        }
        let consumer_set = self
            .consumers
            .entry(track_id.to_string())
            .or_insert_with(|| Arc::new(ArcSwap::from_pointee(HashSet::new())));
        let mut set = consumer_set.value().load_full().as_ref().clone();
        if set.insert(session_id.to_string()) {
            consumer_set.value().store(Arc::new(set));
        }
    }

    pub fn stop_consuming_all(&self, session_id: &str) {
        for consumer_set in self.consumers.iter() {
            let mut set = consumer_set.value().load_full().as_ref().clone();
            if set.remove(session_id) {
                consumer_set.value().store(Arc::new(set));
            }
        }
    }

    pub fn stop_consuming(&self, session_id: &str, track_id: &str) {
        self.consumers
            .entry(track_id.to_string())
            .and_modify(|consumers| {
                let mut set = consumers.load_full().as_ref().clone();
                if set.remove(session_id) {
                    consumers.store(Arc::new(set));
                }
            });
    }

    pub async fn start_producing(&self, session_id: &str, track_info: TrackInfo) {
        let track_id = track_info.id.clone();
        let media_hint = track_info.media_hint.clone();
        let info_session_id = track_info.session_id.clone();

        self.tracks.insert(track_info.id.clone(), track_info);

        for member in self.members.iter() {
            if member.key() == session_id {
                continue;
            }
            let session = GLOBAL_SESSIONS.get(member.key());
            let Some(session) = session else {
                continue;
            };
            let available_track = AvailableTrack {
                id: track_id.clone(),
                media_hint: media_hint.clone(),
                session_id: info_session_id.clone(),
            };
            let _ = session.message_tx.send(WtMessageS2C::TrackAvailable {
                track: available_track,
            });
        }
    }

    pub fn stop_producing(&self, session_id: &str, track_id: &str) {
        self.tracks.remove(track_id);
        self.consumers.remove(track_id);

        for member in self.members.iter() {
            if member.key() == session_id {
                continue;
            }
            let session = GLOBAL_SESSIONS.get(member.key());
            let Some(session) = session else {
                continue;
            };
            let _ = session.message_tx.send(WtMessageS2C::TrackUnavailable {
                id: track_id.to_string(),
            });
        }
    }

    pub fn get_mapped_track_id(&self, track_id: &str, session_id: &str) -> Option<String> {
        if let Some(track_info) = self
            .tracks
            .iter()
            .find(|t| t.client_track_id == track_id && t.session_id == session_id)
        {
            return Some(track_info.id.clone());
        }
        None
    }

    pub fn get_available_tracks(&self, excluding_session_id: &str) -> Vec<AvailableTrack> {
        self.tracks
            .iter()
            .filter(|t| t.session_id != excluding_session_id)
            .map(|t| AvailableTrack {
                id: t.id.clone(),
                media_hint: t.media_hint.clone(),
                session_id: t.session_id.clone(),
            })
            .collect()
    }

    pub async fn dispatch(&self, track_id: &str, data: &[u8]) {
        if let Some(consumer_set) = self.consumers.get(track_id) {
            let sessions = consumer_set.value().load_full();
            for session_id in sessions.iter() {
                let Some(session) = GLOBAL_SESSIONS.get(session_id) else {
                    // shouldn't happen
                    warn!(
                        "Session {} not found while dispatching track {}",
                        session_id, track_id
                    );
                    continue;
                };

                if !session.session_data.read().await.can_listen {
                    continue; // skip deafened users
                }

                let consumer_connection = session.connection.clone();
                drop(session); // Release lock before sending

                let Ok(payload) = rkyv::to_bytes::<rkyv::rancor::Error>(&WtTrackData {
                    id: track_id.to_string(),
                    data: data.to_vec(),
                }) else {
                    warn!("Failed to serialize track data for track {}", track_id);
                    continue;
                };

                if let Err(e) = consumer_connection.send_datagram(payload) {
                    warn!(
                        "Failed to forward track {} data to session {}: {:?}",
                        track_id, session_id, e
                    );
                } else {
                    debug!(
                        "Forwarded track {} data to session {}",
                        track_id, session_id
                    );
                }
            }
        }
    }

    pub fn add_member(&self, session_id: String) {
        self.members.insert(session_id, ());
    }

    pub fn remove_member(&self, session_id: &str) {
        self.members.remove(session_id);
    }
}
