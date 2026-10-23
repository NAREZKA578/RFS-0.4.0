//! Backend-neutral presentation types (Bug №663, variant A, by owner decision).
//!
//! Until this module existed, [`crate::backend::common::BackendResource`] and
//! the neutral [`crate::core::Device`] referred to
//! `crate::backend::vulkan::{WindowSurface, Swapchain, SwapchainRequest}`
//! directly: the raw vulkan types were shipped through the backend-neutral
//! seam, and the render layer had to name a backend module in its own
//! signatures. These types hold whatever the active backend produced and
//! dispatch statically, so the neutral API keeps its shape without carrying
//! backend types, and a non-Vulkan backend answers `NotSupported` instead of
//! failing to compile against a foreign struct.
//!
//! The runtime value surface stays backend-shaped where the neutral device
//! already was: semaphores are passed as `vk::Semaphore`, matching
//! `render_finished_semaphore` / `image_acquired_semaphore`, which return
//! Vulkan sync tokens at the neutral layer by design. Everything *owned* by
//! the caller — surface, chain, per-image handles, extent, format, request —
//! is neutral here.

use crate::ash::vk;
use ash::vk::Handle;
use crate::backend::vulkan::present as vulkan_present;
use crate::error::{RhiError, RhiResult};
use crate::types::{Extent2D, Format};

/// Which present mode the swapchain should use.
///
/// The request is negotiated rather than assumed: a surface that cannot
/// provide a mode gets the nearest thing that works instead of a failure
/// (FIFO is always available, so at worst the request degrades to FIFO).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PresentMode {
    /// Vertical blank synchronised, no tearing, no dropped-frame latency
    /// guarantee. The only mode every surface guarantees.
    #[default]
    Fifo,
    /// Like [`PresentMode::Fifo`], but the compositor may display mid-frame
    /// when it is late.
    FifoRelaxed,
    /// Fresh image each blank, exhausted images stay queued in a mailbox.
    Mailbox,
    /// Present immediately, may tear.
    Immediate,
}

impl PresentMode {
    /// The Vulkan spelling of the mode, for the backend that owns the chain.
    pub(crate) fn as_vulkan(self) -> vk::PresentModeKHR {
        match self {
            Self::Fifo => vk::PresentModeKHR::FIFO,
            Self::FifoRelaxed => vk::PresentModeKHR::FIFO_RELAXED,
            Self::Mailbox => vk::PresentModeKHR::MAILBOX,
            Self::Immediate => vk::PresentModeKHR::IMMEDIATE,
        }
    }
}

/// What the caller wants from the swapchain.
///
/// Availability is negotiated rather than assumed: a surface that cannot
/// provide a mode gets the nearest thing that works instead of a failure.
#[derive(Debug, Clone, Copy)]
pub struct SwapchainRequest {
    pub width: u32,
    pub height: u32,
    /// Preferred present mode; falls back to FIFO, the only guaranteed one.
    pub present_mode: PresentMode,
}

impl Default for SwapchainRequest {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            present_mode: PresentMode::default(),
        }
    }
}

/// One image from [`Swapchain::acquire`], plus the synchronisation that goes
/// with it.
///
/// There is no "still usable but rebuild soon" flag here. The acquire path
/// reports a suboptimal image as `RhiError::SwapchainOutOfDate` rather than
/// leaving two paths for the same condition, and this type carries no second
/// copy of that decision.
pub struct AcquiredImage {
    /// The image to render into.
    pub image_index: u32,
    /// Image view matching `image_index`, for building the render pass.
    pub image_view: u64,
}

/// A window surface, whichever backend built it.
///
/// The surface keeps its backend state (loaders, the owning instance) inside
/// the variant, so the neutral type is still an owner: dropping it destroys
/// the surface, in whatever way the backend does that. A
/// [`WindowSurfaceSource::NotSupported`] surface is a tombstone and never
/// reads into anything useful — `create_swapchain` on it fails with
/// `NotSupported`.
pub struct WindowSurface {
    source: WindowSurfaceSource,
}

