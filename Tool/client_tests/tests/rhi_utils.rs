use rhi::utils::alignment::*;
use rhi::utils::hash::*;

#[test]
fn align_up_zero() {
    assert_eq!(align_up(0, 256), 0);
}

#[test]
fn align_up_one() {
    assert_eq!(align_up(1, 256), 256);
}

#[test]
fn align_up_boundary_minus_one() {
    assert_eq!(align_up(255, 256), 256);
}

#[test]
fn align_up_exact() {
    assert_eq!(align_up(256, 256), 256);
}

#[test]
fn align_up_over() {
    assert_eq!(align_up(257, 256), 512);
}

#[test]
fn align_up_various() {
    assert_eq!(align_up(3, 4), 4);
    assert_eq!(align_up(5, 4), 8);
    assert_eq!(align_up(129, 128), 256);
    assert_eq!(align_up(128, 128), 128);
}

#[test]
fn align_down_zero() {
    assert_eq!(align_down(0, 256), 0);
}

#[test]
fn align_down_below() {
    assert_eq!(align_down(255, 256), 0);
}

#[test]
fn align_down_exact() {
    assert_eq!(align_down(256, 256), 256);
}

#[test]
fn align_down_above() {
    assert_eq!(align_down(300, 256), 256);
}

#[test]
fn align_down_various() {
    assert_eq!(align_down(7, 4), 4);
    assert_eq!(align_down(4, 4), 4);
    assert_eq!(align_down(3, 4), 0);
    assert_eq!(align_down(300, 128), 256);
}

#[test]
fn is_aligned_zero() {
    assert!(is_aligned(0, 256));
}

#[test]
fn is_aligned_exact() {
    assert!(is_aligned(256, 256));
}

#[test]
fn is_aligned_not() {
    assert!(!is_aligned(1, 256));
}

#[test]
fn is_aligned_various() {
    assert!(is_aligned(0, 4));
    assert!(is_aligned(4, 4));
    assert!(!is_aligned(5, 4));
    assert!(is_aligned(128, 128));
    assert!(!is_aligned(129, 128));
}

#[test]
fn hash_data_consistent() {
    let h1 = hash_data(b"test");
    let h2 = hash_data(b"test");
    assert_eq!(h1, h2);
}

#[test]
fn hash_data_different_inputs() {
    let h1 = hash_data(b"test");
    let h2 = hash_data(b"test2");
    assert_ne!(h1, h2);
}

#[test]
fn hash_string_consistent() {
    let h1 = hash_string("hello");
    let h2 = hash_string("hello");
    assert_eq!(h1, h2);
}

#[test]
fn hash_string_different() {
    let h1 = hash_string("hello");
    let h2 = hash_string("world");
    assert_ne!(h1, h2);
}

#[test]
fn hash_string_matches_bytes() {
    let h1 = hash_string("test");
    let h2 = hash_data(b"test");
    assert_eq!(h1, h2);
}

#[test]
fn alignment_constants() {
    assert_eq!(BUFFER_COPY_ALIGNMENT, 4);
    assert_eq!(TEXTURE_COPY_ALIGNMENT, 4);
    assert_eq!(UNIFORM_BUFFER_ALIGNMENT, 256);
    assert_eq!(STORAGE_BUFFER_ALIGNMENT, 16);
}

#[test]
fn format_conversion_vulkan_distinct() {
    use rhi::utils::conversion::FormatConversion;
    use rhi::Format;
    assert_eq!(Format::R8_UNORM.to_vulkan(), 9);
    assert_eq!(Format::RGBA8_UNORM.to_vulkan(), 37);
    assert_eq!(Format::R32_SFLOAT.to_vulkan(), 100);
    assert_ne!(Format::RGBA8_UNORM.to_vulkan(), Format::R8_UNORM.to_vulkan());
    // Consistency across calls.
    assert_eq!(Format::D32_SFLOAT.to_vulkan(), Format::D32_SFLOAT.to_vulkan());
}

