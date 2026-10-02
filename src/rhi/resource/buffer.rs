//! Buffer Resources
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::types::*;
use crate::utils::alignment::{STORAGE_BUFFER_ALIGNMENT, UNIFORM_BUFFER_ALIGNMENT};

use super::GpuResource;

/// Buffer description
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BufferDesc {
    pub size: u64,
    pub usage: BufferUsage,
    pub memory_flags: MemoryPropertyFlags,
    pub sharing_mode: SharingMode,
    pub queue_family_indices: Vec<u32>,
}

impl BufferDesc {
    /// Returns `true` when the description is valid and may be used to
    /// create a buffer.
    pub fn is_valid(&self) -> bool {
        self.size > 0
            && !self.usage.is_empty()
            && !self.memory_flags.is_empty()
            && match self.sharing_mode {
                SharingMode::Exclusive => true,
                SharingMode::Concurrent => !self.queue_family_indices.is_empty(),
            }
    }

    /// Returns `true` when the description allows host-side data mapping.
    pub fn is_host_mappable(&self) -> bool {
        self.memory_flags.contains(MemoryPropertyFlags::HOST_VISIBLE)
    }

    /// Returns the minimum byte offset alignment this buffer's bindings need.
    ///
    /// Bug №222: the old table was inverted relative to the constants declared
    /// in `utils::alignment` — it answered 256 for STORAGE and 64 for UNIFORM,
    /// while `UNIFORM_BUFFER_ALIGNMENT` is 256 and `STORAGE_BUFFER_ALIGNMENT`
    /// is 16. A shader-written buffer therefore got *less* alignment than the
    /// spec requires, which is undefined behaviour on the device rather than
    /// merely conservative.
    ///
    /// The original `usage.contains(VERTEX | INDEX)` also required *both* bits,
    /// so a vertex-only buffer fell through to the 4-byte default. The vertex
    /// and index cases are now separate.
    ///
    /// When several usage bits apply the strictest requirement wins: a buffer
    /// that is both a uniform and a storage buffer must satisfy the uniform
    /// alignment.
    pub fn alignment(&self) -> u64 {
        // Strictest first.
        if self.usage.contains(BufferUsage::UNIFORM) {
            UNIFORM_BUFFER_ALIGNMENT
        } else if self.usage.contains(BufferUsage::STORAGE) {
            STORAGE_BUFFER_ALIGNMENT
        } else if self.usage.contains(BufferUsage::VERTEX) || self.usage.contains(BufferUsage::INDEX)
        {
            // Vertex/index offsets only need natural alignment; Vulkan places
            // no requirement beyond the format's own component size.
            4
        } else {
            1
        }
    }
}

/// Buffer resource
#[derive(Debug, Clone, Default)]
pub struct Buffer {
    desc: BufferDesc,
    device_address: Option<u64>,
    pub(crate) backend: Option<GpuResource>,
}

impl Buffer {
    pub fn new(desc: BufferDesc) -> Self {
        Self {
            desc,
            device_address: None,
            backend: None,
        }
    }

    pub fn from_desc(desc: BufferDesc) -> Self {
        Self::new(desc)
    }

    pub fn desc(&self) -> &BufferDesc {
        &self.desc
    }
    pub fn size(&self) -> u64 {
        self.desc.size
    }

    /// Returns `true` when the buffer may be used as a vertex buffer.
    pub fn is_vertex(&self) -> bool {
        self.desc.usage.contains(BufferUsage::VERTEX)
    }

    /// Returns `true` when the buffer may be used as an index buffer.
    pub fn is_index(&self) -> bool {
        self.desc.usage.contains(BufferUsage::INDEX)
    }

    /// Returns `true` when the buffer may be used as a uniform buffer.
    pub fn is_uniform(&self) -> bool {
        self.desc.usage.contains(BufferUsage::UNIFORM)
    }

    /// Returns `true` when the buffer may be used as a storage buffer.
    pub fn is_storage(&self) -> bool {
        self.desc.usage.contains(BufferUsage::STORAGE)
    }

    /// Returns `true` when the buffer may be used as an indirect argument buffer.
    pub fn is_indirect(&self) -> bool {
        self.desc.usage.contains(BufferUsage::INDIRECT)
    }

    /// Returns `true` when the buffer can be read from or written to by
    /// transfer commands.
    pub fn supports_transfer(&self) -> bool {
        self.desc
            .usage
            .contains(BufferUsage::TRANSFER_SRC)
            || self
                .desc
                .usage
                .contains(BufferUsage::TRANSFER_DST)
    }

    /// Returns `true` when the buffer memory is host-visible.
    pub fn is_host_visible(&self) -> bool {
        self.desc.is_host_mappable()
    }

    /// Returns the device address assigned to this buffer, if any.
    pub fn device_address(&self) -> Option<u64> {
        self.device_address
    }

    /// Assigns a device address to this buffer.
    pub fn set_device_address(&mut self, address: u64) {
        self.device_address = Some(address);
    }

    /// Returns the native backend handle attached by the active backend, if any.
    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    /// Returns `true` when this buffer is backed by native GPU memory.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }

    /// Attaches a native backend handle to this buffer.
    pub(crate) fn set_backend(&mut self, resource: GpuResource) {
        self.backend = Some(resource);
    }
}