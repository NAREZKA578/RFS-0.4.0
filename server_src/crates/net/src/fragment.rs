use rfs_core::packet::PacketType;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::warn;

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
/// Hard ceiling on the total chunk payload buffered by ONE assembler
/// (bug №162): 32 groups x 4096 chunks x ~1.4 KB could otherwise reach
/// ~180 MB per connection. The empty slot vecs are cheap; only bytes that
/// actually arrive count against this budget.
pub const MAX_TOTAL_BUFFERED_BYTES: usize = 8 * 1024 * 1024;

struct FragmentGroup {
    total_chunks: u16,
    total_len: usize,
    chunks: Vec<Option<Vec<u8>>>,
    received: usize,
    buffered: usize,
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
    /// Highest msg_id seen per original type (drives supersede decisions).
    latest_seen: HashMap<u8, u32>,
    /// Highest msg_id that fully assembled per original type.
    latest_completed: HashMap<u8, u32>,
    /// Chunk payload bytes currently held across all groups (bug №162).
    buffered_bytes: usize,
}

/// Wrap-aware "is `a` newer than `b`?" for u32 message ids (bug №165): the
/// server wraps its msg_id at u32::MAX, but plain `>`/`<` would treat a fresh
/// post-wrap id as "older than MAX" and reject every new message — bricking
/// reassembly until latest_seen/latest_completed are reset.
fn frag_newer(a: u32, b: u32) -> bool {
    a != b && a.wrapping_sub(b) < (1 << 31)
}

impl FragmentAssembler {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            latest_seen: HashMap::new(),
            latest_completed: HashMap::new(),
            buffered_bytes: 0,
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
        let _packet_type = PacketType::from_u8(orig_type)?;

        // Bug №17 / №165: a fragment of an *older* message must never evict a
        // fresher in-progress group. Only a strictly newer msg_id supersedes.
        // Comparison is wrap-aware, so the u32 wrap does not freeze the channel.
        if let Some(&completed) = self.latest_completed.get(&orig_type) {
            if !frag_newer(msg_id, completed) {
                // Retransmission / stale fragment of an already-delivered message.
                return None;
            }
        }
        match self.latest_seen.get(&orig_type) {
            Some(&seen) if frag_newer(seen, msg_id) => {
                // Stale fragment of a message superseded by a newer one.
                return None;
            }
            Some(&seen) if frag_newer(msg_id, seen) => {
                // Anti-poison: a single far-future msg_id (attacker-controlled)
                // must not brick reassembly by pinning latest_seen to MAX.
                // Only small forward jumps supersede; huge jumps are dropped
                // without advancing latest_seen.
                const MAX_FORWARD_JUMP: u32 = 1024;
                if msg_id.wrapping_sub(seen) > MAX_FORWARD_JUMP {
                    return None;
                }
                self.latest_seen.insert(orig_type, msg_id);
                let stale: Vec<(u8, u32)> = self
                    .groups
                    .keys()
                    .filter(|(t, m)| *t == orig_type && *m != msg_id)
                    .copied()
                    .collect();
                for key in stale {
                    self.drop_group(key);
                }
            }
            _ => {
                self.latest_seen.insert(orig_type, msg_id);
            }
        }

        // Bug №162: enforce the byte budget BEFORE storing. If an existing
        // group cannot make room (all fresh), the chunk is refused outright.
        let needed = payload.len().saturating_sub(FRAGMENT_HEADER_SIZE);
        if needed > self.remaining_budget() {
            self.evict_for(needed);
            if needed > self.remaining_budget() {
                return None;
            }
        }

        let key = (orig_type, msg_id);
        let group = self.groups.entry(key).or_insert_with(|| FragmentGroup {
            total_chunks,
            total_len,
            chunks: vec![None; total_chunks as usize],
            received: 0,
            buffered: 0,
            updated: Instant::now(),
        });

