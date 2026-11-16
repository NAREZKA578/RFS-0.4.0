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
pub const OCEAN_SIZE: f32 = 600.0;
/// Grid cells per side. 224 gives 50 625 vertices and a 2.68 m cell, which
/// resolves the 24 m wave with nine samples per wavelength. The per-frame CPU
/// rebuild still fits in a few milliseconds; the 160-cell grid left the short
/// swell looking like a corrugated plate.
pub const OCEAN_SEGMENTS: u32 = 224;
/// The world-space height the undistorted sea sits at.
pub const OCEAN_LEVEL: f32 = -0.9;

/// Trough colour. Linear-ish values because the geometry shader multiplies
/// them by the light without a gamma conversion in between. The water shader
/// takes its palette from `WaterMaterial::ocean`; these remain the fallback
/// used when the water pipeline is not installed. Kept in step with the
/// material preset so both paths draw the same sea.
pub const OCEAN_DEEP_COLOR: Vec4 = Vec4::new(0.005, 0.034, 0.086, 1.0);
/// Crest colour, mixed in by the wave height at each vertex.
pub const OCEAN_CREST_COLOR: Vec4 = Vec4::new(0.016, 0.118, 0.166, 1.0);

/// The wave field and the sampling used by both the mesh and the ship.
pub struct Ocean {
    size: f32,
    segments: u32,
    level: f32,
    waves: WaveSystem,
    /// Where the drawn patch is centred, in world XZ. The wave field itself
    /// is defined everywhere; only the finite grid moves. A connected client
    /// walks a server world measured in kilometres, and the grid (600 m wide)
    /// must follow the camera or the sea ends in black space a hundred
    /// metres from the hull. Default: centred on the origin, exactly what the
    /// menu scene always did.
    center: Vec2,
}

