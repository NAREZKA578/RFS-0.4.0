use parking_lot::Mutex;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub struct BandwidthTracker {
    sent_history: Mutex<VecDeque<(Instant, usize)>>,
    received_history: Mutex<VecDeque<(Instant, usize)>>,
    window: Duration,
    sent_bytes: Mutex<u64>,
    received_bytes: Mutex<u64>,
    sent_packets: Mutex<u64>,
    received_packets: Mutex<u64>,
}

impl Default for BandwidthTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl BandwidthTracker {
    pub fn new() -> Self {
        Self {
            sent_history: Mutex::new(VecDeque::new()),
            received_history: Mutex::new(VecDeque::new()),
            window: Duration::from_secs(1),
            sent_bytes: Mutex::new(0),
            received_bytes: Mutex::new(0),
            sent_packets: Mutex::new(0),
            received_packets: Mutex::new(0),
        }
    }

    pub fn with_window(window: Duration) -> Self {
        // Zero window would divide by zero in sent_bps/received_bps (inf).
        let window = if window.is_zero() {
            Duration::from_secs(1)
        } else {
            window
        };
        Self {
            sent_history: Mutex::new(VecDeque::new()),
            received_history: Mutex::new(VecDeque::new()),
            window,
            sent_bytes: Mutex::new(0),
            received_bytes: Mutex::new(0),
            sent_packets: Mutex::new(0),
            received_packets: Mutex::new(0),
        }
    }

    pub fn record_sent(&self, bytes: usize) {
        let now = Instant::now();
        let mut history = self.sent_history.lock();
        history.push_back((now, bytes));
        self.prune_history(&mut history, now);
        *self.sent_bytes.lock() += bytes as u64;
        *self.sent_packets.lock() += 1;
    }

    pub fn record_received(&self, bytes: usize) {
        let now = Instant::now();
        let mut history = self.received_history.lock();
        history.push_back((now, bytes));
        self.prune_history(&mut history, now);
        *self.received_bytes.lock() += bytes as u64;
        *self.received_packets.lock() += 1;
    }

