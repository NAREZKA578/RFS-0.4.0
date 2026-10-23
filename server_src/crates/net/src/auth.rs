//! Per-datagram authentication (bug №700, owner plan point 1).
//!
//! Every datagram going across the wire now carries, **before** the packet
//! header, an authentication block:
//!
//! ```text
//! [tag: 16 bytes][seq: 8 bytes big-endian][packet bytes: header + payload]
//! ```
//!
//! The tag is HMAC-SHA256 truncated to 16 bytes over
//! `direction || seq_be || packet_bytes`, keyed by the match's shared secret.
//! Length is integrity: a tampered or forged datagram refuses at the same
//! door a malformed one does, with no state allocated and no event emitted.
//!
//! The direction constants fold into the MAC input so a client-to-server
//! datagram cannot be replayed toward the client — the two sides would have
//! to agree on the secret anyway, and reflection against the same secret is
//! the first trick to try.
//!
//! The sequence numbers make a captured datagram useless: the receiver keeps
//! a small sliding window per sender and refuses anything older than it has
//! already seen or equal to it. UDP reorder is real, so the window is wide
//! rather than a strict "ever increasing" check.
//!
//! Without a token both sides speak the same unsigned dialect: the block is
//! still there (a zero tag, a zero sequence) so the framing files never
//! fork, but nothing is verified, and the server says so in its log at
//! startup. Secrecy is explicitly out of scope — this layer proves who sent
//! the datagram and that it was not altered, and it proves nothing else.

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// The truncated HMAC over one datagram. 16 bytes is the standard margin for
/// a 256-bit hash in byte-oriented protocol wrappers.
pub const TAG_LEN: usize = 16;
/// The per-sender replay counter, one consumer of the same framing.
pub const SEQ_LEN: usize = 8;
/// The whole block every datagram grows by. Stated in the core budget where
/// the wire economy is defined along with it.
pub const AUTH_WIRE_LEN: usize = TAG_LEN + SEQ_LEN;

/// Which way a datagram travels, folded into the MAC input.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    ServerToClient,
    ClientToServer,
}

impl Direction {
    fn byte(self) -> u8 {
        match self {
            Self::ServerToClient => b's',
            Self::ClientToServer => b'c',
        }
    }
}

/// The HMAC tag over one packet, or the zero tag of the unsigned dialect.
pub fn tag(key: Option<&[u8]>, direction: Direction, seq: u64, packet: &[u8]) -> [u8; TAG_LEN] {
    match key {
        // Unsigned dialect: the block exists so the framing never forks,
        // and the zero tag is what both sides write and expect.
        None => [0u8; TAG_LEN],
        Some(key) => {
            let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
            mac.update(&[direction.byte()]);
            mac.update(&seq.to_be_bytes());
            mac.update(packet);
            let full = mac.finalize().into_bytes();
            let mut out = [0u8; TAG_LEN];
            out.copy_from_slice(&full[..TAG_LEN]);
            out
        }
    }
}

/// Prefixes `packet` with the authentication block in `out`.
pub fn seal(key: Option<&[u8]>, direction: Direction, seq: u64, packet: &[u8], out: &mut Vec<u8>) {
    let start = out.len();
    out.resize(start + AUTH_WIRE_LEN, 0u8);
    out[start..start + TAG_LEN].copy_from_slice(&tag(key, direction, seq, packet));
    out[start + TAG_LEN..].copy_from_slice(&seq.to_be_bytes());
    out.extend_from_slice(packet);
}

/// Verifies and strips the block. `None` means the datagram is refused as
/// forged, tampered, or shaped like nothing this layer ever sent — the
/// caller drops it the same way it drops a packet too small to parse.
pub fn unseal<'a>(
    key: Option<&[u8]>,
    direction: Direction,
    datagram: &'a [u8],
) -> Option<(u64, &'a [u8])> {
    if datagram.len() < AUTH_WIRE_LEN {
        return None;
    }
    let (block, packet) = datagram.split_at(AUTH_WIRE_LEN);
    let seq = u64::from_be_bytes(block[TAG_LEN..].try_into().expect("SEQ_LEN = 8"));
    if tag(key, direction, seq, packet) != block[..TAG_LEN] {
        return None;
    }
    Some((seq, packet))
}

/// Removes the block without checking anything. The tests of neighboring
/// modules speak the unsigned dialect, and the receive funnels take this
/// helper where production verifies.
pub fn unseal_unsigned(datagram: &[u8]) -> Option<&[u8]> {
    datagram
        .len()
        .checked_sub(AUTH_WIRE_LEN)
        .map(|len| &datagram[len..])
}

/// A 64-wide replay window, the classic anti-replay structure: bit `i` of
/// `seen` says whether `highest - i` was accepted. Sequence numbers start
/// at 1 on both senders; 0 is what the unsigned dialect repeats and never
/// reaches this window, because the window is only consulted when a key
/// is present.
#[derive(Debug, Clone)]
pub struct ReplayWindow {
    highest: u64,
    seen: u64,
}

