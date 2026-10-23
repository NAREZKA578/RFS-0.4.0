//! Direct3D 12 Device Resource
//!
//! Implementation of `BackendResource` over a real `ID3D12Device`, its direct
//! command queue, one fence, and — since M2 (BUGS №706) — one direct command
//! allocator plus graphics command list that copies buffers and textures
//! sequentially. Buffers and textures are committed resources: host-visible
//! buffers live in the UPLOAD heap and stay persistently mapped, device-only
//! data copies through staging (UPLOAD heap for uploads, READBACK heap for
//! downloads) with explicit resource barriers.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::backend::common::BackendResource;
use crate::error::{RhiError, RhiResult};
use crate::resource::{Buffer, BufferDesc, GpuResource, Texture, TextureDesc};
use crate::types::{GraphicsApi, TextureDimensions, TextureUsage};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
    D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE, D3D12_HEAP_TYPE_DEFAULT,
    D3D12_HEAP_TYPE_READBACK, D3D12_HEAP_TYPE_UPLOAD, D3D12_MEMORY_POOL_UNKNOWN,
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_0,
    D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_DIMENSION_TEXTURE1D,
    D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_DIMENSION_TEXTURE3D,
    D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
    D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_FLAG_NONE,
    D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_GENERIC_READ,
    D3D12_RESOURCE_STATES, D3D12_RESOURCE_TRANSITION_BARRIER, D3D12_SUBRESOURCE_FOOTPRINT,
    D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    D3D12_TEXTURE_DATA_PITCH_ALIGNMENT, D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
    D3D12_TEXTURE_LAYOUT_UNKNOWN,
    ID3D12CommandAllocator,
    ID3D12CommandList, ID3D12CommandQueue, ID3D12Device, ID3D12Fence,
    ID3D12GraphicsCommandList, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC};

use super::convert;

/// A COM object the backend keeps between calls.
///
/// The `windows` crate deliberately does not mark its COM wrappers
/// `Send`/`Sync`: the runtime cannot know that every object is
/// thread-safe. The D3D12 objects this backend touches are otherwise
/// created single-threaded and carried by a `Send + Sync` device resource,
/// so the wrapper is the boundary where that obligation is taken over.
///
/// A private twin of the D3D11 `DxCom` on purpose: hoisting the type into
/// `backend/common.rs` was proposed and deferred until the backend's owner
/// (GLM2) agrees in the agent chat — the backend folder is the safer
/// default until then.
pub(crate) struct DxCom<T>(pub T);
unsafe impl<T> Send for DxCom<T> {}
unsafe impl<T> Sync for DxCom<T> {}
impl<T: Interface> Clone for DxCom<T> {
    fn clone(&self) -> Self {
        DxCom(self.0.clone())
    }
}

/// One tracked handle of the D3D12 handle table.
pub(crate) enum D3DObject {
    Buffer(BufferEntry),
    Texture(TextureEntry),
}

// COM wrappers and a raw persistent mapped pointer are carried across
// threads only through the handle table below; D3D12 objects are free
// threaded, and the mapped pointer is only dereferenced while the caller
// keeps the mapping alive.
unsafe impl Send for D3DObject {}
unsafe impl Sync for D3DObject {}

/// A committed buffer: which heap it lives in, how big, and (uploads only)
/// its persistent mapping.
pub(crate) struct BufferEntry {
    resource: DxCom<ID3D12Resource>,
    size: u64,
    /// `true` when the buffer lives in the UPLOAD heap and is persistently
    /// mapped for host writes.
    upload_heap: bool,
    /// The mapping returned by `Map` at creation for upload-heap buffers;
    /// `null` for device-only buffers.
    mapped: *mut core::ffi::c_void,
}

/// A committed texture: the resource and the RHI description it was built
/// from (mip sizes and subresource indexing are computed from the desc).
pub(crate) struct TextureEntry {
    resource: DxCom<ID3D12Resource>,
    desc: TextureDesc,
}

