//! Re-export of the external `rhi` crate so the client's `crate::rhi::*` paths resolve.
//!
//! The physical crate lives at `src/rhi/` (crate root = `src/rhi/lib.rs`, its own
//! `Cargo.toml`). This shim module simply re-exports it, keeping `pub mod rhi;`
//! in `src/lib.rs` working without re-compiling the crate as a submodule.

pub use ::rhi::*;