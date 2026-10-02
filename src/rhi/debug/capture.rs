//! Frame Capture
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::stats::FrameStats;

/// Bug №182: an unbounded `commands` list is a real memory sink — a frame
/// capture left running in an unlimited-frame mode grows for as long as the
/// process runs. Recording now stops at this many entries and counts what it
/// dropped, so a capture is bounded *and* honest about being truncated.
pub const MAX_CAPTURED_COMMANDS: usize = 1 << 20;

/// Frame capture
pub struct FrameCapture {
    pub commands: Vec<String>,
    pub stats: FrameStats,
    /// Commands refused because `MAX_CAPTURED_COMMANDS` was reached.
    pub dropped_commands: u64,
}

impl FrameCapture {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            stats: FrameStats::default(),
            dropped_commands: 0,
        }
    }

    /// Records a command string into the capture.
    ///
    /// Returns `false` when the capture is full and the command was dropped, so
    /// a caller that cares can react instead of silently losing records.
    pub fn add_command(&mut self, command: impl Into<String>) -> bool {
        if self.commands.len() >= MAX_CAPTURED_COMMANDS {
            self.dropped_commands += 1;
            return false;
        }
        self.commands.push(command.into());
        true
    }

    /// Returns the total number of recorded commands.
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// True when recording stopped because the capture filled up.
    pub fn is_truncated(&self) -> bool {
        self.dropped_commands > 0
    }
}

/// Capture context
pub struct CaptureContext {
    pub is_capturing: bool,
    pub frame_index: u32,
    pub max_frames: u32,
}

impl CaptureContext {
    pub fn new() -> Self {
        Self {
            is_capturing: false,
            frame_index: 0,
            max_frames: 0,
        }
    }

    /// Begin capturing.
    ///
    /// `max_frames` of 0 means **unlimited** — capture until `stop_capture`.
    /// That is a legitimate mode, but it is not a default: `new()` therefore
    /// starts with `max_frames: 0` and `is_capturing: false`, so an idle
    /// context records nothing. Bug №182 was that the meaning of 0 was
    /// undocumented while the *command list* behind it grew without any bound.
    /// The list is now capped by `MAX_CAPTURED_COMMANDS` and the overflow is
    /// counted, so an unlimited capture cannot exhaust memory.
    pub fn start_capture(&mut self, max_frames: u32) {
        self.is_capturing = true;
        self.max_frames = max_frames;
        self.frame_index = 0;
    }

    pub fn stop_capture(&mut self) {
        self.is_capturing = false;
    }

    /// Moves to the next captured frame and returns `false` once the
    /// configured frame budget has been reached.
    ///
    /// `max_frames == 0` keeps capturing, per the documented meaning above.
    pub fn advance_frame(&mut self) -> bool {
        if !self.is_capturing {
            return false;
        }
        self.frame_index += 1;
        if self.max_frames > 0 && self.frame_index >= self.max_frames {
            self.stop_capture();
            false
        } else {
            true
        }
    }

    /// Frames captured so far in the current run.
    pub fn frames_captured(&self) -> u32 {
        self.frame_index
    }
}

impl Default for CaptureContext {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for FrameCapture {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_budget_means_unlimited_and_is_documented() {
        let mut c = CaptureContext::new();
        c.start_capture(0);
        for _ in 0..10_000 {
            assert!(c.advance_frame(), "an unlimited capture must not stop");
        }
        assert_eq!(c.frames_captured(), 10_000);
        assert!(c.is_capturing);
    }

    #[test]
    fn a_finite_budget_stops_exactly_at_the_limit() {
        let mut c = CaptureContext::new();
        c.start_capture(3);
        assert!(c.advance_frame());
        assert!(c.advance_frame());
        assert!(!c.advance_frame(), "the third frame must exhaust the budget");
        assert!(!c.is_capturing);
        assert_eq!(c.frames_captured(), 3);
    }

    #[test]
    fn an_idle_context_captures_nothing() {
        let mut c = CaptureContext::new();
        assert!(!c.advance_frame());
        assert_eq!(c.frames_captured(), 0);
    }

    /// Bug №182: the command list used to grow without any bound.
    #[test]
    fn the_command_list_is_bounded_and_the_overflow_is_counted() {
        let mut f = FrameCapture::new();
        for i in 0..MAX_CAPTURED_COMMANDS {
            assert!(f.add_command(format!("cmd {i}")));
        }
        assert_eq!(f.command_count(), MAX_CAPTURED_COMMANDS);
        assert!(!f.is_truncated());

        assert!(!f.add_command("one too many"), "the capture is full");
        assert!(f.is_truncated());
        assert_eq!(f.dropped_commands, 1);
        assert_eq!(f.command_count(), MAX_CAPTURED_COMMANDS);
    }
}
