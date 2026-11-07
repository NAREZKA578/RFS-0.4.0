//! Format Conversion Utilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::types::Format;

/// Format conversion trait
pub trait FormatConversion {
    fn to_vulkan(&self) -> u32;
    fn to_dxgi(&self) -> u32;
    fn to_gl(&self) -> u32;

    /// The RHI format that maps to a given `VkFormat`, or `None` when nothing
    /// does.
    ///
    /// The forward direction is one-to-many (several RHI formats can share a
    /// Vulkan code), so this returns the first match and is not a true inverse.
    /// It exists because a window surface picks its own format, and a render
    /// pass has to be built for whatever it picked — without this the
    /// negotiated format could not be described back to the RHI.
    fn from_vulkan(vk_format: u32) -> Option<Format> {
        use Format::*;
        const CANDIDATES: [Format; 10] = [
            B8G8R8A8_SRGB,
            B8G8R8A8_UNORM,
            RGBA8_UNORM,
            RGBA8_SNORM,
            RGBA16_UNORM,
            RGBA16_SFLOAT,
            D32_SFLOAT,
            D24_UNORM_S8_UINT,
            D16_UNORM,
            D24_UNORM,
        ];
        CANDIDATES
            .into_iter()
            .find(|f| f.to_vulkan() == vk_format)
    }
}

