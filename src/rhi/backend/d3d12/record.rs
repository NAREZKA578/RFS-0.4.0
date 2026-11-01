//! Direct3D 12 command translation: recorded [`Command`]s to `ID3D12GraphicsCommandList`.
//!
//! The submit is sequential per the M1/M2 plan (Баг№706): one allocator and
//! one list, reset, recorded, closed, executed and fenced one call after
//! another — so the recorder emits only the calls inside the buffer; the
//! device owns reset/close/execute (see `D3D12Device::submit_sequential`,
//! which every `submit_commands` submission rides).
//!
//! Deliberately narrow, in the shape of the Vulkan translator: the copy and
//! state commands are translated, and the commands belonging to the render
//! path (M3, Баг№707) and the compute path (M4, Баг№708) are refused rather
//! than skipped. A silently dropped command draws a wrong picture and looks
//! like a flaky GPU to whoever hunts it.
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
use super::device::D3D12Device;
use crate::command::commands::Command;
use crate::error::{RhiError, RhiResult};
use crate::resource::TextureDesc;
use crate::types::{AccessFlags, TextureDimensions};
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct3D12::{
    D3D12_PLACED_SUBRESOURCE_FOOTPRINT, D3D12_RESOURCE_BARRIER,
    D3D12_RESOURCE_BARRIER_0, D3D12_RESOURCE_BARRIER_FLAG_NONE,
    D3D12_RESOURCE_BARRIER_TYPE_UAV,
    D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST,
    D3D12_RESOURCE_STATE_COPY_SOURCE, D3D12_RESOURCE_STATE_GENERIC_READ,
    D3D12_RESOURCE_STATES, D3D12_RESOURCE_UAV_BARRIER,
    D3D12_SUBRESOURCE_FOOTPRINT, D3D12_TEXTURE_COPY_LOCATION, D3D12_TEXTURE_COPY_LOCATION_0,
    D3D12_TEXTURE_COPY_TYPE_PLACED_FOOTPRINT, D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX,
    D3D12_VIEWPORT, ID3D12GraphicsCommandList, ID3D12Resource,
};

/// The recorder for one submitted list.
struct Recorder<'a> {
    list: &'a ID3D12GraphicsCommandList,
    device: &'a D3D12Device,
    /// The state the list believes each touched resource to be in. Seeded
    /// with the home `COMMON` (see the module comment); every recorded
    /// transition updates it.
    states: HashMap<u64, D3D12_RESOURCE_STATES>,
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
    };
    for command in commands {
        replay(&mut rec, command)?;
    }
    Ok(())
}

fn replay(rec: &mut Recorder<'_>, command: &Command) -> RhiResult<()> {
    match command {
        // ---------- render and dispatch commands: M3/M4 replace these ----------
        Command::BindGraphicsPipeline(_)
        | Command::BindComputePipeline(_)
        | Command::BindDescriptorSets { .. }
        | Command::BindComputeDescriptorSets { .. }
        | Command::BindVertexBuffers { .. }
        | Command::BindIndexBuffer { .. }
        | Command::Draw { .. }
        | Command::DrawIndexed { .. }
        | Command::BeginRenderPass(_)
        | Command::EndRenderPass
        | Command::NextSubpass
        | Command::ClearColor { .. }
        | Command::ClearDepthStencil { .. }
        | Command::Dispatch { .. } => Err(RhiError::NotSupported(format!(
            "the D3D12 recorder does not replay {} yet: the render-target path \
             (Баг№707, M3) owns the pipeline/root-signature layer, the compute \
             path (Баг№708, M4) owns Dispatch; refusing instead of dropping the \
             command into a wrong picture",
            discriminant(command)
        ))),

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

/// A human name for the refusal messages, so every M3/M4 arm can point at
/// the command family instead of spelling the same words again.
fn discriminant(command: &Command) -> &'static str {
    match command {
        Command::BindGraphicsPipeline(_) => "BindGraphicsPipeline",
        Command::BindComputePipeline(_) => "BindComputePipeline",
        Command::BindDescriptorSets { .. } => "BindDescriptorSets",
        Command::BindComputeDescriptorSets { .. } => "BindComputeDescriptorSets",
        Command::BindVertexBuffers { .. } => "BindVertexBuffers",
        Command::BindIndexBuffer { .. } => "BindIndexBuffer",
        Command::Draw { .. } => "Draw",
        Command::DrawIndexed { .. } => "DrawIndexed",
        Command::BeginRenderPass(_) => "BeginRenderPass",
        Command::EndRenderPass => "EndRenderPass",
        Command::NextSubpass => "NextSubpass",
        Command::ClearColor { .. } => "ClearColor",
        Command::ClearDepthStencil { .. } => "ClearDepthStencil",
        Command::Dispatch { .. } => "Dispatch",
        _ => "the command",
    }
}
