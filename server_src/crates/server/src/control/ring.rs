//! The journal the control port serves: a bounded ring in memory plus a
//! broadcast channel for live subscribers (plan А.1 item 2). The server's
//! logs currently exist only in stdout; the ring is the copy a `log` command
//! reads and a `watch` subscription streams — without any change to what the
//! `tracing-subscriber` fmt layer prints.
//!
//! The ring is deliberately a constant (5000, the plan's default), not a
//! config knob: the plan's config section (А.1 item 4) names no ring field,
//! and an unbounded-log setting is a memory decision pretending to be a
//! preference.

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{Event, Level};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// The plan's default ring size.
pub const DEFAULT_RING_CAP: usize = 5000;

/// The broadcast channel's own queue, in events: how far a slow subscriber
/// may fall before the server stops queueing for it. Dropping a subscriber
/// is the only behaviour that keeps a command storm from filling memory; a
/// lagging reader is told so and keeps going (the `watch` forwarder turns the
/// error into a `lagged` event line, not a silent gap). №755: ×4 (owner
/// order 2026-11-06).
pub const SUBSCRIBER_QUEUE: usize = 4096;

/// One journal line as the panel sees it: the bare level and the formatted
/// fields. No timestamp — every copy of the ring would disagree with the file
/// log's clock anyway, and the fmt layer's lines still carry it for anyone
/// comparing the two.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEvent {
    pub level: String,
    pub text: String,
}

/// The rank each level name sorts by — the numeric spine of the `log`
/// command's `level` filter (the plan: "последние N строк журнала с фильтром
/// по уровню"). A minimum level means it and everything above it.
pub fn level_rank(name: &str) -> Option<u8> {
    match name {
        "TRACE" => Some(0),
        "DEBUG" => Some(1),
        "INFO" => Some(2),
        "WARN" => Some(3),
        "ERROR" => Some(4),
        _ => None,
    }
}

pub fn rank_of(level: &Level) -> u8 {
    match *level {
        Level::TRACE => 0,
        Level::DEBUG => 1,
        Level::INFO => 2,
        Level::WARN => 3,
        Level::ERROR => 4,
        // Exhaustive, but a decorator keeps a new level a compile error.
    }
}

struct Shared {
    buf: Mutex<VecDeque<LogEvent>>,
    tx: broadcast::Sender<LogEvent>,
    cap: usize,
}

/// The clonable ring. One instance per server; every layer event and every
/// `log` command goes through it.
#[derive(Clone)]
pub struct LogRing {
    shared: Arc<Shared>,
}

impl LogRing {
    pub fn new(cap: usize) -> Self {
        let (tx, _) = broadcast::channel(SUBSCRIBER_QUEUE);
        Self {
            shared: Arc::new(Shared {
                buf: Mutex::new(VecDeque::with_capacity(cap)), // №755 resident
                tx,
                cap,
            }),
        }
    }

    /// A live subscription: the receiver sees every event recorded after
    /// this call.
    pub fn subscribe(&self) -> broadcast::Receiver<LogEvent> {
        self.shared.tx.subscribe()
    }

    /// Records one event: appends to the bounded ring and fans out to live
    /// subscribers. Called by the tracing layer — direct enough for tests.
    pub fn record(&self, level: &Level, text: impl fmt::Display) {
        let event = LogEvent {
            level: level.to_string(),
            text: text.to_string(),
        };
        let mut buf = self.shared.buf.lock();
        if self.shared.cap == 0 {
            // Degenerate but legal: nobody promised the ring holds anything.
            drop(buf);
            let _ = self.shared.tx.send(event);
            return;
        }
        while buf.len() >= self.shared.cap {
            buf.pop_front();
        }
        buf.push_back(event.clone());
        drop(buf);
        // Fan-out after the ring so a subscriber never reads backwards.
        let _ = self.shared.tx.send(event);
    }

    /// The last `tail` events, oldest first, filtered by a minimum rank.
    /// `tail` beyond the ring's contents simply asks for everything held —
    /// the answer is honest about the room the ring has, not an error.
    pub fn snapshot_tail(&self, tail: usize, min_rank: Option<u8>) -> Vec<LogEvent> {
        let buf = self.shared.buf.lock();
        let take = tail.min(buf.len());
        let skip = buf.len() - take;
        buf.iter()
            .skip(skip)
            .filter(|event| min_rank.is_none_or(|r| rank_of_str(&event.level) >= r))
            .cloned()
            .collect()
    }

    pub fn len(&self) -> usize {
        self.shared.buf.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn rank_of_str(level: &str) -> u8 {
    match level {
        "TRACE" => 0,
        "DEBUG" => 1,
        "INFO" => 2,
        "WARN" => 3,
        "ERROR" => 4,
        _ => 0,
    }
}

/// The field visitor: the `message` field becomes the text; remaining fields
/// ride as `k=v` after it, so a structured log line reads whole in the panel.
struct FieldVisitor {
    message: Option<String>,
    pairs: Vec<String>,
}

impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        } else {
            self.pairs.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        // record_str is what `info!("{}", x)` actually hits most of the time;
        // routing it to the same shape as record_debug keeps both paths one
        // format.
        if field.name() == "message" {
            self.message = Some(value.to_string());
        } else {
            self.pairs.push(format!("{}={value}", field.name()));
        }
    }
}

