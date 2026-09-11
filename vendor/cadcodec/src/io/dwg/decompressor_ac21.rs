//! LZ77 decompressor for DWG AC1021 (R2007) format
//!
//! AC1021 uses a different LZ77 variant than AC1018. The opcode encoding
//! (instruction format) and copy semantics differ from the AC18 compressor.
//!
//! Based on the reference `DwgLZ77AC21Decompressor`.
//!
//! ## Opcode format
//!
//! The upper nibble of the opcode determines the instruction type:
//! - `0x0_`: Long match (length + 0x13, medium offset)
//! - `0x1_`: Short match (length + 3, medium offset)
//! - `0x2_`: Extended match (16-bit offset, variable length)
//! - `0x3_`–`0xF_`: Compact match (length from nibble, short offset)

/// Decompress AC1021 LZ77 compressed data.
///
/// # Arguments
/// * `source` - Compressed input data
/// * `initial_offset` - Starting offset within source
/// * `length` - Number of compressed bytes to process
/// * `buffer` - Output buffer (must be pre-allocated to decompressed size)
pub fn decompress_ac21(source: &[u8], initial_offset: u32, length: u32, buffer: &mut [u8]) {
    // AC1021 literal copies have a minimum length of 8 and process in chunks of
    // up to 32 bytes.  When the decompressed output is smaller than 32 bytes the
    // internal copy routines (copy_n_reordered / copy_literal) may write past the
    // end of the caller's buffer.  Work around this by using a padded internal
    // buffer when the output is small, then copying the result back.
    const MIN_SAFE: usize = 32;
    if buffer.len() < MIN_SAFE {
        let mut padded = vec![0u8; MIN_SAFE];
        decompress_ac21_inner(source, initial_offset, length, &mut padded);
        buffer.copy_from_slice(&padded[..buffer.len()]);
    } else {
        decompress_ac21_inner(source, initial_offset, length, buffer);
    }
}

/// Inner decompression loop — caller guarantees `buffer.len() >= 32`.
fn decompress_ac21_inner(source: &[u8], initial_offset: u32, length: u32, buffer: &mut [u8]) {
    let mut source_offset: u32 = 0;
    let mut match_length: u32 = 0;
    let mut source_index = initial_offset;
    let mut op_code: u32 = source[source_index as usize] as u32;

    let mut dest_index: u32 = 0;
    let end_index = source_index + length;

    source_index += 1;

    if source_index >= end_index {
        return;
    }

    // Handle initial opcode with upper nibble == 0x20
    if (op_code & 0xF0) == 0x20 {
        source_index += 3;
        match_length = source[source_index as usize - 1] as u32;
        match_length &= 0x07;
    }

    while source_index < end_index {
        // Copy literal bytes
        if match_length == 0 {
            read_literal_length(source, &mut source_index, &mut op_code, &mut match_length);
        }

        copy_literal(
            source,
            &mut source_index,
            buffer,
            &mut dest_index,
            match_length,
        );

        if source_index >= end_index {
            break;
        }

        // Process compressed chunks
        dest_index = copy_decompressed_chunks(
            source,
            end_index,
            buffer,
            dest_index,
            &mut source_index,
            &mut match_length,
            &mut op_code,
            &mut source_offset,
        );
    }
}

/// Copy literal bytes from source to destination using AC1021 byte reordering.
///
/// AC1021 format uses specific byte reordering during literal copies.
/// This matches the C# `copy` method and `m_copyMethods` delegate table exactly.
///
/// For 32-byte chunks: groups of 8 bytes are reversed in order.
/// For sub-32 chunks: each length (1–31) has a specific reordering pattern.
fn copy_literal(src: &[u8], src_index: &mut u32, dst: &mut [u8], dst_index: &mut u32, length: u32) {
    let mut remaining = length;

    // Copy in chunks of 32 (with byte group reordering)
    while remaining >= 32 {
        let si = *src_index as usize;
        let di = *dst_index as usize;

        // Reverse the 4 groups of 8 bytes within the 32-byte block
        copy_4b(src, si + 24, dst, di);
        copy_4b(src, si + 28, dst, di + 4);
        copy_4b(src, si + 16, dst, di + 8);
        copy_4b(src, si + 20, dst, di + 12);
        copy_4b(src, si + 8, dst, di + 16);
        copy_4b(src, si + 12, dst, di + 20);
        copy_4b(src, si, dst, di + 24);
        copy_4b(src, si + 4, dst, di + 28);

        *src_index += 32;
        *dst_index += 32;
        remaining -= 32;
    }

    if remaining > 0 {
        let si = *src_index as usize;
        let di = *dst_index as usize;
        copy_n_reordered(src, si, dst, di, remaining as usize);
        *src_index += remaining;
        *dst_index += remaining;
    }
}

