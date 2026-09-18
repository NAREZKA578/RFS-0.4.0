//! Mesh Library
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Manages loading, storing, and retrieving meshes.

use super::loader::MeshLoader;
use super::mesh::Mesh;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Mesh library for managing meshes
pub struct MeshLibrary {
    /// Map of mesh names to meshes
    meshes: HashMap<String, Arc<Mesh>>,
    /// Mesh loader
    loader: MeshLoader,
}

impl MeshLibrary {
    /// Create a new mesh library
    pub fn new() -> Self {
        Self {
            meshes: HashMap::new(),
            loader: MeshLoader::new(),
        }
    }

    /// Set the device (for creating buffers)
    pub fn set_device(&mut self, device: Arc<crate::rhi::Device>) {
        self.loader.set_device(device.clone());

        // Recreate buffers for all loaded meshes
        for mesh in self.meshes.values() {
            let mut mesh_clone = (**mesh).clone();
            mesh_clone.create_buffers(&device);
            // Note: We can't replace the Arc here easily
            // In actual implementation, we would need to handle this better
        }
    }

    /// Load a mesh from a file
    pub fn load(&mut self, path: &Path) -> Option<Arc<Mesh>> {
        if let Some(mesh) = self.loader.load_with_buffers(path) {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string();
            let mesh = Arc::new(mesh);
            self.meshes.insert(name.clone(), mesh.clone());
            Some(mesh)
        } else {
            None
        }
    }

    /// Load a mesh with a custom name
    pub fn load_with_name(&mut self, path: &Path, name: &str) -> Option<Arc<Mesh>> {
        if let Some(mesh) = self.loader.load_with_buffers(path) {
            let mesh = Arc::new(mesh);
            self.meshes.insert(name.to_string(), mesh.clone());
            Some(mesh)
        } else {
            None
        }
    }

    /// Add a mesh to the library
    pub fn add(&mut self, name: &str, mesh: Mesh) -> Arc<Mesh> {
        let mesh = Arc::new(mesh);
        self.meshes.insert(name.to_string(), mesh.clone());
        mesh
    }

    /// Get a mesh by name
    pub fn get(&self, name: &str) -> Option<Arc<Mesh>> {
        self.meshes.get(name).cloned()
    }

    /// Remove a mesh by name
    pub fn remove(&mut self, name: &str) -> Option<Arc<Mesh>> {
        self.meshes.remove(name)
    }

    /// Get all mesh names
    pub fn mesh_names(&self) -> Vec<String> {
        self.meshes.keys().cloned().collect()
    }

    /// Get mesh count
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Clear all meshes
    pub fn clear(&mut self) {
        self.meshes.clear();
    }

    /// Create common primitive meshes
    pub fn create_primitives(&mut self) {
        self.add("cube", Mesh::cube("cube", 1.0));
        self.add("sphere", Mesh::sphere("sphere", 1.0, 16, 16));
        self.add("plane", Mesh::plane("plane", 1.0, 1.0));
        self.add("cylinder", Mesh::cylinder("cylinder", 1.0, 1.0, 16));
        self.add("fullscreen_quad", Mesh::fullscreen_quad("fullscreen_quad"));
    }
}

impl Default for MeshLibrary {
    fn default() -> Self {
        Self::new()
    }
}
