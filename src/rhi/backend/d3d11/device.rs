//! Direct3D 11 Device Resource
//!
//! Implements `BackendResource` over a real `ID3D11Device` plus its immediate
//! context. Resources are tracked in one handle table keyed by `u64`, mirror
//! of the `GpuResource` handles the RHI types carry.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::common::{BackendResource, ReleaseKind};
use crate::command::commands::Command;
use crate::command::pass::render::{
    Framebuffer, FramebufferDesc, RenderPass, RenderPassBeginInfo, RenderPassDesc,
};
use crate::descriptor::layout::{DescriptorSetLayout, DescriptorSetLayoutDesc, DescriptorType};
use crate::descriptor::{DescriptorInfo, DescriptorSet, DescriptorWrite};
use crate::error::{RhiError, RhiResult};
use crate::pipeline::compute::{ComputePipeline, ComputePipelineDesc};
use crate::pipeline::graphics::{
    ColorBlendState, DepthStencilState, FrontFace, GraphicsPipeline, GraphicsPipelineDesc,
    RasterizerState, StencilOpState, VertexInputRate, VertexInputState,
};
use crate::resource::{
    Buffer, BufferDesc, GpuResource, Sampler, SamplerDesc, Texture, TextureDesc,
    TextureView, TextureViewDesc,
};
use crate::TextureDimensions;
use crate::shader::module::{ShaderFormat, ShaderModule, ShaderModuleDesc};
use crate::types::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D::D3D_PRIMITIVE_TOPOLOGY;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_DEPTH_STENCIL, D3D11_BIND_INDEX_BUFFER,
    D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_UNORDERED_ACCESS,
    D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD,
    D3D11_BLEND_ZERO, D3D11_BOX, D3D11_BUFFER_DESC, D3D11_BUFFER_UAV,
    D3D11_BUFFER_UAV_FLAG_RAW, D3D11_BUFFEREX_SRV, D3D11_BUFFEREX_SRV_FLAG_RAW,
    D3D11_COLOR_WRITE_ENABLE_ALL,
    D3D11_CPU_ACCESS_READ,
    D3D11_CPU_ACCESS_WRITE,
    D3D11_DEPTH_STENCILOP_DESC, D3D11_DEPTH_WRITE_MASK_ALL,
    D3D11_DEPTH_WRITE_MASK_ZERO, D3D11_DEPTH_STENCIL_VIEW_DESC, D3D11_DEPTH_STENCIL_DESC,
    D3D11_DSV_DIMENSION_TEXTURE2D, D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS,
    D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS, D3D11_INPUT_ELEMENT_DESC,
    D3D11_INPUT_PER_INSTANCE_DATA, D3D11_INPUT_PER_VERTEX_DATA,     D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_MAP_WRITE, D3D11_MAP_WRITE_DISCARD,
    D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_VIEW_DESC, D3D11_RTV_DIMENSION_TEXTURE2D,
    D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC, D3D11_SHADER_RESOURCE_VIEW_DESC,
    D3D11_SIGNATURE_PARAMETER_DESC, D3D11_TEX2D_DSV,
    D3D11_TEX2D_RTV, D3D11_TEX2D_SRV, D3D11_TEX2D_UAV, D3D11_TEX3D_SRV, D3D11_TEXTURE2D_DESC,
    D3D11_TEXTURE3D_DESC, D3D11_UNORDERED_ACCESS_VIEW_DESC, D3D11_UAV_DIMENSION_BUFFER,
    D3D11_UAV_DIMENSION_TEXTURE2D, D3D11_USAGE, D3D11_USAGE_DYNAMIC, D3D11_USAGE_DEFAULT,
    D3D11_USAGE_STAGING, D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS,
    ID3D11BlendState, ID3D11Buffer, ID3D11ComputeShader,
    ID3D11DepthStencilState, ID3D11DepthStencilView, ID3D11Device, ID3D11DeviceContext,
    ID3D11InputLayout, ID3D11Multithread, ID3D11PixelShader, ID3D11RasterizerState,
    ID3D11RenderTargetView, ID3D11Resource, ID3D11SamplerState, ID3D11ShaderResourceView,
    ID3D11Texture2D, ID3D11Texture3D, ID3D11UnorderedAccessView, ID3D11VertexShader,
};
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL;
use windows::Win32::Graphics::Direct3D::Fxc::D3DGetInputSignatureBlob;
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_FORMAT, DXGI_FORMAT_R32_TYPELESS, DXGI_SAMPLE_DESC,
};

use super::convert::{self, dxgi_format};

/// A COM object the backend keeps between calls.
///
/// The `windows` crate deliberately does not mark its COM wrappers `Send`/`
/// `Sync`: the runtime cannot know that every object is thread-safe. The
/// device used here is created with `ID3D11Multithread::SetMultithreadProtected`
/// turned on, and every object the backend touches lives under that device or
/// its immediate context, so shared use stays inside the internal-guard model
/// D3D11 defines for device-created objects. The wrapper is the boundary
/// where that obligation is taken over.
pub(crate) struct DxCom<T>(pub T);
unsafe impl<T> Send for DxCom<T> {}
unsafe impl<T> Sync for DxCom<T> {}
impl<T: Interface> Clone for DxCom<T> {
    fn clone(&self) -> Self {
        DxCom(self.0.clone())
    }
}

/// One managed buffer.
#[derive(Clone)]
pub(crate) struct BufferEntry {
    pub buffer: DxCom<ID3D11Buffer>,
    pub size: u64,
    pub usage: BufferUsage,
    pub host_visible: bool,
    /// Raw-buffer views for storage use; created for a storage buffers at
    /// buffer creation time because both views are trivial then.
    pub srv: Option<DxCom<ID3D11ShaderResourceView>>,
    pub uav: Option<DxCom<ID3D11UnorderedAccessView>>,
}

/// One managed image resource plus its RHI description.
pub(crate) struct TextureEntry {
    pub resource: DxCom<ID3D11Resource>,
    pub desc: TextureDesc,
    /// The view roles the resource's usage bits allow. Lives here, not under
    /// its own handle: the RHI texture view reuses the texture's handle, so a
    /// separate tracked entry would be unreachable bookkeeping, and every
    /// lookup that went through it reported a lost kind instead.
    pub roles: ViewEntry,
    /// Staging copy for `download_texture`, made at first download.
    pub download_staging: Option<DxCom<ID3D11Resource>>,
}

/// All view roles one RHI texture view can serve, created from the source
/// texture's usage bits.
pub(crate) struct ViewEntry {
    pub srv: Option<DxCom<ID3D11ShaderResourceView>>,
    pub rtv: Option<DxCom<ID3D11RenderTargetView>>,
    pub uav: Option<DxCom<ID3D11UnorderedAccessView>>,
    pub dsv: Option<DxCom<ID3D11DepthStencilView>>,
}

#[derive(Clone)]
pub(crate) struct GraphicsPipelineEntry {
    pub vs: Option<DxCom<ID3D11VertexShader>>,
    pub ps: Option<DxCom<ID3D11PixelShader>>,
    pub input_layout: Option<DxCom<ID3D11InputLayout>>,
    pub topology: Option<D3D_PRIMITIVE_TOPOLOGY>,
    pub rasterizer: Option<DxCom<ID3D11RasterizerState>>,
    pub blend: Option<DxCom<ID3D11BlendState>>,
    pub blend_constants: [f32; 4],
    pub depth_stencil: Option<DxCom<ID3D11DepthStencilState>>,
    /// Stride per D3D slot, so `BindVertexBuffers` can fill the call.
    pub vertex_strides: HashMap<u32, u32>,
    pub set_layouts: Vec<u64>,
}

pub(crate) struct ComputePipelineEntry {
    pub cs: DxCom<ID3D11ComputeShader>,
}

pub(crate) struct ShaderEntry {
    pub format: ShaderFormat,
    pub code: Vec<u8>,
}

pub(crate) struct FramebufferEntry {
    pub rtvs: Vec<DxCom<ID3D11RenderTargetView>>,
    pub dsv: Option<DxCom<ID3D11DepthStencilView>>,
    pub width: u32,
    pub height: u32,
}

/// Everything a bound descriptor slot needs at bind time.
#[derive(Clone)]
pub(crate) enum WriteKind {
    /// A constant buffer, bound whole. D3D11 binds constant buffers without
    /// offsets, so a write carrying a non-zero offset is refused.
    Constant { buffer: DxCom<ID3D11Buffer> },
    Texture { srv: DxCom<ID3D11ShaderResourceView> },
    /// A storage binding. Compute gets the UAV when there is one; graphics
    /// stages read only, so they always get the SRV.
    Storage {
        uav: Option<DxCom<ID3D11UnorderedAccessView>>,
        srv: Option<DxCom<ID3D11ShaderResourceView>>,
    },
    Sampler { sampler: DxCom<ID3D11SamplerState> },
    CombinedTextureSampler {
        srv: DxCom<ID3D11ShaderResourceView>,
        sampler: DxCom<ID3D11SamplerState>,
    },
}

#[derive(Clone)]
pub(crate) struct SetWrite {
    pub binding: u32,
    pub base_element: u32,
    pub ty: DescriptorType,
    pub stages: ShaderStage,
    pub kind: WriteKind,
}

pub(crate) enum D3DObject {
    Buffer(BufferEntry),
    Texture(TextureEntry),
    View(ViewEntry),
    Sampler(DxCom<ID3D11SamplerState>),
    Shader(ShaderEntry),
    GraphicsPipeline(GraphicsPipelineEntry),
    ComputePipeline(ComputePipelineEntry),
    /// A render pass object is only its description: D3D11 has no render pass
    /// concept, so the emulation keeps the description.
    Pass(RenderPassDesc),
    Framebuffer(FramebufferEntry),
    /// Tracked for `ReleaseKind::DescriptorSetLayout` bookkeeping only.
    Layout,
}

#[derive(Default)]
pub(crate) struct HandleTable {
    pub map: HashMap<u64, D3DObject>,
}

