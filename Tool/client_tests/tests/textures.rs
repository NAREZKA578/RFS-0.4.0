//! Integration tests for the render::textures module.
//!
//! Covers: TextureType, TextureFormat and its RHI Format mapping,
//! TextureUsage and its RHI TextureUsage mapping, TextureWrapMode,
//! TextureFilterMode and SamplerDesc. (Creating actual Texture instances
//! and TextureLibrary entries requires an RHI Device.)

use rfs_client::render::textures::{
    SamplerDesc, TextureFilterMode, TextureFormat, TextureType, TextureUsage, TextureWrapMode,
};
use rfs_client::rhi::{Format, TextureUsage as RhiTextureUsage};

#[test]
fn texture_type_variants_and_default() {
    assert_eq!(TextureType::default(), TextureType::Texture2D);
    assert!(matches!(TextureType::Texture3D, TextureType::Texture3D));
    assert!(matches!(TextureType::TextureCube, TextureType::TextureCube));
    assert!(matches!(
        TextureType::TextureArray,
        TextureType::TextureArray
    ));
    assert!(matches!(
        TextureType::DepthTexture,
        TextureType::DepthTexture
    ));
    assert!(matches!(
        TextureType::RenderTarget,
        TextureType::RenderTarget
    ));
}

#[test]
fn texture_format_default_is_rgba8() {
    assert_eq!(TextureFormat::default(), TextureFormat::RGBA8);
}

#[test]
fn texture_format_maps_to_rhi_format() {
    let cases = [
        (TextureFormat::RGBA8, Format::RGBA8_UNORM),
        (TextureFormat::RGBA16, Format::RG16_UNORM),
        (TextureFormat::RGBA32, Format::RGBA32_SFLOAT),
        (TextureFormat::RGB8, Format::RGBA8_UNORM),
        (TextureFormat::RGB16, Format::RG16_UNORM),
        (TextureFormat::RGB32, Format::R32G32B32_SFLOAT),
        (TextureFormat::RG8, Format::RG8_UNORM),
        (TextureFormat::RG16, Format::RG16_UNORM),
        (TextureFormat::R8, Format::R8_UNORM),
        (TextureFormat::R16, Format::R16_UNORM),
        (TextureFormat::R32, Format::R32_SFLOAT),
        (TextureFormat::Depth16, Format::D16_UNORM),
        (TextureFormat::Depth24, Format::D24_UNORM),
        (TextureFormat::Depth32, Format::D32_SFLOAT),
        (TextureFormat::Depth24Stencil8, Format::D24_UNORM_S8_UINT),
        (TextureFormat::Depth32Stencil8, Format::D32_SFLOAT_S8_UINT),
    ];

    for (texture_format, expected) in cases {
        let rhi_format = Format::from(texture_format);
        assert_eq!(
            rhi_format, expected,
            "mismatch for {:?}",
            texture_format
        );
    }
}

#[test]
fn diffuse_usage_maps_to_sampled() {
    let usages = [
        TextureUsage::Diffuse,
        TextureUsage::Normal,
        TextureUsage::Specular,
        TextureUsage::Roughness,
        TextureUsage::Metallic,
        TextureUsage::AmbientOcclusion,
        TextureUsage::Emissive,
        TextureUsage::Height,
        TextureUsage::Sampled,
        TextureUsage::Storage,
    ];
    for usage in usages {
        assert_eq!(RhiTextureUsage::from(usage), RhiTextureUsage::SAMPLED);
    }
}

#[test]
fn depth_usage_maps_to_depth_stencil_sampled() {
    let expected = RhiTextureUsage::DEPTH_STENCIL_ATTACHMENT | RhiTextureUsage::SAMPLED;
    assert_eq!(RhiTextureUsage::from(TextureUsage::Depth), expected);
    assert_eq!(RhiTextureUsage::from(TextureUsage::Stencil), expected);
}

#[test]
fn render_target_usage_maps_to_color_attachment_sampled() {
    let expected = RhiTextureUsage::COLOR_ATTACHMENT | RhiTextureUsage::SAMPLED;
    assert_eq!(
        RhiTextureUsage::from(TextureUsage::RenderTarget),
        expected
    );
}

#[test]
fn wrap_and_filter_mode_defaults() {
    assert_eq!(TextureWrapMode::default(), TextureWrapMode::Repeat);
    assert_eq!(
        TextureFilterMode::default(),
        TextureFilterMode::Linear
    );

    assert!(matches!(
        TextureWrapMode::MirroredRepeat,
        TextureWrapMode::MirroredRepeat
    ));
    assert!(matches!(
        TextureWrapMode::ClampToEdge,
        TextureWrapMode::ClampToEdge
    ));
    assert!(matches!(
        TextureWrapMode::ClampToBorder,
        TextureWrapMode::ClampToBorder
    ));
    assert!(matches!(
        TextureFilterMode::NearestMipmapLinear,
        TextureFilterMode::NearestMipmapLinear
    ));
    assert!(matches!(
        TextureFilterMode::Anisotropic,
        TextureFilterMode::Anisotropic
    ));
}

#[test]
fn sampler_desc_defaults() {
    let desc = SamplerDesc::default();
    assert_eq!(desc.wrap_mode, TextureWrapMode::Repeat);
    assert_eq!(desc.filter_mode, TextureFilterMode::Linear);
    assert_eq!(desc.anisotropy, 1.0);
    assert_eq!(desc.lod_bias, 0.0);
    assert_eq!(desc.min_lod, 0.0);
    assert_eq!(desc.max_lod, f32::MAX);
    assert_eq!(desc.border_color, [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn sampler_desc_fields_mutable() {
    let mut desc = SamplerDesc {
        wrap_mode: TextureWrapMode::ClampToEdge,
        filter_mode: TextureFilterMode::LinearMipmapLinear,
        anisotropy: 16.0,
        lod_bias: -0.5,
        min_lod: 0.0,
        max_lod: 8.0,
        border_color: [1.0, 0.0, 0.0, 1.0],
    };
    desc.anisotropy = 8.0;
    desc.wrap_mode = TextureWrapMode::MirroredRepeat;

    assert_eq!(desc.anisotropy, 8.0);
    assert_eq!(desc.wrap_mode, TextureWrapMode::MirroredRepeat);
    assert_eq!(desc.filter_mode, TextureFilterMode::LinearMipmapLinear);
    assert_eq!(desc.border_color, [1.0, 0.0, 0.0, 1.0]);
}