//! Error Handling
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use thiserror::Error;

/// Result type for RHI operations
pub type RhiResult<T> = Result<T, RhiError>;

/// Main error type for RHI
#[derive(Debug, Error)]
pub enum RhiError {
    // Backend
    #[error("No backend for API: {0}")]
    NoBackendForApi(String),

    #[error("Backend error: {0}")]
    BackendError(String),

    // Device
    #[error("No physical devices found")]
    NoPhysicalDevices,

    #[error("Invalid physical device index: {0}")]
    InvalidPhysicalDeviceIndex(usize),

    #[error("Device creation failed: {0}")]
    DeviceCreationError(String),

    #[error("Device lost")]
    DeviceLost,

    // Resources
    #[error("Buffer creation failed: {0}")]
    BufferCreationError(String),

    #[error("Texture creation failed: {0}")]
    TextureCreationError(String),

    #[error("Sampler creation failed: {0}")]
    SamplerCreationError(String),

    // Pipeline
    #[error("Pipeline creation failed: {0}")]
    PipelineCreationError(String),

    #[error("Shader compilation failed: {0}")]
    ShaderCompilationError(String),

    // Commands
    #[error("Command buffer error: {0}")]
    CommandBufferError(String),

    #[error("Command buffer not in valid state")]
    CommandBufferInvalidState,

    // Sync
    #[error("Fence error: {0}")]
    FenceError(String),

    #[error("Semaphore error: {0}")]
    SemaphoreError(String),

    #[error("Timeout waiting for fence")]
    FenceTimeout,

    // SwapChain
    #[error("SwapChain error: {0}")]
    SwapChainError(String),

    // Memory
    #[error("Out of memory")]
    OutOfMemory,

    // Validation
    #[error("Validation error: {0}")]
    ValidationError(String),

    // General
    #[error("Not supported: {0}")]
    NotSupported(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}
