//! Frame Capture
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use super::stats::FrameStats;

/// Frame capture
pub struct FrameCapture {
    pub commands: Vec<String>,
    pub stats: FrameStats,
}

impl FrameCapture {
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            stats: FrameStats::default(),
        }
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

    pub fn start_capture(&mut self, max_frames: u32) {
        self.is_capturing = true;
        self.max_frames = max_frames;
        self.frame_index = 0;
    }

    pub fn stop_capture(&mut self) {
        self.is_capturing = false;
    }
}
