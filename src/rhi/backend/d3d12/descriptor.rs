//! Direct3D 12 Descriptor Heaps
//!
//! The four descriptor heaps the graphics path (Баг№707) draws from, created
//! once per device and handed out from a monotone allocator that reserves
//! contiguous blocks. Shader-visible heaps (CBV/SRV/UAV and sampler) back the
//! root descriptor tables; RTV and DSV stay CPU-only, exactly as the API
//! demands: only CBV/SRV/UAV and sampler heaps may carry the shader-visible
//! flag.
//!
//! Exhaustion is an honest refusal, not a silent wrap: the heaps are one big
//! pool per kind with no growth, so a caller that needs more than the capacity
//! gets a spoken error and not a corrupted descriptor index.
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

use std::sync::atomic::{AtomicUsize, Ordering};

use windows::Win32::Graphics::Direct3D12::{
    D3D12_CPU_DESCRIPTOR_HANDLE, D3D12_DESCRIPTOR_HEAP_DESC, D3D12_DESCRIPTOR_HEAP_FLAG_NONE,
    D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE, D3D12_DESCRIPTOR_HEAP_TYPE,
    D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV, D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
    D3D12_DESCRIPTOR_HEAP_TYPE_RTV, D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
    D3D12_GPU_DESCRIPTOR_HANDLE, ID3D12DescriptorHeap, ID3D12Device,
};

use crate::error::{RhiError, RhiResult};

use super::device::DxCom;

/// One descriptor heap kind the backend owns.
///
/// The flagged visibility follows the API rule directly: only CBV/SRV/UAV and
/// sampler heaps may carry `D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE`,
/// because only those two are ever bound for shader access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DescriptorKind {
    CbvSrvUav,
    Sampler,
    Rtv,
    Dsv,
}

impl DescriptorKind {
    fn heap_type(self) -> D3D12_DESCRIPTOR_HEAP_TYPE {
        match self {
            DescriptorKind::CbvSrvUav => D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV,
            DescriptorKind::Sampler => D3D12_DESCRIPTOR_HEAP_TYPE_SAMPLER,
            DescriptorKind::Rtv => D3D12_DESCRIPTOR_HEAP_TYPE_RTV,
            DescriptorKind::Dsv => D3D12_DESCRIPTOR_HEAP_TYPE_DSV,
        }
    }

    /// The pool capacity per kind. CBV/SRV/UAV holds 65,536 descriptors (the
    /// tier-1 cap of a million stays far beyond this backend's needs and the
    /// big pool pins driver observable memory per descriptor); the sampler
    /// pool sits at the documented 2,048 shader-visible maximum; RTV and DSV
    /// are CPU-only side pools of moderate size, sized to the framebuffer
    /// fan-out the backend realistically makes.
    pub(crate) fn capacity(self) -> usize {
        match self {
            DescriptorKind::CbvSrvUav => 65_536,
            DescriptorKind::Sampler => 2_048,
            DescriptorKind::Rtv => 512,
            DescriptorKind::Dsv => 256,
        }
    }

    pub(crate) fn shader_visible(self) -> bool {
        matches!(self, DescriptorKind::CbvSrvUav | DescriptorKind::Sampler)
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            DescriptorKind::CbvSrvUav => "CBV/SRV/UAV",
            DescriptorKind::Sampler => "sampler",
            DescriptorKind::Rtv => "RTV",
            DescriptorKind::Dsv => "DSV",
        }
    }
}

const KIND_COUNT: usize = 4;

fn kind_index(kind: DescriptorKind) -> usize {
    match kind {
        DescriptorKind::CbvSrvUav => 0,
        DescriptorKind::Sampler => 1,
        DescriptorKind::Rtv => 2,
        DescriptorKind::Dsv => 3,
    }
}

/// The four heaps plus the allocator that carves contiguous blocks from
/// them. Created once per device, shared behind `&self` with atomic
/// allocation counters so parallel callers never overlap.
pub(crate) struct DescriptorHeaps {
    heaps: [DxCom<ID3D12DescriptorHeap>; KIND_COUNT],
    /// Per-kind descriptor stride (`GetDescriptorHandleIncrementSize`), in
    /// bytes. Kept from creation: the increment is constant per heap type.
    increments: [usize; KIND_COUNT],
    cpu_starts: [usize; KIND_COUNT],
    gpu_starts: [u64; KIND_COUNT],
    used: [AtomicUsize; KIND_COUNT],
}

unsafe impl Send for DescriptorHeaps {}
unsafe impl Sync for DescriptorHeaps {}

