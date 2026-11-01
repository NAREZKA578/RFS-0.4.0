//! Direct3D 12 Backend
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod convert;
mod descriptor;
mod device;
mod instance;
mod record;

pub use instance::D3D12Backend;
