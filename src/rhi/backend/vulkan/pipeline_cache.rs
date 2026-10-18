//! Vulkan Pipeline Cache
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::vulkan::instance::vk_fail;
use ash::vk;

/// Pipeline cache for Vulkan backend
/// 
/// Improves pipeline creation performance by caching compiled pipelines.
/// The cache is thread-safe and can be shared between multiple logical devices.
pub(crate) struct PipelineCache {
    device: ash::Device,
    cache: vk::PipelineCache,
}

impl PipelineCache {
    /// Create a new pipeline cache
    pub(crate) fn new(device: ash::Device) -> crate::error::RhiResult<Self> {
        // SAFETY: device is a valid logical device; create_info is default (no initial data, no flags).
        let cache = unsafe { device.create_pipeline_cache(&vk::PipelineCacheCreateInfo::default(), None) }
            .map_err(|e| vk_fail(e, "create pipeline cache"))?;
        
        Ok(Self { device, cache })
    }
    
    /// Get the raw pipeline cache handle
    pub(crate) fn handle(&self) -> vk::PipelineCache {
        self.cache
    }
    
    /// Merge external cache data into this cache
    pub(crate) fn merge(&self, data: &[u8]) -> crate::error::RhiResult<()> {
        if data.is_empty() {
            return Ok(());
        }
        let merge_info = vk::PipelineCacheCreateInfo {
            initial_data_size: data.len(),
            p_initial_data: data.as_ptr() as *const std::ffi::c_void,
            ..Default::default()
        };
        // SAFETY: device valid; merge_info.p_initial_data points to `data` slice, which is live for
        // the duration of the call and has the promised initial_data_size.
        let external_cache = unsafe { self.device.create_pipeline_cache(&merge_info, None) }
            .map_err(|e| vk_fail(e, "create external pipeline cache for merge"))?;
        
        // SAFETY: device valid; self.cache and external_cache are live pipeline caches owned by this device.
        unsafe { self.device.merge_pipeline_caches(self.cache, &[external_cache]) }
            .map_err(|e| vk_fail(e, "merge pipeline caches"))?;
        
        // SAFETY: external_cache was created above on this device and is no longer used after merge.
        unsafe { self.device.destroy_pipeline_cache(external_cache, None) };
        
        Ok(())
    }
    
    /// Get cache data for persistence
    pub(crate) fn data(&self) -> crate::error::RhiResult<Vec<u8>> {
        // SAFETY: device and self.cache are valid; returned vector is owned by us.
        let size = unsafe { self.device.get_pipeline_cache_data(self.cache) }
            .map_err(|e| vk_fail(e, "get pipeline cache data size"))?;
        
        if size.is_empty() {
            return Ok(Vec::new());
        }
        
        let mut data = vec![0u8; size.len()];
        // SAFETY: device and self.cache are valid; returned vector is owned by us.
        // TODO(verify): the two get_pipeline_cache_data calls can in principle return different
        // lengths (the cache is mutable between calls); copy_from_slice below panics if result_data
        // is longer than `data`. Confirm the size is stable here or size `data` from result_data.
        let result_data = unsafe { 
            self.device.get_pipeline_cache_data(self.cache)
        }
        .map_err(|e| vk_fail(e, "get pipeline cache data"))?;
        
        data.copy_from_slice(&result_data);
        Ok(data)
    }
}

impl Drop for PipelineCache {
    fn drop(&mut self) {
        // SAFETY: self.cache was created by self.device and is not used after drop.
        unsafe { self.device.destroy_pipeline_cache(self.cache, None) };
    }
}
