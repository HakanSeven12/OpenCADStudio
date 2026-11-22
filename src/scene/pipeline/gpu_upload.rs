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

/// Writes into one buffer, held back so that pieces lying side by side reach
/// the queue as one write.
///
/// The queue makes a staging buffer for every `write_buffer` call, which
/// costs far more than the bytes of one object carry: an edit of 100,000
/// objects spent a second in those calls on Direct3D 12, where building the
/// whole buffer again takes a tenth of one.
#[derive(Default)]
pub(crate) struct WriteBatch {
    bytes: Vec<u8>,
    /// `(offset in the buffer, start in bytes, length)`, in the order given.
    pieces: Vec<(u64, usize, usize)>,
}

impl WriteBatch {
    pub(crate) fn push(&mut self, offset: u64, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.pieces.push((offset, self.bytes.len(), data.len()));
        self.bytes.extend_from_slice(data);
    }

    /// The writes that leave the buffer as the pieces would one by one: in
    /// buffer order, each run of pieces that touch joined into one.
    pub(crate) fn writes(&self) -> Vec<(u64, Vec<u8>)> {
        let bytes = |start: usize, len: usize| &self.bytes[start..start + len];
        let mut order: Vec<usize> = (0..self.pieces.len()).collect();
        order.sort_by_key(|&piece| self.pieces[piece].0);
        let overlap = order.windows(2).any(|pair| {
            let (offset, _, len) = self.pieces[pair[0]];
            offset + len as u64 > self.pieces[pair[1]].0
        });
        if overlap {
            // Joined in buffer order, the later of two pieces over the same
            // bytes could end up under the earlier one.
            return self
                .pieces
                .iter()
                .map(|&(offset, start, len)| (offset, bytes(start, len).to_vec()))
                .collect();
        }
        let mut writes: Vec<(u64, Vec<u8>)> = Vec::new();
        for piece in order {
            let (offset, start, len) = self.pieces[piece];
            match writes.last_mut() {
                Some((first, run)) if *first + run.len() as u64 == offset => {
                    run.extend_from_slice(bytes(start, len));
                }
                _ => writes.push((offset, bytes(start, len).to_vec())),
            }
        }
        writes
    }
}

/// Whether `writes` separate writes cost more than building a buffer of
/// `elements` again.
///
/// A write takes about 11 µs on Direct3D 12 and 1.5 µs on Vulkan, whatever
/// its size, and about as long again when the queue frees its staging
/// buffer. Building the wire buffers again takes 0.2 to 0.7 µs an instance.
/// So a write is worth some thirty instances on Direct3D 12 and five on
/// Vulkan. Thirty-two is used: where that makes Vulkan rebuild sooner than
/// it had to, the two ways cost about the same. A few hundred writes take
/// milliseconds and are never worth a rebuild.
pub(crate) fn rebuild_is_cheaper(writes: usize, elements: u64) -> bool {
    const FEW: usize = 256;
    const INSTANCES_A_WRITE: u64 = 32;
    writes > FEW && writes as u64 * INSTANCES_A_WRITE > elements
}

/// Send `writes` to `buffer`.
pub(crate) fn write_all(queue: &wgpu::Queue, buffer: &wgpu::Buffer, writes: &[(u64, Vec<u8>)]) {
    for (offset, bytes) in writes {
        queue.write_buffer(buffer, *offset, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(pieces: &[(u64, &[u8])]) -> WriteBatch {
        let mut batch = WriteBatch::default();
        for (offset, data) in pieces {
            batch.push(*offset, data);
        }
        batch
    }

    /// Moving every object of a drawing rewrites every slab of the buffer.
    /// They lie one after another, in whatever order the edit names them.
    #[test]
    fn pieces_side_by_side_go_out_as_one_write() {
        let writes = batch(&[
            (4, &[5, 6, 7, 8]),
            (0, &[1, 2, 3, 4]),
            (8, &[9, 10, 11, 12]),
        ])
        .writes();
        assert_eq!(writes, vec![(0, (1..=12).collect::<Vec<u8>>())]);
    }

    #[test]
    fn pieces_apart_stay_apart_and_go_out_in_buffer_order() {
        let writes = batch(&[(32, &[3; 4]), (0, &[1; 4]), (36, &[4; 4]), (16, &[2; 8])]).writes();
        assert_eq!(
            writes,
            vec![
                (0, vec![1; 4]),
                (16, vec![2; 8]),
                (32, vec![3, 3, 3, 3, 4, 4, 4, 4]),
            ]
        );
    }

    /// Joined in buffer order, the later of two pieces over the same bytes
    /// could end up under the earlier one. They go out as they came.
    #[test]
    fn pieces_over_the_same_bytes_go_out_one_by_one_as_given() {
        let given: [(u64, &[u8]); 3] = [(8, &[1; 8]), (4, &[2; 8]), (40, &[3; 4])];
        let writes = batch(&given).writes();
        assert_eq!(
            writes,
            given
                .iter()
                .map(|(offset, data)| (*offset, data.to_vec()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_empty_piece_is_no_write() {
        assert!(batch(&[(8, &[])]).writes().is_empty());
    }

    /// A write costs about what thirty instances cost to build again, so a
    /// patch left with more writes than that share of its buffer is slower
    /// than the rebuild. A few writes are never worth one, whatever the size.
    #[test]
    fn many_separate_writes_cost_more_than_the_rebuild() {
        assert!(!rebuild_is_cheaper(1, 1));
        assert!(!rebuild_is_cheaper(256, 256));
        assert!(rebuild_is_cheaper(2_048, 4_096));
        assert!(rebuild_is_cheaper(50_000, 1_000_000));
        assert!(!rebuild_is_cheaper(5_000, 1_000_000));
        assert!(!rebuild_is_cheaper(1, 100_000));
    }
}
