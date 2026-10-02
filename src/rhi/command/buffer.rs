//! Command Buffer
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::commands::Command;
use super::pool::CommandPool;
use crate::error::{RhiError, RhiResult};
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
    /// Submitted to a queue and not yet completed (bug №233).
    Pending,
    /// The buffer cannot be used again — a submit failed, or it was recorded
    /// out of order. bug №233: this used to be unreachable.
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
    /// Bug №203: the recorded commands used to live only in `CommandEncoder`
    /// and were dropped when `finish()` returned the buffer, so an entire
    /// recording was silently thrown away and nothing reached a backend. The
    /// buffer now owns them, and `end()` seals the list.
    commands: Vec<Command>,
}

impl CommandBuffer {
    pub fn new(pool: CommandPool, desc: CommandBufferDesc) -> Self {
        Self {
            desc,
            pool,
            state: CommandBufferState::Initial,
            commands: Vec::new(),
        }
    }

    /// Append a command. Only valid while recording.
    pub(crate) fn push_command(&mut self, command: Command) {
        self.commands.push(command);
    }

    /// The recorded commands, in order.
    ///
    /// Bug №203: these were dropped by `CommandEncoder::finish`, so this was
    /// always empty and no backend could ever see a recording.
    pub fn commands(&self) -> &[Command] {
        &self.commands
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

    /// Returns `true` if the buffer is submitted and not yet completed.
    pub fn is_pending(&self) -> bool {
        self.state == CommandBufferState::Pending
    }

    /// Bug №181: `begin` and `end` used to be infallible — `end` asserted, so a
    /// misuse aborted the process instead of being reportable, and `begin` was
    /// a silent no-op that let the caller believe it had started a fresh
    /// recording while the previous one was still in progress.
    ///
    /// Both now return a `Result`, and `begin` refuses to re-enter while
    /// already recording so recorded commands cannot be silently discarded.
    pub fn begin(&mut self) -> RhiResult<()> {
        match self.state {
            CommandBufferState::Initial | CommandBufferState::Executable => {
                // Bug №203: a re-begin starts a fresh recording, so the previous
                // command list must go with it. Without this the two recordings
                // would concatenate and the stale half would be replayed.
                self.commands.clear();
                self.state = CommandBufferState::Recording;
                Ok(())
            }
            CommandBufferState::Recording => Err(RhiError::CommandBufferError(
                "cannot begin a command buffer that is already recording; \
                 call end() first so the recorded commands are not discarded"
                    .into(),
            )),
            CommandBufferState::Pending => Err(RhiError::CommandBufferError(format!(
                "cannot begin a command buffer that is pending submission (state={:?})",
                self.state
            ))),
            CommandBufferState::Invalid => Err(RhiError::CommandBufferError(
                "cannot begin an invalid command buffer".into(),
            )),
        }
    }

    /// Stops recording.
    ///
    /// Bug №181: this used to `assert!` and abort the process. It now returns
    /// an error the caller can handle.
    pub fn end(&mut self) -> RhiResult<()> {
        if self.state != CommandBufferState::Recording {
            self.state = CommandBufferState::Invalid;
            return Err(RhiError::CommandBufferInvalidState);
        }
        self.state = CommandBufferState::Executable;
        Ok(())
    }

    /// Marks the buffer as submitted to a queue.
    ///
    /// Bug №233: `Pending` was declared but never reachable, so "submitted and
    /// in flight" was indistinguishable from "ready" and a buffer could be
    /// re-recorded while the GPU was still reading it.
    pub fn submit(&mut self) -> RhiResult<()> {
        if self.state != CommandBufferState::Executable {
            return Err(RhiError::CommandBufferError(format!(
                "only an executable command buffer can be submitted (state={:?})",
                self.state
            )));
        }
        self.state = CommandBufferState::Pending;
        Ok(())
    }

    /// Marks a submitted buffer as finished by the GPU.
    pub fn complete(&mut self) -> RhiResult<()> {
        if self.state != CommandBufferState::Pending {
            return Err(RhiError::CommandBufferError(format!(
                "only a pending command buffer can complete (state={:?})",
                self.state
            )));
        }
        self.state = CommandBufferState::Executable;
        Ok(())
    }

    /// Marks the buffer permanently unusable.
    ///
    /// Bug №233: `Invalid` was declared but never set, so a buffer left in a bad
    /// state after a failed submit looked healthy.
    pub fn invalidate(&mut self, reason: &str) {
        eprintln!("[RHI] command buffer invalidated: {reason}");
        self.state = CommandBufferState::Invalid;
    }

    /// Resets the buffer back to its initial state.
    ///
    /// Bug №181: this unconditionally forced `Initial`, which could resurrect a
    /// buffer the GPU was still reading. Resetting a `Pending` buffer is now
    /// refused.
    pub fn reset(&mut self) -> RhiResult<()> {
        if self.state == CommandBufferState::Pending {
            return Err(RhiError::CommandBufferError(
                "cannot reset a command buffer while it is pending; \
                 call complete() once the GPU is done"
                    .into(),
            ));
        }
        self.state = CommandBufferState::Initial;
        self.commands.clear();
        Ok(())
    }
}
