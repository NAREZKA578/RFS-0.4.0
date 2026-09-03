//! Vulkan Device implementation.

use crate::rhi::{
    BackendDescriptor, BackendType, Buffer, BufferDescriptor, CommandEncoder, Device, Fence,
    GpuInfo, Pipeline, PipelineDescriptor, Shader, ShaderDescriptor, Texture, TextureDescriptor,
};
use ash::vk;
use std::ffi::CStr;
use std::sync::Arc;

/// Vulkan device.
pub struct VulkanDevice {
    entry: ash::Entry,
    instance: ash::Instance,
    physical_device: vk::PhysicalDevice,
    device_arc: Arc<ash::Device>,
    graphics_queue: vk::Queue,
    present_queue: vk::Queue,
    graphics_queue_family: u32,
    present_queue_family: u32,
    gpu_info: GpuInfo,
    memory_properties: vk::PhysicalDeviceMemoryProperties,
    debug_utils: Option<ash::ext::debug_utils::Instance>,
    #[cfg(debug_assertions)]
    debug_messenger: Option<vk::DebugUtilsMessengerEXT>,
}

/// Picks the best physical device by scoring queue capabilities, dedicated
/// memory, and device type.
fn pick_best_physical_device(
    instance: &ash::Instance,
    devices: &[vk::PhysicalDevice],
) -> Result<vk::PhysicalDevice, Box<dyn std::error::Error>> {
    let mut best: Option<(i64, vk::PhysicalDevice)> = None;

    for &device in devices {
        let props = unsafe { instance.get_physical_device_properties(device) };
        let mem = unsafe { instance.get_physical_device_memory_properties(device) };
        let queues = unsafe { instance.get_physical_device_queue_family_properties(device) };

        let has_graphics = queues
            .iter()
            .any(|q| q.queue_flags.contains(vk::QueueFlags::GRAPHICS));
        if !has_graphics {
            continue;
        }

        let mut score: i64 = 0;
        score += mem
            .memory_heaps
            .iter()
            .map(|h| h.size as i64)
            .sum::<i64>()
            .min(1 << 40);
        score += match props.device_type {
            vk::PhysicalDeviceType::DISCRETE_GPU => 10_000,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 5_000,
            vk::PhysicalDeviceType::VIRTUAL_GPU => 2_000,
            _ => 0,
        };

        match best {
            Some((best_score, _)) if best_score >= score => {}
            _ => {
                best = Some((score, device));
            }
        }
    }

    best.map(|(_, d)| d)
        .ok_or_else(|| "No suitable Vulkan physical device found".into())
}

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_types: vk::DebugUtilsMessageTypeFlagsEXT,
    callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT<'_>,
    _user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    let data = &*callback_data;
    let level = match message_severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => "ERROR",
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => "WARN",
        vk::DebugUtilsMessageSeverityFlagsEXT::INFO => "INFO",
        _ => "VERBOSE",
    };
    let message = if data.p_message.is_null() {
        "<no message>".to_string()
    } else {
        unsafe { CStr::from_ptr(data.p_message).to_string_lossy().to_string() }
    };
    let _ = (message_types, level);
    eprintln!("[Vulkan][{}] {message}", level);
    vk::FALSE
}

