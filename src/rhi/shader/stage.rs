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
pub const GEOMETRY_SHADER: ShaderStage = ShaderStage::GEOMETRY;
pub const TESSELLATION_CONTROL_SHADER: ShaderStage = ShaderStage::TESSELLATION_CONTROL;
pub const TESSELLATION_EVALUATION_SHADER: ShaderStage = ShaderStage::TESSELLATION_EVALUATION;
pub const MESH_SHADER: ShaderStage = ShaderStage::MESH;
pub const TASK_SHADER: ShaderStage = ShaderStage::TASK;
pub const CALLABLE_SHADER: ShaderStage = ShaderStage::CALLABLE;

#[cfg(test)]
mod bug_126 {
    use super::*;

    // Bug №126: the named stage constants must reach the new bits.
    #[test]
    fn named_stage_consts_match_the_new_bits() {
        assert_eq!(GEOMETRY_SHADER, ShaderStage::GEOMETRY);
        assert_eq!(TESSELLATION_CONTROL_SHADER, ShaderStage::TESSELLATION_CONTROL);
        assert_eq!(TESSELLATION_EVALUATION_SHADER, ShaderStage::TESSELLATION_EVALUATION);
        assert_eq!(MESH_SHADER, ShaderStage::MESH);
        assert_eq!(TASK_SHADER, ShaderStage::TASK);
        assert_eq!(CALLABLE_SHADER, ShaderStage::CALLABLE);
    }
}
