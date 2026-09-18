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
pub enum ShadowQuality {
    Off,
    Low,
    Medium,
    High,
    Ultra,
}

impl Default for ShadowQuality {
    fn default() -> Self {
        Self::Medium
    }
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

    pub fn update(&mut self, _camera_view: Mat4, _camera_proj: Mat4, cascade_distances: &[f32]) {
        // Update cascade matrices
        // This is a simplified version - actual implementation would:
        // 1. Calculate frustum corners in world space
        // 2. For each cascade, calculate a bounding box
        // 3. Calculate view and projection matrices for each cascade

        for i in 0..self.config.cascade_count {
            // Calculate light view matrix for this cascade
            // This would position the light to cover the cascade's area

            // For now, just use a simple orthographic projection
            let near = if i == 0 {
                0.1
            } else {
                cascade_distances[i - 1]
            };
            let far = cascade_distances[i];

            // Create orthographic projection
            let proj = Mat4::orthographic_rh_gl(-100.0, 100.0, -100.0, 100.0, near, far);

            // Create view matrix (looking in light direction)
            let light_pos = Vec3::ZERO; // Would be calculated based on cascade
            let view = Mat4::look_at_rh(light_pos, light_pos + self.light_direction, Vec3::Y);

            self.light_view_matrices[i] = view;
            self.light_proj_matrices[i] = proj;
            self.light_view_proj_matrices[i] = proj * view;
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