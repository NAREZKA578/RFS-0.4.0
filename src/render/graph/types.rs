//! Graph Types
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

/// Resource usage flags
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceUsage {
    /// Resource is a render target (written to)
    RenderTarget,
    /// Resource is a color attachment (written to)
    ColorAttachment,
    /// Resource is a depth/stencil attachment (written to)
    DepthStencil,
    /// Resource is sampled (read from)
    Sampled,
    /// Resource is used for storage (read/write)
    Storage,
}

impl std::ops::BitOr for ResourceUsage {
    type Output = ResourceUsage;

    fn bitor(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (ResourceUsage::RenderTarget, _) => ResourceUsage::RenderTarget,
            (_, ResourceUsage::RenderTarget) => ResourceUsage::RenderTarget,
            (ResourceUsage::ColorAttachment, ResourceUsage::DepthStencil) => {
                ResourceUsage::ColorAttachment
            }
            (ResourceUsage::DepthStencil, ResourceUsage::ColorAttachment) => {
                ResourceUsage::DepthStencil
            }
            (ResourceUsage::ColorAttachment, _) => ResourceUsage::ColorAttachment,
            (_, ResourceUsage::ColorAttachment) => ResourceUsage::ColorAttachment,
            (ResourceUsage::DepthStencil, _) => ResourceUsage::DepthStencil,
            (_, ResourceUsage::DepthStencil) => ResourceUsage::DepthStencil,
            (ResourceUsage::Sampled, ResourceUsage::Storage) => ResourceUsage::Sampled,
            (ResourceUsage::Storage, ResourceUsage::Sampled) => ResourceUsage::Storage,
            (ResourceUsage::Sampled, _) => ResourceUsage::Sampled,
            (ResourceUsage::Storage, _) => ResourceUsage::Storage,
        }
    }
}