enum WindowSurfaceSource {
    Vulkan(vulkan_present::WindowSurface),
    NotSupported,
}

impl WindowSurface {
    /// Wrap a surface the Vulkan backend produced.
    pub(crate) fn vulkan(inner: vulkan_present::WindowSurface) -> Self {
        Self {
            source: WindowSurfaceSource::Vulkan(inner),
        }
    }

    /// The Vulkan surface inside, when this surface belongs to the Vulkan
    /// backend.
    pub(crate) fn as_vulkan(&self) -> Option<&vulkan_present::WindowSurface> {
        match &self.source {
            WindowSurfaceSource::Vulkan(surface) => Some(surface),
            WindowSurfaceSource::NotSupported => None,
        }
    }
}

/// A live swapchain and the per-image resources sized to it, whichever
/// backend built it.
///
/// Like [`WindowSurface`], the chain is an owner: dropping it tears down the
/// per-image views and the chain itself the way the backend does. The public
/// methods below are the entire neutral surface — every caller keeps working
/// without ever naming a backend module.
pub struct Swapchain {
    source: SwapchainSource,
}

// The size difference between the variants is the design itself (Bug №663,
// variant A by owner decision): the live variant IS the backend object and
// dispatches statically, with no `Box<dyn>` indirection at presentation rate.
#[allow(clippy::large_enum_variant)]
enum SwapchainSource {
    Vulkan(vulkan_present::Swapchain),
    NotSupported,
}

impl Swapchain {
    /// Wrap a chain the Vulkan backend produced.
    pub(crate) fn vulkan(inner: vulkan_present::Swapchain) -> Self {
        Self {
            source: SwapchainSource::Vulkan(inner),
        }
    }

    /// The Vulkan chain inside, when this chain belongs to the Vulkan backend.
    pub(crate) fn as_vulkan_mut(&mut self) -> Option<&mut vulkan_present::Swapchain> {
        match &mut self.source {
            SwapchainSource::Vulkan(chain) => Some(chain),
            SwapchainSource::NotSupported => None,
        }
    }

    /// Take the next image to draw into. `timeout` is nanoseconds;
    /// `u64::MAX` waits indefinitely.
    ///
    /// `image_acquired` is signalled when the presentation engine has
    /// genuinely handed the image over; passing a null semaphore keeps the
    /// old blocking behaviour, which is only correct offscreen. The full
    /// semantics live in the backend implementation; this layer only
    /// dispatches to it and translates the per-image view handle to the
    /// neutral `u64`.
    pub fn acquire(
        &mut self,
        timeout: u64,
        image_acquired: vk::Semaphore,
    ) -> RhiResult<AcquiredImage> {
        match &mut self.source {
            SwapchainSource::Vulkan(chain) => {
                let acquired = chain.acquire(timeout, image_acquired)?;
                Ok(AcquiredImage {
                    image_index: acquired.image_index,
                    image_view: acquired.image_view.as_raw(),
                })
            }
            SwapchainSource::NotSupported => Err(RhiError::NotSupported(
                "this backend cannot acquire swapchain images".into(),
            )),
        }
    }

    /// Hand the finished image to the display.
    ///
    /// `render_finished` is the semaphore the submit signalled; a null one
    /// presents without waiting, which is the offscreen form.
    pub fn present(&self, image_index: u32, render_finished: vk::Semaphore) -> RhiResult<()> {
        match &self.source {
            SwapchainSource::Vulkan(chain) => chain.present(image_index, render_finished),
            SwapchainSource::NotSupported => Err(RhiError::NotSupported(
                "this backend cannot present".into(),
            )),
        }
    }

    /// The swapchain format expressed as an RHI format.
    ///
    /// A surface chooses the format, and a render pass has to be built for
    /// whatever it chose. `None` means the surface offered something the RHI
    /// cannot express, which is reported instead of being approximated.
    pub fn rhi_format(&self) -> Option<Format> {
        match &self.source {
            SwapchainSource::Vulkan(chain) => chain.rhi_format(),
            SwapchainSource::NotSupported => None,
        }
    }