/// Per-stage slot counts, from the feature level and the confirmed D3D10.x
/// hardware option (compute with raw and structured buffers).
#[derive(Clone, Copy)]
pub(crate) struct StageCaps {
    pub constant_buffers: u32,
    pub samplers: u32,
    pub srvs: u32,
    pub uavs: u32,
}

/// The D3D11 device resource handed to `Device::from_parts`.
pub struct D3D11Device {
    #[allow(dead_code)]
    pub(crate) device: DxCom<ID3D11Device>,
    pub(crate) context: DxCom<ID3D11DeviceContext>,
    physical_name: String,
    feature_level: u32,
    handles: Mutex<HandleTable>,
    next_id: AtomicU64,
    sets: Mutex<HashMap<u64, Vec<SetWrite>>>,
    caps: StageCaps,
    /// "Compute Shaders Plus Raw And Structured Buffers Via Shader 4.x": the
    /// gate for storage UAVs at FL 11_0/11_1.
    pub(crate) uav_support: bool,
}

impl std::fmt::Debug for D3D11Device {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("D3D11Device")
            .field("physical", &self.physical_name)
            .field("feature_level", &format_args!("0x{:X}", self.feature_level))
            .finish()
    }
}

impl D3D11Device {
    /// Build the device resource around objects the instance created, and set
    /// device-level state: multithread protection on the underlying device,
    /// and the slot caps for the feature level.
    pub(crate) fn new(
        device: DxCom<ID3D11Device>,
        context: DxCom<ID3D11DeviceContext>,
        physical: &crate::core::PhysicalDevice,
        feature_level: D3D_FEATURE_LEVEL,
    ) -> Self {
        unsafe {
            let mt: ID3D11Multithread = device
                .0
                .cast()
                .expect("the device context1 / multithread cast cannot fail on a real device");
            let _ = mt.SetMultithreadProtected(true);
        }
        let level = feature_level.0 as u32;
        let (cbs, samplers, uavs) = if level >= 45312 {
            (16, 32, 64)
        } else {
            (14, 16, 8)
        };
        let mut hw = D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS {
            ComputeShaders_Plus_RawAndStructuredBuffers_Via_Shader_4_x: false.into(),
        };
        let uav_support = unsafe {
            device
                .0
                .CheckFeatureSupport(
                    D3D11_FEATURE_D3D10_X_HARDWARE_OPTIONS,
                    &mut hw as *mut _ as *mut core::ffi::c_void,
                    std::mem::size_of::<D3D11_FEATURE_DATA_D3D10_X_HARDWARE_OPTIONS>() as u32,
                )
                .is_ok()
        };
        Self {
            device,
            context,
            physical_name: physical.name.clone(),
            feature_level: level,
            handles: Mutex::new(HandleTable::default()),
            next_id: AtomicU64::new(1),
            sets: Mutex::new(HashMap::new()),
            caps: StageCaps { constant_buffers: cbs, samplers, srvs: 128, uavs },
            uav_support,
        }
    }

    pub(crate) fn next_handle(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn context(&self) -> &ID3D11DeviceContext {
        &self.context.0
    }

    pub(crate) fn device(&self) -> &ID3D11Device {
        &self.device.0
    }

    pub(crate) fn caps(&self) -> StageCaps {
        self.caps
    }

    /// Take an object by handle out of tracking.
    pub(crate) fn take_object(&self, handle: u64, what: &str) -> RhiResult<D3DObject> {
        self.handles
            .lock()
            .unwrap()
            .map
            .remove(&handle)
            .ok_or_else(|| {
                RhiError::InternalError(format!(
                    "{what} handle {handle} is not tracked by the D3D11 backend"
                ))
            })
    }

    /// Hold the handle-table lock across a closure over one object.
    ///
    /// No command path inside the closure re-enters the table, so no practical
    /// deadlock arises; this keeps the object alive through the whole call
    /// without cloning COM references.
    pub(crate) fn with_object<R>(
        &self,
        handle: u64,
        what: &str,
        f: impl FnOnce(&D3DObject) -> RhiResult<R>,
    ) -> RhiResult<R> {
        let table = self.handles.lock().unwrap();
        let object = table
            .map
            .get(&handle)
            .ok_or_else(|| {
                RhiError::InternalError(format!(
                    "{what} handle {handle} is not tracked by the D3D11 backend"
                ))
            })?;
        f(object)
    }

    /// The bytecode of a shader module, with the D3D11 format contract
    /// applied: only `ShaderFormat::Dxil` is accepted, which the RHI contract
    /// reads as "a finished DXBC/DXIL container" - what runtime `D3DCompile`
    /// (Fxc) or dxc produce.
    pub(crate) fn shader_bytes<'a>(module: &'a ShaderModule, what: &str) -> RhiResult<&'a [u8]> {
        let desc = module.desc();
        if desc.format != ShaderFormat::Dxil {
            return Err(RhiError::ShaderCompilationError(format!(
                "{what}: D3D11 compiles DXBC/DXIL only; this module carries {:?}. \
                 Compile it with D3DCompile or dxc and attach it as ShaderFormat::Dxil",
                desc.format
            )));
        }
        if module.code_size() == 0 {
            return Err(RhiError::ShaderCompilationError(format!(
                "{what}: the module has no bytecode"
            )));
        }
        Ok(&desc.code)
    }
}

// ===========================================================================
// Resource creation
// ===========================================================================

impl D3D11Device {
    fn buffer_flags(desc: &BufferDesc) -> RhiResult<(D3D11_USAGE, u32, u32, u32)> {
        // Storage must be a default-usage buffer anyway (UAV forbids DYNAMIC),
        // and a host-visible storage buffer would silently lose CPU access.
        if desc.usage.contains(BufferUsage::STORAGE) && desc.is_host_mappable() {
            return Err(RhiError::BufferCreationError(
                "D3D11 storage buffers must be device-local: a UAV forbids the \
                 DYNAMIC usage a host-visible buffer needs"
                    .into(),
            ));
        }
        if desc.usage.contains(BufferUsage::UNIFORM) && desc.size > 65536 {
            return Err(RhiError::BufferCreationError(format!(
                "D3D11 constant buffers cannot exceed 65536 bytes (asked for {})",
                desc.size
            )));
        }
        let mut bind = 0u32;
        if desc.usage.contains(BufferUsage::VERTEX) {
            bind |= D3D11_BIND_VERTEX_BUFFER.0 as u32;
        }
        if desc.usage.contains(BufferUsage::INDEX) {
            bind |= D3D11_BIND_INDEX_BUFFER.0 as u32;
        }
        if desc.usage.contains(BufferUsage::UNIFORM) {
            bind |= D3D11_BIND_CONSTANT_BUFFER.0 as u32;
        }
        if desc.usage.contains(BufferUsage::STORAGE) {
            bind |= D3D11_BIND_SHADER_RESOURCE.0 as u32;
            bind |= D3D11_BIND_UNORDERED_ACCESS.0 as u32;
        }
        let misc = if desc.usage.contains(BufferUsage::STORAGE) {
            D3D11_RESOURCE_MISC_BUFFER_ALLOW_RAW_VIEWS.0 as u32
        } else {
            0
        };
        if desc.is_host_mappable() {
            if bind & (D3D11_BIND_UNORDERED_ACCESS.0 as u32) != 0 {
                return Err(RhiError::BufferCreationError(
                    "a GPU-writable storage buffer cannot also be host-visible on D3D11".into(),
                ));
            }
            // Uploads map with WRITE_DISCARD per call; D3D11 never hands out a
            // stable mapping to the caller.
            Ok((D3D11_USAGE_DYNAMIC, bind, D3D11_CPU_ACCESS_WRITE.0 as u32, misc))
        } else {
            Ok((D3D11_USAGE_DEFAULT, bind, 0, misc))
        }
    }

    fn element_count(size: u64) -> u32 {
        // Raw buffer views count DWORDs: an empty buffer is never legal anyway
        // because `BufferDesc::is_valid` rejects zero size.
        (size / 4) as u32
    }

    pub(crate) fn create_storage_views(
        &self,
        buffer: &ID3D11Buffer,
        desc: &D3D11_BUFFER_DESC,
    ) -> RhiResult<(Option<DxCom<ID3D11ShaderResourceView>>, Option<DxCom<ID3D11UnorderedAccessView>>)> {
        if self.uav_support {
            let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_BUFFEREX,
                Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                    BufferEx: D3D11_BUFFEREX_SRV {
                        FirstElement: 0,
                        NumElements: desc.ByteWidth / 4,
                        Flags: D3D11_BUFFEREX_SRV_FLAG_RAW.0 as u32,
                    },
                },
            };
            let mut srv: Option<ID3D11ShaderResourceView> = None;
            unsafe {
                self.device
                    .0
                    .CreateShaderResourceView(buffer, Some(&srv_desc), Some(&mut srv))
            }
            .map_err(|e| {
                RhiError::BufferCreationError(format!("raw storage-buffer SRV: {e}"))
            })?;
            let uav_desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
                Format: DXGI_FORMAT_R32_TYPELESS,
                ViewDimension: D3D11_UAV_DIMENSION_BUFFER,
                Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                    Buffer: D3D11_BUFFER_UAV {
                        FirstElement: 0,
                        NumElements: desc.ByteWidth / 4,
                        Flags: D3D11_BUFFER_UAV_FLAG_RAW.0 as u32,
                    },
                },
            };
            let mut uav: Option<ID3D11UnorderedAccessView> = None;
            unsafe { self.device.0.CreateUnorderedAccessView(buffer, Some(&uav_desc), Some(&mut uav)) }
                .map_err(|e| RhiError::BufferCreationError(format!("raw storage-buffer UAV: {e}")))?;
            let srv = srv.ok_or_else(|| {
                RhiError::BufferCreationError("raw storage-buffer SRV: no object returned".into())
            })?;
            let uav = uav.ok_or_else(|| {
                RhiError::BufferCreationError("raw storage-buffer UAV: no object returned".into())
            })?;
            Ok((Some(DxCom(srv)), Some(DxCom(uav))))
        } else {
            Err(RhiError::NotSupported(
                "this D3D11 adapter has no raw/structured-buffer-UAV support \
                 (D3D10.x hardware option absent), so storage buffers are unusable"
                    .into(),
            ))
        }
    }
}