impl FormatConversion for Format {
    /// `VkFormat` values from the Vulkan specification.
    ///
    /// Bug №178: the old table was not a VkFormat table at all. It assigned
    /// R16_UNORM = 21 (that is R8G8_SINT), R16_SFLOAT = 31 (B8G8R8_SNORM),
    /// RG8_UNORM = 43 (R8G8B8A8_SRGB), RGBA32_SINT = 101 (R32G32_UINT), and
    /// S8_UINT = 133 (BC1_RGBA_UNORM) — and it also collided internally, e.g.
    /// RGBA16_UNORM = 112 is R64_SFLOAT. Every one of these describes a
    /// different memory layout, so a buffer described as one format was read
    /// back as another.
    fn to_vulkan(&self) -> u32 {
        use Format::*;
        match self {
            // 8-bit
            R8_UNORM => 9,
            R8_SNORM => 10,
            R8_UINT => 13,
            R8_SINT => 14,
            // 16-bit
            R16_UNORM => 70,
            R16_SNORM => 71,
            R16_UINT => 74,
            R16_SINT => 75,
            R16_SFLOAT => 76,
            RG8_UNORM => 16,
            RG8_SNORM => 17,
            RG8_UINT => 20,
            RG8_SINT => 21,
            // 32-bit
            R32_UINT => 98,
            R32_SINT => 99,
            R32_SFLOAT => 100,
            RG16_UNORM => 77,
            RG16_SNORM => 78,
            RG16_UINT => 81,
            RG16_SINT => 82,
            RG16_SFLOAT => 83,
            RGBA8_UNORM => 37,
            RGBA8_SNORM => 38,
            RGBA8_UINT => 41,
            RGBA8_SINT => 42,
            // BGR order, sRGB and linear. These are the formats a window
            // surface actually offers; without them `from_vulkan` could not
            // describe back whatever a swapchain negotiated, and the render
            // pass for it was unbuildable.
            B8G8R8A8_UNORM => 44,
            // Bug №178: 43 is R8G8B8A8_SRGB (the RGBA layout); the B8G8 sRGB
            // format is its own enum entry (50), so an sRGB swapchain was
            // described with the wrong byte order.
            B8G8R8A8_SRGB => 50,
            // 64-bit
            R32G32_UINT => 101,
            R32G32_SINT => 102,
            R32G32_SFLOAT => 103,
            RGBA16_UNORM => 91,
            RGBA16_SFLOAT => 97,
            // 96-bit
            R32G32B32_UINT => 104,
            R32G32B32_SINT => 105,
            R32G32B32_SFLOAT => 106,
            // 128-bit
            RGBA32_UINT => 107,
            RGBA32_SINT => 108,
            RGBA32_SFLOAT => 109,
            // Depth/stencil — the literal vulkan_core.h values: D16 124,
            // X8_D24 125, D32_SFLOAT 126, S8 127, D24S8 129, D32S8 130.
            // Bug №178: these rows carried the previous table's +2 shift
            // (126/127/128/131/132), which is what pushed the BC block to
            // invent its own numbering instead of the spec one.
            D16_UNORM => 124,
            D24_UNORM => 125, // VK_FORMAT_X8_D24_UNORM_PACK32
            D32_SFLOAT => 126,
            D24_UNORM_S8_UINT => 129,
            D32_SFLOAT_S8_UINT => 130,
            S8_UINT => 127,
            // Packed — real enumerants. Bug №178: this pair was shifted by
            // one (123/124), and 124 must stay free for D16_UNORM.
            B10G11R11_UFLOAT => 122,
            E5B9G9R9_UFLOAT => 123,
            // A8_UNORM has no VkFormat. Returning a nearby value here is the
            // same class of bug as the wrong table: the caller would get a
            // format with a different layout and never know. UNDEFINED (0)
            // makes the absence explicit (`_ => 0` below).
            // Compressed, BC — the literal vulkan_core.h series: BC1_RGB
            // starts at 131, right after D32_SFLOAT_S8_UINT, and runs to
            // BC7_SRGB at 146. Bug №178: the block sat anywhere but there.
            BC1_RGB_UNORM => 131,
            BC1_RGB_SRGB => 132,
            BC1_RGBA_UNORM => 133,
            BC1_RGBA_SRGB => 134,
            BC2_UNORM => 135,
            BC2_SRGB => 136,
            BC3_UNORM => 137,
            BC3_SRGB => 138,
            BC4_UNORM => 139,
            BC4_SNORM => 140,
            BC5_UNORM => 141,
            BC5_SNORM => 142,
            BC6H_UFLOAT => 143,
            BC6H_SFLOAT => 144,
            BC7_UNORM => 145,
            BC7_SRGB => 146,
            // ETC2/EAC — VK 147–156 (ETC2 first, then the EAC pair). Bug
            // №178: these compressed formats had no entry at all and silently
            // reported 0, which is UNDEFINED — the same harm as a wrong code,
            // just harder to notice.
            ETC2_R8G8B8_UNORM => 147,
            ETC2_R8G8B8_SRGB => 148,
            ETC2_R8G8B8A1_UNORM => 149,
            ETC2_R8G8B8A1_SRGB => 150,
            ETC2_R8G8B8A8_UNORM => 151,
            ETC2_R8G8B8A8_SRGB => 152,
            EAC_R11_UNORM => 153,
            EAC_R11_SNORM => 154,
            EAC_R11G11_UNORM => 155,
            EAC_R11G11_SNORM => 156,
            // ASTC 4x4…8x8 — VK 157–172; the Format set stops at 8x8.
            ASTC_4x4_UNORM => 157,
            ASTC_4x4_SRGB => 158,
            ASTC_5x4_UNORM => 159,
            ASTC_5x4_SRGB => 160,
            ASTC_5x5_UNORM => 161,
            ASTC_5x5_SRGB => 162,
            ASTC_6x5_UNORM => 163,
            ASTC_6x5_SRGB => 164,
            ASTC_6x6_UNORM => 165,
            ASTC_6x6_SRGB => 166,
            ASTC_8x5_UNORM => 167,
            ASTC_8x5_SRGB => 168,
            ASTC_8x6_UNORM => 169,
            ASTC_8x6_SRGB => 170,
            ASTC_8x8_UNORM => 171,
            ASTC_8x8_SRGB => 172,
            _ => 0,
        }
    }

