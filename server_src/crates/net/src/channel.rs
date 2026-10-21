//! Bug №215: the Channel layer is the send-side ledger for RELIABLE traffic.
//! Every reliable datagram a `Connection` produces is registered here with its
//! payload, cleared when a peer's ack header covers it (`handle_ack`), and
//! re-collected on a timer (`collect_retransmits`) until either the peer
//! confirms delivery or the retry budget runs out.
//!
//! The receive side deliberately lives in `Connection` (`reliable_hcr` /
//! `reliable_gaps`, bug №167): keeping receive bookkeeping in two places is
//! what left this layer silently unwired (bug №215), so the old receive-buffer
//! duplication (bugs №8/№16) is gone with it.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use parking_lot::Mutex;

pub use rfs_core::packet::ChannelType;

use super::connection::seq_is_newer;

/// One reliable datagram we may have to send again.
#[derive(Debug, Clone)]
struct LedgerEntry {
    sequence: u32,
    packet_type: u8,
    data: Vec<u8>,
    /// Last time this entry was sent (or re-collected) onto the wire.
    sent_at: Instant,
    /// How many retransmits this entry has already consumed.
    retry_count: u32,
}

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub channel_type: ChannelType,
    pub send_buffer_size: usize,
    pub max_fragments: usize,
}

impl ChannelConfig {
    pub fn new(channel_type: ChannelType, send_buffer: usize) -> Self {
        Self {
            channel_type,
            send_buffer_size: send_buffer,
            max_fragments: 16,
        }
    }
}

pub struct Channel {
    config: ChannelConfig,
    ledger: Mutex<VecDeque<LedgerEntry>>,
    stats: Mutex<ChannelStats>,
}

/// Send-side counters only; the receive side is `Connection`'s job (№167).
/// Every field is written here and exists so diagnostics stay measurable
/// rather than assumed (registry rule: «зелёная сборка не означает, что кадр
/// рисуется» — ни один из них не выставляется молча).
#[derive(Debug, Default, Clone)]
pub struct ChannelStats {
    /// Distinct reliable datagrams accepted into the ledger. Retransmits do
    /// not re-increment — they refresh the existing entry.
    pub packets_sent: u64,
    /// Retransmits actually handed back by `collect_retransmits`.
    pub packets_retransmitted: u64,
    /// Entries whose retry budget ran out; dropped as never delivered.
    pub packets_lost: u64,
    /// Accepted payload bytes (headers excluded).
    pub bytes_sent: u64,
    /// Entries refused because the ledger hit `send_buffer_size`.
    pub ledger_overflow: u64,
}

impl Channel {
    pub fn new(config: ChannelConfig) -> Self {
        Self {
            config,
            ledger: Mutex::new(VecDeque::new()),
            stats: Mutex::new(ChannelStats::default()),
        }
    }

    pub fn config(&self) -> &ChannelConfig {
        &self.config
    }

    /// Bug №215: register a reliable datagram. The wire helper serializes
    /// first, so what is mirrored is exactly what went out. A second
    /// registration of the same sequence is a retransmit pass: it refreshes
    /// the send stamp and nothing else — the retry counter belongs to
    /// `collect_retransmits` alone.
    pub fn register_sent(&self, sequence: u32, packet_type: u8, data: &[u8]) {
        {
            let mut ledger = self.ledger.lock();
            if let Some(entry) = ledger.iter_mut().find(|e| e.sequence == sequence) {
                entry.sent_at = Instant::now();
                return;
            }

            let cap = self.config.send_buffer_size.max(1);
            while ledger.len() >= cap {
                // Oldest outstanding first: ordering mirrors the send order,
                // and an evicted guarantee is only ever regained by the
                // connection timeout, never by a lie about freshness.
                ledger.pop_front();
                self.stats.lock().ledger_overflow += 1;
            }

            ledger.push_back(LedgerEntry {
                sequence,
                packet_type,
                data: data.to_vec(),
                sent_at: Instant::now(),
                retry_count: 0,
            });
        }

        let mut stats = self.stats.lock();
        stats.packets_sent += 1;
        stats.bytes_sent += data.len() as u64;
    }

    /// Bug №215 (carries the №15 fix): the peer's ack header has arrived.
    /// Cumulative coverage is `seq <= ack`; the SACK bitfield covers
    /// `ack + n` for set bits — the exact layout `ack_info` builds, pinned
    /// both ways by
    /// `bit_layout_agrees_with_the_receiver_across_the_whole_window`.
    pub fn handle_ack(&self, ack: u32, ack_bitfield: u32) {
        if ack == 0 {
            return; // "nothing ever acked" is not an ack of sequence 0.
        }

        let mut ledger = self.ledger.lock();
        // Wrap-aware, no head-of-line: cumulatively covered entries and
        // individually SACKed entries leave independently.
        ledger.retain(move |entry| {
            let covered = !seq_is_newer(entry.sequence, ack)
                || (ack_bitfield & (1 << (entry.sequence.wrapping_sub(ack) & 31))) != 0;
            !covered
        });
    }

