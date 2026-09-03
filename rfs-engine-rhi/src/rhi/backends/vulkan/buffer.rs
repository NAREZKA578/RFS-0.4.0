//! Vulkan Buffer implementation.

use super::registry;
use crate::rhi::{Buffer, BufferDescriptor, BufferUsage};
use ash::vk;
use ash::vk::Handle;
use std::sync::Arc;

/// Finds a memory type index that satisfies the required property flags.
fn find_memory_type(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    type_filter: u32,
    properties: vk::MemoryPropertyFlags,
) -> Option<u32> {
    (0..memory_properties.memory_type_count).find(|&i| {
        (type_filter & (1 << i)) != 0
            && memory_properties.memory_types[i as usize]
                .property_flags
                .contains(properties)
    })
}

/// Vulkan buffer with GPU memory.
pub struct VulkanBuffer {
    device: Arc<ash::Device>,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    size: u64,
    usage: BufferUsage,
}

impl VulkanBuffer {
    /// Creates a new Vulkan buffer.
    pub fn new(
        device: &Arc<ash::Device>,
        descriptor: &BufferDescriptor,
        data: Option<&[u8]>,
        queue_family_index: u32,
        memory_properties: &vk::PhysicalDeviceMemoryProperties,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let buffer_usage = match descriptor.usage {
            BufferUsage::Vertex => vk::BufferUsageFlags::VERTEX_BUFFER,
            BufferUsage::Index => vk::BufferUsageFlags::INDEX_BUFFER,
            BufferUsage::Uniform => vk::BufferUsageFlags::UNIFORM_BUFFER,
            BufferUsage::Storage => vk::BufferUsageFlags::STORAGE_BUFFER,
            BufferUsage::TransferSrc => vk::BufferUsageFlags::TRANSFER_SRC,
            BufferUsage::TransferDst => vk::BufferUsageFlags::TRANSFER_DST,
            BufferUsage::Indirect => vk::BufferUsageFlags::INDIRECT_BUFFER,
        };

        let queue_family = [queue_family_index];
        let buffer_info = vk::BufferCreateInfo::default()
            .size(descriptor.size)
            .usage(buffer_usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .queue_family_indices(&queue_family);

        let buffer = unsafe { device.create_buffer(&buffer_info, None)? };

        let memory_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };

        let memory_type_index = find_memory_type(
            memory_properties,
            memory_requirements.memory_type_bits,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )
        .ok_or("Failed to find suitable memory type for buffer")?;

        let memory_allocate_info = vk::MemoryAllocateInfo::default()
            .allocation_size(memory_requirements.size)
            .memory_type_index(memory_type_index);

        let memory = unsafe { device.allocate_memory(&memory_allocate_info, None)? };

        unsafe { device.bind_buffer_memory(buffer, memory, 0)? };

        // If data is provided, copy it to the buffer
        if let Some(data) = data {
            let mapped = unsafe {
                device.map_memory(memory, 0, descriptor.size, vk::MemoryMapFlags::empty())?
            };
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    mapped as *mut u8,
                    data.len().min(descriptor.size as usize),
                );
            }
            unsafe { device.unmap_memory(memory) };
        }

        registry::register_buffer(buffer);

        Ok(Self {
            device: device.clone(),
            buffer,
            memory,
            size: descriptor.size,
            usage: descriptor.usage,
        })
    }

    /// Returns the Vulkan buffer handle.
    pub fn vk_buffer(&self) -> vk::Buffer {
        self.buffer
    }

    /// Returns the Vulkan device memory handle.
    pub fn vk_memory(&self) -> vk::DeviceMemory {
        self.memory
    }
}

impl Drop for VulkanBuffer {
    fn drop(&mut self) {
        registry::unregister_buffer(self.buffer);
        unsafe {
            self.device.destroy_buffer(self.buffer, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

impl Buffer for VulkanBuffer {
    fn size(&self) -> u64 {
        self.size
    }

    fn usage(&self) -> BufferUsage {
        self.usage
    }

    fn update(&self, data: &[u8], offset: u64) -> Result<(), Box<dyn std::error::Error>> {
        let mapped = unsafe {
            self.device.map_memory(
                self.memory,
                offset,
                data.len() as u64,
                vk::MemoryMapFlags::empty(),
            )?
        };
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), mapped as *mut u8, data.len());
        }
        unsafe { self.device.unmap_memory(self.memory) };
        Ok(())
    }

    fn map(&mut self) -> Result<&mut [u8], Box<dyn std::error::Error>> {
        let mapped = unsafe {
            self.device
                .map_memory(self.memory, 0, self.size, vk::MemoryMapFlags::empty())?
        };
        Ok(unsafe { std::slice::from_raw_parts_mut(mapped as *mut u8, self.size as usize) })
    }

    fn unmap(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        unsafe { self.device.unmap_memory(self.memory) };
        Ok(())
    }

    fn native_handle(&self) -> u64 {
        self.buffer.as_raw()
    }
}
