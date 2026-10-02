//! Shadows
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::rhi::resource::{Texture, TextureDesc, TextureView, TextureViewDesc, TextureViewType};
use crate::rhi::types::{
    Format, SampleCount, TextureAspectFlags, TextureDimensions, TextureUsage,
};
use glam::{Mat4, Vec3};

/// Shadow configuration
#[derive(Debug, Clone)]
pub struct ShadowConfig {
    pub enabled: bool,
    pub resolution: u32,
    pub bias: f32,
    pub normal_bias: f32,
    pub quality: ShadowQuality,
}

impl Default for ShadowConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            resolution: 2048,
            bias: 0.0001,
            normal_bias: 0.001,
            quality: ShadowQuality::Medium,
        }
    }
}

/// Shadow quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[derive(Default)]
pub enum ShadowQuality {
    Off,
    Low,
    #[default]
    Medium,
    High,
    Ultra,
}


/// Cascaded shadow configuration
#[derive(Debug, Clone)]
pub struct CascadedShadowConfig {
    pub cascade_count: usize,
    pub cascade_distances: Vec<f32>,
    pub cascade_resolutions: Vec<u32>,
    pub transition_factor: f32,
}

impl Default for CascadedShadowConfig {
    fn default() -> Self {
        Self {
            cascade_count: 4,
            cascade_distances: vec![10.0, 50.0, 100.0, 200.0],
            cascade_resolutions: vec![2048; 4],
            transition_factor: 0.1,
        }
    }
}

/// Smallest cascade half-extent. A cascade whose depth range is degenerate
/// would otherwise produce a zero-sized box, and an orthographic projection
/// with left == right yields a singular matrix.
pub const MIN_CASCADE_RADIUS: f32 = 0.5;
/// Largest cascade half-extent, so a bad `cascade_distances` cannot ask for a
/// shadow map covering the whole world and destroy shadow-map resolution.
pub const MAX_CASCADE_RADIUS: f32 = 10_000.0;

/// An up-vector that is never parallel to `direction`.
///
/// Bug №184: `look_at` builds its basis from `forward.cross(up)`. If `up` is
/// parallel to `forward` that cross product is zero, the normalisation divides
/// by zero, and the resulting matrix is all NaN — which propagates into every
/// shaded pixel. This picks the least-aligned world axis instead.
pub fn safe_up(direction: Vec3) -> Vec3 {
    let d = direction.normalize();
    // Absolute components tell us how close we are to each axis.
    let ax = d.x.abs();
    let ay = d.y.abs();
    let az = d.z.abs();
    // Choose the axis we are *least* aligned with.
    if ax <= ay && ax <= az {
        Vec3::X
    } else if ay <= az {
        Vec3::Y
    } else {
        Vec3::Z
    }
}

/// True when every element of the matrix is finite.
fn is_finite_mat4(m: &Mat4) -> bool {
    m.as_ref().iter().all(|v| v.is_finite())
}

/// Camera world position and forward axis, recovered from a view matrix.
///
/// A view matrix maps world space into view space, where the camera sits at the
/// origin looking down -Z. Inverting that by hand avoids needing the general
/// matrix inverse: for a rigid view matrix the camera position is
/// `-(R^T * t)`, and the forward axis is the third basis column of `R`.
pub fn camera_basis(view: Mat4) -> (Vec3, Vec3) {
    // A view matrix maps world into view space, where the camera sits at the
    // origin looking down -Z. The inverse of that mapping therefore has the
    // camera position in its translation and the world-space axes in its
    // columns. Going through the inverse avoids depending on the memory
    // layout of the matrix, which is exactly the mistake an earlier hand-rolled
    // version of this made.
    // glam's `inverse` returns a plain Mat4 (it yields NaNs for a singular
    // matrix rather than an Option), so the finiteness check below is the guard.
    let inv = view.inverse();
    if !inv.is_finite() {
        return (Vec3::ZERO, Vec3::NEG_Z);
    }

    // glam exposes the columns; the 4th one is the translation.
    let pos = inv.w_axis.truncate();
    // The camera looks down its own -Z, i.e. against the inverse's Z column.
    let mut forward = -inv.z_axis.truncate().normalize();
    if !forward.is_finite() || forward.length_squared() <= f32::EPSILON {
        forward = Vec3::NEG_Z;
    }
    if !pos.is_finite() {
        return (Vec3::ZERO, forward);
    }
    (pos, forward)
}

