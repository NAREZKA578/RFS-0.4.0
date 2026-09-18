//! Render Statistics
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Tracks rendering performance metrics.

use std::time::{Duration, Instant};

/// Render statistics for tracking performance
#[derive(Debug, Clone)]
pub struct RenderStats {
    /// Frame number
    pub frame_number: u64,
    /// Total frames rendered
    pub total_frames: u64,
    /// FPS (frames per second)
    pub fps: f32,
    /// Frame time (milliseconds)
    pub frame_time_ms: f32,
    /// CPU time (milliseconds)
    pub cpu_time_ms: f32,
    /// GPU time (milliseconds)
    pub gpu_time_ms: f32,
    /// Draw calls
    pub draw_calls: u32,
    /// Triangles rendered
    pub triangles: u32,
    /// Vertices rendered
    pub vertices: u32,
    /// Textures loaded
    pub textures_loaded: usize,
    /// Textures memory (bytes)
    pub textures_memory: u64,
    /// Buffers memory (bytes)
    pub buffers_memory: u64,
    /// Last frame time
    last_frame_time: Instant,
    /// Frame counter for FPS calculation
    frame_counter: u32,
    /// Time accumulator for FPS calculation
    time_accumulator: Duration,
}

impl RenderStats {
    pub fn new() -> Self {
        Self {
            frame_number: 0,
            total_frames: 0,
            fps: 0.0,
            frame_time_ms: 0.0,
            cpu_time_ms: 0.0,
            gpu_time_ms: 0.0,
            draw_calls: 0,
            triangles: 0,
            vertices: 0,
            textures_loaded: 0,
            textures_memory: 0,
            buffers_memory: 0,
            last_frame_time: Instant::now(),
            frame_counter: 0,
            time_accumulator: Duration::default(),
        }
    }

    /// Begin frame
    pub fn begin_frame(&mut self) {
        self.frame_number += 1;
        self.total_frames += 1;
        self.draw_calls = 0;
        self.triangles = 0;
        self.vertices = 0;
        self.cpu_time_ms = 0.0;
        self.gpu_time_ms = 0.0;
    }

    /// End frame and update statistics
    pub fn end_frame(&mut self) {
        let now = Instant::now();
        let delta = now.duration_since(self.last_frame_time);
        self.last_frame_time = now;

        self.frame_time_ms = delta.as_secs_f32() * 1000.0;

        // Update FPS
        self.frame_counter += 1;
        self.time_accumulator += delta;

        if self.time_accumulator >= Duration::from_secs(1) {
            self.fps = self.frame_counter as f32 / self.time_accumulator.as_secs_f32();
            self.frame_counter = 0;
            self.time_accumulator = Duration::default();
        }
    }

    /// Add draw call
    pub fn add_draw_call(&mut self, triangles: u32, vertices: u32) {
        self.draw_calls += 1;
        self.triangles += triangles;
        self.vertices += vertices;
    }

    /// Add CPU time
    pub fn add_cpu_time(&mut self, duration: Duration) {
        self.cpu_time_ms += duration.as_secs_f32() * 1000.0;
    }

    /// Add GPU time
    pub fn add_gpu_time(&mut self, duration: Duration) {
        self.gpu_time_ms += duration.as_secs_f32() * 1000.0;
    }

    /// Add texture memory
    pub fn add_texture_memory(&mut self, size: u64) {
        self.textures_memory += size;
        self.textures_loaded += 1;
    }

    /// Add buffer memory
    pub fn add_buffer_memory(&mut self, size: u64) {
        self.buffers_memory += size;
    }

    /// Get total memory usage
    pub fn total_memory(&self) -> u64 {
        self.textures_memory + self.buffers_memory
    }

    /// Reset statistics
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for RenderStats {
    fn default() -> Self {
        Self::new()
    }
}