#[test]
fn format_conversion_stable_and_known() {
    use rhi::utils::conversion::FormatConversion;
    use rhi::Format;
    assert_eq!(Format::RGBA8_UNORM.to_dxgi(), 28);
    assert_eq!(Format::RGBA8_UNORM.to_gl(), 0x8058);
    // Vulkan spec: D24_UNORM_S8_UINT is 129 and S8_UINT is 127. The previous
    // expectations were the +2-shifted table from before bug №178.
    assert_eq!(Format::D24_UNORM_S8_UINT.to_vulkan(), 129);
    assert_eq!(Format::S8_UINT.to_vulkan(), 127);
    assert_ne!(
        Format::D24_UNORM_S8_UINT.to_vulkan(),
        Format::S8_UINT.to_vulkan(),
        "depth/stencil and stencil must not share a code"
    );
    assert_eq!(Format::R8_UNORM.to_dxgi(), 61);
}

#[test]
fn format_conversion_unknown_returns_zero() {
    use rhi::utils::conversion::FormatConversion;
    use rhi::Format;
    // ETC2/EAC have no DXGI enumerant, so DXGI reports UNDEFINED explicitly,
    // while Vulkan has a real spec code (bug №178 added the EAC rows).
    assert_eq!(Format::ETC2_R8G8B8_UNORM.to_dxgi(), 0);
    assert_eq!(Format::EAC_R11_UNORM.to_vulkan(), 153);
    // A Vulkan code that is not in the RHI table maps back to nothing.
    assert_eq!(Format::from_vulkan(9999), None);
}

/// A window surface negotiates its own format, and it is almost always
/// `B8G8R8A8_SRGB`. The RHI could not describe it before these variants
/// existed, which made a render pass for the surface unbuildable — so
/// presentation could not be finished no matter how much Vulkan code was
/// written. The codes are checked against the spec, not against the backend,
/// so a wrong table here is caught without a GPU.
#[test]
fn presentation_formats_map_to_the_spec_codes() {
    use rhi::utils::conversion::FormatConversion;
    use rhi::Format;
    assert_eq!(Format::B8G8R8A8_UNORM.to_vulkan(), 44); // VK_FORMAT_B8G8R8A8_UNORM
    // 43 is R8G8B8A8_SRGB (RGBA layout); the BGR sRGB format is 50.
    assert_eq!(Format::B8G8R8A8_SRGB.to_vulkan(), 50); // VK_FORMAT_B8G8R8A8_SRGB
    assert_eq!(Format::B8G8R8A8_UNORM.to_dxgi(), 87); // DXGI_FORMAT_B8G8R8A8_UNORM
    assert_eq!(Format::B8G8R8A8_SRGB.to_dxgi(), 91); // DXGI_FORMAT_B8G8R8A8_UNORM_SRGB

    // BGR and RGB are different memory layouts and must not collide.
    assert_ne!(
        Format::B8G8R8A8_UNORM.to_vulkan(),
        Format::RGBA8_UNORM.to_vulkan(),
        "BGR and RGB are the same size but different layouts"
    );
    assert_ne!(
        Format::B8G8R8A8_SRGB.to_vulkan(),
        Format::B8G8R8A8_UNORM.to_vulkan(),
        "sRGB and linear must be distinct formats"
    );
}

/// The inverse direction, which the presentation path depends on: whatever the
/// surface negotiated has to be expressible as an RHI format.
#[test]
fn a_negotiated_surface_format_round_trips_back_to_an_rhi_format() {
    use rhi::utils::conversion::FormatConversion;
    use rhi::Format;
    for format in [
        Format::B8G8R8A8_SRGB,
        Format::B8G8R8A8_UNORM,
        Format::RGBA8_UNORM,
        Format::D32_SFLOAT,
    ] {
        let code = format.to_vulkan();
        let back = <Format as FormatConversion>::from_vulkan(code)
            .unwrap_or_else(|| panic!("{format:?} maps to vk {code} but nothing maps back"));
        assert_eq!(
            back.to_vulkan(),
            code,
            "{format:?} did not survive the round trip (came back as {back:?})"
        );
    }

    // A code that is not an RHI format must be reported, not guessed at.
    assert_eq!(<Format as FormatConversion>::from_vulkan(0), None, "VK_FORMAT_UNDEFINED");
    assert_eq!(<Format as FormatConversion>::from_vulkan(1_000_000), None);
}
