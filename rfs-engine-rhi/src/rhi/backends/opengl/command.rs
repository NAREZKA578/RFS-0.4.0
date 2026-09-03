//! OpenGL Command Encoder implementation.
//!
//! OpenGL executes commands immediately, so this encoder issues GL calls
//! directly (no command buffer recording). It wraps the raw `glow::HasContext`
//! API (all GL calls are `unsafe`) and keeps the small amount of state needed
//! to translate RHI concepts (render passes, pipelines, vertex layouts,
//! uniforms) into raw GL state.

use crate::rhi::command::{
    ColorAttachment, CommandEncoder, DepthAttachment, LoadOp, PrimitiveTopology,
};
use crate::rhi::texture::TextureFormat;
use crate::rhi::types::{Rect, VertexFormat, VertexLayout};
use crate::rhi::{Buffer, Pipeline, Texture};
use glow::{
    HasContext, NativeBuffer, NativeFramebuffer, NativeProgram, NativeTexture, NativeVertexArray,
    PixelUnpackData, UniformLocation,
};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::Arc;

/// OpenGL command encoder.
pub struct OpenGLCommandEncoder {
    gl: Arc<glow::Context>,
    /// Lazily-created shared vertex array object.
    vao: Option<NativeVertexArray>,
    /// Handle of the currently bound program (0 = none).
    current_program: u64,
    /// Vertex layout of the currently bound pipeline.
    current_layout: Option<VertexLayout>,
    /// Primitive topology for draws.
    topology: PrimitiveTopology,
    /// Uniform location cache per program.
    uniforms: HashMap<u64, HashMap<String, Option<UniformLocation>>>,
    /// FBO created by the active render pass (if any).
    fbo: Option<NativeFramebuffer>,
}

impl OpenGLCommandEncoder {
    /// Creates a new encoder bound to the given GL context.
    pub fn new(gl: Arc<glow::Context>) -> Self {
        Self {
            gl,
            vao: None,
            current_program: 0,
            current_layout: None,
            topology: PrimitiveTopology::TriangleList,
            uniforms: HashMap::new(),
            fbo: None,
        }
    }

    /// Converts a backend-agnostic u64 handle into a GL handle.
    fn buffer_from_handle(handle: u64) -> Option<NativeBuffer> {
        Some(NativeBuffer(NonZeroU32::new(handle as u32)?))
    }

    fn texture_from_handle(handle: u64) -> Option<NativeTexture> {
        Some(NativeTexture(NonZeroU32::new(handle as u32)?))
    }

    fn program_from_handle(handle: u64) -> Option<NativeProgram> {
        Some(NativeProgram(NonZeroU32::new(handle as u32)?))
    }

    /// Ensures a VAO exists and is bound.
    fn ensure_vao(&mut self) -> Result<(), String> {
        if self.vao.is_none() {
            let vao = unsafe { self.gl.create_vertex_array() }?;
            self.vao = Some(vao);
        }
        unsafe { self.gl.bind_vertex_array(self.vao) };
        Ok(())
    }

    /// Returns (and caches) the uniform location for a name in the current program.
    fn uniform_location(&mut self, name: &str) -> Option<UniformLocation> {
        if self.current_program == 0 {
            return None;
        }
        let program = Self::program_from_handle(self.current_program)?;
        let cache = self.uniforms.entry(self.current_program).or_default();
        if !cache.contains_key(name) {
            let loc = unsafe { self.gl.get_uniform_location(program, name) };
            cache.insert(name.to_string(), loc);
        }
        cache.get(name).copied().flatten()
    }

    /// Maps an RHI vertex format to (component count, GL type, normalized).
    fn format_to_gl(format: VertexFormat) -> (i32, u32, bool) {
        match format {
            VertexFormat::Float1 => (1, glow::FLOAT, false),
            VertexFormat::Float2 => (2, glow::FLOAT, false),
            VertexFormat::Float3 => (3, glow::FLOAT, false),
            VertexFormat::Float4 => (4, glow::FLOAT, false),
            VertexFormat::Uint2 => (2, glow::UNSIGNED_SHORT, false),
            VertexFormat::Uint4 => (4, glow::UNSIGNED_BYTE, false),
            VertexFormat::Snorm4 => (4, glow::BYTE, true),
        }
    }

