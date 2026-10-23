//! Direct3D 12 Backend Factory (instance)
//!
//! Everything device-shaped goes through
//! [`D3D12Device`](super::device::D3D12Device); this type only owns the DXGI
//! factory and the adapter list, per the same split as the D3D11 backend.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::Backend;
use crate::config::RhiConfig;
use crate::core::{
    Device, DeviceDesc, MemoryHeap, MemoryProperties, MemoryType, PhysicalDevice,
    PhysicalDeviceType, Queue, QueueFamily,
};
use crate::{Extent3D, Features, Limits, MemoryHeapFlags, MemoryPropertyFlags, QueueFlags};
use crate::error::{RhiError, RhiResult};
use crate::types::GraphicsApi;
use crate::types::SampleCount;
use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
use windows::Win32::Graphics::Direct3D12::{
    D3D12CreateDevice, D3D12_COMMAND_LIST_TYPE_DIRECT, D3D12_COMMAND_QUEUE_DESC,
    D3D12_COMMAND_QUEUE_FLAG_NONE, D3D12GetDebugInterface, D3D12_FENCE_FLAG_NONE,
    ID3D12CommandQueue, ID3D12Debug, ID3D12Device, ID3D12Fence,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, DXGI_ADAPTER_FLAG_SOFTWARE, IDXGIAdapter1, IDXGIFactory1,
};

use super::device::{D3D12Device, DxCom};

/// One adapter the backend uses to make devices.
///
/// The `PhysicalDevice` handed to `create_device` is a snapshot; this keeps
/// the live `IDXGIAdapter1` the snapshot was made from. The match back onto
/// the adapter is by the snapshot's (name, vendor, device) triple — the
/// adapter list of a running machine does not change under DirectX.
struct AdapterInfo {
    adapter: DxCom<IDXGIAdapter1>,
    name: String,
    vendor_id: u32,
    device_id: u32,
}

/// The D3D12 backend.
pub struct D3D12Backend {
    factory: DxCom<IDXGIFactory1>,
    adapters: Vec<AdapterInfo>,
}

impl D3D12Backend {
    pub fn new(_config: &RhiConfig) -> RhiResult<Self> {
        let factory: IDXGIFactory1 = unsafe {
            CreateDXGIFactory1().map_err(|e| {
                RhiError::DeviceCreationError(format!("IDXGIFactory1 creation failed: {e}"))
            })?
        };
        let mut adapters = Vec::new();
        // EnumAdapters1 fails (not returns empty) at the first out-of-range
        // index; that failure is the stop signal, not a backend error.
        for index in 0.. {
            let adapter = match unsafe { factory.EnumAdapters1(index) } {
                Ok(adapter) => adapter,
                Err(_) => break,
            };
            let desc = match unsafe { adapter.GetDesc1() } {
                Ok(desc) => desc,
                Err(e) => {
                    eprintln!("[rhi][d3d12] adapter {index} cannot be described: {e}");
                    continue;
                }
            };
            let name = String::from_utf16_lossy(
                &desc.Description
                    [..desc.Description.iter().position(|c| *c == 0).unwrap_or(desc.Description.len())],
            );
            adapters.push(AdapterInfo {
                adapter: DxCom(adapter),
                name,
                vendor_id: desc.VendorId,
                device_id: desc.DeviceId,
            });
        }
        if adapters.is_empty() {
            return Err(RhiError::NoPhysicalDevices);
        }
        Ok(Self {
            factory: DxCom(factory),
            adapters,
        })
    }

    /// The adapter a `PhysicalDevice` report came from.
    ///
    /// Matching is by the triple the report carries. A device the backend
    /// enumerated in this session is always matchable; anything else is an
    /// internal error in the caller, not a missing GPU.
    fn find_adapter(&self, physical: &PhysicalDevice) -> RhiResult<&AdapterInfo> {
        self.adapters
            .iter()
            .find(|a| {
                a.name == physical.name
                    && a.vendor_id == physical.vendor_id
                    && a.device_id == physical.device_id
            })
            .ok_or_else(|| {
                RhiError::BackendError(format!(
                    "physical device '{}' was not enumerated by the D3D12 backend in this session",
                    physical.name
                ))
            })
    }