// Texture objects and their views.
impl D3D11Device {
    /// Bind flags for a texture description.
    fn texture_flags(&self, desc: &TextureDesc) -> RhiResult<u32> {
        if desc.usage.contains(TextureUsage::STORAGE) && !self.uav_support {
            return Err(RhiError::NotSupported(
                "this D3D11 adapter cannot create storage textures: no raw \
                 UAV support"
                    .into(),
            ));
        }
        let mut bind = 0u32;
        if desc.usage.contains(TextureUsage::SAMPLED)
            || desc.usage.contains(TextureUsage::STORAGE)
        {
            bind |= D3D11_BIND_SHADER_RESOURCE.0 as u32;
        }
        if desc.usage.contains(TextureUsage::STORAGE) {
            bind |= D3D11_BIND_UNORDERED_ACCESS.0 as u32;
        }
        if desc.usage.contains(TextureUsage::COLOR_ATTACHMENT) {
            bind |= D3D11_BIND_RENDER_TARGET.0 as u32;
        }
        if desc.usage.contains(TextureUsage::DEPTH_STENCIL_ATTACHMENT) {
            bind |= D3D11_BIND_DEPTH_STENCIL.0 as u32;
        }
        if bind == 0 {
            return Err(RhiError::TextureCreationError(
                "the texture usage produces no D3D11 bind flag; a transfer-only \
                 texture is not expressible in D3D11"
                    .into(),
            ));
        }
        Ok(bind)
    }

    /// Create the native texture object for a description.
    ///
    /// Depth/stencil textures are created with a typeless format, which is the
    /// only shape where a DSV and shader read views can coexist on D3D11.
    fn create_texture_object(
        &self,
        desc: &TextureDesc,
    ) -> RhiResult<(DxCom<ID3D11Resource>, DXGI_FORMAT, DXGI_FORMAT)> {
        let dxgi = dxgi_format(desc.format).ok_or_else(|| {
            RhiError::TextureCreationError(format!(
                "D3D11 has no mapping for format {:?}",
                desc.format
            ))
        })?;
        let typeless = if desc.format.is_depth() || desc.format.is_stencil() {
            super::convert::typeless_base(desc.format).ok_or_else(|| {
                RhiError::TextureCreationError(format!(
                    "D3D11 has no typeless base for format {:?}",
                    desc.format
                ))
            })?
        } else {
            dxgi
        };
        let bind = self.texture_flags(desc)?;
        let handle: DxCom<ID3D11Resource> = match desc.dimensions {
            TextureDimensions::D1 => {
                if desc.height != 1 || desc.depth != 1 {
                    return Err(RhiError::TextureCreationError(
                        "a 1D texture has height and depth 1".into(),
                    ));
                }
                let d = D3D11_TEXTURE2D_DESC {
                    Width: desc.width,
                    Height: desc.height,
                    MipLevels: desc.mip_levels,
                    ArraySize: desc.array_layers,
                    Format: typeless,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: bind,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut out: Option<ID3D11Texture2D> = None;
                unsafe { self.device.0.CreateTexture2D(&d, None, Some(&mut out)) }.map_err(|e| {
                    RhiError::TextureCreationError(format!(
                        "CreateTexture2D 1D {}x{}: {e}",
                        desc.width, desc.height
                    ))
                })?;
                let resource = out
                    .ok_or_else(|| RhiError::TextureCreationError("no texture object".into()))?
                    .cast::<ID3D11Resource>()
                    .map_err(|e| {
                        RhiError::TextureCreationError(format!("texture resource cast: {e}"))
                    })?;
                DxCom(resource)
            }
            TextureDimensions::D2 | TextureDimensions::Cube => {
                if desc.dimensions == TextureDimensions::Cube
                    && (desc.width != desc.height || desc.array_layers % 6 != 0)
                {
                    return Err(RhiError::TextureCreationError(
                        "a cube texture is square and its layer count a multiple of 6".into(),
                    ));
                }
                if desc.sample_count.as_count() > 1 {
                    return Err(RhiError::NotSupported(
                        "multisampled textures are not supported by the D3D11 backend yet"
                            .into(),
                    ));
                }
                let array = if desc.dimensions == TextureDimensions::Cube {
                    desc.array_layers * 6
                } else {
                    desc.array_layers
                };
                let d = D3D11_TEXTURE2D_DESC {
                    Width: desc.width,
                    Height: desc.height,
                    MipLevels: desc.mip_levels,
                    ArraySize: array,
                    Format: typeless,
                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: bind,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut out: Option<ID3D11Texture2D> = None;
                unsafe { self.device.0.CreateTexture2D(&d, None, Some(&mut out)) }.map_err(|e| {
                    RhiError::TextureCreationError(format!(
                        "CreateTexture2D {}x{}: {e}",
                        desc.width, desc.height
                    ))
                })?;
                let resource = out
                    .ok_or_else(|| RhiError::TextureCreationError("no texture object".into()))?
                    .cast::<ID3D11Resource>()
                    .map_err(|e| {
                        RhiError::TextureCreationError(format!("texture resource cast: {e}"))
                    })?;
                DxCom(resource)
            }
            TextureDimensions::D3 => {
                let d = D3D11_TEXTURE3D_DESC {
                    Width: desc.width,
                    Height: desc.height,
                    Depth: desc.depth,
                    MipLevels: desc.mip_levels,
                    Format: typeless,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: bind,
                    CPUAccessFlags: 0,
                    MiscFlags: 0,
                };
                let mut out: Option<ID3D11Texture3D> = None;
                unsafe { self.device.0.CreateTexture3D(&d, None, Some(&mut out)) }.map_err(|e| {
                    RhiError::TextureCreationError(format!(
                        "CreateTexture3D {}x{}x{}: {e}",
                        desc.width, desc.height, desc.depth
                    ))
                })?;
                let resource = out
                    .ok_or_else(|| RhiError::TextureCreationError("no texture object".into()))?
                    .cast::<ID3D11Resource>()
                    .map_err(|e| {
                        RhiError::TextureCreationError(format!("texture resource cast: {e}"))
                    })?;
                DxCom(resource)
            }
        };
        Ok((handle, dxgi, typeless))
    }

    /// The D3D subresource index of one RHI mip/array pair.
    ///
    /// D3D11 orders subresources mip-major: `mip + array_layer * mip_levels`.
    fn subresource_index(texture: &TextureDesc, mip_level: u32, array_layer: u32) -> u32 {
        mip_level + array_layer * texture.mip_levels
    }

    pub(crate) fn create_texture_inner(&self, desc: &TextureDesc) -> RhiResult<u64> {
        if !desc.is_valid() {
            return Err(RhiError::TextureCreationError(
                "the texture description is invalid".into(),
            ));
        }
        let (resource, typed, base) = self.create_texture_object(desc)?;
        let roles = self.create_view_roles(&resource.0, desc, typed, base)?;
        let handle = self.next_handle();
        self.handles.lock().unwrap().map.insert(
            handle,
            D3DObject::Texture(TextureEntry {
                resource,
                desc: desc.clone(),
                roles,
                download_staging: None,
            }),
        );
        Ok(handle)
    }

    pub(crate) fn create_view_roles(
        &self,
        resource: &ID3D11Resource,
        texture: &TextureDesc,
        typed: DXGI_FORMAT,
        base: DXGI_FORMAT,
    ) -> RhiResult<ViewEntry> {
        let mut srv: Option<DxCom<ID3D11ShaderResourceView>> = None;
        let mut rtv: Option<DxCom<ID3D11RenderTargetView>> = None;
        let mut uav: Option<DxCom<ID3D11UnorderedAccessView>> = None;
        let mut dsv: Option<DxCom<ID3D11DepthStencilView>> = None;

        if texture
            .usage
            .intersects(TextureUsage::SAMPLED | TextureUsage::STORAGE)
        {
            let view_format = if texture.format.is_depth() {
                super::convert::depth_srv_format(texture.format).ok_or_else(|| {
                    RhiError::TextureCreationError(format!(
                        "D3D11 has no shader-read format for {:?}",
                        texture.format
                    ))
                })?
            } else {
                base
            };
            let desc = match texture.dimensions {
                TextureDimensions::D1 => D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: view_format,
                    ViewDimension: windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE1D,
                    Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture1D: windows::Win32::Graphics::Direct3D11::D3D11_TEX1D_SRV {
                            MostDetailedMip: 0,
                            MipLevels: texture.mip_levels,
                        },
                    },
                },
                TextureDimensions::D3 => D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: view_format,
                    ViewDimension: windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE3D,
                    Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture3D: D3D11_TEX3D_SRV {
                            MostDetailedMip: 0,
                            MipLevels: texture.mip_levels,
                        },
                    },
                },
                TextureDimensions::D2 | TextureDimensions::Cube => D3D11_SHADER_RESOURCE_VIEW_DESC {
                    Format: view_format,
                    ViewDimension: windows::Win32::Graphics::Direct3D::D3D_SRV_DIMENSION_TEXTURE2D,
                    Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_SHADER_RESOURCE_VIEW_DESC_0 {
                        Texture2D: D3D11_TEX2D_SRV {
                            MostDetailedMip: 0,
                            MipLevels: texture.mip_levels,
                        },
                    },
                },
            };
            let mut out: Option<ID3D11ShaderResourceView> = None;
            unsafe { self.device.0.CreateShaderResourceView(resource, Some(&desc), Some(&mut out)) }
                .map_err(|e| RhiError::TextureCreationError(format!("CreateSRV: {e}")))?;
            srv = Some(DxCom(out.ok_or_else(|| {
                RhiError::TextureCreationError("CreateSRV returned no view".into())
            })?));
        }

        if texture.usage.contains(TextureUsage::COLOR_ATTACHMENT) {
            let desc = D3D11_RENDER_TARGET_VIEW_DESC {
                Format: base,
                ViewDimension: D3D11_RTV_DIMENSION_TEXTURE2D,
                Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_RENDER_TARGET_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_RTV { MipSlice: 0 },
                },
            };
            let mut out: Option<ID3D11RenderTargetView> = None;
            unsafe { self.device.0.CreateRenderTargetView(resource, Some(&desc), Some(&mut out)) }
                .map_err(|e| RhiError::TextureCreationError(format!("CreateRTV: {e}")))?;
            rtv = Some(DxCom(out.ok_or_else(|| {
                RhiError::TextureCreationError("CreateRTV returned no view".into())
            })?));
        }

        if texture.usage.contains(TextureUsage::DEPTH_STENCIL_ATTACHMENT) {
            let desc = D3D11_DEPTH_STENCIL_VIEW_DESC {
                Format: typed,
                ViewDimension: D3D11_DSV_DIMENSION_TEXTURE2D,
                Flags: 0,
                Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_DEPTH_STENCIL_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_DSV { MipSlice: 0 },
                },
            };
            let mut out: Option<ID3D11DepthStencilView> = None;
            unsafe { self.device.0.CreateDepthStencilView(resource, Some(&desc), Some(&mut out)) }
                .map_err(|e| RhiError::TextureCreationError(format!("CreateDSV: {e}")))?;
            dsv = Some(DxCom(out.ok_or_else(|| {
                RhiError::TextureCreationError("CreateDSV returned no view".into())
            })?));
        }

        if texture.usage.contains(TextureUsage::STORAGE) {
            if texture.dimensions != TextureDimensions::D2 {
                return Err(RhiError::NotSupported(
                    "storage access on D3D11 is texture-2D only for now".into(),
                ));
            }
            let desc = D3D11_UNORDERED_ACCESS_VIEW_DESC {
                Format: base,
                ViewDimension: D3D11_UAV_DIMENSION_TEXTURE2D,
                Anonymous: windows::Win32::Graphics::Direct3D11::D3D11_UNORDERED_ACCESS_VIEW_DESC_0 {
                    Texture2D: D3D11_TEX2D_UAV { MipSlice: 0 },
                },
            };
            let mut out: Option<ID3D11UnorderedAccessView> = None;
            unsafe { self.device.0.CreateUnorderedAccessView(resource, Some(&desc), Some(&mut out)) }
                .map_err(|e| RhiError::TextureCreationError(format!("CreateUAV: {e}")))?;
            uav = Some(DxCom(out.ok_or_else(|| {
                RhiError::TextureCreationError("CreateUAV returned no view".into())
            })?));
        }

        Ok(ViewEntry { srv, rtv, uav, dsv })
    }
}