/// Process compressed back-reference chunks.
fn copy_decompressed_chunks(
    src: &[u8],
    end_index: u32,
    dst: &mut [u8],
    mut dest_index: u32,
    source_index: &mut u32,
    match_length: &mut u32,
    op_code: &mut u32,
    source_offset: &mut u32,
) -> u32 {
    *match_length = 0;
    *op_code = src[*source_index as usize] as u32;
    *source_index += 1;

    read_instructions(src, source_index, op_code, match_length, source_offset);

    loop {
        // Copy from already-decompressed data (back-reference)
        copy_back_reference(dst, dest_index, *match_length, *source_offset);
        dest_index += *match_length;

        *match_length = *op_code & 0x07;

        if *match_length != 0 || *source_index >= end_index {
            break;
        }

        *op_code = src[*source_index as usize] as u32;
        *source_index += 1;

        if (*op_code >> 4) == 0 {
            break;
        }

        if (*op_code >> 4) == 15 {
            *op_code &= 15;
        }

        read_instructions(src, source_index, op_code, match_length, source_offset);
    }

    dest_index
}

/// Decode instruction opcodes to determine match length and back-reference offset.
fn read_instructions(
    buffer: &[u8],
    source_index: &mut u32,
    op_code: &mut u32,
    length: &mut u32,
    source_offset: &mut u32,
) {
    match *op_code >> 4 {
        0 => {
            *length = (*op_code & 0x0F) + 0x13;
            *source_offset = buffer[*source_index as usize] as u32;
            *source_index += 1;
            *op_code = buffer[*source_index as usize] as u32;
            *source_index += 1;
            *length = (*op_code >> 3 & 0x10) + *length;
            *source_offset = ((*op_code & 0x78) << 5) + 1 + *source_offset;
        }
        1 => {
            *length = (*op_code & 0x0F) + 3;
            *source_offset = buffer[*source_index as usize] as u32;
            *source_index += 1;
            *op_code = buffer[*source_index as usize] as u32;
            *source_index += 1;
            *source_offset = ((*op_code & 0xF8) << 5) + 1 + *source_offset;
        }
        2 => {
            *source_offset = buffer[*source_index as usize] as u32;
            *source_index += 1;
            *source_offset =
                ((buffer[*source_index as usize] as u32) << 8 & 0xFF00) | *source_offset;
            *source_index += 1;
            *length = *op_code & 7;

            if (*op_code & 8) == 0 {
                *op_code = buffer[*source_index as usize] as u32;
                *source_index += 1;
                *length = (*op_code & 0xF8) + *length;
            } else {
                *source_offset += 1;
                *length = ((buffer[*source_index as usize] as u32) << 3) + *length;
                *source_index += 1;
                *op_code = buffer[*source_index as usize] as u32;
                *source_index += 1;
                *length = ((*op_code & 0xF8) << 8) + *length + 0x100;
            }
        }
        _ => {
            // Default: compact match (upper nibble 3–15)
            *length = *op_code >> 4;
            *source_offset = *op_code & 0x0F;
            *op_code = buffer[*source_index as usize] as u32;
            *source_index += 1;
            *source_offset = ((*op_code & 0xF8) << 1) + *source_offset + 1;
        }
    }
}

/// Read the literal run length from the opcode stream.
fn read_literal_length(buffer: &[u8], source_index: &mut u32, op_code: &mut u32, length: &mut u32) {
    *length = *op_code + 8;
    if *length == 0x17 {
        let mut n = buffer[*source_index as usize] as u32;
        *source_index += 1;
        *length += n;

        if n == 0xFF {
            loop {
                n = buffer[*source_index as usize] as u32;
                *source_index += 1;
                n |= (buffer[*source_index as usize] as u32) << 8;
                *source_index += 1;
                *length += n;
                if n != 0xFFFF {
                    break;
                }
            }
        }
    }
}

/// Copy bytes from a back-reference in the output buffer.
fn copy_back_reference(dst: &mut [u8], dst_index: u32, length: u32, src_offset: u32) {
    if src_offset > dst_index || length == 0 {
        return;
    }
    let mut initial_index = dst_index - src_offset;
    let max_index = initial_index + length;

    let mut di = dst_index;
    while initial_index < max_index {
        if di as usize >= dst.len() || initial_index as usize >= dst.len() {
            break;
        }
        dst[di as usize] = dst[initial_index as usize];
        di += 1;
        initial_index += 1;
    }
}