/// Aspect ratio (width / height) encoded in a projection matrix.
///
/// For a standard right-handed perspective matrix, `m[0] = 1/(aspect * tan(fovy/2))`
/// and `m[5] = 1/tan(fovy/2)`, so the ratio `m[5] / m[0]` is the aspect. Returns
/// 1.0 for a degenerate or non-finite matrix rather than propagating a bad
/// value into the cascade box.
fn projection_aspect(proj: Mat4) -> f32 {
    let m = proj.to_cols_array();
    let x = m[0];
    let y = m[5];
    if x.abs() <= f32::EPSILON || !x.is_finite() || !y.is_finite() {
        return 1.0;
    }
    let aspect = y / x;
    if !aspect.is_finite() || aspect <= 0.0 {
        1.0
    } else {
        aspect.clamp(0.1, 10.0)
    }
}

/// Cascaded shadow map
pub struct CascadedShadowMap {
    pub config: CascadedShadowConfig,
    /// Light direction
    pub light_direction: Vec3,
    /// Light view matrices (one per cascade)
    pub light_view_matrices: Vec<Mat4>,
    /// Light projection matrices (one per cascade)
    pub light_proj_matrices: Vec<Mat4>,
    /// Light view-projection matrices (one per cascade)
    pub light_view_proj_matrices: Vec<Mat4>,
    /// Shadow map textures (one per cascade)
    pub shadow_maps: Vec<Texture>,
    /// Shadow map views
    pub shadow_views: Vec<TextureView>,
}

impl CascadedShadowMap {
    pub fn new(config: CascadedShadowConfig, light_direction: Vec3) -> Self {
        let mut light_view_matrices = Vec::new();
        let mut light_proj_matrices = Vec::new();
        let mut light_view_proj_matrices = Vec::new();

        for _ in 0..config.cascade_count {
            light_view_matrices.push(Mat4::IDENTITY);
            light_proj_matrices.push(Mat4::IDENTITY);
            light_view_proj_matrices.push(Mat4::IDENTITY);
        }

        Self {
            config,
            light_direction: light_direction.normalize(),
            light_view_matrices,
            light_proj_matrices,
            light_view_proj_matrices,
            shadow_maps: Vec::new(),
            shadow_views: Vec::new(),
        }
    }

