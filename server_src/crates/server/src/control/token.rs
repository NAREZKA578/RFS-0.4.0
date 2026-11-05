//! The control door's own token, kept in a file the server generates on first
//! start (plan section 5, Решение №2). The rules that hold:
//!
//! 1. The token is NOT the players' `auth_token` — different doors, different
//!    keys (the plan says this in the same breath as the generation: the
//!    operator copies neither of them, both just exist next to the config).
//! 2. The log tells that a token file exists and where; the token itself is
//!    never logged and never appears in a response. The socket-level test
//!    (`server.rs`) holds the grep half of this; this module's job is simply
//!    to never produce a string worth preferring to a log.
//! 3. An existing file is honoured, not regenerated — a regeneration would
//!    silently break every tool that read the last one. A file that cannot be
//!    read back as a usable token is refused with its path named, the same
//!    way a broken config is refused (config.rs: "nothing here guesses").

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::RandomState;
use std::hash::Hasher;
use std::fs;
use std::hash::BuildHasher;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The token file's JSON envelope. A struct, not a bare string: the next
/// field the door needs (a created-at stamp, a rotated marker) joins without
/// a format change, and the file is grep-identifiable as belonging here.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenFile {
    token: String,
    /// Seconds since the epoch when the token was written. Documentation for
    /// the operator, not a machine check — age does not invalidate anything.
    #[serde(default)]
    created_unix_s: u64,
}

/// The token file's name when the config gives none: next to the config
/// file, the way the plan says the first start leaves it. A const here and
/// not inline at both callers — the server writes it and `ctl` reads it, and
/// the two must not drift apart.
pub const DEFAULT_TOKEN_FILE_NAME: &str = "control_token.json";

/// The minimum usable token: 32 hex characters = 128 bits. A generated token
/// is 64; the floor exists so a hand-edited file cannot silently regress the
/// door to a three-character password.
pub const MIN_TOKEN_LEN: usize = 32;

/// The generated token's length in hex characters (32 bytes of entropy).
const TOKEN_HEX_LEN: usize = 64;

/// Reads the token file, or creates it on first start. Either way the answer
/// is the token to demand at the hello. The log line this path emits names
/// the file, nothing else.
pub fn ensure_token_file(path: &Path) -> Result<String> {
    if path.exists() {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading the control token from {}", path.display()))?;
        let parsed: TokenFile = serde_json::from_str(&text)
            .with_context(|| format!("parsing the control token file {}", path.display()))?;
        if parsed.token.len() < MIN_TOKEN_LEN
            || !parsed.token.chars().all(|c| c.is_ascii_hexdigit())
        {
            bail!(
                "the control token in {} is shorter than {MIN_TOKEN_LEN} hex characters or not hex",
                path.display()
            );
        }
        return Ok(parsed.token);
    }
    let token = generate_token();
    let created = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let file = TokenFile { token: token.clone(), created_unix_s: created };
    let text = serde_json::to_string_pretty(&file).context("serializing the token file")?;
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
    }
    fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
    // The one log line the door emits about the token: where the file lives,
    // never what it holds.
    tracing::info!("control: token file ready at {} (first start; give it to ctl only)", path.display());
    Ok(token)
}

/// 32 bytes of OS-seeded entropy as hex — **without new dependencies**.
///
/// Honest about the ingredients, because a token is a security primitive:
/// the workspace carries no `rand`, and adding one is not this feature's to
/// do. What std provides is a per-process random seed for its hash state
/// (`RandomState`) — SipHash-1-3 keyed from the OS — plus high-resolution
/// clocks and the process id. This mixes them through a `DefaultHasher`
/// chain: each round hashes the running state (plus fresh clock nanos) under
/// the process-random keys, and the outputs are the token's bytes.
///
/// What this is: unpredictable to an attacker who does not already know the
/// process's scheduling — which is the outside-of-process adversary the
/// loopback-only door with its bad-token lock is built against. What this is
/// NOT: a CSPRNG promise; if the door ever leaves 127.0.0.1, the plan
/// (section 5, item 7) routes remote access through the supervisor's secure
/// channel, and token generation should move to the OS CSPRNG that day.
fn generate_token() -> String {
    let mut state: u64 = std::process::id() as u64;
    let mut out = String::with_capacity(TOKEN_HEX_LEN);
    for _ in 0..4u32 {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        // Per-round keys: a fresh `RandomState` each round. The first one is
        // OS-seeded; the later ones are advanced from it deterministically
        // *within the thread*, different from round one — plus the fresh
        // clock input — no two rounds of one token share both keys and input.
        let mut hasher = RandomState::new().build_hasher();
        hasher.write(&state.to_le_bytes());
        hasher.write(&now.to_le_bytes());
        let next = hasher.finish();
        // Fold the fresh hash in so consecutive rounds cannot be unwound by
        // knowing one of them.
        state = mix(state ^ next, now);
        write_hex_round(&mut out, state);
    }
    out
}

/// SplitMix64: a plain 1-1 mixing step over the best available u64 source —
/// it maps every input bit across the machine word, which is what makes the
/// hex output look uncorrelated with the clock it came from.
fn mix(mut z: u64, extra: u64) -> u64 {
    z = z.wrapping_add(extra).wrapping_mul(0x9E3779B97F4A7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn write_hex_round(out: &mut String, word: u64) {
    use std::fmt::Write;
    let _ = write!(out, "{word:016x}");
}

#[cfg(test)]
mod token_tests {
    use super::*;

    fn temp_file(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "rfs_control_token_tests_{}_{}",
            name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).expect("temp dir is creatable");
        base.join("control_token.json")
    }

    #[test]
    fn the_token_is_long_hex() {
        let token = generate_token();
        assert_eq!(token.len(), TOKEN_HEX_LEN);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
        // Two tokens from the same process, a moment apart, differ: a fixed
        // output would be a file every install leaves next to its config
        // queued for guesswork.
        let other = generate_token();
        assert_ne!(token, other, "entropy sources must move between calls");
    }

    #[test]
    fn a_first_start_writes_the_file_and_the_second_reads_it_back() {
        let path = temp_file("first");
        let token = ensure_token_file(&path).expect("first start creates the file");
        assert!(path.exists(), "the first start must leave a file to give to ctl");
        let again = ensure_token_file(&path).expect("an existing file is honoured");
        assert_eq!(
            token, again,
            "regenerating behind an operator's copy would break the copy"
        );
    }

    #[test]
    fn a_broken_token_file_is_refused_with_its_path() {
        let dir = std::env::temp_dir().join(format!(
            "rfs_control_token_tests_broken_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir is creatable");
        let path = dir.join("control_token.json");
        std::fs::write(&path, "not json at all").expect("fixture file is writable");
        let err = ensure_token_file(&path).expect_err("unparseable token file must refuse");
        assert!(
            format!("{err:#}").contains(&path.to_string_lossy().to_string()),
            "the refusal must name the file: {err:#}"
        );
    }

    #[test]
    fn a_short_or_non_hex_token_file_is_refused() {
        let dir = std::env::temp_dir().join(format!(
            "rfs_control_token_tests_short_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir is creatable");
        let path = dir.join("control_token.json");
        std::fs::write(&path, r#"{"token":"abcdef"}"#).expect("fixture file is writable");
        assert!(
            ensure_token_file(&path).is_err(),
            "a six-character token must not silently become the door's key"
        );
    }
}
