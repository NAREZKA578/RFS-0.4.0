use crate::rhi::surface::{Surface, SurfaceDescriptor};

/// OpenGL surface.
pub struct OpenGLSurface {
    descriptor: SurfaceDescriptor,
}

impl OpenGLSurface {
    pub fn new(descriptor: SurfaceDescriptor) -> Self {
        Self { descriptor }
    }
}

impl Surface for OpenGLSurface {
    fn size(&self) -> crate::rhi::types::Extent2D {
        crate::rhi::types::Extent2D::new(self.descriptor.width, self.descriptor.height)
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.descriptor.width = width;
        self.descriptor.height = height;
    }

    fn native_handle(&self) -> u64 {
        0
    }
}
