//! Buffer creation that survives an out-of-memory device.
//!
//! `wgpu::util::DeviceExt::create_buffer_init` — and any
//! `mapped_at_creation: true` followed by `get_mapped_range_mut()` — maps the
//! new buffer unconditionally. When the allocation fails wgpu hands back an
//! *invalid* buffer, and mapping an invalid buffer panics:
//!
//! ```text
//! thread 'main' panicked at wgpu-29.0.4/src/backend/wgpu_core.rs:2253:18:
//!   Error in Buffer::get_mapped_range: Validation Error
//!   Caused by: Buffer with 'block_wire.vertices' label is invalid
//! ```
//!
//! That panic unwinds through iced's main-thread redraw, trips the `wgpu-hal`
//! swapchain assertion in a destructor, and turns into a **non-unwinding
//! abort** — the process dies with the user's unsaved drawing in it.
//!
//! Writing through the queue instead never maps. A failed allocation produces
//! a validation error on the `write_buffer`, which reaches the handler
//! installed by `install_gpu_error_handler` and degrades the frame, which is
//! what that handler was written for: "A bad frame must degrade, not end the
//! session."

use iced::wgpu;

/// Create a buffer and fill it without ever mapping it.
///
/// `COPY_DST` is added to `usage` because the contents arrive via
/// `Queue::write_buffer`. Callers pass the same usage flags they would have
/// given `create_buffer_init`.
///
/// Empty `data` still produces a one-element buffer: wgpu rejects zero-sized
/// buffers, and every draw path here already guards its instance/vertex count
/// before issuing a draw, so a placeholder allocation is harmless.
pub fn upload_buffer<T: bytemuck::Pod>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    data: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    let bytes: &[u8] = bytemuck::cast_slice(data);
    upload_bytes(device, queue, label, bytes, std::mem::size_of::<T>(), usage)
}

/// `upload_buffer` for data already flattened to bytes. `min_size` is the
/// placeholder size used when `bytes` is empty.
pub fn upload_bytes(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    bytes: &[u8],
    min_size: usize,
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    const ALIGN: usize = wgpu::COPY_BUFFER_ALIGNMENT as usize;
    let logical = bytes.len().max(min_size).max(ALIGN);
    // `write_buffer` copies whole `COPY_BUFFER_ALIGNMENT` units, so both the
    // allocation and the source slice are rounded up to it. Every vertex and
    // instance type here is `#[repr(C)]` over 4-byte fields, so the padding
    // branch below is a safety net rather than a routine cost.
    let size = logical.div_ceil(ALIGN) * ALIGN;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size as u64,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    if !bytes.is_empty() {
        if bytes.len().is_multiple_of(ALIGN) {
            queue.write_buffer(&buffer, 0, bytes);
        } else {
            let mut padded = bytes.to_vec();
            padded.resize(bytes.len().div_ceil(ALIGN) * ALIGN, 0);
            queue.write_buffer(&buffer, 0, &padded);
        }
    }
    buffer
}

/// Allocate `size` bytes and write `data` into the front of it.
///
/// For buffers deliberately larger than their initial contents — the wire
/// arena reserves headroom so later edits patch in place instead of
/// reallocating. Same no-mapping guarantee as [`upload_buffer`]; `usage` must
/// already contain `COPY_DST` for the later patches, and it is added here
/// regardless.
pub fn alloc_with_prefix<T: bytemuck::Pod>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    size: u64,
    data: &[T],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    const ALIGN: u64 = wgpu::COPY_BUFFER_ALIGNMENT;
    let size = size.max(ALIGN).div_ceil(ALIGN) * ALIGN;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bytes: &[u8] = bytemuck::cast_slice(data);
    if !bytes.is_empty() {
        let unit = ALIGN as usize;
        if bytes.len().is_multiple_of(unit) {
            queue.write_buffer(&buffer, 0, bytes);
        } else {
            let mut padded = bytes.to_vec();
            padded.resize(bytes.len().div_ceil(unit) * unit, 0);
            queue.write_buffer(&buffer, 0, &padded);
        }
    }
    buffer
}
