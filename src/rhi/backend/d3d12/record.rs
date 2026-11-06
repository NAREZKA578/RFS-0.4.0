//! Direct3D 12 command translation: recorded [`Command`]s to `ID3D12GraphicsCommandList`.
//!
//! The submit is sequential per the M1/M2 plan (Баг№706): one allocator and
//! one list, reset, recorded, closed, executed and fenced one call after
//! another — so the recorder emits only the calls inside the buffer; the
//! device owns reset/close/execute (see `D3D12Device::submit_sequential`,
//! which every `submit_commands` submission rides).
//!
//! Deliberately narrow, in the shape of the Vulkan translator: the copy and
//! state commands are translated, the render path (M3, Баг№707) is translated
//! on top of the same state tracker — one pass in flight, targets driven
//! through tracked transitions — and the compute path (M4, Баг№708) is
//! refused rather than skipped. A silently dropped command draws a wrong
//! picture and looks like a flaky GPU to whoever hunts it.
//!
//! State tracking: D3D12's `ResourceBarrier` must name the resource's exact
//! current `StateBefore`. The recorder therefore tracks one state belief per
//! touched handle inside a list, seeded from the resource's home state —
//! the state its entry recorded at creation (`COMMON` for every DEFAULT
//! heap texture and device-local buffer); and it stays honest to the D3D12
//! heap contract that pins a heap's resources: an UPLOAD-heap resource is
//! born in GENERIC_READ and never transitions (`GENERIC_READ` already
//! contains `COPY_SOURCE`), a READBACK-heap resource is born in COPY_DEST
//! and similarly stays — those two never take a barrier here. Within the
//! list, whatever it records becomes the new truth. A transition whose
//! declared use disagrees with that belief is recorded from the tracker:
//! the D3D12 transition barrier is scoped to the resource, not to a stage
//! pair or a layout declaration, and between contradictory witnesses the
//! tracker is the one that stands behind what was actually recorded.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::collections::HashMap;

use super::convert;
use super::descriptor::DescriptorKind;
use super::device::{D3D12Device, GraphicsPipelineEntry};
use crate::command::commands::Command;
use crate::command::pass::render::LoadOp;
use crate::error::{RhiError, RhiResult};
use crate::resource::TextureDesc;
use crate::types::{AccessFlags, ClearValue, IndexType, Rect2D, TextureDimensions};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_CLEAR_FLAGS, D3D12_CLEAR_FLAG_DEPTH, D3D12_CLEAR_FLAG_STENCIL,
    D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_INDEX_BUFFER_VIEW,
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_RESOURCE_BARRIER,
    D3D12_RESOURCE_BARRIER_0, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_UAV, D3D12_RESOURCE_STATE_COMMON,
    D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COPY_SOURCE,
    D3D12_RESOURCE_STATE_DEPTH_WRITE, D3D12_RESOURCE_STATE_GENERIC_READ,
    D3D12_RESOURCE_STATE_RENDER_TARGET, D3D12_RESOURCE_STATES, D3D12_RESOURCE_UAV_BARRIER,
    D3D12_SUBRESOURCE_FOOTPRINT, D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    D3D12_VERTEX_BUFFER_VIEW, D3D12_VIEWPORT, ID3D12GraphicsCommandList,
    ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_UINT};

/// One render pass in flight: the textures the begin moved into their
/// render states (`resource`, tracked handle, home state). `EndRenderPass`
/// walks every one of them back to COMMON through the same tracker.
struct ActivePass {
    targets: Vec<(ID3D12Resource, u64, D3D12_RESOURCE_STATES)>,
}

/// The recorder for one submitted list.
struct Recorder<'a> {
    list: &'a ID3D12GraphicsCommandList,
    device: &'a D3D12Device,
    /// The state the list believes each touched resource to be in. Seeded
    /// with the home `COMMON` (see the module comment); every recorded
    /// transition updates it.
    states: HashMap<u64, D3D12_RESOURCE_STATES>,
    /// The pass whose targets sit in the render states, for EndRenderPass.
    active_pass: Option<ActivePass>,
    /// The graphics pipeline the draw state carries: its input-layout
    /// strides feed `BindVertexBuffers`.
    pipeline: Option<GraphicsPipelineEntry>,
    /// The currently bound index-buffer view, when one is bound.
    index_view: Option<D3D12_INDEX_BUFFER_VIEW>,
    /// `SetDescriptorHeaps` is a per-list binding, not a per-set one: the
    /// two shader-visible heaps are bound the first time a set binds.
    heaps_set: bool,
}

