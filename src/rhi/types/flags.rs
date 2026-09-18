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
        const ALL_GRAPHICS = (1 << 0) | (1 << 1);
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
