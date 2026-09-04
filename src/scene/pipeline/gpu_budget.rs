//! Per-buffer upload cap, bounded by device limits. This does not measure free VRAM.

use iced::wgpu;

const HARD_CAP_BYTES: usize = 32 * 1024 * 1024;

fn device_ceiling(device: &wgpu::Device) -> usize {
    (usize::try_from(device.limits().max_buffer_size).unwrap_or(usize::MAX) / 10) * 9
}

/// Optional per-buffer cap in MiB, useful for testing smaller chunks.
fn cap_bytes() -> usize {
    static CAP: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CAP.get_or_init(|| {
        std::env::var("OCS_GPU_CHUNK_MIB")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|mib| *mib > 0)
            .and_then(|mib| mib.checked_mul(1024 * 1024))
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

/// Storage buffers must also fit in one binding.
pub fn max_storage_elements<T>(device: &wgpu::Device) -> usize {
    (buffer_budget(device).min(device.limits().max_storage_buffer_binding_size as usize)
        / std::mem::size_of::<T>().max(1))
    .max(1)
}