impl BackendResource for D3D11Device {
    fn as_raw(&self) -> *const () {
        (self.device.0.as_raw()) as *const ()
    }

    fn backend_type(&self) -> GraphicsApi {
        GraphicsApi::Direct3D11
    }

    fn create_buffer(&self, desc: &BufferDesc) -> RhiResult<Buffer> {
        if !desc.is_valid() {
            return Err(RhiError::BufferCreationError(
                "the buffer description is invalid".into(),
            ));
        }
        let (usage, bind, cpu_access, misc) = Self::buffer_flags(desc)?;
        let d3d_desc = D3D11_BUFFER_DESC {
            ByteWidth: desc.size as u32,
            Usage: usage,
            BindFlags: bind,
            CPUAccessFlags: cpu_access,
            MiscFlags: misc,
            StructureByteStride: 0,
        };
        let mut buffer: Option<ID3D11Buffer> = None;
        unsafe { self.device.0.CreateBuffer(&d3d_desc, None, Some(&mut buffer)) }.map_err(|e| {
            RhiError::BufferCreationError(format!("CreateBuffer of {} bytes: {e}", desc.size))
        })?;
        let buffer = buffer.ok_or_else(|| {
            RhiError::BufferCreationError("CreateBuffer returned no object".into())
        })?;
        let (srv, uav) = if desc.usage.contains(BufferUsage::STORAGE) {
            let views = self.create_storage_views(&buffer, &d3d_desc)?;
            views
        } else {
            (None, None)
        };
        let handle = self.next_handle();
        self.handles.lock().unwrap().map.insert(
            handle,
            D3DObject::Buffer(BufferEntry {
                buffer: DxCom(buffer),
                size: desc.size,
                usage: desc.usage,
                host_visible: desc.is_host_mappable(),
                srv,
                uav,
            }),
        );
        let mut out = Buffer::from_desc(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn upload_buffer(&self, buffer: &Buffer, data: &[u8]) -> RhiResult<()> {
        let gpu = buffer.backend().ok_or_else(|| {
            RhiError::BackendError("the buffer has no GPU backing".into())
        })?;
        if data.len() as u64 > buffer.size() {
            return Err(RhiError::BackendError(format!(
                "upload of {} bytes overflows a {}-byte buffer",
                data.len(),
                buffer.size()
            )));
        }
        self.with_object(gpu.handle, "buffer", |object| {
            let D3DObject::Buffer(entry) = object else {
                return Err(RhiError::BackendError("buffer handle lost its kind".into()));
            };
            if !entry.host_visible {
                return Err(RhiError::NotSupported(
                    "device-only D3D11 buffers have no upload path; create the \
                     buffer host-visible and use a staging copy for GPU-local data"
                        .into(),
                ));
            }
            let resource: ID3D11Resource = entry
                .buffer
                .0
                .cast::<ID3D11Resource>()
                .map_err(|e| RhiError::InternalError(format!("buffer resource cast: {e}")))?;
            let mut mapped: D3D11_MAPPED_SUBRESOURCE = unsafe { std::mem::zeroed() };
            unsafe {
                self.context
                    .0
                    .Map(&resource, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&mut mapped))
                    .map_err(|e| RhiError::BackendError(format!("buffer Map: {e}")))?;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.as_ptr(),
                    mapped.pData as *mut u8,
                    data.len(),
                );
                self.context.0.Unmap(&resource, 0);
            }
            Ok(())
        })?;
        Ok(())
    }