/// The D3D12 device resource handed to `Device::from_parts`.
pub struct D3D12Device {
    device: DxCom<ID3D12Device>,
    queue: DxCom<ID3D12CommandQueue>,
    fence: DxCom<ID3D12Fence>,
    /// One direct allocator the sequential copy path resets per call.
    allocator: DxCom<ID3D12CommandAllocator>,
    /// One graphics command list the sequential copy path resets per call.
    command_list: DxCom<ID3D12GraphicsCommandList>,
    physical_name: String,
    handles: Mutex<HashMap<u64, D3DObject>>,
    next_id: AtomicU64,
}

impl std::fmt::Debug for D3D12Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("D3D12Device")
            .field("physical", &self.physical_name)
            .finish()
    }
}

impl D3D12Device {
    /// Build the device resource around objects the instance created, and
    /// create the copy infrastructure: a direct command allocator and a
    /// graphics command list. The list comes out in the recording state
    /// (as documented for `CreateCommandList`), so it is closed here to
    /// make the first `Reset` legal.
    pub(crate) fn new(
        device: DxCom<ID3D12Device>,
        queue: DxCom<ID3D12CommandQueue>,
        fence: DxCom<ID3D12Fence>,
        physical: &crate::core::PhysicalDevice,
    ) -> RhiResult<Self> {
        unsafe {
            let allocator: ID3D12CommandAllocator = device
                .0
                .CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)
                .map_err(|e| {
                    RhiError::DeviceCreationError(format!(
                        "CreateCommandAllocator: {e}"
                    ))
                })?;
            let command_list: ID3D12GraphicsCommandList = device
                .0
                .CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &allocator, None)
                .map_err(|e| {
                    RhiError::DeviceCreationError(format!("CreateCommandList: {e}"))
                })?;
            command_list.Close().map_err(|e| {
                RhiError::DeviceCreationError(format!("initial command list Close: {e}"))
            })?; // so the first Reset is legal
            Ok(Self {
                device,
                queue,
                fence,
                allocator: DxCom(allocator),
                command_list: DxCom(command_list),
                physical_name: physical.name.clone(),
                handles: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
            })
        }
    }

    /// A handle that never hands out zero, the invalid sentinel.
    pub(crate) fn next_handle(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Standard heap properties for one of the three heap types.
    fn heap_properties(heap: D3D12_HEAP_TYPE) -> D3D12_HEAP_PROPERTIES {
        D3D12_HEAP_PROPERTIES {
            Type: heap,
            CPUPageProperty: D3D12_CPU_PAGE_PROPERTY_UNKNOWN,
            MemoryPoolPreference: D3D12_MEMORY_POOL_UNKNOWN,
            CreationNodeMask: 0,
            VisibleNodeMask: 0,
        }
    }

    /// The resource description D3D12 wants for a byte buffer: the layout is
    /// ROW MAJOR by definition (the documentation calls UNKNOWN invalid for
    /// `D3D12_RESOURCE_DIMENSION_BUFFER`).
    fn buffer_desc(bytes: u64) -> D3D12_RESOURCE_DESC {
        D3D12_RESOURCE_DESC {
            Dimension: D3D12_RESOURCE_DIMENSION_BUFFER,
            Alignment: 0,
            Width: bytes,
            Height: 1,
            DepthOrArraySize: 1,
            MipLevels: 1,
            Format: DXGI_FORMAT_UNKNOWN,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Layout: D3D12_TEXTURE_LAYOUT_ROW_MAJOR,
            Flags: D3D12_RESOURCE_FLAG_NONE,
        }
    }

    /// The resource description for a texture of the RHI description.
    ///
    /// D1 keeps `Height` and `depth`-like fields at their degenerate values;
    /// Cube is a 2D texture with six array layers; D3 packs its depth into
    /// `DepthOrArraySize`. Usage bits drive the self-sufficiency flags D3D12
    /// requires for render targets, depth-stencil views and unordered access.
    fn texture_desc(desc: &TextureDesc) -> RhiResult<D3D12_RESOURCE_DESC> {
        let format = convert::dxgi_format(desc.format).ok_or_else(|| {
            RhiError::TextureCreationError(format!("no D3D12 format for {:?}", desc.format))
        })?;
        let dimension = match desc.dimensions {
            TextureDimensions::D1 => D3D12_RESOURCE_DIMENSION_TEXTURE1D,
            TextureDimensions::D2 | TextureDimensions::Cube => {
                D3D12_RESOURCE_DIMENSION_TEXTURE2D
            }
            TextureDimensions::D3 => D3D12_RESOURCE_DIMENSION_TEXTURE3D,
        };
        let height = match desc.dimensions {
            TextureDimensions::D1 => 1,
            _ => desc.height,
        };
        let depth_or_array = match desc.dimensions {
            TextureDimensions::D1 | TextureDimensions::D2 | TextureDimensions::Cube => {
                desc.array_layers as u16
            }
            TextureDimensions::D3 => desc.depth as u16,
        };
        let mut flags = D3D12_RESOURCE_FLAG_NONE;
        if desc.usage.contains(TextureUsage::COLOR_ATTACHMENT) {
            flags |= D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET;
        }
        if desc.usage.contains(TextureUsage::DEPTH_STENCIL_ATTACHMENT) {
            flags |= D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL;
        }
        if desc.usage.contains(TextureUsage::STORAGE) {
            flags |= D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS;
        }
        Ok(D3D12_RESOURCE_DESC {
            Dimension: dimension,
            Alignment: 0,
            Width: desc.width as u64,
            Height: height,
            DepthOrArraySize: depth_or_array,
            MipLevels: desc.mip_levels as u16,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: desc.sample_count.as_count(),
                Quality: 0,
            },
            Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
            Flags: flags,
        })
    }

    /// A short-lived staging buffer in the UPLOAD (to copy from the CPU) or
    /// READBACK (to read back to the CPU) heap. Not tracked in the handle
    /// table: the object lives only inside one transfer.
    fn staging_buffer(&self, bytes: u64, for_upload: bool) -> RhiResult<ID3D12Resource> {
        let (heap, state) = if for_upload {
            (D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ)
        } else {
            (D3D12_HEAP_TYPE_READBACK, D3D12_RESOURCE_STATE_COPY_DEST)
        };
        let props = Self::heap_properties(heap);
        let mut out: Option<ID3D12Resource> = None;
        let desc = Self::buffer_desc(bytes);
        unsafe {
            self.device
                .0
                .CreateCommittedResource(
                    &props,
                    D3D12_HEAP_FLAG_NONE,
                    &desc,
                    state,
                    None,
                    &mut out,
                )
        }
        .map_err(|e| {
            RhiError::BackendError(format!(
                "D3D12 staging ({}) buffer of {} bytes: {e}",
                if for_upload { "upload" } else { "readback" },
                bytes
            ))
        })?;
        out.ok_or_else(|| {
            RhiError::BackendError("the D3D12 staging buffer was not created".into())
        })
    }

    /// The sequential copy path: reset the one allocator and command list,
    /// record the callers' commands, close, execute on the direct queue and
    /// wait on the fence. The plan (TODO.md, M2) wants strictly sequential
    /// transfers: each call owns the whole list from reset to completion.
    fn execute_copy(
        &self,
        record: impl FnOnce(&ID3D12GraphicsCommandList) -> RhiResult<()>,
    ) -> RhiResult<()> {
        unsafe {
            self.allocator.0.Reset().map_err(|e| {
                RhiError::BackendError(format!("command allocator Reset: {e}"))
            })?;
            self.command_list
                .0
                .Reset(&self.allocator.0, None)
                .map_err(|e| RhiError::BackendError(format!("command list Reset: {e}")))?;
            record(&self.command_list.0)?;
            self.command_list
                .0
                .Close()
                .map_err(|e| RhiError::BackendError(format!("command list Close: {e}")))?;
            let list: ID3D12CommandList = self.command_list.0.cast().map_err(|e| {
                RhiError::InternalError(format!("command list cast to raw list: {e}"))
            })?;
            self.queue.0.ExecuteCommandLists(&[Some(list)]);
            self.wait_idle()?;
        }
        Ok(())
    }

    /// A transition barrier from `before` to `after` on all subresources.
    fn transition(
        &self,
        list: &ID3D12GraphicsCommandList,
        resource: &ID3D12Resource,
        before: D3D12_RESOURCE_STATES,
        after: D3D12_RESOURCE_STATES,
    ) {
        let inner = D3D12_RESOURCE_TRANSITION_BARRIER {
            pResource: std::mem::ManuallyDrop::new(Some(resource.clone())),
            Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES,
            StateBefore: before,
            StateAfter: after,
        };
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                Transition: std::mem::ManuallyDrop::new(inner),
            },
        };
        unsafe { list.ResourceBarrier(&[barrier]) };
    }

    fn lookup(&self, handle: u64, what: &str) -> RhiResult<D3DObject> {
        self.handles
            .lock()
            .unwrap()
            .get(&handle)
            .map(|object| match object {
                D3DObject::Buffer(entry) => D3DObject::Buffer(BufferEntry {
                    resource: entry.resource.clone(),
                    size: entry.size,
                    upload_heap: entry.upload_heap,
                    mapped: entry.mapped,
                }),
                D3DObject::Texture(entry) => D3DObject::Texture(TextureEntry {
                    resource: entry.resource.clone(),
                    desc: entry.desc.clone(),
                }),
            })
            .ok_or_else(|| {
                RhiError::InternalError(format!("the {} handle {} is not tracked", what, handle))
            })
    }

    fn buffer_entry(&self, handle: u64) -> RhiResult<BufferEntry> {
        match self.lookup(handle, "buffer")? {
            D3DObject::Buffer(entry) => Ok(entry),
            D3DObject::Texture(_) => Err(RhiError::InternalError(
                "a buffer handle lost its kind".into(),
            )),
        }
    }

    fn texture_entry(&self, handle: u64) -> RhiResult<TextureEntry> {
        match self.lookup(handle, "texture")? {
            D3DObject::Texture(entry) => Ok(entry),
            D3DObject::Buffer(_) => Err(RhiError::InternalError(
                "a texture handle lost its kind".into(),
            )),
        }
    }

    /// Subresource index for a mip level and array layer. D3D12 lays
    /// subresources out mip-major: index = mip + layer * mip_levels, the
    /// same order the D3D11 backend uses.
    fn subresource_index(desc: &TextureDesc, mip_level: u32, array_layer: u32) -> u32 {
        mip_level + array_layer * desc.mip_levels
    }

    /// The row pitch D3D12 demands inside staging buffers: every row is a
    /// multiple of `D3D12_TEXTURE_DATA_PITCH_ALIGNMENT` (256).
    fn aligned_pitch(width_bytes: u32) -> u32 {
        (width_bytes + D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1)
            & !(D3D12_TEXTURE_DATA_PITCH_ALIGNMENT - 1)
    }

    /// Raw byte geometry of one mip inside a texture: the mip size, the
    /// tightly-packed row length, and the D3D12-aligned staging row length.
    fn texture_region_layout(
        desc: &TextureDesc,
        mip_level: u32,
    ) -> RhiResult<(u32, u32, u32, u32)> {
        let mip_w = (desc.width >> mip_level.min(31)).max(1);
        let mip_h = (desc.height >> mip_level.min(31)).max(1);
        let row_bytes = convert::row_pitch(desc.format, mip_w);
        Ok((mip_w, mip_h, row_bytes, Self::aligned_pitch(row_bytes)))
    }

    /// One signal that advances the fence, then a polling wait for it.
    ///
    /// Bug №190's Vulkan lesson, applied up front: a single reusable fence
    /// value per wait, advanced on every wait, not a zero-reset race. The
    /// polling wait is deliberate for M1/M2: it needs no extra window
    /// handle and no `Win32_System_Threading` feature in the manifest, and
    /// `wait_idle` in this backend is a rare, mostly test-facing path. The
    /// in-flight frame ring (Vulkan №282's shape) will replace this when
    /// recording and submission become independent.
    pub(crate) fn fence_signal_wait(&self) -> RhiResult<()> {
        unsafe {
            let future = self.fence.0.GetCompletedValue() + 1;
            self.queue
                .0
                .Signal(&self.fence.0, future)
                .map_err(|e| RhiError::BackendError(format!("fence signal failed: {e}")))?;
            while self.fence.0.GetCompletedValue() < future {
                std::thread::yield_now();
            }
        }
        Ok(())
    }

    /// The one signal that makes `wait_idle` work.
    pub(crate) fn wait_idle(&self) -> RhiResult<()> {
        self.fence_signal_wait()
    }
}

