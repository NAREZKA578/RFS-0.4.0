//! Synchronization Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

pub mod barrier;
pub mod fence;
pub mod semaphore;

pub use barrier::*;
pub use fence::*;
pub use semaphore::*;