    /// Maps a primitive topology to its GL enum.
    fn topology_to_gl(topology: PrimitiveTopology) -> u32 {
        match topology {
            PrimitiveTopology::PointList => glow::POINTS,
            PrimitiveTopology::LineList => glow::LINES,
            PrimitiveTopology::LineStrip => glow::LINE_STRIP,
            PrimitiveTopology::TriangleList => glow::TRIANGLES,
            PrimitiveTopology::TriangleStrip => glow::TRIANGLE_STRIP,
        }
    }

    /// Maps a texture format to (pixel format, pixel type) for uploads.
    fn format_to_pixel(format: TextureFormat) -> (u32, u32) {
        match format {
            TextureFormat::Rgba8Unorm => (glow::RGBA, glow::UNSIGNED_BYTE),
            TextureFormat::Bgra8Unorm => (glow::BGRA, glow::UNSIGNED_BYTE),
            TextureFormat::R8Unorm => (glow::RED, glow::UNSIGNED_BYTE),
            TextureFormat::Rgba16Float => (glow::RGBA, glow::HALF_FLOAT),
            TextureFormat::Rgba32Float => (glow::RGBA, glow::FLOAT),
            TextureFormat::R16Float => (glow::RED, glow::HALF_FLOAT),
            _ => (glow::RGBA, glow::UNSIGNED_BYTE),
        }
    }
}

