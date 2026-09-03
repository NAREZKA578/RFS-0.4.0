//! Сеть движка.
//!
//! Доноры: `RFS-0.3/src/net/handler.rs` + `snapshot.rs` (мерж),
//! `src/game/interpolation.rs` (буфер), `src/app/update.rs:631-646,660-691`
//! (дренаж событий и подпитка интерполятора), `rfs-server/.../mod.rs:1762`
//! (санитайз чата).
//!
//! Исправления относительно донора (BUGS.md §9):
//! - №219: дельта только мержит, пустое ничего не стирает.
//! - №220: интерполятор кормится и снапшотами, и дельтами.
//! - №222: очередь событий возвращает данные, а не топит их в лог.
//! - №223: лимиты чата — из конфига, хардкода нет.

pub mod events;
pub mod interp;
pub mod limits;
pub mod snapshot;

pub use events::EventQueue;
pub use interp::InterpolationBuffer;
pub use limits::{sanitize_chat_msg, ChatLimits};
pub use snapshot::{Delta, SnapshotView};
