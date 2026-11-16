//! # RHI - Rendering Hardware Interface
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! A hardware abstraction for 3D rendering. Windows-only.
//!
//! ## Supported APIs (the honest list, bug №796)
//! - **Vulkan 1.3** — the only backend that renders: instance, device,
//!   memory, present, command recording, pipeline cache.
//! - **Direct3D 12 / Direct3D 11** — skeletons; creation returns
//!   `NotSupported`.
//! - **OpenGL** — stub, slated for removal by owner decision (bug №719).
//!
//! ## Features
//! - Memory management (GPU allocators, mapped buffers)
//! - Debug tools (validation layer hookup, markers, capture)
//! - Persistent pipeline cache (bugs №787-№788)

#![allow(missing_docs)]
#![allow(clippy::needless_range_loop)]

pub mod backend;
pub mod command;
pub mod config;
pub mod core;
pub mod debug;
pub mod descriptor;

// The backend hands native handles (image views, present modes, extents) to
// callers as part of its public API, so `ash` has to be nameable outside the
// crate. Re-exported rather than redeclared, so there is one `ash` in the
// dependency graph.
pub use ash;
pub mod error;
pub mod memory;
pub mod pipeline;
pub mod present;
pub mod query;
pub mod resource;
pub mod shader;
pub mod swapchain;
pub mod sync;
pub mod types;

/// The 2D user interface: immediate and retained element trees, text layout,
/// input handling, and the renderer that turns them into draw calls.
///
/// This declaration was missing, which meant the whole module was never
/// compiled: `rhi` had 59 tests and zero UI tests, and no UI pixel could reach
/// the screen regardless of how correct the drawing code was. Worth stating
/// plainly, because "the pipelines are unit-tested" is exactly the kind of claim
/// that survives a module quietly falling out of the build.
pub mod ui;

pub mod utils;

#[allow(ambiguous_glob_reexports)]
pub use config::*;
#[allow(ambiguous_glob_reexports)]
pub use core::*;
pub use error::*;
#[allow(ambiguous_glob_reexports)]
pub use types::*;

// Re-export for convenience
pub use backend::Backend;

/// Re-exported from the Vulkan backend so tests and the debug overlay can read the
/// validation error count without depending on which backend is active. Bug №292.
pub use backend::vulkan::validation_error_count;

/// Whether the validation layer is genuinely loaded. Bug №297.
///
/// Pairs with [`validation_error_count`]: a zero error count is only meaningful
/// when this is `true`.
pub use backend::vulkan::validation_layer_active;
#[allow(ambiguous_glob_reexports)]
pub use command::*;
#[allow(ambiguous_glob_reexports)]
pub use descriptor::*;
#[allow(ambiguous_glob_reexports)]
pub use memory::*;
#[allow(ambiguous_glob_reexports)]
pub use pipeline::*;
#[allow(ambiguous_glob_reexports)]
pub use query::*;
#[allow(ambiguous_glob_reexports)]
pub use resource::*;
#[allow(ambiguous_glob_reexports)]
pub use shader::*;
#[allow(ambiguous_glob_reexports)]
pub use swapchain::*;
#[allow(ambiguous_glob_reexports)]
pub use sync::*;
