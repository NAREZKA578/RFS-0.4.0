//! Симуляция движка.
//!
//! Доноры: `RFS-0.3/rfs-server/.../connection.rs:191-235` (reconnect),
//! `server/mod.rs:1473-1547` (disconnect), `fire_sim.rs:483-523` (carry/drop),
//! `screens/server_browser.rs:230-241` (favorites).
//!
//! Исправления:
//! - №221: единый `SessionStore::remove_session` для disconnect И reconnect.
//! - №224: `persist::atomic_write` вместо прямого `fs::write`.

pub mod persist;
pub mod session;

pub use persist::atomic_write;
pub use session::{BroadcastEvents, SessionStore};
