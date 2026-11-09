//! Direct3D 12 Device Resource
//!
//! Implementation of `BackendResource` over a real `ID3D12Device`, its direct
//! command queue, one fence, and — since the M1 skeleton (Баг№705) — one direct
//! command allocator plus graphics command list that copies buffers and textures
//! sequentially. Buffers and textures are committed resources: host-visible
//! buffers live in the UPLOAD heap and stay persistently mapped, device-only
//! data copies through staging (UPLOAD heap for uploads, READBACK heap for
//! downloads) with explicit resource barriers.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::backend::common::{BackendResource, ReleaseKind};
use crate::command::commands::Command;
use crate::command::pass::render::{
    Framebuffer, FramebufferDesc, RenderPass, RenderPassBeginInfo, RenderPassDesc,
};
use crate::error::{RhiError, RhiResult};
use crate::resource::{Buffer, BufferDesc, GpuResource, Texture, TextureDesc, TextureView};
use crate::types::{ClearValue, Extent2D, Format, GraphicsApi, IndexType, Offset2D, Rect2D,
    TextureDimensions, TextureUsage};

use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::{D3D10_NAME_UNDEFINED, D3D_PRIMITIVE_TOPOLOGY, ID3DBlob};
use windows::Win32::Graphics::Direct3D::Fxc::D3DReflect;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_SHADER_DESC, D3D11_SIGNATURE_PARAMETER_DESC, ID3D11ShaderReflection,
};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_BLEND_DESC, D3D12_BLEND_ONE, D3D12_BLEND_OP_ADD, D3D12_BLEND_ZERO,
    D3D12_COMMAND_LIST_TYPE_DIRECT, D3D_ROOT_SIGNATURE_VERSION_1_0,
    D3D12_COMPARISON_FUNC_ALWAYS, D3D12_COLOR_WRITE_ENABLE_ALL,
    D3D12_CONSTANT_BUFFER_VIEW_DESC, D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
    D3D12_CPU_PAGE_PROPERTY_UNKNOWN, D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT,
    D3D12_CULL_MODE_NONE,
    D3D12_DEPTH_STENCIL_DESC, D3D12_DEPTH_STENCILOP_DESC,
    D3D12_DEPTH_WRITE_MASK_ALL, D3D12_DEPTH_WRITE_MASK_ZERO,
    D3D12_DESCRIPTOR_RANGE, D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
    D3D12_DESCRIPTOR_RANGE_TYPE, D3D12_DESCRIPTOR_RANGE_TYPE_CBV,
    D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER, D3D12_DESCRIPTOR_RANGE_TYPE_SRV,
    D3D12_FILL_MODE_SOLID, D3D12_GRAPHICS_PIPELINE_STATE_DESC,
    D3D12_HEAP_FLAG_NONE, D3D12_HEAP_PROPERTIES, D3D12_HEAP_TYPE, D3D12_HEAP_TYPE_DEFAULT,
    D3D12_HEAP_TYPE_READBACK, D3D12_HEAP_TYPE_UPLOAD,
    D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
    D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA,
    D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA, D3D12_INPUT_ELEMENT_DESC,
    D3D12_INPUT_LAYOUT_DESC, D3D12_LOGIC_OP_NOOP,
    D3D12_MEMORY_POOL_UNKNOWN, D3D12_PIPELINE_STATE_FLAG_NONE,
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_RASTERIZER_DESC,
    D3D12_RENDER_TARGET_BLEND_DESC, D3D12_RENDER_TARGET_VIEW_DESC,
    D3D12_RENDER_TARGET_VIEW_DESC_0, D3D12_RESOURCE_BARRIER, D3D12_RESOURCE_BARRIER_0,
    D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, D3D12_RESOURCE_DESC,
    D3D12_RESOURCE_DIMENSION_BUFFER, D3D12_RESOURCE_DIMENSION_TEXTURE1D,
    D3D12_RESOURCE_DIMENSION_TEXTURE2D, D3D12_RESOURCE_DIMENSION_TEXTURE3D,
    D3D12_RESOURCE_FLAG_ALLOW_DEPTH_STENCIL, D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET,
    D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS, D3D12_RESOURCE_FLAG_NONE,
    D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_GENERIC_READ,
    D3D12_RESOURCE_STATES, D3D12_RESOURCE_TRANSITION_BARRIER,
    D3D12_ROOT_PARAMETER, D3D12_ROOT_PARAMETER_0,
    D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE, D3D12_ROOT_DESCRIPTOR_TABLE,
    D3D12_ROOT_SIGNATURE_DESC, D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
    D3D12_SAMPLER_DESC,
    D3D12_SHADER_BYTECODE, D3D12_SHADER_RESOURCE_VIEW_DESC,
    D3D12_SHADER_RESOURCE_VIEW_DESC_0, D3D12_SHADER_VISIBILITY_ALL,
    D3D12_SUBRESOURCE_FOOTPRINT, D3D12_TEX2D_RTV, D3D12_TEX2D_ARRAY_DSV,
    D3D12_TEX2DMS_ARRAY_SRV, D3D12_TEX2DMS_DSV, D3D12_TEX2DMS_RTV, D3D12_TEX2DMS_SRV,
    D3D12_TEX3D_RTV, D3D12_TEX1D_RTV, D3D12_TEX1D_ARRAY_DSV, D3D12_TEX2D_ARRAY_RTV,
    D3D12_TEX2D_SRV, D3D12_TEX2D_DSV, D3D12_TEXTURE_COPY_LOCATION,
    D3D12_TEXTURE_COPY_LOCATION_0, D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
    D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, D3D12_TEXTURE_DATA_PITCH_ALIGNMENT,
    D3D12_TEXTURE_LAYOUT_ROW_MAJOR, D3D12_TEXTURE_LAYOUT_UNKNOWN,
    D3D12SerializeRootSignature,
    D3D12_DEPTH_STENCIL_VIEW_DESC, D3D12_DEPTH_STENCIL_VIEW_DESC_0,
    D3D12_DSV_DIMENSION_TEXTURE2D,
    D3D12_DSV_FLAG_NONE, D3D12_TEX1D_SRV, D3D12_TEX1D_ARRAY_SRV,
    D3D12_TEX2D_ARRAY_SRV, D3D12_TEX3D_SRV, D3D12_TEXCUBE_SRV, D3D12_TEXCUBE_ARRAY_SRV,
    D3D12_RTV_DIMENSION_TEXTURE1D, D3D12_RTV_DIMENSION_TEXTURE1DARRAY,
    D3D12_RTV_DIMENSION_TEXTURE2D, D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
    D3D12_RTV_DIMENSION_TEXTURE2DMS, D3D12_RTV_DIMENSION_TEXTURE3D,
    D3D12_DSV_DIMENSION_TEXTURE1D, D3D12_DSV_DIMENSION_TEXTURE1DARRAY,
    D3D12_DSV_DIMENSION_TEXTURE2DMS, D3D12_DSV_DIMENSION_TEXTURE2DARRAY,
    D3D12_TEX1D_ARRAY_RTV, D3D12_TEX1D_DSV, D3D12_STREAM_OUTPUT_DESC,
    D3D12_CACHED_PIPELINE_STATE,
    D3D12_SRV_DIMENSION_TEXTURE1D,
    D3D12_SRV_DIMENSION_TEXTURE1DARRAY, D3D12_SRV_DIMENSION_TEXTURE2D,
    D3D12_SRV_DIMENSION_TEXTURE2DARRAY, D3D12_SRV_DIMENSION_TEXTURE2DMS,
    D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY, D3D12_SRV_DIMENSION_TEXTURE3D,
    D3D12_SRV_DIMENSION_TEXTURECUBE, D3D12_SRV_DIMENSION_TEXTURECUBEARRAY,
    D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
    ID3D12CommandAllocator, ID3D12CommandList, ID3D12CommandQueue,
    ID3D12Device, ID3D12Fence, ID3D12GraphicsCommandList, ID3D12PipelineState,
    ID3D12Resource, ID3D12RootSignature,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};

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
    /// A texture view: the resource it looks into, the RHI format every
    /// descriptor is built from, and the subresource window. No descriptor
    /// is created here — descriptors are written where they land (a
    /// descriptor-set block, a framebuffer attachment).
    View(ViewEntry),
    /// A sampler description. The device-side sampler descriptor is written
    /// where the sampler binding lands, so one sampler description can feed
    /// many sets (the same CreateSampler data, repeated cheaply).
    Sampler(SamplerEntry),
    /// A shader module: the DXBC container bytes, kept for the pipeline
    /// creator that turns per-stage modules into one PSO.
    Shader(ShaderEntry),
    GraphicsPipeline(GraphicsPipelineEntry),
    /// A render pass object is only its description: D3D12 has no render
    /// pass concept, so the emulation keeps the description (the same shape
    /// the D3D11 backend tracks).
    Pass(RenderPassDesc),
    /// A framebuffer: one RTV descriptor per colour attachment and the one
    /// depth-stencil descriptor, allocated once from the CPU-only pools so
    /// the pass binds pointers instead of recreating views per frame.
    Framebuffer(FramebufferEntry),
    /// A descriptor set layout: the bindings and their per-pool descriptor
    /// totals, the plan everything binding-related reads.
    Layout(LayoutEntry),
    /// A descriptor set: the contiguous block reserved per pool, filled by
    /// `write_descriptors` and bound by its GPU handle.
    Set(SetEntry),
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
    /// The D3D12 state the buffer was created in — the state it lives in
    /// between submits. COMMON for the DEFAULT heap, GENERIC_READ for
    /// UPLOAD (the documented `CreateCommittedResource` requirement). The
    /// recorded-list translator seeds its per-list tracker from this, and
    /// the D3D12 heap contract keeps the upload home permanent (an
    /// upload-heap resource never leaves GENERIC_READ, so it never takes a
    /// transition barrier at all).
    home_state: D3D12_RESOURCE_STATES,
}

/// A committed texture: the resource and the RHI description it was built
/// from (mip sizes and subresource indexing are computed from the desc).
pub(crate) struct TextureEntry {
    resource: DxCom<ID3D12Resource>,
    desc: TextureDesc,
    /// The creation state (COMMON today — every texture is born in the
    /// DEFAULT heap); the translator's tracker seeds from it.
    home_state: D3D12_RESOURCE_STATES,
}

impl BufferEntry {
    /// The underlying resource, for call sites outside this module — the
    /// recorded-list translator tracks barrier state against it.
    pub(crate) fn resource(&self) -> &ID3D12Resource {
        &self.resource.0
    }

    /// The buffer's byte size, for the translator's range checks.
    pub(crate) fn size(&self) -> u64 {
        self.size
    }

    /// The state the buffer was created in (see the field doc).
    pub(crate) fn home_state(&self) -> D3D12_RESOURCE_STATES {
        self.home_state
    }
}

impl TextureEntry {
    /// The underlying resource, for the recorded-list translator.
    pub(crate) fn resource(&self) -> &ID3D12Resource {
        &self.resource.0
    }

    /// The descriptor the texture was created from.
    pub(crate) fn desc(&self) -> &TextureDesc {
        &self.desc
    }

    /// The state the texture was created in (see the field doc).
    pub(crate) fn home_state(&self) -> D3D12_RESOURCE_STATES {
        self.home_state
    }
}

/// A texture view: which resource it looks into and the subresource window.
/// Descriptors are created where they land, so the entry carries only the
/// facts every descriptor type (SRV, RTV, DSV) needs — the resource, the
/// view shape, the parent's tracking handle, and the mip/layer window.
pub(crate) struct ViewEntry {
    resource: DxCom<ID3D12Resource>,
    /// The parent texture's tracked handle: the recorded-list translator
    /// tracks barrier state against the *texture*, not the view.
    texture_handle: u64,
    /// The RHI format the view reads through: the resource's own format, or
    /// the depth reformat the caller asked for (`convert::depth_srv_format`).
    format: Format,
    /// The RHI view shape the caller asked for (cube views read six faces).
    view_type: crate::resource::TextureViewType,
    /// The parent's own dimensions and multisample count: descriptor dims
    /// come from the resource tree with the view's window on top.
    dimensions: TextureDimensions,
    multisampled: bool,
    /// The mip window: `base_mip..base_mip + mip_levels`.
    base_mip: u32,
    mip_levels: u32,
    /// The array-layer window: `base_layer..base_layer + layer_count`.
    base_layer: u32,
    layer_count: u32,
}

impl Clone for ViewEntry {
    fn clone(&self) -> Self {
        ViewEntry {
            resource: self.resource.clone(),
            texture_handle: self.texture_handle,
            format: self.format,
            view_type: self.view_type,
            dimensions: self.dimensions,
            multisampled: self.multisampled,
            base_mip: self.base_mip,
            mip_levels: self.mip_levels,
            base_layer: self.base_layer,
            layer_count: self.layer_count,
        }
    }
}

impl ViewEntry {
    pub(crate) fn resource(&self) -> &ID3D12Resource {
        &self.resource.0
    }

    /// The RHI format descriptors are built from.
    pub(crate) fn rhi_format(&self) -> Format {
        self.format
    }

    /// The parent texture's tracked handle for the barrier tracker.
    pub(crate) fn texture_handle(&self) -> u64 {
        self.texture_handle
    }

    /// The RHI view shape the caller asked for.
    pub(crate) fn view_type(&self) -> crate::resource::TextureViewType {
        self.view_type
    }

    /// The parent resource's dimensions.
    pub(crate) fn dimensions(&self) -> TextureDimensions {
        self.dimensions
    }

    /// `true` when the parent is multisampled (its views read MS planes).
    pub(crate) fn multisampled(&self) -> bool {
        self.multisampled
    }

    /// The SRV format: the depth family reads through the typed depth-first
    /// format the conversion layer pairs with the typeless resource.
    pub(crate) fn srv_format(&self) -> Option<DXGI_FORMAT> {
        match convert::typeless_base(self.format) {
            Some(_) => convert::depth_srv_format(self.format),
            None => convert::dxgi_format(self.format),
        }
    }

    /// The RTV/DSV typed format (depth views keep the typed depth).
    pub(crate) fn target_format(&self) -> Option<DXGI_FORMAT> {
        convert::dxgi_format(self.format)
    }

    pub(crate) fn base_mip(&self) -> u32 {
        self.base_mip
    }

    pub(crate) fn mip_levels(&self) -> u32 {
        self.mip_levels
    }

    pub(crate) fn base_layer(&self) -> u32 {
        self.base_layer
    }

    pub(crate) fn layer_count(&self) -> u32 {
        self.layer_count
    }
}

/// A sampler description; the descriptor is written where it binds.
#[derive(Clone)]
pub(crate) struct SamplerEntry {
    desc: crate::resource::SamplerDesc,
}

impl SamplerEntry {
    pub(crate) fn desc(&self) -> &crate::resource::SamplerDesc {
        &self.desc
    }
}

/// A shader module: the DXBC container and its declared format.
#[derive(Clone)]
pub(crate) struct ShaderEntry {
    format: crate::shader::ShaderFormat,
    code: Vec<u8>,
}

impl ShaderEntry {
    pub(crate) fn format(&self) -> crate::shader::ShaderFormat {
        self.format
    }

