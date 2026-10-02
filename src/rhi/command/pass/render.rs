//! Render Pass
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

use crate::resource::{GpuResource, TextureLayout, TextureView};
use crate::types::*;
use bitflags::bitflags;

/// Load operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LoadOp {
    #[default]
    Load,
    Clear,
    DontCare,
}

/// Store operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StoreOp {
    #[default]
    Store,
    DontCare,
}

/// Attachment description
#[derive(Debug, Clone, Default)]
pub struct AttachmentDescription {
    pub format: Format,
    pub samples: SampleCount,
    pub load_op: LoadOp,
    pub store_op: StoreOp,
    pub stencil_load_op: LoadOp,
    pub stencil_store_op: StoreOp,
    pub initial_layout: TextureLayout,
    pub final_layout: TextureLayout,
}

/// Attachment reference
#[derive(Debug, Clone, Default)]
pub struct AttachmentReference {
    pub attachment: u32,
    pub layout: TextureLayout,
}

/// Pipeline bind point
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PipelineBindPoint {
    #[default]
    Graphics,
    Compute,
}

// Subpass flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct SubpassFlags: u32 {
        const NONE = 0;
    }
}

/// Subpass description
#[derive(Debug, Clone, Default)]
pub struct SubpassDescription {
    pub flags: SubpassFlags,
    pub pipeline_bind_point: PipelineBindPoint,
    pub input_attachments: Vec<AttachmentReference>,
    pub color_attachments: Vec<AttachmentReference>,
    pub resolve_attachments: Vec<AttachmentReference>,
    pub depth_stencil_attachment: Option<AttachmentReference>,
    pub preserve_attachments: Vec<u32>,
}

// Dependency flags
bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub struct DependencyFlags: u32 {
        const BY_REGION = 1 << 0;
        const DEVICE_GROUP = 1 << 1;
        const VIEW_LOCAL = 1 << 2;
    }
}

/// Subpass dependency
#[derive(Debug, Clone, Default)]
pub struct SubpassDependency {
    pub src_subpass: u32,
    pub dst_subpass: u32,
    pub src_stage_mask: PipelineStage,
    pub dst_stage_mask: PipelineStage,
    pub src_access_mask: AccessFlags,
    pub dst_access_mask: AccessFlags,
    pub dependency_flags: DependencyFlags,
}

/// Render pass description
#[derive(Debug, Clone, Default)]
pub struct RenderPassDesc {
    pub attachments: Vec<AttachmentDescription>,
    pub subpasses: Vec<SubpassDescription>,
    pub dependencies: Vec<SubpassDependency>,
}

/// Render pass
#[derive(Debug, Clone, Default)]
pub struct RenderPass {
    desc: RenderPassDesc,
    pub(crate) backend: Option<GpuResource>,
}

impl RenderPass {
    pub fn new(desc: RenderPassDesc) -> Self {
        Self {
            desc,
            backend: None,
        }
    }

    /// Attaches a native handle produced by the active backend.
    pub(crate) fn set_backend(&mut self, backend: GpuResource) {
        self.backend = Some(backend);
    }

    /// Returns `true` when the render pass has a native backend handle.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }

    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    pub fn desc(&self) -> &RenderPassDesc {
        &self.desc
    }
}

/// Framebuffer description
#[derive(Debug, Clone, Default)]
pub struct FramebufferDesc {
    pub render_pass: RenderPass,
    pub attachments: Vec<FramebufferAttachment>,
    pub width: u32,
    pub height: u32,
    pub layers: u32,
}

/// Framebuffer attachment
#[derive(Debug, Clone)]
pub struct FramebufferAttachment {
    pub texture_view: TextureView,
    pub layer: u32,
    pub mip_level: u32,
}

/// Framebuffer
#[derive(Debug, Clone, Default)]
pub struct Framebuffer {
    desc: FramebufferDesc,
    pub(crate) backend: Option<GpuResource>,
}

impl Framebuffer {
    pub fn new(desc: FramebufferDesc) -> Self {
        Self {
            desc,
            backend: None,
        }
    }

    /// Attaches a native handle produced by the active backend.
    pub(crate) fn set_backend(&mut self, backend: GpuResource) {
        self.backend = Some(backend);
    }

    /// Returns `true` when the framebuffer has a native backend handle.
    pub fn has_gpu_backing(&self) -> bool {
        self.backend.is_some()
    }

    pub(crate) fn backend(&self) -> Option<GpuResource> {
        self.backend
    }

    pub fn desc(&self) -> &FramebufferDesc {
        &self.desc
    }
}

/// Render pass begin info
#[derive(Debug, Clone, Default)]
pub struct RenderPassBeginInfo {
    pub render_pass: RenderPass,
    pub framebuffer: Framebuffer,
    pub render_area: Rect2D,
    pub clear_values: Vec<ClearValue>,
}
