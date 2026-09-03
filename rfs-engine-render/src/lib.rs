//! RFS engine render core.
//!
//! Донор: `RFS-0.3/src/rhi/*.rs` (только трейты), `src/graphics/camera.rs`,
//! `frustum.rs`, `lighting.rs`.
//!
//! Принципы форка:
//! - Никаких бэкендов-заглушек (DX11/DX12 с `panic!` сюда не переехали).
//! - Бэкенд — один трейт [`rhi::Backend`]; реализация (OpenGL/Vulkan) подключается
//!   позже отдельным крейтом и обязана проходить `NullBackend`-тест этого крейта.
//! - Математика только на `glam`, сериализация — только там, где нужна сети.

pub mod camera;
pub mod frustum;
pub mod lighting;
pub mod rhi;

pub use camera::Camera;
pub use frustum::Frustum;
pub use lighting::{DirectionalLight, LightingRig};
pub use rhi::{Backend, BackendType, GpuInfo};
