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

/// Takes anything displayable rather than `&str`, so a caller with a slot number
/// in the message does not have to allocate a `String` just to borrow it.
pub(crate) fn vk_fail(result: vk::Result, context: impl std::fmt::Display) -> RhiError {
    RhiError::BackendError(format!("{context}: {result:?}"))
}

pub(crate) fn load_loader() -> RhiResult<ash::Entry> {
    // SAFETY: ash::Entry::load finds and dlopens the system Vulkan loader; no user pointers involved.
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
        // SAFETY: instance and optional debug_messenger are valid handles owned by this struct;
        // Arc-based sharing guarantees this runs only when no other owner (e.g. a device) still needs it.
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

// SAFETY: Vulkan guarantees `callback_data` and its `p_message`/`p_message_id_name` are valid
// (non-null when this callback is invoked by the debug utils dispatcher) for the duration of
// this call; we only read them (via CStr::from_ptr) and never retain the pointers. Returning
// VK_FALSE is required so validation proceeds (returning TRUE would abort the call).
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
        if message_severity.contains(Severity::ERROR) {
            VALIDATION_ERROR_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
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

/// Whether the loader really gave us the validation layer. Bug №297.
static VALIDATION_LAYER_ACTIVE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Number of validation messages of severity `ERROR` seen so far.
///
/// Bug №292. The messenger callback only prints to stderr, and a Vulkan call with
/// a validation error still returns `vk::Success` — the driver has no reason to
/// fail the call, it just reports the misuse alongside it. So before this counter,
/// **no test in the project could detect a validation error**: assertions on
/// return values stayed green whether the API use was correct or not, and the
/// report scrolled past in captured output where nobody read it.
///
/// That is not a theoretical gap. It is why bug №290's hardcoded aspect mask
/// survived: the barrier translator was exercised by no test at all, and when the
/// first test did execute it, the negative control — the same test with the defect
/// deliberately reintroduced — passed too.
///
/// Read with [`validation_error_count`]. Intended for tests and for a debug
/// overlay; not a hot path, since it is only written when the layer reports an
/// error.
static VALIDATION_ERROR_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// How many validation `ERROR` messages have been reported.
///
/// See [`VALIDATION_ERROR_COUNT`] for why this exists. Intended usage is to sample
/// before and after the operation under test:
///
/// ```ignore
/// let before = rhi::validation_error_count();
/// device.submit_recorded(&commands).unwrap();
/// assert_eq!(rhi::validation_error_count(), before, "the validator reported an error");
/// ```
///
/// The layer must actually be enabled for this to see anything, and
/// `RhiConfig::validation` is `false` by default, so a test using this has to turn
/// it on — otherwise it asserts against a counter that can never move. Even then,
/// check [`validation_layer_active`]: enabling the flag in the config is a request,
/// and the loader may decline it.
pub fn validation_error_count() -> usize {
    VALIDATION_ERROR_COUNT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the validation layer is actually loaded. Bug №297.
///
/// `true` only if the loader found `VK_LAYER_KHRONOS_validation` and it was enabled
/// on the instance. A `false` result next to [`validation_error_count`] reading zero
/// means the count is zero because nobody is watching — which is exactly the state
/// that made a whole session's "validation is clean" statements meaningless.
///
/// **Only meaningful once an instance exists.** The flag is set inside
/// `create_instance`, so reading it *before* a `VulkanBackend` has been built reports
/// `false` whatever the config says. I hit exactly this while verifying a sweep and
/// concluded the layer was missing when it was present — one level below the mistake
/// the flag was added to prevent. So: create the backend first, then ask.
///
/// Not per-thread and not per-instance: the loader answers once per process, and a
/// second instance cannot disagree with the first about what the loader offers.
pub fn validation_layer_active() -> bool {
    VALIDATION_LAYER_ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
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

    // SAFETY: entry is a valid ash::Entry (just loaded); the returned vector is owned by us,
    // no pointers returned to caller that outlive this block.
    let extension_properties = unsafe { entry.enumerate_instance_extension_properties(None) }
        .map_err(|e| vk_fail(e, "enumerate instance extensions"))?;
    let available_extensions: Vec<&CStr> = extension_properties
        .iter()
        // SAFETY: p.extension_name is a valid null-terminated C string provided by Vulkan
        // for this enumeration; its lifetime is that of the owning `extension_properties` vec.
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
        // SAFETY: entry is a valid ash::Entry; returned vector is owned by us, no lifetime issue.
        let layer_properties = unsafe { entry.enumerate_instance_layer_properties() }
            .map_err(|e| vk_fail(e, "enumerate instance layers"))?;
        let has_validation = layer_properties
            .iter()
            // SAFETY: p.layer_name is a valid null-terminated C string from Vulkan enumeration,
            // lifetime tied to the owning layer_properties vec.
            .any(|p| unsafe { CStr::from_ptr(p.layer_name.as_ptr()) } == VALIDATION_LAYER);
        if has_validation {
            enabled_layers.push(VALIDATION_LAYER);
            VALIDATION_LAYER_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
        } else {
            // Bug №297, requested by AGENT-SBF2.
            //
            // This used to fall through silently, and it is the worst possible
            // failure mode for a validator: the caller asked for validation, did not
            // get it, and heard nothing at all. Every downstream conclusion then
            // inverts — "validation is clean" means "nothing is watching". On this
            // machine it was reached because the layer's own JSON advertised
            // `"api_version": "1.3.204.0"`, which does not exist, so the loader
            // reported the layer as absent.
            //
            // Loud, not fatal: a missing layer is a diagnostics problem, not a reason
            // to refuse to create a device, and refusing would make the whole class
            // of bug undebuggable on a machine without the SDK. But it cannot be
            // silent, and the message says what to do about it.
            //
            // The available names are listed, because the common cause is a `VK_LAYER_PATH`
            // that points at the wrong directory, and "not found" alone does not tell
            // anyone where to look.
            let available: Vec<String> = layer_properties
                .iter()
                // SAFETY: p.layer_name is a valid null-terminated C string from Vulkan
                // enumeration, lifetime tied to the owning layer_properties vec.
                .map(|p| unsafe { CStr::from_ptr(p.layer_name.as_ptr()) }.to_string_lossy())
                .map(|name| name.into_owned())
                .collect();
            eprintln!(
                "[rhi] validation was requested but the layer {:?} is not available. \
                 Validation is OFF, and a clean run then means nothing was checked. \
                 Check VK_LAYER_PATH (the loader searches that directory only) and the \
                 layer's own JSON api_version. Layers the loader can see: [{}]",
                VALIDATION_LAYER.to_string_lossy(),
                if available.is_empty() {
                    "none".to_string()
                } else {
                    available.join(", ")
                }
            );
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

    // SAFETY: entry is valid; create_info references live extension/layer name arrays that
    // outlive the call (extension_ptrs/layer_ptrs built above); p_next chain empty.
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
            // SAFETY: debug_utils is a valid loader over the live instance; messenger_info's
            // callback pointer refers to the free function `debug_messenger_callback`, which is
            // valid for the process lifetime; no allocator used (None).
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
    // SAFETY: instance is a valid live VkInstance; returned vector of handles is owned by the
    // caller, no pointers returned that outlive the call.
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
    // SAFETY: instance and physical device handle are valid; the returned structs/vectors are
    // owned copies (properties, features, memory, queue_families) — no dangling pointers.
    let (properties, features, memory, queue_families) = unsafe {
        (
            instance.get_physical_device_properties(handle),
            instance.get_physical_device_features(handle),
            instance.get_physical_device_memory_properties(handle),
            instance.get_physical_device_queue_family_properties(handle),
        )
    };
    // SAFETY: properties.device_name is a valid null-terminated C array inside the properties
    // struct returned (and owned) above; to_string_lossy copies out before it goes out of scope.
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