impl DescriptorHeaps {
    /// Create all four heaps. Every heap gets its full capacity up front:
    /// the pool never grows, and growing after shader-visible binds is a
    /// rebind hazard this shape deliberately refuses by construction.
    pub(crate) fn new(device: &ID3D12Device) -> RhiResult<Self> {
        let kinds = [
            DescriptorKind::CbvSrvUav,
            DescriptorKind::Sampler,
            DescriptorKind::Rtv,
            DescriptorKind::Dsv,
        ];
        let mut heaps = Vec::with_capacity(KIND_COUNT);
        let mut increments = [0usize; KIND_COUNT];
        let mut cpu_starts = [0usize; KIND_COUNT];
        let mut gpu_starts = [0u64; KIND_COUNT];

        for kind in kinds {
            let index = kind_index(kind);
            let desc = D3D12_DESCRIPTOR_HEAP_DESC {
                Type: kind.heap_type(),
                NumDescriptors: kind.capacity() as u32,
                Flags: if kind.shader_visible() {
                    D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE
                } else {
                    D3D12_DESCRIPTOR_HEAP_FLAG_NONE
                },
                NodeMask: 0,
            };
            let heap: ID3D12DescriptorHeap = unsafe { device.CreateDescriptorHeap::<ID3D12DescriptorHeap>(&desc) }
                .map_err(|e| {
                    RhiError::DeviceCreationError(format!(
                        "CreateDescriptorHeap ({}): {e}",
                        kind.name()
                    ))
                })?;
            increments[index] =
                unsafe { device.GetDescriptorHandleIncrementSize(kind.heap_type()) } as usize;
            cpu_starts[index] = unsafe { heap.GetCPUDescriptorHandleForHeapStart() }.ptr;
            gpu_starts[index] = if kind.shader_visible() {
                unsafe { heap.GetGPUDescriptorHandleForHeapStart() }.ptr
            } else {
                0
            };
            heaps.push(DxCom(heap));
        }

        let heaps = heaps.try_into().ok().unwrap();
        let used = [
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
            AtomicUsize::new(0),
        ];
        Ok(Self {
            heaps,
            increments,
            cpu_starts,
            gpu_starts,
            used,
        })
    }

    /// Reserve `count` consecutive descriptors and return the base index.
    /// An empty allocation is valid: it returns the current high-water mark
    /// and consumes none of the pool.
    pub(crate) fn alloc(&self, kind: DescriptorKind, count: usize) -> RhiResult<usize> {
        let slot = kind_index(kind);
        let capacity = kind.capacity();
        let taken = self.used[slot].fetch_add(count, Ordering::AcqRel);
        let total = taken + count;
        if total > capacity {
            // Roll back so a refused allocation does not eat the pool for the
            // next caller: the failure is observable and repeatable.
            self.used[slot].fetch_sub(count, Ordering::AcqRel);
            return Err(RhiError::BackendError(format!(
                "the {} descriptor heap is exhausted: allocating {} more \
                 descriptors would need {} of the {} reserved",
                kind.name(),
                count,
                total,
                capacity
            )));
        }
        Ok(taken)
    }

    /// The CPU descriptor address for one slot in a kind's pool.
    pub(crate) fn cpu(&self, kind: DescriptorKind, index: usize) -> D3D12_CPU_DESCRIPTOR_HANDLE {
        let slot = kind_index(kind);
        D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: self.cpu_starts[slot] + index * self.increments[slot],
        }
    }

    /// The GPU descriptor address for one slot in a shader-visible pool.
    /// CPU-only pools have no GPU address at all — the API does not give
    /// one, and inventing it would point a shader at memory nobody mapped.
    pub(crate) fn gpu(
        &self,
        kind: DescriptorKind,
        index: usize,
    ) -> RhiResult<D3D12_GPU_DESCRIPTOR_HANDLE> {
        let slot = kind_index(kind);
        if !kind.shader_visible() {
            return Err(RhiError::NotSupported(format!(
                "the {} descriptor heap is CPU-only: it has no GPU address",
                kind.name()
            )));
        }
        Ok(D3D12_GPU_DESCRIPTOR_HANDLE {
            ptr: self.gpu_starts[slot] + index as u64 * self.increments[slot] as u64,
        })
    }

    /// The shader-visible CBV/SRV/UAV heap for `SetDescriptorHeaps`.
    pub(crate) fn cbv_srv_uav_heap(&self) -> &ID3D12DescriptorHeap {
        &self.heaps[kind_index(DescriptorKind::CbvSrvUav)].0
    }

    /// The shader-visible sampler heap for `SetDescriptorHeaps`.
    pub(crate) fn sampler_heap(&self) -> &ID3D12DescriptorHeap {
        &self.heaps[kind_index(DescriptorKind::Sampler)].0
    }
}

#[cfg(test)]
mod descriptor_heap_contract {
    //! The allocator contract on a real D3D12 device (the same shape the
    //! release-surface tests already use): contiguous monotone blocks,
    //! CPU/GPU address agreement, CPU-only refusal, honest exhaustion with
    //! rollback, and the empty-allocation no-op.

    use crate::backend::d3d12::descriptor::{DescriptorHeaps, DescriptorKind};
    use crate::error::{RhiError, RhiResult};
    use windows::Win32::Graphics::Direct3D::D3D_FEATURE_LEVEL_12_0;
    use windows::Win32::Graphics::Direct3D12::{D3D12CreateDevice, ID3D12Device};

