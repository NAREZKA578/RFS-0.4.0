use rhi::error::{RhiError, RhiResult};
use std::error::Error;

#[test]
fn error_display_not_empty() {
    let err = RhiError::NoBackendForApi("Vulkan".into());
    let msg = err.to_string();
    assert!(!msg.is_empty());

    let err = RhiError::BackendError("something failed".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::NoPhysicalDevices;
    assert!(!err.to_string().is_empty());

    let err = RhiError::InvalidPhysicalDeviceIndex(5);
    assert!(!err.to_string().is_empty());

    let err = RhiError::DeviceCreationError("failed".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::DeviceLost;
    assert!(!err.to_string().is_empty());

    let err = RhiError::BufferCreationError("bad size".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::TextureCreationError("bad format".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::SamplerCreationError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::PipelineCreationError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::ShaderCompilationError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::CommandBufferError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::CommandBufferInvalidState;
    assert!(!err.to_string().is_empty());

    let err = RhiError::FenceError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::SemaphoreError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::FenceTimeout;
    assert!(!err.to_string().is_empty());

    let err = RhiError::SwapChainError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::OutOfMemory;
    assert!(!err.to_string().is_empty());

    let err = RhiError::ValidationError("bad".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::NotSupported("feature".into());
    assert!(!err.to_string().is_empty());

    let err = RhiError::InternalError("bug".into());
    assert!(!err.to_string().is_empty());
}

#[test]
fn error_trait_implemented() {
    let err: Box<dyn Error> = Box::new(RhiError::DeviceLost);
    assert!(!err.to_string().is_empty());
    assert!(err.source().is_none());
}

#[test]
fn rhi_result_ok() {
    let r: RhiResult<i32> = Ok(42);
    assert!(r.is_ok());
    assert!(matches!(r, Ok(42)));
}

#[test]
fn rhi_result_err() {
    let r: RhiResult<i32> = Err(RhiError::OutOfMemory);
    assert!(r.is_err());
}

#[test]
fn error_debug() {
    let err = RhiError::InternalError("test".into());
    let debug = format!("{:?}", err);
    assert!(!debug.is_empty());
}
