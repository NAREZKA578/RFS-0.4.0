//! OpenGL Buffer implementation.

use crate::rhi::{Buffer, BufferDescriptor, BufferUsage};
use glow::HasContext;
use std::sync::Arc;

/// OpenGL buffer.
pub struct OpenGLBuffer {
    gl: Arc<glow::Context>,
    id: glow::NativeBuffer,
    size: u64,
    usage: BufferUsage,
    target: u32,
}

impl OpenGLBuffer {
    /// Creates a new OpenGL buffer.
    pub fn new(
        gl: &Arc<glow::Context>,
        descriptor: &BufferDescriptor,
        data: Option<&[u8]>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let target = match descriptor.usage {
            BufferUsage::Vertex => glow::ARRAY_BUFFER,
            BufferUsage::Index => glow::ELEMENT_ARRAY_BUFFER,
            BufferUsage::Uniform => glow::UNIFORM_BUFFER,
            BufferUsage::Storage => glow::SHADER_STORAGE_BUFFER,
            _ => glow::ARRAY_BUFFER,
        };

        let id = unsafe { gl.create_buffer()? };
        unsafe {
            gl.bind_buffer(target, Some(id));
            let usage = if descriptor.dynamic {
                glow::DYNAMIC_DRAW
            } else {
                glow::STATIC_DRAW
            };
            let data = data.unwrap_or(&[]);
            if data.is_empty() {
                gl.buffer_data_size(target, descriptor.size as i32, usage);
            } else {
                gl.buffer_data_u8_slice(target, data, usage);
            }
            gl.bind_buffer(target, None);
        }

        Ok(Self {
            gl: gl.clone(),
            id,
            size: descriptor.size,
            usage: descriptor.usage,
            target,
        })
    }

    /// Returns the GL buffer ID.
    pub fn id(&self) -> glow::NativeBuffer {
        self.id
    }

    /// Returns the GL target for this buffer.
    pub fn gl_target(&self) -> u32 {
        self.target
    }
}

impl Drop for OpenGLBuffer {
    fn drop(&mut self) {
        unsafe { self.gl.delete_buffer(self.id) }
    }
}

impl Buffer for OpenGLBuffer {
    fn size(&self) -> u64 {
        self.size
    }

    fn usage(&self) -> BufferUsage {
        self.usage
    }

    fn update(&self, data: &[u8], offset: u64) -> Result<(), Box<dyn std::error::Error>> {
        unsafe {
            self.gl.bind_buffer(self.target, Some(self.id));
            self.gl
                .buffer_sub_data_u8_slice(self.target, offset as i32, data);
            self.gl.bind_buffer(self.target, None);
        }
        Ok(())
    }

    fn map(&mut self) -> Result<&mut [u8], Box<dyn std::error::Error>> {
        Err("OpenGL buffer mapping not implemented".into())
    }

    fn unmap(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn native_handle(&self) -> u64 {
        self.id.0.get() as u64
    }
}
