//! The action journal (`.DOC/new/server-interface-plan.md`, Приложение А,
//! А.2 item 6; section 6.3: a journal covers everything that changes
//! something). The door and the tick loop both write here, `audit` reads
//! from here.
//!
//! One action, one line, JSON — time, source, command, outcome — appended
//! to the file next to the config (`control.action_log_file` renames it, the
//! directory stays the config's own). The in-memory ring holds the same
//! lines because a file tail is a bad shape for a control answer: `audit`
//! wants the LAST N actions, oldest first, without a disk read racing an
//! append.
//!
//! A file that stops accepting writes never stops the journal: the ring
//! keeps recording, the first failure says so once, and every later append
//! attempt stays silent instead of spamming a full disk. `audit` reports
//! the broken state — a control answer that looked healthy while nothing
//! was being written would be a lie of exactly the kind the config rules
//! refuse.

use anyhow::Context;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::warn;

/// The tail `audit` reads from and the ring keeps: recent actions, capped.
/// A journal is an operational record, not an archive — the file carries
/// the whole history, the ring only what a scan can see at a glance.
pub const RING_CAP: usize = 512;

/// One journaled action. `ts` is UTC wall time (`…T…Z`), `source` is the
/// door (`"door"`) or the tick loop (`"loop"`), `command` is the command
/// line with its visible arguments, `outcome` is what happened. No secret
/// ever belongs in any of these — the control token and the №700 key are
/// never arguments a door receives in the first place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub ts: String,
    pub source: String,
    pub command: String,
    pub outcome: String,
}

#[derive(Debug)]
struct Inner {
    /// The append handle; `None` from the first failed write on — the disk
    /// said a thing the retries are not entitled to keep saying.
    file: Mutex<Option<File>>,
    ring: Mutex<Vec<Entry>>,
    file_failed: AtomicBool,
}

/// A cloneable handle; the door and the loop each hold one over the same
/// file and the same ring.
#[derive(Clone)]
pub struct ActionJournal {
    inner: Arc<Inner>,
}

impl ActionJournal {
    /// Opens (or creates) the journal file in append mode. A failure is a
    /// hard stop at wiring time, the way a config the server cannot read
    /// is: a door that mutates the match without a journal would be acting
    /// outside the plan's own contract for stage 2.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening the action journal {}", path.display()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                file: Mutex::new(Some(file)),
                ring: Mutex::new(Vec::new()),
                file_failed: AtomicBool::new(false),
            }),
        })
    }

    /// Records one action: the ring takes it immediately, the file gets one
    /// flushed append per line so an operator reading the file mid-run sees
    /// what has happened, not what will happen.
    pub fn record(&self, source: &str, command: &str, outcome: &str) {
        let entry = Entry {
            ts: format_now(),
            source: source.into(),
            command: command.into(),
            outcome: outcome.into(),
        };
        {
            let mut ring = self.inner.ring.lock();
            ring.push(entry.clone());
            if ring.len() > RING_CAP {
                let excess = ring.len() - RING_CAP;
                ring.drain(..excess);
            }
        }
        self.append(&entry);
    }

    fn append(&self, entry: &Entry) {
        if self.inner.file_failed.load(Ordering::Relaxed) {
            return;
        }
        let line = match serde_json::to_string(entry) {
            Ok(line) => line,
            Err(_) => "{}".to_string(),
        };
        let mut guard = self.inner.file.lock();
        let Some(file) = guard.as_mut() else {
            return;
        };
        let written = file
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|()| file.flush());
        if written.is_err() {
            self.inner.file_failed.store(true, Ordering::Relaxed);
            *guard = None;
            warn!(
                source = entry.source,
                command = entry.command,
                "action journal: the file stopped accepting writes; actions keep the ring only"
            );
        }
    }

    /// The last `tail` actions, oldest first. `audit` asks for this; a larger
    /// tail than the ring holds returns the whole ring.
    pub fn snapshot_tail(&self, tail: usize) -> Vec<Entry> {
        let ring = self.inner.ring.lock();
        let skipping = ring.len().saturating_sub(tail);
        ring[skipping..].to_vec()
    }

    /// Whether the file stopped accepting writes — `audit` reports it, so an
    /// operator reading the tail is not lied to about where those lines are.
    pub fn file_failed(&self) -> bool {
        self.inner.file_failed.load(Ordering::Relaxed)
    }
}

