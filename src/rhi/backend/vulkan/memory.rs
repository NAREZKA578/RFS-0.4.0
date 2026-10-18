//! Vulkan memory allocation utilities.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::vulkan::instance::vk_fail;
use crate::core::MemoryProperties;
use crate::error::*;
use crate::types::MemoryPropertyFlags;
use ash::vk;

/// Minimal Vulkan memory allocator. Every allocation gets its own dedicated
/// `vk::DeviceMemory` object, memory is owned by the device and released when
/// the logical device is destroyed. Type selection matches the RHI memory
/// property flags against the physical device memory type table.
pub(crate) struct MemoryAllocator {
    device: ash::Device,
    types: Vec<MemoryPropertyFlags>,
}

impl MemoryAllocator {
    pub(crate) fn new(device: ash::Device, memory: &MemoryProperties) -> Self {
        Self {
            device,
            types: memory.memory_types.iter().map(|mt| mt.flags).collect(),
        }
    }

    /// Pick a memory type index that satisfies `required` properties and is
    /// allowed by `allowed_bits` (the `memoryTypeBits` from requirements).
    pub(crate) fn find_memory_type(
        &self,
        allowed_bits: u32,
        required: MemoryPropertyFlags,
    ) -> Option<u32> {
        select_memory_type(&self.types, allowed_bits, required)
    }

    /// Whether the memory type at `index` is host-coherent.
    ///
    /// Coherent memory needs neither [`flush`](Self::flush) nor
    /// [`invalidate`](Self::invalidate) around host access; non-coherent memory
    /// needs both, and skipping them is not a slow path but wrong data.
    pub(crate) fn is_coherent(&self, index: u32) -> bool {
        self.types
            .get(index as usize)
            .is_some_and(|flags| flags.contains(MemoryPropertyFlags::HOST_COHERENT))
    }

    /// Make host writes visible to the device.
    ///
    /// Required before the device reads a non-coherent mapped range. A no-op for
    /// coherent memory, so it is always safe to call.
    ///
    /// `size` is the number of bytes the host wrote, but it is not passed through
    /// as the range size. The spec requires a `VkMappedMemoryRange` size to be a
    /// multiple of `nonCoherentAtomSize` (64 bytes on most desktop parts), and a
    /// written size almost never is: the 112-byte uniform block here is a 64-byte
    /// atom boundary violation. Leaving it unrounded made the tail of the block
    /// invisible to the device, so the shader read a stale matrix.
    ///
    /// `VK_WHOLE_SIZE` is used instead of a rounded size because this allocator
    /// gives every allocation its own dedicated `vk::DeviceMemory`. There is no
    /// neighbouring live data to disturb, so the theoretical saving from a
    /// narrower range does not exist, while a rounded-up range would still need
    /// the limit plumbed in from the physical device.
    pub(crate) fn flush(&self, memory: vk::DeviceMemory, _size: u64) -> RhiResult<()> {
        let range = vk::MappedMemoryRange {
            memory,
            offset: 0,
            size: vk::WHOLE_SIZE,
            ..Default::default()
        };
        // SAFETY: device valid; `memory` is a valid allocation owned by this device and must be
        // currently mapped by the caller (Vulkan requires the range to be host-mapped).
        unsafe { self.device.flush_mapped_memory_ranges(&[range]) }
            .map_err(|e| vk_fail(e, "flush mapped memory"))
    }

    /// Make device writes visible to the host.
    ///
    /// The read-side counterpart of [`flush`](Self::flush): needed before reading
    /// a non-coherent mapped range the GPU has written. It carries the same
    /// `VK_WHOLE_SIZE` reasoning for the same reason.
    pub(crate) fn invalidate(&self, memory: vk::DeviceMemory, _size: u64) -> RhiResult<()> {
        let range = vk::MappedMemoryRange {
            memory,
            offset: 0,
            size: vk::WHOLE_SIZE,
            ..Default::default()
        };
        // SAFETY: device valid; `memory` is a valid allocation owned by this device and must be
        // currently mapped by the caller (Vulkan requires the range to be host-mapped).
        unsafe { self.device.invalidate_mapped_memory_ranges(&[range]) }
            .map_err(|e| vk_fail(e, "invalidate mapped memory"))
    }

    /// Allocate device memory of `size` bytes with the given props.
    pub(crate) fn allocate(
        &self,
        size: u64,
        allowed_bits: u32,
        required: MemoryPropertyFlags,
    ) -> RhiResult<(vk::DeviceMemory, u32)> {
        if size == 0 {
            return Err(RhiError::BackendError(
                "cannot allocate zero sized device memory".into(),
            ));
        }
        let index = self.find_memory_type(allowed_bits, required).ok_or_else(|| {
            RhiError::BackendError(
                "no memory type satisfies the requested properties on this device".into(),
            )
        })?;
        let allocate_info = vk::MemoryAllocateInfo {
            allocation_size: size,
            memory_type_index: index,
            ..Default::default()
        };
        // SAFETY: device valid; size > 0 (checked above) and index came from find_memory_type,
        // so it names a memory type allowed by the caller's memory_type_bits.
        let memory = unsafe { self.device.allocate_memory(&allocate_info, None) }
            .map_err(|e| vk_fail(e, "allocate device memory"))?;
        Ok((memory, index))
    }

