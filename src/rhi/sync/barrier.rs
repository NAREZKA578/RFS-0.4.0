//! Barriers
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::resource::texture::TextureLayout;
use crate::resource::{Buffer, Texture};
use crate::types::*;

/// Memory barrier
#[derive(Debug, Clone, Default)]
pub struct MemoryBarrier {
    pub src_access: AccessFlags,
    pub dst_access: AccessFlags,
}

/// Buffer barrier
#[derive(Debug, Clone, Default)]
pub struct BufferBarrier {
    pub buffer: Buffer,
    pub src_access: AccessFlags,
    pub dst_access: AccessFlags,
    pub offset: u64,
    pub size: u64,
}

/// Texture barrier
#[derive(Debug, Clone, Default)]
pub struct TextureBarrier {
    pub texture: Texture,
    pub src_access: AccessFlags,
    pub dst_access: AccessFlags,
    pub old_layout: TextureLayout,
    pub new_layout: TextureLayout,
}

/// Pipeline barrier
#[derive(Debug, Clone, Default)]
pub struct PipelineBarrier {
    pub src_stage: PipelineStage,
    pub dst_stage: PipelineStage,
    pub memory_barriers: Vec<MemoryBarrier>,
    pub buffer_barriers: Vec<BufferBarrier>,
    pub texture_barriers: Vec<TextureBarrier>,
}

/// Buffer copy region
#[derive(Debug, Clone, Default)]
pub struct BufferCopy {
    pub src_offset: u64,
    pub dst_offset: u64,
    pub size: u64,
}

/// Buffer to texture copy region
#[derive(Debug, Clone, Default)]
pub struct BufferTextureCopy {
    pub buffer_offset: u64,
    pub size: u64,
}