    /// `DXGI_FORMAT` values from `dxgiformat.h`.
    ///
    /// Bug №178: the old table was wrong in both directions — R8_SNORM = 62 is
    /// actually R8_UINT and R8_UINT = 63 is actually R8_SNORM, so the two were
    /// swapped; R32_SFLOAT = 41 collided with R32_UINT; RGBA8_SNORM = 74 is
    /// YCBCR. The interm fix rotated the same rows one slot further and called
    /// the depth entries "aliases" — 45 is D24_UNORM_S8_UINT itself in the
    /// spec, and R32G32_UINT is 17, so the coincidence comment held no truth.
    fn to_dxgi(&self) -> u32 {
        use Format::*;
        match self {
            R8_UNORM => 61,
            R8_SNORM => 63,
            R8_UINT => 62,
            R8_SINT => 64,
            R16_UNORM => 56,
            R16_SNORM => 58,
            R16_UINT => 57,
            R16_SINT => 59,
            R16_SFLOAT => 54,
            RG8_UNORM => 49,
            RG8_SNORM => 51,
            RG8_UINT => 50,
            RG8_SINT => 52,
            RG16_UNORM => 35,
            RG16_SNORM => 37,
            RG16_UINT => 36,
            RG16_SINT => 38,
            RG16_SFLOAT => 34,
            // dxgiformat.h: 40 is D32_FLOAT, so the R32 triple sits at
            // 41/42/43. Bug №178: the rows sat rotated one slot down, so
            // R32_FLOAT was described as D32_FLOAT's code.
            R32_UINT => 42,
            R32_SINT => 43,
            R32_SFLOAT => 41,
            R32G32_UINT => 45,
            R32G32_SINT => 46,
            R32G32B32_UINT => 7,
            R32G32B32_SINT => 8,
            RGBA8_UNORM => 28,
            RGBA8_SNORM => 31,
            RGBA8_UINT => 30,
            RGBA8_SINT => 32,
            B8G8R8A8_UNORM => 87,  // DXGI_FORMAT_B8G8R8A8_UNORM
            B8G8R8A8_SRGB => 91,   // DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
            RGBA16_UNORM => 11,
            RGBA16_SFLOAT => 10,
            RGBA32_UINT => 3,
            RGBA32_SINT => 4,
            RGBA32_SFLOAT => 2,
            B10G11R11_UFLOAT => 24,
            E5B9G9R9_UFLOAT => 23,
            // Depth/stencil — the literal dxgiformat.h entries. They are real
            // named formats, not "documented aliases": D16_UNORM is 55, the
            // D24 family's typeless base R24G8_TYPELESS is 44, D32_FLOAT is
            // 40 and D24_UNORM_S8_UINT is exactly 45. Bug №178: these rows
            // pointed at unrelated formats, and S8_UINT invented a fake
            // "alias" of R8G8_UNORM — D3D has no stencil-only format, so the
            // absence stays explicit at 0.
            D16_UNORM => 55,
            D24_UNORM => 44,
            D32_SFLOAT => 40,
            D24_UNORM_S8_UINT => 45,
            S8_UINT => 0,
            // BC
            BC1_RGB_UNORM => 71,
            BC1_RGB_SRGB => 72,
            BC1_RGBA_UNORM => 71,
            BC1_RGBA_SRGB => 72,
            BC2_UNORM => 74,
            BC2_SRGB => 75,
            BC3_UNORM => 77,
            BC3_SRGB => 78,
            BC4_UNORM => 80,
            BC4_SNORM => 81,
            BC5_UNORM => 83,
            BC5_SNORM => 84,
            BC6H_UFLOAT => 95,
            BC6H_SFLOAT => 96,
            BC7_UNORM => 98,
            BC7_SRGB => 99,
            _ => 0,
        }
    }