    pub fn update(&mut self, camera_view: Mat4, camera_proj: Mat4, cascade_distances: &[f32]) {
        // Guard short/unsorted slices (index OOB panic, degenerate near>=far).
        if cascade_distances.len() < self.config.cascade_count {
            return;
        }

        // Bug №184: the cascades were built around a hardcoded origin with a
        // fixed +/-100 box and never looked at the camera, so a shadow map only
        // covered the 200 m around world zero. Move the ship and its shadows
        // simply stopped existing. Each cascade is now centred on the point the
        // camera frustum reaches halfway through that slice, sized to the
        // slice's radius, so the maps follow the view.
        let (camera_pos, camera_forward) = camera_basis(camera_view);
        // The cascade box follows the view shape, not a square: a 16:9 frustum
        // fitted into a square box wastes shadow-map resolution on the sides.
        let aspect = projection_aspect(camera_proj);

        for i in 0..self.config.cascade_count {
            let near = if i == 0 {
                0.1
            } else {
                cascade_distances[i - 1]
            };
            let mut far = cascade_distances[i];
            if !near.is_finite() || !far.is_finite() || far <= near {
                far = near + 1.0;
            }

            // Centre of this slice, halfway along the camera's forward axis.
            let mid_depth = (near + far) * 0.5;
            let slice_centre = camera_pos + camera_forward * mid_depth;

            // Radius that covers the slice: half its depth, clamped so a
            // pathological cascade range cannot produce a huge or zero box.
            let half = ((far - near) * 0.5).clamp(MIN_CASCADE_RADIUS, MAX_CASCADE_RADIUS);
            let half_x = (half * aspect).clamp(MIN_CASCADE_RADIUS, MAX_CASCADE_RADIUS);
            let half_y = half;

            // Create orthographic projection around the slice.
            // The near/far range is derived from the box rather than from the
            // cascade's own depth slice: an orthographic light has no frustum
            // corner to fit, so the box itself defines the covered depth, and
            // it must reach past the slice on both sides.
            let proj = Mat4::orthographic_rh_gl(
                -half_x,
                half_x,
                -half_y,
                half_y,
                -half * 2.0,
                half * 4.0,
            );

            // Create view matrix (looking in light direction).
            //
            // Bug №184: the view was always `look_at(ZERO, ZERO + dir, Y)`.
            // If the light direction is parallel to Y the forward and up
            // vectors are parallel, the cross product is zero, and the basis
            // collapses to NaN — which silently poisons every cascade matrix
            // and therefore the whole lighting result. `safe_up` picks a vector
            // that is never parallel to the light direction.
            let light_pos = slice_centre - self.light_direction * half * 2.0;
            let target = slice_centre;
            let up = safe_up(self.light_direction);
            let view = Mat4::look_at_rh(light_pos, target, up);

            let vp = proj * view;
            if !is_finite_mat4(&vp) {
                // A non-finite matrix is worse than a stale one: it propagates
                // NaN into every shaded pixel. Keep the previous value.
                continue;
            }

            self.light_view_matrices[i] = view;
            self.light_proj_matrices[i] = proj;
            self.light_view_proj_matrices[i] = vp;
        }
    }

    pub fn get_cascade_matrix(&self, cascade: usize) -> Mat4 {
        if cascade < self.light_view_proj_matrices.len() {
            self.light_view_proj_matrices[cascade]
        } else {
            Mat4::IDENTITY
        }
    }

    pub fn get_shadow_map(&self, cascade: usize) -> Option<&Texture> {
        self.shadow_maps.get(cascade)
    }

    pub fn get_shadow_view(&self, cascade: usize) -> Option<&TextureView> {
        self.shadow_views.get(cascade)
    }
}

/// Shadow map for point lights (omnidirectional)
pub struct OmnidirectionalShadowMap {
    pub resolution: u32,
    pub near_plane: f32,
    pub far_plane: f32,
    /// Cube map texture (6 faces)
    pub cube_map: Texture,
    /// Cube map views (6 faces)
    pub cube_views: Vec<TextureView>,
    /// View matrices (6 faces)
    pub view_matrices: [Mat4; 6],
    /// Projection matrix
    pub proj_matrix: Mat4,
}

