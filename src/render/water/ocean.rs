//! A CPU Gerstner ocean, plus the GPU mesh that draws it.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! The full `WaterPass` needs a reflection image, a refraction image and the
//! opaque pass's depth, and refuses to shade a surface it cannot shade
//! honestly. This module is the simulation-first slice that does not need any
//! of them: a dense grid displaced on the CPU by the same [`WaveSystem`] that
//! answers the ship's buoyancy queries, uploaded into a vertex buffer and drawn
//! by the existing geometry pass.
//!
//! The two halves are deliberately one object. If the drawn vertices came from
//! one wave field and the ship floated on another, the hull would sink through
//! crests that are visibly somewhere else — the kind of mismatch that looks
//! like a physics bug and is really a second source of truth.
//!
//! What this is not: reflective water. There is no mirror camera, no Fresnel
//! reflection and no refraction. The surface is opaque and lit by the geometry
//! pass's single directional light, with vertex colours marking deep water and
//! crests. That keeps the slice honest and leaves `WaterPass` for the frame
//! that can supply its inputs.

use std::sync::Arc;
use std::time::Duration;

use glam::{Vec2, Vec3, Vec4};

use crate::render::meshes::mesh::{Mesh, Vertex};
use crate::render::water::waves::{WaveMethod, WaveSystem};
use crate::rhi::{Buffer, BufferUsage, Device, RhiResult};

/// Side length of the ocean plane, in world units.
pub const OCEAN_SIZE: f32 = 400.0;
/// Grid cells per side. 96 gives 9409 vertices — dense enough that a 0.5 m
/// wave reads as a surface rather than as a few large triangles, and small
/// enough that rebuilding every frame costs well under a millisecond.
pub const OCEAN_SEGMENTS: u32 = 96;
/// The world-space height the undistorted sea sits at.
pub const OCEAN_LEVEL: f32 = -0.9;

/// Trough colour. Linear-ish values because the geometry shader multiplies
/// them by the light without a gamma conversion in between.
pub const OCEAN_DEEP_COLOR: Vec4 = Vec4::new(0.03, 0.11, 0.21, 1.0);
/// Crest colour, mixed in by the wave height at each vertex.
pub const OCEAN_CREST_COLOR: Vec4 = Vec4::new(0.21, 0.58, 0.70, 1.0);

/// The wave field and the sampling used by both the mesh and the ship.
pub struct Ocean {
    size: f32,
    segments: u32,
    level: f32,
    waves: WaveSystem,
}

impl Ocean {
    /// Build a sea of `size` x `size` world units, `segments` cells per side,
    /// whose calm surface is at `level`.
    pub fn new(size: f32, segments: u32, level: f32) -> Self {
        let mut waves = WaveSystem::new(WaveMethod::Gerstner);
        // The authored waves are tuned for a large sea; these two values
        // stretch their wavelength so a 400-unit plane does not look like a
        // corrugated roof.
        waves.config_mut().scale = 0.06;
        waves.config_mut().speed = 0.55;
        Self {
            size: size.max(1.0),
            segments: segments.max(1),
            level,
            waves,
        }
    }

    pub fn size(&self) -> f32 {
        self.size
    }

    pub fn segments(&self) -> u32 {
        self.segments
    }

    /// The height the calm surface would have.
    pub fn level(&self) -> f32 {
        self.level
    }

    pub fn waves(&self) -> &WaveSystem {
        &self.waves
    }

    pub fn waves_mut(&mut self) -> &mut WaveSystem {
        &mut self.waves
    }

    pub fn update(&mut self, delta_time: Duration) {
        self.waves.update(delta_time);
    }

    /// World-space surface height at `(x, z)`, including [`Self::level`].
    pub fn height_at(&self, x: f32, z: f32) -> f32 {
        self.level + self.waves.get_height(Vec3::new(x, 0.0, z))
    }