impl VulkanDevice {
    /// Creates a new Vulkan device.
    pub fn new(desc: &BackendDescriptor) -> Result<Self, Box<dyn std::error::Error>> {
        // Create Vulkan instance
        let entry = unsafe { ash::Entry::load()? };

        let app_info = vk::ApplicationInfo::default()
            .application_name(c"RFS-0.3")
            .application_version(vk::make_api_version(0, 0, 3, 0))
            .engine_name(c"RFS-RHI")
            .engine_version(vk::make_api_version(0, 0, 1, 0))
            .api_version(vk::API_VERSION_1_3);

        // Build instance extension list
        let mut extension_names: Vec<*const std::ffi::c_char> = vec![
            ash::khr::swapchain::NAME.as_ptr(),
            ash::khr::surface::NAME.as_ptr(),
        ];
        #[cfg(target_os = "windows")]
        {
            extension_names.push(ash::khr::win32_surface::NAME.as_ptr());
        }

        // Build validation layer list
        let mut layer_names: Vec<*const std::ffi::c_char> = Vec::new();
        let validation_layer_name = c"VK_LAYER_KHRONOS_validation";

        if desc.enable_validation {
            extension_names.push(ash::ext::debug_utils::NAME.as_ptr());
            layer_names.push(validation_layer_name.as_ptr());
        }

        let create_info = vk::InstanceCreateInfo::default()
            .application_info(&app_info)
            .enabled_extension_names(&extension_names)
            .enabled_layer_names(&layer_names);

        let instance = unsafe { entry.create_instance(&create_info, None)? };

        // Setup debug messenger if validation enabled
        let debug_utils = if desc.enable_validation {
            Some(ash::ext::debug_utils::Instance::new(&entry, &instance))
        } else {
            None
        };
        #[cfg(debug_assertions)]
        let mut debug_messenger = None;
        #[cfg(not(debug_assertions))]
        let _debug_utils = &debug_utils;
        #[cfg(debug_assertions)]
        if let (Some(debug_utils), true) = (&debug_utils, desc.enable_validation) {
            let create_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
                .message_severity(
                    vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                        | vk::DebugUtilsMessageSeverityFlagsEXT::ERROR,
                )
                .message_type(
                    vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                        | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                        | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
                )
                .pfn_user_callback(Some(vulkan_debug_callback));
            match unsafe { debug_utils.create_debug_utils_messenger(&create_info, None) } {
                Ok(messenger) => debug_messenger = Some(messenger),
                Err(e) => eprintln!("Failed to create debug messenger: {e}"),
            }
        }

        // Select physical device with scoring
        let physical_devices = unsafe { instance.enumerate_physical_devices()? };
        if physical_devices.is_empty() {
            return Err("No Vulkan-capable GPU found".into());
        }

        // If preferred_adapter is set, try to use it
        let physical_device = if let Some(preferred) = desc.preferred_adapter {
            if preferred < physical_devices.len() {
                physical_devices[preferred]
            } else {
                pick_best_physical_device(&instance, &physical_devices)?
            }
        } else {
            pick_best_physical_device(&instance, &physical_devices)?
        };

        let device_properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let device_name = unsafe {
            CStr::from_ptr(device_properties.device_name.as_ptr())
                .to_string_lossy()
                .to_string()
        };

        let memory_properties =
            unsafe { instance.get_physical_device_memory_properties(physical_device) };

        // Find queue families
        let queue_family_properties =
            unsafe { instance.get_physical_device_queue_family_properties(physical_device) };

        let graphics_queue_family = queue_family_properties
            .iter()
            .enumerate()
            .find(|(_, props)| props.queue_flags.contains(vk::QueueFlags::GRAPHICS))
            .map(|(i, _)| i as u32)
            .ok_or("No graphics queue family found")?;

        // TODO: Proper present queue family detection requires a VkSurfaceKHR.
        // For now, use the graphics queue family. This works on most GPUs where
        // graphics and present share the same family. Fix in Phase 5 (Swapchain).
        let present_queue_family = graphics_queue_family;

        // Create logical device
        let queue_priorities = [1.0f32];
        let queue_create_info = vk::DeviceQueueCreateInfo::default()
            .queue_family_index(graphics_queue_family)
            .queue_priorities(&queue_priorities);

        let device_extension_names = [ash::khr::swapchain::NAME.as_ptr()];

        // Request device features (sampler anisotropy, geometry shader, etc.)
        let mut device_features = vk::PhysicalDeviceFeatures::default();
        device_features.sampler_anisotropy = vk::TRUE;
        device_features.geometry_shader = vk::TRUE;
        device_features.wide_lines = vk::TRUE;
        device_features.fill_mode_non_solid = vk::TRUE;

        let device_create_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(std::slice::from_ref(&queue_create_info))
            .enabled_extension_names(&device_extension_names)
            .enabled_features(&device_features);

        let device = unsafe { instance.create_device(physical_device, &device_create_info, None)? };

        let device_arc = Arc::new(device);

        let graphics_queue = unsafe { device_arc.get_device_queue(graphics_queue_family, 0) };
        let present_queue = unsafe { device_arc.get_device_queue(present_queue_family, 0) };

        let gpu_info = GpuInfo::new(
            "Vulkan",
            &device_name,
            &format!(
                "Vulkan {}.{}.{}",
                vk::api_version_major(vk::API_VERSION_1_3),
                vk::api_version_minor(vk::API_VERSION_1_3),
                vk::api_version_patch(vk::API_VERSION_1_3)
            ),
            BackendType::Vulkan,
        );

        Ok(Self {
            entry,
            instance,
            physical_device,
            device_arc,
            graphics_queue,
            present_queue,
            graphics_queue_family,
            present_queue_family,
            gpu_info,
            memory_properties,
            debug_utils,
            #[cfg(debug_assertions)]
            debug_messenger,
        })
    }

    /// Returns a reference to the Vulkan instance.
    pub fn instance(&self) -> &ash::Instance {
        &self.instance
    }

    /// Returns the ash entry point loader.
    pub fn entry(&self) -> &ash::Entry {
        &self.entry
    }

    /// Returns a reference to the logical device.
    pub fn device_arc(&self) -> &ash::Device {
        &self.device_arc
    }

