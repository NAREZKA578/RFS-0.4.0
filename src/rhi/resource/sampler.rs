//! Sampler Resources
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use serde::{Deserialize, Serialize};

/// Filter mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum FilterMode {
    #[default]
    Nearest,
    Linear,
}

/// Address mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AddressMode {
    #[default]
    Repeat,
    MirroredRepeat,
    ClampToEdge,
    ClampToBorder,
    MirrorClampToEdge,
}

/// Compare operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum CompareOp {
    #[default]
    Never,
    Less,
    Equal,
    LessOrEqual,
    Greater,
    NotEqual,
    GreaterOrEqual,
    Always,
}

/// Border color
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BorderColor {
    #[default]
    FloatTransparentBlack,
    IntTransparentBlack,
    FloatOpaqueBlack,
    IntOpaqueBlack,
    FloatOpaqueWhite,
    IntOpaqueWhite,
}

/// Sampler description
#[derive(Debug, Clone, Default)]
pub struct SamplerDesc {
    pub mag_filter: FilterMode,
    pub min_filter: FilterMode,
    pub mipmap_mode: FilterMode,
    pub address_mode_u: AddressMode,
    pub address_mode_v: AddressMode,
    pub address_mode_w: AddressMode,
    pub mip_lod_bias: f32,
    pub max_anisotropy: f32,
    pub compare_enable: bool,
    pub compare_op: CompareOp,
    pub min_lod: f32,
    pub max_lod: f32,
    pub border_color: BorderColor,
    pub unnormalized_coordinates: bool,
}

impl SamplerDesc {
    /// Linear filtered sampler with clamping.
    pub fn linear_clamp() -> Self {
        Self {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_mode: FilterMode::Linear,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            max_lod: f32::MAX,
            ..Self::default()
        }
    }

    /// Linear filtered sampler with repeat wrapping.
    pub fn linear_repeat() -> Self {
        Self {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_mode: FilterMode::Linear,
            max_lod: f32::MAX,
            ..Self::default()
        }
    }

    /// Anisotropic sampler with repeat wrapping.
    pub fn anisotropic(max_anisotropy: f32) -> Self {
        Self {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_mode: FilterMode::Linear,
            max_anisotropy,
            max_lod: f32::MAX,
            ..Self::default()
        }
    }

    /// Nearest filtered sampler with clamping (good for UI / pixel art).
    pub fn nearest_clamp() -> Self {
        Self {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            ..Self::default()
        }
    }

    /// Depth comparison sampler for shadow mapping.
    pub fn shadow_compare(compare_op: CompareOp) -> Self {
        Self {
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_mode: FilterMode::Nearest,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            compare_enable: true,
            compare_op,
            max_lod: f32::MAX,
            ..Self::default()
        }
    }

    /// Returns `true` when LOD clamping is consistent. Only one custom border
    /// color mode can be active when clamping is enabled.
    pub fn is_valid(&self) -> bool {
        self.min_lod >= 0.0
            && self.max_lod >= self.min_lod
            && self.max_anisotropy >= 0.0
            && !(self.unnormalized_coordinates
                && self.address_mode_u == AddressMode::ClampToBorder)
    }
}

/// Sampler resource
#[derive(Debug, Clone)]
pub struct Sampler {
    desc: SamplerDesc,
}

impl Sampler {
    pub fn new(desc: SamplerDesc) -> Self {
        Self { desc }
    }
    pub fn from_desc(desc: SamplerDesc) -> Self {
        Self::new(desc)
    }

    pub fn desc(&self) -> &SamplerDesc {
        &self.desc
    }

    /// Returns `true` when the sampler performs depth comparison.
    pub fn is_comparison(&self) -> bool {
        self.desc.compare_enable
    }

    /// Returns `true` when the sampler uses any linear filter.
    pub fn is_linear(&self) -> bool {
        self.desc.mag_filter == FilterMode::Linear
            || self.desc.min_filter == FilterMode::Linear
            || self.desc.mipmap_mode == FilterMode::Linear
    }

    /// Returns `true` when the sampler is anisotropic.
    pub fn is_anisotropic(&self) -> bool {
        self.desc.max_anisotropy > 0.0
    }
}
