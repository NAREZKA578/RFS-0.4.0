//! Vulkan instance creation, debug messenger and physical device enumeration.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::config::RhiConfig;
use crate::core::{
    MemoryHeap, MemoryProperties, MemoryType, PhysicalDevice, PhysicalDeviceType, QueueFamily,
};
use crate::error::*;
use crate::types::{
    Extent3D, Features, Limits, MemoryHeapFlags, MemoryPropertyFlags, QueueFlags, SampleCount,
};
use ash::vk;
use std::ffi::{c_char, c_void, CStr};
use std::sync::Arc;

pub const APP_NAME: &CStr = c"RFS-Client";
pub const ENGINE_NAME: &CStr = c"RFS";
pub const API_VERSION: u32 = vk::make_api_version(0, 1, 3, 0);
pub const VALIDATION_LAYER: &CStr = c"VK_LAYER_KHRONOS_validation";
pub const DEBUG_UTILS_EXT: &CStr = c"VK_EXT_debug_utils";

/// Instance extensions required to present to a window.
///
/// The first is the generic surface interface; the second is the Win32
/// platform binding. Both are instance-level — a `VkSurfaceKHR` is created from
/// the instance, not the device.
pub const SURFACE_EXTENSIONS: [&CStr; 2] =
    [c"VK_KHR_surface", c"VK_KHR_win32_surface"];

pub(crate) fn vk_fail(result: vk::Result, context: &str) -> RhiError {
    RhiError::BackendError(format!("{context}: {result:?}"))
}

pub(crate) fn load_loader() -> RhiResult<ash::Entry> {
    unsafe { ash::Entry::load() }
        .map_err(|e| RhiError::NoBackendForApi(format!("Vulkan loader unavailable: {e}")))
}

/// Owns all instance-level Vulkan objects. Shared between the backend and the
/// logical devices it created; the instance is destroyed only when the last
/// owner is dropped. This keeps the `VkInstance` alive for as long as any
/// device created from it exists.
pub(crate) struct DestroyableInstance {
    pub entry: ash::Entry,
    pub instance: ash::Instance,
    pub debug_utils: Option<ash::ext::debug_utils::Instance>,
    pub debug_messenger: Option<vk::DebugUtilsMessengerEXT>,
}