    /// Map device memory and return the host pointer.
    ///
    /// The mapping must be released with [`unmap`](Self::unmap) before the
    /// memory is freed: Vulkan requires it, and freeing memory that is still
    /// mapped is a validation error rather than a harmless shortcut.
    pub(crate) fn map(
        &self,
        memory: vk::DeviceMemory,
        size: u64,
    ) -> RhiResult<*mut u8> {
        // SAFETY: device valid; `memory` is valid, offset 0 + size is within the allocation (caller
        // passes an allocation size it owns); memory must not already be mapped.
        let ptr = unsafe { self.device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
            .map_err(|e| vk_fail(e, "map device memory"))?;
        Ok(ptr as *mut u8)
    }

    /// Release a mapping taken by [`map`](Self::map).
    ///
    /// Infallible in practice: `vkUnmapMemory` has no return value, so there is
    /// nothing here to report and nothing to get wrong.
    pub(crate) fn unmap(&self, memory: vk::DeviceMemory) {
        // SAFETY: device valid; `memory` is a valid allocation owned by this device that was
        // previously mapped by `map` and has not yet been unmapped or freed.
        unsafe { self.device.unmap_memory(memory) }
    }
}

/// Choose a memory type from a property table, preferring a coherent one.
///
/// Nothing in Vulkan forces a host-visible allocation to be host-coherent, and a
/// non-coherent range only becomes visible to the device after a flush. Taking
/// the coherent type when one is on offer removes a whole class of
/// correct-on-integrated, wrong-on-discrete bugs, and costs nothing when it is
/// available: `find_memory_type` used to return the first match, which on some
/// devices was the non-coherent one.
///
/// Kept free of `ash::Device` so the choice can be tested without a GPU.
fn select_memory_type(
    types: &[MemoryPropertyFlags],
    allowed_bits: u32,
    required: MemoryPropertyFlags,
) -> Option<u32> {
    let required_bits = required.bits();
    let usable = |index: u32| {
        index < types.len() as u32
            && allowed_bits & (1 << index) != 0
            && (types[index as usize].bits() & required_bits) == required_bits
    };
    (0..types.len() as u32)
        .find(|&i| usable(i) && types[i as usize].contains(MemoryPropertyFlags::HOST_COHERENT))
        .or_else(|| (0..types.len() as u32).find(|&i| usable(i)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VISIBLE: MemoryPropertyFlags = MemoryPropertyFlags::HOST_VISIBLE;
    const COHERENT: MemoryPropertyFlags = MemoryPropertyFlags::HOST_COHERENT;
    const DEVICE_LOCAL: MemoryPropertyFlags = MemoryPropertyFlags::DEVICE_LOCAL;

    #[test]
    fn a_coherent_type_is_preferred_over_an_earlier_visible_one() {
        // Discrete-card ordering: index 0 is visible but not coherent.
        let types = [VISIBLE, VISIBLE | COHERENT];
        assert_eq!(select_memory_type(&types, 0b11, VISIBLE), Some(1));
    }

    #[test]
    fn a_non_coherent_type_is_still_chosen_when_it_is_all_there_is() {
        let types = [VISIBLE, DEVICE_LOCAL];
        assert_eq!(select_memory_type(&types, 0b11, VISIBLE), Some(0));
    }

    #[test]
    fn a_type_outside_the_allowed_mask_is_never_chosen() {
        // Only the coherent type is allowed, so it must win over the plain one.
        let types = [VISIBLE, VISIBLE | COHERENT];
        assert_eq!(select_memory_type(&types, 0b10, VISIBLE), Some(1));
    }

    #[test]
    fn a_type_missing_a_required_property_is_rejected() {
        let types = [VISIBLE, DEVICE_LOCAL];
        assert_eq!(
            select_memory_type(&types, 0b11, VISIBLE | DEVICE_LOCAL),
            None
        );
    }

    #[test]
    fn nothing_is_returned_for_an_empty_table() {
        assert_eq!(select_memory_type(&[], 0b11, VISIBLE), None);
    }

    #[test]
    fn a_required_coherent_allocation_is_not_silently_downgraded() {
        // The staging path used to hard-require coherence and fail outright.
        // It now falls back, so the check has to be that asking for coherence
        // explicitly still refuses the non-coherent type.
        let types = [VISIBLE, DEVICE_LOCAL];
        assert_eq!(select_memory_type(&types, 0b11, VISIBLE | COHERENT), None);
    }
}