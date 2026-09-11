//! LZ77 decompressor for DWG AC18 (R2004+) format
//!
//! This reverses the compression performed by [`super::compression::DwgLZ77AC18Compressor`].
//!
//! ## Encoding format
//!
//! The compressed stream consists of:
//! - **Initial literal run** (length prefix 0x01–0x0F or 0x00+extended)
//! - **Match references** interleaved with trailing literals:
//!   - Short match (0x40–0xFF): offset ≤ 0x400, length 3–14
//!   - Medium match (0x20–0x3F): offset ≤ 0x4000
//!   - Long match (0x10, 0x12–0x1F): offset > 0x4000
//!   - Each match encodes 0–3 trailing literal bytes in its lower bits
//!   - If trailing count is 0, additional literals may follow (prefixed 0x00–0x0F)
//! - **Terminator**: opcode 0x11
//!
//! Based on the ODA (Open Design Alliance) specification and matched against
//! the compressor in [`super::compression`].

/// Decompress AC18 LZ77-compressed data.
///
/// # Arguments
/// * `source` – Compressed byte stream
/// * `decompressed_size` – Expected output size
///
/// # Returns
/// Decompressed data, truncated to `decompressed_size`.
pub fn decompress_ac18(source: &[u8], decompressed_size: usize) -> Vec<u8> {
    let mut output = Vec::with_capacity(decompressed_size);
    let mut si = 0; // source index

    if source.is_empty() || decompressed_size == 0 {
        return output;
    }

    // ── Initial literal run ──
    // The compressor always starts with a literal prefix (>= 4 bytes).
    let first = source[si];
    si += 1;

    let lit_count = if first == 0x00 {
        // Extended literal length: read_ext_length() + 0x0F + 3
        read_ext_length(source, &mut si) + 0x0F + 3
    } else if first < 0x10 {
        // Short literal length: opcode + 3
        first as usize + 3
    } else {
        // Not a literal prefix — put back and read as opcode
        si -= 1;
        0
    };

    copy_literals(source, &mut si, &mut output, lit_count, decompressed_size);

    // ── Main decompression loop ──
    loop {
        if si >= source.len() || output.len() >= decompressed_size {
            break;
        }

        let opcode = source[si];
        si += 1;

        if opcode >= 0x40 {
            // ── Short match ──
            // Offset ≤ 0x400, length 3–14
            let length = ((opcode >> 4) as usize) - 1;
            if si >= source.len() {
                break;
            }
            let b1 = source[si] as usize;
            si += 1;
            let offset = (((opcode as usize) & 0x0C) >> 2 | (b1 << 2)) + 1;
            let trailing = (opcode & 0x03) as usize;

            copy_match(&mut output, offset, length, decompressed_size);
            copy_literals(source, &mut si, &mut output, trailing, decompressed_size);
        } else if opcode >= 0x21 {
            // ── Medium match, short length (0x21–0x3F) ──
            let length = (opcode & 0x1F) as usize + 2;
            if si + 1 >= source.len() {
                break;
            }
            let b1 = source[si] as usize;
            si += 1;
            let b2 = source[si] as usize;
            si += 1;
            let offset = ((b1 >> 2) | (b2 << 6)) + 1;
            let trailing = b1 & 0x03;

            copy_match(&mut output, offset, length, decompressed_size);
            copy_literals(source, &mut si, &mut output, trailing, decompressed_size);
        } else if opcode == 0x20 {
            // ── Medium match, extended length ──
            let length = read_ext_length(source, &mut si) + 0x21;
            if si + 1 >= source.len() {
                break;
            }
            let b1 = source[si] as usize;
            si += 1;
            let b2 = source[si] as usize;
            si += 1;
            let offset = ((b1 >> 2) | (b2 << 6)) + 1;
            let trailing = b1 & 0x03;

            copy_match(&mut output, offset, length, decompressed_size);
            copy_literals(source, &mut si, &mut output, trailing, decompressed_size);
        } else if opcode == 0x11 {
            // ── END marker ──
            break;
        } else if opcode >= 0x10 {
            // ── Long match (0x10, 0x12–0x1F) ──
            // Offset > 0x4000
            let len_bits = (opcode & 0x07) as usize;
            let length = if len_bits == 0 {
                read_ext_length(source, &mut si) + 0x09
            } else {
                len_bits + 2
            };
            if si + 1 >= source.len() {
                break;
            }
            let b1 = source[si] as usize;
            si += 1;
            let b2 = source[si] as usize;
            si += 1;
            let offset = ((b1 >> 2) | (b2 << 6) | ((opcode as usize & 0x08) << 11)) + 0x4000;
            let trailing = b1 & 0x03;

            copy_match(&mut output, offset, length, decompressed_size);
            copy_literals(source, &mut si, &mut output, trailing, decompressed_size);
        } else {
            // ── Literal run (0x00–0x0F, after match with trailing=0) ──
            let count = if opcode == 0x00 {
                read_ext_length(source, &mut si) + 0x0F + 3
            } else {
                opcode as usize + 3
            };
            copy_literals(source, &mut si, &mut output, count, decompressed_size);
        }
    }

    output.truncate(decompressed_size);
    output
}

