//! Particle
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use glam::{Vec2, Vec3, Vec4};

/// Particle struct
///
/// The buffer this fills is bound as a **storage buffer** in the particle
/// pass's vertex stage, and the shader declares its own view of the same
/// bytes (`particle.wgsl`). WGSL lays a struct out by its members'
/// alignments, and in both the uniform and the storage address spaces
/// `vec3<f32>` is **16-aligned** — the same rule the light buffer's shader
/// comment spells out. This struct is uploaded raw (`upload_buffer` casts the
/// slice to bytes), so every offset here has to be exactly the WGSL one:
///
/// | field          | offset |
/// |----------------|--------|
/// | `position`     | 0      |
/// | `velocity`     | 16     |
/// | `size`         | 32     |
/// | `color`        | 48     |
/// | `lifetime`     | 64     |
/// | `max_lifetime` | 68     |
/// | `rotation`     | 72     |
/// | `rotation_speed` | 76   |
/// | `texture_index` | 80    |
/// | `emitter_id`   | 84     |
///
/// The struct's WGSL size is 88, and the array stride rounds up to the
/// alignment — **96 bytes**. Without the pads the fields would pack at
/// 4-byte offsets and the shader would read every vector after `position`
/// from the wrong place, and every particle would be drawn from garbage.
///
/// The pads are never read on the CPU side, which is deliberate: they exist
/// so the upload's bytes and the shader's offsets agree.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Particle {
    pub position: Vec3,
    /// To carry `velocity` from 12 to the WGSL offset 16.
    #[allow(dead_code)]
    pub(crate) pad0: f32,
    pub velocity: Vec3,
    /// To carry `size` from 28 to the WGSL offset 32.
    #[allow(dead_code)]
    pub(crate) pad1: f32,
    pub size: Vec2,
    /// To carry `color` (16-aligned in WGSL) from 40 to the WGSL offset 48.
    #[allow(dead_code)]
    pub(crate) pad2: f32,
    #[allow(dead_code)]
    pub(crate) pad3: f32,
    pub color: Vec4,
    pub lifetime: f32,
    pub max_lifetime: f32,
    pub rotation: f32,
    pub rotation_speed: f32,
    pub texture_index: u32,
    pub emitter_id: u32,
    /// The tail pad: the WGSL array stride is `round_up(88, 16)` = 96.
    #[allow(dead_code)]
    pub(crate) pad4: [f32; 2],
}

// The upload is raw bytes, so these are not implementation details but the
// contract with `particle.wgsl`. If one of them fails to compile, a `glam`
// layout change moved a field and the shader now reads it from somewhere
// else — fix the shader's declaration and this struct together, or the
// picture comes out wrong in a way no test of the CPU side can see.
const _: () = assert!(std::mem::size_of::<Particle>() == 96);
const _: () = assert!(std::mem::offset_of!(Particle, velocity) == 16);
const _: () = assert!(std::mem::offset_of!(Particle, size) == 32);
const _: () = assert!(std::mem::offset_of!(Particle, color) == 48);
const _: () = assert!(std::mem::offset_of!(Particle, lifetime) == 64);
const _: () = assert!(std::mem::offset_of!(Particle, texture_index) == 80);
const _: () = assert!(std::mem::offset_of!(Particle, emitter_id) == 84);

impl Particle {
    pub fn new() -> Self {
        Self {
            position: Vec3::ZERO,
            pad0: 0.0,
            velocity: Vec3::ZERO,
            pad1: 0.0,
            size: Vec2::ONE,
            pad2: 0.0,
            pad3: 0.0,
            color: Vec4::ONE,
            lifetime: 0.0,
            max_lifetime: 1.0,
            rotation: 0.0,
            rotation_speed: 0.0,
            texture_index: 0,
            emitter_id: 0,
            pad4: [0.0; 2],
        }
    }

    pub fn is_alive(&self) -> bool {
        self.lifetime < self.max_lifetime
    }

    pub fn update(&mut self, delta_time: std::time::Duration) {
        let delta_seconds = delta_time.as_secs_f32();

        self.position += self.velocity * delta_seconds;
        self.lifetime += delta_seconds;
        self.rotation += self.rotation_speed * delta_seconds;
    }
}

impl Default for Particle {
    fn default() -> Self {
        Self::new()
    }
}
