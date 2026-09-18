//! Validation
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::config::ValidationSeverity;

/// Validation error type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValidationErrorType {
    General,
    Device,
    Pipeline,
    Descriptor,
    Command,
    Memory,
    Shader,
    SwapChain,
    Sync,
}

/// Validation error
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub message: String,
    pub severity: ValidationSeverity,
    pub ty: ValidationErrorType,
}

impl ValidationError {
    pub fn new(message: String, severity: ValidationSeverity, ty: ValidationErrorType) -> Self {
        Self {
            message,
            severity,
            ty,
        }
    }
}