    /// Returns the physical device.
    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.physical_device
    }

    /// Returns a reference to the Vulkan device.
    pub fn device(&self) -> &ash::Device {
        &self.device_arc
    }

    /// Returns the graphics queue.
    pub fn graphics_queue(&self) -> vk::Queue {
        self.graphics_queue
    }

    /// Returns the present queue.
    pub fn present_queue(&self) -> vk::Queue {
        self.present_queue
    }

    /// Returns the graphics queue family index.
    pub fn graphics_queue_family(&self) -> u32 {
        self.graphics_queue_family
    }

    /// Returns the present queue family index.
    pub fn present_queue_family(&self) -> u32 {
        self.present_queue_family
    }

    /// Returns the memory properties.
    pub fn memory_properties(&self) -> &vk::PhysicalDeviceMemoryProperties {
        &self.memory_properties
    }

    /// Finds a memory type index that matches the given properties.
    pub fn find_memory_type(
        &self,
        type_filter: u32,
        properties: vk::MemoryPropertyFlags,
    ) -> Option<u32> {
        (0..self.memory_properties.memory_type_count).find(|&i| {
            (type_filter & (1 << i)) != 0
                && self.memory_properties.memory_types[i as usize]
                    .property_flags
                    .contains(properties)
        })
    }
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        unsafe {
            #[cfg(debug_assertions)]
            if let (Some(du), Some(messenger)) = (&self.debug_utils, self.debug_messenger) {
                du.destroy_debug_utils_messenger(messenger, None);
            }
            self.device_arc.destroy_device(None);
            self.instance.destroy_instance(None);
        }
    }
}

/// Vulkan device wrapper that can be shared as Arc<dyn Device>.
pub struct VulkanDeviceHandle {
    inner: Arc<VulkanDevice>,
}

impl VulkanDeviceHandle {
    pub fn new(device: Arc<VulkanDevice>) -> Self {
        Self { inner: device }
    }

    #[allow(dead_code)]
    pub fn inner(&self) -> &Arc<VulkanDevice> {
        &self.inner
    }
}

impl Device for VulkanDeviceHandle {
    fn create_buffer(
        &self,
        descriptor: &BufferDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Box<dyn Buffer>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::buffer::VulkanBuffer::new(
            &self.inner.device_arc,
            descriptor,
            data,
            self.inner.graphics_queue_family,
            &self.inner.memory_properties,
        )?))
    }

    fn create_texture(
        &self,
        descriptor: &TextureDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Box<dyn Texture>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::texture::VulkanTexture::new_with_queue(
            &self.inner.device_arc,
            descriptor,
            data,
            &self.inner.memory_properties,
            Some(self.inner.graphics_queue),
            self.inner.graphics_queue_family,
        )?))
    }

    fn create_shader(
        &self,
        descriptor: &ShaderDescriptor,
    ) -> Result<Box<dyn Shader>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::shader::VulkanShader::new(
            &self.inner.device_arc,
            descriptor,
        )?))
    }

    fn create_pipeline(
        &self,
        descriptor: &PipelineDescriptor,
    ) -> Result<Box<dyn Pipeline>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::pipeline::VulkanPipeline::new(
            &self.inner.device_arc,
            descriptor,
        )?))
    }

    fn create_command_encoder(
        &self,
    ) -> Result<Box<dyn CommandEncoder>, Box<dyn std::error::Error>> {
        Ok(Box::new(super::command::VulkanCommandEncoder::new(
            &self.inner.device_arc,
            self.inner.graphics_queue_family,
        )?))
    }

    fn submit(
        &self,
        commands: &[&dyn CommandEncoder],
        fence: Option<&mut Fence>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        super::command::VulkanCommandEncoder::submit_batch(
            &self.inner.device_arc,
            self.inner.graphics_queue,
            commands,
            fence,
        )
    }

    fn create_fence(&self, signaled: bool) -> Result<Fence, Box<dyn std::error::Error>> {
        Ok(super::sync::VulkanFence::create(
            &self.inner.device_arc,
            signaled,
        ))
    }

    fn wait_for_fence(
        &self,
        fence: &Fence,
        timeout_ns: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        super::sync::VulkanFence::wait(&self.inner.device_arc, fence, timeout_ns)
    }

    fn destroy_fence(&self, fence: Fence) {
        super::sync::VulkanFence::destroy(&self.inner.device_arc, fence)
    }

    fn wait_idle(&self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe { self.inner.device_arc.device_wait_idle()? };
        Ok(())
    }

    fn backend_type(&self) -> BackendType {
        BackendType::Vulkan
    }

    fn gpu_info(&self) -> GpuInfo {
        self.inner.gpu_info.clone()
    }

    fn clear(&self, _color: [f32; 4], _depth: f32) {
        // Vulkan clears are encoded inside render passes via LoadOp::Clear;
        // there is no out-of-pass clear. No-op.
    }
}