/// Copy N bytes using the reference byte-reordering copy methods.
///
/// Each length (1–31) has a specific reordering pattern that matches
/// the `m_copyMethods` delegate table in `DwgLZ77AC21Decompressor.cs`.
/// This reordering is essential for correct AC1021 decompression.
///
/// Made public for cross-validation by the compressor's inverse permutation tests.
#[doc(hidden)]
pub fn decompress_copy_n_reordered(
    src: &[u8],
    si: usize,
    dst: &mut [u8],
    di: usize,
    length: usize,
) {
    copy_n_reordered(src, si, dst, di, length);
}

fn copy_n_reordered(src: &[u8], si: usize, dst: &mut [u8], di: usize, length: usize) {
    match length {
        0 => {}
        1 => copy_1b(src, si, dst, di),
        2 => copy_2b(src, si, dst, di),
        3 => copy_3b(src, si, dst, di),
        4 => copy_4b(src, si, dst, di),
        5 => {
            copy_1b(src, si + 4, dst, di);
            copy_4b(src, si, dst, di + 1);
        }
        6 => {
            copy_1b(src, si + 5, dst, di);
            copy_4b(src, si + 1, dst, di + 1);
            copy_1b(src, si, dst, di + 5);
        }
        7 => {
            copy_2b(src, si + 5, dst, di);
            copy_4b(src, si + 1, dst, di + 2);
            copy_1b(src, si, dst, di + 6);
        }
        8 => copy_8b(src, si, dst, di),
        9 => {
            copy_1b(src, si + 8, dst, di);
            copy_8b(src, si, dst, di + 1);
        }
        10 => {
            copy_1b(src, si + 9, dst, di);
            copy_8b(src, si + 1, dst, di + 1);
            copy_1b(src, si, dst, di + 9);
        }
        11 => {
            copy_2b(src, si + 9, dst, di);
            copy_8b(src, si + 1, dst, di + 2);
            copy_1b(src, si, dst, di + 10);
        }
        12 => {
            copy_4b(src, si + 8, dst, di);
            copy_8b(src, si, dst, di + 4);
        }
        13 => {
            copy_1b(src, si + 12, dst, di);
            copy_4b(src, si + 8, dst, di + 1);
            copy_8b(src, si, dst, di + 5);
        }
        14 => {
            copy_1b(src, si + 13, dst, di);
            copy_4b(src, si + 9, dst, di + 1);
            copy_8b(src, si + 1, dst, di + 5);
            copy_1b(src, si, dst, di + 13);
        }
        15 => {
            copy_2b(src, si + 13, dst, di);
            copy_4b(src, si + 9, dst, di + 2);
            copy_8b(src, si + 1, dst, di + 6);
            copy_1b(src, si, dst, di + 14);
        }
        16 => copy_16b(src, si, dst, di),
        17 => {
            copy_8b(src, si + 9, dst, di);
            copy_1b(src, si + 8, dst, di + 8);
            copy_8b(src, si, dst, di + 9);
        }
        18 => {
            copy_1b(src, si + 17, dst, di);
            copy_16b(src, si + 1, dst, di + 1);
            copy_1b(src, si, dst, di + 17);
        }
        19 => {
            copy_3b(src, si + 16, dst, di);
            copy_16b(src, si, dst, di + 3);
        }
        20 => {
            copy_4b(src, si + 16, dst, di);
            copy_8b(src, si + 8, dst, di + 4);
            copy_8b(src, si, dst, di + 12);
        }
        21 => {
            copy_1b(src, si + 20, dst, di);
            copy_4b(src, si + 16, dst, di + 1);
            copy_8b(src, si + 8, dst, di + 5);
            copy_8b(src, si, dst, di + 13);
        }
        22 => {
            copy_2b(src, si + 20, dst, di);
            copy_4b(src, si + 16, dst, di + 2);
            copy_8b(src, si + 8, dst, di + 6);
            copy_8b(src, si, dst, di + 14);
        }
        23 => {
            copy_3b(src, si + 20, dst, di);
            copy_4b(src, si + 16, dst, di + 3);
            copy_8b(src, si + 8, dst, di + 7);
            copy_8b(src, si, dst, di + 15);
        }
        24 => {
            copy_8b(src, si + 16, dst, di);
            copy_16b(src, si, dst, di + 8);
        }
        25 => {
            copy_8b(src, si + 17, dst, di);
            copy_1b(src, si + 16, dst, di + 8);
            copy_16b(src, si, dst, di + 9);
        }
        26 => {
            copy_1b(src, si + 25, dst, di);
            copy_8b(src, si + 17, dst, di + 1);
            copy_1b(src, si + 16, dst, di + 9);
            copy_16b(src, si, dst, di + 10);
        }
        27 => {
            copy_2b(src, si + 25, dst, di);
            copy_8b(src, si + 17, dst, di + 2);
            copy_1b(src, si + 16, dst, di + 10);
            copy_16b(src, si, dst, di + 11);
        }
        28 => {
            copy_4b(src, si + 24, dst, di);
            copy_8b(src, si + 16, dst, di + 4);
            copy_8b(src, si + 8, dst, di + 12);
            copy_8b(src, si, dst, di + 20);
        }
        29 => {
            copy_1b(src, si + 28, dst, di);
            copy_4b(src, si + 24, dst, di + 1);
            copy_8b(src, si + 16, dst, di + 5);
            copy_8b(src, si + 8, dst, di + 13);
            copy_8b(src, si, dst, di + 21);
        }
        30 => {
            copy_2b(src, si + 28, dst, di);
            copy_4b(src, si + 24, dst, di + 2);
            copy_8b(src, si + 16, dst, di + 6);
            copy_8b(src, si + 8, dst, di + 14);
            copy_8b(src, si, dst, di + 22);
        }
        31 => {
            copy_1b(src, si + 30, dst, di);
            copy_4b(src, si + 26, dst, di + 1);
            copy_8b(src, si + 18, dst, di + 5);
            copy_8b(src, si + 10, dst, di + 13);
            copy_8b(src, si + 2, dst, di + 21);
            copy_2b(src, si, dst, di + 29);
        }
        _ => unreachable!("copy_n_reordered called with length >= 32"),
    }
}