impl Recorder<'_> {
    /// The believed state of a tracked resource: seeded from the resource's
    /// home state on first use, then exactly what this list recorded.
    fn state(&mut self, handle: u64, home: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_STATES {
        *self.states.entry(handle).or_insert(home)
    }

    /// Record a transition to `after` and update the tracker.
    ///
    /// Heap-bound resources are skipped entirely: an UPLOAD-heap resource
    /// lives and dies in GENERIC_READ (which already contains COPY_SOURCE),
    /// a READBACK-heap one in COPY_DEST (which is the copy destination) —
    /// the D3D12 heap contract refuses any transition out of those states,
    /// and every copy use these commands make is already covered by them.
    /// For the movable kind the tracker is the only before-state this list
    /// can stand behind: a transition whose resource was never moved by
    /// this list would name a `StateBefore` the GPU never entered *inside
    /// this submission*. Redundant barriers (already believed to be
    /// `after`) are dropped here: `StateBefore == StateAfter` is rejected
    /// as an error, and the skipped one is exactly what "already there"
    /// means.
    fn go(
        &mut self,
        resource: &ID3D12Resource,
        handle: u64,
        home: D3D12_RESOURCE_STATES,
        after: D3D12_RESOURCE_STATES,
    ) {
        if heap_stuck(home) {
            return;
        }
        let before = self.state(handle, home);
        if before == after {
            return;
        }
        self.device
            .transition(self.list, resource, before, after);
        self.states.insert(handle, after);
    }

    /// One global UAV barrier — the D3D12 answer to memory barriers that
    /// name unordered access: a fence over every UAV write the list made so
    /// far, rather than one per resource.
    fn uav_barrier(&self) {
        let barrier = D3D12_RESOURCE_BARRIER {
            Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
            Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
            Anonymous: D3D12_RESOURCE_BARRIER_0 {
                UAV: std::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                    pResource: std::mem::ManuallyDrop::new(None),
                }),
            },
        };
        unsafe { self.list.ResourceBarrier(&[barrier]) };
    }
}

/// The D3D12 heap contract, as a predicate: UPLOAD-heap resources are born
/// in GENERIC_READ and READBACK-heap ones in COPY_DEST — the documented
/// `CreateCommittedResource` initial states for those heaps — and the
/// runtime refuses transition barriers on them. Nothing recorded anywhere
/// in this list moves them, and the copy actions of both directions (a
/// source read through GENERIC_READ's COPY_SOURCE bit, a destination write
/// through COPY_DEST) need no state of their own.
fn heap_stuck(home: D3D12_RESOURCE_STATES) -> bool {
    home == D3D12_RESOURCE_STATE_GENERIC_READ || home == D3D12_RESOURCE_STATE_COPY_DEST
}

/// Replay `commands` onto an open `list`. Called from
/// `D3D12Device::submit_commands` with the freshly reset sequential list.
pub(crate) fn record(
    list: &ID3D12GraphicsCommandList,
    device: &D3D12Device,
    commands: &[Command],
) -> RhiResult<()> {
    let mut rec = Recorder {
        list,
        device,
        states: HashMap::new(),
        active_pass: None,
        pipeline: None,
        index_view: None,
        heaps_set: false,
    };
    for command in commands {
        replay(&mut rec, command)?;
    }
    Ok(())
}

