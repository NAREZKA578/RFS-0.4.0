//! Utils Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//! **TODO: Full implementation required**

pub mod alignment;
pub mod conversion;
pub mod hash;

pub use alignment::*;
pub use conversion::*;
pub use hash::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::{format_block, format_size, is_compressed_format, mip_level_size};
    use crate::resource::buffer::BufferDesc;
    use crate::types::{BufferUsage, Format};
    use crate::utils::FormatConversion;

    /// Bug №221: the depth formats were grouped by "4 bytes per component",
    /// giving D16_UNORM = 8 and D32_SFLOAT = 12. Both are wrong by 4x and 3x,
    /// and a depth attachment sized with them would be 4-8x oversized or, with
    /// D16, under-allocated relative to what the hardware writes.
    #[test]
    fn depth_texel_sizes_are_correct() {
        assert_eq!(format_size(Format::D16_UNORM), 2);
        assert_eq!(format_size(Format::D24_UNORM), 4);
        assert_eq!(format_size(Format::D32_SFLOAT), 4);
        assert_eq!(format_size(Format::D24_UNORM_S8_UINT), 4);
        assert_eq!(format_size(Format::D32_SFLOAT_S8_UINT), 8);
        assert_eq!(format_size(Format::S8_UINT), 1);
    }

    #[test]
    fn uncompressed_texel_sizes_still_hold() {
        assert_eq!(format_size(Format::R8_UNORM), 1);
        assert_eq!(format_size(Format::RG8_UNORM), 2);
        assert_eq!(format_size(Format::R32_SFLOAT), 4);
        assert_eq!(format_size(Format::RGBA8_UNORM), 4);
        assert_eq!(format_size(Format::RG16_SFLOAT), 4);
        assert_eq!(format_size(Format::RGBA16_SFLOAT), 8);
        assert_eq!(format_size(Format::RGBA32_SFLOAT), 16);
        assert_eq!(format_size(Format::R32G32B32_SFLOAT), 12);
        assert_eq!(format_size(Format::B10G11R11_UFLOAT), 4);
    }

    /// Bug №221: compressed formats used to report 0, which is not a size at
    /// all. They must report their block size and expose the block footprint.
    #[test]
    fn compressed_formats_report_a_block_size_not_zero() {
        for f in [
            Format::BC1_RGB_UNORM,
            Format::BC1_RGBA_UNORM,
            Format::BC3_UNORM,
            Format::BC7_UNORM,
            Format::ASTC_5x4_UNORM,
            Format::ASTC_8x8_UNORM,
            Format::ETC2_R8G8B8_UNORM,
        ] {
            assert!(is_compressed_format(f), "{f:?} should be compressed");
            assert_ne!(format_size(f), 0, "{f:?} must not report a zero size");
            let block = format_block(f).unwrap_or_else(|| panic!("{f:?} needs a block"));
            assert!(block.width >= 4 && block.height >= 4, "{f:?} block too small");
            assert_eq!(format_size(f), block.bytes);
        }
        assert!(!is_compressed_format(Format::RGBA8_UNORM));
        assert!(format_block(Format::RGBA8_UNORM).is_none());
    }

    /// Bug №221: a 5x5 BC1 image is one 4x4 block, not 25 texels. Sizing it by
    /// extents x block bytes is how compressed uploads get truncated.
    #[test]
    fn mip_size_rounds_up_to_whole_blocks() {
        // 5x5 BC1 -> ceil(5/4)^2 = 4 blocks of 8 bytes = 32.
        assert_eq!(mip_level_size(Format::BC1_RGB_UNORM, 5, 5, 1), 32);
        // 8x8 BC1 -> 4 blocks of 8 = 32.
        assert_eq!(mip_level_size(Format::BC1_RGB_UNORM, 8, 8, 1), 32);
        // 4x4 BC3 -> 1 block of 16 = 16.
        assert_eq!(mip_level_size(Format::BC3_UNORM, 4, 4, 1), 16);
        // Uncompressed path is a plain product.
        assert_eq!(mip_level_size(Format::RGBA8_UNORM, 5, 5, 1), 100);
        assert_eq!(mip_level_size(Format::RGBA8_UNORM, 5, 5, 2), 200);
    }

    /// Bug №222: the table was inverted against its own constants. A uniform
    /// buffer must land on 256 and a storage buffer on 16; the old code answered
    /// the opposite, which is undefined behaviour on the device.
    #[test]
    fn buffer_alignment_matches_the_declared_constants() {
        let uniform = BufferDesc {
            usage: BufferUsage::UNIFORM,
            ..Default::default()
        };
        let storage = BufferDesc {
            usage: BufferUsage::STORAGE,
            ..Default::default()
        };
        assert_eq!(uniform.alignment(), UNIFORM_BUFFER_ALIGNMENT);
        assert_eq!(storage.alignment(), STORAGE_BUFFER_ALIGNMENT);
        assert_eq!(uniform.alignment(), 256);
        assert_eq!(storage.alignment(), 16);
    }

    /// Bug №222: the old `contains(VERTEX | INDEX)` needed *both* bits, so a
    /// vertex-only buffer fell through to the 4-byte default and was reported
    /// as needing less alignment than it could be given.
    #[test]
    fn vertex_and_index_buffers_are_handled_separately() {
        for usage in [BufferUsage::VERTEX, BufferUsage::INDEX] {
            let desc = BufferDesc {
                usage,
                ..Default::default()
            };
            assert!(
                is_power_of_two(desc.alignment()),
                "{usage:?} must still get a power-of-two alignment"
            );
            assert!(desc.alignment() >= 4, "{usage:?} alignment too small");
        }
    }

    /// A buffer usable as both uniform and storage must satisfy the stricter
    /// of the two, otherwise a shader binding it as uniform is misaligned.
    #[test]
    fn a_dual_purpose_buffer_takes_the_stricter_alignment() {
        let desc = BufferDesc {
            usage: BufferUsage::UNIFORM | BufferUsage::STORAGE,
            ..Default::default()
        };
        assert_eq!(desc.alignment(), UNIFORM_BUFFER_ALIGNMENT);
    }

    /// Bug №223: `align_up(value, 0)` used to compute `value + u64::MAX`, which
    /// panics on overflow in debug builds.
    #[test]
    fn align_up_treats_zero_alignment_as_a_no_op() {
        assert_eq!(align_up(0, 0), 0);
        assert_eq!(align_up(1, 0), 1);
        assert_eq!(align_up(12345, 0), 12345);
        assert_eq!(align_up(u64::MAX, 0), u64::MAX);
    }

    /// Bug №223: the bitmask form was only valid for powers of two, so an
    /// alignment of 3, 5 or 100 produced garbage silently.
    #[test]
    fn align_up_is_correct_for_non_power_of_two_alignments() {
        assert_eq!(align_up(0, 3), 0);
        assert_eq!(align_up(1, 3), 3);
        assert_eq!(align_up(3, 3), 3);
        assert_eq!(align_up(4, 3), 6);
        assert_eq!(align_up(7, 5), 10);
        assert_eq!(align_up(1, 100), 100);
        assert_eq!(align_up(101, 100), 200);

        for alignment in [3u64, 5, 6, 7, 100, 12] {
            for value in 0u64..64 {
                let up = align_up(value, alignment);
                assert!(up >= value, "align_up went backwards for {alignment}");
                assert_eq!(up % alignment, 0, "align_up({value},{alignment}) = {up}");
                // and it must be the *nearest* such multiple
                if up > value {
                    assert!(up - alignment < value, "align_up overshot for {alignment}");
                }
            }
        }
    }

    #[test]
    fn align_down_and_is_aligned_agree_with_align_up() {
        for alignment in [1u64, 2, 4, 3, 5, 16, 100, 256] {
            for value in 0u64..200 {
                let up = align_up(value, alignment);
                assert!(is_aligned(up, alignment), "align_up produced a misaligned {up}");
                let down = align_down(value, alignment);
                assert!(is_aligned(down, alignment));
                assert!(down <= value);
                assert!(up - down <= alignment.max(1), "down/up disagree for {alignment}");
            }
        }
        assert_eq!(align_down(0, 0), 0);
        assert_eq!(align_down(7, 0), 7);
        assert!(is_aligned(12345, 0));
    }

    /// Bug №178: the old Vulkan table assigned R16_UNORM = 21 (that is
    /// R8G8_SINT) and RG8_UNORM = 43 (that is R8G8B8A8_SRGB). Both describe a
    /// different memory layout, so a buffer written one way was read another.
    #[test]
    fn vulkan_format_codes_are_the_spec_values() {
        // A representative sample of the ranges that were wrong.
        assert_eq!(Format::R8_UNORM.to_vulkan(), 9);
        assert_eq!(Format::R8_SINT.to_vulkan(), 14);
        assert_eq!(Format::RG8_UNORM.to_vulkan(), 16);
        assert_eq!(Format::R16_UNORM.to_vulkan(), 70);
        assert_eq!(Format::R16_SFLOAT.to_vulkan(), 76);
        assert_eq!(Format::RG16_SFLOAT.to_vulkan(), 83);
        assert_eq!(Format::RGBA8_UNORM.to_vulkan(), 37);
        assert_eq!(Format::RGBA16_UNORM.to_vulkan(), 91);
        assert_eq!(Format::RGBA16_SFLOAT.to_vulkan(), 97);
        assert_eq!(Format::R32_SFLOAT.to_vulkan(), 100);
        assert_eq!(Format::RGBA32_SINT.to_vulkan(), 108);
        assert_eq!(Format::RGBA32_SFLOAT.to_vulkan(), 109);
        assert_eq!(Format::D16_UNORM.to_vulkan(), 126);
        assert_eq!(Format::D32_SFLOAT.to_vulkan(), 128);
        assert_eq!(Format::S8_UINT.to_vulkan(), 129);
        assert_eq!(Format::D24_UNORM_S8_UINT.to_vulkan(), 131);
        assert_eq!(Format::BC7_UNORM.to_vulkan(), 145);
    }

    /// No two distinct uncompressed formats may share a VkFormat. The old
    /// table collided on 101 (RG16_UNORM and RGBA32_SINT) and on 112
    /// (RGBA16_UNORM, which is really R64_SFLOAT).
    #[test]
    fn vulkan_codes_do_not_collide_for_uncompressed_formats() {
        let uncompressed = [
            Format::R8_UNORM, Format::R8_SNORM, Format::R8_UINT, Format::R8_SINT,
            Format::R16_UNORM, Format::R16_SNORM, Format::R16_UINT, Format::R16_SINT,
            Format::R16_SFLOAT, Format::RG8_UNORM, Format::RG8_SNORM, Format::RG8_UINT,
            Format::RG8_SINT, Format::R32_UINT, Format::R32_SINT, Format::R32_SFLOAT,
            Format::RG16_UNORM, Format::RG16_SNORM, Format::RG16_UINT, Format::RG16_SINT,
            Format::RG16_SFLOAT, Format::RGBA8_UNORM, Format::RGBA8_SNORM,
            Format::RGBA8_UINT, Format::RGBA8_SINT, Format::RGBA16_UNORM,
            Format::RGBA16_SFLOAT, Format::R32G32_UINT, Format::R32G32_SINT,
            Format::R32G32_SFLOAT, Format::R32G32B32_UINT, Format::R32G32B32_SINT,
            Format::R32G32B32_SFLOAT, Format::RGBA32_UINT, Format::RGBA32_SINT,
            Format::RGBA32_SFLOAT,
        ];
        let mut seen: Vec<(u32, Format)> = Vec::new();
        for f in uncompressed {
            let code = f.to_vulkan();
            assert_ne!(code, 0, "{f:?} must map to a real VkFormat");
            if let Some((_, other)) = seen.iter().find(|(c, _)| *c == code) {
                panic!("{f:?} and {other:?} both map to VkFormat {code}");
            }
            seen.push((code, f));
        }
    }

    /// Bug №178: R8_SNORM = 62 and R8_UINT = 63 were swapped, R32_SFLOAT = 41
    /// collided with R32_UINT, and RGBA8_SNORM = 74 is a YCBCR format.
    #[test]
    fn dxgi_format_codes_are_the_spec_values() {
        assert_eq!(Format::R8_UNORM.to_dxgi(), 61);
        assert_eq!(Format::R8_UINT.to_dxgi(), 62);
        assert_eq!(Format::R8_SNORM.to_dxgi(), 63);
        assert_eq!(Format::R8_SINT.to_dxgi(), 64);
        assert_eq!(Format::R16_SFLOAT.to_dxgi(), 54);
        assert_eq!(Format::R16_UNORM.to_dxgi(), 55);
        assert_eq!(Format::RG16_UNORM.to_dxgi(), 35);
        assert_eq!(Format::R32_SFLOAT.to_dxgi(), 40);
        assert_eq!(Format::R32_UINT.to_dxgi(), 41);
        assert_eq!(Format::R32_SINT.to_dxgi(), 42);
        assert_eq!(Format::RGBA8_UNORM.to_dxgi(), 28);
        assert_eq!(Format::RGBA8_SNORM.to_dxgi(), 31);
        assert_eq!(Format::RGBA16_SFLOAT.to_dxgi(), 10);
        assert_eq!(Format::RGBA32_SFLOAT.to_dxgi(), 2);
    }

    /// Bug №178: D32_SFLOAT mapped to 0x88F2, which is not a GL depth format.
    #[test]
    fn gl_depth_formats_are_real_enumerants() {
        assert_eq!(Format::D16_UNORM.to_gl(), 0x81A5);
        assert_eq!(Format::D32_SFLOAT.to_gl(), 0x8CAC);
        assert_eq!(Format::D24_UNORM_S8_UINT.to_gl(), 0x88F0);
        assert_eq!(Format::D32_SFLOAT_S8_UINT.to_gl(), 0x8CAD);
        assert_ne!(Format::D32_SFLOAT.to_gl(), 0x88F2);
    }

    /// A format with no equivalent in a given API must report absence (0)
    /// rather than a nearby value. Returning a plausible-looking wrong code is
    /// the same defect as the original table.
    #[test]
    fn missing_equivalents_report_zero_not_a_guess() {
        // No VkFormat for a standalone alpha format.
        assert_eq!(Format::A8_UNORM.to_vulkan(), 0);
        // No DXGI format for combined depth32float+stencil8.
        assert_eq!(Format::D32_SFLOAT_S8_UINT.to_dxgi(), 0);
    }

    /// Bug №232: `hash_pipeline_desc` returned the constant 0 for everything,
    /// so any cache keyed by it collapsed all pipelines onto one entry.
    #[test]
    fn pipeline_hashes_distinguish_different_descriptions() {
        let a = (1u32, 2u32, "alpha");
        let b = (1u32, 2u32, "alpha");
        let c = (1u32, 3u32, "alpha");
        let d = (1u32, 2u32, "beta");

        assert_eq!(hash_pipeline_desc(&a), hash_pipeline_desc(&b), "equal input, equal key");
        assert_ne!(hash_pipeline_desc(&a), hash_pipeline_desc(&c), "a field difference must show");
        assert_ne!(hash_pipeline_desc(&a), hash_pipeline_desc(&d), "a string difference must show");
        assert_ne!(hash_pipeline_desc(&a), 0, "the key must not be the old constant");
    }

    #[test]
    fn hash_key_separates_value_kinds() {
        // Same bytes, different tag: must not collide.
        assert_ne!(hash_key("a", &1u32), hash_key("b", &1u32));
        // The plain byte hash still works.
        assert_eq!(hash_data(b"abc"), hash_data(b"abc"));
        assert_ne!(hash_data(b"abc"), hash_data(b"abd"));
        assert_eq!(hash_string("abc"), hash_data(b"abc"));
    }

    fn is_power_of_two(v: u64) -> bool {
        v != 0 && v & (v - 1) == 0
    }
}
