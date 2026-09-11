//! DWG Handle section writer
//!
//! The handle section maps object handles to their file offsets.
//! Entries are sorted by handle and delta-encoded: each entry stores
//! the difference from the previous handle and file position.
//!
//! ## Chunk format
//!
//! Entries are grouped into chunks of up to 2032 bytes. Each chunk:
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │ 2 bytes  — chunk size (big-endian u16)       │
//! │ N bytes  — delta-encoded handle/offset pairs │
//! │ 2 bytes  — CRC-16 over chunk (big-endian)    │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! The section is terminated by an empty chunk (size=0, CRC only).
//!
//! ## Encoding
//!
//! - **Handle delta**: unsigned modular short (7-bit continuation, MSB flag)
//! - **Offset delta**: signed modular short (7-bit continuation, bit 6 = sign on last byte)
//!
//! Based on the reference `DwgHandleWriter`.

use crate::io::dwg::crc::{crc16, CRC16_SEED};

/// Maximum total chunk bytes (including 2-byte size header, before CRC).
/// Per the ODA spec, chunks can be at most 2032 bytes total.
const MAX_CHUNK_TOTAL: usize = 2032;

/// Write the handle section from a sorted handle→offset map.
///
/// # Arguments
/// * `handle_map` - Handle→file-offset pairs. Will be sorted by handle internally.
/// * `section_offset` - Base offset to add to all positions (0 for AC18 relative, absolute for AC15).
///
/// # Returns
/// Complete handle section bytes (all chunks including terminator).
pub fn write_handles(handle_map: &[(u64, i64)], section_offset: i32) -> Vec<u8> {
    // Sort by handle value
    let mut sorted: Vec<(u64, i64)> = handle_map.to_vec();
    sorted.sort_by_key(|&(h, _)| h);

    let mut output = Vec::with_capacity(sorted.len() * 4 + 64);
    let mut prev_handle: u64 = 0;
    let mut prev_loc: i64 = 0;

    // Start first chunk: 2-byte size placeholder
    let mut chunk_start = output.len();
    output.push(0);
    output.push(0);

    let mut handle_buf = [0u8; 10];
    let mut loc_buf = [0u8; 5];

    for &(handle, position) in &sorted {
        let handle_delta = handle - prev_handle;
        let loc = position + section_offset as i64;
        let loc_delta = loc - prev_loc;

        let handle_size = encode_modular_short_unsigned(handle_delta, &mut handle_buf);
        let loc_size = encode_modular_short_signed(loc_delta as i32, &mut loc_buf);

        // Check if adding this entry would exceed chunk limit (2032 total incl. header)
        let chunk_total_len = output.len() - chunk_start; // includes 2-byte header
        if chunk_total_len + handle_size + loc_size > MAX_CHUNK_TOTAL {
            // Finalize current chunk
            finalize_chunk(&mut output, chunk_start);

            // Start new chunk
            chunk_start = output.len();
            output.push(0);
            output.push(0);

            // Re-encode with reset deltas (prev=0)
            let handle_delta = handle;
            let loc = position + section_offset as i64;
            let loc_delta = loc;

            let handle_size = encode_modular_short_unsigned(handle_delta, &mut handle_buf);
            let loc_size = encode_modular_short_signed(loc_delta as i32, &mut loc_buf);

            output.extend_from_slice(&handle_buf[..handle_size]);
            output.extend_from_slice(&loc_buf[..loc_size]);
        } else {
            output.extend_from_slice(&handle_buf[..handle_size]);
            output.extend_from_slice(&loc_buf[..loc_size]);
        }

        prev_handle = handle;
        prev_loc = loc;
    }

    // Finalize last data chunk
    finalize_chunk(&mut output, chunk_start);

    // Write empty terminator chunk (size=0, CRC)
    let term_start = output.len();
    output.push(0);
    output.push(0);
    finalize_chunk(&mut output, term_start);

    output
}

/// Finalize a chunk: patch the 2-byte BE size header and append 2-byte BE CRC.
fn finalize_chunk(output: &mut Vec<u8>, chunk_start: usize) {
    let chunk_len = (output.len() - chunk_start) as u16;

    // Patch size in big-endian at chunk_start
    output[chunk_start] = (chunk_len >> 8) as u8;
    output[chunk_start + 1] = (chunk_len & 0xFF) as u8;

    // Compute CRC-16 over the entire chunk (including the size header)
    let crc = crc16(CRC16_SEED, &output[chunk_start..]);

    // Append CRC in big-endian
    output.push((crc >> 8) as u8);
    output.push((crc & 0xFF) as u8);
}

/// Encode an unsigned value as a modular short (7-bit continuation encoding).
///
/// Each byte stores 7 data bits. The MSB (bit 7) is set on all bytes
/// except the last, indicating more bytes follow.
///
/// Returns the number of bytes written to `buf`.
fn encode_modular_short_unsigned(mut value: u64, buf: &mut [u8]) -> usize {
    let mut i = 0;
    while value >= 0x80 {
        buf[i] = (value & 0x7F) as u8 | 0x80;
        i += 1;
        value >>= 7;
    }
    buf[i] = value as u8;
    i + 1
}