    /// The report one adapter produces.
    ///
    /// Limits are conservative FL 12_0 tables, not a per-adapter
    /// `CheckFeatureSupport` walk — no device exists yet at this point, and
    /// the RHI's `Limits` do not distinguish within them for the sizes a
    /// test or the renderer touches. The memory heaps come from the adapter
    /// description: `DedicatedVideoMemory` is the device-local heap, its
    /// counterpart `SharedSystemMemory` carries the UPLOAD/READBACK heap
    /// types.
    fn describe_adapter(&self, info: &AdapterInfo) -> RhiResult<PhysicalDevice> {
        let desc = unsafe { info.adapter.0.GetDesc1() }.map_err(|e| {
            RhiError::BackendError(format!("adapter description lost after enumeration: {e}"))
        })?;
        let is_software = desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;

        let queue_flags =
            QueueFlags::GRAPHICS | QueueFlags::COMPUTE | QueueFlags::TRANSFER | QueueFlags::PRESENT;
        Ok(PhysicalDevice {
            name: info.name.clone(),
            vendor_id: desc.VendorId,
            device_id: desc.DeviceId,
            device_type: if is_software {
                PhysicalDeviceType::Cpu
            } else {
                PhysicalDeviceType::Discrete
            },
            features: Features {
                geometry_shader: true,
                tessellation_shader: true,
                sampler_anisotropy: true,
                texture_compression_bc: true,
                // Depth clamping is a rasterizer setting on D3D12
                // (`DepthClipEnable = false`); the report stays conservative
                // until a pipeline actually exercises it.
                depth_clamp: false,
                depth_bounds: false,
                compute: true,
                indirect_compute: false,
                // Raw/structured UAVs are what D3D12 does, but the report
                // stays conservative at FL 12_0; device-level use confirms
                // per-adapter support the same way D3D11 does.
                storage_buffer: false,
                storage_image: false,
                ..Default::default()
            },
            limits: Limits {
                max_texture_size: 16384,
                max_texture_layers: 2048,
                max_texture_mips: 14,
                max_buffer_size: 1 << 30,
                max_uniform_buffer_size: 65536,
                max_storage_buffer_size: 1 << 26,
                max_push_constants_size: 0,
                max_bound_descriptor_sets: 8,
                max_per_stage_descriptors: 128,
                max_color_attachments: 8,
                max_sample_count: SampleCount::X4,
                max_viewports: 16,
                max_compute_work_group_count: [65535; 3],
                max_compute_work_group_size: [1024; 3],
            },
            memory_properties: MemoryProperties {
                memory_types: vec![
                    MemoryType {
                        flags: MemoryPropertyFlags::DEVICE_LOCAL,
                        heap_index: 0,
                    },
                    MemoryType {
                        flags: MemoryPropertyFlags::HOST_VISIBLE
                            | MemoryPropertyFlags::HOST_COHERENT
                            | MemoryPropertyFlags::HOST_CACHED,
                        heap_index: 1,
                    },
                ],
                memory_heaps: vec![
                    MemoryHeap {
                        size: desc.DedicatedVideoMemory as u64,
                        flags: MemoryHeapFlags::DEVICE_LOCAL,
                    },
                    MemoryHeap {
                        size: desc.SharedSystemMemory as u64,
                        flags: MemoryHeapFlags::empty(),
                    },
                ],
            },
            queue_families: vec![QueueFamily {
                index: 0,
                flags: queue_flags,
                queue_count: 1,
                timestamp_valid_bits: 64,
                min_image_transfer_granularity: Extent3D { width: 1, height: 1, depth: 1 },
            }],
        })
    }

