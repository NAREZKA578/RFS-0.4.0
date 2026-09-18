//! Texture Loader
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Loads textures from various image file formats.

use super::texture::{Texture, TextureFormat, TextureUsage};
use std::path::Path;
use std::sync::Arc;

/// Texture loader
pub struct TextureLoader {
    /// Device reference
    pub(crate) device: Option<Arc<crate::rhi::Device>>,
}

impl TextureLoader {
    /// Create a new texture loader
    pub fn new() -> Self {
        Self { device: None }
    }

    /// Set the device
    pub fn set_device(&mut self, device: Arc<crate::rhi::Device>) {
        self.device = Some(device);
    }

    /// Load a texture from a file
    pub fn load(&self, path: &Path) -> Option<Texture> {
        if self.device.is_none() {
            return None;
        }

        let device = self.device.as_ref().unwrap();
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "png" | "jpg" | "jpeg" | "bmp" | "tga" | "dds" | "ktx2" => {
                self.load_image(path, device)
            }
            _ => None,
        }
    }

    /// Load an image file
    fn load_image(&self, path: &Path, device: &Arc<crate::rhi::Device>) -> Option<Texture> {
        // In actual implementation, this would:
        // 1. Read the image file
        // 2. Decode it to RGBA8 data
        // 3. Create a texture
        // 4. Upload the data

        // For now, create a placeholder texture
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unnamed")
            .to_string();

        let texture = Texture::new_2d(
            &name,
            TextureFormat::RGBA8,
            256,
            256,
            TextureUsage::Diffuse,
            device,
        );

        // Upload some placeholder data (white texture)
        let data = vec![255u8; 256 * 256 * 4];
        texture.upload_data(&data, device);

        Some(texture)
    }

    /// Load a cube map
    pub fn load_cube(
        &self,
        paths: [&Path; 6],
        device: &Arc<crate::rhi::Device>,
    ) -> Option<Texture> {
        if paths.iter().any(|p| !p.exists()) {
            return None;
        }

        // In actual implementation, this would load 6 images
        // and create a cube map texture

        // For now, create a placeholder cube map
        let texture = Texture::new_cube(
            "cube_map",
            TextureFormat::RGBA8,
            256,
            1,
            TextureUsage::Diffuse,
            device,
        );

        // Upload placeholder data
        let data = vec![255u8; 256 * 256 * 4 * 6];
        texture.upload_data(&data, device);

        Some(texture)
    }

    /// Load a depth texture
    pub fn load_depth(
        &self,
        width: u32,
        height: u32,
        device: &Arc<crate::rhi::Device>,
    ) -> Option<Texture> {
        Some(Texture::new_depth("depth", width, height, device))
    }

    /// Load a render target texture
    pub fn load_render_target(
        &self,
        width: u32,
        height: u32,
        format: TextureFormat,
        device: &Arc<crate::rhi::Device>,
    ) -> Option<Texture> {
        Some(Texture::new_render_target(
            "render_target",
            width,
            height,
            format,
            device,
        ))
    }

    /// Create a solid color texture
    pub fn create_solid_color(
        &self,
        name: &str,
        color: [u8; 4],
        width: u32,
        height: u32,
        device: &Arc<crate::rhi::Device>,
    ) -> Option<Texture> {
        let texture = Texture::new_2d(
            name,
            TextureFormat::RGBA8,
            width,
            height,
            TextureUsage::Diffuse,
            device,
        );

        let data: Vec<u8> = (0..width * height).flat_map(|_| color.to_vec()).collect();
        texture.upload_data(&data, device);

        Some(texture)
    }

    /// Create a checkerboard texture
    pub fn create_checkerboard(
        &self,
        name: &str,
        size: u32,
        device: &Arc<crate::rhi::Device>,
    ) -> Option<Texture> {
        let texture = Texture::new_2d(
            name,
            TextureFormat::RGBA8,
            size,
            size,
            TextureUsage::Diffuse,
            device,
        );

        let mut data = vec![0u8; (size * size * 4) as usize];
        for y in 0..size {
            for x in 0..size {
                let index = ((y * size + x) * 4) as usize;
                if (x + y) % 2 == 0 {
                    data[index..index + 4].copy_from_slice(&[255, 255, 255, 255]);
                } else {
                    data[index..index + 4].copy_from_slice(&[0, 0, 0, 255]);
                }
            }
        }

        texture.upload_data(&data, device);

        Some(texture)
    }
}

impl Default for TextureLoader {
    fn default() -> Self {
        Self::new()
    }
}
