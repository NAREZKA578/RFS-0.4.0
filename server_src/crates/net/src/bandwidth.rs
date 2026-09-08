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
    max_bps: f64,
    bucket: Mutex<TokenBucket>,
}

struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    last_update: Instant,
}

impl BandwidthLimiter {
    pub fn new(max_bps: f64) -> Self {
        Self {
            max_bps,
            bucket: Mutex::new(TokenBucket {
                tokens: max_bps,
                max_tokens: max_bps,
                last_update: Instant::now(),
            }),
        }
    }

    pub fn try_consume(&self, bytes: usize) -> bool {
        let mut bucket = self.bucket.lock();
        let now = Instant::now();
        let elapsed = now - bucket.last_update;
        bucket.last_update = now;
        
        bucket.tokens += elapsed.as_secs_f64() * self.max_bps;
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
        let seconds = needed / self.max_bps;
        Duration::from_secs_f64(seconds)
    }

    pub fn set_max_bps(&self, max_bps: f64) {
        let mut bucket = self.bucket.lock();
        bucket.max_tokens = max_bps;
        if bucket.tokens > max_bps {
            bucket.tokens = max_bps;
        }
    }
}