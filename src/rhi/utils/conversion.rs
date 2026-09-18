//! Format Conversion Utilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::types::Format;

/// Format conversion trait
pub trait FormatConversion {
    fn to_vulkan(&self) -> u32;
    fn to_dxgi(&self) -> u32;
    fn to_gl(&self) -> u32;
}

impl FormatConversion for Format {
    fn to_vulkan(&self) -> u32 {
        unimplemented!()
    }

    fn to_dxgi(&self) -> u32 {
        unimplemented!()
    }

    fn to_gl(&self) -> u32 {
        unimplemented!()
    }
}
