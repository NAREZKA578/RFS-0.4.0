//! Debug Markers
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

/// Debug label
#[derive(Debug, Clone)]
pub struct DebugLabel {
    pub label: String,
    pub color: [f32; 4],
}

impl DebugLabel {
    pub fn new(label: String, color: [f32; 4]) -> Self {
        Self { label, color }
    }
}

/// Debug marker
#[derive(Debug, Clone)]
pub struct DebugMarker {
    pub marker: String,
}

impl DebugMarker {
    pub fn new(marker: String) -> Self {
        Self { marker }
    }
}

/// Debug utils
pub struct DebugUtils {
    pub enable_validation: bool,
    pub enable_debug_markers: bool,
}

impl DebugUtils {
    pub fn new(enable_validation: bool, enable_debug_markers: bool) -> Self {
        Self {
            enable_validation,
            enable_debug_markers,
        }
    }

    /// Returns `true` when markers should be submitted to the backend.
    pub fn markers_enabled(&self) -> bool {
        self.enable_debug_markers
    }

    /// Returns `true` when validation layers are enabled.
    pub fn validation_enabled(&self) -> bool {
        self.enable_validation
    }
}