        // Ignore chunks that disagree with the group already in progress.
        if group.total_chunks != total_chunks || group.total_len != total_len {
            return None;
        }
        if group.chunks[index].is_none() {
            let chunk = &payload[FRAGMENT_HEADER_SIZE..];
            group.chunks[index] = Some(chunk.to_vec());
            group.received += 1;
            group.buffered += chunk.len();
            self.buffered_bytes += chunk.len();
            // Only a *new* chunk refreshes the TTL. A duplicate chunk must not
            // keep an attacker-fed group alive past FRAGMENT_GROUP_TTL (bug №162).
            group.updated = Instant::now();

            if group.received == group.total_chunks as usize {
                return self.assemble(key);
            }
        }
        None
    }

    fn remaining_budget(&self) -> usize {
        MAX_TOTAL_BUFFERED_BYTES.saturating_sub(self.buffered_bytes)
    }

    /// Build the message once all chunks of a group are present.
    fn assemble(&mut self, key: (u8, u32)) -> Option<(PacketType, Vec<u8>)> {
        let orig_type = key.0;
        let msg_id = key.1;
        let mut group = self.groups.remove(&key)?;
        self.buffered_bytes = self.buffered_bytes.saturating_sub(group.buffered);

        let mut message = Vec::with_capacity(group.total_len);
        for slot in &group.chunks {
            match slot {
                Some(bytes) => message.extend_from_slice(bytes),
                // Unreachable when received == total_chunks, kept as a total.
                None => {
                    warn!("fragment group {key:?} completed with a missing slot, dropping");
                    return None;
                }
            }
        }

        // Bug №166: a lying/corrupt total_len used to delete the whole group
        // AND the arriving chunks — a corrected retransmit had nothing to join
        // and the message was lost forever. Keep the group (reset) so a
        // retransmission with the right total_len can rebuild it.
        if message.len() != group.total_len {
            warn!(
                "fragment reassembly length mismatch ({} != {}), resetting group {key:?}",
                message.len(),
                group.total_len
            );
            group.chunks = vec![None; group.total_chunks as usize];
            group.received = 0;
            group.buffered = 0;
            group.updated = Instant::now();
            self.groups.insert(key, group);
            return None;
        }

        let last = self.latest_completed.entry(orig_type).or_insert(0);
        if frag_newer(msg_id, *last) {
            *last = msg_id;
        }
        Some((PacketType::from_u8(orig_type).unwrap(), message))
    }

    fn evict_stale(&mut self) {
        // Cap the number of in-flight groups: drop the oldest first.
        if self.groups.len() > MAX_FRAGMENT_GROUPS {
            let mut keys: Vec<((u8, u32), Instant)> = self
                .groups
                .iter()
                .map(|(k, g)| (*k, g.updated))
                .collect();
            keys.sort_by_key(|(_, t)| *t);
            for (key, _) in keys.into_iter().take(self.groups.len() - MAX_FRAGMENT_GROUPS) {
                self.drop_group(key);
            }
        }
        // Expire groups whose TTL ran out (only new chunks refresh it, so a
        // naive dup-chunk spammer cannot pin a group forever — bug №162).
        let now = Instant::now();
        let expired: Vec<(u8, u32)> = self
            .groups
            .iter()
            .filter(|(_, g)| now.duration_since(g.updated) >= FRAGMENT_GROUP_TTL)
            .map(|(k, _)| *k)
            .collect();
        for key in expired {
            self.drop_group(key);
        }
    }

    /// Evict oldest groups until `needed` bytes would fit in the budget.
    fn evict_for(&mut self, needed: usize) {
        while self.remaining_budget() < needed && !self.groups.is_empty() {
            let oldest = self
                .groups
                .iter()
                .min_by_key(|(_, g)| g.updated)
                .map(|(k, _)| *k);
            match oldest {
                Some(key) => self.drop_group(key),
                None => break,
            }
        }
    }

    fn drop_group(&mut self, key: (u8, u32)) {
        if let Some(group) = self.groups.remove(&key) {
            self.buffered_bytes = self.buffered_bytes.saturating_sub(group.buffered);
        }
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
    // Bug №17: `chunks.len() as u16` / `data.len() as u32` below used to
    // wrap silently on absurd inputs. Refuse loudly instead — the receiver
    // would reject such a message anyway (MAX_* guards in push).
    if data.is_empty() {
        warn!("make_fragments: empty message, dropping");
        return Vec::new();
    }
    if data.len() > MAX_REASSEMBLED_SIZE {
        warn!("make_fragments: message too large ({}), dropping", data.len());
        return Vec::new();
    }
    let chunk_size = max_payload.saturating_sub(FRAGMENT_HEADER_SIZE).max(1);
    let chunks: Vec<&[u8]> = data.chunks(chunk_size).collect();
    if chunks.len() > MAX_FRAGMENTS_PER_MESSAGE as usize {
        warn!("make_fragments: too many chunks ({}), dropping", chunks.len());
        return Vec::new();
    }
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