//! Particle System
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! GPU-based particle system for efficient rendering of large numbers of particles.

use super::emitter::ParticleEmitter;
use super::particle::Particle;
use crate::rhi::{Buffer, BufferUsage, Device};
use std::sync::Arc;
use std::time::Duration;

/// Particle system
///
/// The data side of the particles: emitters, the live pool, the upload of the
/// live particles into the GPU buffer. It deliberately renders nothing — the
/// drawing is the particle pass's (`crate::render::passes::particles`), which
/// binds this buffer and draws the live count instanced.
pub struct ParticleSystem {
    /// Device reference
    device: Arc<Device>,
    /// Particle emitters
    emitters: Vec<ParticleEmitter>,
    /// Particle buffer (for GPU particles)
    particle_buffer: Option<Buffer>,
    /// Particle count buffer
    particle_count_buffer: Option<Buffer>,
    /// Maximum particles
    max_particles: u32,
    /// Current particle count
    particle_count: u32,
}

impl ParticleSystem {
    pub fn new(device: Arc<Device>, max_particles: u32) -> Self {
        Self {
            device,
            emitters: Vec::new(),
            particle_buffer: None,
            particle_count_buffer: None,
            max_particles,
            particle_count: 0,
        }
    }

    pub fn max_particles(&self) -> u32 {
        self.max_particles
    }

    pub fn particle_count(&self) -> u32 {
        self.particle_count
    }

    /// The buffer the live particles are uploaded into, for the pass that
    /// draws them. `None` until [`Self::initialize`] ran; the particle pass
    /// refuses to draw without one rather than binding nothing.
    pub(crate) fn particle_buffer(&self) -> Option<&Buffer> {
        self.particle_buffer.as_ref()
    }

    /// Initialize the particle system
    pub fn initialize(&mut self) -> crate::rhi::RhiResult<()> {
        // Create particle buffer
        let particle_size = std::mem::size_of::<Particle>() as u64;
        let buffer_size = particle_size * self.max_particles as u64;

        self.particle_buffer = Some(
            self.device
                .create_buffer(
                    buffer_size,
                    BufferUsage::VERTEX | BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
                    false,
                )
                .map_err(|e| {
                    crate::rhi::RhiError::BackendError(format!("the particle buffer: {e}"))
                })?,
        );

        // Create particle count buffer
        self.particle_count_buffer = Some(
            self.device
                .create_buffer(4, BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST, false)
                .map_err(|e| {
                    crate::rhi::RhiError::BackendError(format!("the particle count buffer: {e}"))
                })?,
        );

        Ok(())
    }

    /// Add an emitter
    pub fn add_emitter(&mut self, emitter: ParticleEmitter) {
        self.emitters.push(emitter);
    }

    /// Remove an emitter
    pub fn remove_emitter(&mut self, index: usize) -> Option<ParticleEmitter> {
        if index < self.emitters.len() {
            Some(self.emitters.remove(index))
        } else {
            None
        }
    }

    /// Get an emitter
    pub fn get_emitter(&self, index: usize) -> Option<&ParticleEmitter> {
        self.emitters.get(index)
    }

    /// Get a mutable emitter
    pub fn get_emitter_mut(&mut self, index: usize) -> Option<&mut ParticleEmitter> {
        self.emitters.get_mut(index)
    }

    /// Update all emitters
    ///
    /// The count this leaves behind is the count **in the buffer** — what the
    /// particle pass draws — not the sum of the emitters' live pools; the two
    /// differ exactly when the buffer could not hold everything.
    pub fn update(&mut self, delta_time: Duration) {
        for emitter in &mut self.emitters {
            emitter.update(delta_time);
        }

        // Update particle buffer. If the upload failed the GPU holds the previous
        // frame's particles, so the count is forced to zero rather than letting
        // the pass draw a stale frame under a fresh count.
        if !self.update_particle_buffer() {
            self.particle_count = 0;
        }
    }

    /// Update particle buffer with current particle data
    ///
    /// Returns whether the GPU now holds this frame's particles. A failed
    /// upload used to be invisible: the count kept rising while the buffer kept
    /// the previous frame's contents, so the pass drew stale particles.
    fn update_particle_buffer(&mut self) -> bool {
        // Upload only LIVE particles (dead pool entries must not reach GPU).
        let mut particles: Vec<Particle> = Vec::new();

        for emitter in &self.emitters {
            particles.extend(emitter.live_particles());
        }

        // The buffer holds `max_particles` entries, and several emitters can
        // be alive at once: the sum of their pools can exceed the buffer. The
        // pass draws `particle_count` instances and the shader indexes the
        // buffer by the instance index, so the count may never name more
        // particles than the buffer holds — the excess is dropped here rather
        // than read out of bounds on the GPU.
        let capacity = self.max_particles as usize;
        if particles.len() > capacity {
            crate::warn!(
                "particles",
                "{} live particles for a buffer of {capacity} entries: the excess is not drawn",
                particles.len()
            );
            particles.truncate(capacity);
        }

        // The count the pass draws is the count **in the buffer**, not the
        // count in the emitters: the two differ exactly when the buffer could
        // not hold everything, and the draw must agree with what was
        // uploaded.
        self.particle_count = particles.len() as u32;

        let mut uploaded = true;

        // Upload to GPU
        if let Some(buffer) = &self.particle_buffer {
            uploaded &= self.device.upload_buffer(buffer, &particles).is_ok();
        }

        // Update particle count
        if let Some(buffer) = &self.particle_count_buffer {
            let count = particles.len() as u32;
            uploaded &= self.device.upload_buffer(buffer, &[count]).is_ok();
        }

        uploaded
    }

    /// Clear all emitters
    pub fn clear(&mut self) {
        self.emitters.clear();
        self.particle_count = 0;
    }
}

/// Particle system configuration
#[derive(Debug, Clone)]
pub struct ParticleSystemConfig {
    pub max_particles: u32,
    pub max_emitters: usize,
    pub gpu_particles: bool,
    pub compute_shader: bool,
}

impl Default for ParticleSystemConfig {
    fn default() -> Self {
        Self {
            max_particles: 100000,
            max_emitters: 100,
            gpu_particles: true,
            compute_shader: true,
        }
    }
}
