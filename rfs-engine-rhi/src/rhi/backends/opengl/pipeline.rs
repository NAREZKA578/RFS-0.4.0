//! OpenGL Pipeline implementation.

use crate::rhi::pipeline::{
    BlendMode, CompareFunc, CullMode, DepthState, FrontFace, Pipeline as PipelineTrait,
    PipelineDescriptor, PolygonMode,
};
use glow::{HasContext, NativeProgram, NativeShader};
use std::num::NonZeroU32;
use std::sync::Arc;

/// OpenGL pipeline state.
pub struct OpenGLPipeline {
    gl: Arc<glow::Context>,
    program: Option<NativeProgram>,
    vertex_layout: crate::rhi::types::VertexLayout,
    blend_mode: BlendMode,
    cull_mode: CullMode,
    front_face: FrontFace,
    /// Polygon fill mode (kept for descriptor parity with other backends).
    #[allow(dead_code)]
    polygon_mode: PolygonMode,
    depth_state: DepthState,
}

impl OpenGLPipeline {
    /// Creates a new OpenGL pipeline state, linking the vertex/fragment
    /// shaders referenced by `descriptor` into a program.
    pub fn new(
        gl: &Arc<glow::Context>,
        descriptor: &PipelineDescriptor,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let program = if descriptor.vertex_shader != 0 && descriptor.fragment_shader != 0 {
            let vs = NativeShader(
                NonZeroU32::new(descriptor.vertex_shader as u32)
                    .ok_or("invalid vertex shader handle")?,
            );
            let fs = NativeShader(
                NonZeroU32::new(descriptor.fragment_shader as u32)
                    .ok_or("invalid fragment shader handle")?,
            );

            let program = unsafe { gl.create_program()? };
            unsafe {
                gl.attach_shader(program, vs);
                gl.attach_shader(program, fs);
                gl.link_program(program);
            }
            if !unsafe { gl.get_program_link_status(program) } {
                let log = unsafe { gl.get_program_info_log(program) };
                unsafe {
                    gl.delete_program(program);
                }
                return Err(format!("Program link failed: {}", log).into());
            }
            // Shaders can be deleted after successful linking.
            unsafe {
                gl.delete_shader(vs);
                gl.delete_shader(fs);
            }
            Some(program)
        } else {
            None
        };

        Ok(Self {
            gl: gl.clone(),
            program,
            vertex_layout: descriptor.vertex_layout.clone(),
            blend_mode: descriptor.blend_mode,
            cull_mode: descriptor.cull_mode,
            front_face: descriptor.front_face,
            polygon_mode: descriptor.polygon_mode,
            depth_state: descriptor.depth_state.clone(),
        })
    }

    /// Returns the linked program, if any.
    pub fn program(&self) -> Option<NativeProgram> {
        self.program
    }

    /// Applies the pipeline state to the GL context.
    pub fn apply(&self, gl: &glow::Context) {
        if let Some(program) = self.program {
            unsafe {
                gl.use_program(Some(program));
            }
        }
        // Apply blend mode
        unsafe {
            match self.blend_mode {
                BlendMode::None => gl.disable(glow::BLEND),
                BlendMode::Alpha => {
                    gl.enable(glow::BLEND);
                    gl.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
                }
                BlendMode::Additive => {
                    gl.enable(glow::BLEND);
                    gl.blend_func(glow::SRC_ALPHA, glow::ONE);
                }
                BlendMode::Premultiplied => {
                    gl.enable(glow::BLEND);
                    gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
                }
            }

            // Apply cull mode
            match self.cull_mode {
                CullMode::None => gl.disable(glow::CULL_FACE),
                CullMode::Front => {
                    gl.enable(glow::CULL_FACE);
                    gl.cull_face(glow::FRONT);
                }
                CullMode::Back => {
                    gl.enable(glow::CULL_FACE);
                    gl.cull_face(glow::BACK);
                }
                CullMode::FrontAndBack => {
                    gl.enable(glow::CULL_FACE);
                    gl.cull_face(glow::FRONT_AND_BACK);
                }
            }

            // Apply front face
            match self.front_face {
                FrontFace::Ccw => gl.front_face(glow::CCW),
                FrontFace::Cw => gl.front_face(glow::CW),
            }

            // Apply depth state
            if self.depth_state.enabled {
                gl.enable(glow::DEPTH_TEST);
                let func = match self.depth_state.compare_func {
                    CompareFunc::Never => glow::NEVER,
                    CompareFunc::Less => glow::LESS,
                    CompareFunc::Equal => glow::EQUAL,
                    CompareFunc::LessEqual => glow::LEQUAL,
                    CompareFunc::Greater => glow::GREATER,
                    CompareFunc::NotEqual => glow::NOTEQUAL,
                    CompareFunc::GreaterEqual => glow::GEQUAL,
                    CompareFunc::Always => glow::ALWAYS,
                };
                gl.depth_func(func);
            } else {
                gl.disable(glow::DEPTH_TEST);
            }
            gl.depth_mask(self.depth_state.write_enabled);
        }
    }
}

impl Drop for OpenGLPipeline {
    fn drop(&mut self) {
        if let Some(program) = self.program {
            unsafe { self.gl.delete_program(program) }
        }
    }
}

impl PipelineTrait for OpenGLPipeline {
    fn vertex_layout(&self) -> &crate::rhi::types::VertexLayout {
        &self.vertex_layout
    }

    fn blend_mode(&self) -> BlendMode {
        self.blend_mode
    }

    fn cull_mode(&self) -> CullMode {
        self.cull_mode
    }

    fn bind(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.apply(&self.gl);
        Ok(())
    }

    fn native_handle(&self) -> u64 {
        self.program.map(|p| p.0.get() as u64).unwrap_or(0)
    }
}