    /// The extent the chain was built with, as an RHI value.
    ///
    /// This follows the window: a chain built for a resized window reports
    /// the resized extent from creation on.
    pub fn extent(&self) -> Extent2D {
        match &self.source {
            SwapchainSource::Vulkan(chain) => {
                Extent2D::new(chain.extent.width, chain.extent.height)
            }
            SwapchainSource::NotSupported => Extent2D::default(),
        }
    }

    /// Raw handles of the images in the chain, one per presentable image.
    ///
    /// The images stay owned by the chain; only the handles are copied, so
    /// a caller can publish them (or have them adopted as textures) without
    /// gaining a second owner.
    pub fn images(&self) -> Vec<u64> {
        match &self.source {
            SwapchainSource::Vulkan(chain) => chain
                .images
                .iter()
                .map(|image| image.as_raw())
                .collect(),
            SwapchainSource::NotSupported => Vec::new(),
        }
    }

    /// Raw handles of the per-image views the chain created, parallel to
    /// [`Swapchain::images`].
    ///
    /// Like the images above, the views stay owned by the chain.
    pub fn image_views(&self) -> Vec<u64> {
        match &self.source {
            SwapchainSource::Vulkan(chain) => chain
                .image_views
                .iter()
                .map(|view| view.as_raw())
                .collect(),
            SwapchainSource::NotSupported => Vec::new(),
        }
    }

    /// How many images the chain has. This is what a backend with a present
    /// semaphore per image needs, and it is only final when the chain exists.
    pub fn image_count(&self) -> usize {
        match &self.source {
            SwapchainSource::Vulkan(chain) => chain.images.len(),
            SwapchainSource::NotSupported => 0,
        }
    }
}

#[cfg(test)]
mod neutrality {
    use super::*;

    /// A chain constructed by no backend behaves like "no presentation": the
    /// sync calls refuse rather than panic or pretend, and the data accessors
    /// report emptiness. This is exactly what a caller on a backend that does
    /// not present sees, through the same neutral type it holds.
    #[test]
    fn a_tombstone_chain_refuses_and_reports_nothing() {
        let mut chain = Swapchain {
            source: SwapchainSource::NotSupported,
        };
        let acquired = chain.acquire(u64::MAX, vk::Semaphore::null());
        assert!(matches!(acquired, Err(RhiError::NotSupported(_))));
        let presented = chain.present(0, vk::Semaphore::null());
        assert!(matches!(presented, Err(RhiError::NotSupported(_))));
        let tuned_present = chain.acquire(u64::MAX, vk::Semaphore::null());
        assert!(
            matches!(tuned_present, Err(RhiError::NotSupported(_))),
            "a second acquire must still refuse, not fall through to a vulkan variant"
        );
        assert!(chain.rhi_format().is_none());
        assert_eq!(chain.extent(), Extent2D::default());
        assert!(chain.images().is_empty());
        assert!(chain.image_views().is_empty());
        assert_eq!(chain.image_count(), 0);
    }

    /// The request and the mode spellings travel the neutral seam in one
    /// direction only, and the Vulkan backend receives exactly the mode the
    /// caller asked for, one code per variant.
    #[test]
    fn present_modes_map_one_to_one_to_vulkan_codes() {
        assert_eq!(PresentMode::Fifo.as_vulkan(), vk::PresentModeKHR::FIFO);
        assert_eq!(
            PresentMode::FifoRelaxed.as_vulkan(),
            vk::PresentModeKHR::FIFO_RELAXED
        );
        assert_eq!(PresentMode::Mailbox.as_vulkan(), vk::PresentModeKHR::MAILBOX);
        assert_eq!(
            PresentMode::Immediate.as_vulkan(),
            vk::PresentModeKHR::IMMEDIATE
        );
        // The request default must continue to be the mode every surface
        // guarantees; silently becoming "best effort" would change what
        // callers get when they ask for nothing.
        assert_eq!(PresentMode::default(), PresentMode::Fifo);
        let request = SwapchainRequest::default();
        assert_eq!(
            (request.width, request.height, request.present_mode),
            (1280, 720, PresentMode::Fifo)
        );
    }
}
