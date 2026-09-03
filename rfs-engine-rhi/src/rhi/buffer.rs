//! Buffer abstraction (vertex, index, uniform buffers).

use super::types::VertexLayout;

/// Buffer usage flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BufferUsage {
    /// Vertex buffer.
    Vertex,
    /// Index buffer.
    Index,
    /// Uniform buffer.
    Uniform,
    /// Storage buffer.
    Storage,
    /// Transfer source.
    TransferSrc,
    /// Transfer destination.
    TransferDst,
    /// Indirect commands.
    Indirect,
}

impl BufferUsage {
    /// Returns a combined usage flags value (for OpenGL compatibility).
    pub fn flags(&self) -> u32 {
        match self {
            Self::Vertex => 1,
            Self::Index => 2,
            Self::Uniform => 4,
            Self::Storage => 8,
            Self::TransferSrc => 16,
            Self::TransferDst => 32,
            Self::Indirect => 64,
        }
    }
}

/// Buffer descriptor.
#[derive(Debug, Clone)]
pub struct BufferDescriptor {
    /// Size of the buffer in bytes.
    pub size: u64,
    /// Buffer usage.
    pub usage: BufferUsage,
    /// Whether the buffer can be updated dynamically.
    pub dynamic: bool,
    /// Optional vertex layout (for vertex buffers).
    pub vertex_layout: Option<VertexLayout>,
}

impl BufferDescriptor {
    pub fn new(size: u64, usage: BufferUsage) -> Self {
        Self {
            size,
            usage,
            dynamic: false,
            vertex_layout: None,
        }
    }

    pub fn vertex(size: u64, layout: VertexLayout) -> Self {
        Self {
            size,
            usage: BufferUsage::Vertex,
            dynamic: false,
            vertex_layout: Some(layout),
        }
    }

    pub fn index(size: u64) -> Self {
        Self {
            size,
            usage: BufferUsage::Index,
            dynamic: false,
            vertex_layout: None,
        }
    }

    pub fn uniform(size: u64) -> Self {
        Self {
            size,
            usage: BufferUsage::Uniform,
            dynamic: true,
            vertex_layout: None,
        }
    }
}

/// Buffer trait — implemented by each backend.
pub trait Buffer: Send + Sync {
    /// Returns the size of the buffer in bytes.
    fn size(&self) -> u64;

    /// Returns the usage of the buffer.
    fn usage(&self) -> BufferUsage;

    /// Updates the buffer data.
    fn update(&self, data: &[u8], offset: u64) -> Result<(), Box<dyn std::error::Error>>;

    /// Maps the buffer for CPU read/write access.
    fn map(&mut self) -> Result<&mut [u8], Box<dyn std::error::Error>>;

    /// Unmaps the buffer.
    fn unmap(&mut self) -> Result<(), Box<dyn std::error::Error>>;

    /// Returns the backend-specific handle.
    fn native_handle(&self) -> u64;
}
