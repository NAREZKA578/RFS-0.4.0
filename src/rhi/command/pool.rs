//! Command Pool
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::buffer::{CommandBuffer, CommandBufferDesc, CommandBufferFlags, CommandBufferLevel};
use bitflags::bitflags;

// Command pool flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CommandPoolFlags: u32 {
        const NONE = 0;
        const TRANSIENT = 1 << 0;
        const RESET_COMMAND_BUFFER = 1 << 1;
    }
}

/// Command pool description
#[derive(Debug, Clone, Default)]
pub struct CommandPoolDesc {
    pub queue_family_index: u32,
    pub flags: CommandPoolFlags,
}

/// Command pool
#[derive(Debug, Clone, Default)]
pub struct CommandPool {
    desc: CommandPoolDesc,
    queue_family_index: u32,
}

impl CommandPool {
    pub fn new(desc: CommandPoolDesc) -> Self {
        Self {
            desc: desc.clone(),
            queue_family_index: desc.queue_family_index,
        }
    }

    pub fn desc(&self) -> &CommandPoolDesc {
        &self.desc
    }
    pub fn queue_family_index(&self) -> u32 {
        self.queue_family_index
    }

    /// Allocates a primary command buffer from this pool.
    pub fn allocate(&self) -> CommandBuffer {
        self.allocate_level(CommandBufferLevel::Primary)
    }

    /// Allocates a command buffer of the given level.
    pub fn allocate_level(&self, level: CommandBufferLevel) -> CommandBuffer {
        CommandBuffer::new(
            self.clone(),
            CommandBufferDesc {
                level,
                queue_family_index: Some(self.queue_family_index),
                flags: CommandBufferFlags::default(),
            },
        )
    }
}