/// Encode a signed value as a signed modular short.
///
/// For negative values: encode absolute value with bit 6 set on the last byte
/// to indicate the sign.
///
/// For positive values: same as unsigned modular short but uses 6 data bits
/// on the last byte (bit 6 is the sign indicator, clear = positive).
///
/// Returns the number of bytes written to `buf`.
fn encode_modular_short_signed(value: i32, buf: &mut [u8]) -> usize {
    let mut i = 0;
    if value < 0 {
        let mut v = -value;
        while v >= 64 {
            buf[i] = (v as u8 & 0x7F) | 0x80;
            i += 1;
            v >>= 7;
        }
        // Set bit 6 to indicate negative
        buf[i] = v as u8 | 0x40;
        i + 1
    } else {
        let mut v = value;
        while v >= 64 {
            buf[i] = (v as u8 & 0x7F) | 0x80;
            i += 1;
            v >>= 7;
        }
        buf[i] = v as u8;
        i + 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_modular_unsigned_small() {
        let mut buf = [0u8; 10];
        let n = encode_modular_short_unsigned(42, &mut buf);
        assert_eq!(n, 1);
        assert_eq!(buf[0], 42);
    }

    #[test]
    fn test_encode_modular_unsigned_two_bytes() {
        let mut buf = [0u8; 10];
        let n = encode_modular_short_unsigned(200, &mut buf);
        assert_eq!(n, 2);
        // 200 = 0b11001000 → 7-bit: 0b1001000 | 0x80, 0b1
        assert_eq!(buf[0], (200 & 0x7F) as u8 | 0x80); // 0xC8 → 0x48 | 0x80 = 0xC8
        assert_eq!(buf[1], (200 >> 7) as u8); // 1
    }

    #[test]
    fn test_encode_modular_signed_positive() {
        let mut buf = [0u8; 5];
        let n = encode_modular_short_signed(10, &mut buf);
        assert_eq!(n, 1);
        assert_eq!(buf[0], 10);
    }

    #[test]
    fn test_encode_modular_signed_negative() {
        let mut buf = [0u8; 5];
        let n = encode_modular_short_signed(-10, &mut buf);
        assert_eq!(n, 1);
        // -10: |10| < 64, so last byte = 10 | 0x40 = 0x4A
        assert_eq!(buf[0], 10 | 0x40);
    }

    #[test]
    fn test_encode_modular_signed_zero() {
        let mut buf = [0u8; 5];
        let n = encode_modular_short_signed(0, &mut buf);
        assert_eq!(n, 1);
        assert_eq!(buf[0], 0);
    }

    #[test]
    fn test_write_handles_empty() {
        let data = write_handles(&[], 0);

        // Empty map → one data chunk (size=2, just header) + terminator chunk
        // Each chunk: 2-byte size + 2-byte CRC = 4 bytes minimum
        // Data chunk: 2(size=2) + 2(CRC) = 4 bytes
        // Term chunk: 2(size=2) + 2(CRC) = 4 bytes
        assert_eq!(data.len(), 8);

        // Last chunk should be empty (size=0x0002 in BE)
        let term_start = data.len() - 4;
        let term_size = u16::from_be_bytes([data[term_start], data[term_start + 1]]);
        assert_eq!(term_size, 2); // just the 2-byte header itself
    }

    #[test]
    fn test_write_handles_single_entry() {
        let map = vec![(1u64, 100i64)];
        let data = write_handles(&map, 0);

        // Should have at least one data chunk + terminator
        assert!(data.len() > 8);

        // First chunk size (BE u16)
        let chunk_size = u16::from_be_bytes([data[0], data[1]]);
        assert!(chunk_size > 2, "Chunk should contain data beyond header");
    }

    #[test]
    fn test_write_handles_sorted() {
        // Provide unsorted entries — they should be sorted internally
        let map = vec![(5u64, 500i64), (1u64, 100i64), (3u64, 300i64)];
        let data = write_handles(&map, 0);
        assert!(data.len() > 8);
    }

    #[test]
    fn test_write_handles_with_offset() {
        let map = vec![(1u64, 100i64)];
        let data_no_offset = write_handles(&map, 0);
        let data_with_offset = write_handles(&map, 1000);

        // Different offsets produce different encoded data
        assert_ne!(data_no_offset, data_with_offset);
    }

    #[test]
    fn test_chunk_crc_valid() {
        let map = vec![(1u64, 100i64), (2u64, 200i64)];
        let data = write_handles(&map, 0);

        // First chunk: read size (BE), then verify CRC
        let chunk_size = u16::from_be_bytes([data[0], data[1]]) as usize;
        let chunk_data = &data[0..chunk_size];
        let chunk_crc_bytes = &data[chunk_size..chunk_size + 2];
        let stored_crc = u16::from_be_bytes([chunk_crc_bytes[0], chunk_crc_bytes[1]]);
        let computed_crc = crc16(CRC16_SEED, chunk_data);
        assert_eq!(stored_crc, computed_crc, "Chunk CRC mismatch");
    }
}
