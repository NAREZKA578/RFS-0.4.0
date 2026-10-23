//! Direct3D 11 Backend Factory (instance)
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use crate::backend::Backend;
use crate::config::RhiConfig;
use crate::core::{
    Device, DeviceDesc, MemoryHeap, MemoryProperties, MemoryType, PhysicalDevice,
    PhysicalDeviceType, Queue, QueueFamily,
};
use crate::error::{RhiError, RhiResult};
use crate::types::GraphicsApi;
use crate::{Extent3D, Features, Limits, MemoryHeapFlags, MemoryPropertyFlags, QueueFlags, SampleCount};
use windows::Win32::Graphics::Direct3D::{
    D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1, D3D_DRIVER_TYPE_UNKNOWN,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_CREATE_DEVICE_DEBUG,
    D3D11_CREATE_DEVICE_FLAG, D3D11_SDK_VERSION, ID3D11Device, ID3D11DeviceContext,
};
use windows::Win32::Graphics::Dxgi::{
    CreateDXGIFactory1, IDXGIFactory1, IDXGIAdapter1, DXGI_ADAPTER_FLAG_SOFTWARE,
};

use super::device::{D3D11Device, DxCom};

/// One adapter the backend uses to make devices.
///
/// The `PhysicalDevice` handed to `create_device` is a snapshot; this keeps the
/// live `IDXGIAdapter1` the snapshot was made from. The match back onto the
/// adapter is by the snapshot's (name, vendor, device) triple — the adapter list
/// of a running machine does not change under D3D11.
struct AdapterInfo {
    adapter: DxCom<IDXGIAdapter1>,
    name: String,
    vendor_id: u32,
    device_id: u32,
}

/// The D3D11 backend. Everything device-shaped goes through
/// [`D3D11Device`](super::device::D3D11Device); this type only owns the DXGI
/// factory and the adapter list, per the same split as the Vulkan backend's
/// `VulkanBackend` + `DestroyableInstance`.
pub struct D3D11Backend {
    factory: DxCom<IDXGIFactory1>,
    adapters: Vec<AdapterInfo>,
    /// The levels advertise the features of the most advanced level actually
    /// reached; conservative FL 11_0 tables are what the reports claim.
    feature_levels: [D3D_FEATURE_LEVEL; 2],
}

