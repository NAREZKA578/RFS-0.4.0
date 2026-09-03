//! Vulkan image registry.
//!
//! The RHI `CommandEncoder::begin_render_pass` identifies attachments only by a
//! `u64` native texture handle, and the encoder has no other reference to the
//! textures. To build real `VkRenderingAttachmentInfo`/`VkFramebuffer` we keep a
//! process-global registry mapping each `VkImage` handle to the image-view,
//! sampler, extent and format information owned by `VulkanTexture`.
//!
//! Every `VulkanTexture` registers itself on creation and unregisters on drop,
//! so entries never dangle. Lookups are infallible for live textures.

use crate::rhi::texture::{TextureFormat, TextureUsage};
use ash::vk;
use ash::vk::Handle;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Immutable snapshot of a live texture needed to set up a render pass.
#[derive(Debug, Clone, Copy)]
pub struct RegisteredImage {
    /// The VkImage itself (for copy operations).
    pub image: vk::Image,
    /// VkImageView of the texture.
    pub view: vk::ImageView,
    /// VkSampler of the texture (may be null for render/depth targets).
    pub sampler: vk::Sampler,
    /// Image width.
    pub width: u32,
    /// Image height.
    pub height: u32,
    /// Image depth (1 for 2D).
    #[allow(dead_code)]
    pub depth: u32,
    /// RHI texture format.
    pub format: TextureFormat,
    /// RHI texture usage.
    pub usage: TextureUsage,
    /// Aspect mask derived from the format.
    pub aspect: vk::ImageAspectFlags,
    /// Number of array layers.
    pub array_layers: u32,
}

/// The process-global image registry.
fn registry() -> &'static Mutex<HashMap<u64, RegisteredImage>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, RegisteredImage>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The process-global "default render target" selection.
///
/// The RHI expresses the on-screen target as `texture_handle == 0` (OpenGL
/// "default framebuffer" semantics). Under Vulkan there is no default
/// framebuffer: the app acquires a swapchain image and, using this module,
/// designates it (and an optional depth target) as the default target. The
/// command encoder then resolves handle `0` to these.
fn target_state() -> &'static Mutex<TargetState> {
    static STATE: OnceLock<Mutex<TargetState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(TargetState::default()))
}

/// Default color/depth targets used for `texture_handle == 0`.
#[derive(Debug, Default, Clone)]
struct TargetState {
    color: u64,
    depth: u64,
}

/// Sets the default color target (the current swapchain image handle).
pub fn set_default_color_target(handle: u64) {
    if let Ok(mut s) = target_state().lock() {
        s.color = handle;
    }
}

/// Sets the default depth target (an off-screen depth texture handle).
pub fn set_default_depth_target(handle: u64) {
    if let Ok(mut s) = target_state().lock() {
        s.depth = handle;
    }
}

/// Returns the default color target, resolved to a registered image (if any).
pub fn lookup_default_color() -> Option<RegisteredImage> {
    let handle = target_state().lock().ok()?.color;
    if handle == 0 {
        return None;
    }
    lookup(handle)
}

/// Returns the default depth target, resolved to a registered image (if any).
pub fn lookup_default_depth() -> Option<RegisteredImage> {
    let handle = target_state().lock().ok()?.depth;
    if handle == 0 {
        return None;
    }
    lookup(handle)
}

/// A registered buffer (raw handle → VkBuffer).
fn buffer_registry() -> &'static Mutex<HashMap<u64, vk::Buffer>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, vk::Buffer>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A registered pipeline (raw handle → pipeline/layout/descriptor info).
#[derive(Debug, Clone, Copy)]
pub struct RegisteredPipeline {
    pub pipeline: vk::Pipeline,
    pub layout: vk::PipelineLayout,
    pub descriptor_set_layout: vk::DescriptorSetLayout,
    /// Number of texture (combined image sampler) bindings in the descriptor
    /// set layout (bindings `0..texture_bindings`).
    pub texture_bindings: u32,
}

fn pipeline_registry() -> &'static Mutex<HashMap<u64, RegisteredPipeline>> {
    static REGISTRY: OnceLock<Mutex<HashMap<u64, RegisteredPipeline>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Registers a VkBuffer under its raw handle.
pub fn register_buffer(buffer: vk::Buffer) {
    if let Ok(mut map) = buffer_registry().lock() {
        map.insert(buffer.as_raw(), buffer);
    }
}

/// Removes a buffer from the registry.
pub fn unregister_buffer(buffer: vk::Buffer) {
    if let Ok(mut map) = buffer_registry().lock() {
        map.remove(&buffer.as_raw());
    }
}

/// Looks up a VkBuffer by raw handle.
pub fn lookup_buffer(raw_handle: u64) -> Option<vk::Buffer> {
    buffer_registry().lock().ok()?.get(&raw_handle).copied()
}

/// Registers a pipeline.
pub fn register_pipeline(
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    descriptor_set_layout: vk::DescriptorSetLayout,
    texture_bindings: u32,
) {
    if let Ok(mut map) = pipeline_registry().lock() {
        map.insert(
            pipeline.as_raw(),
            RegisteredPipeline {
                pipeline,
                layout,
                descriptor_set_layout,
                texture_bindings,
            },
        );
    }
}

/// Removes a pipeline from the registry.
pub fn unregister_pipeline(pipeline: vk::Pipeline) {
    if let Ok(mut map) = pipeline_registry().lock() {
        map.remove(&pipeline.as_raw());
    }
}

/// Looks up a pipeline by raw handle.
pub fn lookup_pipeline(raw_handle: u64) -> Option<RegisteredPipeline> {
    pipeline_registry().lock().ok()?.get(&raw_handle).copied()
}

/// Looks up the descriptor set layout for a given pipeline layout handle.
/// (Used by the command encoder to bind texture descriptor sets.)
pub fn lookup_descriptor_set_layout(layout: vk::PipelineLayout) -> vk::DescriptorSetLayout {
    if let Ok(map) = pipeline_registry().lock() {
        for (_, p) in map.iter() {
            if p.layout == layout {
                return p.descriptor_set_layout;
            }
        }
    }
    vk::DescriptorSetLayout::null()
}

/// Registers an image under its raw `VkImage` handle.
pub fn register(image: vk::Image, info: RegisteredImage) {
    if image.is_null() {
        return;
    }
    if let Ok(mut map) = registry().lock() {
        map.insert(image.as_raw(), info);
    }
}

/// Removes an image from the registry (called by `VulkanTexture::drop`).
pub fn unregister(image: vk::Image) {
    if let Ok(mut map) = registry().lock() {
        map.remove(&image.as_raw());
    }
}

/// Looks up a registered image by raw handle.
pub fn lookup(raw_handle: u64) -> Option<RegisteredImage> {
    registry().lock().ok()?.get(&raw_handle).copied()
}

/// Looks up a registered image by `VkImage`.
#[allow(dead_code)]
pub fn lookup_vk(image: vk::Image) -> Option<RegisteredImage> {
    lookup(image.as_raw())
}

/// Clears the registry (used by tests).
#[allow(dead_code)]
pub fn clear() {
    if let Ok(mut map) = registry().lock() {
        map.clear();
    }
}
