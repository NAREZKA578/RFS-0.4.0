//! Shader Stages
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::types::ShaderStage;

/// Shader stage names
pub const VERTEX_SHADER: ShaderStage = ShaderStage::VERTEX;
pub const FRAGMENT_SHADER: ShaderStage = ShaderStage::FRAGMENT;
pub const COMPUTE_SHADER: ShaderStage = ShaderStage::COMPUTE;
pub const RAY_GEN_SHADER: ShaderStage = ShaderStage::RAY_GEN;
pub const ANY_HIT_SHADER: ShaderStage = ShaderStage::ANY_HIT;
pub const CLOSEST_HIT_SHADER: ShaderStage = ShaderStage::CLOSEST_HIT;
pub const MISS_SHADER: ShaderStage = ShaderStage::MISS;
pub const INTERSECTION_SHADER: ShaderStage = ShaderStage::INTERSECTION;