    /// World-space surface normal at `(x, z)`, pointing up.
    pub fn normal_at(&self, x: f32, z: f32) -> Vec3 {
        self.waves.get_normal(Vec3::new(x, 0.0, z))
    }

    /// Vertices and triangle indices for the current sea.
    ///
    /// Vertex positions are already in world space (including the level), so
    /// the water entity's transform stays identity and the buoyancy query and
    /// the drawn surface share one coordinate space.
    pub fn build_geometry(&self) -> (Vec<Vertex>, Vec<u32>) {
        let stride = self.segments as usize + 1;
        let mut vertices = vec![Vertex::default(); stride * stride];
        self.fill_vertices(&mut vertices);

        let mut indices = Vec::with_capacity(self.segments as usize * self.segments as usize * 6);
        for row in 0..self.segments {
            for col in 0..self.segments {
                let a = (row * self.segments + col) as u32;
                let b = a + 1;
                let c = a + self.segments + 1;
                let d = c + 1;
                // Counter-clockwise seen from above. The geometry pass is
                // double-sided, but a correct winding keeps the normals and
                // any future back-face culling in agreement.
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }

        (vertices, indices)
    }

    /// Recompute `vertices` for the current time and wave scale.
    ///
    /// The length is fixed by the grid, so this reuses the caller's allocation
    /// rather than building a fresh vector every frame.
    pub fn fill_vertices(&self, vertices: &mut [Vertex]) {
        let stride = self.segments as usize + 1;
        debug_assert_eq!(
            vertices.len(),
            stride * stride,
            "the ocean vertex buffer must match the grid"
        );

        let half = self.size * 0.5;
        let step = self.size / self.segments as f32;
        let crest_extent = self.waves.max_height().max(1e-4);

        for row in 0..=self.segments as usize {
            let z = -half + row as f32 * step;
            for col in 0..=self.segments as usize {
                let x = -half + col as f32 * step;
                let point = Vec3::new(x, 0.0, z);
                let wave = self.waves.get_height(point);
                let normal = self.waves.get_normal(point);
                let crest = ((wave / crest_extent) * 0.5 + 0.5).clamp(0.0, 1.0);
                let color = OCEAN_DEEP_COLOR.lerp(OCEAN_CREST_COLOR, crest);

                vertices[row * stride + col] = Vertex::new(Vec3::new(x, self.level + wave, z))
                    .with_normal(normal)
                    .with_tex_coord(Vec2::new(
                        col as f32 / self.segments as f32,
                        row as f32 / self.segments as f32,
                    ))
                    .with_color(color);
            }
        }
    }
}

impl Default for Ocean {
    fn default() -> Self {
        Self::new(OCEAN_SIZE, OCEAN_SEGMENTS, OCEAN_LEVEL)
    }
}

/// The ocean as a drawable mesh: wave field, CPU vertices, GPU buffers.
pub struct OceanMesh {
    ocean: Ocean,
    mesh: Arc<Mesh>,
    vertex_buffer: Arc<Buffer>,
    vertices: Vec<Vertex>,
}

impl OceanMesh {
    /// Build the grid and its GPU buffers.
    ///
    /// The vertex buffer is host-visible on purpose. The grid changes every
    /// frame, and the mesh path's one-shot staging upload (`cpu_visible =
    /// false`) would submit and wait on a transfer every frame; a mapped
    /// buffer turns the update into a memcpy and a flush.
    pub fn new(device: &Device, ocean: Ocean) -> RhiResult<Self> {
        let (vertices, indices) = ocean.build_geometry();

        let vertex_size = (vertices.len() * std::mem::size_of::<Vertex>()) as u64;
        let vertex_buffer = device.create_buffer(
            vertex_size,
            BufferUsage::VERTEX | BufferUsage::TRANSFER_DST | BufferUsage::TRANSFER_SRC,
            true,
        )?;
        device.upload_buffer(&vertex_buffer, &vertices)?;

        let mut mesh = Mesh::new("ocean")
            .with_vertices(vertices.clone())
            .with_indices(indices);
        // The index buffer is static; the ordinary mesh path is correct for
        // it. The vertex buffer is replaced afterwards with the host-visible
        // one the per-frame update writes into.
        mesh.create_index_buffer(device)?;
        mesh.vertex_buffer = Some(Arc::new(vertex_buffer.clone()));

        Ok(Self {
            ocean,
            mesh: Arc::new(mesh),
            vertex_buffer: Arc::new(vertex_buffer),
            vertices,
        })
    }

