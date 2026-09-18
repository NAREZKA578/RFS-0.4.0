// Integration tests for the rhi::debug module (accessible at rhi::debug::*).

use rhi::debug::capture::{CaptureContext, FrameCapture};
use rhi::debug::markers::{DebugLabel, DebugMarker, DebugUtils};
use rhi::debug::stats::{FrameStats, GpuStats, PipelineStats};
use rhi::debug::validation::{ValidationError, ValidationErrorType};
use rhi::config::ValidationSeverity;

#[test]
fn gpu_stats_default_and_fields() {
    let stats = GpuStats::default();
    assert_eq!(stats.frame_count, 0);
    assert_eq!(stats.draw_calls, 0);
    assert_eq!(stats.triangles, 0);
    assert_eq!(stats.gpu_time_ns, 0);
}

#[test]
fn frame_stats_default_and_fields() {
    let stats = FrameStats::default();
    assert_eq!(stats.frame_index, 0);
    assert_eq!(stats.draw_calls, 0);
    assert_eq!(stats.vertices, 0);
}

#[test]
fn pipeline_stats_default_and_fields() {
    let stats = PipelineStats::default();
    assert_eq!(stats.invocations, 0);
}

#[test]
fn debug_label_new() {
    let label = DebugLabel::new("gbuffer".to_string(), [1.0, 0.5, 0.25, 1.0]);
    assert_eq!(label.label, "gbuffer");
    assert_eq!(label.color[0], 1.0);
    assert_eq!(label.color[3], 1.0);
}

#[test]
fn debug_marker_new() {
    let marker = DebugMarker::new("begin_pass".to_string());
    assert_eq!(marker.marker, "begin_pass");
}

#[test]
fn debug_utils_new() {
    let utils = DebugUtils::new(true, true);
    assert!(utils.enable_validation);
    assert!(utils.enable_debug_markers);
    let utils = DebugUtils::new(false, false);
    assert!(!utils.enable_validation);
}

#[test]
fn frame_capture_new() {
    let capture = FrameCapture::new();
    assert!(capture.commands.is_empty());
    assert_eq!(capture.stats.draw_calls, 0);
}

#[test]
fn capture_context_state_machine() {
    let mut ctx = CaptureContext::new();
    assert!(!ctx.is_capturing);
    assert_eq!(ctx.frame_index, 0);
    assert_eq!(ctx.max_frames, 0);

    ctx.start_capture(120);
    assert!(ctx.is_capturing);
    assert_eq!(ctx.max_frames, 120);
    assert_eq!(ctx.frame_index, 0);

    ctx.stop_capture();
    assert!(!ctx.is_capturing);
}

#[test]
fn validation_error_new() {
    let err = ValidationError::new(
        "out of memory".to_string(),
        ValidationSeverity::Error,
        ValidationErrorType::Memory,
    );
    assert_eq!(err.message, "out of memory");
    assert_eq!(err.ty, ValidationErrorType::Memory);
    assert_eq!(err.severity, ValidationSeverity::Error);
}

#[test]
fn validation_error_type_variants() {
    let _ = ValidationErrorType::General;
    let _ = ValidationErrorType::Device;
    let _ = ValidationErrorType::Pipeline;
    let _ = ValidationErrorType::Descriptor;
    let _ = ValidationErrorType::Command;
    let _ = ValidationErrorType::Shader;
    let _ = ValidationErrorType::SwapChain;
    let _ = ValidationErrorType::Sync;
}

#[test]
fn validation_severity_variants() {
    let _ = ValidationSeverity::Info;
    let _ = ValidationSeverity::Warning;
    let _ = ValidationSeverity::Error;
    let _ = ValidationSeverity::Verbose;
}