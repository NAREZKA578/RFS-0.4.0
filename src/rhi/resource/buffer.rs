//! Buffer Resources
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::types::*;

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

    /// Returns the alignment needed for descriptors on this buffer, in bytes.
    pub fn alignment(&self) -> u64 {
        if self.usage.contains(BufferUsage::STORAGE) {
            256
        } else if self.usage.contains(BufferUsage::UNIFORM) {
            64
        } else if self.usage.contains(BufferUsage::VERTEX | BufferUsage::INDEX) {
            16
        } else {
            4
        }
    }
}

/// Buffer resource
#[derive(Debug, Clone, Default)]
pub struct Buffer {
    desc: BufferDesc,
    device_address: Option<u64>,
}

impl Buffer {
    pub fn new(desc: BufferDesc) -> Self {
        Self {
            desc,
            device_address: None,
        }
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
            .contains(BufferUsage::TRANSFER_SRC | BufferUsage::TRANSFER_DST)
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
}