/// Read an extended length value from the compressed stream.
///
/// The format uses run-length encoding: each 0x00 byte adds 0xFF to the total,
/// and the first non-zero byte adds its value and terminates the sequence.
///
/// This reverses [`DwgLZ77AC18Compressor::write_len()`].
fn read_ext_length(source: &[u8], si: &mut usize) -> usize {
    let mut total = 0usize;
    loop {
        if *si >= source.len() {
            break;
        }
        let b = source[*si];
        *si += 1;
        if b == 0 {
            total += 0xFF;
        } else {
            total += b as usize;
            break;
        }
    }
    total
}

/// Copy literal bytes from the compressed source to the output.
#[inline]
fn copy_literals(
    source: &[u8],
    si: &mut usize,
    output: &mut Vec<u8>,
    count: usize,
    max_size: usize,
) {
    for _ in 0..count {
        if *si >= source.len() || output.len() >= max_size {
            break;
        }
        output.push(source[*si]);
        *si += 1;
    }
}

/// Copy a match from earlier in the output buffer (back-reference).
#[inline]
fn copy_match(output: &mut Vec<u8>, offset: usize, length: usize, max_size: usize) {
    let start = output.len().saturating_sub(offset);
    for i in 0..length {
        if output.len() >= max_size {
            break;
        }
        let idx = start + i;
        if idx < output.len() {
            // Must index one-at-a-time because the output grows during the copy
            // (overlapping matches, e.g., run-length encoding of repeating bytes).
            let byte = output[idx];
            output.push(byte);
        } else {
            // Reference beyond output — emit zero (shouldn't happen in valid data)
            output.push(0);
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::compression::DwgLZ77AC18Compressor;

    /// Roundtrip test: compress then decompress and verify.
    fn roundtrip(data: &[u8]) {
        let mut comp = DwgLZ77AC18Compressor::new();
        let compressed = comp.compress(data, 0, data.len());
        let decompressed = decompress_ac18(&compressed, data.len());
        assert_eq!(
            decompressed.len(),
            data.len(),
            "Length mismatch: expected {}, got {}",
            data.len(),
            decompressed.len()
        );
        assert_eq!(
            &decompressed[..],
            data,
            "Data mismatch after roundtrip (first diff at byte {})",
            decompressed
                .iter()
                .zip(data.iter())
                .position(|(a, b)| a != b)
                .unwrap_or(0)
        );
    }

    #[test]
    fn test_roundtrip_zeros() {
        roundtrip(&vec![0u8; 1000]);
    }

    #[test]
    fn test_roundtrip_small() {
        roundtrip(b"Hello, World! This is a test of the decompressor.");
    }

    #[test]
    fn test_roundtrip_repetitive() {
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(b"ABCDEFGH");
        }
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_sequential() {
        let data: Vec<u8> = (0..=255).cycle().take(2000).collect();
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_mixed() {
        let mut data = Vec::new();
        // Some unique data
        data.extend_from_slice(b"Unique header text 12345");
        // Followed by repetitive data
        for _ in 0..50 {
            data.extend_from_slice(b"REPEAT");
        }
        // More unique data
        data.extend_from_slice(b"Another unique section here");
        // More repetitive
        for _ in 0..30 {
            data.extend_from_slice(b"XY");
        }
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_page_sized() {
        // Test with 0x7400 bytes (typical DWG page size)
        roundtrip(&vec![0xAA; 0x7400]);
    }

    #[test]
    fn test_roundtrip_page_sized_pattern() {
        let mut data = vec![0u8; 0x7400];
        for (i, byte) in data.iter_mut().enumerate() {
            *byte = (i % 256) as u8;
        }
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_minimum_data() {
        // Minimum size that the compressor can handle (needs at least 4 bytes for hash)
        roundtrip(&[1, 2, 3, 4]);
    }

    #[test]
    fn test_roundtrip_large_repetitive() {
        // Large enough to trigger long matches
        roundtrip(&vec![42u8; 100_000]);
    }

    #[test]
    fn test_decompress_empty() {
        let result = decompress_ac18(&[], 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_decompress_zero_target() {
        let result = decompress_ac18(&[0x01, 0xAA, 0xBB, 0xCC, 0xDD, 0x11, 0x00, 0x00], 0);
        assert!(result.is_empty());
    }

    #[test]
    fn test_read_ext_length_simple() {
        let data = [5u8];
        let mut si = 0;
        assert_eq!(read_ext_length(&data, &mut si), 5);
        assert_eq!(si, 1);
    }

    #[test]
    fn test_read_ext_length_extended() {
        // 0, 0, 3 → 0xFF + 0xFF + 3 = 0x201
        let data = [0u8, 0, 3];
        let mut si = 0;
        assert_eq!(read_ext_length(&data, &mut si), 0x201);
        assert_eq!(si, 3);
    }
}