impl Drop for DestroyableInstance {
    fn drop(&mut self) {
        unsafe {
            if let (Some(debug_utils), Some(messenger)) =
                (self.debug_utils.as_ref(), self.debug_messenger.take())
            {
                debug_utils.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}

fn null_or<T>(ptr: *const T, empty: bool) -> *const T {
    if empty {
        std::ptr::null()
    } else {
        ptr
    }
}

fn has_extension(names: &[&CStr], wanted: &CStr) -> bool {
    names.contains(&wanted)
}

unsafe extern "system" fn debug_messenger_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_types: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut c_void,
) -> vk::Bool32 {
    use vk::DebugUtilsMessageSeverityFlagsEXT as Severity;
    let level = if message_severity.contains(Severity::ERROR) {
        "ERROR"
    } else if message_severity.contains(Severity::WARNING) {
        "WARNING"
    } else if message_severity.contains(Severity::INFO) {
        "INFO"
    } else {
        "VERBOSE"
    };
    if !callback_data.is_null() {
        let data = &*callback_data;
        let message = CStr::from_ptr(data.p_message).to_string_lossy();
        if data.p_message_id_name.is_null() {
            eprintln!("[Vulkan {level}] {message}");
        } else {
            let id = CStr::from_ptr(data.p_message_id_name).to_string_lossy();
            eprintln!("[Vulkan {level}] {id}: {message}");
        }
    }
    vk::FALSE
}

pub(crate) fn create_instance(config: &RhiConfig) -> RhiResult<Arc<DestroyableInstance>> {
    let entry = load_loader()?;

    let app_info = vk::ApplicationInfo {
        p_application_name: APP_NAME.as_ptr(),
        application_version: vk::make_api_version(0, 0, 1, 0),
        p_engine_name: ENGINE_NAME.as_ptr(),
        engine_version: vk::make_api_version(0, 0, 1, 0),
        api_version: API_VERSION,
        ..Default::default()
    };

    let extension_properties = unsafe { entry.enumerate_instance_extension_properties(None) }
        .map_err(|e| vk_fail(e, "enumerate instance extensions"))?;
    let available_extensions: Vec<&CStr> = extension_properties
        .iter()
        .map(|p| unsafe { CStr::from_ptr(p.extension_name.as_ptr()) })
        .collect();

    let mut enabled_extensions: Vec<&CStr> = Vec::new();
    if has_extension(&available_extensions, DEBUG_UTILS_EXT) {
        enabled_extensions.push(DEBUG_UTILS_EXT);
    }
    // Presentation needs these at the *instance* level. Without them a
    // VkSurfaceKHR cannot be created at all, which is why the swapchain was a
    // stub: offscreen work needs no surface, so the extensions were never
    // requested and creating one would have failed validation.
    //
    // Requested only if present, and their absence is reported when a surface
    // is actually built rather than breaking offscreen rendering.
    for wanted in SURFACE_EXTENSIONS {
        if has_extension(&available_extensions, wanted) {
            enabled_extensions.push(wanted);
        }
    }

    let mut enabled_layers: Vec<&CStr> = Vec::new();
    if config.validation {
        let layer_properties = unsafe { entry.enumerate_instance_layer_properties() }
            .map_err(|e| vk_fail(e, "enumerate instance layers"))?;
        let has_validation = layer_properties
            .iter()
            .any(|p| unsafe { CStr::from_ptr(p.layer_name.as_ptr()) } == VALIDATION_LAYER);
        if has_validation {
            enabled_layers.push(VALIDATION_LAYER);
        }
    }

    let extension_ptrs: Vec<*const c_char> = enabled_extensions.iter().map(|s| s.as_ptr()).collect();
    let layer_ptrs: Vec<*const c_char> = enabled_layers.iter().map(|s| s.as_ptr()).collect();

    let create_info = vk::InstanceCreateInfo {
        p_application_info: &app_info,
        enabled_layer_count: layer_ptrs.len() as u32,
        pp_enabled_layer_names: null_or(layer_ptrs.as_ptr(), layer_ptrs.is_empty()),
        enabled_extension_count: extension_ptrs.len() as u32,
        pp_enabled_extension_names: null_or(extension_ptrs.as_ptr(), extension_ptrs.is_empty()),
        ..Default::default()
    };

    let instance = unsafe { entry.create_instance(&create_info, None) }
        .map_err(|e| vk_fail(e, "create instance"))?;

    let debug_utils_available = has_extension(&available_extensions, DEBUG_UTILS_EXT);
    let debug_utils = debug_utils_available
        .then(|| ash::ext::debug_utils::Instance::new(&entry, &instance));

    let debug_messenger = if config.validation {
        if let Some(debug_utils) = &debug_utils {
            let messenger_info = vk::DebugUtilsMessengerCreateInfoEXT {
                message_severity: vk::DebugUtilsMessageSeverityFlagsEXT::VERBOSE
                    | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                    | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                    | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                message_type: vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                pfn_user_callback: Some(debug_messenger_callback),
                ..Default::default()
            };
            let messenger = unsafe { debug_utils.create_debug_utils_messenger(&messenger_info, None) }
                .map_err(|e| vk_fail(e, "create debug messenger"))?;
            Some(messenger)
        } else {
            None
        }
    } else {
        None
    };

    Ok(Arc::new(DestroyableInstance {
        entry,
        instance,
        debug_utils,
        debug_messenger,
    }))
}

pub(crate) fn enumerate_physical_devices(
    instance: &ash::Instance,
) -> RhiResult<Vec<vk::PhysicalDevice>> {
    unsafe { instance.enumerate_physical_devices() }
        .map_err(|e| vk_fail(e, "enumerate physical devices"))
}

fn map_device_type(device_type: vk::PhysicalDeviceType) -> PhysicalDeviceType {
    match device_type {
        vk::PhysicalDeviceType::DISCRETE_GPU => PhysicalDeviceType::Discrete,
        vk::PhysicalDeviceType::INTEGRATED_GPU => PhysicalDeviceType::Integrated,
        vk::PhysicalDeviceType::VIRTUAL_GPU => PhysicalDeviceType::Virtual,
        vk::PhysicalDeviceType::CPU => PhysicalDeviceType::Cpu,
        _ => PhysicalDeviceType::Virtual,
    }
}

fn is_enabled(feature: vk::Bool32) -> bool {
    feature == vk::TRUE
}

fn map_features(features: &vk::PhysicalDeviceFeatures) -> Features {
    Features {
        geometry_shader: is_enabled(features.geometry_shader),
        tessellation_shader: is_enabled(features.tessellation_shader),
        mesh_shader: false,
        shader_float64: is_enabled(features.shader_float64),
        shader_int64: is_enabled(features.shader_int64),
        multi_draw_indirect: is_enabled(features.multi_draw_indirect),
        depth_bounds: is_enabled(features.depth_bounds),
        depth_clamp: is_enabled(features.depth_clamp),
        texture_compression_bc: is_enabled(features.texture_compression_bc),
        texture_compression_astc: is_enabled(features.texture_compression_astc_ldr),
        texture_compression_etc2: is_enabled(features.texture_compression_etc2),
        sampler_anisotropy: is_enabled(features.sampler_anisotropy),
        storage_buffer: true,
        storage_image: true,
        compute: true,
        indirect_compute: true,
        ray_tracing: false,
        ray_query: false,
        variable_rate_shading: false,
        conservative_raster: false,
        sparse_binding: is_enabled(features.sparse_binding),
        memory_budget: false,
        descriptor_indexing: false,
        buffer_device_address: false,
    }
}

fn max_sample_count(flags: [vk::SampleCountFlags; 3]) -> SampleCount {
    let mut best = 0u32;
    for flag_set in flags {
        for (bit, flag) in [
            (1u32, vk::SampleCountFlags::TYPE_1),
            (2, vk::SampleCountFlags::TYPE_2),
            (4, vk::SampleCountFlags::TYPE_4),
            (8, vk::SampleCountFlags::TYPE_8),
            (16, vk::SampleCountFlags::TYPE_16),
            (32, vk::SampleCountFlags::TYPE_32),
            (64, vk::SampleCountFlags::TYPE_64),
        ] {
            if flag_set.contains(flag) && bit > best {
                best = bit;
            }
        }
    }
    match best {
        2 => SampleCount::X2,
        4 => SampleCount::X4,
        8 => SampleCount::X8,
        16 => SampleCount::X16,
        32 => SampleCount::X32,
        64 => SampleCount::X64,
        _ => SampleCount::X1,
    }
}

fn map_limits(limits: &vk::PhysicalDeviceLimits, max_buffer_size: u64) -> Limits {
    let max_texture_size = limits
        .max_image_dimension1_d
        .max(limits.max_image_dimension2_d)
        .max(limits.max_image_dimension3_d);
    Limits {
        max_texture_size,
        max_texture_layers: limits.max_image_array_layers,
        max_texture_mips: max_texture_size.max(1).next_power_of_two().trailing_zeros() + 1,
        max_buffer_size,
        max_uniform_buffer_size: limits.max_uniform_buffer_range as u64,
        max_storage_buffer_size: limits.max_storage_buffer_range as u64,
        max_push_constants_size: limits.max_push_constants_size,
        max_bound_descriptor_sets: limits.max_bound_descriptor_sets,
        max_per_stage_descriptors: limits.max_per_stage_resources,
        max_color_attachments: limits.max_color_attachments,
        max_sample_count: max_sample_count([
            limits.framebuffer_color_sample_counts,
            limits.framebuffer_depth_sample_counts,
            limits.framebuffer_stencil_sample_counts,
        ]),
        max_viewports: limits.max_viewports,
        max_compute_work_group_count: [
            limits.max_compute_work_group_count[0],
            limits.max_compute_work_group_count[1],
            limits.max_compute_work_group_count[2],
        ],
        max_compute_work_group_size: [
            limits.max_compute_work_group_size[0],
            limits.max_compute_work_group_size[1],
            limits.max_compute_work_group_size[2],
        ],
    }
}

fn map_memory_property_flags(flags: vk::MemoryPropertyFlags) -> MemoryPropertyFlags {
    let mut out = MemoryPropertyFlags::empty();
    if flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL) {
        out |= MemoryPropertyFlags::DEVICE_LOCAL;
    }
    if flags.contains(vk::MemoryPropertyFlags::HOST_VISIBLE) {
        out |= MemoryPropertyFlags::HOST_VISIBLE;
    }
    if flags.contains(vk::MemoryPropertyFlags::HOST_COHERENT) {
        out |= MemoryPropertyFlags::HOST_COHERENT;
    }
    if flags.contains(vk::MemoryPropertyFlags::HOST_CACHED) {
        out |= MemoryPropertyFlags::HOST_CACHED;
    }
    out
}

fn map_memory_properties(memory: &vk::PhysicalDeviceMemoryProperties) -> MemoryProperties {
    let memory_types = memory.memory_types[..memory.memory_type_count as usize]
        .iter()
        .map(|t| MemoryType {
            flags: map_memory_property_flags(t.property_flags),
            heap_index: t.heap_index,
        })
        .collect();
    let memory_heaps = memory.memory_heaps[..memory.memory_heap_count as usize]
        .iter()
        .map(|h| MemoryHeap {
            size: h.size,
            flags: if h.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
                MemoryHeapFlags::DEVICE_LOCAL
            } else {
                MemoryHeapFlags::empty()
            },
        })
        .collect();
    MemoryProperties {
        memory_types,
        memory_heaps,
    }
}

