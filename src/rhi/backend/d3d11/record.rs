//! Direct3D 11 Command Translation
//!
//! Replays a recorded [`Command`] list on the immediate context. D3D11 has no
//! command buffers: one submit replays in order on the shared immediate
//! context, and the per-submit recorder state starts fresh. Backend object
//! state that D3D11 itself carries (pipeline binds, render targets, the index
//! buffer) lives on the context across submits; the recorder only re-derives
//! what it validates.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use super::convert;
use super::device::{BufferEntry, DxCom, D3D11Device, D3DObject};
use crate::command::commands::Command;
use crate::command::pass::render::RenderPassBeginInfo;
use crate::error::{RhiError, RhiResult};
use crate::pipeline::compute::ComputePipeline;
use crate::pipeline::graphics::GraphicsPipeline;
use crate::resource::{Buffer, TextureView};
use crate::types::*;
use std::collections::HashMap;
use windows::core::Interface;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CLEAR_DEPTH, D3D11_CLEAR_STENCIL, D3D11_VIEWPORT,
    ID3D11DepthStencilView, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Resource,
};

pub(crate) struct Recorder {
    rtvs: Vec<DxCom<ID3D11RenderTargetView>>,
    dsv: Option<DxCom<ID3D11DepthStencilView>>,
    width: u32,
    height: u32,
    has_index_buffer: bool,
    vertex_strides: HashMap<u32, u32>,
}

impl Recorder {
    pub(crate) fn new() -> Self {
        Self {
            rtvs: Vec::new(),
            dsv: None,
            width: 0,
            height: 0,
            has_index_buffer: false,
            vertex_strides: HashMap::new(),
        }
    }