    fn prune_history(&self, history: &mut VecDeque<(Instant, usize)>, now: Instant) {
        let cutoff = now - self.window;
        while let Some((time, _)) = history.front() {
            if *time < cutoff {
                history.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn sent_bps(&self) -> f64 {
        let history = self.sent_history.lock();
        if history.is_empty() {
            return 0.0;
        }
        let now = Instant::now();
        let cutoff = now - self.window;
        let total: usize = history.iter()
            .filter(|(t, _)| *t >= cutoff)
            .map(|(_, b)| *b)
            .sum();
        total as f64 / self.window.as_secs_f64()
    }

    pub fn received_bps(&self) -> f64 {
        let history = self.received_history.lock();
        if history.is_empty() {
            return 0.0;
        }
        let now = Instant::now();
        let cutoff = now - self.window;
        let total: usize = history.iter()
            .filter(|(t, _)| *t >= cutoff)
            .map(|(_, b)| *b)
            .sum();
        total as f64 / self.window.as_secs_f64()
    }

    pub fn sent_bytes_total(&self) -> u64 {
        *self.sent_bytes.lock()
    }

    pub fn received_bytes_total(&self) -> u64 {
        *self.received_bytes.lock()
    }

    pub fn sent_packets_total(&self) -> u64 {
        *self.sent_packets.lock()
    }

    pub fn received_packets_total(&self) -> u64 {
        *self.received_packets.lock()
    }

    pub fn avg_packet_size_sent(&self) -> f64 {
        let packets = *self.sent_packets.lock();
        if packets == 0 {
            0.0
        } else {
            *self.sent_bytes.lock() as f64 / packets as f64
        }
    }

    pub fn avg_packet_size_received(&self) -> f64 {
        let packets = *self.received_packets.lock();
        if packets == 0 {
            0.0
        } else {
            *self.received_bytes.lock() as f64 / packets as f64
        }
    }

    pub fn stats(&self) -> BandwidthStats {
        BandwidthStats {
            sent_bps: self.sent_bps(),
            received_bps: self.received_bps(),
            sent_bytes_total: self.sent_bytes_total(),
            received_bytes_total: self.received_bytes_total(),
            sent_packets_total: self.sent_packets_total(),
            received_packets_total: self.received_packets_total(),
            avg_packet_size_sent: self.avg_packet_size_sent(),
            avg_packet_size_received: self.avg_packet_size_received(),
        }
    }

    pub fn reset(&self) {
        self.sent_history.lock().clear();
        self.received_history.lock().clear();
        *self.sent_bytes.lock() = 0;
        *self.received_bytes.lock() = 0;
        *self.sent_packets.lock() = 0;
        *self.received_packets.lock() = 0;
    }
}

#[derive(Debug, Clone)]
pub struct BandwidthStats {
    pub sent_bps: f64,
    pub received_bps: f64,
    pub sent_bytes_total: u64,
    pub received_bytes_total: u64,
    pub sent_packets_total: u64,
    pub received_packets_total: u64,
    pub avg_packet_size_sent: f64,
    pub avg_packet_size_received: f64,
}

impl Default for BandwidthStats {
    fn default() -> Self {
        Self {
            sent_bps: 0.0,
            received_bps: 0.0,
            sent_bytes_total: 0,
            received_bytes_total: 0,
            sent_packets_total: 0,
            received_packets_total: 0,
            avg_packet_size_sent: 0.0,
            avg_packet_size_received: 0.0,
        }
    }
}

pub struct BandwidthLimiter {
    bucket: Mutex<TokenBucket>,
}

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    max_bps: f64,
    last_update: Instant,
}

impl BandwidthLimiter {
    pub fn new(max_bps: f64) -> Self {
        // NaN/negative/zero would blackhole everything (NaN >= cost is false)
        // or divide by zero in wait_for — sanitize to a sane default.
        let sane = if max_bps.is_finite() && max_bps > 0.0 {
            max_bps
        } else {
            1_000_000.0
        };
        Self {
            bucket: Mutex::new(TokenBucket {
                tokens: sane,
                max_tokens: sane,
                max_bps: sane,
                last_update: Instant::now(),
            }),
        }
    }

    pub fn try_consume(&self, bytes: usize) -> bool {
        let mut bucket = self.bucket.lock();
        let now = Instant::now();
        let elapsed = now - bucket.last_update;
        bucket.last_update = now;
        
        bucket.tokens += elapsed.as_secs_f64() * bucket.max_bps;
        if bucket.tokens > bucket.max_tokens {
            bucket.tokens = bucket.max_tokens;
        }
        
        let cost = bytes as f64;
        if bucket.tokens >= cost {
            bucket.tokens -= cost;
            true
        } else {
            false
        }
    }

    pub fn wait_for(&self, bytes: usize) -> Duration {
        let bucket = self.bucket.lock();
        let tokens = bucket.tokens;
        let cost = bytes as f64;
        
        if tokens >= cost {
            return Duration::ZERO;
        }
        
        let needed = cost - tokens;
        let seconds = needed / bucket.max_bps;
        Duration::from_secs_f64(seconds)
    }

    pub fn set_max_bps(&self, max_bps: f64) {
        // Bug №176: `max_bps.max(0.0)` still let 0 through, and a zero rate
        // makes `wait_for` compute `needed / 0` = inf, which
        // `Duration::from_secs_f64` panics on — turning a rate change into a
        // crash of the send path. NaN compares false against everything, so
        // the same guard as the constructor is required here.
        let sane = if max_bps.is_finite() && max_bps > 0.0 {
            max_bps
        } else {
            1_000_000.0
        };
        // Bug №39: bumping max_tokens alone left the bucket refill rate at
        // the OLD tempo, so the "new" limit silently did nothing. Update the
        // rate too.
        let mut bucket = self.bucket.lock();
        bucket.max_bps = sane;
        bucket.max_tokens = sane;
        if bucket.tokens > sane {
            bucket.tokens = sane;
        }
    }
}

/// What the limiter says about putting a packet on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// Tokens available — send it now.
    Send,
    /// Out of tokens, and the packet is unreliable. Dropping it is correct:
    /// the next state/update supersedes it anyway.
    Drop,
    /// Out of tokens, but the packet is reliable. It must be retried, never
    /// discarded — the Channel layer has no retransmit queue yet, so a drop
    /// here loses the message for good (bug №171). The payload is how long to
    /// wait before tokens should have accrued.
    Retry(Duration),
}

impl BandwidthLimiter {
    /// Ask for permission to send `bytes`.
    ///
    /// `reliable` must reflect the packet's channel: a reliable packet that
    /// gets `Retry` is the caller's responsibility to resend, an unreliable
    /// one may be dropped without consequence.
    pub fn admit(&self, bytes: usize, reliable: bool) -> Admission {
        if self.try_consume(bytes) {
            Admission::Send
        } else if reliable {
            Admission::Retry(self.wait_for(bytes))
        } else {
            Admission::Drop
        }
    }
}

#[cfg(test)]
mod limiter_tests {
    use super::*;

