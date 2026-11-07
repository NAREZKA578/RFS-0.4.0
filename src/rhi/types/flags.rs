//! Bitflags for RHI states
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct QueueFlags: u32 {
        const GRAPHICS = 1 << 0;
        const COMPUTE = 1 << 1;
        const TRANSFER = 1 << 2;
        const SPARSE_BINDING = 1 << 3;
        const PRESENT = 1 << 4;
        const PROTECTED = 1 << 5;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct BufferUsage: u32 {
        const TRANSFER_SRC = 1 << 0;
        const TRANSFER_DST = 1 << 1;
        const UNIFORM = 1 << 2;
        const STORAGE = 1 << 3;
        const INDEX = 1 << 4;
        const VERTEX = 1 << 5;
        const INDIRECT = 1 << 6;
        const SHADER_DEVICE_ADDRESS = 1 << 7;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct TextureUsage: u32 {
        const TRANSFER_SRC = 1 << 0;
        const TRANSFER_DST = 1 << 1;
        const SAMPLED = 1 << 2;
        const STORAGE = 1 << 3;
        const COLOR_ATTACHMENT = 1 << 4;
        const DEPTH_STENCIL_ATTACHMENT = 1 << 5;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct TextureAspectFlags: u32 {
        const COLOR = 1 << 0;
        const DEPTH = 1 << 1;
        const STENCIL = 1 << 2;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct ShaderStage: u32 {
        const VERTEX = 1 << 0;
        const FRAGMENT = 1 << 1;
        const COMPUTE = 1 << 2;
        const RAY_GEN = 1 << 3;
        const ANY_HIT = 1 << 4;
        const CLOSEST_HIT = 1 << 5;
        const MISS = 1 << 6;
        const INTERSECTION = 1 << 7;
        /// Bug №126: geometry and tessellation stages were unnameable —
        /// features.rs declares `geometry_shader` / `tessellation_shader` /
        /// `mesh_shader`, but no `ShaderStage` bit could say "I run in that
        /// stage", so a mesh- or geometry-stage pipeline could not be
        /// described at all.
        const GEOMETRY = 1 << 8;
        const TESSELLATION_CONTROL = 1 << 9;
        const TESSELLATION_EVALUATION = 1 << 10;
        const MESH = 1 << 11;
        const TASK = 1 << 12;
        /// Callable shaders run in a ray pipeline (SBT calls), not raster.
        const CALLABLE = 1 << 13;
        /// The graphics-family stage set. Mesh stages replace the vertex
        /// stage inside a graphics pipeline, so they belong here;
        /// INTERSECTION/CALLABLE are ray-pipeline stages and stay out.
        const ALL_GRAPHICS = (1 << 0)
            | (1 << 1)
            | (1 << 8)
            | (1 << 9)
            | (1 << 10)
            | (1 << 11)
            | (1 << 12);
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct PipelineStage: u64 {
        const TOP_OF_PIPE = 1 << 0;
        const DRAW_INDIRECT = 1 << 1;
        const VERTEX_INPUT = 1 << 2;
        const VERTEX_SHADER = 1 << 3;
        const FRAGMENT_SHADER = 1 << 4;
        const COLOR_ATTACHMENT_OUTPUT = 1 << 5;
        const COMPUTE_SHADER = 1 << 6;
        const TRANSFER = 1 << 7;
    const BOTTOM_OF_PIPE = 1 << 8;
    const HOST = 1 << 9;
    /// Bug №293: depth and stencil testing happens here, not in the fragment
    /// shader. Without this stage there is no legal `dst_stage` for a barrier that
    /// makes a depth write visible — naming `FRAGMENT_SHADER` instead is
    /// `VUID-vkCmdPipelineBarrier-pImageMemoryBarriers-02820`, and naming
    /// `COLOR_ATTACHMENT_OUTPUT` is the same error. Both are how a render graph
    /// ends up unable to synchronise its depth buffer at all.
    const EARLY_FRAGMENT_TESTS = 1 << 10;
    /// Depth and stencil tests that write late. Kept separate because a barrier
    /// for the depth *write* and one for the depth *read* want different stages.
    const LATE_FRAGMENT_TESTS = 1 << 11;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct AccessFlags: u64 {
        const NONE = 0;
        const INDIRECT_COMMAND_READ = 1 << 0;
        const INDEX_READ = 1 << 1;
        const VERTEX_ATTRIBUTE_READ = 1 << 2;
        const UNIFORM_READ = 1 << 3;
        const SHADER_READ = 1 << 4;
        const SHADER_WRITE = 1 << 5;
        const COLOR_ATTACHMENT_READ = 1 << 6;
        const COLOR_ATTACHMENT_WRITE = 1 << 7;
        const DEPTH_STENCIL_ATTACHMENT_READ = 1 << 8;
        const DEPTH_STENCIL_ATTACHMENT_WRITE = 1 << 9;
        const TRANSFER_READ = 1 << 10;
        const TRANSFER_WRITE = 1 << 11;
        const HOST_READ = 1 << 12;
        const HOST_WRITE = 1 << 13;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct MemoryPropertyFlags: u32 {
        const DEVICE_LOCAL = 1 << 0;
        const HOST_VISIBLE = 1 << 1;
        const HOST_COHERENT = 1 << 2;
        const HOST_CACHED = 1 << 3;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct MemoryHeapFlags: u32 {
        const DEVICE_LOCAL = 1 << 0;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct PipelineFlags: u32 {
        const NONE = 0;
        const DISABLE_OPTIMIZATION = 1 << 0;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct DynamicState: u32 {
        const VIEWPORT = 1 << 0;
        const SCISSOR = 1 << 1;
        const BLEND_CONSTANTS = 1 << 2;
    }
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
    pub struct ColorComponentFlags: u32 {
        const R = 1 << 0;
        const G = 1 << 1;
        const B = 1 << 2;
        const A = 1 << 3;
    }
}

#[cfg(test)]
mod bug_126 {
    use super::*;

    // Bug №126: geometry / tessellation / mesh / task / callable stages
    // used to be unnameable — no `ShaderStage` bit existed for any of them.
    #[test]
    fn extended_stage_bits_are_distinct_and_nonzero() {
        assert_ne!(ShaderStage::GEOMETRY, ShaderStage::empty());
        assert_ne!(ShaderStage::TESSELLATION_CONTROL, ShaderStage::empty());
        assert_ne!(ShaderStage::TESSELLATION_EVALUATION, ShaderStage::empty());
        assert_ne!(ShaderStage::MESH, ShaderStage::empty());
        assert_ne!(ShaderStage::TASK, ShaderStage::empty());
        assert_ne!(ShaderStage::CALLABLE, ShaderStage::empty());

        let combined = ShaderStage::GEOMETRY
            | ShaderStage::TESSELLATION_CONTROL
            | ShaderStage::TESSELLATION_EVALUATION
            | ShaderStage::MESH
            | ShaderStage::TASK
            | ShaderStage::CALLABLE;
        assert_eq!(combined.bits().count_ones(), 6);
    }

    #[test]
    fn all_graphics_covers_mesh_family_and_excludes_ray_stages() {
        let all = ShaderStage::ALL_GRAPHICS;
        assert!(all.contains(ShaderStage::VERTEX));
        assert!(all.contains(ShaderStage::FRAGMENT));
        assert!(all.contains(ShaderStage::GEOMETRY));
        assert!(all.contains(ShaderStage::MESH));
        assert!(all.contains(ShaderStage::TASK));
        // Ray-pipeline stages stay out of the raster family.
        assert!(!all.contains(ShaderStage::INTERSECTION));
        assert!(!all.contains(ShaderStage::CALLABLE));
    }
}