impl OmnidirectionalShadowMap {
    pub fn new(resolution: u32, near: f32, far: f32) -> Self {
        // Create cube map
        let cube_map = Texture::new(TextureDesc {
            width: resolution,
            height: resolution,
            depth: 1,
            mip_levels: 1,
            array_layers: 6,
            format: Format::D32_SFLOAT,
            usage: TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::SAMPLED,
            sample_count: SampleCount::X1,
            dimensions: TextureDimensions::Cube,
            sharing_mode: Default::default(),
            queue_family_indices: vec![],
        });

        // Create views for each face
        let mut cube_views = Vec::new();
        for face in 0..6 {
            let view = cube_map.create_view(TextureViewDesc {
                texture: cube_map.clone(),
                format: None,
                view_type: TextureViewType::Cube,
                aspects: TextureAspectFlags::DEPTH,
                base_mip_level: 0,
                mip_level_count: 1,
                base_array_layer: face,
                array_layer_count: 1,
            });
            cube_views.push(view);
        }

        // Create view matrices for each face
        let view_matrices = [
            Mat4::look_at_rh(Vec3::ZERO, Vec3::X, Vec3::NEG_Y), // +X
            Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_X, Vec3::NEG_Y), // -X
            Mat4::look_at_rh(Vec3::ZERO, Vec3::Y, Vec3::Z),     // +Y
            Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Y, Vec3::NEG_Z), // -Y
            Mat4::look_at_rh(Vec3::ZERO, Vec3::Z, Vec3::NEG_Y), // +Z
            Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::NEG_Y), // -Z
        ];

        // Create projection matrix
        let proj_matrix = Mat4::perspective_rh_gl(
            std::f32::consts::PI / 2.0, // 90 degrees
            1.0,
            near,
            far,
        );

        Self {
            resolution,
            near_plane: near,
            far_plane: far,
            cube_map,
            cube_views,
            view_matrices,
            proj_matrix,
        }
    }

    pub fn get_view_matrix(&self, face: usize) -> Mat4 {
        if face < 6 {
            self.view_matrices[face]
        } else {
            Mat4::IDENTITY
        }
    }

    pub fn get_proj_matrix(&self) -> Mat4 {
        self.proj_matrix
    }

    pub fn get_view_proj_matrix(&self, face: usize) -> Mat4 {
        self.proj_matrix * self.get_view_matrix(face)
    }

    pub fn get_face_view(&self, face: usize) -> Option<&TextureView> {
        self.cube_views.get(face)
    }
}

/// Shadow map for spot lights
pub struct SpotLightShadowMap {
    pub resolution: u32,
    pub near_plane: f32,
    pub far_plane: f32,
    pub fov: f32,
    pub aspect_ratio: f32,
    /// Shadow map texture
    pub texture: Texture,
    /// Shadow map view
    pub view: TextureView,
    /// View matrix
    pub view_matrix: Mat4,
    /// Projection matrix
    pub proj_matrix: Mat4,
    /// View-projection matrix
    pub view_proj_matrix: Mat4,
}

impl SpotLightShadowMap {
    pub fn new(resolution: u32, fov: f32, aspect_ratio: f32, near: f32, far: f32) -> Self {
        let texture = Texture::new(TextureDesc {
            width: resolution,
            height: resolution,
            depth: 1,
            mip_levels: 1,
            array_layers: 1,
            format: Format::D32_SFLOAT,
            usage: TextureUsage::DEPTH_STENCIL_ATTACHMENT | TextureUsage::SAMPLED,
            sample_count: SampleCount::X1,
            dimensions: TextureDimensions::D2,
            sharing_mode: Default::default(),
            queue_family_indices: vec![],
        });

        let view = texture.create_view(TextureViewDesc {
            texture: texture.clone(),
            format: None,
            view_type: TextureViewType::D2,
            aspects: TextureAspectFlags::DEPTH,
            base_mip_level: 0,
            mip_level_count: 1,
            base_array_layer: 0,
            array_layer_count: 1,
        });

        let proj_matrix = Mat4::perspective_rh_gl(fov, aspect_ratio, near, far);

        Self {
            resolution,
            near_plane: near,
            far_plane: far,
            fov,
            aspect_ratio,
            texture,
            view,
            view_matrix: Mat4::IDENTITY,
            proj_matrix,
            view_proj_matrix: Mat4::IDENTITY,
        }
    }

    pub fn update(&mut self, position: Vec3, direction: Vec3, up: Vec3) {
        self.view_matrix = Mat4::look_at_rh(position, position + direction, up);
        self.view_proj_matrix = self.proj_matrix * self.view_matrix;
    }

    pub fn get_view_proj_matrix(&self) -> Mat4 {
        self.view_proj_matrix
    }
}