    /// Bug №215: retransmit pass. Entries untouched for longer than `timeout`
    /// come back (up to `max_count` per call) with their payload bytes; the
    /// caller re-wraps them in a FRESH header carrying the ORIGINAL sequence,
    /// so the receiver's SACK accounting (№167) stays keyed on the same
    /// numbering. An entry whose retry budget ran out is declared lost and
    /// dropped — the connection timeout is the final safety net.
    pub fn collect_retransmits(
        &self,
        timeout: Duration,
        max_retries: u32,
        max_count: usize,
    ) -> Vec<(u32, u8, Vec<u8>)> {
        let mut results = Vec::new();
        let now = Instant::now();

        let mut ledger = self.ledger.lock();
        ledger.retain_mut(|entry| {
            if results.len() >= max_count {
                return true;
            }
            if now.duration_since(entry.sent_at) < timeout {
                return true; // still fresh: wait for the peer's ack header.
            }
            entry.retry_count += 1;
            if entry.retry_count > max_retries {
                self.stats.lock().packets_lost += 1;
                return false;
            }
            // Stamp now regardless of what the caller does with the packet: a
            // caller that fails to send retries on the NEXT pass, which is
            // the correct backoff for a saturated link.
            entry.sent_at = now;
            self.stats.lock().packets_retransmitted += 1;
            results.push((entry.sequence, entry.packet_type, entry.clone().data));
            true
        });

        results
    }

    pub fn clear_send_queue(&self) {
        self.ledger.lock().clear();
    }

    pub fn stats(&self) -> ChannelStats {
        self.stats.lock().clone()
    }

    pub fn pending_count(&self) -> usize {
        self.ledger.lock().len()
    }

    #[cfg(test)]
    pub fn contains(&self, sequence: u32) -> bool {
        self.ledger.lock().iter().any(|e| e.sequence == sequence)
    }
}

#[cfg(test)]
mod ledger_tests {
    use super::*;
    use rfs_core::packet::PacketType;

    const DATA_A: &[u8] = b"command-ack-a";
    const DATA_B: &[u8] = b"command-ack-b";

    fn channel() -> Channel {
        Channel::new(ChannelConfig::new(ChannelType::ReliableOrdered, 8))
    }

    #[test]
    fn a_registered_datagram_is_mirrored_once() {
        let ch = channel();
        ch.register_sent(1, PacketType::CommandAck as u8, DATA_A);
        ch.register_sent(1, PacketType::CommandAck as u8, DATA_B);

        assert!(ch.contains(1), "the entry survives the duplicate register");
        assert_eq!(ch.pending_count(), 1, "the same sequence is one entry");
        assert_eq!(ch.stats().packets_sent, 1, "retransmits do not re-count");
    }

    #[test]
    fn ack_bits_and_cumulative_both_retire_entries() {
        let ch = channel();
        ch.register_sent(10, PacketType::CommandAck as u8, DATA_A);
        ch.register_sent(11, PacketType::CommandAck as u8, DATA_B);
        ch.register_sent(12, PacketType::CommandAck as u8, DATA_A);

        // Cumulative ack of 10: only 10 leaves. (№15: no head-of-line.)
        ch.handle_ack(10, 0);
        assert_eq!(ch.pending_count(), 2);

        // SACK bit 2 (ack + 2 === 12) leaves 12 independently of 11.
        ch.handle_ack(10, 1 << 2);
        assert_eq!(ch.pending_count(), 1, "only 11 remains pinned");
        assert!(ch.contains(11));
    }

    #[test]
    fn overflow_evicts_the_oldest_and_counts() {
        let ch = channel(); // send_buffer_size = 8
        for seq in 1..=10u32 {
            ch.register_sent(seq, PacketType::CommandAck as u8, DATA_A);
        }
        assert_eq!(ch.pending_count(), 8);
        let stats = ch.stats();
        assert_eq!(stats.ledger_overflow, 2, "1 and 2 were evicted");
        assert!(!ch.contains(1) && !ch.contains(2));
        assert!(ch.contains(9), "fresh entries win");
    }

    #[test]
    fn collect_respects_the_timeout_and_stamps_the_retry() {
        let ch = channel();
        ch.register_sent(1, PacketType::CommandAck as u8, DATA_A);

        // Nothing collected while the entry is fresh.
        assert!(
            ch.collect_retransmits(Duration::from_millis(30), 3, 8)
                .is_empty(),
            "a fresh entry must wait for its ack"
        );

        std::thread::sleep(Duration::from_millis(40));
        let got = ch.collect_retransmits(Duration::from_millis(30), 3, 8);
        assert_eq!(got.len(), 1);
        assert_eq!(
            got[0],
            (1, PacketType::CommandAck as u8, DATA_A.to_vec()),
            "the retransmit carries the original sequence, type and bytes"
        );
        assert_eq!(ch.stats().packets_retransmitted, 1);
        assert_eq!(ch.pending_count(), 1, "still awaiting the ack");
    }

    #[test]
    fn the_retry_budget_declares_the_entry_lost() {
        // NLLINE-CHECK-TEMP: change `entry.retry_count += 1` to
        // `entry.retry_count = 0` in `collect_retransmits` and this test
        // hangs as an eternal resend queue — the budget disappears.
        let ch = channel();
        ch.register_sent(1, PacketType::CommandAck as u8, DATA_A);

        for pass in 1..=3u32 {
            let got = ch.collect_retransmits(Duration::ZERO, 3, 8);
            if pass <= 3 {
                assert_eq!(got.len(), 1, "pass {pass} still has budget");
            }
        }
        let got = ch.collect_retransmits(Duration::ZERO, 3, 8);
        assert!(got.is_empty(), "budget exhausted — declared lost");
        assert_eq!(ch.stats().packets_lost, 1);
        assert_eq!(ch.pending_count(), 0);
    }

    #[test]
    fn max_count_throttles_the_pass() {
        let ch = channel();
        for seq in 1..=5u32 {
            ch.register_sent(seq, PacketType::CommandAck as u8, DATA_A);
        }
        let got = ch.collect_retransmits(Duration::ZERO, 10, 2);
        assert_eq!(got.len(), 2, "the caller may throttle a burst");
        assert_eq!(ch.pending_count(), 5, "throttled entries stay queued");
    }
}
