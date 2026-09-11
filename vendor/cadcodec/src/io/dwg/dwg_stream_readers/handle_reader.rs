//! DWG Handles section reader
//!
//! Reads the AcDb:Handles section from a DWG file, producing a
//! `HashMap<u64, i64>` mapping handles to file offsets. This is used
//! by the object reader to locate individual objects in the AcDb:AcDbObjects
//! section.
//!
//! Mirrors the reference `DwgHandleReader`.
//!
//! ## Section layout
//!
//! The Handles section consists of multiple chunks, each up to 2032 bytes:
//!
//! ```text
//! ┌─────────────────────────────────────┐
//! │ Chunk:                              │
//! │   Chunk size (RS big-endian)        │
//! │   Handle-offset pairs (MC/SMC)      │
//! │   CRC-16 (RS big-endian)            │
//! └─────────────────────────────────────┘
//! ```

use std::collections::HashMap;

use crate::error::Result;

/// Read the Handles section from raw section bytes (already decompressed).
///
/// The handle section is byte-aligned (not bitstream), consisting of
/// size-delimited chunks with modular-char encoded handle/offset pairs.
///
/// # Arguments
/// * `data` - Complete section buffer (just the handle data, no sentinels)
///
/// # Returns
/// `HashMap<u64, i64>` mapping object handles to their file offsets within
/// the AcDb:AcDbObjects section.
pub fn read_handles(data: &[u8]) -> Result<HashMap<u64, i64>> {
    let mut handle_map: HashMap<u64, i64> = HashMap::new();
    let mut pos: usize = 0;

    // Process chunks until we run out of data
    while pos + 2 <= data.len() {
        // Read big-endian u16 chunk size.
        // Per ODA spec §5.5, the size includes the 2-byte size header itself
        // but NOT the trailing 2-byte CRC.  A size of 2 means an empty
        // terminator chunk (no data, only the size header).
        let size = ((data[pos] as usize) << 8) | (data[pos + 1] as usize);
        pos += 2;

        if size <= 2 || size > 2048 {
            break;
        }

        // Data bytes = size − 2 (subtract the 2-byte size header).
        // This matches the reference `maxSectionOffset = size - 2`.
        let data_bytes = size - 2;
        let chunk_end = (pos + data_bytes).min(data.len());

        // Delta-decode handle + offset pairs
        let mut last_handle: u64 = 0;
        let mut last_offset: i64 = 0;

        while pos < chunk_end {
            // Handle delta (MC — unsigned modular char)
            let (handle_delta, bytes_read) = read_mc(&data[pos..]);
            pos += bytes_read;

            // Offset delta (SMC — signed modular char)
            let (offset_delta, bytes_read) = read_smc(&data[pos..]);
            pos += bytes_read;

            last_handle = last_handle.wrapping_add(handle_delta);
            last_offset = last_offset.wrapping_add(offset_delta);

            handle_map.insert(last_handle, last_offset);
        }

        // Skip CRC (2 bytes big-endian)
        if pos + 2 <= data.len() {
            pos += 2;
        }
    }

    Ok(handle_map)
}

/// Read unsigned Modular Char (MC) from byte slice.
/// Returns (value, bytes_consumed).
fn read_mc(data: &[u8]) -> (u64, usize) {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    let mut i: usize = 0;

    loop {
        if i >= data.len() {
            break;
        }
        let b = data[i];
        i += 1;
        value |= ((b & 0x7F) as u64) << shift;
        if (b & 0x80) == 0 {
            break;
        }
        shift += 7;
    }

    (value, i)
}

/// Read signed Modular Char (SMC) from byte slice.
/// Returns (value, bytes_consumed).
fn read_smc(data: &[u8]) -> (i64, usize) {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    let mut i: usize = 0;
    let mut last_byte: u8 = 0;

    loop {
        if i >= data.len() {
            break;
        }
        let b = data[i];
        i += 1;
        last_byte = b;

        if (b & 0x80) == 0 {
            // Final byte: bits 0-5 = value, bit 6 = sign
            value |= ((b & 0x3F) as u64) << shift;
            break;
        } else {
            value |= ((b & 0x7F) as u64) << shift;
            shift += 7;
        }
    }

    let signed_value = value as i64;
    if (last_byte & 0x40) != 0 {
        (-signed_value, i)
    } else {
        (signed_value, i)
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mc_encoding() {
        // Single byte: 5 → 0x05
        let (val, len) = read_mc(&[0x05]);
        assert_eq!(val, 5);
        assert_eq!(len, 1);

        // Two bytes: 200 = 0b11001000
        // byte0: (200 & 0x7F) | 0x80 = 0xC8
        // byte1: (200 >> 7) = 1 → 0x01
        let (val, len) = read_mc(&[0xC8, 0x01]);
        assert_eq!(val, 200);
        assert_eq!(len, 2);
    }

    #[test]
    fn test_smc_encoding() {
        // Single byte positive: 36 → 0x24 (bit 6 = 0 = positive)
        let (val, _) = read_smc(&[0x24]);
        assert_eq!(val, 36);

        // Single byte negative: -36 → 0x64 (36 | 0x40)
        let (val, _) = read_smc(&[0x64]);
        assert_eq!(val, -36);

        // Multi-byte positive: 100 → first byte 0xE4 (100|0x80), second byte 0x00
        let (val, len) = read_smc(&[0xE4, 0x00]);
        assert_eq!(val, 100);
        assert_eq!(len, 2);
    }

    #[test]
    fn test_handle_reader_basic() {
        // Build a chunk with 2 handle-offset pairs using correct MC/SMC encoding
        let mut chunk_data = Vec::new();

        // Handle 1: MC delta=5 (0x05), SMC offset=100 (0xE4, 0x00)
        chunk_data.push(0x05); // MC: 5
        chunk_data.push(0xE4); // SMC: 100 (multi-byte)
        chunk_data.push(0x00);

        // Handle 2: MC delta=3 (0x03), SMC offset=50 (0x32, positive, single byte)
        chunk_data.push(0x03); // MC: 3
        chunk_data.push(0x32); // SMC: 50

        // Build full section: big-endian size + chunk + CRC
        // Size includes the 2-byte size header itself (matching writer convention)
        let mut data = Vec::new();
        let size = (2 + chunk_data.len()) as u16;
        data.push((size >> 8) as u8);
        data.push((size & 0xFF) as u8);
        data.extend_from_slice(&chunk_data);
        data.push(0x00); // CRC dummy
        data.push(0x00);

        let handles = read_handles(&data).unwrap();
        assert_eq!(handles.len(), 2);
        assert_eq!(handles[&5_u64], 100_i64);
        assert_eq!(handles[&8_u64], 150_i64); // 5+3=8, 100+50=150
    }
}