    /// OpenGL sized/internal format enumerants.
    ///
    /// Bug №178: D32_SFLOAT was mapped to 0x88F2, which is not a GL depth
    /// format at all. The correct enumerant is GL_DEPTH_COMPONENT32F = 0x8CAC.
    fn to_gl(&self) -> u32 {
        use Format::*;
        match self {
            R8_UNORM => 0x8229, // GL_R8
            R8_UINT => 0x8232,
            R8_SINT => 0x8231,
            RG8_UNORM => 0x822B, // GL_RG8
            R16_UNORM => 0x822A, // GL_R16
            R16_SFLOAT => 0x822F, // GL_R16F
            R32_SFLOAT => 0x822E, // GL_R32F
            RG16_UNORM => 0x822C, // GL_RG16
            RG16_SFLOAT => 0x8230, // GL_RG16F
            RGBA8_UNORM => 0x8058, // GL_RGBA8
            B8G8R8A8_UNORM => 0x8058, // GL_RGBA8 (GL has no BGRA internal
                                      // format; the component order is the
                                      // upload's GL_BGRA type's business)
            B8G8R8A8_SRGB => 0x8C41,  // GL_SRGB8_ALPHA8
            RGBA16_UNORM => 0x805B, // GL_RGBA16
            RGBA16_SFLOAT => 0x881A, // GL_RGBA16F
            RGBA32_SFLOAT => 0x8814, // GL_RGBA32F
            B10G11R11_UFLOAT => 0x8C3A, // GL_R11F_G11F_B10F
            E5B9G9R9_UFLOAT => 0x8C3D, // GL_RGB9_E5
            D16_UNORM => 0x81A5, // GL_DEPTH_COMPONENT16
            D24_UNORM => 0x81A6, // GL_DEPTH_COMPONENT24
            D32_SFLOAT => 0x8CAC, // GL_DEPTH_COMPONENT32F
            D24_UNORM_S8_UINT => 0x88F0, // GL_DEPTH24_STENCIL8
            D32_SFLOAT_S8_UINT => 0x8CAD, // GL_DEPTH32F_STENCIL8
            S8_UINT => 0x8D48, // GL_STENCIL_INDEX8 (sized; 0x8D24 is nothing)
            // ETC2/EAC KHR series (glext.h). Bug №178: these had no entry and
            // reported 0 = UNDEFINED.
            EAC_R11_UNORM => 0x9270, // GL_COMPRESSED_R11_EAC
            EAC_R11_SNORM => 0x9271, // GL_COMPRESSED_SIGNED_R11_EAC
            EAC_R11G11_UNORM => 0x9272, // GL_COMPRESSED_RG11_EAC
            EAC_R11G11_SNORM => 0x9273, // GL_COMPRESSED_SIGNED_RG11_EAC
            ETC2_R8G8B8_UNORM => 0x9274, // GL_COMPRESSED_RGB8_ETC2
            ETC2_R8G8B8_SRGB => 0x9275, // GL_COMPRESSED_SRGB8_ETC2
            ETC2_R8G8B8A1_UNORM => 0x9276, // GL_COMPRESSED_RGB8_PUNCHTHROUGH_ALPHA1_ETC2
            ETC2_R8G8B8A1_SRGB => 0x9277, // GL_COMPRESSED_SRGB8_PUNCHTHROUGH_ALPHA1_ETC2
            ETC2_R8G8B8A8_UNORM => 0x9278, // GL_COMPRESSED_RGBA8_ETC2_EAC
            ETC2_R8G8B8A8_SRGB => 0x9279, // GL_COMPRESSED_SRGB8_ALPHA8_ETC2_EAC
            // ASTC KHR series: RGBA layouts at 0x93B0…, SRGB_ALPHA at 0x93D1…,
            // one enumerant per layout.
            ASTC_4x4_UNORM => 0x93B0,
            ASTC_4x4_SRGB => 0x93D1,
            ASTC_5x4_UNORM => 0x93B1,
            ASTC_5x4_SRGB => 0x93D2,
            ASTC_5x5_UNORM => 0x93B2,
            ASTC_5x5_SRGB => 0x93D3,
            ASTC_6x5_UNORM => 0x93B3,
            ASTC_6x5_SRGB => 0x93D4,
            ASTC_6x6_UNORM => 0x93B4,
            ASTC_6x6_SRGB => 0x93D5,
            ASTC_8x5_UNORM => 0x93B5,
            ASTC_8x5_SRGB => 0x93D6,
            ASTC_8x6_UNORM => 0x93B6,
            ASTC_8x6_SRGB => 0x93D7,
            ASTC_8x8_UNORM => 0x93B7,
            ASTC_8x8_SRGB => 0x93D8,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod bug_178 {
    use super::{Format, FormatConversion};

    // Bug №178: the compressed formats had no real table entries (ETC2/EAC
    // and ASTC reported 0 = UNDEFINED), the BC block sat on top of the
    // depth/stencil codes, and the depth/packed/B8G8-SRGB rows were shifted
    // against the spec as well. Every value below is taken from the primary
    // sources (vulkan_core.h, dxgiformat.h, gl.xml), not from the audit text.
    #[test]
    fn vk_depth_tail_and_packed_are_the_spec_values() {
        assert_eq!(Format::D16_UNORM.to_vulkan(), 124);
        assert_eq!(Format::D24_UNORM.to_vulkan(), 125); // X8_D24_UNORM_PACK32
        assert_eq!(Format::D32_SFLOAT.to_vulkan(), 126);
        assert_eq!(Format::S8_UINT.to_vulkan(), 127);
        assert_eq!(Format::D24_UNORM_S8_UINT.to_vulkan(), 129);
        assert_eq!(Format::D32_SFLOAT_S8_UINT.to_vulkan(), 130);
        assert_eq!(Format::B10G11R11_UFLOAT.to_vulkan(), 122);
        assert_eq!(Format::E5B9G9R9_UFLOAT.to_vulkan(), 123);
        // 43 is R8G8B8A8_SRGB (RGBA byte order); the B8G8 family lives at 44+.
        assert_eq!(Format::B8G8R8A8_UNORM.to_vulkan(), 44);
        assert_eq!(Format::B8G8R8A8_SRGB.to_vulkan(), 50);
    }

    #[test]
    fn vk_bc_block_is_the_spec_enumeration() {
        assert_eq!(Format::BC1_RGB_UNORM.to_vulkan(), 131);
        assert_eq!(Format::BC1_RGBA_SRGB.to_vulkan(), 134);
        assert_eq!(Format::BC7_SRGB.to_vulkan(), 146);
    }

    #[test]
    fn vk_etc2_eac_astc_match_the_khr_ranges() {
        assert_eq!(Format::ETC2_R8G8B8_UNORM.to_vulkan(), 147);
        assert_eq!(Format::ETC2_R8G8B8A8_SRGB.to_vulkan(), 152);
        assert_eq!(Format::EAC_R11_UNORM.to_vulkan(), 153);
        assert_eq!(Format::ASTC_4x4_UNORM.to_vulkan(), 157);
        assert_eq!(Format::ASTC_8x8_SRGB.to_vulkan(), 172);
    }

    #[test]
    fn dxgi_and_gl_compressed_codes_are_the_spec_ones() {
        // dxgiformat.h: the R16/R8G8/R32 rows sat rotated against the spec.
        assert_eq!(Format::R16_UNORM.to_dxgi(), 56);
        assert_eq!(Format::R16_SFLOAT.to_dxgi(), 54);
        assert_eq!(Format::RG8_UNORM.to_dxgi(), 49);
        assert_eq!(Format::RG8_SINT.to_dxgi(), 52);
        assert_eq!(Format::R32_SFLOAT.to_dxgi(), 41);
        assert_eq!(Format::R32_UINT.to_dxgi(), 42);
        assert_eq!(Format::R32_SINT.to_dxgi(), 43);
        // Depth: DXGI names the real typed / typeless entries themselves.
        assert_eq!(Format::D16_UNORM.to_dxgi(), 55);
        assert_eq!(Format::D24_UNORM.to_dxgi(), 44); // R24G8_TYPELESS
        assert_eq!(Format::D24_UNORM_S8_UINT.to_dxgi(), 45);
        assert_eq!(Format::D32_SFLOAT.to_dxgi(), 40); // D32_FLOAT
        // There is no stencil-only DXGI format; absence stays explicit.
        assert_eq!(Format::S8_UINT.to_dxgi(), 0);
        // D3D has one BC1 layout, so BC1_RGBA shares BC1's codes.
        assert_eq!(Format::BC1_RGB_UNORM.to_dxgi(), 71);
        assert_eq!(Format::BC1_RGBA_UNORM.to_dxgi(), 71);
        assert_eq!(Format::BC7_UNORM.to_dxgi(), 98);
        // ASTC/ETC2 have no DXGI enumerants; 0 (UNDEFINED) stays explicit.
        assert_eq!(Format::ETC2_R8G8B8_UNORM.to_dxgi(), 0);
        assert_eq!(Format::ASTC_4x4_UNORM.to_dxgi(), 0);
        // GL: the KHR compressed sets, plus the depth and stencil rows of the
        // same record — 0x8D24 does not exist in gl.xml at all; the sized
        // stencil format is GL_STENCIL_INDEX8. GL has no BGRA internal
        // format: the layout is RGBA8 (0x8058); 0x8059 is GL_RGB10_A2.
        assert_eq!(Format::D16_UNORM.to_gl(), 0x81A5);
        assert_eq!(Format::D32_SFLOAT.to_gl(), 0x8CAC);
        assert_eq!(Format::S8_UINT.to_gl(), 0x8D48); // GL_STENCIL_INDEX8
        assert_eq!(Format::B8G8R8A8_UNORM.to_gl(), 0x8058); // GL_RGBA8
        assert_eq!(Format::EAC_R11_UNORM.to_gl(), 0x9270);
        assert_eq!(Format::ETC2_R8G8B8A8_SRGB.to_gl(), 0x9279);
        assert_eq!(Format::ASTC_4x4_UNORM.to_gl(), 0x93B0);
        assert_eq!(Format::ASTC_8x8_SRGB.to_gl(), 0x93D8);
    }
}