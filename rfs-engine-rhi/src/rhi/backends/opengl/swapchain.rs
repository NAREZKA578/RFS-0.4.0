use crate::rhi::swapchain::{SwapChain, SwapChainDescriptor};

/// OpenGL swap chain.
pub struct OpenGLSwapChain {
    descriptor: SwapChainDescriptor,
}

impl OpenGLSwapChain {
    pub fn new(
        _surface: &dyn crate::rhi::surface::Surface,
        descriptor: SwapChainDescriptor,
    ) -> Self {
        Self { descriptor }
    }
}

impl SwapChain for OpenGLSwapChain {
    fn current_image_index(&self) -> u32 {
        0
    }

    fn image_count(&self) -> u32 {
        self.descriptor.image_count
    }

    fn get_image(&self, _index: u32) -> &dyn crate::rhi::texture::Texture {
        panic!("OpenGL swap chain images are not accessible as textures")
    }

    fn current_image(&self) -> &dyn crate::rhi::texture::Texture {
        panic!("OpenGL swap chain images are not accessible as textures")
    }

    fn acquire_next_image(&mut self) -> Result<u32, Box<dyn std::error::Error>> {
        Ok(0)
    }

    fn present(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn resize(&mut self, width: u32, height: u32) -> Result<(), Box<dyn std::error::Error>> {
        self.descriptor.width = width;
        self.descriptor.height = height;
        Ok(())
    }

    fn size(&self) -> crate::rhi::types::Extent2D {
        crate::rhi::types::Extent2D::new(self.descriptor.width, self.descriptor.height)
    }
}