    fn create_texture(&self, desc: &TextureDesc) -> RhiResult<Texture> {
        let handle = self.create_texture_inner(desc)?;
        let mut out = Texture::from_desc(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_texture_view(&self, texture: &Texture, desc: &TextureViewDesc) -> RhiResult<TextureView> {
        let gpu = texture.backend().ok_or_else(|| {
            RhiError::BackendError("the texture has no GPU backing".into())
        })?;
        self.with_object(gpu.handle, "texture", |object| {
            let D3DObject::Texture(entry) = object else {
                return Err(RhiError::InternalError(
                    "a texture handle lost its kind".into(),
                ));
            };
            if let Some(format) = desc.format {
                if format != entry.desc.format {
                    return Err(RhiError::NotSupported(format!(
                        "a D3D11 view cannot recast {:?} into {:?}; D3D11 views keep the \
                         type their resource was created with",
                        entry.desc.format, format
                    )));
                }
            }
            if desc.base_mip_level != 0
                || desc.mip_level_count != entry.desc.mip_levels
                || desc.base_array_layer != 0
                || desc.array_layer_count != entry.desc.array_layers
            {
                return Err(RhiError::NotSupported(
                    "D3D11 views created by this backend span whole mips and layers \
                     of the resource; per-mip or per-layer subsets are not supported"
                        .into(),
                ));
            }
            let mut out = TextureView::from_parts(texture.clone(), desc.clone());
            out.set_backend(gpu);
            Ok(out)
        })
    }

    fn create_sampler(&self, desc: &SamplerDesc) -> RhiResult<Sampler> {
        if desc.unnormalized_coordinates {
            return Err(RhiError::NotSupported(
                "D3D11 sampler coordinates are always normalized".into(),
            ));
        }
        let sampler_desc = D3D11_SAMPLER_DESC {
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
        };
        let mut out: Option<ID3D11SamplerState> = None;
        unsafe { self.device.0.CreateSamplerState(&sampler_desc, Some(&mut out)) }.map_err(|e| {
            RhiError::TextureCreationError(format!("CreateSamplerState: {e}"))
        })?;
        let sampler = out.ok_or_else(|| {
            RhiError::TextureCreationError("CreateSamplerState returned no state".into())
        })?;
        let handle = self.next_handle();
        self.handles.lock().unwrap().map.insert(handle, D3DObject::Sampler(DxCom(sampler)));
        let mut out_sampler = Sampler::from_desc(desc.clone());
        out_sampler.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out_sampler)
    }

    fn upload_texture(&self, texture: &Texture, data: &[u8]) -> RhiResult<()> {
        let gpu = texture.backend().ok_or_else(|| {
            RhiError::BackendError("the texture has no GPU backing".into())
        })?;
        self.with_object(gpu.handle, "texture", |object| {
            let D3DObject::Texture(entry) = object else {
                return Err(RhiError::InternalError(
                    "a texture handle lost its kind".into(),
                ));
            };
            let pitch = convert::row_pitch(entry.desc.format, entry.desc.width) as usize;
            let needed = (entry.desc.height as usize) * pitch;
            if data.len() < needed {
                return Err(RhiError::BackendError(format!(
                    "the {}-byte upload is too small: the texture needs {} bytes \
                     (format {:?}, {}x{}@row-pitch {})",
                    data.len(),
                    needed,
                    entry.desc.format,
                    entry.desc.width,
                    entry.desc.height,
                    pitch
                )));
            }
            let staging = D3D11_TEXTURE2D_DESC {
                Width: entry.desc.width,
                Height: entry.desc.height,
                MipLevels: 1,
                ArraySize: 1,
                Format: convert::dxgi_format(entry.desc.format).ok_or_else(|| {
                    RhiError::BackendError(format!(
                        "no D3D11 staging format for {:?}",
                        entry.desc.format
                    ))
                })?,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
                MiscFlags: 0,
            };
            let mut st: Option<ID3D11Texture2D> = None;
            unsafe { self.device.0.CreateTexture2D(&staging, None, Some(&mut st)) }
                .map_err(|e| RhiError::BackendError(format!("staging texture: {e}")))?;
            let st_res = st.ok_or_else(|| {
                RhiError::BackendError("the staging texture object was not created".into())
            })?;
            let st_res: ID3D11Resource = st_res.cast().map_err(|e| {
                RhiError::BackendError(format!("staging resource cast: {e}"))
            })?;
            let mut mapped: D3D11_MAPPED_SUBRESOURCE = unsafe { std::mem::zeroed() };
            unsafe {
                self.context.0.Map(
                    &st_res,
                    0,
                    D3D11_MAP_WRITE,
                    0,
                    Some(&mut mapped),
                )
            }
            .map_err(|e| RhiError::BackendError(format!("staging Map: {e}")))?;
            unsafe {
                for row in 0..entry.desc.height as usize {
                    let src = data.as_ptr().add(row * pitch);
                    let dst = (mapped.pData as *mut u8).add(row * mapped.RowPitch as usize);
                    std::ptr::copy_nonoverlapping(src, dst, pitch);
                }
                self.context.0.Unmap(&st_res, 0);
                self.context.0.CopyResource(&entry.resource.0, &st_res);
            }
            Ok(())
        })
    }

    fn create_shader_module(&self, desc: &ShaderModuleDesc) -> RhiResult<ShaderModule> {
        if !matches!(desc.format, ShaderFormat::Dxil) {
            return Err(RhiError::NotSupported(format!(
                "D3D11 accepts DXBC/DXIL containers only, got format {:?}",
                desc.format
            )));
        }
        if desc.code.is_empty() {
            return Err(RhiError::ShaderCompilationError(
                "the shader module is empty".into(),
            ));
        }
        // Format by itself is only a label; the bytes must carry the container
        // magic both D3D11 producers (fxc and dxc) emit, or the refusal belongs
        // here — a driver-level rejection much later would point at the draw,
        // not at the bytes that caused it.
        if &desc.code[..4] != b"DXBC" {
            return Err(RhiError::ShaderCompilationError(
                "the shader module bytes are not a D3D11 shader container; \
                 a DXIL or DXBC container starts with the DXBC magic"
                    .into(),
            ));
        }
        let handle = self.next_handle();
        self.handles.lock().unwrap().map.insert(
            handle,
            D3DObject::Shader(ShaderEntry {
                format: desc.format,
                code: desc.code.clone(),
            }),
        );
        let mut out = ShaderModule::new(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_descriptor_set_layout(
        &self,
        _desc: &DescriptorSetLayoutDesc,
    ) -> RhiResult<DescriptorSetLayout> {
        // D3D11 has no layout object; the backend validates bindings when a
        // pipeline is created. Keep the layout at its description so writes can
        // still be checked there.
        let handle = self.next_handle();
        self.handles.lock().unwrap().map.insert(handle, D3DObject::Layout);
        let mut out = DescriptorSetLayout::new(_desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_descriptor_set(&self, layout: &DescriptorSetLayout) -> RhiResult<DescriptorSet> {
        if !layout.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the descriptor set layout has no GPU backing".into(),
            ));
        }
        let handle = self.next_handle();
        let mut set = DescriptorSet::new(layout.clone());
        set.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(set)
    }

    fn write_descriptors(&self, set: &DescriptorSet, writes: &[DescriptorWrite]) -> RhiResult<()> {
        let gpu = set.backend().ok_or_else(|| {
            RhiError::BackendError("the descriptor set has no GPU backing".into())
        })?;
        let mut sets = self.sets.lock().unwrap();
        let entries = sets.entry(gpu.handle).or_insert_with(Vec::new);
        for write in writes {
            let binding = write.dst_binding;
            let layout_binding = set
                .layout()
                .binding(binding)
                .ok_or_else(|| {
                    RhiError::BackendError(format!(
                        "binding {} is not in the set layout, so D3D11 cannot know \
                         the slot the write belongs to",
                        binding
                    ))
                })?;
            for (element, info) in write.descriptors.iter().enumerate() {
                entries.push(SetWrite {
                    binding,
                    base_element: write.dst_array_element + element as u32,
                    ty: layout_binding.ty,
                    stages: layout_binding.stages,
                    kind: self.build_write(info)?,
                });
            }
        }
        Ok(())
    }

    fn create_compute_pipeline(
        &self,
        desc: &ComputePipelineDesc,
        layout: &DescriptorSetLayout,
    ) -> RhiResult<ComputePipeline> {
        let Some(module_gpu) = desc.shader_module().backend() else {
            return Err(RhiError::BackendError(
                "a compute pipeline shader module has no GPU backing".into(),
            ));
        };
        let Some(layout_gpu) = layout.backend() else {
            return Err(RhiError::BackendError(
                "a compute pipeline set layout has no GPU backing".into(),
            ));
        };
        let (code, format) = self.with_object(module_gpu.handle, "shader module", |object| {
            let D3DObject::Shader(entry) = object else {
                return Err(RhiError::InternalError(
                    "a shader handle lost its kind".into(),
                ));
            };
            Ok((entry.code.clone(), entry.format))
        })?;
        if !matches!(format, ShaderFormat::Dxil) {
            return Err(RhiError::PipelineCreationError(
                "compute pipelines accept DXBC/DXIL shader modules only".into(),
            ));
        }
        let mut out: Option<ID3D11ComputeShader> = None;
        unsafe { self.device.0.CreateComputeShader(&code, None, Some(&mut out)) }
            .map_err(|e| RhiError::PipelineCreationError(format!("CreateComputeShader: {e}")))?;
        let cs = DxCom(out.ok_or_else(|| {
            RhiError::PipelineCreationError("CreateComputeShader returned no shader".into())
        })?);
        let handle = self.next_handle();
        self.handles.lock().unwrap().map.insert(
            handle,
            D3DObject::ComputePipeline(ComputePipelineEntry { cs }),
        );
        let mut pipeline = ComputePipeline::new(desc.clone());
        pipeline.set_backend(GpuResource {
            handle,
            memory: layout_gpu.handle,
            mapped: 0,
        });
        Ok(pipeline)
    }

    fn dispatch_compute(
        &self,
        pipeline: &ComputePipeline,
        sets: &[&DescriptorSet],
        group_count_x: u32,
        group_count_y: u32,
        group_count_z: u32,
    ) -> RhiResult<()> {
        let Some(backend) = pipeline.backend() else {
            return Err(RhiError::BackendError(
                "compute pipeline has no GPU backing".into(),
            ));
        };
        let cs = self.with_object(backend.handle, "compute pipeline", |object| {
            let D3DObject::ComputePipeline(entry) = object else {
                return Err(RhiError::InternalError(
                    "a compute pipeline handle lost its kind".into(),
                ));
            };
            Ok(entry.cs.clone())
        })?;
        let ctx = &self.context.0;
        unsafe {
            ctx.CSSetShader(Some(&cs.0), None);
            for set in sets {
                let Some(set_gpu) = set.backend() else {
                    return Err(RhiError::BackendError(
                        "bound descriptor set has no GPU backing".into(),
                    ));
                };
                let writes = self.sets.lock().unwrap();
                let Some(list) = writes.get(&set_gpu.handle) else {
                    continue;
                };
                for write in list {
                    self.bind_compute_slot(ctx, write)?;
                }
            }
            ctx.Dispatch(group_count_x, group_count_y, group_count_z);
        }
        Ok(())
    }

    fn download_buffer(&self, buffer: &Buffer, offset: u64, size: u64) -> RhiResult<Vec<u8>> {
        if size == 0 {
            return Ok(Vec::new());
        }
        let gpu = buffer.backend().ok_or_else(|| {
            RhiError::BackendError("the buffer has no GPU backing".into())
        })?;
        let end = offset.saturating_add(size);
        if end > buffer.size() {
            return Err(RhiError::BackendError(format!(
                "the download of {}..{} overflows a {}-byte buffer",
                offset,
                end,
                buffer.size()
            )));
        }
        let staging = D3D11_BUFFER_DESC {
            ByteWidth: size as u32,
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
            StructureByteStride: 0,
        };
        let mut st: Option<ID3D11Buffer> = None;
        unsafe { self.device.0.CreateBuffer(&staging, None, Some(&mut st)) }
            .map_err(|e| RhiError::BackendError(format!("download staging: {e}")))?;
        let st = st.ok_or_else(|| {
            RhiError::BackendError("the download staging buffer was not created".into())
        })?;
        let st_res: ID3D11Resource = st
            .cast()
            .map_err(|e| RhiError::BackendError(format!("staging resource cast: {e}")))?;
        self.with_object(gpu.handle, "buffer", |object| {
            let D3DObject::Buffer(entry) = object else {
                return Err(RhiError::InternalError(
                    "a buffer handle lost its kind".into(),
                ));
            };
            let src: ID3D11Resource = entry.buffer.0.cast().map_err(|e| {
                RhiError::InternalError(format!("buffer resource cast: {e}"))
            })?;
            let region = D3D11_BOX {
                left: offset as u32,
                right: end as u32,
                top: 0,
                bottom: 1,
                front: 0,
                back: 1,
            };
            unsafe {
                self.context.0.CopySubresourceRegion(
                    &st_res,
                    0,
                    0,
                    0,
                    0,
                    &src,
                    0,
                    Some(&region as *const D3D11_BOX),
                )
            };
            Ok(())
        })?;
        let mut mapped: D3D11_MAPPED_SUBRESOURCE = unsafe { std::mem::zeroed() };
        let mut out = vec![0u8; size as usize];
        unsafe {
            self.context
                .0
                .Map(&st_res, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
                .map_err(|e| RhiError::BackendError(format!("download Map: {e}")))?;
            std::ptr::copy_nonoverlapping(
                mapped.pData as *const u8,
                out.as_mut_ptr(),
                size as usize,
            );
            self.context.0.Unmap(&st_res, 0);
        }
        Ok(out)
    }

    fn download_texture(
        &self,
        texture: &Texture,
        mip_level: u32,
        array_layer: u32,
    ) -> RhiResult<Vec<u8>> {
        let gpu = texture.backend().ok_or_else(|| {
            RhiError::BackendError("the texture has no GPU backing".into())
        })?;
        self.with_object(gpu.handle, "texture", |object| {
            let D3DObject::Texture(entry) = object else {
                return Err(RhiError::InternalError(
                    "a texture handle lost its kind".into(),
                ));
            };
            let desc = &entry.desc;
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
            if desc.depth > 1 {
                return Err(RhiError::NotSupported(
                    "D3D11 texture downloads are implemented for 1D/2D textures \
                     only, not 3D volumes"
                        .into(),
                ));
            }
            let format = convert::dxgi_format(desc.format).ok_or_else(|| {
                RhiError::BackendError(format!(
                    "no D3D11 staging format for {:?}",
                    desc.format
                ))
            })?;
            let staging = D3D11_TEXTURE2D_DESC {
                Width: desc.width,
                Height: desc.height,
                MipLevels: desc.mip_levels,
                ArraySize: desc.array_layers,
                Format: format,
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: 1,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };
            let mut st: Option<ID3D11Texture2D> = None;
            unsafe { self.device.0.CreateTexture2D(&staging, None, Some(&mut st)) }
                .map_err(|e| RhiError::BackendError(format!("download staging: {e}")))?;
            let st = st.ok_or_else(|| {
                RhiError::BackendError("the download staging texture was not created".into())
            })?;
            let st_res: ID3D11Resource = st.cast().map_err(|e| {
                RhiError::BackendError(format!("staging resource cast: {e}"))
            })?;
            unsafe { self.context.0.CopyResource(&st_res, &entry.resource.0) };
            let subresource =
                Self::subresource_index(desc, mip_level, array_layer);
            let mip_w = (desc.width >> mip_level).max(1);
            let mip_h = (desc.height >> mip_level).max(1);
            let row_bytes = convert::row_pitch(desc.format, mip_w) as usize;
            let mut mapped: D3D11_MAPPED_SUBRESOURCE = unsafe { std::mem::zeroed() };
            let mut out = vec![0u8; mip_h as usize * row_bytes];
            unsafe {
                self.context
                    .0
                    .Map(&st_res, subresource, D3D11_MAP_READ, 0, Some(&mut mapped))
                    .map_err(|e| RhiError::BackendError(format!("download Map: {e}")))?;
                let base = mapped.pData as *const u8;
                let src_pitch = mapped.RowPitch as usize;
                for row in 0..mip_h as usize {
                    let src = base.add(row * src_pitch);
                    let dst = out.as_mut_ptr().add(row * row_bytes);
                    std::ptr::copy_nonoverlapping(src, dst, row_bytes);
                }
                self.context.0.Unmap(&st_res, subresource);
            }
            Ok(out)
        })
    }

    fn create_render_pass(&self, desc: &RenderPassDesc) -> RhiResult<RenderPass> {
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .map
            .insert(handle, D3DObject::Pass(desc.clone()));
        let mut out = RenderPass::new(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_framebuffer(&self, desc: &FramebufferDesc) -> RhiResult<Framebuffer> {
        let mut rtvs = Vec::new();
        let mut dsv = None;
        for attachment in &desc.attachments {
            let view_gpu = attachment.texture_view.backend().ok_or_else(|| {
                RhiError::BackendError(
                    "a framebuffer attachment has no GPU backing".into(),
                )
            })?;
            let (rtv, depth) = self.with_object(view_gpu.handle, "texture view", |object| {
                let D3DObject::Texture(entry) = object else {
                    return Err(RhiError::InternalError(
                        "a texture view handle lost its kind".into(),
                    ));
                };
                Ok((entry.roles.rtv.clone(), entry.roles.dsv.clone()))
            })?;
            if let Some(depth) = depth {
                if dsv.is_some() {
                    return Err(RhiError::BackendError(
                        "the framebuffer declares more than one depth attachment; \
                         D3D11 has one depth-stencil slot"
                            .into(),
                    ));
                }
                dsv = Some(depth);
            } else if let Some(rtv) = rtv {
                rtvs.push(rtv);
            } else {
                return Err(RhiError::BackendError(
                    "the attachment view serves as neither a colour nor a depth \
                     target in D3D11; it needs COLOR or DEPTH usage"
                        .into(),
                ));
            }
        }
        if rtvs.is_empty() {
            return Err(RhiError::BackendError(
                "a D3D11 framebuffer needs at least one colour attachment".into(),
            ));
        }
        let entry = FramebufferEntry {
            rtvs,
            dsv,
            width: desc.width,
            height: desc.height,
        };
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .map
            .insert(handle, D3DObject::Framebuffer(entry));
        let mut out = Framebuffer::new(desc.clone());
        out.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(out)
    }

    fn create_graphics_pipeline(
        &self,
        desc: &GraphicsPipelineDesc,
        render_pass: &RenderPass,
        set_layouts: &[&DescriptorSetLayout],
    ) -> RhiResult<GraphicsPipeline> {
        if !render_pass.has_gpu_backing() {
            return Err(RhiError::BackendError(
                "the graphics pipeline's render pass has no GPU backing".into(),
            ));
        }
        for layout in set_layouts {
            if !layout.has_gpu_backing() {
                return Err(RhiError::BackendError(
                    "one of the graphics pipeline's set layouts has no GPU backing".into(),
                ));
            }
        }
        let (vs, ps, vs_code) = self.create_pipeline_shaders_graphics(desc)?;
        let Some(vs) = vs else {
            return Err(RhiError::PipelineCreationError(
                "a D3D11 graphics pipeline needs a vertex shader".into(),
            ));
        };
        let Some(ps) = ps else {
            return Err(RhiError::PipelineCreationError(
                "a D3D11 graphics pipeline needs a fragment shader".into(),
            ));
        };
        let input_layout = match (&desc.vertex_input_state, &vs_code) {
            (Some(input), Some(vs_code)) => self.vertex_input_layout(vs_code, input)?,
            (Some(_), None) => {
                return Err(RhiError::PipelineCreationError(
                    "the pipeline declares a vertex input, but no vertex shader \
                     gave bytecode to build the input layout from"
                        .into(),
                ));
            }
            (None, _) => None,
        };
        let topology =
            convert::d3d_topology(desc.input_assembly_state.topology).ok_or_else(|| {
                RhiError::PipelineCreationError(format!(
                    "D3D11 has no primitive topology for {:?}",
                    desc.input_assembly_state.topology
                ))
            })?;
        let rasterizer = desc
            .rasterizer_state
            .as_ref()
            .map(|state| self.create_rasterizer_state(state))
            .transpose()?;
        let blend_state = desc
            .color_blend_state
            .as_ref()
            .map(|state| {
                let object = self.create_blend_state(
                    state,
                    desc.multisample_state.alpha_to_coverage_enable,
                )?;
                Ok((object, state.blend_constants))
            })
            .transpose()?;
        let depth_stencil = desc
            .depth_stencil_state
            .as_ref()
            .map(|state| self.create_depth_stencil_state(state))
            .transpose()?;
        let mut vertex_strides = HashMap::new();
        if let Some(input) = &desc.vertex_input_state {
            for binding in &input.bindings {
                vertex_strides.insert(binding.binding, binding.stride);
            }
        }
        let set_handles: Vec<u64> = set_layouts
            .iter()
            .map(|layout| {
                layout
                    .backend()
                    .expect("the set layout backing was verified above")
                    .handle
            })
            .collect();
        let entry = GraphicsPipelineEntry {
            vs: Some(vs),
            ps: Some(ps),
            input_layout,
            topology: Some(topology),
            rasterizer,
            blend: blend_state.as_ref().map(|(object, _)| object.clone()),
            blend_constants: blend_state
                .map(|(_, constants)| constants)
                .unwrap_or_default(),
            depth_stencil,
            vertex_strides,
            set_layouts: set_handles,
        };
        let handle = self.next_handle();
        self.handles
            .lock()
            .unwrap()
            .map
            .insert(handle, D3DObject::GraphicsPipeline(entry));
        let mut pipeline = GraphicsPipeline::new(desc.clone());
        pipeline.set_backend(GpuResource { handle, memory: 0, mapped: 0 });
        Ok(pipeline)
    }

    fn wait_idle(&self) -> RhiResult<()> {
        unsafe { self.context.0.Flush() };
        Ok(())
    }

    fn release(&self, _kind: ReleaseKind, object: u64) {
        self.handles.lock().unwrap().map.remove(&object);
    }

    fn release_many(&self, objects: &[(ReleaseKind, u64)]) {
        let mut table = self.handles.lock().unwrap();
        for (_, object) in objects {
            table.map.remove(object);
        }
    }

    fn release_texture(&self, texture: &crate::resource::Texture) {
        let Some(gpu) = texture.backend() else {
            return;
        };
        self.release(ReleaseKind::Image, gpu.handle);
    }

    fn release_framebuffer(&self, framebuffer: &crate::command::pass::render::Framebuffer) {
        let Some(gpu) = framebuffer.backend() else {
            return;
        };
        self.release(ReleaseKind::Framebuffer, gpu.handle);
    }

    fn release_texture_view(&self, view: &crate::resource::TextureView) {
        let _ = view.backend();
    }

    fn tracked_object_count(&self) -> usize {
        self.handles.lock().unwrap().map.len()
    }

    fn tracked_object_summary(&self) -> String {
        let table = self.handles.lock().unwrap();
        let mut buffer = 0usize;
        let mut texture = 0usize;
        let mut view = 0usize;
        let mut sampler = 0usize;
        let mut shader = 0usize;
        let mut graphics_pipeline = 0usize;
        let mut compute_pipeline = 0usize;
        let mut pass = 0usize;
        let mut framebuffer = 0usize;
        let mut layout = 0usize;
        for object in table.map.values() {
            match object {
                D3DObject::Buffer(_) => buffer += 1,
                D3DObject::Texture(_) => texture += 1,
                D3DObject::View(_) => view += 1,
                D3DObject::Sampler(_) => sampler += 1,
                D3DObject::Shader(_) => shader += 1,
                D3DObject::GraphicsPipeline(_) => graphics_pipeline += 1,
                D3DObject::ComputePipeline(_) => compute_pipeline += 1,
                D3DObject::Pass(_) => pass += 1,
                D3DObject::Framebuffer(_) => framebuffer += 1,
                D3DObject::Layout => layout += 1,
            }
        }
        format!(
            "buffers {buffer}, textures {texture}, views {view}, samplers {sampler}, \
             shaders {shader}, graphics pipelines {graphics_pipeline}, \
             compute pipelines {compute_pipeline}, passes {pass}, \
             framebuffers {framebuffer}, layouts {layout}"
        )
    }

    fn submit_commands(&self, commands: &[Command]) -> RhiResult<()> {
        let mut recorder = super::record::Recorder::new();
        for command in commands {
            recorder.execute(self, command)?;
        }
        Ok(())
    }

    fn draw_framebuffer_indexed(
        &self,
        framebuffer: &Framebuffer,
        pipeline: &GraphicsPipeline,
        sets: &[&DescriptorSet],
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

// Descriptor binding state and pipelines.
impl D3D11Device {
    pub(crate) fn build_write(&self, info: &DescriptorInfo) -> RhiResult<WriteKind> {
        let slot_err = |what: &'static str| {
            RhiError::BackendError(format!(
                "the resource behind a {} write has no GPU backing", what
            ))
        };
        match info {
            DescriptorInfo::Sampler(sampler) => {
                let gpu = sampler.backend().ok_or_else(|| slot_err("sampler"))?;
                let sampler = self.with_object(gpu.handle, "sampler", |object| {
                    let D3DObject::Sampler(s) = object else {
                        return Err(RhiError::InternalError(
                            "a sampler handle lost its kind".into(),
                        ));
                    };
                    Ok(s.clone())
                })?;
                Ok(WriteKind::Sampler { sampler })
            }
            DescriptorInfo::Texture(view, combined) => {
                let gpu = view.backend().ok_or_else(|| slot_err("texture view"))?;
                let srv = self.with_object(gpu.handle, "texture view", |object| {
                    let D3DObject::Texture(entry) = object else {
                        return Err(RhiError::InternalError(
                            "a texture view handle lost its kind".into(),
                        ));
                    };
                    entry.roles.srv.clone().ok_or_else(|| {
                        RhiError::BackendError(
                            "the texture has no SRV for a shader write; was it created \
                             with SAMPLED or STORAGE usage?"
                                .into(),
                        )
                    })
                })?;
                if let Some(sampler) = combined {
                    let gpu = sampler.backend().ok_or_else(|| slot_err("sampler"))?;
                    let sampler = self.with_object(gpu.handle, "sampler", |object| {
                        let D3DObject::Sampler(s) = object else {
                            return Err(RhiError::InternalError(
                                "a combined sampler handle lost its kind".into(),
                            ));
                        };
                        Ok(s.clone())
                    })?;
                    return Ok(WriteKind::CombinedTextureSampler { srv, sampler });
                }
                Ok(WriteKind::Texture { srv })
            }
            DescriptorInfo::Buffer(buffer, offset, _) => {
                let binding = buffer.desc().usage.intersects(
                    BufferUsage::UNIFORM | BufferUsage::STORAGE,
                );
                if !binding {
                    return Err(RhiError::BackendError(
                        "the buffer write is not a uniform or storage buffer".into(),
                    ));
                }
                let gpu = buffer.backend().ok_or_else(|| slot_err("buffer"))?;
                self.with_object(gpu.handle, "buffer", |object| {
                    let D3DObject::Buffer(entry) = object else {
                        return Err(RhiError::InternalError(
                            "a buffer handle lost its kind".into(),
                        ));
                    };
                    if *offset != 0 {
                        return Err(RhiError::NotSupported(
                            "D3D11 binds constant buffers whole, and this backend \
                             binds storage buffers whole too, so a write carrying a \
                             non-zero offset cannot be honoured"
                                .into(),
                        ));
                    }
                    if entry.usage.contains(BufferUsage::UNIFORM) {
                        Ok(WriteKind::Constant { buffer: entry.buffer.clone() })
                    } else {
                        Ok(WriteKind::Storage {
                            uav: entry.uav.clone(),
                            srv: entry.srv.clone(),
                        })
                    }
                })
            }
            other => Err(RhiError::NotSupported(format!(
                "this D3D11 descriptor write kind ({other:?}) has no mapping",
            ))),
        }
    }

    fn create_pipeline_shaders_graphics(
        &self,
        desc: &GraphicsPipelineDesc,
    ) -> RhiResult<(
        Option<DxCom<ID3D11VertexShader>>,
        Option<DxCom<ID3D11PixelShader>>,
        Option<Vec<u8>>,
    )> {
        let mut vs = None;
        let mut ps = None;
        let mut vs_code = None;
        for stage in &desc.shader_stages {
            let module_gpu = stage.module.backend().ok_or_else(|| {
                RhiError::PipelineCreationError(
                    "a pipeline shader module has no GPU backing".into(),
                )
            })?;
            let (code, format) = self.with_object(module_gpu.handle, "shader module", |object| {
                let D3DObject::Shader(entry) = object else {
                    return Err(RhiError::InternalError(
                        "a shader handle lost its kind".into(),
                    ));
                };
                Ok((entry.code.clone(), entry.format))
            })?;
            if !matches!(format, ShaderFormat::Dxil) {
                return Err(RhiError::PipelineCreationError(
                    "D3D11 pipelines accept DXBC/DXIL shader modules only".into(),
                ));
            }
            match stage.stage {
                ShaderStage::VERTEX => {
                    let mut out: Option<ID3D11VertexShader> = None;
                    unsafe { self.device.0.CreateVertexShader(&code, None, Some(&mut out)) }
                        .map_err(|e| {
                            RhiError::PipelineCreationError(format!(
                                "CreateVertexShader: {e}"
                            ))
                        })?;
                    vs = Some(DxCom(out.ok_or_else(|| {
                        RhiError::PipelineCreationError("CreateVertexShader returned no shader".into())
                    })?));
                    vs_code = Some(code);
                }
                ShaderStage::FRAGMENT => {
                    let mut out: Option<ID3D11PixelShader> = None;
                    unsafe { self.device.0.CreatePixelShader(&code, None, Some(&mut out)) }
                        .map_err(|e| {
                            RhiError::PipelineCreationError(format!(
                                "CreatePixelShader: {e}"
                            ))
                        })?;
                    ps = Some(DxCom(out.ok_or_else(|| {
                        RhiError::PipelineCreationError("CreatePixelShader returned no shader".into())
                    })?));
                }
                _ => {
                    return Err(RhiError::PipelineCreationError(format!(
                        "stage {:?} has no place in a D3D11 graphics pipeline",
                        stage.stage
                    )));
                }
            }
        }
        Ok((vs, ps, vs_code))
    }

    fn create_rasterizer_state(
        &self,
        desc: &RasterizerState,
    ) -> RhiResult<DxCom<ID3D11RasterizerState>> {
        let fill = convert::d3d_fill_mode(desc.polygon_mode).ok_or_else(|| {
            RhiError::PipelineCreationError(
                "D3D11 has no point fill mode; a point mask needs a shader (not a \
                 rasterizer)"
                    .into(),
            )
        })?;
        let cull = convert::d3d_cull_mode(desc.cull_mode).ok_or_else(|| {
            RhiError::PipelineCreationError(
                "D3D11 cannot cull front and back faces at once; use \
                 rasterizer_discard_enable to skip drawing instead"
                    .into(),
            )
        })?;
        let d = D3D11_RASTERIZER_DESC {
            FillMode: fill,
            CullMode: cull,
            FrontCounterClockwise: matches!(
                desc.front_face,
                FrontFace::CounterClockwise
            )
            .into(),
            DepthBias: if desc.depth_bias_enable {
                desc.depth_bias_constant as i32
            } else {
                0
            },
            DepthBiasClamp: desc.depth_bias_clamp,
            SlopeScaledDepthBias: if desc.depth_bias_enable {
                desc.depth_bias_slope
            } else {
                0.0
            },
            DepthClipEnable: (!desc.depth_clamp_enable).into(),
            ScissorEnable: true.into(),
            MultisampleEnable: false.into(),
            AntialiasedLineEnable: false.into(),
        };
        let mut out: Option<ID3D11RasterizerState> = None;
        unsafe { self.device.0.CreateRasterizerState(&d, Some(&mut out)) }.map_err(|e| {
            RhiError::PipelineCreationError(format!("CreateRasterizerState: {e}"))
        })?;
        Ok(DxCom(out.ok_or_else(|| {
            RhiError::PipelineCreationError("CreateRasterizerState returned no state".into())
        })?))
    }

    pub(crate) fn bind_compute_slot(
        &self,
        ctx: &ID3D11DeviceContext,
        write: &SetWrite,
    ) -> RhiResult<()> {
        match &write.kind {
            WriteKind::Constant { buffer } => {
                unsafe {
                    ctx.CSSetConstantBuffers(
                        write.binding,
                        Some(&[Some(buffer.0.clone())][..]),
                    )
                };
            }
            WriteKind::Storage { uav, srv } => {
                if let Some(uav) = uav {
                    unsafe {
                        ctx.CSSetUnorderedAccessViews(
                            write.binding,
                            1,
                            Some([Some(uav.0.clone())].as_ptr()),
                            None,
                        )
                    };
                } else if let Some(srv) = srv {
                    unsafe {
                        ctx.CSSetShaderResources(
                            write.binding,
                            Some(&[Some(srv.0.clone())][..]),
                        )
                    };
                } else {
                    return Err(RhiError::BackendError(
                        "a storage write has neither a UAV nor an SRV".into(),
                    ));
                }
            }
            WriteKind::Texture { srv } => {
                unsafe {
                    ctx.CSSetShaderResources(
                        write.binding,
                        Some(&[Some(srv.0.clone())][..]),
                    )
                };
            }
            WriteKind::Sampler { sampler } => {
                unsafe {
                    ctx.CSSetSamplers(
                        write.binding,
                        Some(&[Some(sampler.0.clone())][..]),
                    )
                };
            }
            WriteKind::CombinedTextureSampler { srv, sampler } => {
                unsafe {
                    ctx.CSSetShaderResources(
                        write.binding,
                        Some(&[Some(srv.0.clone())][..]),
                    );
                    ctx.CSSetSamplers(
                        write.binding,
                        Some(&[Some(sampler.0.clone())][..]),
                    );
                }
            }
        }
        Ok(())
    }

    pub(crate) fn set_writes(&self, handle: u64) -> Vec<SetWrite> {
        self.sets
            .lock()
            .unwrap()
            .get(&handle)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn bind_graphics_slot(
        &self,
        ctx: &ID3D11DeviceContext,
        stages: ShaderStage,
        write: &SetWrite,
    ) -> RhiResult<()> {
        match (stages, &write.kind) {
            (ShaderStage::VERTEX, WriteKind::Constant { buffer }) => {
                let buffers = [Some(buffer.0.clone())];
                unsafe { ctx.VSSetConstantBuffers(write.binding, Some(&buffers[..])) };
            }
            (ShaderStage::VERTEX, WriteKind::Texture { srv }) => {
                let views = [Some(srv.0.clone())];
                unsafe { ctx.VSSetShaderResources(write.binding, Some(&views[..])) };
            }
            (
                ShaderStage::VERTEX,
                WriteKind::CombinedTextureSampler { srv, sampler },
            ) => {
                let views = [Some(srv.0.clone())];
                unsafe { ctx.VSSetShaderResources(write.binding, Some(&views[..])) };
                let samplers = [Some(sampler.0.clone())];
                unsafe { ctx.VSSetSamplers(write.binding, Some(&samplers[..])) };
            }
            (ShaderStage::VERTEX, WriteKind::Sampler { sampler }) => {
                let samplers = [Some(sampler.0.clone())];
                unsafe { ctx.VSSetSamplers(write.binding, Some(&samplers[..])) };
            }
            (ShaderStage::VERTEX, WriteKind::Storage { srv, .. }) => {
                let Some(srv) = srv else {
                    return Err(RhiError::BackendError(
                        "a graphics storage read has no SRV; D3D11 graphics stages \
                         read storage buffers through them"
                            .into(),
                    ));
                };
                let views = [Some(srv.0.clone())];
                unsafe { ctx.VSSetShaderResources(write.binding, Some(&views[..])) };
            }
            (ShaderStage::FRAGMENT, WriteKind::Constant { buffer }) => {
                let buffers = [Some(buffer.0.clone())];
                unsafe { ctx.PSSetConstantBuffers(write.binding, Some(&buffers[..])) };
            }
            (ShaderStage::FRAGMENT, WriteKind::Texture { srv }) => {
                let views = [Some(srv.0.clone())];
                unsafe { ctx.PSSetShaderResources(write.binding, Some(&views[..])) };
            }
            (
                ShaderStage::FRAGMENT,
                WriteKind::CombinedTextureSampler { srv, sampler },
            ) => {
                let views = [Some(srv.0.clone())];
                unsafe { ctx.PSSetShaderResources(write.binding, Some(&views[..])) };
                let samplers = [Some(sampler.0.clone())];
                unsafe { ctx.PSSetSamplers(write.binding, Some(&samplers[..])) };
            }
            (ShaderStage::FRAGMENT, WriteKind::Sampler { sampler }) => {
                let samplers = [Some(sampler.0.clone())];
                unsafe { ctx.PSSetSamplers(write.binding, Some(&samplers[..])) };
            }
            (ShaderStage::FRAGMENT, WriteKind::Storage { srv, .. }) => {
                let Some(srv) = srv else {
                    return Err(RhiError::BackendError(
                        "a graphics storage read has no SRV; D3D11 graphics stages \
                         read storage buffers through them"
                            .into(),
                    ));
                };
                let views = [Some(srv.0.clone())];
                unsafe { ctx.PSSetShaderResources(write.binding, Some(&views[..])) };
            }
            (ShaderStage::COMPUTE, _) => {
                self.bind_compute_slot(ctx, write)?;
            }
            (stages, _) => {
                return Err(RhiError::NotSupported(format!(
                    "no D3D11 binding path for stage {:?}",
                    stages
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn create_blend_state(
        &self,
        state: &ColorBlendState,
        alpha_to_coverage: bool,
    ) -> RhiResult<DxCom<ID3D11BlendState>> {
        if state.logic_op_enable {
            return Err(RhiError::NotSupported(
                "the D3D11 1.0 blend state object has no logic operation; a \
                 pipeline with logic_op_enable needs the 1.1 object instead"
                    .into(),
            ));
        }
        let template = match state.attachments.first() {
            Some(first) => D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: first.blend_enable.into(),
                SrcBlend: convert::d3d_blend_factor(first.src_color_blend_factor),
                DestBlend: convert::d3d_blend_factor(first.dst_color_blend_factor),
                BlendOp: convert::d3d_blend_op(first.color_blend_op),
                SrcBlendAlpha: convert::d3d_blend_factor(first.src_alpha_blend_factor),
                DestBlendAlpha: convert::d3d_blend_factor(first.dst_alpha_blend_factor),
                BlendOpAlpha: convert::d3d_blend_op(first.alpha_blend_op),
                RenderTargetWriteMask: convert::d3d_color_write(first.color_write_mask).0 as u8,
            },
            None => D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: false.into(),
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_ZERO,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ZERO,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
            },
        };
        let d = D3D11_BLEND_DESC {
            AlphaToCoverageEnable: alpha_to_coverage.into(),
            IndependentBlendEnable: false.into(),
            RenderTarget: [template; 8],
        };
        let mut out: Option<ID3D11BlendState> = None;
        unsafe { self.device.0.CreateBlendState(&d, Some(&mut out)) }.map_err(|e| {
            RhiError::PipelineCreationError(format!("CreateBlendState: {e}"))
        })?;
        Ok(DxCom(out.ok_or_else(|| {
            RhiError::PipelineCreationError("CreateBlendState returned no state".into())
        })?))
    }

    pub(crate) fn create_depth_stencil_state(
        &self,
        state: &DepthStencilState,
    ) -> RhiResult<DxCom<ID3D11DepthStencilState>> {
        let face = |s: &StencilOpState| D3D11_DEPTH_STENCILOP_DESC {
            StencilFailOp: convert::d3d_stencil_op(s.fail_op),
            StencilDepthFailOp: convert::d3d_stencil_op(s.depth_fail_op),
            StencilPassOp: convert::d3d_stencil_op(s.pass_op),
            StencilFunc: convert::d3d_compare_func(s.compare_op),
        };
        let d = D3D11_DEPTH_STENCIL_DESC {
            DepthEnable: state.depth_test_enable.into(),
            DepthWriteMask: if state.depth_write_enable {
                D3D11_DEPTH_WRITE_MASK_ALL
            } else {
                D3D11_DEPTH_WRITE_MASK_ZERO
            },
            DepthFunc: convert::d3d_compare_func(state.depth_compare_op),
            StencilEnable: state.stencil_test_enable.into(),
            StencilReadMask: state.front.compare_mask as u8,
            StencilWriteMask: state.front.write_mask as u8,
            FrontFace: face(&state.front),
            BackFace: face(&state.back),
        };
        let mut out: Option<ID3D11DepthStencilState> = None;
        unsafe { self.device.0.CreateDepthStencilState(&d, Some(&mut out)) }.map_err(|e| {
            RhiError::PipelineCreationError(format!("CreateDepthStencilState: {e}"))
        })?;
        Ok(DxCom(out.ok_or_else(|| {
            RhiError::PipelineCreationError("CreateDepthStencilState returned no state".into())
        })?))
    }

    pub(crate) fn vertex_input_layout(
        &self,
        vs_code: &[u8],
        input: &VertexInputState,
    ) -> RhiResult<Option<DxCom<ID3D11InputLayout>>> {
        let blob = unsafe {
            D3DGetInputSignatureBlob(
                vs_code.as_ptr() as *const core::ffi::c_void,
                vs_code.len(),
            )
        }
        .map_err(|e| {
            RhiError::PipelineCreationError(format!("input signature blob: {e}"))
        })?;
        let params = unsafe {
            std::slice::from_raw_parts(
                blob.GetBufferPointer() as *const D3D11_SIGNATURE_PARAMETER_DESC,
                blob.GetBufferSize() / std::mem::size_of::<D3D11_SIGNATURE_PARAMETER_DESC>(),
            )
        };
        let mut elements = Vec::new();
        for param in params {
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
                    "no D3D11 format for the vertex attribute at location {}",
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
            let instance = matches!(slot.input_rate, VertexInputRate::Instance);
            elements.push(D3D11_INPUT_ELEMENT_DESC {
                SemanticName: param.SemanticName,
                SemanticIndex: param.SemanticIndex,
                Format: format,
                InputSlot: attribute.binding,
                AlignedByteOffset: attribute.offset,
                InputSlotClass: if instance {
                    D3D11_INPUT_PER_INSTANCE_DATA
                } else {
                    D3D11_INPUT_PER_VERTEX_DATA
                },
                InstanceDataStepRate: if instance { 1 } else { 0 },
            });
        }
        if elements.is_empty() {
            // A vertex shader that reads nothing but SV_VertexID carries no
            // input signature. D3D11 has no layout to make for that: the
            // feed state stays NULL and the draw pulls from the vertex id
            // alone, which `record.rs` already handles by not binding one.
            return Ok(None);
        }
        let mut out: Option<ID3D11InputLayout> = None;
        unsafe {
            self.device
                .0
                .CreateInputLayout(&elements, vs_code, Some(&mut out))
        }
        .map_err(|e| {
            RhiError::PipelineCreationError(format!("CreateInputLayout: {e}"))
        })?;
        Ok(Some(DxCom(out.ok_or_else(|| {
            RhiError::PipelineCreationError("CreateInputLayout returned no layout".into())
        })?)))
    }
}