impl BackendResource for D3D12Device {
    fn as_raw(&self) -> *const () {
        (self.device.0.as_raw()) as *const ()
    }

    fn backend_type(&self) -> GraphicsApi {
        GraphicsApi::Direct3D12
    }

    fn wait_idle(&self) -> RhiResult<()> {
        self.wait_idle()
    }

    fn create_buffer(&self, desc: &BufferDesc) -> RhiResult<Buffer> {
        if !desc.is_valid() {
            return Err(RhiError::BufferCreationError(
                "the buffer description is invalid".into(),
            ));
        }
        let host_visible = desc.is_host_mappable();
        let (heap, initial_state) = if host_visible {
            // UPLOAD heap: CPU writes, GPU reads; the initial state is
            // GENERIC_READ as the documentation requires for uploads.
            (D3D12_HEAP_TYPE_UPLOAD, D3D12_RESOURCE_STATE_GENERIC_READ)
        } else {
            (D3D12_HEAP_TYPE_DEFAULT, D3D12_RESOURCE_STATE_COMMON)
        };
        let props = Self::heap_properties(heap);
        let d3d_desc = Self::buffer_desc(desc.size);
        let mut buffer: Option<ID3D12Resource> = None;
        unsafe {
            self.device
                .0
                .CreateCommittedResource(
                    &props,
                    D3D12_HEAP_FLAG_NONE,
                    &d3d_desc,
                    initial_state,
                    None,
                    &mut buffer,
                )
        }
        .map_err(|e| {
            RhiError::BufferCreationError(format!(
                "CreateCommittedResource of a {}-byte buffer: {e}",
                desc.size
            ))
        })?;
        let buffer = buffer.ok_or_else(|| {
            RhiError::BufferCreationError("CreateCommittedResource returned no resource".into())
        })?;
        let mut mapped = std::ptr::null_mut();
        if host_visible {
            // D3D12 upload-heap buffers stay mapped for their whole life
            // (the API allows it and the pointer stays valid), so host
            // writes go straight in.
            unsafe { buffer.Map(0, None, Some(&mut mapped)) }.map_err(|e| {
                RhiError::BufferCreationError(format!("upload buffer Map: {e}"))
            })?;
        }
        let handle = self.next_handle();
        self.handles.lock().unwrap().insert(
            handle,
            D3DObject::Buffer(BufferEntry {
                resource: DxCom(buffer),
                size: desc.size,
                upload_heap: host_visible,
                mapped,
            }),
        );
        let mut out = Buffer::from_desc(desc.clone());
        out.set_backend(GpuResource {
            handle,
            memory: 0,
            mapped: mapped as usize,
        });
        Ok(out)
    }

