//! Particle System
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! GPU-based particle system for efficient rendering of large numbers of particles.

use super::emitter::ParticleEmitter;
use super::particle::Particle;
use crate::render::core::RenderContext;
use crate::render::meshes::Mesh;
use crate::rhi::{Buffer, BufferUsage, CommandEncoder, Device, Pipeline, Texture, TextureView};
use std::sync::Arc;
use std::time::Duration;

/// Particle system
pub struct ParticleSystem {
    /// Device reference
    device: Arc<Device>,
    /// Particle emitters
    emitters: Vec<ParticleEmitter>,
    /// Particle buffer (for GPU particles)
    particle_buffer: Option<Buffer>,
    /// Particle count buffer
    particle_count_buffer: Option<Buffer>,
    /// Pipeline
    pipeline: Option<Pipeline>,
    /// Particle mesh (for mesh-based particles)
    particle_mesh: Option<Arc<Mesh>>,
    /// Maximum particles
    max_particles: u32,
    /// Current particle count
    particle_count: u32,
    /// Particle texture atlas
    texture_atlas: Option<Arc<Texture>>,
    /// Texture atlas view
    texture_view: Option<Arc<TextureView>>,
}

impl ParticleSystem {
    pub fn new(device: Arc<Device>, max_particles: u32) -> Self {
        Self {
            device,
            emitters: Vec::new(),
            particle_buffer: None,
            particle_count_buffer: None,
            pipeline: None,
            particle_mesh: None,
            max_particles,
            particle_count: 0,
            texture_atlas: None,
            texture_view: None,
        }
    }

    pub fn max_particles(&self) -> u32 {
        self.max_particles
    }

    pub fn particle_count(&self) -> u32 {
        self.particle_count
    }

    /// Initialize the particle system
    pub fn initialize(&mut self) {
        // Create particle buffer
        let particle_size = std::mem::size_of::<Particle>() as u64;
        let buffer_size = particle_size * self.max_particles as u64;

        self.particle_buffer = Some(self.device.create_buffer(
            buffer_size,
            BufferUsage::VERTEX | BufferUsage::STORAGE | BufferUsage::TRANSFER_DST,
            false,
        ));

        // Create particle count buffer
        self.particle_count_buffer = Some(self.device.create_buffer(
            4,
            BufferUsage::UNIFORM | BufferUsage::TRANSFER_DST,
            false,
        ));

        // Create particle mesh (simple quad)
        self.particle_mesh = Some(Arc::new(Mesh::fullscreen_quad("particle_quad")));

        // Create pipeline
        // This would create a pipeline for rendering particles
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
    pub fn update(&mut self, delta_time: Duration) {
        self.particle_count = 0;

        for emitter in &mut self.emitters {
            emitter.update(delta_time);
            self.particle_count += emitter.particle_count();
        }

        // Update particle buffer
        self.update_particle_buffer();
    }

    /// Update particle buffer with current particle data
    fn update_particle_buffer(&mut self) {
        // Collect all particles from all emitters
        let mut particles: Vec<Particle> = Vec::new();

        for emitter in &self.emitters {
            particles.extend_from_slice(&emitter.particles());
        }

        // Upload to GPU
        if let Some(buffer) = &self.particle_buffer {
            self.device.upload_buffer(buffer, &particles);
        }

        // Update particle count
        if let Some(buffer) = &self.particle_count_buffer {
            let count = particles.len() as u32;
            self.device.upload_buffer(buffer, &[count]);
        }
    }

    /// Render all particles
    pub fn render(&self, encoder: &mut CommandEncoder, _context: &RenderContext) {
        if self.particle_count == 0 || self.pipeline.is_none() {
            return;
        }

        // Bind pipeline
        encoder.bind_pipeline(self.pipeline.as_ref().unwrap());

        // Bind particle buffer
        if let Some(buffer) = &self.particle_buffer {
            encoder.bind_vertex_buffer(buffer);
        }

        // Bind particle count buffer
        if let Some(buffer) = &self.particle_count_buffer {
            encoder.bind_uniform_buffer(buffer, 0);
        }

        // Bind texture atlas
        if let Some(view) = &self.texture_view {
            encoder.bind_texture(view, 0);
        }

        // Set camera matrices
        // This would set the view and projection matrices

        // Draw particles
        if let Some(mesh) = &self.particle_mesh {
            encoder.bind_vertex_buffer(mesh.vertex_buffer().unwrap());
            let index_type = match mesh.index_type() {
                crate::render::meshes::IndexType::U8 => crate::rhi::IndexType::U16,
                crate::render::meshes::IndexType::U16 => crate::rhi::IndexType::U16,
                crate::render::meshes::IndexType::U32 => crate::rhi::IndexType::U32,
            };
            encoder.bind_index_buffer(mesh.index_buffer().unwrap(), index_type);
            encoder.draw_indexed_instanced(mesh.index_count(), self.particle_count, 0, 0, 0);
        }
    }

    /// Set texture atlas
    pub fn set_texture_atlas(&mut self, texture: Arc<Texture>) {
        self.texture_atlas = Some(texture.clone());
        self.texture_view = Some(Arc::new(texture.create_view(Default::default())));
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
