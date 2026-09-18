//! Device Capabilities
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::core::device::MemoryProperties;
use crate::types::*;

/// Device capabilities derived from a physical device.
#[derive(Debug, Clone, Default)]
pub struct DeviceCaps {
    pub features: Features,
    pub limits: Limits,
    pub memory_properties: MemoryProperties,
}

/// Required feature that is not supported by a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingFeature {
    /// Machine-readable feature identifier (matches the `Features` field).
    pub name: &'static str,
    /// Human-readable description of the feature.
    pub description: &'static str,
}

impl DeviceCaps {
    /// Returns `true` when every feature requested in `required` is supported.
    pub fn supports(&self, required: &Features) -> bool {
        self.missing_features(required).is_empty()
    }

    /// Returns the list of requested features the device does not support.
    pub fn missing_features(&self, required: &Features) -> Vec<MissingFeature> {
        let available = &self.features;
        let mut missing = Vec::new();

        macro_rules! check {
            ($name:literal, $desc:literal, $field:ident) => {
                if required.$field && !available.$field {
                    missing.push(MissingFeature {
                        name: $name,
                        description: $desc,
                    });
                }
            };
        }

        check!(
            "geometry_shader",
            "Geometry shaders are not supported",
            geometry_shader
        );
        check!(
            "tessellation_shader",
            "Tessellation shaders are not supported",
            tessellation_shader
        );
        check!("mesh_shader", "Mesh shaders are not supported", mesh_shader);
        check!(
            "shader_float64",
            "64-bit float shader arithmetic is not supported",
            shader_float64
        );
        check!(
            "shader_int64",
            "64-bit integer shader arithmetic is not supported",
            shader_int64
        );
        check!(
            "multi_draw_indirect",
            "Multi-draw indirect is not supported",
            multi_draw_indirect
        );
        check!(
            "depth_bounds",
            "Depth bounds testing is not supported",
            depth_bounds
        );
        check!(
            "depth_clamp",
            "Depth clamping is not supported",
            depth_clamp
        );
        check!(
            "texture_compression_bc",
            "BC texture compression is not supported",
            texture_compression_bc
        );
        check!(
            "texture_compression_astc",
            "ASTC texture compression is not supported",
            texture_compression_astc
        );
        check!(
            "texture_compression_etc2",
            "ETC2 texture compression is not supported",
            texture_compression_etc2
        );
        check!(
            "sampler_anisotropy",
            "Anisotropic filtering is not supported",
            sampler_anisotropy
        );
        check!(
            "storage_buffer",
            "Storage buffers are not supported",
            storage_buffer
        );
        check!(
            "storage_image",
            "Storage images are not supported",
            storage_image
        );
        check!("compute", "Compute shaders are not supported", compute);
        check!(
            "indirect_compute",
            "Indirect compute dispatch is not supported",
            indirect_compute
        );
        check!(
            "ray_tracing",
            "Ray tracing is not supported",
            ray_tracing
        );
        check!("ray_query", "Ray queries are not supported", ray_query);
        check!(
            "variable_rate_shading",
            "Variable rate shading is not supported",
            variable_rate_shading
        );
        check!(
            "conservative_raster",
            "Conservative rasterization is not supported",
            conservative_raster
        );
        check!(
            "sparse_binding",
            "Sparse binding is not supported",
            sparse_binding
        );
        check!(
            "memory_budget",
            "Memory budget queries are not supported",
            memory_budget
        );
        check!(
            "descriptor_indexing",
            "Descriptor indexing is not supported",
            descriptor_indexing
        );
        check!(
            "buffer_device_address",
            "Buffer device address is not supported",
            buffer_device_address
        );

        missing
    }

    /// Returns the highest sample count the device can render to.
    pub fn max_sample_count(&self) -> SampleCount {
        let max = self.limits.max_sample_count.as_count();
        match max {
            n if n >= 64 => SampleCount::X64,
            n if n >= 32 => SampleCount::X32,
            n if n >= 16 => SampleCount::X16,
            n if n >= 8 => SampleCount::X8,
            n if n >= 4 => SampleCount::X4,
            n if n >= 2 => SampleCount::X2,
            _ => SampleCount::X1,
        }
    }

    /// Returns `true` if the device supports a given MSAA sample count.
    pub fn supports_sample_count(&self, count: SampleCount) -> bool {
        count.as_count() <= self.limits.max_sample_count.as_count()
    }

    /// Returns the largest supported dimension for a side of a texture.
    pub fn max_texture_size(&self) -> u32 {
        self.limits.max_texture_size
    }

    /// Returns the largest supported buffer size in bytes.
    pub fn max_buffer_size(&self) -> u64 {
        self.limits.max_buffer_size
    }
}