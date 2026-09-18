//! Materials Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod library;
pub mod material;
pub mod pbr;
pub mod water;

pub use library::MaterialLibrary;
pub use material::{BlendMode, CullMode, Material, MaterialParameters, MaterialType};
pub use pbr::PbrMaterial;
pub use water::WaterMaterial;
