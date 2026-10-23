//! Direct3D 12 Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod convert;
mod device;
mod instance;

pub use instance::D3D12Backend;