    pub(crate) fn code(&self) -> &[u8] {
        &self.code
    }
}

/// The root signature, PSO, and binding facts the recorded draw path reads.
#[derive(Clone)]
pub(crate) struct GraphicsPipelineEntry {
    root_signature: DxCom<ID3D12RootSignature>,
    pso: DxCom<ID3D12PipelineState>,
    topology: D3D_PRIMITIVE_TOPOLOGY,
    /// `stride per input slot`, filling `BindVertexBuffers` views.
    vertex_strides: Vec<(u32, u32)>,
    /// How many root-table parameters the pipeline declares — one per set.
    num_sets: usize,
    viewports: Vec<crate::types::Viewport>,
    scissors: Vec<crate::types::Scissor>,
    blend_factor: [f32; 4],
}

impl GraphicsPipelineEntry {
    pub(crate) fn root_signature(&self) -> &ID3D12RootSignature {
        &self.root_signature.0
    }

    pub(crate) fn pso(&self) -> &ID3D12PipelineState {
        &self.pso.0
    }

    pub(crate) fn topology(&self) -> D3D_PRIMITIVE_TOPOLOGY {
        self.topology
    }

    pub(crate) fn vertex_strides(&self) -> &[(u32, u32)] {
        &self.vertex_strides
    }

    pub(crate) fn num_sets(&self) -> usize {
        self.num_sets
    }

    pub(crate) fn viewports(&self) -> &[crate::types::Viewport] {
        &self.viewports
    }

    pub(crate) fn scissors(&self) -> &[crate::types::Scissor] {
        &self.scissors
    }

    pub(crate) fn blend_factor(&self) -> [f32; 4] {
        self.blend_factor
    }
}

/// A descriptor set layout: the bindings and each pool's descriptor total.
#[derive(Clone)]
pub(crate) struct LayoutEntry {
    pub(crate) bindings: Vec<crate::descriptor::DescriptorBinding>,
    /// Total descriptors drawing from the CBV/SRV/UAV pool.
    pub(crate) cbv_srv_uav_count: u32,
    /// Total descriptors drawing from the sampler pool.
    pub(crate) sampler_count: u32,
    /// The root-table kind classification: `None` means a sampler-only set
    /// (or an empty one — no tables at all).
    pub(crate) cbv_srv_uav: bool,
    pub(crate) sampler: bool,
}

impl LayoutEntry {
    /// The declared-count roll of descriptors the binding consumes, for the
    /// set block foreign binding offset math.
    pub(crate) fn descriptor_offsets(&self) -> Vec<(u32, u32)> {
        let mut offsets = Vec::with_capacity(self.bindings.len());
        let mut running: u32 = 0;
        for binding in &self.bindings {
            offsets.push((binding.binding, running));
            running += binding.count;
        }
        offsets
    }

    /// The offset of one declared binding, `None` when the write addresses a
    /// binding the layout has no entry for.
    pub(crate) fn binding_offset(&self, binding: u32) -> Option<(D3D12_DESCRIPTOR_RANGE_TYPE, u32)> {
        let mut running: u32 = 0;
        for declared in &self.bindings {
            if declared.binding == binding {
                return Some((convert::descriptor_range_type(declared.ty)?, running));
            }
            running += declared.count;
        }
        None
    }

    /// The size of one declared binding.
    pub(crate) fn binding_count(&self, binding: u32) -> Option<u32> {
        self.bindings
            .iter()
            .find(|declared| declared.binding == binding)
            .map(|declared| declared.count)
    }
}

/// A descriptor set: block bases reserved once at creation.
#[derive(Clone)]
pub(crate) struct SetEntry {
    layout: LayoutEntry,
    /// Base index in the CBV/SRV/UAV pool, when the set uses it.
    block_base: Option<usize>,
    /// Base index in the sampler pool, when the set uses it.
    sampler_block_base: Option<usize>,
}

impl SetEntry {
    pub(crate) fn layout(&self) -> &LayoutEntry {
        &self.layout
    }

    pub(crate) fn block_base(&self) -> Option<usize> {
        self.block_base
    }

    pub(crate) fn sampler_block_base(&self) -> Option<usize> {
        self.sampler_block_base
    }
}

