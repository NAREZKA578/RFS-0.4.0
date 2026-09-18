//! Meshes Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod library;
pub mod loader;
pub mod mesh;

pub use library::MeshLibrary;
pub use loader::MeshLoader;
pub use mesh::{IndexType, Mesh, MeshFlags, PrimitiveType, Vertex, VertexAttribute};
