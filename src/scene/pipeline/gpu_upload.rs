//! Upload through the queue, avoiding direct mapping of a failed allocation.

use iced::wgpu;

/// Create an unmapped buffer and upload its contents, adding COPY_DST usage.
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