/// A framebuffer: descriptor slots allocated once, bound by pointer.
#[derive(Clone)]
pub(crate) struct FramebufferEntry {
    /// RTV pool slots, one per colour attachment in declared order.
    pub(crate) rtv_slots: Vec<usize>,
    /// The parent texture's tracked handle per RTV, declared order — the
    /// recorded pass moves those textures through their bar states.
    pub(crate) rtv_handles: Vec<u64>,
    /// DSV pool slot when a depth attachment exists.
    pub(crate) dsv_slot: Option<usize>,
    /// The depth texture's tracked handle, when it exists.
    pub(crate) dsv_handle: Option<u64>,
    /// Window of the framebuffer the render area pins.
    pub(crate) width: u32,
    pub(crate) height: u32,
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
    /// The four descriptor heaps the graphics and compute paths draw from
    /// (M3, Баг№707): CBV/SRV/UAV and sampler shader-visible, RTV/DSV
    /// CPU-only. Built once here, carved from by the view, set, and
    /// descriptor-table creators through `heaps()`.
    heaps: super::descriptor::DescriptorHeaps,
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
            let heaps = super::descriptor::DescriptorHeaps::new(&device.0)?;
            Ok(Self {
                device,
                queue,
                fence,
                allocator: DxCom(allocator),
                command_list: DxCom(command_list),
                heaps,
                physical_name: physical.name.clone(),
                handles: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
            })
        }
    }

    /// The descriptor heaps shared by every view, set, and pipeline creator
    /// (M3, Баг№707).
    pub(crate) fn heaps(&self) -> &super::descriptor::DescriptorHeaps {
        &self.heaps
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
    ///
    /// Depth formats are created per the `convert.rs` contract (bug №737):
    /// the resource itself takes the *typeless base*, and the DSV/SRV views
    /// carry the typed format — a typed depth resource would refuse the
    /// reinterpreted views the format pairing was built for. Every non-depth
    /// format keeps its typed format, since only the depth family has a
    /// typeless base.
    fn texture_desc(desc: &TextureDesc) -> RhiResult<D3D12_RESOURCE_DESC> {
        let format = match convert::typeless_base(desc.format) {
            Some(typeless) => typeless,
            None => convert::dxgi_format(desc.format).ok_or_else(|| {
                RhiError::TextureCreationError(format!(
                    "no D3D12 format for {:?}",
                    desc.format
                ))
            })?,
        };
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

    /// The sequential submit path: reset the one allocator and command list,
    /// record the caller's list, close, execute on the direct queue and wait
    /// on the fence. Strictly sequential by the M1/M2 plan (TODO.md —
    /// Баг№706): every call owns the whole list from reset to completion,
    /// and `submit_commands` (M2) rides the same path — a recorded command
    /// list and a copy are the same shape here until a frame ring (Vulkan
    /// №282's shape) replaces the waits.
    fn submit_sequential(
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
            let outcome = record(&self.command_list.0);
            // BUGS №760: an aborted record leaves the list in the recording
            // state. The next call's `allocator Reset` then fails with
            // 0x80004005, and every later refusal comes back as that garbage
            // allocator error instead of the honest one — even passes that
            // happened before the first failure walked through it. Close the
            // discarded list and clear the allocator's pending state; the
            // caller's own error goes back untouched.
            if let Err(record_error) = outcome {
                Self::discard_aborted_list(&self.command_list.0, &self.allocator.0);
                return Err(record_error);
            }
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

    /// BUGS №760's cleanup: close a command list whose `record` wrote only
    /// part of itself, and reset the allocator it sits on. Without this the
    /// list stays in the recording state and the next submit's allocator
    /// reset fails with 0x80004005 — masking every later refusal with that
    /// garbage error. Both calls are best-effort: the honest outcome here is
    /// the caller's own error, not a newer one about the scavenger.
    fn discard_aborted_list(
        command_list: &ID3D12GraphicsCommandList,
        allocator: &ID3D12CommandAllocator,
    ) {
        unsafe {
            let _ = command_list.Close();
            let _ = allocator.Reset();
        }
    }

    /// BUGS №761's rule: D3D12 requires a constant buffer view's
    /// `SizeInBytes` to be a multiple of 256 — a non-multiple faults the
    /// device. The declared byte range rounds up to the hardware's
    /// alignment; the shader reads the same first bytes and the trailing
    /// range is padding.
    fn cbv_view_size(bytes: u64) -> u32 {
        let alignment = D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT as u64;
        (bytes.div_ceil(alignment) * alignment) as u32
    }

    /// A transition barrier from `before` to `after` on all subresources.
    pub(crate) fn transition(
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
                    home_state: entry.home_state,
                }),
                D3DObject::Texture(entry) => D3DObject::Texture(TextureEntry {
                    resource: entry.resource.clone(),
                    desc: entry.desc.clone(),
                    home_state: entry.home_state,
                }),
                D3DObject::View(entry) => D3DObject::View(entry.clone()),
                D3DObject::Sampler(entry) => D3DObject::Sampler(entry.clone()),
                D3DObject::Shader(entry) => D3DObject::Shader(entry.clone()),
                D3DObject::GraphicsPipeline(entry) => D3DObject::GraphicsPipeline(entry.clone()),
                D3DObject::Pass(desc) => D3DObject::Pass(desc.clone()),
                D3DObject::Framebuffer(entry) => D3DObject::Framebuffer(entry.clone()),
                D3DObject::Layout(entry) => D3DObject::Layout(entry.clone()),
                D3DObject::Set(entry) => D3DObject::Set(entry.clone()),
            })
            .ok_or_else(|| {
                RhiError::InternalError(format!("the {} handle {} is not tracked", what, handle))
            })
    }

    pub(crate) fn buffer_entry(&self, handle: u64) -> RhiResult<BufferEntry> {
        match self.lookup(handle, "buffer")? {
            D3DObject::Buffer(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a buffer handle lost its kind".into(),
            )),
        }
    }

    pub(crate) fn texture_entry(&self, handle: u64) -> RhiResult<TextureEntry> {
        match self.lookup(handle, "texture")? {
            D3DObject::Texture(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a texture handle lost its kind".into(),
            )),
        }
    }

    /// Subresource index for a mip level and array layer. D3D12 lays
    /// subresources out mip-major: index = mip + layer * mip_levels, the
    /// same order the D3D11 backend uses.
    pub(crate) fn subresource_index(desc: &TextureDesc, mip_level: u32, array_layer: u32) -> u32 {
        mip_level + array_layer * desc.mip_levels
    }

    /// The row pitch D3D12 demands inside staging buffers: every row is a
    /// multiple of `D3D12_TEXTURE_DATA_PITCH_ALIGNMENT` (256).
    pub(crate) fn aligned_pitch(width_bytes: u32) -> u32 {
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

    // ————————————————————————————————————————————————————————————————————
    // M3 private helpers (Баг№707): tracked-entry lookups, descriptor
    // classification, the root signature builder, and the descriptor
    // constructors the creators above call into. Not trait members — they
    // are implementation details of this device, and the trait impl stays
    // the trait's shape only.
    // ————————————————————————————————————————————————————————————————————

    pub(crate) fn view_entry(&self, handle: u64) -> RhiResult<ViewEntry> {
        match self.lookup(handle, "texture view")? {
            D3DObject::View(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a texture view handle lost its kind".into(),
            )),
        }
    }

    pub(crate) fn sampler_entry(&self, handle: u64) -> RhiResult<SamplerEntry> {
        match self.lookup(handle, "sampler")? {
            D3DObject::Sampler(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a sampler handle lost its kind".into(),
            )),
        }
    }

    pub(crate) fn shader_entry(&self, handle: u64) -> RhiResult<ShaderEntry> {
        match self.lookup(handle, "shader module")? {
            D3DObject::Shader(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a shader module handle lost its kind".into(),
            )),
        }
    }

    pub(crate) fn layout_entry(&self, handle: u64) -> RhiResult<LayoutEntry> {
        match self.lookup(handle, "descriptor set layout")? {
            D3DObject::Layout(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a descriptor set layout handle lost its kind".into(),
            )),
        }
    }

    pub(crate) fn set_entry(&self, handle: u64) -> RhiResult<SetEntry> {
        match self.lookup(handle, "descriptor set")? {
            D3DObject::Set(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a descriptor set handle lost its kind".into(),
            )),
        }
    }

    pub(crate) fn framebuffer_entry(&self, handle: u64) -> RhiResult<FramebufferEntry> {
        match self.lookup(handle, "framebuffer")? {
            D3DObject::Framebuffer(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a framebuffer handle lost its kind".into(),
            )),
        }
    }

    /// The render pass description its tracked handle carries: a D3D12
    /// render pass is only its description, and the recorded
    /// `BeginRenderPass` reads it back.
    pub(crate) fn pass_description(&self, handle: u64) -> RhiResult<RenderPassDesc> {
        match self.lookup(handle, "render pass")? {
            D3DObject::Pass(desc) => Ok(desc),
            _ => Err(RhiError::InternalError(
                "a render pass handle lost its kind".into(),
            )),
        }
    }

    /// The raw device, for the recorder's temporary-view creations: a
    /// standalone clear binds its view through a CPU pool slot the recorder
    /// owns, and the descriptor creator it needs is the device's.
    pub(crate) fn raw_device(&self) -> &ID3D12Device {
        &self.device.0
    }

    pub(crate) fn pipeline_entry(&self, handle: u64) -> RhiResult<GraphicsPipelineEntry> {
        match self.lookup(handle, "graphics pipeline")? {
            D3DObject::GraphicsPipeline(entry) => Ok(entry),
            _ => Err(RhiError::InternalError(
                "a graphics pipeline handle lost its kind".into(),
            )),
        }
    }

    /// One descriptor-set binding, classified into the pools D3D12 splits
    /// over: sampler bindings live in the sampler heap, everything else in
    /// the CBV/SRV/UAV heap. Types this backend cannot reflect, and mixed
    /// kinds in one set, come back as spoken refusals — one root descriptor
    /// table draws from exactly one heap kind, and this binding model keeps
    /// one table per set.
    pub(crate) fn classify_descriptors(
        bindings: &[crate::descriptor::DescriptorBinding],
    ) -> RhiResult<(u32, u32, bool, bool)> {
        let mut cbv_srv_uav_count: u32 = 0;
        let mut sampler_count: u32 = 0;
        for binding in bindings {
            if binding.count == 0 {
                return Err(RhiError::PipelineCreationError(format!(
                    "descriptor binding {} declares a zero descriptor count; \
                     a binding that holds nothing cannot be planned",
                    binding.binding
                )));
            }
            match convert::descriptor_range_type(binding.ty) {
                Some(D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER) => {
                    sampler_count += binding.count;
                }
                Some(_) => cbv_srv_uav_count += binding.count,
                None => {
                    return Err(RhiError::NotSupported(format!(
                        "descriptor binding {} uses {:?}, which has no D3D12 \
                         reflection in this backend's scope: combined \
                         image-sampler pairs are written as two plain \
                         bindings, and inline blocks plus acceleration \
                         structures need shapes this backend does not build",
                        binding.binding, binding.ty
                    )));
                }
            }
        }
        let cbv_srv_uav = cbv_srv_uav_count > 0;
        let sampler = sampler_count > 0;
        if cbv_srv_uav && sampler {
            return Err(RhiError::PipelineCreationError(
                "a descriptor set mixing sampler bindings with CBV/SRV/UAV \
                 bindings spans two D3D12 heap kinds; write them as separate \
                 sets"
                    .into(),
            ));
        }
        Ok((cbv_srv_uav_count, sampler_count, cbv_srv_uav, sampler))
    }

    /// Build and create the root signature: one descriptor-table parameter
    /// per set, space = set index, register = the binding number the layout
    /// declared. The ranges are kept alive until serialization finishes —
    /// the parameter structs point into them by address.
    pub(crate) fn build_root_signature(
        &self,
        layouts: &[&crate::descriptor::DescriptorSetLayout],
    ) -> RhiResult<DxCom<ID3D12RootSignature>> {
        let mut parameters: Vec<D3D12_ROOT_PARAMETER> = Vec::with_capacity(layouts.len());
        let mut range_tables: Vec<Vec<D3D12_DESCRIPTOR_RANGE>> = Vec::with_capacity(layouts.len());
        for (index, layout) in layouts.iter().enumerate() {
            let Some(gpu) = layout.backend() else {
                return Err(RhiError::BackendError(
                    "a descriptor set layout has no GPU backing".into(),
                ));
            };
            let entry = self.layout_entry(gpu.handle)?;
            let table_ranges: Vec<D3D12_DESCRIPTOR_RANGE> = entry
                .bindings
                .iter()
                .filter_map(|binding| {
                    convert::descriptor_range_type(binding.ty)
                        .map(|ty| (ty, binding.binding, binding.count))
                })
                .map(|(ty, register, count)| D3D12_DESCRIPTOR_RANGE {
                    RangeType: ty,
                    NumDescriptors: count,
                    BaseShaderRegister: register,
                    RegisterSpace: index as u32,
                    OffsetInDescriptorsFromTableStart: D3D12_DESCRIPTOR_RANGE_OFFSET_APPEND,
                })
                .collect();
            parameters.push(D3D12_ROOT_PARAMETER {
                ParameterType: D3D12_ROOT_PARAMETER_TYPE_DESCRIPTOR_TABLE,
                Anonymous: D3D12_ROOT_PARAMETER_0 {
                    DescriptorTable: D3D12_ROOT_DESCRIPTOR_TABLE {
                        NumDescriptorRanges: table_ranges.len() as u32,
                        pDescriptorRanges: table_ranges.as_ptr(),
                    },
                },
                ShaderVisibility: D3D12_SHADER_VISIBILITY_ALL,
            });
            range_tables.push(table_ranges);
        }
        let desc = D3D12_ROOT_SIGNATURE_DESC {
            NumParameters: parameters.len() as u32,
            pParameters: parameters.as_ptr(),
            NumStaticSamplers: 0,
            pStaticSamplers: std::ptr::null(),
            Flags: D3D12_ROOT_SIGNATURE_FLAG_ALLOW_INPUT_ASSEMBLER_INPUT_LAYOUT,
        };
        unsafe {
            let mut blob: Option<ID3DBlob> = None;
            let mut errors: Option<ID3DBlob> = None;
            D3D12SerializeRootSignature(
                &desc,
                D3D_ROOT_SIGNATURE_VERSION_1_0,
                &mut blob,
                Some(&mut errors),
            )
            .map_err(|e| {
                let detail = if let Some(text) = &errors {
                    let raw = std::slice::from_raw_parts(
                        text.GetBufferPointer() as *const u8,
                        text.GetBufferSize(),
                    );
                    let message = String::from_utf8_lossy(raw).trim().to_string();
                    if message.is_empty() {
                        None
                    } else {
                        Some(format!(" ({message})"))
                    }
                } else {
                    None
                };
                RhiError::PipelineCreationError(format!(
                    "root signature serialization: {e}{}",
                    detail.unwrap_or_default()
                ))
            })?;
            let blob = blob.ok_or_else(|| {
                RhiError::PipelineCreationError(
                    "root signature serialization returned no blob".into(),
                )
            })?;
            let bytes =
                std::slice::from_raw_parts(blob.GetBufferPointer() as *const u8, blob.GetBufferSize());
            let signature: ID3D12RootSignature = self
                .device
                .0
                .CreateRootSignature::<ID3D12RootSignature>(0, bytes)
                .map_err(|e| {
                    RhiError::PipelineCreationError(format!("CreateRootSignature: {e}"))
                })?;
            Ok(DxCom(signature))
        }
    }

    /// The D3D12 sampler descriptor of one tracked sampler description.
    pub(crate) fn sampler_desc(
        &self,
        desc: &crate::resource::SamplerDesc,
    ) -> D3D12_SAMPLER_DESC {
        D3D12_SAMPLER_DESC {
            Filter: if desc.compare_enable {
                convert::d3d_comparison_filter(desc.min_filter, desc.mag_filter, desc.mipmap_mode)
            } else {
                convert::d3d_filter(desc.min_filter, desc.mag_filter, desc.mipmap_mode)
            },
            AddressU: convert::d3d_address_mode(desc.address_mode_u),
            AddressV: convert::d3d_address_mode(desc.address_mode_v),
            AddressW: convert::d3d_address_mode(desc.address_mode_w),
            MipLODBias: desc.mip_lod_bias,
            MaxAnisotropy: desc.max_anisotropy as u32,
            ComparisonFunc: convert::d3d_compare_func(desc.compare_op),
            BorderColor: convert::d3d_border_color(desc.border_color),
            MinLOD: desc.min_lod,
            MaxLOD: desc.max_lod,
        }
    }

    /// The SRV descriptor of one tracked view: the format comes from the
    /// depth pairing when the resource is typeless, the dimension from the
    /// resource tree with the view's window on top.
    pub(crate) fn srv_desc(
        &self,
        view: &ViewEntry,
    ) -> RhiResult<D3D12_SHADER_RESOURCE_VIEW_DESC> {
        let format = view.srv_format().ok_or_else(|| {
            RhiError::BackendError(format!(
                "the view format {:?} has no D3D12 SRV form",
                view.rhi_format()
            ))
        })?;
        let (dimension, anonymous) = if view.multisampled() {
            if view.layer_count() > 1 {
                (
                    D3D12_SRV_DIMENSION_TEXTURE2DMSARRAY,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DMSArray: D3D12_TEX2DMS_ARRAY_SRV {
                            FirstArraySlice: view.base_layer(),
                            ArraySize: view.layer_count(),
                        },
                    },
                )
            } else {
                (
                    D3D12_SRV_DIMENSION_TEXTURE2DMS,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DMS: D3D12_TEX2DMS_SRV {
                            UnusedField_NothingToDefine: 0,
                        },
                    },
                )
            }
        } else {
            match (view.view_type(), view.layer_count()) {
                (crate::resource::TextureViewType::D1, 1)
                | (crate::resource::TextureViewType::D1Array, 1) => (
                    D3D12_SRV_DIMENSION_TEXTURE1D,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture1D: D3D12_TEX1D_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::D1Array, _) => (
                    D3D12_SRV_DIMENSION_TEXTURE1DARRAY,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture1DArray: D3D12_TEX1D_ARRAY_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            FirstArraySlice: view.base_layer(),
                            ArraySize: view.layer_count(),
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::D1, _) => (
                    D3D12_SRV_DIMENSION_TEXTURE1DARRAY,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture1DArray: D3D12_TEX1D_ARRAY_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            FirstArraySlice: view.base_layer(),
                            ArraySize: view.layer_count(),
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::D2, 1)
                | (crate::resource::TextureViewType::D2Array, 1) => (
                    D3D12_SRV_DIMENSION_TEXTURE2D,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D12_TEX2D_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            PlaneSlice: 0,
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::D2Array, _) => (
                    D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            FirstArraySlice: view.base_layer(),
                            ArraySize: view.layer_count(),
                            PlaneSlice: 0,
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::D2, _) => (
                    D3D12_SRV_DIMENSION_TEXTURE2DARRAY,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2DArray: D3D12_TEX2D_ARRAY_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            FirstArraySlice: view.base_layer(),
                            ArraySize: view.layer_count(),
                            PlaneSlice: 0,
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::Cube, _) => (
                    D3D12_SRV_DIMENSION_TEXTURECUBE,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        TextureCube: D3D12_TEXCUBE_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::CubeArray, _) => (
                    D3D12_SRV_DIMENSION_TEXTURECUBEARRAY,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        TextureCubeArray: D3D12_TEXCUBE_ARRAY_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            First2DArrayFace: view.base_layer(),
                            NumCubes: view.layer_count() / 6,
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
                (crate::resource::TextureViewType::D3, _) => (
                    D3D12_SRV_DIMENSION_TEXTURE3D,
                    D3D12_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture3D: D3D12_TEX3D_SRV {
                            MostDetailedMip: view.base_mip(),
                            MipLevels: view.mip_levels(),
                            ResourceMinLODClamp: 0.0,
                        },
                    },
                ),
            }
        };
        Ok(D3D12_SHADER_RESOURCE_VIEW_DESC {
            Format: format,
            ViewDimension: dimension,
            Shader4ComponentMapping: D3D12_DEFAULT_SHADER_4_COMPONENT_MAPPING,
            Anonymous: anonymous,
        })
    }

    /// The render-target descriptor one framebuffer attachment binds: one
    /// mip and one slice of the view — a render target writes one image;
    /// layered writes (geometry-shader fodder) have no caller in this scope
    /// and come back as a spoken refusal. Cube views make no render
    /// targets anywhere in D3D12.
    pub(crate) fn framebuffer_rtv_desc(
        view: &ViewEntry,
        attachment_mip: u32,
        attachment_layer: u32,
    ) -> RhiResult<D3D12_RENDER_TARGET_VIEW_DESC> {
        let format = view.target_format().ok_or_else(|| {
            RhiError::BackendError(format!(
                "the attachment format {:?} has no D3D12 render-target form",
                view.rhi_format()
            ))
        })?;
        if view.multisampled() {
            if attachment_layer != 0 {
                return Err(RhiError::BackendError(
                    "a multisampled render target is one slice; the \
                     attachment names a layer of it"
                        .into(),
                ));
            }
            return Ok(D3D12_RENDER_TARGET_VIEW_DESC {
                Format: format,
                ViewDimension: D3D12_RTV_DIMENSION_TEXTURE2DMS,
                Anonymous: D3D12_RENDER_TARGET_VIEW_DESC_0 {
                    Texture2DMS: D3D12_TEX2DMS_RTV {
                        UnusedField_NothingToDefine: 0,
                    },
                },
            });
        }
        if matches!(
            view.view_type(),
            crate::resource::TextureViewType::Cube | crate::resource::TextureViewType::CubeArray
        ) {
            return Err(RhiError::NotSupported(
                "a cube view makes no render target; bind one face as a plain \
                 2D view instead"
                    .into(),
            ));
        }
        let mip = view.base_mip() + attachment_mip;
        let (dimension, anonymous) = match (view.view_type(), view.dimensions()) {
            (crate::resource::TextureViewType::D1, TextureDimensions::D1) => {
                if view.layer_count() > 1 || attachment_layer != 0 {
                    (
                        D3D12_RTV_DIMENSION_TEXTURE1DARRAY,
                        D3D12_RENDER_TARGET_VIEW_DESC_0 {
                            Texture1DArray: D3D12_TEX1D_ARRAY_RTV {
                                MipSlice: mip,
                                FirstArraySlice: view.base_layer() + attachment_layer,
                                ArraySize: 1,
                            },
                        },
                    )
                } else {
                    (
                        D3D12_RTV_DIMENSION_TEXTURE1D,
                        D3D12_RENDER_TARGET_VIEW_DESC_0 {
                            Texture1D: D3D12_TEX1D_RTV { MipSlice: mip },
                        },
                    )
                }
            }
            (_, TextureDimensions::D3) => (
                D3D12_RTV_DIMENSION_TEXTURE3D,
                D3D12_RENDER_TARGET_VIEW_DESC_0 {
                    Texture3D: D3D12_TEX3D_RTV {
                        MipSlice: mip,
                        FirstWSlice: attachment_layer,
                        WSize: 1,
                    },
                },
            ),
            _ => {
                if view.layer_count() > 1 || attachment_layer != 0 {
                    (
                        D3D12_RTV_DIMENSION_TEXTURE2DARRAY,
                        D3D12_RENDER_TARGET_VIEW_DESC_0 {
                            Texture2DArray: D3D12_TEX2D_ARRAY_RTV {
                                MipSlice: mip,
                                FirstArraySlice: view.base_layer() + attachment_layer,
                                ArraySize: 1,
                                PlaneSlice: 0,
                            },
                        },
                    )
                } else {
                    (
                        D3D12_RTV_DIMENSION_TEXTURE2D,
                        D3D12_RENDER_TARGET_VIEW_DESC_0 {
                            Texture2D: D3D12_TEX2D_RTV {
                                MipSlice: mip,
                                PlaneSlice: 0,
                            },
                        },
                    )
                }
            }
        };
        Ok(D3D12_RENDER_TARGET_VIEW_DESC {
            Format: format,
            ViewDimension: dimension,
            Anonymous: anonymous,
        })
    }

    /// The depth-stencil descriptor one framebuffer attachment binds: the
    /// same one-mip-one-slice rule as the RTV sibling.
    pub(crate) fn framebuffer_dsv_desc(
        view: &ViewEntry,
        attachment_mip: u32,
        attachment_layer: u32,
    ) -> RhiResult<D3D12_DEPTH_STENCIL_VIEW_DESC> {
        let format = view.target_format().ok_or_else(|| {
            RhiError::BackendError(format!(
                "the depth attachment format {:?} has no D3D12 form",
                view.rhi_format()
            ))
        })?;
        if view.multisampled() {
            if attachment_layer != 0 {
                return Err(RhiError::BackendError(
                    "a multisampled depth target is one slice; the attachment \
                     names a layer of it"
                        .into(),
                ));
            }
            return Ok(D3D12_DEPTH_STENCIL_VIEW_DESC {
                Format: format,
                ViewDimension: D3D12_DSV_DIMENSION_TEXTURE2DMS,
                Flags: D3D12_DSV_FLAG_NONE,
                Anonymous: D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                    Texture2DMS: D3D12_TEX2DMS_DSV {
                        UnusedField_NothingToDefine: 0,
                    },
                },
            });
        }
        let mip = view.base_mip() + attachment_mip;
        let (dimension, anonymous) = match (view.view_type(), view.dimensions()) {
            (crate::resource::TextureViewType::D1, TextureDimensions::D1) => {
                if view.layer_count() > 1 || attachment_layer != 0 {
                    (
                        D3D12_DSV_DIMENSION_TEXTURE1DARRAY,
                        D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                            Texture1DArray: D3D12_TEX1D_ARRAY_DSV {
                                MipSlice: mip,
                                FirstArraySlice: view.base_layer() + attachment_layer,
                                ArraySize: 1,
                            },
                        },
                    )
                } else {
                    (
                        D3D12_DSV_DIMENSION_TEXTURE1D,
                        D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                            Texture1D: D3D12_TEX1D_DSV { MipSlice: mip },
                        },
                    )
                }
            }
            _ => {
                if view.layer_count() > 1 || attachment_layer != 0 {
                    (
                        D3D12_DSV_DIMENSION_TEXTURE2DARRAY,
                        D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                            Texture2DArray: D3D12_TEX2D_ARRAY_DSV {
                                MipSlice: mip,
                                FirstArraySlice: view.base_layer() + attachment_layer,
                                ArraySize: 1,
                            },
                        },
                    )
                } else {
                    (
                        D3D12_DSV_DIMENSION_TEXTURE2D,
                        D3D12_DEPTH_STENCIL_VIEW_DESC_0 {
                            Texture2D: D3D12_TEX2D_DSV { MipSlice: mip },
                        },
                    )
                }
            }
        };
        Ok(D3D12_DEPTH_STENCIL_VIEW_DESC {
            Format: format,
            ViewDimension: dimension,
            Flags: D3D12_DSV_FLAG_NONE,
            Anonymous: anonymous,
        })
    }

    /// The vertex-input elements from the VS input signature reflection: one
    /// element per location the shader reads, fed from the pipeline's input
    /// declaration. A shader that reads nothing (SV_VertexID only) carries
    /// an empty signature, and then no layout is set at all.
    ///
    /// The reflection goes through `D3DReflect` and the returned
    /// `ID3D11ShaderReflection`, not through `D3DGetInputSignatureBlob`:
    /// that blob is a finished DXBC container (36-byte header + the ISGN
    /// chunk with its string table), and casting its bytes to an
    /// array of descriptors fed raw container bytes to `Register` — the
    /// shader-map read ASCII (`"TECO"…`) where a location had to be. The
    /// reflection interface hands the parameters out one at a time, which
    /// is the only honest contract here.
    ///
    /// The reflector comes back with the elements on purpose: the semantic
    /// names of the elements are `PCSTR`s into its string table, and the
    /// pipeline creation reads them when it builds the PSO. A local
    /// reflector dropped at this function's end would free the strings
    /// before the PSO creation read them — the caller binds the returned
    /// reflector so it outlives `CreateGraphicsPipelineState`.
    pub(crate) fn vertex_input_elements(
        vs_code: &[u8],
        input: &crate::pipeline::graphics::VertexInputState,
    ) -> RhiResult<(
        Vec<D3D12_INPUT_ELEMENT_DESC>,
        Option<ID3D11ShaderReflection>,
    )> {
        let mut reflector: Option<ID3D11ShaderReflection> = None;
        unsafe {
            D3DReflect(
                vs_code.as_ptr() as *const core::ffi::c_void,
                vs_code.len(),
                &ID3D11ShaderReflection::IID,
                &mut reflector as *mut _ as *mut *mut core::ffi::c_void,
            )
        }
        .map_err(|e| {
            RhiError::PipelineCreationError(format!("vertex signature reflection: {e}"))
        })?;
        let reflector = reflector.ok_or_else(|| {
            RhiError::PipelineCreationError(
                "the vertex signature reflection returned no reflector".into(),
            )
        })?;
        let mut shader_desc = D3D11_SHADER_DESC::default();
        unsafe { reflector.GetDesc(&mut shader_desc) }.map_err(|e| {
            RhiError::PipelineCreationError(format!("vertex shader description: {e}"))
        })?;
        let mut elements = Vec::new();
        for parameter in 0..shader_desc.InputParameters {
            let mut param = D3D11_SIGNATURE_PARAMETER_DESC::default();
            unsafe { reflector.GetInputParameterDesc(parameter, &mut param) }.map_err(
                |e| {
                    RhiError::PipelineCreationError(format!(
                        "vertex shader input parameter {parameter}: {e}"
                    ))
                },
            )?;
            // System-value inputs (SV_VertexID and friends) sit in the
            // signature without consuming a vertex-buffer attribute: they
            // must not swallow a real attribute's location (SV_VertexID
            // occupies register 0), and a system semantic is not a
            // vertex-buffer element.
            if param.SystemValueType != D3D10_NAME_UNDEFINED {
                continue;
            }
            let attribute = input
                .attributes
                .iter()
                .find(|a| a.location == param.Register)
                .ok_or_else(|| {
                    RhiError::PipelineCreationError(format!(
                        "the vertex shader reads vertex location {} and the \
                         pipeline's vertex input provides no attribute for it",
                        param.Register
                    ))
                })?;
            let format = convert::dxgi_format(attribute.format).ok_or_else(|| {
                RhiError::PipelineCreationError(format!(
                    "no D3D12 format for the vertex attribute at location {}",
                    attribute.location
                ))
            })?;
            let slot = input
                .bindings
                .iter()
                .find(|b| b.binding == attribute.binding)
                .ok_or_else(|| {
                    RhiError::PipelineCreationError(format!(
                        "the attribute at location {} uses binding {} and the \
                         pipeline's vertex input provides no such binding",
                        attribute.location, attribute.binding
                    ))
                })?;
            let instance =
                matches!(slot.input_rate, crate::pipeline::graphics::VertexInputRate::Instance);
            elements.push(D3D12_INPUT_ELEMENT_DESC {
                SemanticName: param.SemanticName,
                SemanticIndex: param.SemanticIndex,
                Format: format,
                InputSlot: attribute.binding,
                AlignedByteOffset: attribute.offset,
                InputSlotClass: if instance {
                    D3D12_INPUT_CLASSIFICATION_PER_INSTANCE_DATA
                } else {
                    D3D12_INPUT_CLASSIFICATION_PER_VERTEX_DATA
                },
                InstanceDataStepRate: if instance { 1 } else { 0 },
            });
        }
        Ok((elements, Some(reflector)))
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
                home_state: initial_state,
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
        self.submit_sequential(|list| {
            self.transition(
                list,
                &dst.0,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            unsafe {
                // `CopyBufferRegion` is note-exceptional among the D3D12 copy
                // calls: the DESTINATION comes first (dst, dstoffset, src,
                // srcoffset, bytes), for both the SDK and the windows 0.61
                // bindings. Reading it as src-first copies backwards: the
                // upload zeroes the staging instead of filling the buffer
                // (BUGS №729).
                list.CopyBufferRegion(&dst.0, 0, &staging, 0, data.len() as u64)
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
        self.submit_sequential(|list| {
            self.transition(
                list,
                &src.0,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
            );
            unsafe {
                // Destination-first order, see the upload comment above
                // (BUGS №729): the READBACK staging is written from the
                // device-local buffer.
                list.CopyBufferRegion(&staging, 0, &src.0, offset, size)
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
                home_state: D3D12_RESOURCE_STATE_COMMON,
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
        // The upload contract is the one the Vulkan backend carries today:
        // the mip chain of layer 0 fills from one tightly packed buffer, and
        // anything wider is refused rather than half-written. The first
        // version of this path wrote subresource 0 with a hand-rolled mip-0
        // layout and left the rest of the chain as uninitialized committed
        // memory (bug №737).
        if desc.array_layers > 1 {
            return Err(RhiError::NotSupported(
                "multi-layer texture upload is not supported yet".into(),
            ));
        }
        // The staging geometry is the driver's word, not a re-derivation: one
        // `GetCopyableFootprints` call covers the layer-0 mip chain —
        // subresource indexing puts the mip above the layer, so the first
        // `mip_levels` entries are exactly layer 0 — and returns every
        // subresource's 256-byte-aligned staging offset, row count, and
        // tight row size. The desc must be the one the resource was created
        // from (`texture_desc` may hand back a typeless base for depth
        // formats; rows and pitches are computed the same either way).
        let resource_desc = Self::texture_desc(&desc)?;
        let count = desc.mip_levels.max(1);
        let mut layouts = vec![D3D12_PLACED_SUBRESOURCE_FOOTPRINT::default(); count as usize];
        let mut num_rows = vec![0u32; count as usize];
        let mut row_size = vec![0u64; count as usize];
        let mut total = 0u64;
        // SAFETY: the three output slices match `count` exactly and the
        // resource desc is a plain value passed by reference.
        unsafe {
            self.device.0.GetCopyableFootprints(
                &resource_desc,
                0,
                count,
                0,
                Some(layouts.as_mut_ptr()),
                Some(num_rows.as_mut_ptr()),
                Some(row_size.as_mut_ptr()),
                Some(&mut total),
            );
        }
        // The input rows sit at the tight row size the driver reports — for
        // every format of the current table this equals the 4-aligned row
        // stride the contract shares with Vulkan — and every staging row
        // sits at the 256-byte `RowPitch`.
        let needed = num_rows
            .iter()
            .zip(row_size.iter())
            .map(|(rows, size)| *rows as u64 * *size)
            .sum::<u64>();
        if (data.len() as u64) < needed {
            return Err(RhiError::BackendError(format!(
                "the {}-byte upload is too small: the mip chain needs {} bytes \
                 (format {:?}, {}x{}, {} mips)",
                data.len(),
                needed,
                desc.format,
                desc.width,
                desc.height,
                desc.mip_levels
            )));
        }
        let staging = self.staging_buffer(total, true)?;
        // SAFETY: `total` bytes come from the freshly created staging buffer;
        // every slice below stays inside it by construction.
        unsafe {
            let mut ptr = std::ptr::null_mut();
            staging
                .Map(0, None, Some(&mut ptr))
                .map_err(|e| RhiError::BackendError(format!("upload staging Map: {e}")))?;
            let base = ptr as *mut u8;
            let mut cursor = 0usize;
            for mip in 0..count as usize {
                let rows = num_rows[mip] as usize;
                let tight = row_size[mip] as usize;
                let pitch = layouts[mip].Footprint.RowPitch as usize;
                let dst = base.add(layouts[mip].Offset as usize);
                let src = data.as_ptr().add(cursor);
                for row in 0..rows {
                    std::ptr::copy_nonoverlapping(
                        src.add(row * tight),
                        dst.add(row * pitch),
                        tight,
                    );
                }
                cursor += rows * tight;
            }
            staging.Unmap(0, None);
        }
        // One transition pair spans the whole chain: the resource goes to
        // COPY_DEST once, the copies land mip by mip — dst-first per the
        // `CopyTextureRegion` argument order (bug №729's lesson) — and the
        // resource returns to COMMON.
        let tex = entry.resource.clone();
        self.submit_sequential(|list| {
            self.transition(
                list,
                &tex.0,
                D3D12_RESOURCE_STATE_COMMON,
                D3D12_RESOURCE_STATE_COPY_DEST,
            );
            for mip in 0..count as usize {
                let src_view = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::ManuallyDrop::new(Some(staging.clone())),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: layouts[mip],
                    },
                };
                let dst_view = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::ManuallyDrop::new(Some(tex.0.clone())),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: Self::subresource_index(&desc, mip as u32, 0),
                    },
                };
                unsafe { list.CopyTextureRegion(&dst_view, 0, 0, 0, &src_view, None) };
            }
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
        self.submit_sequential(|list| {
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

    /// The M2 seam (Баг№706): recorded `Command`s replay on a real graphics
    /// command list. Same sequential shape the copy paths own (a one-allocator
    /// reset, record, close, `ExecuteCommandLists`, fence wait) — a recorded
    /// list and a copy are one path here until a frame ring replaces the
    /// waits. The translation itself lives in `record.rs`, next to the
    /// D3D11/Vulkan translators.
    fn submit_commands(&self, commands: &[crate::command::commands::Command]) -> RhiResult<()> {
        if commands.is_empty() {
            return Err(RhiError::BackendError(
                "refusing to submit an empty command list: the recording produced nothing, \
                 which almost always means the encoder dropped the commands"
                    .into(),
            ));
        }
        self.submit_sequential(|list| super::record::record(list, self, commands))
    }

    /// Destroy a tracked buffer or texture now instead of at device teardown.
    ///
    /// Bug №731: the defaults are no-ops, which leaked every explicitly
    /// released object until the device was dropped — the D3D12 edition of
    /// the leak bug №289 fixed for Vulkan. The handle table is this
    /// backend's single bookkeeping, so a release removes the entry, which
    /// also drops the last carried COM reference and destroys the committed
    /// resource.
    ///
    /// Untracked handles and kinds this backend does not own are no-ops,
    /// which is what makes a repeated `release` harmless rather than a
    /// double free (the same contract the Vulkan backend follows). Handles
    /// stay private to this crate by design — see `Device::release_many` —
    /// so the kind-matching half of the contract is covered by the in-crate
    /// tests below this impl.
    fn release(&self, kind: ReleaseKind, object: u64) {
        self.release_many(&[(kind, object)]);
    }

    /// Bug №731: see [`BackendResource::release_many`]; the batch is one
    /// removal pass, one `wait_idle` for the whole batch, and one destroy
    /// pass — the same shape `Device::release_many` documents for the
    /// per-batch wait.
    fn release_many(&self, objects: &[(ReleaseKind, u64)]) {
        if objects.is_empty() {
            return;
        }
        // Collect first, destroy second: removing and destroying while
        // holding the lock would serialise driver calls behind the mutex,
        // and removing inline already makes a repeated handle in one batch
        // a no-op rather than a double remove.
        let mut dead: Vec<D3DObject> = Vec::new();
        let Ok(mut handles) = self.handles.lock() else {
            return;
        };
        for (kind, object) in objects {
            let matches = match kind {
                ReleaseKind::Buffer => {
                    matches!(handles.get(object), Some(D3DObject::Buffer(_)))
                }
                ReleaseKind::Image => {
                    matches!(handles.get(object), Some(D3DObject::Texture(_)))
                }
                // Framebuffers and views are not tracked here yet: dropping
                // them is the honest no-op, and the Vulkan side of this
                // surface shows what to add once they exist.
                _ => false,
            };
            if matches {
                if let Some(entry) = handles.remove(object) {
                    dead.push(entry);
                }
            }
        }
        drop(handles);
        if dead.is_empty() {
            return;
        }
        // Nothing the GPU may still be reading is destroyed first: the
        // Vulkan side of this surface waits for the device before it frees
        // (bug №289), release traffic here is resize/teardown traffic, and
        // the copy path keeps every resource in COMMON state anyway. One
        // wait covers the whole batch, not one per object.
        self.wait_idle().ok();
        for entry in dead {
            if let D3DObject::Buffer(entry) = &entry {
                if entry.upload_heap && !entry.mapped.is_null() {
                    // An upload-heap buffer is persistently mapped; the
                    // mapping must not outlive the resource it points into.
                    // Result ignored: nothing sensible can be done if the
                    // driver reports a range issue, and the release stands.
                    unsafe { entry.resource.0.Unmap(0, None) };
                }
            }
            // Dropping the entry drops the last COM reference, which
            // releases the committed resource itself.
            drop(entry);
        }
    }

    /// See [`BackendResource::release_texture`]; overrides the no-op default
    /// that leaked the texture until device teardown (bug №731).
    fn release_texture(&self, texture: &crate::resource::Texture) {
        let Some(gpu) = texture.backend() else {
            return;
        };
        self.release(ReleaseKind::Image, gpu.handle);
    }

    fn tracked_object_count(&self) -> usize {
        self.handles.lock().unwrap().len()
    }

    fn tracked_object_summary(&self) -> String {
        let handles = self.handles.lock().unwrap();
        let mut buffers = 0usize;
        let mut textures = 0usize;
        let mut others = 0usize;
        for object in handles.values() {
            match object {
                D3DObject::Buffer(_) => buffers += 1,
                D3DObject::Texture(_) => textures += 1,
                D3DObject::View(_)
                | D3DObject::Sampler(_)
                | D3DObject::Shader(_)
                | D3DObject::GraphicsPipeline(_)
                | D3DObject::Pass(_)
                | D3DObject::Framebuffer(_)
                | D3DObject::Layout(_)
                | D3DObject::Set(_) => others += 1,
            }
        }
        format!(
            "buffers: {}, textures: {}, others: {}",
            buffers, textures, others
        )
    }

    // ————————————————————————————————————————————————————————————————————
    // M3 (Баг№707): views, samplers, shader modules, descriptor sets,
    // render passes and pipelines. The shape follows the D3D11 backend —
    // every creator makes one tracked entry — with the D3D12 subtlety that
    // a descriptor is a heap slot, not a COM object: the descriptor bytes
    // are written where they land (a set's block, a framebuffer slot), so
    // the entries carry the facts and not the descriptors.
    // ————————————————————————————————————————————————————————————————————

    fn create_shader_module(&self, desc: &crate::shader::ShaderModuleDesc) -> RhiResult<crate::shader::ShaderModule> {
        if !matches!(desc.format, crate::shader::ShaderFormat::Dxil) {
            return Err(RhiError::NotSupported(format!(
                "D3D12 accepts DXBC/DXIL containers only, got format {:?}",
                desc.format
            )));
        }
        if desc.code.is_empty() {
            return Err(RhiError::ShaderCompilationError(
                "the shader module is empty".into(),
            ));
        }
        // Format by itself is only a label; the bytes must carry the
        // container magic both D3D producers (fxc and dxc) emit, or the
        // refusal belongs here — a driver-level rejection much later would
        // point at the draw, not at the bytes that caused it.
        if &desc.code[..4] != b"DXBC" {
            return Err(RhiError::ShaderCompilationError(
                "the shader module bytes are not a D3D12 shader container; \
                 a DXIL or DXBC container starts with the DXBC magic"
                    .into(),
            ));
        }
        let handle = self.next_handle();
        self.handles.lock().unwrap().insert(
            handle,
            D3DObject::Shader(ShaderEntry {
                format: desc.format,
                code: desc.code.clone(),
            }),
        );
        let mut out = crate::shader::ShaderModule::new(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_sampler(&self, desc: &crate::resource::SamplerDesc) -> RhiResult<crate::resource::Sampler> {
        if desc.unnormalized_coordinates {
            return Err(RhiError::NotSupported(
                "D3D12 sampler coordinates are always normalized".into(),
            ));
        }
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .insert(handle, D3DObject::Sampler(SamplerEntry { desc: desc.clone() }));
        let mut out = crate::resource::Sampler::from_desc(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_descriptor_set_layout(
        &self,
        desc: &crate::descriptor::DescriptorSetLayoutDesc,
    ) -> RhiResult<crate::descriptor::DescriptorSetLayout> {
        let (cbv_srv_uav_count, sampler_count, cbv_srv_uav, sampler) =
            Self::classify_descriptors(&desc.bindings)?;
        // Duplicate binding numbers are invisible in the desc but would be
        // a silent overwrite in the offset plan the writes navigate.
        for (index, binding) in desc.bindings.iter().enumerate() {
            for earlier in &desc.bindings[..index] {
                if earlier.binding == binding.binding {
                    return Err(RhiError::PipelineCreationError(format!(
                        "descriptor binding {} is declared twice; D3D12 has \
                         one register per binding number",
                        binding.binding
                    )));
                }
            }
        }
        let handle = self.next_handle();
        self.handles.lock().unwrap().insert(
            handle,
            D3DObject::Layout(LayoutEntry {
                bindings: desc.bindings.clone(),
                cbv_srv_uav_count,
                sampler_count,
                cbv_srv_uav,
                sampler,
            }),
        );
        let mut out = crate::descriptor::DescriptorSetLayout::new(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_descriptor_set(&self, layout: &crate::descriptor::DescriptorSetLayout) -> RhiResult<crate::descriptor::DescriptorSet> {
        let Some(gpu) = layout.backend() else {
            return Err(RhiError::BackendError(
                "a descriptor set layout has no GPU backing".into(),
            ));
        };
        let entry = self.layout_entry(gpu.handle)?;
        // The set reserves its block(s) now: a set's descriptors are
        // contiguous by design so binding is one GPU pointer per table.
        let block_base = if entry.cbv_srv_uav_count > 0 {
            Some(self.heaps.alloc(
                super::descriptor::DescriptorKind::CbvSrvUav,
                entry.cbv_srv_uav_count as usize,
            )?)
        } else {
            None
        };
        let sampler_block_base = if entry.sampler_count > 0 {
            Some(self.heaps.alloc(
                super::descriptor::DescriptorKind::Sampler,
                entry.sampler_count as usize,
            )?)
        } else {
            None
        };
        let handle = self.next_handle();
        self.handles.lock().unwrap().insert(
            handle,
            D3DObject::Set(SetEntry {
                layout: entry.clone(),
                block_base,
                sampler_block_base,
            }),
        );
        let mut out = crate::descriptor::DescriptorSet::new(layout.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn write_descriptors(
        &self,
        set: &crate::descriptor::DescriptorSet,
        writes: &[crate::descriptor::DescriptorWrite],
    ) -> RhiResult<()> {
        let Some(gpu) = set.backend() else {
            return Err(RhiError::BackendError(
                "the descriptor set being written has no GPU backing".into(),
            ));
        };
        let entry = self.set_entry(gpu.handle)?;
        let layout = entry.layout().clone();
        let block_base = entry.block_base();
        let sampler_block_base = entry.sampler_block_base();
        for write in writes {
            let Some((range_type, binding_offset)) = layout.binding_offset(write.dst_binding) else {
                return Err(RhiError::BackendError(format!(
                    "a descriptor write addresses binding {}, which the set's \
                     layout has no entry for",
                    write.dst_binding
                )));
            };
            let declared_count = layout.binding_count(write.dst_binding).unwrap_or(0);
            if write.dst_array_element >= declared_count {
                return Err(RhiError::BackendError(format!(
                    "a descriptor write's element {} is past the {} declared \
                     for binding {}",
                    write.dst_array_element, declared_count, write.dst_binding
                )));
            }
            for (slot_index, info) in write.descriptors.iter().enumerate() {
                let slot = binding_offset + write.dst_array_element + slot_index as u32;
                if slot_index as u32 >= declared_count - write.dst_array_element {
                    return Err(RhiError::BackendError(format!(
                        "a descriptor write to binding {} supplies {} items \
                         past element {}, which exceeds the binding's declared {}",
                        write.dst_binding,
                        write.descriptors.len(),
                        write.dst_array_element,
                        declared_count
                    )));
                }
                unsafe {
                    match info {
                        crate::descriptor::DescriptorInfo::Sampler(sampler) => {
                            if range_type != D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER {
                                return Err(RhiError::BackendError(format!(
                                    "binding {} declares a non-sampler kind but \
                                     the write supplies a sampler",
                                    write.dst_binding
                                )));
                            }
                            let Some(sampler_base) = sampler_block_base else {
                                return Err(RhiError::InternalError(
                                    "the set reserved no sampler descriptors".into(),
                                ));
                            };
                            let Some(sampler_gpu) = sampler.backend() else {
                                return Err(RhiError::BackendError(
                                    "the written sampler has no GPU backing".into(),
                                ));
                            };
                            let sampler_entry = self.sampler_entry(sampler_gpu.handle)?;
                            let sampler_desc = self.sampler_desc(sampler_entry.desc());
                            self.device.0.CreateSampler(
                                &sampler_desc,
                                self.heaps
                                    .cpu(super::descriptor::DescriptorKind::Sampler, sampler_base + slot as usize),
                            );
                        }
                        crate::descriptor::DescriptorInfo::Buffer(buffer, offset, size) => {
                            if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER {
                                return Err(RhiError::BackendError(format!(
                                    "binding {} declares a sampler kind but the \
                                     write supplies a buffer",
                                    write.dst_binding
                                )));
                            }
                            if range_type != D3D12_DESCRIPTOR_RANGE_TYPE_CBV {
                                return Err(RhiError::NotSupported(format!(
                                    "binding {} declares a {:?} kind whose write \
                                     supplies a buffer; D3D12 makes a buffer \
                                     binding a constant buffer view here, which \
                                     is M4's compute territory",
                                    write.dst_binding, range_type
                                )));
                            }
                            let Some(base) = block_base else {
                                return Err(RhiError::InternalError(
                                    "the set reserved no CBV/SRV/UAV descriptors".into(),
                                ));
                            };
                            let Some(buffer_gpu) = buffer.backend() else {
                                return Err(RhiError::BackendError(
                                    "the written buffer has no GPU backing".into(),
                                ));
                            };
                            let buffer_entry = self.buffer_entry(buffer_gpu.handle)?;
                            // A CBV addresses the whole buffer from a
                            // 256-aligned location; a non-zero offset has no
                            // place in this binding model — the caller
                            // creates the sub-buffer it means instead.
                            if *offset != 0 {
                                return Err(RhiError::NotSupported(format!(
                                    "a D3D12 constant buffer view binds the \
                                     whole resource; the write's offset {} has \
                                     no reflection — bind the sub-buffer itself",
                                    offset
                                )));
                            }
                            if *size == 0 {
                                return Err(RhiError::BackendError(
                                    "a constant buffer view of zero bytes \
                                     writes nothing and describes nothing"
                                        .into(),
                                ));
                            }
                            let range_end = *offset + *size;
                            if range_end > buffer_entry.size() {
                                return Err(RhiError::BackendError(format!(
                                    "the constant buffer view covers {}..{} of a \
                                     {}-byte buffer",
                                    *offset,
                                    range_end,
                                    buffer_entry.size()
                                )));
                            }
                            let view_desc = D3D12_CONSTANT_BUFFER_VIEW_DESC {
                                BufferLocation: buffer_entry.resource().GetGPUVirtualAddress(),
                                // BUGS №761: D3D12 requires the view's size
                                // to be a multiple of 256; the declared
                                // byte range alone faults the device (the
                                // removal surfaces on the next pipeline
                                // creation as 0x887A0005). The view rounds
                                // the range up: the shader reads the same
                                // first bytes, the trailing range is the
                                // alignment padding.
                                SizeInBytes: Self::cbv_view_size(range_end),
                            };
                            self.device.0.CreateConstantBufferView(
                                Some(&view_desc),
                                self.heaps.cpu(
                                    super::descriptor::DescriptorKind::CbvSrvUav,
                                    base + slot as usize,
                                ),
                            );
                        }
                        crate::descriptor::DescriptorInfo::Texture(view, combined) => {
                            if combined.is_some() {
                                return Err(RhiError::NotSupported(
                                    "a combined image-sampler write has no D3D12 \
                                     form here; the sampler goes into its own \
                                     binding"
                                        .into(),
                                ));
                            }
                            if range_type == D3D12_DESCRIPTOR_RANGE_TYPE_SAMPLER {
                                return Err(RhiError::BackendError(format!(
                                    "binding {} declares a sampler kind but the \
                                     write supplies a texture",
                                    write.dst_binding
                                )));
                            }
                            if range_type != D3D12_DESCRIPTOR_RANGE_TYPE_SRV {
                                return Err(RhiError::NotSupported(format!(
                                    "binding {} declares a {:?} kind whose write \
                                     supplies a texture; storage views are M4 \
                                     compute territory",
                                    write.dst_binding, range_type
                                )));
                            }
                            let Some(base) = block_base else {
                                return Err(RhiError::InternalError(
                                    "the set reserved no CBV/SRV/UAV descriptors".into(),
                                ));
                            };
                            let Some(view_gpu) = view.backend() else {
                                return Err(RhiError::BackendError(
                                    "the written texture view has no GPU backing".into(),
                                ));
                            };
                            let view_entry = self.view_entry(view_gpu.handle)?;
                            let srv_desc = self.srv_desc(&view_entry)?;
                            self.device.0.CreateShaderResourceView(
                                view_entry.resource(),
                                Some(&srv_desc),
                                self.heaps.cpu(
                                    super::descriptor::DescriptorKind::CbvSrvUav,
                                    base + slot as usize,
                                ),
                            );
                        }
                        crate::descriptor::DescriptorInfo::InlineUniformBlock(_) => {
                            return Err(RhiError::NotSupported(
                                "inline uniform blocks have no D3D12 form in \
                                 this backend's scope".into(),
                            ));
                        }
                        crate::descriptor::DescriptorInfo::AccelerationStructure(_) => {
                            return Err(RhiError::NotSupported(
                                "acceleration structures have no D3D12 form in \
                                 this backend's scope".into(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    // ————————————————————————————————————————————————————————————————————
    // M3, continued (Баг№707): texture views — the windows every descriptor
    // reads through —, render passes, framebuffers (their RTV/DSV slots are
    // allocated once and bound by pointer), graphics pipelines (one root
    // signature, one PSO, input-layout reflection from the VS signature)
    // and the immediate framebuffer draw helper. Depth classification: a
    // view whose RHI format is the depth family binds a DSV; every other
    // attachment binds an RTV.
    // ————————————————————————————————————————————————————————————————————

    fn create_texture_view(
        &self,
        texture: &Texture,
        desc: &crate::resource::TextureViewDesc,
    ) -> RhiResult<TextureView> {
        let gpu = texture.backend().ok_or_else(|| {
            RhiError::BackendError("the texture being viewed has no GPU backing".into())
        })?;
        let tex = self.texture_entry(gpu.handle)?;
        let resource_format = tex.desc().format;
        // The format policy follows the D3D11 backend's honesty with one
        // D3D12 extension: a view keeps the resource's format, and only a
        // depth resource (created typeless) recasts to the typed depth-side
        // format the conversion layer pairs with it.
        let format = desc.format.unwrap_or(resource_format);
        if desc.format.is_some() && desc.format != Some(resource_format) {
            let paired = convert::typeless_base(resource_format).is_some()
                && convert::depth_srv_format(resource_format) == convert::dxgi_format(format);
            if !paired {
                return Err(RhiError::NotSupported(format!(
                    "a D3D12 view cannot recast {:?} into {:?}; views keep the \
                     resource's format, and only the depth family couples with \
                     its typeless SRV partner",
                    resource_format, format
                )));
            }
        }
        // The subresource windows: a zero count is "the rest", the remainder
        // convention the RHI view descriptions name. Every window must land
        // inside the resource — an out-of-window view would read subresources
        // the resource never made.
        let base_mip = desc.base_mip_level;
        let mip_levels = if desc.mip_level_count == 0 {
            tex.desc().mip_levels - base_mip
        } else {
            desc.mip_level_count
        };
        if base_mip >= tex.desc().mip_levels
            || mip_levels == 0
            || base_mip + mip_levels > tex.desc().mip_levels
        {
            return Err(RhiError::BackendError(format!(
                "the view mip window {}..{} does not fit the resource's {} \
                 mip levels",
                base_mip,
                base_mip + mip_levels,
                tex.desc().mip_levels
            )));
        }
        let base_layer = desc.base_array_layer;
        let layer_count = if desc.array_layer_count == 0 {
            tex.desc().array_layers - base_layer
        } else {
            desc.array_layer_count
        };
        if base_layer >= tex.desc().array_layers
            || layer_count == 0
            || base_layer + layer_count > tex.desc().array_layers
        {
            return Err(RhiError::BackendError(format!(
                "the view layer window {}..{} does not fit the resource's {} \
                 array layers",
                base_layer, base_layer + layer_count, tex.desc().array_layers
            )));
        }
        // The view shape announces what the shaders read through: every
        // shape only reads out of the resource tree it cuts into.
        let wants_d1 = matches!(
            desc.view_type,
            crate::resource::TextureViewType::D1 | crate::resource::TextureViewType::D1Array
        );
        let has_d1 = matches!(tex.desc().dimensions, TextureDimensions::D1);
        if wants_d1 != has_d1 {
            return Err(RhiError::BackendError(format!(
                "a {:?} view reads a D1 tree; the resource is {:?}",
                desc.view_type, tex.desc().dimensions
            )));
        }
        let wants_d2 = matches!(
            desc.view_type,
            crate::resource::TextureViewType::D2
                | crate::resource::TextureViewType::D2Array
                | crate::resource::TextureViewType::Cube
                | crate::resource::TextureViewType::CubeArray
        );
        let has_d2 = matches!(tex.desc().dimensions, TextureDimensions::D2);
        if wants_d2 != has_d2 {
            return Err(RhiError::BackendError(format!(
                "a {:?} view reads a D2 tree; the resource is {:?}",
                desc.view_type, tex.desc().dimensions
            )));
        }
        let wants_d3 = matches!(desc.view_type, crate::resource::TextureViewType::D3);
        let has_d3 = matches!(tex.desc().dimensions, TextureDimensions::D3);
        if wants_d3 != has_d3 {
            return Err(RhiError::BackendError(format!(
                "a {:?} view reads a D3 tree; the resource is {:?}",
                desc.view_type, tex.desc().dimensions
            )));
        }
        if (desc.view_type == crate::resource::TextureViewType::Cube && layer_count != 6)
            || (desc.view_type == crate::resource::TextureViewType::CubeArray
                && layer_count % 6 != 0)
        {
            return Err(RhiError::BackendError(format!(
                "a cube view reads cube faces in whole six-face groups; the \
                 asked window of {} layers is not one",
                layer_count
            )));
        }
        let multisampled = tex.desc().sample_count.as_count() > 1;
        if multisampled && (base_mip != 0 || mip_levels != 1) {
            return Err(RhiError::BackendError(
                "a multisampled resource carries one mip level; its views \
                 read that one level"
                    .into(),
            ));
        }
        let entry = ViewEntry {
            resource: tex.resource.clone(),
            texture_handle: gpu.handle,
            format,
            view_type: desc.view_type,
            dimensions: tex.desc().dimensions,
            multisampled,
            base_mip,
            mip_levels,
            base_layer,
            layer_count,
        };
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .insert(handle, D3DObject::View(entry));
        let mut out = TextureView::from_parts(texture.clone(), desc.clone());
        out.set_backend(GpuResource {
            handle,
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_render_pass(&self, desc: &RenderPassDesc) -> RhiResult<RenderPass> {
        if desc.subpasses.len() > 1 {
            return Err(RhiError::NotSupported(format!(
                "a D3D12 render pass is one subpass today; the description \
                 names {} subpasses, and a recorded NextSubpass has no \
                 translation",
                desc.subpasses.len()
            )));
        }
        for (index, attachment) in desc.attachments.iter().enumerate() {
            if convert::dxgi_format(attachment.format).is_none() {
                return Err(RhiError::BackendError(format!(
                    "render pass attachment {} has no D3D12 form for {:?}",
                    index, attachment.format
                )));
            }
        }
        for subpass in &desc.subpasses {
            for reference in subpass
                .color_attachments
                .iter()
                .chain(subpass.input_attachments.iter())
                .chain(subpass.resolve_attachments.iter())
            {
                if reference.attachment as usize >= desc.attachments.len() {
                    return Err(RhiError::BackendError(format!(
                        "a subpass references attachment {}, which the pass \
                         does not declare",
                        reference.attachment
                    )));
                }
            }
            if let Some(depth) = &subpass.depth_stencil_attachment {
                if depth.attachment as usize >= desc.attachments.len() {
                    return Err(RhiError::BackendError(format!(
                        "a subpass references its depth attachment {}, which \
                         the pass does not declare",
                        depth.attachment
                    )));
                }
            }
        }
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .insert(handle, D3DObject::Pass(desc.clone()));
        let mut out = RenderPass::new(desc.clone());
        out.set_backend(GpuResource {
            handle,
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_framebuffer(&self, desc: &FramebufferDesc) -> RhiResult<Framebuffer> {
        let Some(pass_gpu) = desc.render_pass.backend() else {
            return Err(RhiError::BackendError(
                "the framebuffer's render pass has no GPU backing".into(),
            ));
        };
        let pass = match self.lookup(pass_gpu.handle, "render pass")? {
            D3DObject::Pass(entry) => entry,
            _ => Err(RhiError::InternalError(
                "a render pass handle lost its kind".into(),
            ))?,
        };
        if desc.attachments.len() != pass.attachments.len() {
            return Err(RhiError::BackendError(format!(
                "the framebuffer declares {} attachments and its render pass \
                 described {}",
                desc.attachments.len(),
                pass.attachments.len()
            )));
        }
        if desc.layers != 1 {
            return Err(RhiError::NotSupported(format!(
                "a D3D12 framebuffer renders one array layer today; the \
                 description names {}",
                desc.layers
            )));
        }
        if desc.width == 0 || desc.height == 0 {
            return Err(RhiError::BackendError(
                "a framebuffer of zero extent describes nothing to render \
                 into"
                    .into(),
            ));
        }
        // Slots claimed per attachment, rewound when any attachment fails —
        // a half-made framebuffer leaves the pool exactly as it found it.
        let mut claimed: Vec<(super::descriptor::DescriptorKind, usize)> = Vec::new();
        let build = |claimed: &mut Vec<(super::descriptor::DescriptorKind, usize)>| {
            let mut rtv_slots: Vec<usize> = Vec::new();
            let mut rtv_handles: Vec<u64> = Vec::new();
            let mut dsv_slot = None;
            let mut dsv_handle = None;
            for (index, attachment) in desc.attachments.iter().enumerate() {
                let Some(view_gpu) = attachment.texture_view.backend() else {
                    return Err(RhiError::BackendError(format!(
                        "framebuffer attachment {} has no GPU backing",
                        index
                    )));
                };
                let view = self.view_entry(view_gpu.handle)?;
                if pass.attachments[index].format != view.rhi_format() {
                    return Err(RhiError::BackendError(format!(
                        "framebuffer attachment {} reads {:?} while the render \
                         pass declared {:?}; the pass and its framebuffers \
                         agree on formats",
                        index,
                        view.rhi_format(),
                        pass.attachments[index].format
                    )));
                }
                let depth = convert::typeless_base(view.rhi_format()).is_some();
                if depth {
                    if dsv_slot.is_some() {
                        return Err(RhiError::BackendError(
                            "the framebuffer declares more than one depth \
                             attachment"
                                .into(),
                        ));
                    }
                    let slot = self.heaps.alloc(super::descriptor::DescriptorKind::Dsv, 1)?;
                    claimed.push((super::descriptor::DescriptorKind::Dsv, 1));
                    let dsv_desc = Self::framebuffer_dsv_desc(
                        &view,
                        attachment.mip_level,
                        attachment.layer,
                    )?;
                    unsafe {
                        self.device.0.CreateDepthStencilView(
                            view.resource(),
                            Some(&dsv_desc),
                            self.heaps.cpu(super::descriptor::DescriptorKind::Dsv, slot),
                        );
                    }
                    dsv_slot = Some(slot);
                    dsv_handle = Some(view.texture_handle());
                } else {
                    let slot = self.heaps.alloc(super::descriptor::DescriptorKind::Rtv, 1)?;
                    claimed.push((super::descriptor::DescriptorKind::Rtv, 1));
                    let rtv_desc = Self::framebuffer_rtv_desc(
                        &view,
                        attachment.mip_level,
                        attachment.layer,
                    )?;
                    unsafe {
                        self.device.0.CreateRenderTargetView(
                            view.resource(),
                            Some(&rtv_desc),
                            self.heaps.cpu(super::descriptor::DescriptorKind::Rtv, slot),
                        );
                    }
                    rtv_slots.push(slot);
                    rtv_handles.push(view.texture_handle());
                }
            }
            Ok(FramebufferEntry {
                rtv_slots,
                rtv_handles,
                dsv_slot,
                dsv_handle,
                width: desc.width,
                height: desc.height,
            })
        };
        let built = build(&mut claimed);
        let entry = match built {
            Ok(entry) => entry,
            Err(e) => {
                for (kind, count) in claimed {
                    self.heaps.rewind(kind, count);
                }
                return Err(e);
            }
        };
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .insert(handle, D3DObject::Framebuffer(entry));
        let mut out = Framebuffer::new(desc.clone());
        out.set_backend(GpuResource {
            handle,
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn create_graphics_pipeline(
        &self,
        desc: &crate::pipeline::graphics::GraphicsPipelineDesc,
        render_pass: &crate::command::pass::render::RenderPass,
        set_layouts: &[&crate::descriptor::DescriptorSetLayout],
    ) -> RhiResult<crate::pipeline::GraphicsPipeline> {
        // The pass names the formats the PSO's fixed-function stage agreed
        // to: its rendered targets and its depth target. The layouts name
        // the root tables. Wiring anything to whatever else is how a driver
        // emits an unreadable validation text; here each mismatch is a
        // sentence.
        let Some(pass_gpu) = render_pass.backend() else {
            return Err(RhiError::BackendError(
                "the graphics pipeline's render pass has no GPU backing".into(),
            ));
        };
        let pass: RenderPassDesc = match self.lookup(pass_gpu.handle, "render pass")? {
            D3DObject::Pass(entry) => entry,
            _ => Err(RhiError::InternalError(
                "a render pass handle lost its kind".into(),
            ))?,
        };
        for layout in set_layouts {
            if layout.backend().is_none() {
                return Err(RhiError::BackendError(
                    "one of the graphics pipeline's set layouts has no GPU \
                     backing"
                        .into(),
                ));
            }
        }
        let signature = self.build_root_signature(set_layouts)?;
        // Stages: one vertex shader, an optional pixel shader. Depth-only
        // pipelines (no pixel stage) are the D3D12 shape for shadow passes.
        let mut vs: Option<&crate::pipeline::graphics::PipelineShaderStage> = None;
        let mut ps: Option<&crate::pipeline::graphics::PipelineShaderStage> = None;
        for stage in &desc.shader_stages {
            if stage.stage.contains(crate::types::ShaderStage::VERTEX) {
                if vs.is_some() {
                    return Err(RhiError::PipelineCreationError(
                        "the pipeline declares two vertex stages; D3D12 draws \
                         through one"
                            .into(),
                    ));
                }
                vs = Some(stage);
            } else if stage.stage.contains(crate::types::ShaderStage::FRAGMENT) {
                if ps.is_some() {
                    return Err(RhiError::PipelineCreationError(
                        "the pipeline declares two fragment stages; D3D12 \
                         pixels through one"
                            .into(),
                    ));
                }
                ps = Some(stage);
            } else {
                return Err(RhiError::NotSupported(format!(
                    "D3D12 graphics pipelines assemble vertex and fragment \
                     stages here; the announced {:?} stage belongs to another \
                     family (tessellation/geometry stages and compute land in \
                     their own pipeline shapes, and ray tracing refuses for \
                     now)",
                    stage.stage
                )));
            }
        }
        let Some(vs) = vs else {
            return Err(RhiError::PipelineCreationError(
                "a D3D12 graphics pipeline needs a vertex shader".into(),
            ));
        };
        let vs_entry = self.shader_entry(
            vs.module
                .backend()
                .ok_or_else(|| {
                    RhiError::BackendError(
                        "the vertex shader module has no GPU backing".into(),
                    )
                })?
                .handle,
        )?;
        let ps_entry = match ps {
            Some(ps) => {
                Some(self.shader_entry(ps.module.backend().ok_or_else(|| {
                    RhiError::BackendError("the fragment shader module has no GPU backing".into())
                })?.handle)?)
            }
            None => None,
        };
        let vs_bytecode = D3D12_SHADER_BYTECODE {
            pShaderBytecode: vs_entry.code().as_ptr() as *const core::ffi::c_void,
            BytecodeLength: vs_entry.code().len(),
        };
        let ps_bytecode = match &ps_entry {
            Some(entry) => D3D12_SHADER_BYTECODE {
                pShaderBytecode: entry.code().as_ptr() as *const core::ffi::c_void,
                BytecodeLength: entry.code().len(),
            },
            None => D3D12_SHADER_BYTECODE {
                pShaderBytecode: core::ptr::null(),
                BytecodeLength: 0,
            },
        };
        // Vertex input: elements reflected straight out of the VS input
        // signature — an SV_VertexID-only shader carries none. The returned
        // reflector is bound right here: the elements' semantic names are
        // pointers into its string table, and the PSO creation below reads
        // them; the binding holds the reflector to this function's end.
        let (elements, vs_reflector) = match &desc.vertex_input_state {
            Some(input) => Self::vertex_input_elements(vs_entry.code(), input)?,
            None => (Vec::new(), None),
        };
        let _vs_reflector_alive_to_pso = vs_reflector;
        let input_layout = if elements.is_empty() {
            D3D12_INPUT_LAYOUT_DESC {
                pInputElementDescs: core::ptr::null(),
                NumElements: 0,
            }
        } else {
            D3D12_INPUT_LAYOUT_DESC {
                pInputElementDescs: elements.as_ptr(),
                NumElements: elements.len() as u32,
            }
        };
        let topology =
            convert::d3d_topology(desc.input_assembly_state.topology).ok_or_else(|| {
                RhiError::NotSupported(format!(
                    "D3D12 has no primitive topology for {:?}",
                    desc.input_assembly_state.topology
                ))
            })?;
        let rasterizer = match &desc.rasterizer_state {
            Some(state) => {
                if state.rasterizer_discard_enable {
                    return Err(RhiError::NotSupported(
                        "a rasterizer-discard pipeline has no D3D12 form; D3D12 \
                         throws whole pipelines away instead"
                            .into(),
                    ));
                }
                D3D12_RASTERIZER_DESC {
                    FillMode: convert::d3d_fill_mode(state.polygon_mode).ok_or_else(|| {
                        RhiError::PipelineCreationError(format!(
                            "D3D12 has no fill mode for {:?}",
                            state.polygon_mode
                        ))
                    })?,
                    CullMode: convert::d3d_cull_mode(state.cull_mode).ok_or_else(|| {
                        RhiError::PipelineCreationError(format!(
                            "D3D12 has no cull mode for {:?}",
                            state.cull_mode
                        ))
                    })?,
                    FrontCounterClockwise: matches!(
                        state.front_face,
                        crate::pipeline::graphics::FrontFace::CounterClockwise
                    )
                    .into(),
                    DepthBias: state.depth_bias_constant as i32,
                    DepthBiasClamp: state.depth_bias_clamp,
                    SlopeScaledDepthBias: state.depth_bias_slope,
                    // The RHI's clamp names the one D3D12 does not clip; the
                    // field is the enable of the clip itself.
                    DepthClipEnable: (!state.depth_clamp_enable).into(),
                    MultisampleEnable: (desc.multisample_state.sample_count.as_count() > 1)
                        .into(),
                    AntialiasedLineEnable: false.into(),
                    ForcedSampleCount: 0,
                    ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
                }
            }
            None => D3D12_RASTERIZER_DESC {
                FillMode: D3D12_FILL_MODE_SOLID,
                CullMode: D3D12_CULL_MODE_NONE,
                FrontCounterClockwise: true.into(),
                DepthBias: 0,
                DepthBiasClamp: 0.0,
                SlopeScaledDepthBias: 0.0,
                DepthClipEnable: true.into(),
                MultisampleEnable: (desc.multisample_state.sample_count.as_count() > 1).into(),
                AntialiasedLineEnable: false.into(),
                ForcedSampleCount: 0,
                ConservativeRaster: D3D12_CONSERVATIVE_RASTERIZATION_MODE_OFF,
            },
        };
        let depth_stencil = match &desc.depth_stencil_state {
            Some(state) => {
                if state.stencil_test_enable
                    && (state.front.compare_mask != state.back.compare_mask
                        || state.front.write_mask != state.back.write_mask)
                {
                    return Err(RhiError::NotSupported(
                        "D3D12 carries one stencil read/write mask for both \
                         faces; per-face masks refuse here"
                            .into(),
                    ));
                }
                D3D12_DEPTH_STENCIL_DESC {
                    DepthEnable: state.depth_test_enable.into(),
                    DepthWriteMask: if state.depth_write_enable {
                        D3D12_DEPTH_WRITE_MASK_ALL
                    } else {
                        D3D12_DEPTH_WRITE_MASK_ZERO
                    },
                    DepthFunc: convert::d3d_compare_func(state.depth_compare_op),
                    StencilEnable: state.stencil_test_enable.into(),
                    StencilReadMask: if state.stencil_test_enable {
                        state.front.compare_mask as u8
                    } else {
                        0
                    },
                    StencilWriteMask: if state.stencil_test_enable {
                        state.front.write_mask as u8
                    } else {
                        0
                    },
                    FrontFace: D3D12_DEPTH_STENCILOP_DESC {
                        StencilFailOp: convert::d3d_stencil_op(state.front.fail_op),
                        StencilDepthFailOp: convert::d3d_stencil_op(state.front.depth_fail_op),
                        StencilPassOp: convert::d3d_stencil_op(state.front.pass_op),
                        StencilFunc: convert::d3d_compare_func(state.front.compare_op),
                    },
                    BackFace: D3D12_DEPTH_STENCILOP_DESC {
                        StencilFailOp: convert::d3d_stencil_op(state.back.fail_op),
                        StencilDepthFailOp: convert::d3d_stencil_op(state.back.depth_fail_op),
                        StencilPassOp: convert::d3d_stencil_op(state.back.pass_op),
                        StencilFunc: convert::d3d_compare_func(state.back.compare_op),
                    },
                }
            }
            None => D3D12_DEPTH_STENCIL_DESC {
                DepthEnable: false.into(),
                DepthWriteMask: D3D12_DEPTH_WRITE_MASK_ZERO,
                DepthFunc: D3D12_COMPARISON_FUNC_ALWAYS,
                StencilEnable: false.into(),
                StencilReadMask: 0,
                StencilWriteMask: 0,
                FrontFace: D3D12_DEPTH_STENCILOP_DESC {
                    StencilFailOp: convert::d3d_stencil_op(
                        crate::pipeline::graphics::StencilOp::Keep,
                    ),
                    StencilDepthFailOp: convert::d3d_stencil_op(
                        crate::pipeline::graphics::StencilOp::Keep,
                    ),
                    StencilPassOp: convert::d3d_stencil_op(
                        crate::pipeline::graphics::StencilOp::Keep,
                    ),
                    StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
                },
                BackFace: D3D12_DEPTH_STENCILOP_DESC {
                    StencilFailOp: convert::d3d_stencil_op(
                        crate::pipeline::graphics::StencilOp::Keep,
                    ),
                    StencilDepthFailOp: convert::d3d_stencil_op(
                        crate::pipeline::graphics::StencilOp::Keep,
                    ),
                    StencilPassOp: convert::d3d_stencil_op(
                        crate::pipeline::graphics::StencilOp::Keep,
                    ),
                    StencilFunc: D3D12_COMPARISON_FUNC_ALWAYS,
                },
            },
        };
        // Blend: every target gets the one attachment's description. The
        // RHI's independent per-target blend is a spoken refusal — one shape
        // per descriptor keeps the matches to the declared weights honest.
        if desc
            .color_blend_state
            .as_ref()
            .is_some_and(|state| state.attachments.len() > 1)
        {
            return Err(RhiError::NotSupported(
                "independent per-target blend states have no reflection in \
                 this binding's shape; each pipeline blends by one attachment \
                 description"
                    .into(),
            ));
        }
        let (blend_target, blend_constants) = match &desc.color_blend_state {
            Some(state) => match state.attachments.first() {
                Some(attachment) => (
                    D3D12_RENDER_TARGET_BLEND_DESC {
                        BlendEnable: attachment.blend_enable.into(),
                        LogicOpEnable: state.logic_op_enable.into(),
                        SrcBlend: convert::d3d_blend_factor(attachment.src_color_blend_factor),
                        DestBlend: convert::d3d_blend_factor(attachment.dst_color_blend_factor),
                        BlendOp: convert::d3d_blend_op(attachment.color_blend_op),
                        SrcBlendAlpha: convert::d3d_blend_factor(attachment.src_alpha_blend_factor),
                        DestBlendAlpha: convert::d3d_blend_factor(attachment.dst_alpha_blend_factor),
                        BlendOpAlpha: convert::d3d_blend_op(attachment.alpha_blend_op),
                        LogicOp: if state.logic_op_enable {
                            convert::d3d_logic_op(state.logic_op)
                        } else {
                            D3D12_LOGIC_OP_NOOP
                        },
                        RenderTargetWriteMask: convert::d3d_color_write(attachment.color_write_mask)
                            .0 as u8,
                    },
                    state.blend_constants,
                ),
                None => (
                    D3D12_RENDER_TARGET_BLEND_DESC {
                        BlendEnable: false.into(),
                        LogicOpEnable: false.into(),
                        SrcBlend: D3D12_BLEND_ONE,
                        DestBlend: D3D12_BLEND_ZERO,
                        BlendOp: D3D12_BLEND_OP_ADD,
                        SrcBlendAlpha: D3D12_BLEND_ONE,
                        DestBlendAlpha: D3D12_BLEND_ZERO,
                        BlendOpAlpha: D3D12_BLEND_OP_ADD,
                        LogicOp: D3D12_LOGIC_OP_NOOP,
                        RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
                    },
                    state.blend_constants,
                ),
            },
            None => (
                D3D12_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: false.into(),
                    LogicOpEnable: false.into(),
                    SrcBlend: D3D12_BLEND_ONE,
                    DestBlend: D3D12_BLEND_ZERO,
                    BlendOp: D3D12_BLEND_OP_ADD,
                    SrcBlendAlpha: D3D12_BLEND_ONE,
                    DestBlendAlpha: D3D12_BLEND_ZERO,
                    BlendOpAlpha: D3D12_BLEND_OP_ADD,
                    LogicOp: D3D12_LOGIC_OP_NOOP,
                    RenderTargetWriteMask: D3D12_COLOR_WRITE_ENABLE_ALL.0 as u8,
                    },
                [0.0, 0.0, 0.0, 0.0],
            ),
        };
        let blend = D3D12_BLEND_DESC {
            AlphaToCoverageEnable: desc
                .multisample_state
                .alpha_to_coverage_enable
                .into(),
            IndependentBlendEnable: false.into(),
            RenderTarget: [blend_target; 8],
        };
        // The PSO agrees with the pass's formats: color targets in declared
        // order, the one depth target, and everything else an untranslated
        // mismatch.
        let mut rtv_formats = [DXGI_FORMAT_UNKNOWN; 8];
        let mut num_rtv = 0u32;
        let mut dsv_format = DXGI_FORMAT_UNKNOWN;
        for attachment in &pass.attachments {
            if convert::typeless_base(attachment.format).is_some() {
                if dsv_format != DXGI_FORMAT_UNKNOWN {
                    return Err(RhiError::BackendError(
                        "the render pass names more than one depth \
                         attachment; a D3D12 PSO depth-writes one"
                            .into(),
                    ));
                }
                dsv_format = convert::dxgi_format(attachment.format)
                    .ok_or_else(|| {
                        RhiError::BackendError(format!(
                            "render pass depth format {:?} has no D3D12 form",
                            attachment.format
                        ))
                    })?;
            } else {
                let format = convert::dxgi_format(attachment.format).ok_or_else(|| {
                    RhiError::BackendError(format!(
                        "render pass format {:?} has no D3D12 form",
                        attachment.format
                    ))
                })?;
                if num_rtv as usize >= rtv_formats.len() {
                    return Err(RhiError::BackendError(
                        "the render pass names more color attachments than \
                         D3D12's eight slots"
                            .into(),
                    ));
                }
                rtv_formats[num_rtv as usize] = format;
                num_rtv += 1;
            }
        }
        let pso_desc = D3D12_GRAPHICS_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::ManuallyDrop::new(Some(signature.0.clone())),
            VS: vs_bytecode,
            PS: ps_bytecode,
            DS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: core::ptr::null(),
                BytecodeLength: 0,
            },
            HS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: core::ptr::null(),
                BytecodeLength: 0,
            },
            GS: D3D12_SHADER_BYTECODE {
                pShaderBytecode: core::ptr::null(),
                BytecodeLength: 0,
            },
            StreamOutput: D3D12_STREAM_OUTPUT_DESC {
                pSODeclaration: core::ptr::null(),
                NumEntries: 0,
                pBufferStrides: core::ptr::null(),
                NumStrides: 0,
                RasterizedStream: 0,
            },
            BlendState: blend,
            SampleMask: u32::MAX,
            RasterizerState: rasterizer,
            DepthStencilState: depth_stencil,
            InputLayout: input_layout,
            IBStripCutValue: D3D12_INDEX_BUFFER_STRIP_CUT_VALUE_DISABLED,
            PrimitiveTopologyType: convert::d3d_topology_type(
                desc.input_assembly_state.topology,
            )
            .ok_or_else(|| {
                RhiError::PipelineCreationError(format!(
                    "D3D12 has no topology family for {:?}",
                    desc.input_assembly_state.topology
                ))
            })?,
            NumRenderTargets: num_rtv,
            RTVFormats: rtv_formats,
            DSVFormat: dsv_format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: desc.multisample_state.sample_count.as_count(),
                Quality: 0,
            },
            NodeMask: 0,
            CachedPSO: D3D12_CACHED_PIPELINE_STATE {
                pCachedBlob: core::ptr::null(),
                CachedBlobSizeInBytes: 0,
            },
            Flags: D3D12_PIPELINE_STATE_FLAG_NONE,
        };
        let pso: ID3D12PipelineState = unsafe {
            self.device
                .0
                .CreateGraphicsPipelineState::<ID3D12PipelineState>(&pso_desc)
        }
        .map_err(|e| {
            RhiError::PipelineCreationError(format!("CreateGraphicsPipelineState: {e}"))
        })?;
        // Strides per input slot fill the recorded BindVertexBuffers's
        // buffer views.
        let mut vertex_strides = Vec::new();
        if let Some(input) = &desc.vertex_input_state {
            for binding in &input.bindings {
                vertex_strides.push((binding.binding, binding.stride));
            }
        }
        let entry = GraphicsPipelineEntry {
            root_signature: signature,
            pso: DxCom(pso),
            topology,
            vertex_strides,
            num_sets: set_layouts.len(),
            viewports: desc.viewport_state.viewports.clone(),
            scissors: desc.viewport_state.scissors.clone(),
            blend_factor: blend_constants,
        };
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .insert(handle, D3DObject::GraphicsPipeline(entry));
        let mut out = crate::pipeline::GraphicsPipeline::new(desc.clone());
        out.set_backend(GpuResource {
            handle,
            memory: 0,
            mapped: 0,
        });
        Ok(out)
    }

    fn draw_framebuffer_indexed(
        &self,
        framebuffer: &Framebuffer,
        pipeline: &crate::pipeline::GraphicsPipeline,
        sets: &[&crate::descriptor::DescriptorSet],
        vertex_buffer: Option<(&Buffer, u32)>,
        index_buffer: Option<(&Buffer, u32)>,
        index_type: IndexType,
        vertex_count: u32,
        first_vertex: u32,
        index_count: u32,
        first_index: u32,
        vertex_offset: i32,
        clear_values: &[ClearValue],
    ) -> RhiResult<()> {
        let desc = framebuffer.desc();
        let mut commands = vec![Command::BeginRenderPass(RenderPassBeginInfo {
            render_pass: desc.render_pass.clone(),
            framebuffer: framebuffer.clone(),
            render_area: Rect2D {
                offset: Offset2D { x: 0, y: 0 },
                extent: Extent2D {
                    width: desc.width,
                    height: desc.height,
                },
            },
            clear_values: clear_values.to_vec(),
        })];
        commands.push(Command::BindGraphicsPipeline(pipeline.clone()));
        if !sets.is_empty() {
            commands.push(Command::BindDescriptorSets {
                pipeline: pipeline.clone(),
                first_set: 0,
                sets: sets.iter().map(|set| (*set).clone()).collect(),
            });
        }
        if let Some((buffer, slot)) = vertex_buffer {
            commands.push(Command::BindVertexBuffers {
                first_binding: slot,
                buffers: vec![(buffer.clone(), 0)],
            });
        }
        if let Some((buffer, _slot)) = index_buffer {
            commands.push(Command::BindIndexBuffer {
                buffer: buffer.clone(),
                offset: 0,
                index_type,
            });
        }
        if index_buffer.is_some() {
            commands.push(Command::DrawIndexed {
                index_count,
                instance_count: 1,
                first_index,
                vertex_offset,
                first_instance: 0,
            });
        } else {
            commands.push(Command::Draw {
                vertex_count,
                instance_count: 1,
                first_vertex,
                first_instance: 0,
            });
        }
        commands.push(Command::EndRenderPass);
        self.submit_commands(&commands)
    }
}

// At the end of the file on purpose: clippy's `items_after_test_module` fires when
// anything follows a `mod tests`, and a warning here is a warning every future
// contributor learns to ignore.

#[cfg(test)]
mod release_surface {
    //! Bug №731: the raw `(kind, handle)` release surface.
    //!
    //! `backend()` is private to this crate by design — `Device::release_many`
    //! takes resource references instead — so the kind-matching half of the
    //! release contract cannot be reached from the client tests without
    //! exposing handles. It is checked here, where the handle table itself
    //! decides what a release is.

    use crate::backend::common::ReleaseKind;
    use crate::backend::d3d12::D3D12Backend;
    use crate::backend::Backend;
    use crate::config::RhiConfig;
    use crate::core::device::{Device, DeviceDesc};
    use crate::types::{BufferUsage, Format, TextureUsage};

    fn d3d12_test_device() -> Device {
        let b = D3D12Backend::new(&RhiConfig::default()).unwrap();
        let devices = b.enumerate_physical_devices().unwrap();
        b.create_device(&devices[0], &DeviceDesc::default()).unwrap()
    }

    /// A release must remove the entry whose kind matches the handle it was
    /// given, stay a no-op for a kind mismatch, an unknown handle, and a
    /// repeat, and clear the texture through the public red-texture path.
    #[test]
    fn release_removes_only_the_matching_kind() {
        let device = d3d12_test_device();
        let backend = device.backend_resource().unwrap();
        let before = device.tracked_object_count();

        // The host-visible buffer lands in the UPLOAD heap with a persistent
        // mapping, so its release also exercises the unmap-before-drop path.
        let buffer = device.create_buffer(256, BufferUsage::UNIFORM, true).unwrap();
        let texture = device
            .create_texture(8, 8, 1, Format::RGBA8_UNORM, TextureUsage::TRANSFER_SRC, 1)
            .unwrap();
        assert_eq!(
            device.tracked_object_count(),
            before + 2,
            "the created buffer and texture must be tracked"
        );

        // A kind that does not own the handle must not release it: the
        // handle vocabulary and the handle table must agree, or a stray call
        // would destroy an object of the wrong kind.
        backend.release(
            ReleaseKind::Image,
            buffer.backend().expect("the buffer has a GPU backing").handle,
        );
        assert_eq!(
            device.tracked_object_count(),
            before + 2,
            "an Image release over a buffer handle must be a no-op"
        );
        backend.release(
            ReleaseKind::Buffer,
            texture.backend().expect("the texture has a GPU backing").handle,
        );
        assert_eq!(
            device.tracked_object_count(),
            before + 2,
            "a Buffer release over a texture handle must be a no-op"
        );

        // Matching kinds release exactly one entry each, and a repeated
        // release must not re-insert or panic.
        backend.release(
            ReleaseKind::Buffer,
            buffer.backend().expect("the buffer has a GPU backing").handle,
        );
        assert_eq!(
            device.tracked_object_count(),
            before + 1,
            "the released buffer must leave the handle table"
        );
        backend.release(
            ReleaseKind::Buffer,
            buffer.backend().expect("the buffer has a GPU backing").handle,
        );
        assert_eq!(
            device.tracked_object_count(),
            before + 1,
            "a repeated release must stay harmless"
        );

        device.release_texture(&texture);
        assert_eq!(
            device.tracked_object_count(),
            before,
            "the released texture must leave the handle table"
        );
    }

    /// The summary must keep reporting the truth after releases: the counting
    /// side reads the same handle table the release side edits.
    #[test]
    fn release_texture_summary_reports_the_removal() {
        let device = d3d12_test_device();
        let texture = device
            .create_texture(8, 8, 1, Format::RGBA8_UNORM, TextureUsage::TRANSFER_SRC, 1)
            .unwrap();
        assert_eq!(tracked_textures_of(&device), 1);
        device.release_texture(&texture);
        assert_eq!(tracked_textures_of(&device), 0);
    }

    /// Read the tracked-texture part of the summary without parsing the
    /// string: the release side must edit the entry the summary counts, and
    /// the public count is the closest observable to it.
    fn tracked_textures_of(device: &Device) -> usize {
        let summary = device.tracked_object_summary();
        match summary.rsplit_once("textures: ") {
            Some((_, tail)) => tail
                .split(',')
                .next()
                .and_then(|head| head.trim().parse::<usize>().ok())
                .unwrap_or(usize::MAX),
            None => usize::MAX,
        }
    }
}

// At the end of the file on purpose: clippy's `items_after_test_module` fires when
// anything follows a `mod tests`, and a warning here is a warning every future
// contributor learns to ignore.

#[cfg(test)]
mod texture_desc_contract {
    //! Bug №737: `texture_desc` must build depth resources from the typeless
    //! base `convert.rs` promises. convert.rs carries the pair
    //! `typeless_base`/`depth_srv_format` with its own unit test; the creator
    //! ignoring the base would leave a typed depth resource whose vigilance
    //! — DSV/SRV views of M3 — never lands on it.

    use super::super::convert;
    use super::D3D12Device;
    use crate::resource::TextureDesc;
    use crate::types::{Format, SampleCount, SharingMode, TextureDimensions, TextureUsage};

    #[test]
    fn a_depth_texture_resource_desc_is_typeless() {
        for format in [
            Format::D16_UNORM,
            Format::D24_UNORM,
            Format::D24_UNORM_S8_UINT,
            Format::D32_SFLOAT,
            Format::D32_SFLOAT_S8_UINT,
        ] {
            let desc = TextureDesc {
                width: 8,
                height: 8,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                format,
                usage: TextureUsage::DEPTH_STENCIL_ATTACHMENT,
                sample_count: SampleCount::X1,
                dimensions: TextureDimensions::D2,
                sharing_mode: SharingMode::Exclusive,
                queue_family_indices: Vec::new(),
            };
            let resource = D3D12Device::texture_desc(&desc)
                .expect("the depth texture description must build")
                .Format;
            let want = convert::typeless_base(format)
                .expect("every depth format of the table has a typeless base");
            assert_eq!(
                resource, want,
                "{format:?} must be created as typeless {want:?}, not as {resource:?}"
            );
        }

        // Non-depth formats keep their typed format: the typeless base
        // exists only for the depth family.
        for format in [
            Format::RGBA8_UNORM,
            Format::B8G8R8A8_UNORM,
            Format::R32_SFLOAT,
            Format::RGBA32_SFLOAT,
        ] {
            let desc = TextureDesc {
                width: 8,
                height: 8,
                depth: 1,
                mip_levels: 1,
                array_layers: 1,
                format,
                usage: TextureUsage::TRANSFER_SRC,
                sample_count: SampleCount::X1,
                dimensions: TextureDimensions::D2,
                sharing_mode: SharingMode::Exclusive,
                queue_family_indices: Vec::new(),
            };
            let resource = D3D12Device::texture_desc(&desc)
                .expect("the color texture description must build")
                .Format;
            let want = convert::dxgi_format(format)
                .expect("every color format of the table has a DX12 format");
            assert_eq!(
                resource, want,
                "{format:?} is not a depth format: it must keep {want:?}, not {resource:?}"
            );
        }
    }
}

#[cfg(test)]
mod vertex_signature_contract {
    //! BUGS №758: the D3D12 input-signature reflection used to read a
    //! `D3DGetInputSignatureBlob` blob's raw bytes as a descriptor array —
    //! but the blob is a finished DXBC container (a 36-byte header and the
    //! ISGN chunk with its string table), so `Register` read ASCII bytes
    //! (`2104404263`, `1332475536`, …) and the shader-map named garbage
    //! locations (the live symptom: a real draw pipeline was refused, then a
    //! descriptor-table pipeline panicked the same way). The D3D11 sibling
    //! got the mirrored contract at №759; the two backends keep one shape on
    //! purpose.
    //!
    //! These contracts pin the honest reflection: true locations, no
    //! system-value inputs, and refusals that name the shader's location.
    use super::*;
    use crate::backend::d3d11::compile::compile_hlsl;

    const TWO_INPUT_VS: &[u8] = b"struct VSIn { float3 pos : TEXCOORD0; float4 col : TEXCOORD1; };\n\
         struct VOut { float4 pos : SV_Position; float4 col : COLOR0; };\n\
         VOut main(VSIn input) { VOut o = (VOut)0; o.pos = float4(input.pos, 1.0); \
         o.col = input.col; return o; }\n";

    const VERTEX_ID_VS: &[u8] =
        b"struct VOut { float4 pos : SV_Position; };\n\
         VOut main(uint id : SV_VertexID) { \
         VOut o = (VOut)0; float2 xy = float2(id == 1 ? 3.0 : -1.0, id == 2 ? 3.0 : -1.0); \
         o.pos = float4(xy, 0.0, 1.0); return o; }\n";

    fn two_location_input() -> crate::pipeline::graphics::VertexInputState {
        crate::pipeline::graphics::VertexInputState::from_formats(&[
            (0, crate::types::Format::R32G32B32_SFLOAT),
            (1, crate::types::Format::RGBA32_SFLOAT),
        ])
    }

    #[test]
    fn a_spanning_signature_reflects_true_locations() {
        let code = match compile_hlsl(TWO_INPUT_VS, "main", "vs_5_0") {
            Ok(code) => code,
            Err(e) => panic!("compile: {e:?}"),
        };
        let (elements, reflector) =
            D3D12Device::vertex_input_elements(&code, &two_location_input()).unwrap();
        assert!(
            reflector.is_some(),
            "the elements borrow the reflection object's string table: the \
             reflector must come back so the pipeline creation keeps it \
             alive past the PSO build"
        );
        assert_eq!(
            elements.len(),
            2,
            "a two-input vertex shader reflects two elements, saw {elements:?}"
        );
        // TEXCOORD0 at offset 0 and TEXCOORD1 at offset 12 — the declared
        // attribute offsets of the pipeline's first binding.
        assert_eq!(elements[0].AlignedByteOffset, 0);
        assert_eq!(elements[1].AlignedByteOffset, 12);
        assert_eq!(elements[0].InputSlot, 0);
        assert_eq!(elements[1].InputSlot, 0);
    }

    #[test]
    fn a_vertex_id_only_shader_reflects_no_inputs() {
        let code = match compile_hlsl(VERTEX_ID_VS, "main", "vs_5_0") {
            Ok(code) => code,
            Err(e) => panic!("compile: {e:?}"),
        };
        let (elements, reflector) =
            D3D12Device::vertex_input_elements(&code, &two_location_input()).unwrap();
        assert!(
            elements.is_empty(),
            "SV_VertexID-only shaders carry no vertex-buffer elements: the \
             system value occupies register 0 and must not swallow a real \
             attribute's location; saw {elements:?}"
        );
        assert!(reflector.is_some(), "empty still reflects through an object");
    }

    #[test]
    fn a_missing_attribute_names_the_true_location() {
        let code = match compile_hlsl(TWO_INPUT_VS, "main", "vs_5_0") {
            Ok(code) => code,
            Err(e) => panic!("compile: {e:?}"),
        };
        let result = D3D12Device::vertex_input_elements(
            &code,
            &crate::pipeline::graphics::VertexInputState::default(),
        );
        match result {
            Err(RhiError::PipelineCreationError(text)) => assert!(
                text.contains("location 0"),
                "the reflection must name the shader's first input location; \
                 got {text:?} — garbage register words are the old blob-cast \
                 bug (№758) back"
            ),
            other => panic!("a missing attribute must be a refusal, saw {other:?}"),
        }
    }
}

#[cfg(test)]
mod aborted_submit_contract {
    //! BUGS №760 contracts: an aborted `record` leaves the command list in
    //! the recording state, and the next call's allocator reset fails with
    //! 0x80004005 — every later refusal afterwards surfaces as that garbage
    //! allocator error instead of the honest one. The cleanup closes the
    //! discarded list and clears the allocator's pending state, and the
    //! contracts here prove the pair survives an aborted record on a real
    //! device.
    use super::*;
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};
    use windows::Win32::Graphics::Direct3D::{D3D_FEATURE_LEVEL_12_0};

    fn direct_trio() -> (ID3D12Device, ID3D12CommandAllocator, ID3D12GraphicsCommandList) {
        let mut device: Option<ID3D12Device> = None;
        unsafe {
            D3D12CreateDevice::<_, ID3D12Device>(None, D3D_FEATURE_LEVEL_12_0, &mut device)
        }
        .expect("the contract device must come up on feature level 12.0");
        let device = device.expect("D3D12CreateDevice returned no device");
        let allocator: ID3D12CommandAllocator = unsafe {
            device.CreateCommandAllocator::<ID3D12CommandAllocator>(
                D3D12_COMMAND_LIST_TYPE_DIRECT,
            )
        }
        .expect("the contract allocator must come up");
        let list: ID3D12GraphicsCommandList = unsafe {
            device.CreateCommandList::<_, _, ID3D12GraphicsCommandList>(
                0,
                D3D12_COMMAND_LIST_TYPE_DIRECT,
                &allocator,
                None,
            )
        }
        .expect("the contract list must come up");
        (device, allocator, list)
    }

    #[test]
    fn an_aborted_list_must_not_poison_the_next_submit() {
        let (device, allocator, list) = direct_trio();
        let _ = device;
        // A freshly created list sits in the recording state, the same state
        // `submit_sequential` leaves after a refused `record`.
        D3D12Device::discard_aborted_list(&list, &allocator);
        unsafe { allocator.Reset() }.expect(
            "the scavenged iterator must leave the allocator reusable: the \
             0x80004005 of a poisoned allocator is the mask that turns every \
             later refusal into garbage (BUGS №760)",
        );
        unsafe { list.Reset(&allocator, None) }.expect(
            "the next submit's command list reset must succeed after an \
             aborted record was cleaned up",
        );
    }
}

#[cfg(test)]
mod cbv_alignment_contract {
    //! BUGS №761's rule: D3D12 requires a constant buffer view's
    //! `SizeInBytes` to be a multiple of 256. A sixteen-byte cbuffer range
    //! written raw faulted the device — the removal surfaced on the next
    //! pipeline creation as 0x887A0005 in the descriptor-table pixel test.
    //! The rounded view covers the padding, the shader reads the same first
    //! bytes.
    use super::*;

    #[test]
    fn the_view_size_rounds_declared_bytes_up_to_the_alignment() {
        assert_eq!(
            D3D12_CONSTANT_BUFFER_DATA_PLACEMENT_ALIGNMENT, 256,
            "the D3D12 constant buffer alignment this rule adjusts to is 256"
        );
        assert_eq!(
            D3D12Device::cbv_view_size(16),
            256,
            "a 16-byte cbuffer range must write a 256-byte view — the raw 16 \
             is the fault D3D12 names (BUGS №761)"
        );
        assert_eq!(
            D3D12Device::cbv_view_size(255),
            256,
            "one byte under the alignment rounds up to a full window"
        );
        assert_eq!(
            D3D12Device::cbv_view_size(256),
            256,
            "an already aligned range stays itself"
        );
        assert_eq!(
            D3D12Device::cbv_view_size(257),
            512,
            "one byte over the alignment grows the window by one more"
        );
    }
}