    /// Create the D3D12 device objects for one adapter: the device at
    /// FL 12_0, the direct command queue and the fence the  wait-idle
    /// signals.
    ///
    /// The debug layer must be enabled before the first device is created;
    /// a machine without the D3D12 tools must still be usable, so a failed
    /// `D3D12GetDebugInterface` downgrades to a spoken warning rather than
    /// an error, same contract as the D3D11 backend.
    fn create_device_objects(
        adapter: &DxCom<IDXGIAdapter1>,
        validation: bool,
    ) -> RhiResult<(DxCom<ID3D12Device>, DxCom<ID3D12CommandQueue>, DxCom<ID3D12Fence>)> {
        if validation {
            let mut debug: Option<ID3D12Debug> = None;
            match unsafe { D3D12GetDebugInterface::<ID3D12Debug>(&mut debug) } {
                Ok(()) => unsafe {
                    debug
                        .ok_or_else(|| {
                            RhiError::DeviceCreationError(
                                "D3D12GetDebugInterface returned no debug object".into(),
                            )
                        })?
                        .EnableDebugLayer();
                },
                Err(e) => eprintln!(
                    "[rhi][d3d12] the debug layer is not installed ({e}); \
                     D3D12 validation diagnostics are off"
                ),
            }
        }

        let mut device: Option<ID3D12Device> = None;
        unsafe {
            D3D12CreateDevice::<_, ID3D12Device>(
                &adapter.0,
                D3D_FEATURE_LEVEL_12_0,
                &mut device,
            )
        }
        .map_err(|e| RhiError::DeviceCreationError(format!("D3D12CreateDevice failed: {e}")))?;
        let device = device.ok_or_else(|| {
            RhiError::DeviceCreationError("D3D12CreateDevice returned no device".into())
        })?;

        let queue: ID3D12CommandQueue = unsafe {
            device.CreateCommandQueue::<ID3D12CommandQueue>(&D3D12_COMMAND_QUEUE_DESC {
                Type: D3D12_COMMAND_LIST_TYPE_DIRECT,
                Flags: D3D12_COMMAND_QUEUE_FLAG_NONE,
                NodeMask: 0,
                Priority: 0,
            })
        }
        .map_err(|e| RhiError::DeviceCreationError(format!("command queue creation failed: {e}")))?;

        let fence: ID3D12Fence =
            unsafe { device.CreateFence::<ID3D12Fence>(0, D3D12_FENCE_FLAG_NONE) }.map_err(
                |e| RhiError::DeviceCreationError(format!("fence creation failed: {e}")),
            )?;

        Ok((DxCom(device), DxCom(queue), DxCom(fence)))
    }

    pub fn wait_device_idle(&self, device: &Device) -> RhiResult<()> {
        device.wait_idle()
    }
}

impl Backend for D3D12Backend {
    fn name(&self) -> &str {
        "Direct3D 12"
    }

    fn api(&self) -> GraphicsApi {
        GraphicsApi::Direct3D12
    }

    fn enumerate_physical_devices(&self) -> RhiResult<Vec<PhysicalDevice>> {
        self.adapters
            .iter()
            .map(|info| self.describe_adapter(info))
            .collect()
    }

    fn create_device(&self, physical: &PhysicalDevice, desc: &DeviceDesc) -> RhiResult<Device> {
        let info = self.find_adapter(physical)?;
        let (device, queue, fence) = Self::create_device_objects(&info.adapter, desc.validation)?;

        let queues = vec![Queue {
            family_index: 0,
            index: 0,
            flags: QueueFlags::GRAPHICS | QueueFlags::COMPUTE | QueueFlags::TRANSFER
                | QueueFlags::PRESENT,
        }];
        let resource = Box::new(D3D12Device::new(device, queue, fence, physical)?);
        Ok(Device::from_parts(physical.clone(), queues, resource))
    }

    fn wait_idle(&self, device: &Device) -> RhiResult<()> {
        self.wait_device_idle(device)
    }
}