    pub(crate) fn execute(&mut self, device: &D3D11Device, command: &Command) -> RhiResult<()> {
        let ctx = device.context();
        match command {
            Command::BindGraphicsPipeline(pipeline) => {
                self.bind_graphics_pipeline(device, ctx, pipeline)
            }
            Command::BindComputePipeline(pipeline) => {
                self.bind_compute_pipeline(device, ctx, pipeline)
            }
            Command::BindDescriptorSets {
                pipeline: _,
                first_set,
                sets,
            } => {
                if *first_set != 0 {
                    return Err(RhiError::NotSupported(
                        "the D3D11 recorder expects descriptor sets at first_set 0".into(),
                    ));
                }
                for set in sets {
                    let Some(set_gpu) = set.backend() else {
                        return Err(RhiError::BackendError(
                            "bound descriptor set has no GPU backing".into(),
                        ));
                    };
                    for write in device.set_writes(set_gpu.handle) {
                        device.bind_graphics_slot(ctx, write.stages, &write)?;
                    }
                }
                Ok(())
            }
            Command::BindComputeDescriptorSets {
                pipeline: _,
                first_set,
                sets,
            } => {
                if *first_set != 0 {
                    return Err(RhiError::NotSupported(
                        "the D3D11 recorder expects descriptor sets at first_set 0".into(),
                    ));
                }
                for set in sets {
                    let Some(set_gpu) = set.backend() else {
                        return Err(RhiError::BackendError(
                            "bound descriptor set has no GPU backing".into(),
                        ));
                    };
                    for write in device.set_writes(set_gpu.handle) {
                        device.bind_compute_slot(ctx, &write)?;
                    }
                }
                Ok(())
            }
            Command::BindVertexBuffers {
                first_binding,
                buffers,
            } => {
                let mut views = Vec::new();
                let mut strides = Vec::new();
                let mut offsets = Vec::new();
                for (index, (buffer, offset)) in buffers.iter().enumerate() {
                    let slot = first_binding + index as u32;
                    let entry = buffer_entry(device, buffer)?;
                    if *offset > u32::MAX as u64 {
                        return Err(RhiError::NotSupported(
                            "a D3D11 vertex buffer offset does not fit 32 bits".into(),
                        ));
                    }
                    let stride = self.vertex_strides.get(&slot).copied().ok_or_else(|| {
                        RhiError::BackendError(format!(
                            "no stride was declared for vertex binding {slot} by the \
                             bound pipeline"
                        ))
                    })?;
                    views.push(Some(entry.buffer.0.clone()));
                    strides.push(stride);
                    offsets.push(*offset as u32);
                }
                unsafe {
                    ctx.IASetVertexBuffers(
                        *first_binding,
                        buffers.len() as u32,
                        Some(views.as_ptr()),
                        Some(strides.as_ptr()),
                        Some(offsets.as_ptr()),
                    )
                };
                Ok(())
            }
            Command::BindIndexBuffer {
                buffer,
                offset,
                index_type,
            } => {
                if *offset > u32::MAX as u64 {
                    return Err(RhiError::NotSupported(
                        "a D3D11 index buffer offset does not fit 32 bits".into(),
                    ));
                }
                let entry = buffer_entry(device, buffer)?;
                unsafe {
                    ctx.IASetIndexBuffer(
                        Some(&entry.buffer.0),
                        convert::d3d_index_format(*index_type),
                        *offset as u32,
                    )
                };
                self.has_index_buffer = true;
                Ok(())
            }
            Command::SetViewport(viewport) => {
                let vp = [D3D11_VIEWPORT {
                    TopLeftX: viewport.x,
                    TopLeftY: viewport.y,
                    Width: viewport.width,
                    Height: viewport.height,
                    MinDepth: viewport.min_depth,
                    MaxDepth: viewport.max_depth,
                }];
                unsafe { ctx.RSSetViewports(Some(&vp)) };
                Ok(())
            }
            Command::SetScissor(scissors) => {
                unsafe { ctx.RSSetScissorRects(Some(&iter_rects(std::slice::from_ref(scissors)))) };
                Ok(())
            }
            Command::Draw {
                vertex_count,
                instance_count,
                first_vertex,
                first_instance,
            } => {
                self.require_pass()?;
                if *first_instance != 0 {
                    return Err(RhiError::NotSupported(
                        "the D3D11 recorder has no per-instance VB offset path for \
                         a non-zero first_instance"
                            .into(),
                    ));
                }
                unsafe {
                    if *instance_count <= 1 {
                        ctx.Draw(*vertex_count, *first_vertex);
                    } else {
                        ctx.DrawInstanced(*vertex_count, *instance_count, *first_vertex, 0);
                    }
                }
                Ok(())
            }
            Command::DrawIndexed {
                index_count,
                instance_count,
                first_index,
                vertex_offset,
                first_instance,
            } => {
                self.require_pass()?;
                if !self.has_index_buffer {
                    return Err(RhiError::BackendError(
                        "an indexed draw needs an index buffer bound".into(),
                    ));
                }
                if *first_instance != 0 {
                    return Err(RhiError::NotSupported(
                        "the D3D11 recorder has no per-instance VB offset path for \
                         a non-zero first_instance"
                            .into(),
                    ));
                }
                unsafe {
                    if *instance_count <= 1 {
                        ctx.DrawIndexed(*index_count, *first_index, *vertex_offset);
                    } else {
                        ctx.DrawIndexedInstanced(
                            *index_count,
                            *instance_count,
                            *first_index,
                            *vertex_offset,
                            0,
                        );
                    }
                }
                Ok(())
            }
            Command::Dispatch { x, y, z } => {
                if !self.rtvs.is_empty() || self.dsv.is_some() {
                    return Err(RhiError::BackendError(
                        "a compute dispatch inside a render pass is not a D3D11 \
                         operation"
                            .into(),
                    ));
                }
                unsafe { ctx.Dispatch(*x, *y, *z) };
                Ok(())
            }
            Command::ClearColor {
                texture,
                value,
                rects,
            } => {
                if !rects.is_empty() {
                    return Err(RhiError::NotSupported(
                        "partial colour clears need D3D11 ClearView, which the \
                         recorder does not model yet"
                            .into(),
                    ));
                }
                let ClearValue::Color { r, g, b, a } = value else {
                    return Err(RhiError::BackendError(
                        "a colour clear takes a colour value".into(),
                    ));
                };
                let rtv = view_rtv(device, texture)?;
                let color = [*r, *g, *b, *a];
                unsafe { ctx.ClearRenderTargetView(Some(&rtv.0), &color) };
                Ok(())
            }
            Command::ClearDepthStencil {
                texture,
                depth,
                stencil,
                rects,
            } => {
                if !rects.is_empty() {
                    return Err(RhiError::NotSupported(
                        "partial depth clears need D3D11 ClearView, which the \
                         recorder does not model yet"
                            .into(),
                    ));
                }
                let dsv = view_dsv(device, texture)?;
                let flags = if *stencil != 0 {
                    D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL
                } else {
                    D3D11_CLEAR_DEPTH
                };
                unsafe { ctx.ClearDepthStencilView(Some(&dsv.0), flags.0 as u32, *depth, *stencil as u8) };
                Ok(())
            }
            Command::CopyBuffer { src, dst, regions } => {
                let src_res = buffer_resource(device, src)?;
                let dst_res = buffer_resource(device, dst)?;
                for region in regions {
                    if region.size == 0 {
                        continue;
                    }
                    let region_box = D3D11_BOX {
                        left: region.src_offset as u32,
                        right: (region.src_offset + region.size) as u32,
                        top: 0,
                        bottom: 1,
                        front: 0,
                        back: 1,
                    };
                    unsafe {
                        ctx.CopySubresourceRegion(
                            &dst_res,
                            0,
                            region.dst_offset as u32,
                            0,
                            0,
                            &src_res,
                            0,
                            Some(&region_box),
                        )
                    };
                }
                Ok(())
            }
            Command::CopyBufferToTexture { .. } => Err(RhiError::NotSupported(
                "the D3D11 recorder does not model buffer-to-texture copies yet; \
                 use upload_texture"
                    .into(),
            )),
            Command::PipelineBarrier { .. } => Ok(()),
            Command::BeginRenderPass(info) => self.begin_render_pass(device, ctx, info),
            Command::EndRenderPass => self.end_render_pass(ctx),
            Command::NextSubpass => Err(RhiError::NotSupported(
                "the D3D11 recorder models single-subpass render passes only".into(),
            )),
            Command::BeginDebugLabel { .. } | Command::EndDebugLabel => Ok(()),
            Command::DispatchRays { .. } | Command::BuildAccelerationStructure => {
                Err(RhiError::NotSupported(
                    "this D3D11 recorder has no ray tracing path".into(),
                ))
            }
        }
    }