    fn upload_buffer(&self, buffer: &Buffer, data: &[u8]) -> RhiResult<()> {
        let gpu = buffer
            .backend()
            .ok_or_else(|| RhiError::BackendError("the buffer has no GPU backing".into()))?;
        if data.len() as u64 > buffer.size() {
            return Err(RhiError::BackendError(format!(
                "upload of {} bytes overflows a {}-byte buffer",
                data.len(),
                buffer.size()
            )));
        }
        if data.is_empty() {
            return Ok(());
        }
        let entry = self.buffer_entry(gpu.handle)?;
        if entry.upload_heap {
            // Persistently mapped from creation: the pointer stored in the
            // handle table is valid for the buffer's lifetime.
            if entry.mapped.is_null() {
                return Err(RhiError::InternalError(
                    "the upload-heap buffer lost its persistent mapping".into(),
                ));
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    entry.mapped as *mut u8,
                    data.len(),
                );
            }
            return Ok(());
        }
        // Device-only buffers copy through the UPLOAD heap.
        let staging = self.staging_buffer(data.len() as u64, true)?;
        unsafe {
            let mut ptr = std::ptr::null_mut();
            staging.Map(0, None, Some(&mut ptr)).map_err(|e| {
                RhiError::BackendError(format!("staging buffer Map: {e}"))
            })?;
            std::ptr::copy_nonoverlapping(
                data.as_ptr(),
                ptr as *mut u8,
                data.len(),
            );
            staging.Unmap(0, None);
        }
        let dst = entry.resource.clone();
        self.execute_copy(|list| {
            self.transition(
                list,
                &dst.0,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            unsafe {
                list.CopyBufferRegion(&staging, 0, &dst.0, 0, data.len() as u64)
            };
            self.transition(
                list,
                &dst.0,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_COMMON,
            );
            Ok(())
        })?;
        // The staging resource releases itself on drop.
        Ok(())
    }

