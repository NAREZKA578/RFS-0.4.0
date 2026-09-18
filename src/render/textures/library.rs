//! Texture Library
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Manages loading, storing, and retrieving textures.

use super::loader::TextureLoader;
use super::texture::{Texture, TextureFormat, TextureUsage};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// Texture library for managing textures
pub struct TextureLibrary {
    /// Map of texture names to textures
    textures: HashMap<String, Arc<Texture>>,
    /// Texture loader
    loader: TextureLoader,
}

impl TextureLibrary {
    /// Create a new texture library
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
            loader: TextureLoader::new(),
        }
    }

    /// Set the device (for creating textures)
    pub fn set_device(&mut self, device: Arc<crate::rhi::Device>) {
        self.loader.set_device(device.clone());
    }

    /// Load a texture from a file
    pub fn load(&mut self, path: &Path) -> Option<Arc<Texture>> {
        if let Some(texture) = self.loader.load(path) {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unnamed")
                .to_string();
            let texture = Arc::new(texture);
            self.textures.insert(name.clone(), texture.clone());
            Some(texture)
        } else {
            None
        }
    }

    /// Load a texture with a custom name
    pub fn load_with_name(&mut self, path: &Path, name: &str) -> Option<Arc<Texture>> {
        if let Some(texture) = self.loader.load(path) {
            let texture = Arc::new(texture);
            self.textures.insert(name.to_string(), texture.clone());
            Some(texture)
        } else {
            None
        }
    }

    /// Add a texture to the library
    pub fn add(&mut self, name: &str, texture: Texture) -> Arc<Texture> {
        let texture = Arc::new(texture);
        self.textures.insert(name.to_string(), texture.clone());
        texture
    }

    /// Get a texture by name
    pub fn get(&self, name: &str) -> Option<Arc<Texture>> {
        self.textures.get(name).cloned()
    }

    /// Remove a texture by name
    pub fn remove(&mut self, name: &str) -> Option<Arc<Texture>> {
        self.textures.remove(name)
    }

    /// Get all texture names
    pub fn texture_names(&self) -> Vec<String> {
        self.textures.keys().cloned().collect()
    }

    /// Get texture count
    pub fn texture_count(&self) -> usize {
        self.textures.len()
    }

    /// Clear all textures
    pub fn clear(&mut self) {
        self.textures.clear();
    }

    /// Create a white texture
    pub fn create_white(&mut self, name: &str, size: u32) -> Arc<Texture> {
        let device = self
            .loader
            .device
            .as_ref()
            .expect("No device available to create white texture");
        self.add(
            name,
            self.loader
                .create_solid_color(name, [255, 255, 255, 255], size, size, device)
                .unwrap(),
        )
    }

    /// Create a black texture
    pub fn create_black(&mut self, name: &str, size: u32) -> Arc<Texture> {
        let device = self
            .loader
            .device
            .as_ref()
            .expect("No device available to create black texture");
        self.add(
            name,
            self.loader
                .create_solid_color(name, [0, 0, 0, 255], size, size, device)
                .unwrap(),
        )
    }

    /// Create a checkerboard texture
    pub fn create_checkerboard(&mut self, name: &str, size: u32) -> Arc<Texture> {
        let device = self
            .loader
            .device
            .as_ref()
            .expect("No device available to create checkerboard texture");
        self.add(
            name,
            self.loader.create_checkerboard(name, size, device).unwrap(),
        )
    }

    /// Create common textures
    pub fn create_common(&mut self) {
        if self.loader.device.is_some() {
            self.create_white("white", 256);
            self.create_black("black", 256);
            self.create_checkerboard("checkerboard", 256);

            // Create normal map (flat)
            let device = self
                .loader
                .device
                .as_ref()
                .expect("No device available to create normal map");
            let normal_data = vec![[128u8, 128u8, 255u8, 255u8]; 256 * 256].concat();
            let normal_tex = Texture::new_2d(
                "normal_flat",
                TextureFormat::RGBA8,
                256,
                256,
                TextureUsage::Normal,
                device,
            );
            normal_tex.upload_data(&normal_data, device);
            self.add("normal_flat", normal_tex);
        }
    }
}

impl Default for TextureLibrary {
    fn default() -> Self {
        Self::new()
    }
}