impl CommandEncoder for OpenGLCommandEncoder {
    fn begin_render_pass(
        &mut self,
        color_attachments: &[ColorAttachment],
        depth_attachment: Option<&DepthAttachment>,
    ) {
        // Render to the default framebuffer when there are no attachments
        // or the primary color attachment is the default framebuffer (0).
        let primary = color_attachments
            .first()
            .map(|a| a.texture_handle)
            .unwrap_or(0);
        let use_fbo = primary != 0 && color_attachments.iter().any(|a| a.texture_handle != 0);

        if use_fbo {
            // Clean up previous FBO to prevent leak
            if let Some(old_fbo) = self.fbo.take() {
                unsafe { self.gl.delete_framebuffer(old_fbo) };
            }
            let fbo = match unsafe { self.gl.create_framebuffer() } {
                Ok(fbo) => fbo,
                Err(e) => {
                    tracing::warn!("Failed to create render pass framebuffer: {}", e);
                    return;
                }
            };
            self.fbo = Some(fbo);
            unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo)) };

            for (i, att) in color_attachments.iter().enumerate() {
                if att.texture_handle == 0 {
                    continue;
                }
                if let Some(tex) = Self::texture_from_handle(att.texture_handle) {
                    unsafe {
                        self.gl.framebuffer_texture_2d(
                            glow::FRAMEBUFFER,
                            glow::COLOR_ATTACHMENT0 + i as u32,
                            glow::TEXTURE_2D,
                            Some(tex),
                            0,
                        );
                    }
                }
            }

            if let Some(depth) = depth_attachment {
                if let Some(tex) = Self::texture_from_handle(depth.texture_handle) {
                    unsafe {
                        self.gl.framebuffer_texture_2d(
                            glow::FRAMEBUFFER,
                            glow::DEPTH_ATTACHMENT,
                            glow::TEXTURE_2D,
                            Some(tex),
                            0,
                        );
                    }
                }
            }

            let draw_bufs: Vec<u32> = color_attachments
                .iter()
                .enumerate()
                .filter(|(_, a)| a.texture_handle != 0)
                .map(|(i, _)| glow::COLOR_ATTACHMENT0 + i as u32)
                .collect();
            if draw_bufs.is_empty() {
                unsafe {
                    self.gl.draw_buffer(glow::NONE);
                    self.gl.read_buffer(glow::NONE);
                }
            } else {
                unsafe { self.gl.draw_buffers(&draw_bufs) };
            }

            let status = unsafe { self.gl.check_framebuffer_status(glow::FRAMEBUFFER) };
            if status != glow::FRAMEBUFFER_COMPLETE {
                tracing::warn!("Render pass framebuffer is incomplete; falling back to default");
                unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
                if let Some(bad_fbo) = self.fbo.take() {
                    unsafe { self.gl.delete_framebuffer(bad_fbo) };
                }
                return;
            }
        } else {
            unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
        }

        // Apply clear operations.
        let mut clear_mask = 0u32;
        let mut clear_color_set = false;
        for att in color_attachments {
            if att.load_op == LoadOp::Clear {
                if !clear_color_set {
                    let c = att.clear_color;
                    unsafe { self.gl.clear_color(c.r, c.g, c.b, c.a) };
                    clear_color_set = true;
                }
                clear_mask |= glow::COLOR_BUFFER_BIT;
            }
        }
        if let Some(depth) = depth_attachment {
            if depth.depth_load_op == LoadOp::Clear {
                clear_mask |= glow::DEPTH_BUFFER_BIT;
            }
        }
        if clear_mask != 0 {
            unsafe { self.gl.clear(clear_mask) };
        }
    }

    fn end_render_pass(&mut self) {
        if let Some(fbo) = self.fbo.take() {
            unsafe { self.gl.bind_framebuffer(glow::FRAMEBUFFER, None) };
            unsafe { self.gl.delete_framebuffer(fbo) };
        }
        // Restore default-framebuffer draw/read targets: draw buffer state is
        // context state, so a depth-only pass (draw_buffer(NONE)) would leave
        // the default framebuffer unwritable otherwise.
        unsafe {
            self.gl.draw_buffer(glow::BACK);
            self.gl.read_buffer(glow::BACK);
            // Reset global state applied by pipelines so code that runs after
            // the encoder (e.g. the immediate-mode UI renderer) starts from a
            // clean GL state.
            self.gl.disable(glow::CULL_FACE);
            self.gl.disable(glow::DEPTH_TEST);
            self.gl.disable(glow::BLEND);
            self.gl.disable(glow::SCISSOR_TEST);
            self.gl.bind_vertex_array(None);
        }
    }

    fn bind_pipeline(&mut self, pipeline: &dyn Pipeline) {
        if let Err(e) = pipeline.bind() {
            tracing::warn!("Pipeline bind failed: {}", e);
            return;
        }
        self.current_program = pipeline.native_handle();
        self.current_layout = Some(pipeline.vertex_layout().clone());
    }

    fn bind_vertex_buffer(&mut self, binding: u32, buffer: &dyn Buffer) {
        if binding != 0 {
            tracing::warn!(
                "bind_vertex_buffer: binding {} ignored (OpenGL 3.3 supports a single binding)",
                binding
            );
        }
        if let Err(e) = self.ensure_vao() {
            tracing::warn!("Failed to create VAO: {}", e);
            return;
        }
        let Some(id) = Self::buffer_from_handle(buffer.native_handle()) else {
            return;
        };
        unsafe { self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(id)) };

        if let Some(layout) = &self.current_layout {
            for attr in &layout.attributes {
                let (size, ty, normalized) = Self::format_to_gl(attr.format);
                unsafe {
                    self.gl.enable_vertex_attrib_array(attr.location);
                    self.gl.vertex_attrib_pointer_f32(
                        attr.location,
                        size,
                        ty,
                        normalized,
                        layout.stride as i32,
                        attr.offset as i32,
                    );
                }
            }
        }
    }

    fn bind_instance_buffer(&mut self, first_location: u32, buffer: &dyn Buffer, stride: u32) {
        if let Err(e) = self.ensure_vao() {
            tracing::warn!("Failed to create VAO: {}", e);
            return;
        }
        let Some(id) = Self::buffer_from_handle(buffer.native_handle()) else {
            return;
        };
        unsafe { self.gl.bind_buffer(glow::ARRAY_BUFFER, Some(id)) };
        for i in 0..4u32 {
            let loc = first_location + i;
            unsafe {
                self.gl.enable_vertex_attrib_array(loc);
                self.gl.vertex_attrib_pointer_f32(
                    loc,
                    4,
                    glow::FLOAT,
                    false,
                    stride as i32,
                    (i * 16) as i32,
                );
                self.gl.vertex_attrib_divisor(loc, 1);
            }
        }
    }

    fn bind_index_buffer(&mut self, buffer: &dyn Buffer) {
        if let Err(e) = self.ensure_vao() {
            tracing::warn!("Failed to create VAO: {}", e);
            return;
        }
        if let Some(id) = Self::buffer_from_handle(buffer.native_handle()) {
            unsafe { self.gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, Some(id)) };
        }
    }

    fn bind_uniform_buffer(&mut self, binding: u32, buffer: &dyn Buffer) {
        if let Some(id) = Self::buffer_from_handle(buffer.native_handle()) {
            unsafe {
                self.gl.bind_buffer(glow::UNIFORM_BUFFER, Some(id));
                self.gl
                    .bind_buffer_base(glow::UNIFORM_BUFFER, binding, Some(id));
            }
        }
    }

    fn bind_texture(&mut self, binding: u32, texture: &dyn Texture) {
        if let Err(e) = texture.bind(binding) {
            tracing::warn!("Texture bind failed: {}", e);
        }
    }

    fn set_uniform_f32(&mut self, name: &str, x: f32) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_1_f32(Some(&loc), x) };
        }
    }

    fn set_uniform_vec2(&mut self, name: &str, x: f32, y: f32) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_2_f32(Some(&loc), x, y) };
        }
    }

    fn set_uniform_vec3(&mut self, name: &str, x: f32, y: f32, z: f32) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_3_f32(Some(&loc), x, y, z) };
        }
    }

    fn set_uniform_vec4(&mut self, name: &str, x: f32, y: f32, z: f32, w: f32) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_4_f32(Some(&loc), x, y, z, w) };
        }
    }

    fn set_uniform_i32(&mut self, name: &str, x: i32) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_1_i32(Some(&loc), x) };
        }
    }

    fn set_uniform_mat3(&mut self, name: &str, value: &[f32; 9]) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_matrix_3_f32_slice(Some(&loc), false, value) };
        }
    }

    fn set_uniform_mat4(&mut self, name: &str, value: &[f32; 16]) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_matrix_4_f32_slice(Some(&loc), false, value) };
        }
    }

    fn set_uniform_texture(&mut self, name: &str, unit: u32) {
        if let Some(loc) = self.uniform_location(name) {
            unsafe { self.gl.uniform_1_i32(Some(&loc), unit as i32) };
        }
    }

    fn set_viewport(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        min_depth: f32,
        max_depth: f32,
    ) {
        unsafe {
            self.gl
                .viewport(x as i32, y as i32, width as i32, height as i32);
            self.gl.depth_range_f32(min_depth, max_depth);
        }
    }

    fn set_scissor(&mut self, rect: &Rect) {
        unsafe {
            self.gl.enable(glow::SCISSOR_TEST);
            self.gl
                .scissor(rect.x, rect.y, rect.width as i32, rect.height as i32);
        }
    }

    fn set_primitive_topology(&mut self, topology: PrimitiveTopology) {
        self.topology = topology;
    }

    fn draw(&mut self, vertices: u32, instances: u32, first_vertex: u32, _first_instance: u32) {
        if let Err(e) = self.ensure_vao() {
            tracing::warn!("Failed to create VAO: {}", e);
            return;
        }
        let mode = Self::topology_to_gl(self.topology);
        if instances <= 1 {
            unsafe {
                self.gl
                    .draw_arrays(mode, first_vertex as i32, vertices as i32)
            };
        } else {
            unsafe {
                self.gl.draw_arrays_instanced(
                    mode,
                    first_vertex as i32,
                    vertices as i32,
                    instances as i32,
                )
            };
        }
    }

    fn draw_indexed(
        &mut self,
        indices: u32,
        instances: u32,
        first_index: u32,
        _vertex_offset: i32,
        _first_instance: u32,
    ) {
        if let Err(e) = self.ensure_vao() {
            tracing::warn!("Failed to create VAO: {}", e);
            return;
        }
        let mode = Self::topology_to_gl(self.topology);
        let offset = (first_index * std::mem::size_of::<u32>() as u32) as i32;
        if instances <= 1 {
            unsafe {
                self.gl
                    .draw_elements(mode, indices as i32, glow::UNSIGNED_INT, offset)
            };
        } else {
            unsafe {
                self.gl.draw_elements_instanced(
                    mode,
                    indices as i32,
                    glow::UNSIGNED_INT,
                    offset,
                    instances as i32,
                )
            };
        }
    }

    fn update_buffer(&mut self, buffer: &mut dyn Buffer, data: &[u8], offset: u64) {
        if let Err(e) = buffer.update(data, offset) {
            tracing::warn!("Buffer update failed: {}", e);
        }
    }

    fn copy_buffer_to_buffer(
        &mut self,
        src: &dyn Buffer,
        dst: &dyn Buffer,
        src_offset: u64,
        dst_offset: u64,
        size: u64,
    ) {
        let (Some(src_id), Some(dst_id)) = (
            Self::buffer_from_handle(src.native_handle()),
            Self::buffer_from_handle(dst.native_handle()),
        ) else {
            return;
        };
        unsafe {
            self.gl.bind_buffer(glow::COPY_READ_BUFFER, Some(src_id));
            self.gl.bind_buffer(glow::COPY_WRITE_BUFFER, Some(dst_id));
            self.gl.copy_buffer_sub_data(
                glow::COPY_READ_BUFFER,
                glow::COPY_WRITE_BUFFER,
                src_offset as i32,
                dst_offset as i32,
                size as i32,
            );
            self.gl.bind_buffer(glow::COPY_READ_BUFFER, None);
            self.gl.bind_buffer(glow::COPY_WRITE_BUFFER, None);
        }
    }

    fn copy_texture_to_texture(&mut self, src: &dyn Texture, dst: &dyn Texture) {
        let (Some(src_id), Some(dst_id)) = (
            Self::texture_from_handle(src.native_handle()),
            Self::texture_from_handle(dst.native_handle()),
        ) else {
            return;
        };
        let w = src.width() as i32;
        let h = src.height() as i32;

        // Determine if this is a depth copy based on source format
        let is_depth = src.format().is_depth();
        let attachment = if is_depth {
            glow::DEPTH_ATTACHMENT
        } else {
            glow::COLOR_ATTACHMENT0
        };
        let buffer_bit = if is_depth {
            glow::DEPTH_BUFFER_BIT
        } else {
            glow::COLOR_BUFFER_BIT
        };

        let (Ok(src_fbo), Ok(dst_fbo)) = (unsafe { self.gl.create_framebuffer() }, unsafe {
            self.gl.create_framebuffer()
        }) else {
            return;
        };

        unsafe {
            self.gl
                .bind_framebuffer(glow::READ_FRAMEBUFFER, Some(src_fbo));
            self.gl.framebuffer_texture_2d(
                glow::READ_FRAMEBUFFER,
                attachment,
                glow::TEXTURE_2D,
                Some(src_id),
                0,
            );
            self.gl
                .bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(dst_fbo));
            self.gl.framebuffer_texture_2d(
                glow::DRAW_FRAMEBUFFER,
                attachment,
                glow::TEXTURE_2D,
                Some(dst_id),
                0,
            );
            self.gl
                .blit_framebuffer(0, 0, w, h, 0, 0, w, h, buffer_bit, glow::NEAREST);
            self.gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            self.gl.delete_framebuffer(src_fbo);
            self.gl.delete_framebuffer(dst_fbo);
        }
    }

    fn copy_buffer_to_texture(&mut self, src: &dyn Buffer, dst: &dyn Texture) {
        let Some(src_id) = Self::buffer_from_handle(src.native_handle()) else {
            return;
        };
        let (format, ty) = Self::format_to_pixel(dst.format());
        unsafe {
            self.gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, Some(src_id));
            self.gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                0,
                0,
                dst.width() as i32,
                dst.height() as i32,
                format,
                ty,
                PixelUnpackData::BufferOffset(0),
            );
            self.gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
        }
    }

    fn push_debug_group(&mut self, _name: &str) {
        // OpenGL 3.3 core has no debug groups; no-op.
    }

    fn pop_debug_group(&mut self) {
        // OpenGL 3.3 core has no debug groups; no-op.
    }
}