    fn download_buffer(&self, buffer: &Buffer, offset: u64, size: u64) -> RhiResult<Vec<u8>> {
        if size == 0 {
            return Ok(Vec::new());
        }
        let gpu = buffer
            .backend()
            .ok_or_else(|| RhiError::BackendError("the buffer has no GPU backing".into()))?;
        let end = offset.saturating_add(size);
        if end > buffer.size() {
            return Err(RhiError::BackendError(format!(
                "the download of {}..{} overflows a {}-byte buffer",
                offset,
                end,
                buffer.size()
            )));
        }
        let entry = self.buffer_entry(gpu.handle)?;
        let staging = self.staging_buffer(size, false)?;
        let src = entry.resource.clone();
        self.execute_copy(|list| {
            self.transition(
                list,
                &src.0,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            unsafe {
                list.CopyBufferRegion(&src.0, offset, &staging, 0, size)
            };
            self.transition(
                list,
                &src.0,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_COMMON,
            );
            Ok(())
        })?;
        let mut out = vec![0u8; size as usize];
        unsafe {
            let mut ptr = std::ptr::null_mut();
            staging.Map(0, None, Some(&mut ptr)).map_err(|e| {
                RhiError::BackendError(format!("download staging Map: {e}"))
            })?;
            std::ptr::copy_nonoverlapping(
                ptr as *const u8,
                out.as_mut_ptr(),
                size as usize,
            );
            staging.Unmap(0, None);
        }
        Ok(out)
    }

    fn create_texture(&self, desc: &TextureDesc) -> RhiResult<Texture> {
        if !desc.is_valid() {
            return Err(RhiError::TextureCreationError(
                "the texture description is invalid".into(),
            ));
        }
        if desc.depth > 1 && desc.array_layers > 1 {
            return Err(RhiError::TextureCreationError(
                "D3D12 supports 3D textures or texture arrays, not both at once".into(),
            ));
        }
        let d3d_desc = Self::texture_desc(desc)?;
        let props = Self::heap_properties(D3D12_HEAP_TYPE_DEFAULT);
        let mut texture: Option<ID3D12Resource> = None;
        unsafe {
            self.device
                .0
                .CreateCommittedResource(
                    &props,
                    D3D12_HEAP_FLAG_NONE,
                    &d3d_desc,
                    D3D12_RESOURCE_STATE_COMMON,
                    None,
                    &mut texture,
                )
        }
        .map_err(|e| {
            RhiError::TextureCreationError(format!("CreateCommittedResource texture: {e}"))
        })?;
        let texture = texture.ok_or_else(|| {
            RhiError::TextureCreationError("CreateCommittedResource returned no texture".into())
        })?;
        let handle = self.next_handle();
        self.handles.lock().unwrap().insert(
            handle,
            D3DObject::Texture(TextureEntry {
                resource: DxCom(texture),
                desc: desc.clone(),
            }),
        );
        let mut out = Texture::from_desc(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn upload_texture(&self, texture: &Texture, data: &[u8]) -> RhiResult<()> {
        let gpu = texture
            .backend()
            .ok_or_else(|| RhiError::BackendError("the texture has no GPU backing".into()))?;
        let entry = self.texture_entry(gpu.handle)?;
        let desc = entry.desc.clone();
        if !matches!(
            desc.dimensions,
            TextureDimensions::D1 | TextureDimensions::D2
        ) {
            return Err(RhiError::NotSupported(
                "D3D12 texture uploads cover 1D and 2D textures; 3D and cube roofs need a \
                 different layout and are honest rejections here"
                    .into(),
            ));
        }
        if texture.is_multisampled() {
            return Err(RhiError::NotSupported(
                "multisampled textures have no direct upload path in D3D12; \
                 uploads go through a resolve"
                    .into(),
            ));
        }
        // The staging layout: one packed block of rows with D3D12's 256-byte
        // pitch alignment, offset 0 (which satisfies the 512-byte placement
        // alignment on its own).
        let (_, rows, tight_row, aligned_row) = Self::texture_region_layout(&desc, 0)?;
        let needed = rows as usize * tight_row as usize;
        if data.len() < needed {
            return Err(RhiError::BackendError(format!(
                "the {}-byte upload is too small: the texture needs {} bytes \
                 (format {:?}, {}x{}@row-pitch {})",
                data.len(),
                needed,
                desc.format,
                desc.width,
                desc.height,
                tight_row
            )));
        }
        let staging_bytes = rows as usize * aligned_row as usize;
        let staging = self.staging_buffer(staging_bytes as u64, true)?;
        unsafe {
            let mut ptr = std::ptr::null_mut();
            staging.Map(0, None, Some(&mut ptr))
                .map_err(|e| RhiError::BackendError(format!("upload staging Map: {e}")))?;
            let base = ptr as *mut u8;
            for row in 0..rows as usize {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr().add(row * tight_row as usize),
                    base.add(row * aligned_row as usize),
                    tight_row as usize,
                );
            }
            staging.Unmap(0, None);
        }
        let tex = entry.resource.clone();
        let dxgi = convert::dxgi_format(desc.format).ok_or_else(|| {
            RhiError::BackendError(format!("no D3D12 format for {:?}", desc.format))
        })?;
        let src_view = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(staging.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: 0,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: dxgi,
                        Width: desc.width,
                        Height: desc.height,
                        Depth: 1,
                        RowPitch: aligned_row,
                    },
                },
            },
        };
        let dst_view = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(tex.0.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: 0 },
        };
        self.execute_copy(|list| {
            self.transition(
                list,
                &tex.0,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            unsafe {
                list.CopyTextureRegion(&dst_view, 0, 0, 0, &src_view, None)
            };
            self.transition(
                list,
                &tex.0,
                D3D12_RESOURCE_STATE_COPY_DEST,
                D3D12_RESOURCE_STATE_COMMON,
            );
            Ok(())
        })?;
        Ok(())
    }

    fn download_texture(
        &self,
        texture: &Texture,
        mip_level: u32,
        array_layer: u32,
    ) -> RhiResult<Vec<u8>> {
        let gpu = texture
            .backend()
            .ok_or_else(|| RhiError::BackendError("the texture has no GPU backing".into()))?;
        let entry = self.texture_entry(gpu.handle)?;
        let desc = entry.desc.clone();
        if mip_level >= desc.mip_levels {
            return Err(RhiError::BackendError(format!(
                "mip level {} is outside a {}-level texture",
                mip_level, desc.mip_levels
            )));
        }
        if array_layer >= desc.array_layers {
            return Err(RhiError::BackendError(format!(
                "array layer {} is outside a {}-layer texture",
                array_layer, desc.array_layers
            )));
        }
        if !matches!(
            desc.dimensions,
            TextureDimensions::D1 | TextureDimensions::D2
        ) {
            return Err(RhiError::NotSupported(
                "D3D12 texture downloads cover 1D and 2D textures; 3D and cube roofs need a \
                 different layout and are honest rejections here"
                    .into(),
            ));
        }
        if texture.is_multisampled() {
            return Err(RhiError::NotSupported(
                "multisampled textures have no direct download path in D3D12; \
                 downloads go through a resolve"
                    .into(),
            ));
        }
        let (_, rows, tight_row, aligned_row) =
            Self::texture_region_layout(&desc, mip_level)?;
        let staging_bytes = rows as u64 * aligned_row as u64;
        let staging = self.staging_buffer(staging_bytes, false)?;
        let tex = entry.resource.clone();
        let dxgi = convert::dxgi_format(desc.format).ok_or_else(|| {
            RhiError::BackendError(format!("no D3D12 format for {:?}", desc.format))
        })?;
        let dst_view = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(staging.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                PlacedFootprint: D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: 0,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: dxgi,
                        Width: (desc.width >> mip_level.min(31)).max(1),
                        Height: rows,
                        Depth: 1,
                        RowPitch: aligned_row,
                    },
                },
            },
        };
        let subresource = Self::subresource_index(&desc, mip_level, array_layer);
        let src_view = D3D12_TEXTURE_COPY_LOCATION {
            pResource: std::mem::ManuallyDrop::new(Some(tex.0.clone())),
            Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
            Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: subresource },
        };
        self.execute_copy(|list| {
            self.transition(
                list,
                &tex.0,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            unsafe {
                list.CopyTextureRegion(&dst_view, 0, 0, 0, &src_view, None)
            };
            self.transition(
                list,
                &tex.0,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                D3D12_RESOURCE_STATE_COMMON,
            );
            Ok(())
        })?;
        let mut out = vec![0u8; rows as usize * tight_row as usize];
        unsafe {
            let mut ptr = std::ptr::null_mut();
            staging.Map(0, None, Some(&mut ptr))
                .map_err(|e| RhiError::BackendError(format!("readback staging Map: {e}")))?;
            let base = ptr as *const u8;
            for row in 0..rows as usize {
                std::ptr::copy_nonoverlapping(
                    base.add(row * aligned_row as usize),
                    out.as_mut_ptr().add(row * tight_row as usize),
                    tight_row as usize,
                );
            }
            staging.Unmap(0, None);
        }
        Ok(out)
    }

    fn tracked_object_count(&self) -> usize {
        self.handles.lock().unwrap().len()
    }

    fn tracked_object_summary(&self) -> String {
        let handles = self.handles.lock().unwrap();
        let mut buffers = 0usize;
        let mut textures = 0usize;
        for object in handles.values() {
            match object {
                D3DObject::Buffer(_) => buffers += 1,
                D3DObject::Texture(_) => textures += 1,
            }
        }
        format!("buffers: {}, textures: {}", buffers, textures)
    }
}
