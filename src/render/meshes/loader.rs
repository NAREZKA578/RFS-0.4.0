//! Mesh Loader
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Loads meshes from various file formats.

use super::mesh::Mesh;
use std::path::Path;
use std::sync::Arc;

/// Mesh loader
pub struct MeshLoader {
    /// Device reference
    device: Option<Arc<crate::rhi::Device>>,
}

impl MeshLoader {
    /// Create a new mesh loader
    pub fn new() -> Self {
        Self { device: None }
    }

    /// Set the device
    pub fn set_device(&mut self, device: Arc<crate::rhi::Device>) {
        self.device = Some(device);
    }

    /// Load a mesh from a file
    pub fn load(&self, path: &Path) -> Option<Mesh> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "obj" => self.load_obj(path),
            "gltf" | "glb" => self.load_gltf(path),
            "fbx" => self.load_fbx(path),
            _ => None,
        }
    }

    /// Load a mesh from OBJ file
    fn load_obj(&self, _path: &Path) -> Option<Mesh> {
        // In actual implementation, this would parse the OBJ file
        // and create a mesh

        // For now, return a cube as a placeholder
        Some(Mesh::cube("obj_mesh", 1.0))
    }

    /// Load a mesh from glTF file
    fn load_gltf(&self, _path: &Path) -> Option<Mesh> {
        // In actual implementation, this would parse the glTF file
        // and create a mesh

        // For now, return a cube as a placeholder
        Some(Mesh::cube("gltf_mesh", 1.0))
    }

    /// Load a mesh from FBX file
    fn load_fbx(&self, _path: &Path) -> Option<Mesh> {
        // In actual implementation, this would parse the FBX file
        // and create a mesh

        // For now, return a cube as a placeholder
        Some(Mesh::cube("fbx_mesh", 1.0))
    }

    /// Load a mesh with buffers
    pub fn load_with_buffers(&self, path: &Path) -> Option<Mesh> {
        let mut mesh = self.load(path)?;

        if let Some(device) = &self.device {
            mesh.create_buffers(device);
        }

        Some(mesh)
    }
}

impl Default for MeshLoader {
    fn default() -> Self {
        Self::new()
    }
}
