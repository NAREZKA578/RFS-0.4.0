//! The depth buffer the frame's passes share.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! In a forward renderer the opaque geometry decides what is in front of
//! what, and the transparent objects and the water have to be tested against
//! **that** depth. Each pass making its own depth buffer is the mistake: the
//! transparent pass clears its own depth to the far plane, so every transparent
//! object is in front of every other, and a transparent object behind a wall
//! is drawn over the wall.
//!
//! So the depth buffer is one object that the passes share: the forward pass
//! writes it, the transparent and the water passes attach it read-only.
//!
//! The handle is shared rather than the texture, because the texture is
//! created when the size is known — and the size is known only when the frame
//! is being recorded, by which time the other passes have to be asking for
//! the view already.
//!
//! The layout is part of the contract, and it is a strict one. The forward
//! pass leaves the depth in `DEPTH_STENCIL_ATTACHMENT_OPTIMAL`, and the
//! passes that read it expect it there. A pass that attaches the depth
//! without the forward pass having written it would be reading an image
//! whose layout nobody set, and the validator would say so — so the passes
//! that read the depth check that it has been written and skip themselves
//! with a reason if it has not.

use std::sync::{Arc, Mutex};

use crate::rhi::{Device, Format, Texture, TextureUsage, TextureView};

/// The state of the shared depth buffer.
#[derive(Default)]
struct DepthTargetState {
    texture: Option<Texture>,
    view: Option<TextureView>,
    width: u32,
    height: u32,
    /// Whether a pass has written the depth.
    ///
    /// The passes that read the depth need to know that there is something to
    /// read: an image nobody has written has no layout and no contents, and
    /// the depth test against it is a test against nothing.
    written: bool,
    /// How many times the buffer has been recreated.
    ///
    /// A framebuffer holds the *view* of the depth, and a view of a
    /// destroyed image is not a view of anything. The number tells a pass
    /// that the framebuffer it built has to be rebuilt.
    generation: u64,
}

/// The depth buffer the frame's passes share.
///
/// Cheap to clone, and the clones are the same buffer: the handle is the
/// thing, not the texture.
#[derive(Clone, Default)]
pub struct DepthTarget {
    state: Arc<Mutex<DepthTargetState>>,
}

impl DepthTarget {
    /// A depth buffer of no size yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The depth buffer's view, created or resized as needed.
    ///
    /// The buffer is created for the size asked for, and recreated when the
    /// size changes: a framebuffer's attachments must all be the size of the
    /// render area, and a framebuffer built for the old size draws into an
    /// image of the old size.
    pub fn view(&self, device: &Device, width: u32, height: u32) -> Option<TextureView> {
        if width == 0 || height == 0 {
            return None;
        }
        let Ok(mut state) = self.state.lock() else {
            return None;
        };
        let stale = state
            .texture
            .as_ref()
            .is_none_or(|texture| texture.width() != width || texture.height() != height);
        if stale {
            let texture = device.create_texture(
                width,
                height,
                1,
                Format::D32_SFLOAT,
                TextureUsage::DEPTH_STENCIL_ATTACHMENT,
                1,
            );
            let view = match device.create_texture_view(
                &texture,
                &crate::rhi::TextureViewDesc {
                    texture: texture.clone(),
                    format: None,
                    view_type: crate::rhi::TextureViewType::D2,
                    aspects: crate::rhi::TextureAspectFlags::DEPTH,
                    base_mip_level: 0,
                    mip_level_count: 1,
                    base_array_layer: 0,
                    array_layer_count: 1,
                },
            ) {
                Ok(view) => view,
                Err(e) => {
                    crate::error!("depth", "the depth view could not be created: {e}");
                    return None;
                }
            };
            state.texture = Some(texture);
            state.view = Some(view);
            state.width = width;
            state.height = height;
            // The new image has no contents and no layout: whoever writes it
            // first says so.
            state.written = false;
            state.generation += 1;
        }
        state.view.clone()
    }

    /// How many times the buffer has been recreated.
    ///
    /// A framebuffer holds the depth's *view*, and a view of a destroyed
    /// image is not a view of anything. A pass that cached a framebuffer
    /// rebuilds it when this number changes.
    pub fn generation(&self) -> u64 {
        self.state.lock().map(|state| state.generation).unwrap_or(0)
    }

    /// The depth buffer's texture, if it has been created.
    pub fn texture(&self) -> Option<Texture> {
        self.state.lock().ok()?.texture.clone()
    }

    /// Note that a pass has written the depth.
    ///
    /// The passes that read the depth need to know that there is something to
    /// read: an image nobody has written has no layout and no contents.
    pub fn mark_written(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.written = true;
        }
    }

    /// Whether the depth has been written.
    pub fn is_written(&self) -> bool {
        self.state.lock().map(|state| state.written).unwrap_or(false)
    }

    /// The size the depth buffer is currently at.
    pub fn extent(&self) -> (u32, u32) {
        self.state
            .lock()
            .map(|state| (state.width, state.height))
            .unwrap_or((0, 0))
    }
}
