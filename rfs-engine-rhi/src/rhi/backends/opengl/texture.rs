//! OpenGL Texture implementation.

use crate::rhi::{Texture, TextureDescriptor, TextureFormat, TextureType, WrapMode};
use glow::HasContext;
use std::sync::Arc;

/// OpenGL texture.
#[derive(Debug)]
pub struct OpenGLTexture {
    gl: Arc<glow::Context>,
    id: glow::NativeTexture,
    target: u32,
    width: u32,
    height: u32,
    depth: u32,
    format: TextureFormat,
    mip_levels: u32,
}

impl OpenGLTexture {
    /// Returns the GL target for this texture.
    pub fn gl_target(&self) -> u32 {
        self.target
    }

    /// Creates a new OpenGL texture.
    pub fn new(
        gl: &Arc<glow::Context>,
        descriptor: &TextureDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let id = unsafe { gl.create_texture()? };

        let (internal_format, format, ty) = match descriptor.format {
            TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm => {
                (glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE)
            }
            TextureFormat::R8Unorm => (glow::R8, glow::RED, glow::UNSIGNED_BYTE),
            TextureFormat::Depth24 => (
                glow::DEPTH_COMPONENT24,
                glow::DEPTH_COMPONENT,
                glow::UNSIGNED_INT,
            ),
            TextureFormat::Depth32Float => {
                (glow::DEPTH_COMPONENT32F, glow::DEPTH_COMPONENT, glow::FLOAT)
            }
            TextureFormat::Depth24Stencil8 => (
                glow::DEPTH24_STENCIL8,
                glow::DEPTH_STENCIL,
                glow::UNSIGNED_INT_24_8,
            ),
            _ => (glow::RGBA8, glow::RGBA, glow::UNSIGNED_BYTE),
        };

        let target = match descriptor.texture_type {
            TextureType::Texture2D => glow::TEXTURE_2D,
            TextureType::CubeMap => glow::TEXTURE_CUBE_MAP,
        };

        let min_filter = if descriptor.generate_mipmaps {
            glow::LINEAR_MIPMAP_LINEAR
        } else {
            match descriptor.min_filter {
                crate::rhi::Filter::Nearest => glow::NEAREST,
                crate::rhi::Filter::Linear => glow::LINEAR,
            }
        };
        let mag_filter = match descriptor.mag_filter {
            crate::rhi::Filter::Nearest => glow::NEAREST,
            crate::rhi::Filter::Linear => glow::LINEAR,
        };
        let (wrap_s, wrap_t) = match descriptor.wrap_u {
            WrapMode::Clamp => (glow::CLAMP_TO_EDGE, glow::CLAMP_TO_EDGE),
            WrapMode::Repeat => (glow::REPEAT, glow::REPEAT),
            WrapMode::MirrorRepeat => (glow::MIRRORED_REPEAT, glow::MIRRORED_REPEAT),
            WrapMode::Border => (glow::CLAMP_TO_BORDER, glow::CLAMP_TO_BORDER),
        };

        unsafe {
            gl.bind_texture(target, Some(id));
            match descriptor.texture_type {
                TextureType::Texture2D => {
                    gl.tex_image_2d(
                        target,
                        0,
                        internal_format as i32,
                        descriptor.width as i32,
                        descriptor.height as i32,
                        0,
                        format,
                        ty,
                        data,
                    );
                }
                TextureType::CubeMap => {
                    let bpp = descriptor.format.bytes_per_pixel();
                    let face_size = (descriptor.width * descriptor.height * bpp) as usize;
                    let bytes = data.unwrap_or(&[]);
                    for i in 0..6u32 {
                        let face_data = if bytes.len() >= (i as usize + 1) * face_size {
                            Some(&bytes[i as usize * face_size..(i as usize + 1) * face_size])
                        } else {
                            None
                        };
                        gl.tex_image_2d(
                            glow::TEXTURE_CUBE_MAP_POSITIVE_X + i,
                            0,
                            internal_format as i32,
                            descriptor.width as i32,
                            descriptor.height as i32,
                            0,
                            format,
                            ty,
                            face_data,
                        );
                    }
                }
            }
            gl.tex_parameter_i32(target, glow::TEXTURE_MIN_FILTER, min_filter as i32);
            gl.tex_parameter_i32(target, glow::TEXTURE_MAG_FILTER, mag_filter as i32);
            gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_S, wrap_s as i32);
            gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_T, wrap_t as i32);
            if descriptor.texture_type == TextureType::CubeMap {
                gl.tex_parameter_i32(target, glow::TEXTURE_WRAP_R, wrap_t as i32);
            }
            if descriptor.wrap_u == WrapMode::Border {
                gl.tex_parameter_f32_slice(
                    target,
                    glow::TEXTURE_BORDER_COLOR,
                    &[1.0, 1.0, 1.0, 1.0],
                );
            }
            if descriptor.generate_mipmaps {
                gl.generate_mipmap(target);
            }
            gl.bind_texture(target, None);
        }

        Ok(Self {
            gl: gl.clone(),
            id,
            target,
            width: descriptor.width,
            height: descriptor.height,
            depth: descriptor.depth,
            format: descriptor.format,
            mip_levels: descriptor.mip_levels,
        })
    }

    /// Returns the GL texture ID.
    pub fn id(&self) -> glow::NativeTexture {
        self.id
    }

    /// Binds this texture to the given unit.
    pub fn bind(&self, unit: u32) {
        unsafe {
            self.gl.active_texture(glow::TEXTURE0 + unit);
            self.gl.bind_texture(self.target, Some(self.id));
        }
    }
}

impl Drop for OpenGLTexture {
    fn drop(&mut self) {
        unsafe { self.gl.delete_texture(self.id) }
    }
}

impl Texture for OpenGLTexture {
    fn width(&self) -> u32 {
        self.width
    }

    fn height(&self) -> u32 {
        self.height
    }

    fn depth(&self) -> u32 {
        self.depth
    }

    fn format(&self) -> TextureFormat {
        self.format
    }

    fn mip_levels(&self) -> u32 {
        self.mip_levels
    }

    fn update(
        &mut self,
        data: &[u8],
        _mip_level: u32,
        _layer: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if self.target != glow::TEXTURE_2D {
            return Err("texture update is only supported for 2D textures".into());
        }
        let (format, ty) = match self.format {
            TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm => {
                (glow::RGBA, glow::UNSIGNED_BYTE)
            }
            TextureFormat::R8Unorm => (glow::RED, glow::UNSIGNED_BYTE),
            _ => (glow::RGBA, glow::UNSIGNED_BYTE),
        };
        unsafe {
            self.gl.bind_texture(self.target, Some(self.id));
            self.gl.tex_sub_image_2d(
                self.target,
                0,
                0,
                0,
                self.width as i32,
                self.height as i32,
                format,
                ty,
                glow::PixelUnpackData::Slice(data),
            );
            self.gl.bind_texture(self.target, None);
        }
        Ok(())
    }

    fn bind(&self, unit: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.bind(unit);
        Ok(())
    }

    fn native_handle(&self) -> u64 {
        self.id.0.get() as u64
    }
}
