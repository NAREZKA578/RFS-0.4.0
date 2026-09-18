//! Command Buffer
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::pool::CommandPool;
use bitflags::bitflags;

/// Command buffer level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CommandBufferLevel {
    #[default]
    Primary,
    Secondary,
}

/// Command buffer state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CommandBufferState {
    Initial,
    Recording,
    Executable,
    Pending,
    Invalid,
}

// Command buffer flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct CommandBufferFlags: u32 {
        const NONE = 0;
        const ONE_TIME_SUBMIT = 1 << 0;
        const RENDER_PASS_CONTINUE = 1 << 1;
        const SIMULTANEOUS_USE = 1 << 2;
    }
}

/// Command buffer description
#[derive(Debug, Clone, Default)]
pub struct CommandBufferDesc {
    pub level: CommandBufferLevel,
    pub queue_family_index: Option<u32>,
    pub flags: CommandBufferFlags,
}

/// Command buffer
pub struct CommandBuffer {
    desc: CommandBufferDesc,
    pool: CommandPool,
    state: CommandBufferState,
}

impl CommandBuffer {
    pub fn new(pool: CommandPool, desc: CommandBufferDesc) -> Self {
        Self {
            desc,
            pool,
            state: CommandBufferState::Initial,
        }
    }

    pub fn desc(&self) -> &CommandBufferDesc {
        &self.desc
    }
    pub fn pool(&self) -> &CommandPool {
        &self.pool
    }
    pub fn state(&self) -> CommandBufferState {
        self.state
    }

    /// Returns `true` if the buffer is being recorded.
    pub fn is_recording(&self) -> bool {
        self.state == CommandBufferState::Recording
    }

    /// Returns `true` if the buffer is ready to be submitted.
    pub fn is_executable(&self) -> bool {
        self.state == CommandBufferState::Executable
    }

    /// Starts recording. No-op when already recording.
    pub fn begin(&mut self) {
        if self.state == CommandBufferState::Initial || self.state == CommandBufferState::Executable {
            self.state = CommandBufferState::Recording;
        }
    }

    /// Stops recording. Panics when called before `begin`.
    pub fn end(&mut self) {
        assert!(
            self.is_recording(),
            "cannot end a command buffer that is not recording (state={:?})",
            self.state
        );
        self.state = CommandBufferState::Executable;
    }

    /// Resets the buffer back to its initial state.
    pub fn reset(&mut self) {
        self.state = CommandBufferState::Initial;
    }
}
