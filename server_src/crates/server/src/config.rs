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
    ///
    /// Bug №216: blank is also a *decision* now. See `allow_unsigned`.
    #[serde(default)]
    pub auth_token: Option<String>,
    /// Bug №216: the operator's explicit answer to "the shared secret is
    /// blank". `false` (the serde default — a missing field means it) refuses
    /// to start a server the config file cannot name as keyed; `true` states
    /// the unsigned dialect deliberately, and the log still announces it.
    /// Fail-closed by construction: convenience cannot silently outrun the
    /// №700 hole every fresh install would otherwise reopen.
    #[serde(default)]
    pub allow_unsigned: bool,
    /// The control port (stage 1 of `server-interface-plan.md`, Приложение А).
    /// Every field keeps an old file loading untouched: a file that never
    /// mentioned `control` runs the defaults — port on, listening on the
    /// loopback only.
    #[serde(default)]
    pub control: ControlConfig,
}

/// The control door's knobs (plan section 5 + А.1 item 4). The numbers here
/// are the plan author's proposals the owner left adjustable; the *shape*
/// (loopback-only, one token file, capped) is the owner's accepted decision
/// and is not a knob.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ControlConfig {
    /// `false` — nothing is bound, the door does not exist for this run.
    pub enabled: bool,
    /// The TCP port, loopback (`127.0.0.1`) only by construction. Zero is
    /// refused: a random-assigned port is one `ctl` cannot discover from the
    /// file the plan says it reads.
    pub port: u16,
    /// The token file's name, resolved relative to the config file's
    /// directory by `main`. Created on first start; never regenerated behind
    /// tools that already read it.
    pub token_file: Option<String>,
    /// Concurrent connections the port serves (plan: 4); the excess is
    /// answered busy and closed.
    pub max_connections: usize,
    /// One line's byte cap (plan: 64 KiB). Longer input answers
    /// `bad_arguments` and closes — never a read with no bound.
    pub max_line_bytes: usize,
    /// Commands per second per connection (plan: 20); a token bucket refills
    /// continuously, so a burst may spend the allowance without a spike
    /// reaching the command layer.
    pub commands_per_second: u32,
    /// Consecutive wrong tokens before the door pauses authentication
    /// (plan: 5). Counted across connections: a reconnected spray counts on.
    pub max_bad_tokens: u32,
    /// How long the pause lasts once tripped (plan: 60 s).
    pub bad_token_cooldown_s: u64,
    /// The action journal's file name (stage 2, А.2 item 6), resolved
    /// relative to the config file's directory like `token_file`. `None`
    /// keeps the default name; a blank string is refused by
    /// `validate_control` — an operator who named the field named a file.
    #[serde(default)]
    pub action_log_file: Option<String>,
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            port: 7778,
            token_file: None,
            max_connections: 4,
            max_line_bytes: 64 * 1024,
            commands_per_second: 20,
            max_bad_tokens: 5,
            bad_token_cooldown_s: 60,
            action_log_file: None,
        }
    }
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
        // Bug №216 (fail closed): a blank secret is a choice the file must
        // own. `main` cannot tell "the operator never wrote a token" from
        // "the operator declines auth", and the №700 hole was exactly the
        // first of those running as the second — so the distinction lives
        // here, in the only gate that runs before the network starts.
        let token_set = self
            .auth_token
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty());
        if !token_set && !self.allow_unsigned {
            anyhow::bail!(
                "auth_token is empty and allow_unsigned is false — every datagram would \
                 be accepted on framing alone. Set `auth_token` to a shared secret, or \
                 set \"allow_unsigned\": true explicitly if the unsigned dialect is the \
                 intended arrangement"
            );
        }
        self.validate_control()
            .context("control")?;
        Ok(())
    }

    /// The control section's own rules. Every refusal names its field, like
    /// the vote_kick ones above — a server that starts with a knob it
    /// silently clamped is a server running an arrangement the operator did
    /// not configure.
    fn validate_control(&self) -> Result<()> {
        let c = &self.control;
        if c.enabled {
            if c.port == 0 {
                anyhow::bail!("control.port must be a fixed port, not 0 — ctl reads the \
                               port from this file and cannot discover a random one");
            }
            if c.max_connections == 0 {
                anyhow::bail!("control.max_connections must be at least 1");
            }
            if c.max_connections > 32 {
                anyhow::bail!(
                    "control.max_connections {} exceeds the plan's ceiling of 32; \
                     the door serves a panel and ctl, not a fleet",
                    c.max_connections
                );
            }
            if c.max_line_bytes < 1024 {
                anyhow::bail!("control.max_line_bytes must be at least 1024 — a smaller \
                               cap breaks the `perf:` line's own length on its way in");
            }
            if c.max_line_bytes > 4 * 1024 * 1024 {
                anyhow::bail!("control.max_line_bytes {} exceeds 4 MiB", c.max_line_bytes);
            }
            if c.commands_per_second == 0 {
                anyhow::bail!("control.commands_per_second must be at least 1");
            }
            if c.max_bad_tokens == 0 {
                anyhow::bail!("control.max_bad_tokens must be at least 1");
            }
            if c.bad_token_cooldown_s == 0 {
                anyhow::bail!("control.bad_token_cooldown_s must be at least 1");
            }
            if let Some(file) = &c.token_file {
                if file.trim().is_empty() {
                    anyhow::bail!("control.token_file must name a file, not a blank string");
                }
            }
            if let Some(file) = &c.action_log_file {
                if file.trim().is_empty() {
                    anyhow::bail!("control.action_log_file must name a file, not a blank string");
                }
            }
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
        // The fixture names a token (bug №216): a file that predates the auth
        // fields loads blank and is refused at that door by design — the
        // fail-closed contract — so this legacy-format test carries it.
        let text = r#"{"vote_kick": {"enabled": false, "min_votes": 3, "majority": 0.5, "window_ticks": 10}, "auth_token": "station-secret", "future_section": {}}"#;
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

    /// Bug №216: a blank secret with no explicit opt-out refuses to start —
    /// that exact combination was the №700 hole running live. The explicit
    /// `allow_unsigned = true` is the operator's deliberate answer and is
    /// honoured; a named secret needs no opt-out at all.
    #[test]
    fn a_blank_token_is_refused_unless_unsigned_is_explicit() {
        let blank = MatchConfig::default();
        assert!(
            blank.validate().is_err(),
            "a blank secret without the opt-out must refuse to start"
        );

        let explicit = MatchConfig {
            allow_unsigned: true,
            ..MatchConfig::default()
        };
        assert!(
            explicit.validate().is_ok(),
            "the explicit unsigned dialect is the operator's choice"
        );

        let keyed = MatchConfig {
            auth_token: Some("station-secret".to_string()),
            allow_unsigned: false,
            ..MatchConfig::default()
        };
        assert!(keyed.validate().is_ok(), "a named secret courts no opt-out");
    }

    // ---- the control section (stage 1, Приложение А) ----

    /// Defaults are legal: a file without a `control` section starts the door
    /// on 7778, and the defaults survive a validate round.
    #[test]
    fn the_control_defaults_are_valid_and_the_door_is_on() {
        let config = MatchConfig {
            allow_unsigned: true,
            ..MatchConfig::default()
        };
        config.validate().expect("the control defaults are valid");
        assert!(config.control.enabled);
        assert_eq!(config.control.port, 7778);
        assert_eq!(config.control.max_connections, 4);
        assert_eq!(config.control.token_file, None);
    }

    /// An old file that never mentioned `control` loads into the same
    /// defaults — the forward-compat rule the config module promises for
    /// every new section.
    #[test]
    fn a_file_without_the_control_section_still_loads() {
        let dir = temp_dir("no-control");
        let path = dir.join("server_config.json");
        let text = r#"{"vote_kick": {"enabled": false, "min_votes": 2, "majority": 0.5, "window_ticks": 10}, "allow_unsigned": true}"#;
        std::fs::write(&path, text).expect("fixture file is writable");
        let config = MatchConfig::load(&path).expect("an old file loads");
        assert_eq!(config.control, ControlConfig::default());
    }

    /// Every knob's nonsense value is refused with the field named — the
    /// module's own rule, applied to the new section one field at a time.
    #[test]
    fn unusable_control_values_are_refused_with_the_field_named() {
        let cases: Vec<(&str, ControlConfig)> = vec![
            (
                "control.port",
                ControlConfig { port: 0, ..ControlConfig::default() },
            ),
            (
                "control.max_connections",
                ControlConfig { max_connections: 0, ..ControlConfig::default() },
            ),
            (
                "control.max_connections",
                ControlConfig { max_connections: 33, ..ControlConfig::default() },
            ),
            (
                "control.max_line_bytes",
                ControlConfig { max_line_bytes: 512, ..ControlConfig::default() },
            ),
            (
                "control.commands_per_second",
                ControlConfig { commands_per_second: 0, ..ControlConfig::default() },
            ),
            (
                "control.max_bad_tokens",
                ControlConfig { max_bad_tokens: 0, ..ControlConfig::default() },
            ),
            (
                "control.bad_token_cooldown_s",
                ControlConfig { bad_token_cooldown_s: 0, ..ControlConfig::default() },
            ),
            (
                "control.token_file",
                ControlConfig { token_file: Some("   ".into()), ..ControlConfig::default() },
            ),
            // Stage 2 (А.2 item 6): a journal the operator named but did not
            // name a file for is the same nonsense any other blank field is.
            (
                "control.action_log_file",
                ControlConfig { action_log_file: Some("   ".into()), ..ControlConfig::default() },
            ),
        ];
        for (field, control) in cases {
            let config = MatchConfig {
                allow_unsigned: true,
                control,
                ..MatchConfig::default()
            };
            let err = config.validate().expect_err(&format!("{field} must refuse"));
            assert!(
                format!("{err:#}").contains(field),
                "the refusal must name {field}: {err:#}"
            );
        }
    }

    /// Disabled is simply off: every knob may be nonsense, nothing is
    /// validated, nothing is bound. A refused disabled section would make
    /// the off switch unusable with a stale file.
    #[test]
    fn a_disabled_control_section_validates_regardless_of_its_knobs() {
        let config = MatchConfig {
            allow_unsigned: true,
            control: ControlConfig {
                enabled: false,
                port: 0,
                max_connections: 0,
                ..ControlConfig::default()
            },
            ..MatchConfig::default()
        };
        config.validate().expect("off is off");
    }

    /// The written default file carries the section — the first-run file an
    /// operator opens names the knobs without a plan document open.
    #[test]
    fn the_created_default_file_contains_the_control_section() {
        let dir = temp_dir("default-control");
        let path = dir.join("server_config.json");
        MatchConfig::load(&path).expect("first run creates the file");
        let text = std::fs::read_to_string(&path).expect("the file was written");
        assert!(
            text.contains("\"control\""),
            "the default file must expose the section: {text}"
        );
    }
}