/// Copy 1 byte (straight).
#[inline(always)]
fn copy_1b(src: &[u8], si: usize, dst: &mut [u8], di: usize) {
    dst[di] = src[si];
}

/// Copy 2 bytes (byte-swapped — this is critical for AC1021!).
#[inline(always)]
fn copy_2b(src: &[u8], si: usize, dst: &mut [u8], di: usize) {
    dst[di] = src[si + 1];
    dst[di + 1] = src[si];
}

/// Copy 3 bytes (reversed — this is critical for AC1021!).
#[inline(always)]
fn copy_3b(src: &[u8], si: usize, dst: &mut [u8], di: usize) {
    dst[di] = src[si + 2];
    dst[di + 1] = src[si + 1];
    dst[di + 2] = src[si];
}

/// Copy 4 bytes (straight).
#[inline(always)]
fn copy_4b(src: &[u8], si: usize, dst: &mut [u8], di: usize) {
    dst[di] = src[si];
    dst[di + 1] = src[si + 1];
    dst[di + 2] = src[si + 2];
    dst[di + 3] = src[si + 3];
}

/// Copy 8 bytes (two groups of 4, straight order).
#[inline(always)]
fn copy_8b(src: &[u8], si: usize, dst: &mut [u8], di: usize) {
    copy_4b(src, si, dst, di);
    copy_4b(src, si + 4, dst, di + 4);
}

/// Copy 16 bytes (swap two 8-byte halves).
#[inline(always)]
fn copy_16b(src: &[u8], si: usize, dst: &mut [u8], di: usize) {
    copy_8b(src, si + 8, dst, di);
    copy_8b(src, si, dst, di + 8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompress_empty() {
        let source = [0u8; 1];
        let mut buffer = [0u8; 16];
        // Single opcode byte, length = 0 → no output
        decompress_ac21(&source, 0, 0, &mut buffer);
        assert!(buffer.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_copy_back_reference() {
        let mut buf = [0u8; 16];
        buf[0] = 0xAA;
        buf[1] = 0xBB;
        buf[2] = 0xCC;
        // Copy 3 bytes from offset 3 (i.e., from position 0)
        copy_back_reference(&mut buf, 3, 3, 3);
        assert_eq!(buf[3], 0xAA);
        assert_eq!(buf[4], 0xBB);
        assert_eq!(buf[5], 0xCC);
    }

    #[test]
    fn test_copy_back_reference_overlapping() {
        let mut buf = [0u8; 16];
        buf[0] = 0x42;
        // Copy with offset=1 repeats the single byte
        copy_back_reference(&mut buf, 1, 4, 1);
        assert_eq!(&buf[1..5], &[0x42, 0x42, 0x42, 0x42]);
    }
}