    /// A bare D3D12 device (default adapter, the FL 12_0 floor the backend
    /// itself uses) — the heap pool needs only the device, and building it
    /// directly keeps the contract tests free of the whole instance path and
    /// its lifetime plumbing.
    fn fresh_pool() -> RhiResult<DescriptorHeaps> {
        let mut device: Option<ID3D12Device> = None;
        unsafe { D3D12CreateDevice::<_, ID3D12Device>(None, D3D_FEATURE_LEVEL_12_0, &mut device) }
            .map_err(|e| RhiError::DeviceCreationError(format!("D3D12CreateDevice: {e}")))?;
        let native = device.ok_or_else(|| {
            RhiError::DeviceCreationError("D3D12CreateDevice returned no device".into())
        })?;
        DescriptorHeaps::new(&native)
    }

    /// Blocks hand out in the order taken, never overlapping: the second
    /// block starts exactly where the first ends.
    #[test]
    fn blocks_are_contiguous_and_monotone() {
        let heaps = fresh_pool().unwrap();
        let first = heaps.alloc(DescriptorKind::CbvSrvUav, 64).unwrap();
        let second = heaps.alloc(DescriptorKind::CbvSrvUav, 32).unwrap();
        assert_eq!(first, 0, "the first block starts at the pool base");
        assert_eq!(
            second,
            64,
            "the next block starts exactly after the previous one"
        );
        // The address arithmetic agrees with the indices: slot 64 of the
        // first block's stride equals the second block's base address.
        assert_eq!(
            heaps.cpu(DescriptorKind::CbvSrvUav, 64).ptr,
            heaps.cpu(DescriptorKind::CbvSrvUav, second).ptr,
            "slot 64 must be the second block's base CPU address"
        );
    }

    /// The GPU view of a slot is the heap's GPU start moved by the stride:
    /// the root descriptor table hands the GPU the same descriptor the CPU
    /// wrote, so both views must agree slot for slot.
    #[test]
    fn cpu_and_gpu_views_agree_slot_by_slot() {
        let heaps = fresh_pool().unwrap();
        let base = heaps.alloc(DescriptorKind::Sampler, 4).unwrap();
        assert_eq!(base, 0);
        let cpu0 = heaps.cpu(DescriptorKind::Sampler, 0);
        let cpu3 = heaps.cpu(DescriptorKind::Sampler, 3);
        let gpu0 = heaps.gpu(DescriptorKind::Sampler, 0).unwrap();
        let gpu3 = heaps.gpu(DescriptorKind::Sampler, 3).unwrap();
        let span = (cpu3.ptr - cpu0.ptr) as u64;
        assert_eq!(
            span,
            gpu3.ptr - gpu0.ptr,
            "the CPU stride and the GPU stride are the same per slot"
        );
    }

    /// RTV and DSV pools are CPU-only by API contract: asking for a GPU
    /// address over them is a spoken refusal, not a fake pointer.
    #[test]
    fn cpu_only_kinds_refuse_gpu_addresses() {
        let heaps = fresh_pool().unwrap();
        for kind in [DescriptorKind::Rtv, DescriptorKind::Dsv] {
            heaps.alloc(kind, 1).unwrap();
            let error = heaps.gpu(kind, 0).unwrap_err();
            match error {
                RhiError::NotSupported(message) => {
                    assert!(
                        message.contains("CPU-only"),
                        "the refusal must name the CPU-only contract, got: {message}"
                    );
                }
                other => panic!("expected NotSupported, got: {other:?}"),
            }
        }
    }

    /// Exhaustion is a spoken refusal that rolls the counter back, so a
    /// refused over-allocation does not eat the pool for the next caller.
    #[test]
    fn exhaustion_refuses_and_rolls_back() {
        let heaps = fresh_pool().unwrap();
        let capacity = DescriptorKind::Dsv.capacity();
        assert_eq!(capacity, 256, "the DSV pool is the documented size");
        let base = heaps.alloc(DescriptorKind::Dsv, capacity).unwrap();
        assert_eq!(base, 0, "the full pool is one block from the base");
        let error = heaps.alloc(DescriptorKind::Dsv, 1).unwrap_err();
        match error {
            RhiError::BackendError(message) => {
                assert!(
                    message.contains("exhausted") && message.contains("256"),
                    "the exhaustion refusal names the capacity, got: {message}"
                );
            }
            other => panic!("expected a backend refusal, got: {other:?}"),
        }
        // The rollback: the refused allocation left no mark, the caller
        // retrying a smaller request succeeds.
        assert_eq!(
            heaps.alloc(DescriptorKind::Dsv, 0).unwrap(),
            capacity,
            "an empty allocation lands at the high-water mark"
        );
    }

    /// An empty allocation costs nothing: the mark stays, and the first
    /// real allocation starts at the same place free blocks started.
    #[test]
    fn empty_allocations_cost_nothing() {
        let heaps = fresh_pool().unwrap();
        let probe = heaps.alloc(DescriptorKind::Rtv, 0).unwrap();
        let probe_again = heaps.alloc(DescriptorKind::Rtv, 0).unwrap();
        assert_eq!(probe, probe_again, "empty allocations never advance");
        let first = heaps.alloc(DescriptorKind::Rtv, 8).unwrap();
        assert_eq!(first, 0, "the first real block starts at the base");
    }
}