    /// Advance the waves and push the new grid into the GPU buffer.
    pub fn update(&mut self, device: &Device, delta_time: Duration) -> RhiResult<()> {
        self.ocean.update(delta_time);
        self.ocean.fill_vertices(&mut self.vertices);
        device.upload_buffer(&self.vertex_buffer, &self.vertices)
    }

    pub fn ocean(&self) -> &Ocean {
        &self.ocean
    }

    pub fn ocean_mut(&mut self) -> &mut Ocean {
        &mut self.ocean
    }

    pub fn mesh(&self) -> Arc<Mesh> {
        self.mesh.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_has_the_documented_counts() {
        let ocean = Ocean::new(100.0, 4, -1.0);
        let (vertices, indices) = ocean.build_geometry();
        assert_eq!(vertices.len(), 25, "5 x 5 vertices");
        assert_eq!(indices.len(), 4 * 4 * 6, "two triangles per cell");
        assert!(
            indices
                .iter()
                .all(|index| (*index as usize) < vertices.len()),
            "every index must point inside the grid"
        );
    }

    #[test]
    fn a_cleared_sea_samples_its_level() {
        let mut ocean = Ocean::new(100.0, 4, -0.9);
        ocean.waves_mut().clear_waves();
        assert_eq!(ocean.height_at(12.0, -7.0), -0.9);
        assert_eq!(ocean.normal_at(12.0, -7.0), Vec3::Y);
    }

    #[test]
    fn every_vertex_is_finite_and_the_heights_follow_the_wave_field() {
        let mut ocean = Ocean::new(40.0, 8, -0.9);
        ocean.update(Duration::from_secs_f32(1.25));
        let (vertices, _) = ocean.build_geometry();
        let extent = ocean.waves().max_height() + 1e-3;

        for vertex in &vertices {
            assert!(
                vertex.position.is_finite(),
                "position {:?}",
                vertex.position
            );
            assert!(vertex.normal.is_finite(), "normal {:?}", vertex.normal);
            assert!(
                (vertex.position.y - ocean.level()).abs() <= extent,
                "height {} escaped the wave envelope {}",
                vertex.position.y,
                extent
            );
            assert!(vertex.normal.y >= 0.0, "a surface normal points down");
        }

        // The centre of the grid is a real sample of the same field, not a
        // separately invented height.
        let stride = ocean.segments() as usize + 1;
        let centre = &vertices[(stride / 2) * stride + stride / 2];
        assert!((centre.position.y - ocean.height_at(0.0, 0.0)).abs() < 1e-5);
    }

    #[test]
    fn the_wave_scale_moves_the_whole_surface() {
        let mut ocean = Ocean::new(40.0, 4, -0.9);
        let point = (3.0, 7.0);
        let calm = ocean.height_at(point.0, point.1);

        ocean.waves_mut().set_height_scale(2.0);
        let rough = ocean.height_at(point.0, point.1);
        assert!(
            ((rough - ocean.level()) - 2.0 * (calm - ocean.level())).abs() < 1e-5,
            "calm {calm}, rough {rough}"
        );
    }

    #[test]
    fn the_crest_colour_is_brighter_than_the_trough() {
        assert!(OCEAN_CREST_COLOR.length() > OCEAN_DEEP_COLOR.length());
    }
}
