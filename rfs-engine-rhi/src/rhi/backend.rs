//! Graphics backend abstraction.

use super::device::Device;
use super::surface::{Surface, SurfaceDescriptor};
use std::sync::Arc;

/// Type of graphics backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackendType {
    /// OpenGL 3.3+ backend.
    OpenGL,
    /// Vulkan backend.
    Vulkan,
    /// DirectX 11 backend.
    DirectX11,
    /// DirectX 12 backend.
    DirectX12,
}

impl BackendType {
    /// Returns the name of the backend.
    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenGL => "OpenGL 3.3",
            Self::Vulkan => "Vulkan 1.3",
            Self::DirectX11 => "DirectX 11",
            Self::DirectX12 => "DirectX 12",
        }
    }

    /// Returns true if this backend is available on the current platform.
    pub fn is_available(&self) -> bool {
        match self {
            Self::OpenGL => true,
            Self::Vulkan => cfg!(feature = "vulkan"),
            Self::DirectX11 => cfg!(feature = "dx11") && cfg!(windows),
            Self::DirectX12 => cfg!(feature = "dx12") && cfg!(windows),
        }
    }

    /// Returns all available backends on the current platform.
    pub fn available_backends() -> Vec<Self> {
        #[allow(unused_mut)]
        let mut backends = vec![Self::OpenGL];
        #[cfg(feature = "vulkan")]
        backends.push(Self::Vulkan);
        #[cfg(feature = "dx11")]
        backends.push(Self::DirectX11);
        #[cfg(feature = "dx12")]
        backends.push(Self::DirectX12);
        backends
    }

    /// Returns the preferred backend for the current platform.
    pub fn preferred() -> Self {
        #[cfg(feature = "vulkan")]
        {
            if Self::Vulkan.is_available() {
                return Self::Vulkan;
            }
        }
        #[cfg(all(feature = "dx12", windows))]
        {
            if Self::DirectX12.is_available() {
                return Self::DirectX12;
            }
        }
        #[cfg(all(feature = "dx11", windows))]
        {
            if Self::DirectX11.is_available() {
                return Self::DirectX11;
            }
        }
        Self::OpenGL
    }
}

impl std::fmt::Display for BackendType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Backend descriptor for creating a graphics backend.
#[derive(Debug, Clone)]
pub struct BackendDescriptor {
    /// Type of backend to create.
    pub backend_type: BackendType,
    /// Whether to enable validation layers (Vulkan/OpenGL debug).
    pub enable_validation: bool,
    /// Whether to enable vsync.
    pub enable_vsync: bool,
    /// Preferred GPU adapter index (if multiple GPUs available).
    pub preferred_adapter: Option<usize>,
}

impl Default for BackendDescriptor {
    fn default() -> Self {
        Self {
            backend_type: BackendType::preferred(),
            enable_validation: cfg!(debug_assertions),
            enable_vsync: true,
            preferred_adapter: None,
        }
    }
}

/// Graphics backend trait — implemented by each backend.
pub trait Backend: Send + Sync {
    /// Returns the type of this backend.
    fn backend_type(&self) -> BackendType;

    /// Returns a reference to the device.
    fn device(&self) -> &Arc<dyn Device>;

    /// Returns a mutable reference to the device.
    fn device_mut(&mut self) -> &mut Arc<dyn Device>;

    /// Creates a surface for the given window handle.
    fn create_surface(
        &self,
        descriptor: &SurfaceDescriptor,
    ) -> Result<Box<dyn Surface>, Box<dyn std::error::Error>>;

    /// Creates a swap chain for the given surface.
    fn create_swap_chain(
        &self,
        surface: &dyn Surface,
        descriptor: &super::swapchain::SwapChainDescriptor,
    ) -> Result<Box<dyn super::swapchain::SwapChain>, Box<dyn std::error::Error>>;

    /// Presents the swap chain to the screen.
    fn present(
        &self,
        swap_chain: &dyn super::swapchain::SwapChain,
    ) -> Result<(), Box<dyn std::error::Error>>;

    /// Returns GPU information.
    fn gpu_info(&self) -> GpuInfo;

    /// Returns true if this backend supports ray tracing.
    fn supports_ray_tracing(&self) -> bool {
        false
    }

    /// Returns true if this backend supports mesh shaders.
    fn supports_mesh_shaders(&self) -> bool {
        false
    }
}

/// GPU adapter information.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    /// GPU vendor name.
    pub vendor: String,
    /// GPU device name.
    pub device: String,
    /// GPU driver version.
    pub driver_version: String,
    /// Dedicated video memory in bytes.
    pub dedicated_video_memory: usize,
    /// Shared system memory in bytes.
    pub shared_system_memory: usize,
    /// Backend type.
    pub backend: BackendType,
}

impl GpuInfo {
    pub fn new(vendor: &str, device: &str, driver_version: &str, backend: BackendType) -> Self {
        Self {
            vendor: vendor.to_string(),
            device: device.to_string(),
            driver_version: driver_version.to_string(),
            dedicated_video_memory: 0,
            shared_system_memory: 0,
            backend,
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} | {} | Driver: {}",
            self.backend, self.device, self.driver_version
        )
    }
}