impl Default for ReplayWindow {
    fn default() -> Self {
        Self {
            highest: 0,
            seen: 0,
        }
    }
}

impl ReplayWindow {
    /// Whether this sequence may be applied now. Accepting a number marks
    /// it, so a second appearance refuses even inside the window.
    pub fn accept(&mut self, seq: u64) -> bool {
        if seq == 0 {
            return false;
        }
        if seq > self.highest {
            // Everything older than the previous high falls out of the
            // window; a jump past it empties the mask entirely.
            let shift = seq - self.highest;
            self.seen = if shift >= 64 { 0 } else { self.seen << shift };
            self.seen |= 1;
            self.highest = seq;
            return true;
        }
        let back = self.highest - seq;
        if back >= 64 {
            return false;
        }
        let bit = 1u64 << back;
        if self.seen & bit != 0 {
            return false;
        }
        self.seen |= bit;
        true
    }
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    fn key() -> Vec<u8> {
        b"match-secret-for-tests".to_vec()
    }

    #[test]
    fn a_sealed_datagram_opens_to_its_packet() {
        let packet = b"packet bytes".as_slice();
        let mut wire = Vec::new();
        seal(
            Some(&key()),
            Direction::ClientToServer,
            1,
            packet,
            &mut wire,
        );
        assert_eq!(AUTH_WIRE_LEN + packet.len(), wire.len(), "framing");
        let (seq, rest) =
            unseal(Some(&key()), Direction::ClientToServer, &wire).expect("the block opens");
        assert_eq!(seq, 1);
        assert_eq!(rest, packet);
    }

    #[test]
    fn a_tampered_datagram_opens_to_nothing() {
        let packet = b"packet bytes".as_slice();
        let mut wire = Vec::new();
        seal(
            Some(&key()),
            Direction::ClientToServer,
            1,
            packet,
            &mut wire,
        );
        let last = wire.len() - 1;
        wire[last] ^= 0x80;
        assert!(unseal(Some(&key()), Direction::ClientToServer, &wire).is_none());
    }

    #[test]
    fn a_wire_from_the_other_direction_opens_to_nothing() {
        let packet = b"packet bytes".as_slice();
        let mut wire = Vec::new();
        seal(
            Some(&key()),
            Direction::ClientToServer,
            1,
            packet,
            &mut wire,
        );
        assert!(unseal(Some(&key()), Direction::ServerToClient, &wire).is_none());
    }

    #[test]
    fn a_foreign_secret_opens_to_nothing() {
        let packet = b"packet bytes".as_slice();
        let mut wire = Vec::new();
        seal(
            Some(&key()),
            Direction::ClientToServer,
            1,
            packet,
            &mut wire,
        );
        assert!(
            unseal(
                Some(&b"the other match's secret".to_vec()),
                Direction::ClientToServer,
                &wire
            )
            .is_none(),
            "a datagram whose tag was made under a different secret must refuse"
        );
    }

    #[test]
    fn without_a_key_the_zero_block_passes_without_verification() {
        let packet = b"packet bytes".as_slice();
        let mut wire = Vec::new();
        seal(None, Direction::ClientToServer, 0, packet, &mut wire);
        assert_eq!(
            &tag(None, Direction::ClientToServer, 0, packet),
            &[0u8; TAG_LEN]
        );
        let (seq, rest) = unseal(None, Direction::ClientToServer, &wire).expect("unsigned opens");
        assert_eq!(seq, 0);
        assert_eq!(rest, packet);
    }

    #[test]
    fn a_window_refuses_the_same_sequence_twice() {
        let mut window = ReplayWindow::default();
        assert!(window.accept(5));
        assert!(!window.accept(5));
        assert!(window.accept(6), "the next number still lands");
        assert!(!window.accept(6));
    }

    #[test]
    fn a_window_refuses_a_sequence_older_than_it_remembers() {
        let mut window = ReplayWindow::default();
        assert!(window.accept(1));
        for n in 2..=64 {
            assert!(window.accept(n), "filling the window must hold, n={n}");
        }
        assert!(
            !window.accept(1),
            "the first number just fell out of the window"
        );
    }

    #[test]
    fn a_new_high_number_walks_the_window_forward() {
        let mut window = ReplayWindow::default();
        for n in 1..=64 {
            assert!(window.accept(n), "filling the window must hold, n={n}");
        }
        assert!(window.accept(65), "one past the high still lands");
        assert!(
            !window.accept(2),
            "2 is exactly on the outgoing edge of the window"
        );
        assert!(!window.accept(65), "and the new high is already marked");
    }

    #[test]
    fn a_window_absorbs_reorder_around_a_leap() {
        let mut window = ReplayWindow::default();
        assert!(window.accept(1));
        assert!(window.accept(100));
        // 99 is inside the window and unseen, then 100 must stay marked.
        assert!(window.accept(99));
        assert!(!window.accept(100));
    }
}
