use std::{
    cmp::Reverse,
    collections::{BinaryHeap, HashMap},
    time::{Duration, Instant},
};

use crate::WtFragmentedTrackData;

type FragmentKey = (String, u32);

struct FragmentBuffer {
    fragments: Vec<Option<Vec<u8>>>,
    received: u16,
    total: u16,
    created_at: Instant,
}

pub struct FragmentAssembler {
    map: HashMap<FragmentKey, FragmentBuffer>,
    exp: BinaryHeap<Reverse<(Instant, FragmentKey)>>,
    ttl: Duration,
}

pub struct ReassembledDatagram {
    pub id: String,
    pub data: Vec<u8>,
}

impl FragmentAssembler {
    pub fn new(ttl: Duration) -> Self {
        Self {
            map: HashMap::new(),
            exp: BinaryHeap::new(),
            ttl,
        }
    }

    pub fn insert(&mut self, fragment: WtFragmentedTrackData) -> Option<ReassembledDatagram> {
        self.evict_stale();
        // only one fragment
        if fragment.fragment_count == 1 {
            return Some(ReassembledDatagram {
                id: fragment.id,
                data: fragment.data,
            });
        }

        let key: FragmentKey = (fragment.id.clone(), fragment.sequence_id);

        let buf = self
            .map
            .entry(key.clone())
            .or_insert_with(|| FragmentBuffer {
                fragments: vec![None; fragment.fragment_count as usize],
                received: 0,
                total: fragment.fragment_count,
                created_at: Instant::now(),
            });
        self.exp
            .push(Reverse((buf.created_at + self.ttl, key.clone())));

        if fragment.fragment_index >= buf.total {
            return None;
        }

        let idx = fragment.fragment_index as usize;
        if buf.fragments[idx].is_none() {
            buf.fragments[idx] = Some(fragment.data);
            buf.received += 1;
        }

        if buf.received == buf.total {
            // all fragments received
            if let Some(buf) = self.map.remove(&key) {
                self.exp.retain(|e| e.0.1 != key);
                let total_len: usize = buf
                    .fragments
                    .iter()
                    .map(|f| f.as_ref().map_or(0, |v| v.len()))
                    .sum();
                let mut assembled = Vec::with_capacity(total_len);
                for frag in buf.fragments.iter().flatten() {
                    assembled.extend_from_slice(frag);
                }
                return Some(ReassembledDatagram {
                    id: key.0,
                    data: assembled,
                });
            }
        }

        None
    }

    pub fn evict_stale(&mut self) {
        let now = Instant::now();
        loop {
            let remove = if let Some(entry) = self.exp.peek_mut()
                && entry.0.0 > now
            {
                true
            } else {
                false
            };
            if remove {
                let entry = self.exp.pop().unwrap();
                self.map.remove(&entry.0.1);
            } else {
                break;
            }
        }
    }
}

impl Default for FragmentAssembler {
    fn default() -> Self {
        Self::new(Duration::from_secs(1))
    }
}
