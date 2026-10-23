//! The server config file.
//!
//! Owner decision (2026-10-19, VoteKick question of №634): the match keeps the
//! feature and its strength lives in a config file that the server creates
//! with defaults when missing and reads at start. Nothing here guesses: a file
//! that exists but cannot be parsed, or that specifies an unusable value, is
//! refused with the value named — a server that silently ran a different
//! arrangement than the file claims is indistinguishable from a server the
//! operator did not configure.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// The file's path default: the working directory, the way `--ships` defaults
/// to the current match.
pub const DEFAULT_CONFIG_PATH: &str = "server_config.json";

/// Everything an operator can set for a match so far. New sections are added
/// as `#[serde(default)]` fields — an older file then keeps loading, which is
/// the direction a config file should evolve in: added knobs must not require
/// rewriting every existing file.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MatchConfig {
    #[serde(default)]
    pub vote_kick: VoteKickConfig,
    /// Bug №700: the shared secret HMAC-SHA256 keys every datagram with.
    /// A blank value or a missing field runs the server in the unsigned
    /// dialect — accepted framing, no key — and `main` says so in the log.
    #[serde(default)]
    pub auth_token: Option<String>,
}

/// The VoteKick feature: master switch, counting rule and window rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoteKickConfig {
    /// `false` — the command is refused with a named reason, votes never open.
    pub enabled: bool,
    /// The floor on votes needed, whatever the online count is. This is what
    /// stops a single player from self-kicking at 100 % majority.
    pub min_votes: u32,
    /// When online count × majority exceeds `min_votes`, that number wins:
    /// 0.6 asks for six of ten players.
    pub majority: f32,
    /// The vote lives this many simulation ticks after its last ballot, then
    /// expires without a verdict.
    pub window_ticks: u64,
}

impl Default for VoteKickConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_votes: 2,
            majority: 0.6,
            window_ticks: 600,
        }
    }
}

impl MatchConfig {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            // The create-on-first-start the owner asked for: the default file
            // is written so there is something to edit, and the same defaults
            // are used for this run.
            let default = Self::default();
            default
                .write(path)
                .with_context(|| format!("writing the default config to {}", path.display()))?;
            return Ok(default);
        }
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading the server config from {}", path.display()))?;
        let config: Self =
            serde_json::from_str(&text).context(format!("parsing {}", path.display()))?;
        config
            .validate()
            .context(format!("checking {}", path.display()))?;
        Ok(config)
    }

    pub fn write(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent).context(format!("creating {}", parent.display()))?;
            }
        }
        let text = serde_json::to_string_pretty(self).context("serializing the config")?;
        fs::write(path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let vk = &self.vote_kick;
        if !(0.0..=1.0).contains(&vk.majority) {
            anyhow::bail!("vote_kick.majority {} is outside 0..=1", vk.majority);
        }
        if vk.min_votes == 0 {
            anyhow::bail!("vote_kick.min_votes must be at least 1");
        }
        if vk.window_ticks == 0 {
            anyhow::bail!("vote_kick.window_ticks must be at least 1");
        }
        Ok(())
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "rfs_server_config_tests_{}_{}",
            name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock is before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).expect("temp dir is creatable");
        base
    }

    #[test]
    fn a_missing_file_is_created_with_defaults_and_loaded_back() {
        let dir = temp_dir("created");
        let path = dir.join("server_config.json");
        assert!(!path.exists(), "the fixture must start without a file");

        let config = MatchConfig::load(&path).expect("a missing file loads the defaults");
        assert!(
            path.exists(),
            "the first run must leave a file the operator can edit"
        );

        let on_disk: MatchConfig =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("the file was written"))
                .expect("the written file must parse");
        assert_eq!(config, on_disk);
        assert_eq!(config, MatchConfig::default());
        assert!(config.vote_kick.enabled);
    }

    #[test]
    fn an_unusable_value_is_refused_instead_of_reconfigured() {
        let dir = temp_dir("invalid");
        let path = dir.join("server_config.json");
        let text = r#"{"vote_kick": {"enabled": true, "min_votes": 2, "majority": 1.5, "window_ticks": 600}}"#;
        std::fs::write(&path, text).expect("fixture file is writable");

        let err = MatchConfig::load(&path).expect_err("1.5 is not a share");
        assert!(
            format!("{err:#}").contains("vote_kick.majority"),
            "the refusal must name the field: {err:#}"
        );
    }

    #[test]
    fn a_broken_file_is_refused_with_its_path() {
        let dir = temp_dir("broken");
        let path = dir.join("server_config.json");
        std::fs::write(&path, "not json at all").expect("fixture file is writable");

        let err = MatchConfig::load(&path).expect_err("a broken file must not load");
        assert!(
            err.to_string()
                .contains(&path.to_string_lossy().to_string()),
            "the refusal must name the file: {err}"
        );
    }

    /// A second section the config format does not know yet must not require a
    /// full rewrite of every existing file — the direction of evolution the
    /// struct comment promises.
    #[test]
    fn an_unknown_section_keeps_the_file_loadable() {
        let dir = temp_dir("forward");
        let path = dir.join("server_config.json");
        let text = r#"{"vote_kick": {"enabled": false, "min_votes": 3, "majority": 0.5, "window_ticks": 10}, "future_section": {}}"#;
        std::fs::write(&path, text).expect("fixture file is writable");

        let config = MatchConfig::load(&path).expect("an unknown section is not fatal");
        assert!(!config.vote_kick.enabled);
        assert_eq!(config.vote_kick.min_votes, 3);
    }

    /// Two defaults that would make the feature a lie must be refused at the
    /// same door: a zero window expires instantly, a zero floor lets one ballot
    /// pass whatever the online count says.
    #[test]
    fn zero_window_and_zero_floor_are_refused() {
        for (field, json) in [
            (
                "window_ticks",
                r#"{"enabled": true, "min_votes": 2, "majority": 0.6, "window_ticks": 0}"#,
            ),
            (
                "min_votes",
                r#"{"enabled": true, "min_votes": 0, "majority": 0.6, "window_ticks": 600}"#,
            ),
        ] {
            let dir = temp_dir("zero");
            let path = dir.join("server_config.json");
            std::fs::write(&path, format!(r#"{{"vote_kick": {json}}}"#))
                .expect("fixture file is writable");
            let err = MatchConfig::load(&path).expect_err("a zero knob is unusable");
            assert!(
                format!("{err:#}").contains(field),
                "the refusal must name {field}: {err:#}"
            );
        }
    }
}
