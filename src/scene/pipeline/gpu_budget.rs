//! One VRAM-aware budget for every GPU buffer this renderer allocates.
//!
//! `wgpu::Limits::max_buffer_size` is an **API validation limit**, not a
//! statement about physical memory: NVIDIA reports several GB of it on a 2 GB
//! card. Sizing a chunk from that limit therefore produces buffers larger than
//! the whole device, the allocation fails, and the frame dies — issue #203
//! (`mesh_gpu`), #358 (`face3d_gpu`) and the block-wire path were all the same
//! bug rediscovered.
//!
//! `mesh_gpu.rs` was the only site that clamped the result. This module makes
//! that clamp the single shared rule so a new upload path cannot reintroduce
//! the mistake by copying one of the unclamped expressions.

use iced::wgpu;

/// Upper bound on any single buffer, regardless of what the device claims it
/// would validate. Chosen to match the clamp `mesh_gpu` has used since #203:
/// large enough that chunk overhead stays negligible, small enough that a
/// failed allocation is recoverable on a low-VRAM card.
const HARD_CAP_BYTES: usize = 32 * 1024 * 1024;

/// 10% headroom below the device's validation limit, matching the `/ 10 * 9`
/// the individual sites used before they shared this helper.
fn device_ceiling(device: &wgpu::Device) -> usize {
    (device.limits().max_buffer_size as usize / 10) * 9
}

/// Test/debug override, in MiB. Raising it above the real VRAM is the
/// cheapest way to force an out-of-memory path and check that the renderer
/// degrades the frame instead of aborting.
fn cap_bytes() -> usize {
    static CAP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CAP.get_or_init(|| {
        std::env::var("OCS_GPU_CHUNK_MIB")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|mib| *mib > 0)
            .map(|mib| mib * 1024 * 1024)
            .unwrap_or(HARD_CAP_BYTES)
    })
}

/// Largest byte size a single buffer built by this renderer may take.
pub fn buffer_budget(device: &wgpu::Device) -> usize {
    device_ceiling(device).min(cap_bytes())
}

/// How many `T` fit in one buffer. Never returns 0, so callers can pass the
/// result straight to `slice::chunks`, which panics on a zero chunk size.
pub fn max_elements<T>(device: &wgpu::Device) -> usize {
    (buffer_budget(device) / std::mem::size_of::<T>().max(1)).max(1)
}

/// `max_elements` rounded down to a whole number of `group` elements, for
/// vertex data whose primitives must not straddle a chunk boundary: 3 for a
/// triangle list, 6 for the quad expansion the wire shaders use.
///
/// Always returns at least one whole group.
pub fn max_elements_grouped<T>(device: &wgpu::Device, group: usize) -> usize {
    let group = group.max(1);
    (max_elements::<T>(device) / group).max(1) * group
}
