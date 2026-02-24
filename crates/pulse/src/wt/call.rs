use std::{
    collections::HashSet,
    sync::{Arc, atomic::Ordering},
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use pulse_api::{AvailableTrack, WtFragmentedTrackData, WtMessageS2C};
use tokio::sync::Mutex;

use crate::wt::{GLOBAL_SESSIONS, TrackInfo};

#[derive(Clone, Debug)]
pub struct PendingMember {
    pub session_id: String,
    pub key_package: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Call {
    pub id: String,
    pub tracks: DashMap<String, TrackInfo>,
    pub consumers: DashMap<String, Arc<ArcSwap<HashSet<String>>>>, // track -> set of session ids
    pub members: DashMap<String, ()>,                              // session ids in this call
    pub mls_state: Arc<Mutex<MlsState>>,
}

#[derive(Clone, Debug)]
pub struct MlsState {
    pub pending_members: Vec<PendingMember>, // members waiting for Add proposals
    pub pending_proposals: Vec<PendingProposal>, // proposals waiting to be flushed
    // if we're currently waiting on a commit, new proposals should be queued here
    // when proposals are flushed, all of them should be included in the next commit
    pub pending_commit: Option<PendingCommit>,
    pub pending_acks: HashSet<String>, // records session IDs that haven't acked current commit
    pub current_epoch: u64, // the current epoch. starts with 0 whenever there is only one member, increments with every commit
    pub pending_epoch_change: bool,
    pub full_members: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PendingCommit {
    pub proposals: Vec<PendingProposal>,
}

#[derive(Clone, Debug)]
pub enum PendingProposal {
    Add {
        session_id: String,
        key_package: Vec<u8>,
    },
    Remove {
        session_id: String,
    },
}
const FRAGMENT_ENVELOPE_OVERHEAD: usize = 128;

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
                    warn!(
                        "Session {} not found while dispatching track {}",
                        session_id, track_id
                    );
                    continue;
                };

                if !session.can_listen.load(Ordering::SeqCst) {
                    continue; // skip deafened users
                }

                let consumer_connection = session.connection.clone();
                let seq_counter = session.seq_counter.clone();
                drop(session); 

                let max_datagram = consumer_connection
                    .max_datagram_size()
                    .expect("Failed to get max datagram size from connection");
                let max_payload = max_datagram.saturating_sub(FRAGMENT_ENVELOPE_OVERHEAD);
                if max_payload == 0 {
                    warn!(
                        "Max datagram size ({}) too small for session {}",
                        max_datagram, session_id
                    );
                    continue;
                }
                let fragment_count = data.len().div_ceil(max_payload);
                if fragment_count > u16::MAX as usize {
                    warn!(
                        "Data too large ({} bytes, {} fragments) for track {}",
                        data.len(), fragment_count, track_id
                    );
                    continue;
                }
                let sequence_id = seq_counter.fetch_add(1, Ordering::Relaxed);

                let mut failed = false;
                for (i, chunk) in data.chunks(max_payload).enumerate() {
                    let fragment = WtFragmentedTrackData {
                        id: track_id.to_string(),
                        sequence_id,
                        fragment_index: i as u16,
                        fragment_count: fragment_count as u16,
                        data: chunk.to_vec(),
                    };
                    let Ok(bytes) = rkyv::to_bytes::<rkyv::rancor::Error>(&fragment) else {
                        warn!("Failed to serialize fragment for track {}", track_id);
                        failed = true;
                        break;
                    };
                    if let Err(e) = consumer_connection.send_datagram(bytes) {
                        warn!(
                            "Failed to forward track {} fragment to session {}: {:?}",
                            track_id, session_id, e
                        );
                        failed = true;
                        break;
                    }
                }

                if !failed {
                    debug!(
                        "Forwarded track {} data ({} fragment(s)) to session {}",
                        track_id, fragment_count, session_id
                    );
                }
            }
        }
    }

    pub async fn add_member(&self, session_id: String, key_package: Vec<u8>) {
        if self.members.contains_key(&session_id) {
            // this is probably a reconnection, so we can just ignore it
            return;
        }
        if !self.members.is_empty() {
            let pending_member = PendingMember {
                session_id: session_id.clone(),
                key_package: key_package.clone(),
            };
            let mut state = self.mls_state.lock().await;
            state.pending_members.push(pending_member);
            state.pending_proposals.push(PendingProposal::Add {
                session_id: session_id.clone(),
                key_package,
            });
            info!(
                "Added pending Add proposal for session {} to call {}",
                session_id, self.id
            );
        } else {
            // if this is the first member, we can just add them without a proposal
            let mut state = self.mls_state.lock().await;
            state.full_members.push(session_id.clone());
            info!(
                "Added first member {} to call {}, initialized MLS group with epoch 0",
                session_id, self.id
            );
        }
        self.members.insert(session_id, ());
    }

    pub async fn remove_member(&self, session_id: &str) {
        self.members.remove(session_id);
        if self.members.is_empty() {
            // the mls group should be cleared
            let mut state = self.mls_state.lock().await;
            state.pending_members.clear();
            state.pending_proposals.clear();
            state.pending_commit = None;
            state.pending_acks.clear();
            state.current_epoch = 0;
            state.pending_epoch_change = false;
            state.full_members.clear();
            return;
        }
        let mut state = self.mls_state.lock().await;
        if state.full_members.contains(&session_id.to_string()) {
            state.pending_proposals.push(PendingProposal::Remove {
                session_id: session_id.to_string(),
            });
        } else {
            // if the removed member was pending, just remove them without a proposal
            state.pending_members.retain(|m| m.session_id != session_id);
            state.pending_proposals.retain(|p| match p {
                PendingProposal::Add { session_id: s, .. } => s != session_id,
                PendingProposal::Remove { session_id: s } => s != session_id,
            });
        }
    }

    pub async fn flush_proposals(&self) -> Option<(Vec<Vec<u8>>, Vec<String>, u64)> {
        let mut state = self.mls_state.lock().await;
        if state.pending_commit.is_some() {
            info!(
                "Already waiting for commit on call {}, cannot flush new proposals yet",
                self.id
            );
            return None;
        }
        let pending_proposals = std::mem::take(&mut state.pending_proposals);
        if pending_proposals.is_empty() {
            return None;
        }
        state.pending_commit = Some(PendingCommit {
            proposals: pending_proposals.clone(),
        });
        let proposals = pending_proposals
            .iter()
            .filter_map(|p| match p {
                PendingProposal::Add { key_package, .. } => {
                    let proposal_result = crate::environment::EXTERNAL_SENDER.create_add_proposal(
                        self.id.as_bytes(),
                        state.current_epoch,
                        key_package,
                    );

                    let proposal_data = match proposal_result {
                        Ok(data) => data,
                        Err(e) => {
                            error!("Failed to create Add proposal: {:?}", e);
                            return None;
                        }
                    };
                    Some(proposal_data)
                }
                PendingProposal::Remove { session_id } => {
                    let idx = state.full_members.iter().position(|s| s == session_id)?;
                    let proposal_result = crate::environment::EXTERNAL_SENDER
                        .create_remove_proposal(
                            self.id.as_bytes(),
                            state.current_epoch,
                            idx as u32,
                        );

                    let proposal_data = match proposal_result {
                        Ok(data) => data,
                        Err(e) => {
                            error!("Failed to create Remove proposal: {:?}", e);
                            return None;
                        }
                    };
                    Some(proposal_data)
                }
            })
            .collect::<Vec<_>>();

        let recipients = state.full_members.clone();
        Some((proposals, recipients, state.current_epoch))
    }

    pub async fn increment_epoch(&self) -> Option<u64> {
        let mut state = self.mls_state.lock().await;
        if state.pending_epoch_change {
            state.current_epoch += 1;
            state.pending_acks.clear();
            state.pending_epoch_change = false;
            Some(state.current_epoch)
        } else {
            None
        }
    }

    /// Record a commit acknowledgement from a member
    /// Returns true if all members have acked (ready to advance epoch)
    pub async fn record_commit_ack(&self, session_id: &str, epoch: u64) -> bool {
        let mut state = self.mls_state.lock().await;
        if !state.pending_epoch_change {
            warn!(
                "Received commit ack from session {} for epoch {}, but no epoch change is pending",
                session_id, epoch
            );
            return false;
        }
        if epoch != state.current_epoch + 1 {
            warn!(
                "Received commit ack for epoch {}, but next epoch is {}",
                epoch,
                state.current_epoch + 1
            );
            return false;
        }
        state.pending_acks.remove(session_id);

        let all_acked = state.pending_acks.is_empty();

        if all_acked {
            info!("All members have acknowledged commit for call {}", self.id);
        } else {
            debug!(
                "Session {} acknowledged commit, {} remaining",
                session_id,
                state.pending_acks.len()
            );
        }

        all_acked
    }
}