impl Ocean {
    /// Build a sea of `size` x `size` world units, `segments` cells per side,
    /// whose calm surface is at `level`.
    pub fn new(size: f32, segments: u32, level: f32) -> Self {
        let waves = WaveSystem::new(WaveMethod::Gerstner);
        Self {
            size: size.max(1.0),
            segments: segments.max(1),
            level,
            waves,
            center: Vec2::ZERO,
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

    /// Where the drawn patch is centred, in world XZ.
    pub fn center(&self) -> Vec2 {
        self.center
    }

    /// Move the drawn patch to a new world XZ centre.
    ///
    /// The wave phase is sampled at each vertex's *world* position, so a
    /// moved patch still shows the same sea: the pattern is anchored to the
    /// world, only the finite window over it slides. Callers that hand-craft
    /// a centre should snap it to whole grid cells (`size / segments`), which
    /// keeps every vertex at a world position it already had — a recentred
    /// patch is then pixel-identical to the old one and nothing swims.
    pub fn set_center(&mut self, center: Vec2) {
        self.center = center;
    }

    /// Half the patch's side length: the farthest the surface reaches from
    /// its centre.
    pub fn half_extent(&self) -> f32 {
        self.size * 0.5
    }

    /// Slide the drawn patch toward `focus` (a world XZ point, e.g. the
    /// camera's or our hull's position), so a connected client walking a
    /// kilometre-scale server world keeps the sea under it instead of
    /// leaving it behind.
    ///
    /// The move happens only once the focus has drifted past a quarter of
    /// the patch's side (the sea's edge would be closing in otherwise), and
    /// the new centre is snapped to whole grid cells: the new window then
    /// covers exactly vertex positions the old one did, so the drawn sea is
    /// identical — only moved, never swimming. Returns whether it moved.
    pub fn recenter_toward(&mut self, focus: Vec3) -> bool {
        let threshold = self.size * 0.25;
        let offset = Vec2::new(focus.x, focus.z) - self.center;
        if offset.length() <= threshold {
            return false;
        }
        let cell = self.size / self.segments as f32;
        self.center = Vec2::new(
            (focus.x / cell).round() * cell,
            (focus.z / cell).round() * cell,
        );
        true
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
        let source = self.source_position(x, z);
        self.level + self.waves.get_height(source)
    }

    /// World-space surface normal at `(x, z)`, pointing up.
    pub fn normal_at(&self, x: f32, z: f32) -> Vec3 {
        self.waves.get_normal(self.source_position(x, z))
    }

    /// Invert the Gerstner displacement: find the undisplaced grid point whose
    /// displaced position is `(x, z)`.
    ///
    /// The surface is `P = source + D(source)`, so the fixed-point iteration
    /// `source = P - D(source)` converges quickly — the Jacobian of `D` is
    /// capped below one by the wave system. Buoyancy queries a world position,
    /// but the wave phase is defined in source space; without this inversion
    /// the ship would float on the water that used to be there.
    fn source_position(&self, x: f32, z: f32) -> Vec3 {
        let mut source = Vec3::new(x, 0.0, z);
        for _ in 0..3 {
            let offset = self.waves.get_horizontal_displacement(source);
            source = Vec3::new(x - offset.x, 0.0, z - offset.y);
        }
        source
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
        // The vertices are laid out with `segments + 1` per row, so the index
        // of the next row is `a + stride`, not `a + segments`. Using the cell
        // count as the row stride (the obvious mistake) shifts every row by
        // one vertex per row and stitches triangles across non-adjacent rows:
        // the surface still covers the screen, but it is made of large skewed
        // plates rather than the grid the vertices describe.
        let stride = self.segments + 1;
        for row in 0..self.segments {
            for col in 0..self.segments {
                let a = row * stride + col;
                let b = a + 1;
                let c = a + stride;
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
            let z = self.center.y - half + row as f32 * step;
            for col in 0..=self.segments as usize {
                let x = self.center.x - half + col as f32 * step;
                let point = Vec3::new(x, 0.0, z);
                let wave = self.waves.get_height(point);
                let normal = self.waves.get_normal(point);
                let offset = self.waves.get_horizontal_displacement(point);
                let crest = ((wave / crest_extent) * 0.5 + 0.5).clamp(0.0, 1.0);
                let color = OCEAN_DEEP_COLOR.lerp(OCEAN_CREST_COLOR, crest);

                vertices[row * stride + col] =
                    Vertex::new(Vec3::new(x + offset.x, self.level + wave, z + offset.y))
                        .with_normal(normal)
                        // The water shader evaluates the wave field at the
                        // *source* position. The displaced vertex no longer
                        // maps one-to-one to the wave phase, and using its
                        // world XZ would shift the normals, foam and ripples
                        // relative to the crests they belong to.
                        .with_tex_coord(Vec2::new(x, z))
                        // Alpha is the normalised crest height. The water shader
                        // uses it to place foam and the shallow-water tint without
                        // needing to know the wave envelope.
                        .with_color(Vec4::new(color.x, color.y, color.z, crest));
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
///
/// The mesh's cached bounds are computed once, at construction, for the
/// patch centred on the origin. Recentring does NOT refresh them — the
/// geometry pass draws every item it was built with and never re-culls
/// (`Scene::cull` runs only inside `items_for_scene`), so a moved patch
/// keeps drawing wherever its vertices are, and the stale bounds are
/// metadata, not a cull decision. Nothing mutates a mesh under its `Arc`.
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
    fn indices_follow_the_vertex_grid_not_the_cell_stride() {
        let ocean = Ocean::new(10.0, 4, 0.0);
        let (_, indices) = ocean.build_geometry();
        let stride = 5u32;

        // The first cell of the first row: (0,0), (0,1), (1,0), (1,1).
        assert_eq!(&indices[..6], &[0, stride, 1, 1, stride, stride + 1]);
        // The second cell of the same row shifts by one column.
        assert_eq!(&indices[6..12], &[1, stride + 1, 2, 2, stride + 1, stride + 2]);
        // The first cell of the second row starts at vertex `stride`, not at
        // `segments` — the one-off that produced skewed triangles.
        let second_row = 4 * 6;
        assert_eq!(
            &indices[second_row..second_row + 6],
            &[
                stride,
                stride + stride,
                stride + 1,
                stride + 1,
                stride + stride,
                stride + stride + 1
            ]
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
        // separately invented height. The vertex has been displaced sideways,
        // so compare it with the height at its *source* position, not at the
        // world origin.
        let stride = ocean.segments() as usize + 1;
        let centre = &vertices[(stride / 2) * stride + stride / 2];
        let source_height = ocean.level() + ocean.waves().get_height(Vec3::ZERO);
        assert!((centre.position.y - source_height).abs() < 1e-5);
    }

    #[test]
    fn a_recentred_patch_keeps_the_field_and_moves_the_window() {
        // The wave field is anchored to the world: moving the drawn patch
        // must not change the height the field reports at a given world
        // point, only where the finite grid sits.
        let mut ocean = Ocean::new(40.0, 4, -0.9);
        ocean.update(Duration::from_secs_f32(1.25));
        let world_point = Vec3::new(3.0, 0.0, 7.0);
        let height_before = ocean.height_at(world_point.x, world_point.z);
        let (vertices_before, _) = ocean.build_geometry();

        // A whole-cell offset (3 cells in x, 2 in z, step 10 m): the new
        // grid's source positions sit on the SAME source lattice the old
        // grid used, so the overlap of the two windows must draw identically
        // — the sea does not swim under the camera.
        ocean.set_center(Vec2::new(30.0, -20.0));
        let (vertices_after, _) = ocean.build_geometry();
        let stride = ocean.segments() as usize + 1;

        // new(row, col) samples the same source as old(row - 2, col + 3).
        for row in 2..=4usize {
            for col in 0..=1usize {
                let old = &vertices_before[(row - 2) * stride + col + 3];
                let new = &vertices_after[row * stride + col];
                assert!(
                    new.position.abs_diff_eq(old.position, 1e-5),
                    "the overlap moved: new {:?} vs old {:?}",
                    new.position,
                    old.position
                );
                assert!(new.normal.abs_diff_eq(old.normal, 1e-5));
                assert!(new.tex_coord.abs_diff_eq(old.tex_coord, 1e-5));
            }
        }

        // The field still answers heights at the same world position, and
        // the moved window covers the moved centre.
        assert!(
            (ocean.height_at(world_point.x, world_point.z) - height_before).abs() < 1e-5,
            "the field moved with the patch"
        );
        let centre_vertex = &vertices_after[(stride / 2) * stride + stride / 2];
        let patch_centre = Vec2::new(centre_vertex.position.x, centre_vertex.position.z);
        assert!(
            patch_centre.abs_diff_eq(ocean.center(), ocean.size() / ocean.segments() as f32),
            "the grid centre drifted: {:?} vs {:?}",
            patch_centre,
            ocean.center()
        );
    }

    #[test]
    fn recenter_toward_snaps_to_cells_and_waits_until_needed() {
        let mut ocean = Ocean::new(600.0, 224, OCEAN_LEVEL);
        // Inside a quarter of the patch: nothing moves.
        assert!(!ocean.recenter_toward(Vec3::new(10.0, 0.0, 10.0)));
        assert_eq!(ocean.center(), Vec2::ZERO);

        // Past the threshold: the centre lands on a whole-cell multiple, so
        // the vertices keep world positions they already had.
        assert!(ocean.recenter_toward(Vec3::new(400.0, 0.0, -320.0)));
        let cell = 600.0 / 224.0;
        assert!(
            (ocean.center().x / cell).fract().abs() < 1e-4,
            "the centre is not on the cell grid: {:?}",
            ocean.center()
        );
        assert!(
            (ocean.center().y / cell).fract().abs() < 1e-4,
            "the centre is not on the cell grid: {:?}",
            ocean.center()
        );
        let expected = Vec2::new((400.0 / cell).round() * cell, (-320.0 / cell).round() * cell);
        assert!(
            ocean.center().abs_diff_eq(expected, 1e-3),
            "{:?} vs {:?}",
            ocean.center(),
            expected
        );
    }

    #[test]
    fn the_wave_scale_moves_the_whole_surface() {
        let mut ocean = Ocean::new(40.0, 4, -0.9);
        // Sample the source field directly: with Gerstner displacement the
        // world point that maps to a given source moves with the scale, so a
        // fixed world query is no longer exactly linear.
        let point = Vec3::new(3.0, 0.0, 7.0);
        let calm = ocean.waves().get_height(point);

        ocean.waves_mut().set_height_scale(2.0);
        let rough = ocean.waves().get_height(point);
        assert!(
            (rough - 2.0 * calm).abs() < 1e-5,
            "calm {calm}, rough {rough}"
        );
    }

    #[test]
    fn the_crest_colour_is_brighter_than_the_trough() {
        assert!(OCEAN_CREST_COLOR.length() > OCEAN_DEEP_COLOR.length());
    }
}
