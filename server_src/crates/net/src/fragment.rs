use rfs_core::packet::PacketType;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Wire layout of a [`PacketType::Fragment`] payload:
/// `[orig_type: u8][total_chunks: u16 LE][index: u16 LE][total_len: u32 LE][msg_id: u32 LE][chunk bytes...]`
pub const FRAGMENT_HEADER_SIZE: usize = 13;
/// Upper bound for a single reassembled message (sanity check against corrupt headers).
pub const MAX_REASSEMBLED_SIZE: usize = 4 * 1024 * 1024;
/// Upper bound for the chunk count of a single message.
pub const MAX_FRAGMENTS_PER_MESSAGE: u16 = 4096;
/// Incomplete groups older than this are evicted.
pub const FRAGMENT_GROUP_TTL: Duration = Duration::from_secs(2);
/// Max number of incomplete groups held at once.
pub const MAX_FRAGMENT_GROUPS: usize = 32;

struct FragmentGroup {
    total_chunks: u16,
    total_len: usize,
    chunks: Vec<Option<Vec<u8>>>,
    received: usize,
    updated: Instant,
}

/// Reassembles messages split by [`crate::connection::Connection::fragment_large_packet`].
///
/// Keyed by `(original packet type, message id)`. When fragments of a newer message
/// of the same type arrive, older incomplete groups of that type are dropped: on
/// the sequenced channel a newer snapshot supersedes an older one, so keeping the
/// stale group would only waste memory.
#[derive(Default)]
pub struct FragmentAssembler {
    groups: HashMap<(u8, u32), FragmentGroup>,
}

impl FragmentAssembler {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
        }
    }

    /// Feed one `Fragment` payload. Returns `(original packet type, full message bytes)`
    /// once the last missing chunk arrives, `None` otherwise (including corrupt input).
    pub fn push(&mut self, payload: &[u8]) -> Option<(PacketType, Vec<u8>)> {
        self.evict_stale();

        if payload.len() < FRAGMENT_HEADER_SIZE {
            return None;
        }
        let orig_type = payload[0];
        let total_chunks = u16::from_le_bytes([payload[1], payload[2]]);
        let index = u16::from_le_bytes([payload[3], payload[4]]) as usize;
        let total_len = u32::from_le_bytes([payload[5], payload[6], payload[7], payload[8]]) as usize;
        let msg_id = u32::from_le_bytes([payload[9], payload[10], payload[11], payload[12]]);

        if total_chunks == 0
            || total_chunks > MAX_FRAGMENTS_PER_MESSAGE
            || index >= total_chunks as usize
            || total_len == 0
            || total_len > MAX_REASSEMBLED_SIZE
        {
            return None;
        }
        let packet_type = PacketType::from_u8(orig_type)?;

        // A newer message of the same type supersedes older incomplete ones.
        let stale: Vec<(u8, u32)> = self
            .groups
            .keys()
            .filter(|(t, m)| *t == orig_type && *m != msg_id)
            .copied()
            .collect();
        for key in stale {
            self.groups.remove(&key);
        }

        let chunk = &payload[FRAGMENT_HEADER_SIZE..];
        let key = (orig_type, msg_id);
        let group = self.groups.entry(key).or_insert_with(|| FragmentGroup {
            total_chunks,
            total_len,
            chunks: vec![None; total_chunks as usize],
            received: 0,
            updated: Instant::now(),
        });

        // Ignore chunks that disagree with the group already in progress.
        if group.total_chunks != total_chunks || group.total_len != total_len {
            return None;
        }
        if group.chunks[index].is_none() {
            group.chunks[index] = Some(chunk.to_vec());
            group.received += 1;
        }
        group.updated = Instant::now();

        if group.received == group.total_chunks as usize {
            let group = self.groups.remove(&key)?;
            let mut message = Vec::with_capacity(group.total_len);
            for slot in &group.chunks {
                message.extend_from_slice(slot.as_ref()?);
            }
            if message.len() != group.total_len {
                return None;
            }
            return Some((packet_type, message));
        }
        None
    }

    fn evict_stale(&mut self) {
        if self.groups.len() > MAX_FRAGMENT_GROUPS {
            // Drop the oldest groups first.
            let mut keys: Vec<((u8, u32), Instant)> = self
                .groups
                .iter()
                .map(|(k, g)| (*k, g.updated))
                .collect();
            keys.sort_by_key(|(_, t)| *t);
            for (key, _) in keys.into_iter().take(self.groups.len() - MAX_FRAGMENT_GROUPS) {
                self.groups.remove(&key);
            }
        }
        let now = Instant::now();
        self.groups
            .retain(|_, g| now.duration_since(g.updated) < FRAGMENT_GROUP_TTL);
    }
}

/// Split a serialized message into `Fragment` payloads with a 13-byte header each.
/// `msg_id` must be unique per message (per sender); receivers use it to tell
/// interleaved fragmented messages apart.
pub fn make_fragments(
    orig_type: PacketType,
    data: &[u8],
    msg_id: u32,
    max_payload: usize,
) -> Vec<Vec<u8>> {
    let chunk_size = max_payload.saturating_sub(FRAGMENT_HEADER_SIZE).max(1);
    let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
    let total_chunks = chunks.len() as u16;
    let mut out = Vec::with_capacity(chunks.len());
    for (i, chunk) in chunks.iter().enumerate() {
        let mut payload = Vec::with_capacity(chunk.len() + FRAGMENT_HEADER_SIZE);
        payload.push(orig_type as u8);
        payload.extend_from_slice(&total_chunks.to_le_bytes());
        payload.extend_from_slice(&(i as u16).to_le_bytes());
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(&msg_id.to_le_bytes());
        payload.extend_from_slice(chunk);
        out.push(payload);
    }
    out
}
