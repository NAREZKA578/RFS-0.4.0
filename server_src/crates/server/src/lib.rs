//! The server as a library (plan H5, «Server structure»): the match
//! orchestration is split into modules so tools and tests can drive the same
//! code the binary runs, instead of each tool reimplementing what a player, a
//! tracked ship or a tick loop is.
//!
//! - `match_` — match sizing, world loading, ship/player bookkeeping.
//! - `tick` — input application, projectile spawning, event translation.
//! - `net` — the command plane with its refusal reasons.
//! - `config` — the server config file (created on first start, read at start).
//! - `votes` — the config-driven VoteKick machine and its glue.
//! - `control` — loop housekeeping (tick cadence accounting).
pub mod config;
pub mod control;
pub mod match_;
pub mod net;
pub mod tick;
pub mod votes;
