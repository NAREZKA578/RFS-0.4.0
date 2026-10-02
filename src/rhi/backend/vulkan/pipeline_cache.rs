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
        let external_cache = unsafe { self.device.create_pipeline_cache(&merge_info, None) }
            .map_err(|e| vk_fail(e, "create external pipeline cache for merge"))?;
        
        unsafe { self.device.merge_pipeline_caches(self.cache, &[external_cache]) }
            .map_err(|e| vk_fail(e, "merge pipeline caches"))?;
        
        unsafe { self.device.destroy_pipeline_cache(external_cache, None) };
        
        Ok(())
    }
    
    /// Get cache data for persistence
    pub(crate) fn data(&self) -> crate::error::RhiResult<Vec<u8>> {
        let size = unsafe { self.device.get_pipeline_cache_data(self.cache) }
            .map_err(|e| vk_fail(e, "get pipeline cache data size"))?;
        
        if size.is_empty() {
            return Ok(Vec::new());
        }
        
        let mut data = vec![0u8; size.len()];
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
        unsafe { self.device.destroy_pipeline_cache(self.cache, None) };
    }
}
