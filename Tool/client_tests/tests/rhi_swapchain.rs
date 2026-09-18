// Integration tests for the rhi::swapchain module.
//
// Covers: Surface, SurfaceCapabilities, SwapChainDesc/SwapChain construction,
// and present-mode/color-space enum variants.

use rhi::swapchain::surface::{Surface, SurfaceCapabilities, SurfaceDesc};
use rhi::swapchain::swapchain::{
    ColorSpace, CompositeAlpha, FullscreenExclusive, PresentMode, SwapChain, SwapChainDesc,
    SurfaceTransform, SwapChainImage,
};
use rhi::resource::texture::TextureView;
use rhi::{Format, SharingMode, TextureUsage};

#[test]
fn surface_new_and_desc() {
    let surface = Surface::new(SurfaceDesc {
        width: 1920,
        height: 1080,
    });
    assert_eq!(surface.desc().width, 1920);
    assert_eq!(surface.desc().height, 1080);
}

#[test]
fn surface_desc_default() {
    let d = SurfaceDesc::default();
    assert_eq!(d.width, 0);
}

#[test]
fn surface_capabilities_default() {
    let caps = SurfaceCapabilities::default();
    assert_eq!(caps.min_image_count, 0);
    assert!(caps.supported_formats.is_empty());
    assert!(caps.supported_present_modes.is_empty());
}

#[test]
fn surface_capabilities_creation() {
    let caps = SurfaceCapabilities {
        supported_present_modes: vec![PresentMode::Fifo, PresentMode::Mailbox],
        supported_formats: vec![Format::RGBA8_UNORM, Format::R32_SFLOAT],
        supported_color_spaces: vec![ColorSpace::Srgb],
        supported_usage_flags: TextureUsage::COLOR_ATTACHMENT,
        min_image_count: 2,
        max_image_count: 8,
        max_image_extent: rhi::Extent2D::new(4096, 4096),
        current_extent: rhi::Extent2D::new(1920, 1080),
    };
    assert_eq!(caps.min_image_count, 2);
    assert_eq!(caps.supported_formats.len(), 2);
    assert!(caps.supported_usage_flags.contains(TextureUsage::COLOR_ATTACHMENT));
}

#[test]
fn color_space_variants() {
    assert_eq!(ColorSpace::default(), ColorSpace::Srgb);
    let _ = ColorSpace::Hdr10;
    let _ = ColorSpace::DolbyVision;
    let _ = ColorSpace::Hlg;
}

#[test]
fn present_mode_variants() {
    assert_eq!(PresentMode::default(), PresentMode::Immediate);
    let _ = PresentMode::Mailbox;
    let _ = PresentMode::Fifo;
    let _ = PresentMode::FifoRelaxed;
}

#[test]
fn surface_transform_variants() {
    assert_eq!(SurfaceTransform::default(), SurfaceTransform::Identity);
    let _ = SurfaceTransform::Rotate90;
    let _ = SurfaceTransform::Rotate180;
    let _ = SurfaceTransform::Rotate270;
    let _ = SurfaceTransform::HorizontalFlip;
}

#[test]
fn composite_alpha_variants() {
    assert_eq!(CompositeAlpha::default(), CompositeAlpha::Opaque);
    let _ = CompositeAlpha::PreMultiplied;
    let _ = CompositeAlpha::PostMultiplied;
    let _ = CompositeAlpha::Inherit;
}

#[test]
fn fullscreen_exclusive_variants() {
    assert_eq!(FullscreenExclusive::default(), FullscreenExclusive::None);
    let _ = FullscreenExclusive::Fullscreen;
    let _ = FullscreenExclusive::ApplicationControlled;
}

#[test]
fn swap_chain_desc_creation() {
    let desc = SwapChainDesc {
        surface: Surface::new(SurfaceDesc {
            width: 1280,
            height: 720,
        }),
        width: 1280,
        height: 720,
        format: Format::RGBA8_UNORM,
        color_space: ColorSpace::Srgb,
        present_mode: PresentMode::Fifo,
        buffer_count: 3,
        usage: TextureUsage::COLOR_ATTACHMENT,
        sharing_mode: SharingMode::Exclusive,
        queue_family_indices: vec![],
        pre_transform: SurfaceTransform::Identity,
        alpha_composite: CompositeAlpha::Opaque,
        clipped: true,
        fullscreen_exclusive: FullscreenExclusive::None,
    };
    assert_eq!(desc.buffer_count, 3);
    assert_eq!(desc.width, 1280);
    assert_eq!(desc.height, 720);
}

#[test]
fn swap_chain_new_desc_images_empty() {
    let sc = SwapChain::new(SwapChainDesc {
        width: 640,
        height: 480,
        buffer_count: 2,
        ..Default::default()
    });
    assert_eq!(sc.desc().width, 640);
    assert_eq!(sc.images().len(), 0);
}

#[test]
fn swap_chain_image_fields() {
    let image = SwapChainImage {
        texture: rhi::Texture::default(),
        view: TextureView::default(),
        index: 0,
    };
    assert_eq!(image.index, 0);
}

#[test]
fn swap_chain_desc_is_valid() {
    let valid = SwapChainDesc {
        width: 640,
        height: 480,
        buffer_count: 2,
        format: rhi::Format::RGBA8_UNORM,
        ..Default::default()
    };
    assert!(valid.is_valid());

    let invalid = SwapChainDesc {
        width: 0,
        height: 480,
        buffer_count: 2,
        format: rhi::Format::RGBA8_UNORM,
        ..Default::default()
    };
    assert!(!invalid.is_valid());

    let undefined = SwapChainDesc {
        width: 640,
        height: 480,
        buffer_count: 2,
        format: rhi::Format::Undefined,
        ..Default::default()
    };
    assert!(!undefined.is_valid());
}

#[test]
fn swap_chain_acquire_and_present_cycle() {
    let mut sc = SwapChain::new(SwapChainDesc {
        width: 800,
        height: 600,
        buffer_count: 3,
        format: rhi::Format::RGBA8_UNORM,
        usage: TextureUsage::COLOR_ATTACHMENT,
        ..Default::default()
    });

    let first = sc.acquire_next_image().unwrap();
    assert_eq!(first, 0);
    assert_eq!(sc.current_image_index(), Some(0));

    let second = sc.acquire_next_image().unwrap();
    assert_eq!(second, 1);
    let third = sc.acquire_next_image().unwrap();
    assert_eq!(third, 2);
    // Wraps around.
    let fourth = sc.acquire_next_image().unwrap();
    assert_eq!(fourth, 0);

    assert_eq!(sc.images().len(), 3);
    sc.present(2).unwrap();
    assert_eq!(sc.current_image_index(), Some(2));
}

#[test]
fn swap_chain_present_invalid_index() {
    let mut sc = SwapChain::new(SwapChainDesc {
        width: 640,
        height: 480,
        buffer_count: 2,
        format: rhi::Format::RGBA8_UNORM,
        ..Default::default()
    });
    sc.acquire_next_image().unwrap();
    assert!(sc.present(99).is_err());
}

#[test]
fn swap_chain_images_are_textures() {
    let mut sc = SwapChain::new(SwapChainDesc {
        width: 400,
        height: 300,
        buffer_count: 2,
        format: rhi::Format::B8G8R8_UNORM,
        usage: TextureUsage::COLOR_ATTACHMENT,
        ..Default::default()
    });
    sc.acquire_next_image().unwrap();
    let img = &sc.images()[0];
    assert_eq!(img.texture.width(), 400);
    assert_eq!(img.texture.height(), 300);
    assert_eq!(img.texture.format(), rhi::Format::B8G8R8_UNORM);
}