fn replay(rec: &mut Recorder<'_>, command: &Command) -> RhiResult<()> {
    match command {
        // ---------- the M3 graphics path (Баг№707) ----------
        Command::BeginRenderPass(info) => {
            if rec.active_pass.is_some() {
                return Err(RhiError::BackendError(
                    "BeginRenderPass records into a pass already in flight; \
                     the list supports one pass at a time"
                        .into(),
                ));
            }
            let Some(fb_gpu) = info.framebuffer.backend() else {
                return Err(RhiError::BackendError(
                    "the render pass's framebuffer has no GPU backing".into(),
                ));
            };
            let fb = rec.device.framebuffer_entry(fb_gpu.handle)?;
            let Some(pass_gpu) = info.render_pass.backend() else {
                return Err(RhiError::BackendError(
                    "the begin's render pass has no GPU backing".into(),
                ));
            };
            let pass = rec.device.pass_description(pass_gpu.handle)?;
            if info.clear_values.len() > pass.attachments.len() {
                return Err(RhiError::BackendError(format!(
                    "the begin names {} clear values for {} attachments; the \
                     values are positional, so there is nothing for the rest \
                     to sit on",
                    info.clear_values.len(),
                    pass.attachments.len()
                )));
            }

            // The begins' clear values decode before anything is recorded:
            // a kind mismatch at position 5 must kill the begin, not draw
            // its first four targets and throw the rest away.
            let mut clears: Vec<PassClear> = Vec::new();
            let mut colour_ord = 0usize;
            for (index, attachment) in pass.attachments.iter().enumerate() {
                if convert::typeless_base(attachment.format).is_some() {
                    if attachment.load_op != LoadOp::Clear {
                        continue;
                    }
                    let Some(ClearValue::DepthStencil { depth, stencil }) =
                        info.clear_values.get(index)
                    else {
                        return Err(RhiError::BackendError(format!(
                            "the pass loads depth attachment {} Clear, but the \
                             clear value at that position is not a DepthStencil \
                             one",
                            index
                        )));
                    };
                    let Some(slot) = fb.dsv_slot else {
                        return Err(RhiError::InternalError(
                            "the pass clears a depth attachment, its \
                             framebuffer reserved no DSV slot"
                                .into(),
                        ));
                    };
                    clears.push(PassClear::Depth {
                        slot,
                        depth: *depth,
                        stencil: *stencil,
                        stencil_plane: matches!(
                            attachment.format,
                            crate::types::Format::D24_UNORM_S8_UINT
                                | crate::types::Format::D32_SFLOAT_S8_UINT
                        ),
                    });
                } else {
                    let Some(slot) = fb.rtv_slots.get(colour_ord).copied() else {
                        return Err(RhiError::InternalError(format!(
                            "the pass declares colour attachment {} beyond the \
                             {} RTVs its framebuffer reserves",
                            index,
                            fb.rtv_slots.len()
                        )));
                    };
                    colour_ord += 1;
                    if attachment.load_op != LoadOp::Clear {
                        continue;
                    }
                    let Some(ClearValue::Color { r, g, b, a }) =
                        info.clear_values.get(index)
                    else {
                        return Err(RhiError::BackendError(format!(
                            "the pass loads colour attachment {} Clear, but the \
                             clear value at that position is not a Colour one",
                            index
                        )));
                    };
                    clears.push(PassClear::Colour {
                        slot,
                        colour: [*r, *g, *b, *a],
                    });
                }
            }

            // The targets transit their tracked textures into the render
            // states. A heap-pinned home has no transition in it and no
            // render-target use either: the refusal is spoken here, not left
            // for the driver's validation text (the №744 lesson).
            let mut targets: Vec<(ID3D12Resource, u64, D3D12_RESOURCE_STATES)> =
                Vec::with_capacity(fb.rtv_handles.len() + fb.dsv_handle.iter().count());
            for handle in fb.rtv_handles.iter().copied() {
                targets.push(target_of(rec.device, handle)?);
            }
            if let Some(handle) = fb.dsv_handle {
                targets.push(target_of(rec.device, handle)?);
            }
            for (resource, handle, home) in &targets {
                let after = if fb.dsv_handle == Some(*handle) {
                    D3D12_RESOURCE_STATE_DEPTH_WRITE
                } else {
                    D3D12_RESOURCE_STATE_RENDER_TARGET
                };
                rec.go(resource, *handle, *home, after);
            }

            // The pass binds its descriptors by pointer once: the RTV/DSV
            // slots were allocated at framebuffer creation.
            let rtv_cpus: Vec<D3D12_CPU_DESCRIPTOR_HANDLE> = fb
                .rtv_slots
                .iter()
                .map(|slot| rec.device.heaps().cpu(DescriptorKind::Rtv, *slot))
                .collect();
            let dsv_cpu: Option<D3D12_CPU_DESCRIPTOR_HANDLE> = fb
                .dsv_slot
                .map(|slot| rec.device.heaps().cpu(DescriptorKind::Dsv, slot));
            unsafe {
                rec.list.OMSetRenderTargets(
                    rtv_cpus.len() as u32,
                    if rtv_cpus.is_empty() {
                        None
                    } else {
                        Some(rtv_cpus.as_ptr())
                    },
                    false,
                    dsv_cpu.as_ref().map(std::ptr::from_ref),
                );
            }

            // Then the clears: whole-target view clear depths and colours,
            // as D3D12 has no per-pass load at all.
            for clear in &clears {
                match clear {
                    PassClear::Colour { slot, colour } => {
                        let cpu = rec.device.heaps().cpu(DescriptorKind::Rtv, *slot);
                        unsafe { rec.list.ClearRenderTargetView(cpu, colour, None) };
                    }
                    PassClear::Depth {
                        slot,
                        depth,
                        stencil,
                        stencil_plane,
                    } => {
                        let cpu = rec.device.heaps().cpu(DescriptorKind::Dsv, *slot);
                        let flags = if *stencil_plane {
                            D3D12_CLEAR_FLAGS(D3D12_CLEAR_FLAG_DEPTH.0 | D3D12_CLEAR_FLAG_STENCIL.0)
                        } else {
                            D3D12_CLEAR_FLAG_DEPTH
                        };
                        unsafe {
                            rec.list.ClearDepthStencilView(cpu, flags, *depth, *stencil as u8, None)
                        };
                    }
                }
            }

            rec.active_pass = Some(ActivePass { targets });
            Ok(())
        }

        Command::EndRenderPass => {
            let Some(flight) = rec.active_pass.take() else {
                return Err(RhiError::BackendError(
                    "EndRenderPass closes no begin: the list is outside a \
                     render pass"
                        .into(),
                ));
            };
            for (resource, handle, home) in &flight.targets {
                rec.go(resource, *handle, *home, D3D12_RESOURCE_STATE_COMMON);
            }
            Ok(())
        }

        Command::BindGraphicsPipeline(pipeline) => {
            let gpu = pipeline.backend().ok_or_else(|| {
                RhiError::BackendError("the graphics pipeline has no GPU backing".into())
            })?;
            let entry = rec.device.pipeline_entry(gpu.handle)?;
            unsafe {
                rec.list.SetGraphicsRootSignature(Some(entry.root_signature()));
                rec.list.SetPipelineState(Some(entry.pso()));
                rec.list.IASetPrimitiveTopology(entry.topology());
            }
            let viewports: Vec<D3D12_VIEWPORT> = entry
                .viewports()
                .iter()
                .map(|v| D3D12_VIEWPORT {
                    TopLeftX: v.x,
                    TopLeftY: v.y,
                    Width: v.width,
                    Height: v.height,
                    MinDepth: v.min_depth,
                    MaxDepth: v.max_depth,
                })
                .collect();
            if !viewports.is_empty() {
                unsafe { rec.list.RSSetViewports(&viewports) };
            }
            let scissors: Vec<RECT> = entry
                .scissors()
                .iter()
                .map(|s| RECT {
                    left: s.offset.x,
                    top: s.offset.y,
                    right: s.offset.x.saturating_add(s.extent.width as i32),
                    bottom: s.offset.y.saturating_add(s.extent.height as i32),
                })
                .collect();
            if !scissors.is_empty() {
                unsafe { rec.list.RSSetScissorRects(&scissors) };
            }
            unsafe { rec.list.OMSetBlendFactor(Some(&entry.blend_factor())) };
            rec.pipeline = Some(entry);
            Ok(())
        }

        Command::BindDescriptorSets {
            pipeline,
            first_set,
            sets,
        } => {
            let gpu = pipeline.backend().ok_or_else(|| {
                RhiError::BackendError("a descriptor set's pipeline has no GPU backing".into())
            })?;
            let entry = rec.device.pipeline_entry(gpu.handle)?;
            let first = *first_set as usize;
            if first + sets.len() > entry.num_sets() {
                return Err(RhiError::BackendError(format!(
                    "the sets bind at {}: the pipeline declares {} set slots \
                     and {} sets would overtake it",
                    first,
                    entry.num_sets(),
                    sets.len()
                )));
            }
            // The two shader-visible heaps are a per-list binding, bound the
            // first time a set binds — not per set.
            if !rec.heaps_set {
                let heaps = [
                    Some(rec.device.heaps().cbv_srv_uav_heap().clone()),
                    Some(rec.device.heaps().sampler_heap().clone()),
                ];
                unsafe { rec.list.SetDescriptorHeaps(&heaps) };
                rec.heaps_set = true;
            }
            for (offset, set) in sets.iter().enumerate() {
                let Some(set_gpu) = set.backend() else {
                    return Err(RhiError::BackendError(format!(
                        "descriptor set {} has no GPU backing",
                        first + offset
                    )));
                };
                let set_entry = rec.device.set_entry(set_gpu.handle)?;
                let parameter = first_set + offset as u32;
                // One table per set: the set's block sits in exactly one
                // heap (the layout classification refused the mixed ones),
                // and an empty set binds nothing.
                if let Some(base) = set_entry.block_base() {
                    let gpu_handle = rec
                        .device
                        .heaps()
                        .gpu(DescriptorKind::CbvSrvUav, base)?;
                    unsafe {
                        rec.list
                            .SetGraphicsRootDescriptorTable(parameter, gpu_handle)
                    };
                } else if let Some(base) = set_entry.sampler_block_base() {
                    let gpu_handle = rec.device.heaps().gpu(DescriptorKind::Sampler, base)?;
                    unsafe {
                        rec.list
                            .SetGraphicsRootDescriptorTable(parameter, gpu_handle)
                    };
                }
            }
            Ok(())
        }

        Command::BindVertexBuffers { first_binding, buffers } => {
            let Some(entry) = rec.pipeline.clone() else {
                return Err(RhiError::BackendError(
                    "BindVertexBuffers records with no graphics pipeline \
                     bound: its input layout owns the strides"
                        .into(),
                ));
            };
            let mut views: Vec<D3D12_VERTEX_BUFFER_VIEW> = Vec::with_capacity(buffers.len());
            for (index, (buffer, offset)) in buffers.iter().enumerate() {
                let slot = first_binding + index as u32;
                let Some((_, stride)) = entry
                    .vertex_strides()
                    .iter()
                    .find(|(bound, _)| bound == &slot)
                else {
                    return Err(RhiError::BackendError(format!(
                        "the pipeline binds no vertex input at slot {slot}; \
                         a view has no stride to take"
                    )));
                };
                if stride % 4 != 0 {
                    return Err(RhiError::NotSupported(format!(
                        "the vertex input at slot {slot} strides {stride} \
                         bytes; the input assembler wants four-byte strides"
                    )));
                }
                let gpu = buffer.backend().ok_or_else(|| {
                    RhiError::BackendError("a vertex buffer has no GPU backing".into())
                })?;
                let buffer_entry = rec.device.buffer_entry(gpu.handle)?;
                check_range(*offset, 0, buffer_entry.size(), "vertex buffer")?;
                let space = buffer_entry.size() - *offset;
                if space > u32::MAX as u64 {
                    return Err(RhiError::BackendError(format!(
                        "a vertex buffer view of {space} bytes after offset \
                         {offset} exceeds the u32 bound D3D12's view carries"
                    )));
                }
                let location =
                    unsafe { buffer_entry.resource().GetGPUVirtualAddress() } + *offset;
                views.push(D3D12_VERTEX_BUFFER_VIEW {
                    BufferLocation: location,
                    SizeInBytes: space as u32,
                    StrideInBytes: *stride,
                });
            }
            unsafe { rec.list.IASetVertexBuffers(*first_binding, Some(&views)) };
            Ok(())
        }

        Command::BindIndexBuffer {
            buffer,
            offset,
            index_type,
        } => {
            let gpu = buffer.backend().ok_or_else(|| {
                RhiError::BackendError("the index buffer has no GPU backing".into())
            })?;
            let entry = rec.device.buffer_entry(gpu.handle)?;
            check_range(*offset, 0, entry.size(), "index buffer")?;
            let space = entry.size() - *offset;
            if space > u32::MAX as u64 {
                return Err(RhiError::BackendError(format!(
                    "an index buffer view of {space} bytes after offset \
                     {offset} exceeds the u32 bound D3D12's view carries"
                )));
            }
            let view = D3D12_INDEX_BUFFER_VIEW {
                BufferLocation: unsafe { entry.resource().GetGPUVirtualAddress() }
                    + *offset,
                SizeInBytes: space as u32,
                Format: match index_type {
                    IndexType::U16 => DXGI_FORMAT_R16_UINT,
                    IndexType::U32 => DXGI_FORMAT_R32_UINT,
                },
            };
            unsafe { rec.list.IASetIndexBuffer(Some(&view)) };
            rec.index_view = Some(view);
            Ok(())
        }

        Command::Draw {
            vertex_count,
            instance_count,
            first_vertex,
            first_instance,
        } => {
            if rec.active_pass.is_none() {
                return Err(RhiError::BackendError(
                    "Draw records outside a render pass: a draw commands a \
                     render target, and none is bound"
                        .into(),
                ));
            }
            unsafe {
                rec.list.DrawInstanced(
                    *vertex_count,
                    *instance_count,
                    *first_vertex,
                    *first_instance,
                )
            };
            Ok(())
        }

        Command::DrawIndexed {
            index_count,
            instance_count,
            first_index,
            vertex_offset,
            first_instance,
        } => {
            if rec.active_pass.is_none() {
                return Err(RhiError::BackendError(
                    "DrawIndexed records outside a render pass: a draw \
                     commands a render target, and none is bound"
                        .into(),
                ));
            }
            if rec.index_view.is_none() {
                return Err(RhiError::BackendError(
                    "DrawIndexed records with no index buffer bound"
                        .into(),
                ));
            }
            unsafe {
                rec.list.DrawIndexedInstanced(
                    *index_count,
                    *instance_count,
                    *first_index,
                    *vertex_offset,
                    *first_instance,
                )
            };
            Ok(())
        }

        Command::ClearColor {
            texture,
            value,
            rects,
        } => {
            let ClearValue::Color { r, g, b, a } = value else {
                return Err(RhiError::BackendError(
                    "ClearColor names a depth/stencil value; \
                     ClearDepthStencil is the command for a depth plane"
                        .into(),
                ));
            };
            let (resource, handle, home, view) = standalone_target(rec.device, texture)?;
            if convert::typeless_base(view.rhi_format()).is_some() {
                return Err(RhiError::BackendError(
                    "ClearColor names a depth texture: its view binds a DSV, \
                     not an RTV; ClearDepthStencil is the command for it"
                        .into(),
                ));
            }
            // One temporary RTV slot from the device's own CPU pool: the
            // clear needs a descriptor for the time of the call, and the
            // slot goes straight back after the clear.
            let slot = rec.device.heaps().alloc(DescriptorKind::Rtv, 1)?;
            let desc = D3D12Device::framebuffer_rtv_desc(&view, 0, 0)?;
            unsafe {
                rec.device.raw_device().CreateRenderTargetView(
                    view.resource(),
                    Some(&desc),
                    rec.device.heaps().cpu(DescriptorKind::Rtv, slot),
                );
            }
            rec.go(&resource, handle, home, D3D12_RESOURCE_STATE_RENDER_TARGET);
            let rects = clear_rects(rects);
            unsafe {
                rec.list.ClearRenderTargetView(
                    rec.device.heaps().cpu(DescriptorKind::Rtv, slot),
                    &[*r, *g, *b, *a],
                    rects.as_deref(),
                );
            }
            rec.go(&resource, handle, home, D3D12_RESOURCE_STATE_COMMON);
            rec.device.heaps().rewind(DescriptorKind::Rtv, 1);
            Ok(())
        }

        Command::ClearDepthStencil {
            texture,
            depth,
            stencil,
            rects,
        } => {
            let (resource, handle, home, view) = standalone_target(rec.device, texture)?;
            if convert::typeless_base(view.rhi_format()).is_none() {
                return Err(RhiError::BackendError(
                    "ClearDepthStencil names a colour texture: its view binds \
                     an RTV, not a DSV; ClearColor is the command for it"
                        .into(),
                ));
            }
            let slot = rec.device.heaps().alloc(DescriptorKind::Dsv, 1)?;
            let desc = D3D12Device::framebuffer_dsv_desc(&view, 0, 0)?;
            unsafe {
                rec.device.raw_device().CreateDepthStencilView(
                    view.resource(),
                    Some(&desc),
                    rec.device.heaps().cpu(DescriptorKind::Dsv, slot),
                );
            }
            let flags = if matches!(
                view.rhi_format(),
                crate::types::Format::D24_UNORM_S8_UINT | crate::types::Format::D32_SFLOAT_S8_UINT
            ) {
                D3D12_CLEAR_FLAGS(D3D12_CLEAR_FLAG_DEPTH.0 | D3D12_CLEAR_FLAG_STENCIL.0)
            } else {
                D3D12_CLEAR_FLAG_DEPTH
            };
            rec.go(&resource, handle, home, D3D12_RESOURCE_STATE_DEPTH_WRITE);
            let rects = clear_rects(rects);
            unsafe {
                rec.list.ClearDepthStencilView(
                    rec.device.heaps().cpu(DescriptorKind::Dsv, slot),
                    flags,
                    *depth,
                    *stencil as u8,
                    rects.as_deref(),
                );
            }
            rec.go(&resource, handle, home, D3D12_RESOURCE_STATE_COMMON);
            rec.device.heaps().rewind(DescriptorKind::Dsv, 1);
            Ok(())
        }

        // ---------- the M4 compute path refuses until its terrain ----------
        Command::BindComputePipeline(_)
        | Command::BindComputeDescriptorSets { .. }
        | Command::Dispatch { .. } => Err(RhiError::NotSupported(format!(
            "the D3D12 recorder does not replay {} yet: the compute path \
             (Баг№708, M4) owns it; refusing instead of dropping the \
             command into a wrong picture",
            discriminant(command)
        ))),

        Command::NextSubpass => Err(RhiError::NotSupported(
            "this backend supports single-subpass render passes; NextSubpass \
             has no subpass to step into"
                .into(),
        )),

        // ---------- the M2 surface ----------
        Command::CopyBuffer { src, dst, regions } => {
            let src_gpu = src
                .backend()
                .ok_or_else(|| RhiError::BackendError("copy source buffer has no GPU backing".into()))?;
            let dst_gpu = dst
                .backend()
                .ok_or_else(|| RhiError::BackendError("copy destination buffer has no GPU backing".into()))?;
            let device = rec.device;
            let src_entry = device.buffer_entry(src_gpu.handle)?;
            let dst_entry = device.buffer_entry(dst_gpu.handle)?;
            for region in regions {
                check_range(region.src_offset, region.size, src_entry.size(), "copy source")?;
                check_range(region.dst_offset, region.size, dst_entry.size(), "copy destination")?;
            }
            let src_res = src_entry.resource().clone();
            let dst_res = dst_entry.resource().clone();
            let src_handle = src_gpu.handle;
            let dst_handle = dst_gpu.handle;
            let src_home = src_entry.home_state();
            let dst_home = dst_entry.home_state();

            rec.go(&src_res, src_handle, src_home, D3D12_RESOURCE_STATE_COPY_SOURCE);
            rec.go(&dst_res, dst_handle, dst_home, D3D12_RESOURCE_STATE_COPY_DEST);
            for region in regions {
                if region.size == 0 {
                    continue;
                }
                unsafe {
                    // Destination first: among the D3D12 copies
                    // `CopyBufferRegion` names the destination before the
                    // source (the noted exception, BUGS №729), in both the
                    // SDK order and the windows 0.61 bindings.
                    rec.list.CopyBufferRegion(
                        &dst_res,
                        region.dst_offset,
                        &src_res,
                        region.src_offset,
                        region.size,
                    );
                }
            }
            rec.go(&src_res, src_handle, src_home, D3D12_RESOURCE_STATE_COMMON);
            rec.go(&dst_res, dst_handle, dst_home, D3D12_RESOURCE_STATE_COMMON);
            Ok(())
        }

        Command::CopyBufferToTexture { src, dst, regions } => {
            let src_gpu = src
                .backend()
                .ok_or_else(|| RhiError::BackendError("upload source buffer has no GPU backing".into()))?;
            let dst_gpu = dst
                .backend()
                .ok_or_else(|| RhiError::BackendError("upload destination texture has no GPU backing".into()))?;
            let device = rec.device;
            let src_entry = device.buffer_entry(src_gpu.handle)?;
            let dst_entry = device.texture_entry(dst_gpu.handle)?;
            let desc: TextureDesc = dst_entry.desc().clone();
            if !matches!(desc.dimensions, TextureDimensions::D1 | TextureDimensions::D2) {
                return Err(RhiError::NotSupported(
                    "a recorded CopyBufferToTexture covers 1D and 2D textures; \
                     3D and cube uploads need subresource handling the RHI copy \
                     region cannot name yet"
                        .into(),
                ));
            }
            if dst.is_multisampled() {
                return Err(RhiError::NotSupported(
                    "multisampled textures have no direct upload path in D3D12; \
                     uploads go through a resolve"
                        .into(),
                ));
            }
            let width = desc.width.max(1);
            let height = match desc.dimensions {
                TextureDimensions::D1 => 1,
                _ => desc.height.max(1),
            };
            let active: Vec<_> = regions.iter().filter(|region| region.size != 0).collect();
            if active.is_empty() {
                return Ok(());
            }
            // The D3D12 copy contract demands every staging row at a multiple
            // of 256 and every region offset too. A tightly packed row only
            // satisfies that when its width is already a multiple of 256 —
            // the managed `upload_texture` builds its own staging and covers
            // the general case, the recorded path can only speak for the
            // aligned one and refuses the rest loudly instead of choosing
            // between an error later and silently corrupt rows.
            let tight = convert::row_pitch(desc.format, width);
            if !tight.is_multiple_of(256) {
                return Err(RhiError::NotSupported(format!(
                    "a recorded CopyBufferToTexture demands 256-byte row pitch, \
                     the tight row of {:?} at width {} is {} bytes; use \
                     upload_texture, which stages the rows itself",
                    desc.format, width, tight
                )));
            }
            for region in regions {
                check_range(
                    region.buffer_offset,
                    (height as u64).saturating_mul(tight as u64),
                    src_entry.size(),
                    "upload source buffer",
                )?;
                if !region.buffer_offset.is_multiple_of(256) {
                    return Err(RhiError::NotSupported(format!(
                        "a recorded CopyBufferToTexture demands a 256-aligned \
                         buffer offset, saw {} (the managed upload_texture takes \
                         the misaligned case by building its own staging)",
                        region.buffer_offset
                    )));
                }
            }

            let dxgi = convert::dxgi_format(desc.format).ok_or_else(|| {
                RhiError::BackendError(format!("no D3D12 format for {:?}", desc.format))
            })?;
            let dst_res = dst_entry.resource().clone();
            let src_res = src_entry.resource().clone();
            let dst_handle = dst_gpu.handle;
            let dst_home = dst_entry.home_state();
            rec.go(&dst_res, dst_handle, dst_home, D3D12_RESOURCE_STATE_COPY_DEST);
            for region in &active {
                // The recorded copy fills subresource 0 (mip 0, layer 0) —
                // the one a `BufferTextureCopy` (`buffer_offset`, `size`)
                // effectively names; deeper use goes through the managed
                // upload path that owns the per-mip footprints.
                let footprint = D3D12_PLACED_SUBRESOURCE_FOOTPRINT {
                    Offset: region.buffer_offset,
                    Footprint: D3D12_SUBRESOURCE_FOOTPRINT {
                        Format: dxgi,
                        Width: width,
                        Height: height,
                        Depth: 1,
                        RowPitch: tight,
                    },
                };
                let src_view = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::ManuallyDrop::new(Some(src_res.clone())),
                    Type: D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        PlacedFootprint: footprint,
                    },
                };
                let dst_view = D3D12_TEXTURE_COPY_LOCATION {
                    pResource: std::mem::ManuallyDrop::new(Some(dst_res.clone())),
                    Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
                    Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 {
                        SubresourceIndex: D3D12Device::subresource_index(&desc, 0, 0),
                    },
                };
                unsafe { rec.list.CopyTextureRegion(&dst_view, 0, 0, 0, &src_view, None) };
            }
            rec.go(&dst_res, dst_handle, dst_home, D3D12_RESOURCE_STATE_COMMON);
            Ok(())
        }

        Command::PipelineBarrier {
            memory_barriers,
            buffer_barriers,
            texture_barriers,
            ..
        } => {
            // Buffer and texture barriers map to per-resource transition
            // barriers. The command-level stage pair stays in the RHI's own
            // voids: a D3D12 transition barrier is scoped to the resource,
            // and the barriers' access pairs already name the uses the
            // stages would describe.
            for barrier in buffer_barriers {
                let gpu = barrier
                    .buffer
                    .backend()
                    .ok_or_else(|| RhiError::BackendError("barrier buffer has no GPU backing".into()))?;
                let entry = rec.device.buffer_entry(gpu.handle)?;
                let res = entry.resource().clone();
                let home = entry.home_state();
                let after = convert::buffer_state(barrier.dst_access);
                rec.go(&res, gpu.handle, home, after);
            }
            for barrier in texture_barriers {
                let gpu = barrier
                    .texture
                    .backend()
                    .ok_or_else(|| RhiError::BackendError("barrier texture has no GPU backing".into()))?;
                let entry = rec.device.texture_entry(gpu.handle)?;
                let res = entry.resource().clone();
                let home = entry.home_state();
                let after = convert::texture_state(barrier.new_layout);
                rec.go(&res, gpu.handle, home, after);
            }
            // Memory barriers have no per-resource D3D12 twin. When the
            // barrier names unordered access — a UAV write to make visible,
            // or a read of one — the D3D12 answer is the one global UAV
            // barrier, covering every unordered access the list made so far.
            // Barriers without UAV traffic ride the list's program order and
            // the transitions recorded above.
            if memory_barriers.iter().any(|b| {
                (b.src_access | b.dst_access).intersects(AccessFlags::SHADER_WRITE | AccessFlags::SHADER_READ)
            }) {
                rec.uav_barrier();
            }
            Ok(())
        }

        Command::SetViewport(viewport) => {
            let vp = D3D12_VIEWPORT {
                TopLeftX: viewport.x,
                TopLeftY: viewport.y,
                Width: viewport.width,
                Height: viewport.height,
                MinDepth: viewport.min_depth,
                MaxDepth: viewport.max_depth,
            };
            unsafe { rec.list.RSSetViewports(&[vp]) };
            Ok(())
        }

        Command::SetScissor(scissor) => {
            let rect = RECT {
                left: scissor.offset.x,
                top: scissor.offset.y,
                right: scissor
                    .offset
                    .x
                    .saturating_add(scissor.extent.width as i32),
                bottom: scissor
                    .offset
                    .y
                    .saturating_add(scissor.extent.height as i32),
            };
            unsafe { rec.list.RSSetScissorRects(&[rect]) };
            Ok(())
        }

        // ---------- never supported by this backend ----------
        Command::DispatchRays { .. } => Err(RhiError::NotSupported(
            "ray tracing dispatch has no D3D12 path: the backend never created \
             a ray tracing pipeline state or shader table"
                .into(),
        )),
        Command::BuildAccelerationStructure => Err(RhiError::NotSupported(
            "BuildAccelerationStructure has no D3D12 path: BLAS/TLAS construction \
             is not implemented in the backend"
                .into(),
        )),
        Command::BeginDebugLabel { label, .. } => {
            // No marker sink is wired into this recorder; silently swallowing
            // a capture marker hides it from the capture, which is the only
            // thing it exists for.
            Err(RhiError::NotSupported(format!(
                "debug label {label:?} cannot be recorded: the D3D12 recorder \
                 carries no marker sink"
            )))
        }
        Command::EndDebugLabel => Err(RhiError::NotSupported(
            "EndDebugLabel cannot be recorded for the same reason as BeginDebugLabel".into(),
        )),
    }
}

