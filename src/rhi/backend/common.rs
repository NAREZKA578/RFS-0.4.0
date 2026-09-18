//! Common Backend Utilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::types::GraphicsApi;

/// Trait for backend resources
pub trait BackendResource {
    fn as_raw(&self) -> *const ();
    fn backend_type(&self) -> GraphicsApi;
}