impl D3D11Backend {
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
                    eprintln!("[rhi][d3d11] adapter {index} cannot be described: {e}");
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
            feature_levels: [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0],
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
                    "physical device '{}' was not enumerated by the D3D11 backend in this session",
                    physical.name
                ))
            })
    }

    /// The report one adapter produces.
    ///
    /// Limits are the conservative intersection of all D3D11 hardware (the FL
    /// 11_1 tables lifted from the SDK's D3D11_core_structures), not a
    /// per-adapter `CheckFeatureSupport` walk — no device exists yet at this
    /// point, and the RHI's own `Limits` do not distinguish within them for
    /// the sizes a test or the renderer touches.
    fn describe_adapter(&self, info: &AdapterInfo) -> RhiResult<PhysicalDevice> {
        let desc = unsafe { info.adapter.0.GetDesc1() }.map_err(|e| {
            RhiError::BackendError(format!("adapter description lost after enumeration: {e}"))
        })?;
        let is_software = desc.Flags & (DXGI_ADAPTER_FLAG_SOFTWARE.0 as u32) != 0;

        let queue_flags = QueueFlags::GRAPHICS | QueueFlags::COMPUTE | QueueFlags::TRANSFER | QueueFlags::PRESENT;
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
                depth_clamp: false,
                depth_bounds: false,
                compute: true,
                indirect_compute: false,
                // D3D10.x-style raw/structured UAVs are hardware dependent
                // even at FL 11_0/11_1; the report stays conservative, and
                // device-level use confirms per-adapter support.
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

    /// Create the D3D11 device objects for one adapter, with the debug layer
    /// when asked for and a fallback when the layer is not installed.
    ///
    /// BGRA support is unconditionally requested: the presentation surface
    /// formats the RHI describes are DXGI B8G8R8A8 family, and a device
    /// created without the flag renders them wrong on some drivers.
    fn create_device_objects(
        adapter: &DxCom<IDXGIAdapter1>,
        validation: bool,
    ) -> RhiResult<(DxCom<ID3D11Device>, DxCom<ID3D11DeviceContext>, D3D_FEATURE_LEVEL)> {
        let levels: &[D3D_FEATURE_LEVEL] = &[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
        let step = |flags: D3D11_CREATE_DEVICE_FLAG| -> windows::core::Result<
            (Option<ID3D11Device>, Option<ID3D11DeviceContext>, D3D_FEATURE_LEVEL),
        > {
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut level = D3D_FEATURE_LEVEL_11_0;
            // An explicit adapter forces `D3D_DRIVER_TYPE_UNKNOWN`; anything
            // else is rejected with E_INVALIDARG by the SDK contract, and the
            // flag was the very first thing the device test caught (№710).
            unsafe {
                D3D11CreateDevice(
                    &adapter.0,
                    D3D_DRIVER_TYPE_UNKNOWN,
                    windows::Win32::Foundation::HMODULE::default(),
                    flags,
                    Some(levels),
                    D3D11_SDK_VERSION,
                    Some(&mut device),
                    Some(&mut level),
                    Some(&mut context),
                )?;
            }
            Ok((device, context, level))
        };
        let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        let attempt = if validation { flags | D3D11_CREATE_DEVICE_DEBUG } else { flags };
        match step(attempt) {
            Ok((device, context, level)) => {
                let device = device.ok_or_else(|| {
                    RhiError::DeviceCreationError(
                        "D3D11CreateDevice returned no device".into(),
                    )
                })?;
                let context = context.ok_or_else(|| {
                    RhiError::DeviceCreationError(
                        "D3D11CreateDevice returned no immediate context".into(),
                    )
                })?;
                Ok((DxCom(device), DxCom(context), level))
            }
            Err(e) if validation => {
                // The debug layer is a redistributable; a machine without it
                // must still be usable, so the second try runs without the
                // flag, and the lost diagnostics are said out loud.
                eprintln!(
                    "[rhi][d3d11] device creation with the debug layer failed ({e}); \
                     retrying without it - D3D11 validation diagnostics are off"
                );
                let (device, context, level) = step(flags)
                    .map_err(|e2| RhiError::DeviceCreationError(format!("D3D11CreateDevice failed: {e2}")))?;
                let device = device.ok_or_else(|| {
                    RhiError::DeviceCreationError(
                        "D3D11CreateDevice returned no device".into(),
                    )
                })?;
                let context = context.ok_or_else(|| {
                    RhiError::DeviceCreationError(
                        "D3D11CreateDevice returned no immediate context".into(),
                    )
                })?;
                Ok((DxCom(device), DxCom(context), level))
            }
            Err(e) => Err(RhiError::DeviceCreationError(format!(
                "D3D11CreateDevice failed: {e}"
            ))),
        }
    }

    pub fn wait_device_idle(&self, device: &Device) -> RhiResult<()> {
        device.wait_idle()
    }
}

impl Backend for D3D11Backend {
    fn name(&self) -> &str {
        "Direct3D 11"
    }

    fn api(&self) -> GraphicsApi {
        GraphicsApi::Direct3D11
    }

    fn enumerate_physical_devices(&self) -> RhiResult<Vec<PhysicalDevice>> {
        self.adapters
            .iter()
            .map(|info| self.describe_adapter(info))
            .collect()
    }

    fn create_device(&self, physical: &PhysicalDevice, desc: &DeviceDesc) -> RhiResult<Device> {
        let info = self.find_adapter(physical)?;
        let (device, context, level) = Self::create_device_objects(&info.adapter, desc.validation)?;

        let queues = vec![Queue {
            family_index: 0,
            index: 0,
            flags: QueueFlags::GRAPHICS | QueueFlags::COMPUTE | QueueFlags::TRANSFER | QueueFlags::PRESENT,
        }];
        let resource = Box::new(D3D11Device::new(device, context, &physical.clone(), level));
        Ok(Device::from_parts(physical.clone(), queues, resource))
    }

    fn wait_idle(&self, device: &Device) -> RhiResult<()> {
        self.wait_device_idle(device)
    }
}