/// One attachment clear the begin decoded: a D3D12 clear names a
/// descriptor, not an attachment, so the positional values turn into
/// whole-target clear facts before anything is recorded.
enum PassClear {
    Colour {
        slot: usize,
        colour: [f32; 4],
    },
    Depth {
        slot: usize,
        depth: f32,
        stencil: u32,
        stencil_plane: bool,
    },
}

/// A range is refused before any part of it is recorded: offset shall lie at
/// or before the limit, and offset + size shall not overtake the end of it.
fn check_range(offset: u64, size: u64, limit: u64, what: &str) -> RhiResult<()> {
    let end = offset.saturating_add(size);
    if end > limit {
        return Err(RhiError::BackendError(format!(
            "the {what} range {offset}..{end} overflows a {limit}-byte resource"
        )));
    }
    Ok(())
}

/// The tracked texture behind a render target: the resource to move, its
/// tracked handle, and the home state the transit leaves from. A
/// heap-pinned home is refused here — a pinned state cannot become a
/// render target either, and emitting a transition for it would only hand
/// the runtime a rejection (the №744 lesson).
fn target_of(
    device: &D3D12Device,
    handle: u64,
) -> RhiResult<(ID3D12Resource, u64, D3D12_RESOURCE_STATES)> {
    let entry = device.texture_entry(handle)?;
    let home = entry.home_state();
    if heap_stuck(home) {
        return Err(RhiError::NotSupported(format!(
            "a render target (texture {handle}) lives in a pinned heap \
             state ({home:?}) that cannot name a transition, let alone a \
             render-state one; the backend refuses instead of recording a \
             barrier the runtime would throw out"
        )));
    }
    Ok((entry.resource().clone(), handle, home))
}