    fn require_pass(&self) -> RhiResult<()> {
        if self.rtvs.is_empty() && self.dsv.is_none() {
            return Err(RhiError::BackendError(
                "a draw command is outside a render pass".into(),
            ));
        }
        Ok(())
    }

    fn seed_viewport(
        &self,
        info: &RenderPassBeginInfo,
    ) -> [D3D11_VIEWPORT; 1] {
        if self.width > 0 && self.height > 0 {
            [D3D11_VIEWPORT {
                TopLeftX: 0.0,
                TopLeftY: 0.0,
                Width: self.width as f32,
                Height: self.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]
        } else {
            [D3D11_VIEWPORT {
                TopLeftX: info.render_area.offset.x as f32,
                TopLeftY: info.render_area.offset.y as f32,
                Width: info.render_area.extent.width as f32,
                Height: info.render_area.extent.height as f32,
                MinDepth: 0.0,
                MaxDepth: 1.0,
            }]
        }
    }

    fn begin_render_pass(
        &mut self,
        device: &D3D11Device,
        ctx: &ID3D11DeviceContext,
        info: &RenderPassBeginInfo,
    ) -> RhiResult<()> {
        if !self.rtvs.is_empty() || self.dsv.is_some() {
            return Err(RhiError::BackendError(
                "nested render passes are not recordable in D3D11".into(),
            ));
        }
        let Some(fb_gpu) = info.framebuffer.backend() else {
            return Err(RhiError::BackendError(
                "the render pass begin framebuffer has no GPU backing".into(),
            ));
        };
        let (rtvs, dsv, width, height) = device.with_object(fb_gpu.handle, "framebuffer", |object| {
            let D3DObject::Framebuffer(entry) = object else {
                return Err(RhiError::InternalError(
                    "a framebuffer handle lost its kind".into(),
                ));
            };
            Ok((
                entry.rtvs.clone(),
                entry.dsv.clone(),
                entry.width,
                entry.height,
            ))
        })?;
        let mut bound: Vec<Option<ID3D11RenderTargetView>> =
            rtvs.iter().map(|rtv| Some(rtv.0.clone())).collect();
        while bound.len() < 8 {
            bound.push(None);
        }
        unsafe { ctx.OMSetRenderTargets(Some(bound.as_slice()), dsv.as_ref().map(|d| &d.0)) };
        self.rtvs = rtvs;
        self.dsv = dsv;
        self.width = width;
        self.height = height;
        let seeded = self.seed_viewport(info);
        unsafe { ctx.RSSetViewports(Some(&seeded)) };
        for (value, rtv) in info.clear_values.iter().zip(self.rtvs.iter()) {
            if let ClearValue::Color { r, g, b, a } = value {
                let color = [*r, *g, *b, *a];
                unsafe { ctx.ClearRenderTargetView(Some(&rtv.0), &color) };
            }
        }
        for value in &info.clear_values {
            if let ClearValue::DepthStencil { depth, stencil } = value {
                if let Some(dsv) = &self.dsv {
                    let flags = if *stencil != 0 {
                        D3D11_CLEAR_DEPTH | D3D11_CLEAR_STENCIL
                    } else {
                        D3D11_CLEAR_DEPTH
                    };
                    unsafe {
                        ctx.ClearDepthStencilView(
                            Some(&dsv.0),
                            flags.0 as u32,
                            *depth,
                            *stencil as u8,
                        )
                    };
                }
                break;
            }
        }
        Ok(())
    }

    fn end_render_pass(&mut self, ctx: &ID3D11DeviceContext) -> RhiResult<()> {
        if self.rtvs.is_empty() && self.dsv.is_none() {
            return Err(RhiError::BackendError(
                "EndRenderPass has no render pass to end".into(),
            ));
        }
        let unbound = [None, None, None, None, None, None, None, None];
        unsafe {
            ctx.OMSetRenderTargets(Some(unbound.as_slice()), Option::<&ID3D11DepthStencilView>::None)
        };
        self.rtvs.clear();
        self.dsv = None;
        self.has_index_buffer = false;
        Ok(())
    }

    fn bind_graphics_pipeline(
        &mut self,
        device: &D3D11Device,
        ctx: &ID3D11DeviceContext,
        pipeline: &GraphicsPipeline,
    ) -> RhiResult<()> {
        let Some(gpu) = pipeline.backend() else {
            return Err(RhiError::BackendError(
                "the bound graphics pipeline has no GPU backing".into(),
            ));
        };
        let entry = device.with_object(gpu.handle, "graphics pipeline", |object| {
            let D3DObject::GraphicsPipeline(entry) = object else {
                return Err(RhiError::InternalError(
                    "a graphics pipeline handle lost its kind".into(),
                ));
            };
            Ok(entry.clone())
        })?;
        let Some(topology) = entry.topology else {
            return Err(RhiError::InternalError(
                "a graphics pipeline lost its topology".into(),
            ));
        };
        unsafe {
            match &entry.input_layout {
                Some(layout) => ctx.IASetInputLayout(Some(&layout.0)),
                None => {
                    ctx.IASetInputLayout(Option::<&windows::Win32::Graphics::Direct3D11::ID3D11InputLayout>::None)
                }
            }
            ctx.IASetPrimitiveTopology(topology);
            match &entry.vs {
                Some(vs) => ctx.VSSetShader(Some(&vs.0), None),
                None => ctx.VSSetShader(
                    Option::<&windows::Win32::Graphics::Direct3D11::ID3D11VertexShader>::None,
                    None,
                ),
            }
            match &entry.ps {
                Some(ps) => ctx.PSSetShader(Some(&ps.0), None),
                None => ctx.PSSetShader(
                    Option::<&windows::Win32::Graphics::Direct3D11::ID3D11PixelShader>::None,
                    None,
                ),
            }
            match &entry.rasterizer {
                Some(state) => ctx.RSSetState(Some(&state.0)),
                None => ctx.RSSetState(
                    Option::<&windows::Win32::Graphics::Direct3D11::ID3D11RasterizerState>::None,
                ),
            }
            match &entry.blend {
                Some(blend) => {
                    ctx.OMSetBlendState(Some(&blend.0), Some(&entry.blend_constants), 0xffffffff)
                }
                None => ctx.OMSetBlendState(
                    Option::<&windows::Win32::Graphics::Direct3D11::ID3D11BlendState>::None,
                    Some(&entry.blend_constants),
                    0xffffffff,
                ),
            }
            match &entry.depth_stencil {
                Some(state) => ctx.OMSetDepthStencilState(Some(&state.0), 0),
                None => ctx.OMSetDepthStencilState(
                    Option::<&windows::Win32::Graphics::Direct3D11::ID3D11DepthStencilState>::None,
                    0,
                ),
            }
        }
        self.vertex_strides = entry.vertex_strides.clone();
        let desc = pipeline.desc();
        if !desc.viewport_state.viewports.is_empty() {
            let viewports: Vec<D3D11_VIEWPORT> = desc
                .viewport_state
                .viewports
                .iter()
                .map(|v| D3D11_VIEWPORT {
                    TopLeftX: v.x,
                    TopLeftY: v.y,
                    Width: v.width,
                    Height: v.height,
                    MinDepth: v.min_depth,
                    MaxDepth: v.max_depth,
                })
                .collect();
            unsafe { ctx.RSSetViewports(Some(viewports.as_slice())) };
        }
        if !desc.viewport_state.scissors.is_empty() {
            let rects: Vec<RECT> = desc
                .viewport_state
                .scissors
                .iter()
                .map(|s| RECT {
                    left: s.offset.x,
                    top: s.offset.y,
                    right: s.offset.x + s.extent.width as i32,
                    bottom: s.offset.y + s.extent.height as i32,
                })
                .collect();
            unsafe { ctx.RSSetScissorRects(Some(rects.as_slice())) };
        }
        Ok(())
    }

    fn bind_compute_pipeline(
        &self,
        device: &D3D11Device,
        ctx: &ID3D11DeviceContext,
        pipeline: &ComputePipeline,
    ) -> RhiResult<()> {
        let Some(gpu) = pipeline.backend() else {
            return Err(RhiError::BackendError(
                "the bound compute pipeline has no GPU backing".into(),
            ));
        };
        let cs = device.with_object(gpu.handle, "compute pipeline", |object| {
            let D3DObject::ComputePipeline(entry) = object else {
                return Err(RhiError::InternalError(
                    "a compute pipeline handle lost its kind".into(),
                ));
            };
            Ok(entry.cs.0.clone())
        })?;
        unsafe { ctx.CSSetShader(Some(&cs), None) };
        Ok(())
    }
}

fn buffer_entry(device: &D3D11Device, buffer: &Buffer) -> RhiResult<BufferEntry> {
    let Some(gpu) = buffer.backend() else {
        return Err(RhiError::BackendError(
            "a bound buffer has no GPU backing".into(),
        ));
    };
    device.with_object(gpu.handle, "buffer", |object| {
        let D3DObject::Buffer(entry) = object else {
            return Err(RhiError::InternalError(
                "a buffer handle lost its kind".into(),
            ));
        };
        Ok(entry.clone())
    })
}

fn buffer_resource(device: &D3D11Device, buffer: &Buffer) -> RhiResult<ID3D11Resource> {
    let entry = buffer_entry(device, buffer)?;
    entry
        .buffer
        .0
        .cast()
        .map_err(|e| RhiError::InternalError(format!("buffer resource cast: {e}")))
}

fn view_rtv(
    device: &D3D11Device,
    view: &TextureView,
) -> RhiResult<DxCom<ID3D11RenderTargetView>> {
    let Some(gpu) = view.backend() else {
        return Err(RhiError::BackendError(
            "a cleared texture view has no GPU backing".into(),
        ));
    };
    device.with_object(gpu.handle, "texture view", |object| {
        let D3DObject::Texture(entry) = object else {
            return Err(RhiError::InternalError(
                "a texture view handle lost its kind".into(),
            ));
        };
        entry.roles.rtv.clone().ok_or_else(|| {
            RhiError::BackendError(
                "the view has no render target view; colour clears need one".into(),
            )
        })
    })
}

fn view_dsv(device: &D3D11Device, view: &TextureView) -> RhiResult<DxCom<ID3D11DepthStencilView>> {
    let Some(gpu) = view.backend() else {
        return Err(RhiError::BackendError(
            "a cleared texture view has no GPU backing".into(),
        ));
    };
    device.with_object(gpu.handle, "texture view", |object| {
        let D3DObject::Texture(entry) = object else {
            return Err(RhiError::InternalError(
                "a texture view handle lost its kind".into(),
            ));
        };
        entry.roles.dsv.clone().ok_or_else(|| {
            RhiError::BackendError(
                "the view has no depth stencil view; depth clears need one".into(),
            )
        })
    })
}

fn iter_rects(scissors: &[Rect2D]) -> Vec<RECT> {
    scissors
        .iter()
        .map(|s| RECT {
            left: s.offset.x,
            top: s.offset.y,
            right: s.offset.x + s.extent.width as i32,
            bottom: s.offset.y + s.extent.height as i32,
        })
        .collect()
}