/// UTC wall time formatted for the journal: `2026-11-04T12:34:56.123Z`. The
/// days to civil conversion is Howard Hinnant's `civil_from_days`, rejected
/// here in favour of a tested 30-line function over a new dependency (plan
/// А.1 item 5: none needed for the control port).
fn format_now() -> String {
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format_utc(wall)
}

fn format_utc(wall: Duration) -> String {
    let secs = wall.as_secs();
    let millis = wall.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
}

/// Count of whole days since 1970-01-01 → a (year, month, day) UTC triple.
/// `div_euclid` keeps `era` exact for negative (pre-epoch) day counts, so a
/// clock jumped behind 1970 formats something sane instead of wrapping.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m as u32, d)
}

#[cfg(test)]
mod journal_tests {
    use super::*;
    use crate::config::MatchConfig;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "rfs_server_journal_tests_{}_{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).expect("temp dir is creatable");
        base
    }

    /// Format pins against dates chosen for their exposure: the epoch, a
    /// leap day (2000-02-29), a date past the 2100 non-leap century rule
    /// (2033 is still short of it — 2100 is where naive leap logic rots),
    /// and a known-good popular reference (1_700_000_000).
    #[test]
    fn the_utc_formatter_pins_known_dates() {
        let f = |s: u64| format_utc(Duration::from_secs(s));
        assert_eq!(f(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(f(951_782_400), "2000-02-29T00:00:00.000Z", "leap day");
        assert_eq!(f(1_700_000_000), "2023-11-14T22:13:20.000Z");
        // 2100 is not a leap year (century rule); 2100-01-01 = 47_482 days,
        // Jan(31)+Feb(28) puts Mar 1 on day 47_541 = 4_107_542_400. The naive
        // 400-year leap wrap would name 2100-02-29 here.
        assert_eq!(f(4_107_542_400), "2100-03-01T00:00:00.000Z");
    }

    /// A clock behind epoch (`unwrap_or_default` path) formats as 1970, not
    /// as a panic: a journal must never be the thing that takes the server
    /// down.
    #[test]
    fn the_formatter_survives_a_pre_epoch_clock() {
        let y = civil_from_days(-1_000).0;
        assert!(y < 1970, "a pre-epoch day belongs before 1970");
    }

    #[test]
    fn one_action_is_one_line_the_operations_scanner_can_read() {
        let path = temp_dir("one_line").join("journal_one_line.jsonl");
        let _ = std::fs::remove_file(&path);
        let journal = ActionJournal::open(&path).expect("the journal opens");
        journal.record("door", "kick 7", "queued");
        journal.record("loop", "kick 7", "reason delivered and kicked");

        let text = std::fs::read_to_string(&path).expect("the file is readable");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "one action, one line, in write order");
        let first: Entry =
            serde_json::from_str(lines[0]).expect("a journal line is the JSON shape");
        assert_eq!(first.source, "door");
        assert_eq!(first.command, "kick 7");
        assert_eq!(first.outcome, "queued");
        assert_eq!(first.ts.len(), "2026-11-04T12:34:56.123Z".len());
        assert!(first.ts.ends_with('Z'), "the stamp carries its timezone");
    }

    #[test]
    fn the_ring_serves_the_last_actions_oldest_first() {
        let path = temp_dir("ring").join("journal_ring.jsonl");
        let _ = std::fs::remove_file(&path);
        let journal = ActionJournal::open(&path).expect("the journal opens");
        for i in 0..(RING_CAP + 10) {
            journal.record("door", &format!("cmd {i}"), "queued");
        }
        let tail = journal.snapshot_tail(5);
        assert_eq!(tail.len(), 5);
        assert_eq!(tail[0].command, format!("cmd {}", RING_CAP + 5));
        assert_eq!(tail[4].command, format!("cmd {}", RING_CAP + 9));
        // The oldest entries exceeding the cap are gone from the ring.
        assert!(journal.snapshot_tail(RING_CAP + 10).len() <= RING_CAP);
    }

    /// The config's own rule: the default names no journal, so the loader
    /// resolves the built-in name. (The full defaults-are-valid rule lives
    /// in config's own test suite — validate is module-private there.)
    #[test]
    fn the_default_config_accepts_an_unnamed_journal() {
        let config = MatchConfig::default();
        assert_eq!(config.control.action_log_file, None);
    }
}