/// The tracked texture behind a standalone clear's view, plus the view
/// itself (the descriptor factories take one): the same pinned-home
/// refusal. The colour/depth split belongs to the individual arms, where
/// the arrived command's demand is known and the mismatch can name it.
fn standalone_target(
    device: &D3D12Device,
    texture: &crate::resource::TextureView,
) -> RhiResult<(
    ID3D12Resource,
    u64,
    D3D12_RESOURCE_STATES,
    super::device::ViewEntry,
)> {
    let gpu = texture.backend().ok_or_else(|| {
        RhiError::BackendError("the clear target view has no GPU backing".into())
    })?;
    let view = device.view_entry(gpu.handle)?;
    let (resource, handle, home) = target_of(device, view.texture_handle())?;
    Ok((resource, handle, home, view))
}

/// The rects of a clear command as the D3D12 clear calls name them; an
/// empty list means the whole target, expressed as `None` — the API's way
/// of saying "no rect windows, clear everything".
fn clear_rects(rects: &[Rect2D]) -> Option<Vec<RECT>> {
    if rects.is_empty() {
        return None;
    }
    Some(
        rects
            .iter()
            .map(|rect| RECT {
                left: rect.offset.x,
                top: rect.offset.y,
                right: rect.offset.x.saturating_add(rect.extent.width as i32),
                bottom: rect.offset.y.saturating_add(rect.extent.height as i32),
            })
            .collect(),
    )
}

/// A human name for the refusal messages, so the M4 refusal arm can point
/// at the command family instead of spelling the same words again.
fn discriminant(command: &Command) -> &'static str {
    match command {
        Command::BindComputePipeline(_) => "BindComputePipeline",
        Command::BindComputeDescriptorSets { .. } => "BindComputeDescriptorSets",
        _ => "the command",
    }
}
