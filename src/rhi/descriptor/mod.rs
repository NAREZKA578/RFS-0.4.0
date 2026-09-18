//! Descriptor Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

pub mod allocator;
pub mod binding;
pub mod layout;
pub mod set;

pub use allocator::*;
pub use binding::*;
pub use layout::*;
pub use set::*;
