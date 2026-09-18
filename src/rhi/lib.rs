//! # RHI - Rendering Hardware Interface
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! This module provides a **cross-platform, high-performance** abstraction
//! over modern graphics APIs for 3D rendering in client applications.
//!
//! ## Supported APIs
//! - **Vulkan 1.3** (Primary)
//! - **Direct3D 12** (Windows)
//! - **Direct3D 11** (Legacy, Windows)
//! - **OpenGL 4.6** (Legacy, cross-platform)
//!
//! ## Features
//! - Cross-platform (Windows, Linux)
//! - Modern rendering (Ray Tracing, Mesh Shading, VRS, Bindless)
//! - High performance (minimal abstraction overhead)
//! - Thread-safe (multi-threaded command recording)
//! - Memory management (GPU allocators)
//! - Debug tools (validation, markers, capture)

#![allow(missing_docs)]
#![allow(dead_code)]
#![allow(clippy::needless_range_loop)]

pub mod backend;
pub mod command;
pub mod config;
pub mod core;
pub mod debug;
pub mod descriptor;
pub mod error;
pub mod memory;
pub mod pipeline;
pub mod query;
pub mod resource;
pub mod shader;
pub mod swapchain;
pub mod sync;
pub mod types;
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