fn map_queue_flags(flags: vk::QueueFlags) -> QueueFlags {
    let mut out = QueueFlags::empty();
    if flags.contains(vk::QueueFlags::GRAPHICS) {
        out |= QueueFlags::GRAPHICS;
    }
    if flags.contains(vk::QueueFlags::COMPUTE) {
        out |= QueueFlags::COMPUTE;
    }
    if flags.contains(vk::QueueFlags::TRANSFER) {
        out |= QueueFlags::TRANSFER;
    }
    if flags.contains(vk::QueueFlags::SPARSE_BINDING) {
        out |= QueueFlags::SPARSE_BINDING;
    }
    if flags.contains(vk::QueueFlags::PROTECTED) {
        out |= QueueFlags::PROTECTED;
    }
    out
}

fn map_queue_family(index: u32, family: &vk::QueueFamilyProperties) -> QueueFamily {
    QueueFamily {
        index,
        flags: map_queue_flags(family.queue_flags),
        queue_count: family.queue_count,
        timestamp_valid_bits: family.timestamp_valid_bits,
        min_image_transfer_granularity: Extent3D {
            width: family.min_image_transfer_granularity.width,
            height: family.min_image_transfer_granularity.height,
            depth: family.min_image_transfer_granularity.depth,
        },
    }
}

pub(crate) fn physical_device_report(
    instance: &ash::Instance,
    handle: vk::PhysicalDevice,
) -> PhysicalDevice {
    let (properties, features, memory, queue_families) = unsafe {
        (
            instance.get_physical_device_properties(handle),
            instance.get_physical_device_features(handle),
            instance.get_physical_device_memory_properties(handle),
            instance.get_physical_device_queue_family_properties(handle),
        )
    };
    let name = unsafe { CStr::from_ptr(properties.device_name.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let max_buffer_size = memory.memory_heaps[..memory.memory_heap_count as usize]
        .iter()
        .map(|heap| heap.size)
        .max()
        .unwrap_or(256 * 1024 * 1024);
    PhysicalDevice {
        name,
        vendor_id: properties.vendor_id,
        device_id: properties.device_id,
        device_type: map_device_type(properties.device_type),
        features: map_features(&features),
        limits: map_limits(&properties.limits, max_buffer_size),
        memory_properties: map_memory_properties(&memory),
        queue_families: queue_families
            .iter()
            .enumerate()
            .map(|(index, family)| map_queue_family(index as u32, family))
            .collect(),
    }
}