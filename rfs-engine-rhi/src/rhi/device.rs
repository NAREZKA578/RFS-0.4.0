//! Graphics device abstraction.

use super::buffer::{Buffer, BufferDescriptor};
use super::command::CommandEncoder;
use super::pipeline::{Pipeline, PipelineDescriptor};
use super::shader::{Shader, ShaderDescriptor};
use super::sync::Fence;
use super::texture::{Texture, TextureDescriptor};

/// Device descriptor for creating a graphics device.
#[derive(Debug, Clone, Default)]
pub struct DeviceDescriptor {
    /// Maximum number of frames in flight.
    pub max_frames_in_flight: u32,
    /// Whether to enable debug markers.
    pub enable_debug_markers: bool,
}

/// Graphics device trait — implemented by each backend.
pub trait Device: Send + Sync {
    /// Creates a buffer.
    fn create_buffer(
        &self,
        descriptor: &BufferDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Box<dyn Buffer>, Box<dyn std::error::Error>>;

    /// Creates a texture.
    fn create_texture(
        &self,
        descriptor: &TextureDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Box<dyn Texture>, Box<dyn std::error::Error>>;

    /// Creates a shader.
    fn create_shader(
        &self,
        descriptor: &ShaderDescriptor,
    ) -> Result<Box<dyn Shader>, Box<dyn std::error::Error>>;

    /// Creates a pipeline.
    fn create_pipeline(
        &self,
        descriptor: &PipelineDescriptor,
    ) -> Result<Box<dyn Pipeline>, Box<dyn std::error::Error>>;

    /// Creates a command encoder.
    fn create_command_encoder(&self)
        -> Result<Box<dyn CommandEncoder>, Box<dyn std::error::Error>>;

    /// Submits command buffers for execution.
    fn submit(
        &self,
        commands: &[&dyn CommandEncoder],
        fence: Option<&mut Fence>,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Creates a fence for synchronization.
    fn create_fence(&self, signaled: bool) -> Result<Fence, Box<dyn std::error::Error>>;

    /// Waits for a fence to be signaled.
    fn wait_for_fence(
        &self,
        fence: &Fence,
        timeout_ns: u64,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Destroys a fence.
    fn destroy_fence(&self, fence: Fence);

    /// Waits for all pending GPU work to complete.
    fn wait_idle(&self) -> Result<(), Box<dyn std::error::Error>>;

    /// Returns the backend type.
    fn backend_type(&self) -> super::backend::BackendType;

    /// Returns the GPU info.
    fn gpu_info(&self) -> super::backend::GpuInfo;

    /// Clears the default (window) framebuffer. Backends that cannot clear
    /// outside of a render pass should no-op or warn.
    fn clear(&self, color: [f32; 4], depth: f32) {
        let _ = (color, depth);
        tracing::warn!(
            "Device::clear is not implemented for backend {:?}",
            self.backend_type()
        );
    }
}