/// The `tracing` layer: every event the subscriber stack decides to emit is
/// copied into the ring. It stores, prints nothing, and never blocks the
/// emitting thread beyond the small ring lock — the fmt layer stays alone in
/// owning the printed form.
pub struct RingLayer {
    ring: LogRing,
}

impl RingLayer {
    pub fn new(cap: usize) -> (Self, LogRing) {
        let ring = LogRing::new(cap);
        (Self { ring: ring.clone() }, ring)
    }
}

impl<S> Layer<S> for RingLayer
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visitor = FieldVisitor {
            message: None,
            pairs: Vec::new(),
        };
        event.record(&mut visitor);
        let text = match visitor.message {
            Some(message) if visitor.pairs.is_empty() => message,
            Some(message) => format!("{message} {}", visitor.pairs.join(" ")),
            None => visitor.pairs.join(" "),
        };
        self.ring.record(event.metadata().level(), text);
    }
}

#[cfg(test)]
mod ring_tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;
    use std::time::SystemTime;

    #[test]
    fn the_ring_holds_at_most_its_cap_and_serves_the_tail_chronologically() {
        let ring = LogRing::new(3);
        for i in 0..5 {
            ring.record(&Level::INFO, format!("line {i}"));
        }
        assert_eq!(ring.len(), 3, "a 5-event record with cap 3 keeps the newest 3");
        let tail = ring.snapshot_tail(10, None);
        let texts: Vec<&str> = tail.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["line 2", "line 3", "line 4"]);
        assert_eq!(tail[0].level, "INFO");
    }

    #[test]
    fn the_tail_is_bounded_by_what_it_asks_for() {
        let ring = LogRing::new(10);
        for i in 0..4 {
            ring.record(&Level::INFO, format!("line {i}"));
        }
        assert_eq!(ring.snapshot_tail(2, None).len(), 2);
        assert_eq!(ring.snapshot_tail(100, None).len(), 4);
        assert!(LogRing::new(10).is_empty());
    }

    #[test]
    fn the_level_filter_is_inclusive_on_the_named_level() {
        let ring = LogRing::new(10);
        ring.record(&Level::INFO, "noise");
        ring.record(&Level::WARN, "watch this");
        ring.record(&Level::ERROR, "and this");
        let warned = ring.snapshot_tail(10, level_rank("WARN"));
        let texts: Vec<&str> = warned.iter().map(|e| e.text.as_str()).collect();
        assert_eq!(texts, vec!["watch this", "and this"]);
    }

    #[tokio::test]
    async fn a_subscriber_sees_everything_recorded_after_it_subscribed() {
        let ring = LogRing::new(10);
        ring.record(&Level::INFO, "before");
        let mut rx = ring.subscribe();
        ring.record(&Level::WARN, "after");
        let got = rx.recv().await.expect("the event reaches the subscriber");
        assert_eq!(got.text, "after");
        assert_eq!(got.level, "WARN");
    }

    /// The path the wire actually takes: a tracing event emitted under a real
    /// subscriber lands in the ring with its message intact. This is the
    /// proof the Layer/visitor wiring works, not just the ring.
    #[tokio::test]
    async fn an_event_emitted_through_tracing_reaches_the_ring() {
        let (layer, ring) = RingLayer::new(10);
        // A one-layer registry is a complete subscriber here, with the same
        // mechanism (record/visit) production uses.
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("ring reaches the control port: {} items", 1);
        });
        let events = ring.snapshot_tail(10, None);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].level, "INFO");
        assert_eq!(events[0].text, "ring reaches the control port: 1 items");
    }

    /// A zero-cap ring must not wedge the emitter: the only acceptable
    /// behaviour of "keep nothing" is keep nothing and still fan out.
    #[tokio::test]
    async fn a_zero_cap_ring_records_nothing_and_still_fans_out() {
        let ring = LogRing::new(0);
        let mut rx = ring.subscribe();
        ring.record(&Level::INFO, "ghost");
        assert!(ring.is_empty());
        assert_eq!(rx.recv().await.expect("fanout survives an empty ring").text, "ghost");
    }

    /// A lagged subscriber returns Lagged and keeps receiving afterwards —
    /// the forwarder relies on that contract to report the gap for it.
    #[tokio::test]
    async fn a_lagging_subscriber_is_told_how_much_it_missed() {
        let ring = LogRing::new(10);
        let mut rx = ring.subscribe();
        for i in 0..(SUBSCRIBER_QUEUE + 50) {
            ring.record(&Level::INFO, i);
        }
        let err = rx.try_recv().expect_err("the lag fires before an item");
        assert!(
            matches!(err, broadcast::error::TryRecvError::Lagged(missed) if missed > 0),
            "the lag must surface as a count: {err}"
        );
        let _ = SystemTime::now(); // keep the import honest in cfg-skip builds
    }
}