    fn limiter(bps: f64) -> BandwidthLimiter {
        BandwidthLimiter::new(bps)
    }

    #[test]
    fn an_unreliable_packet_may_be_dropped() {
        // Bug №171: this is the old behaviour and it is fine for unreliable
        // traffic — a newer packet supersedes it.
        let l = limiter(1_000.0);
        assert_eq!(l.admit(1_000, false), Admission::Send);
        assert_eq!(l.admit(1_000, false), Admission::Drop);
    }

    #[test]
    fn a_reliable_packet_is_never_advised_to_drop() {
        let l = limiter(1_000.0);
        assert_eq!(l.admit(1_000, true), Admission::Send);
        match l.admit(1_000, true) {
            Admission::Retry(wait) => {
                assert!(wait > Duration::ZERO, "a retry must carry a real wait");
            }
            other => panic!("a reliable packet must never be dropped, got {other:?}"),
        }
    }

    #[test]
    fn the_retry_wait_matches_the_refill_rate() {
        // 1 000 B/s budget, bucket drained, 500 B packet -> ~0.5 s.
        let l = limiter(1_000.0);
        assert!(l.try_consume(1_000));
        let Admission::Retry(wait) = l.admit(500, true) else {
            panic!("expected a retry");
        };
        let secs = wait.as_secs_f64();
        assert!(
            (0.4..0.6).contains(&secs),
            "expected roughly half a second, got {secs}"
        );
    }

    #[test]
    fn a_degenerate_limit_never_deadlocks_reliable_traffic() {
        // set_max_bps(0) used to leave max_bps at 0, and `wait_for` then
        // computed `needed / 0` = inf, which `Duration::from_secs_f64` panics
        // on (bug №176). It must instead sanitise to a usable rate.
        //
        // `wait_for` reads the rate straight out of the bucket rather than
        // re-sanitising, so it is the direct expression of the invariant:
        // whatever the caller set, a full bucket must never divide by zero,
        // produce a non-finite duration, or deadlock reliable traffic.
        for degenerate in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            let l = limiter(1_000.0);
            assert!(l.try_consume(1_000), "drain the bucket first");
            l.set_max_bps(degenerate);

            // A full bucket at a sanitised rate => zero wait, and `admit`
            // accepts the reliable packet instead of retrying forever.
            assert_eq!(l.wait_for(0), Duration::ZERO, "{degenerate}");

            // The rate itself must be usable: positive, finite and non-zero,
            // otherwise `wait_for` would panic or return a silly wait.
            let rate = l.bucket.lock().max_bps;
            assert!(
                rate.is_finite() && rate > 0.0,
                "{degenerate}: sanitised to an unusable rate {rate}"
            );

            // A packet far larger than the bucket cannot be admitted, and the
            // resulting retry must be a real, finite, non-zero wait rather
            // than a panic or an infinite stall.
            let Admission::Retry(wait) = l.admit(rate.ceil() as usize * 2 + 1_000_000, true) else {
                panic!("{degenerate}: expected a retry, not an immediate send");
            };
            assert!(
                wait > Duration::ZERO && wait < Duration::from_secs(60),
                "{degenerate}: a sanitised rate must give a bounded wait, got {wait:?}"
            );
        }
    }
}