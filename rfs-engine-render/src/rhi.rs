//! Минимальные RHI-абстракции движка.
//!
//! В доноре (`RFS-0.3/src/rhi/`) рядом с этими трейтами лежали четыре бэкенда,
//! два из которых — честные заглушки с `panic!` в горячем пути
//! (`dx11/swapchain.rs:17-18`, `dx12/swapchain.rs:17-18`, конструкторы возвращают
//! `Err("not yet fully implemented")`). Сюда переехали только трейты и
//! `NullBackend` для headless-тестов; настоящий бэкенд пишется поверх.

use std::sync::Arc;

/// Какой графический API реализует бэкенд.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendType {
    Null,
    OpenGl,
    Vulkan,
}

/// Сводка о GPU для логов/телеметрии.
#[derive(Debug, Clone)]
pub struct GpuInfo {
    pub device_name: String,
    pub backend: BackendType,
}

impl GpuInfo {
    pub fn new(device_name: impl Into<String>, backend: BackendType) -> Self {
        Self {
            device_name: device_name.into(),
            backend,
        }
    }
}

/// Дескриптор кадра: размер цели + vsync. Расширяется по мере портирования бэкенда.
#[derive(Debug, Clone, Copy)]
pub struct FrameDescriptor {
    pub width: u32,
    pub height: u32,
    pub vsync: bool,
}

/// Бэкенд движка. Контракт:
/// - `begin_frame`/`end_frame` всегда парны; двойной `begin_frame` без `end_frame`
///   обязан вернуть ошибку, а не течь ресурсом (см. GL-FBO-LEAK-1 в доноре).
/// - Все методы потокобезопасны относительно главного потока рендера.
pub trait Backend: Send + Sync {
    fn backend_type(&self) -> BackendType;
    fn gpu_info(&self) -> GpuInfo;
    fn begin_frame(&self, desc: &FrameDescriptor) -> anyhow::Result<()>;
    fn end_frame(&self) -> anyhow::Result<()>;
}

/// Заглушка для headless-тестов и демо без GPU.
pub struct NullBackend {
    info: GpuInfo,
    in_frame: std::sync::atomic::AtomicBool,
}

impl NullBackend {
    pub fn new() -> Self {
        Self {
            info: GpuInfo::new("null-device", BackendType::Null),
            in_frame: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl Default for NullBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for NullBackend {
    fn backend_type(&self) -> BackendType {
        BackendType::Null
    }

    fn gpu_info(&self) -> GpuInfo {
        self.info.clone()
    }

    fn begin_frame(&self, desc: &FrameDescriptor) -> anyhow::Result<()> {
        if desc.width == 0 || desc.height == 0 {
            anyhow::bail!("zero-size frame target");
        }
        if self
            .in_frame
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            anyhow::bail!("begin_frame without end_frame (would leak the previous target)");
        }
        Ok(())
    }

    fn end_frame(&self) -> anyhow::Result<()> {
        self.in_frame
            .store(false, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

/// Общий доступ к бэкенду.
pub type BackendRef = Arc<dyn Backend>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_begin_is_an_error_not_a_leak() {
        let b = NullBackend::new();
        let d = FrameDescriptor {
            width: 1280,
            height: 720,
            vsync: true,
        };
        b.begin_frame(&d).unwrap();
        assert!(b.begin_frame(&d).is_err());
        b.end_frame().unwrap();
        b.begin_frame(&d).unwrap();
    }

    #[test]
    fn zero_size_frame_rejected() {
        let b = NullBackend::new();
        let d = FrameDescriptor {
            width: 0,
            height: 0,
            vsync: false,
        };
        assert!(b.begin_frame(&d).is_err());
    }
}
