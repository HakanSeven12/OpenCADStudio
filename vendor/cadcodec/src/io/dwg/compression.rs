//! LZ77 compression for DWG format
//!
//! The DWG format uses a custom LZ77 variant for section data compression
//! in AC18 (R2004) and later file formats. Each section page is independently
//! compressed using this algorithm.
//!
//! ## Algorithm overview
//!
//! The compression uses a hash table to find matching sequences in the
//! already-processed data. When a match of 3+ bytes is found, it encodes
//! a back-reference (offset + length) instead of the literal bytes.
//!
//! ## Encoding format (AC18)
//!
//! The compressed stream consists of opcodes followed by data:
//!
//! - **Literal run**: Copies N bytes directly from source
//! - **Short matches** (offset ≤ 0x400, length 2–16): Single opcode byte encodes both
//! - **Medium matches** (offset ≤ 0x4000): Opcode 0x20 + length
//! - **Long matches** (offset > 0x4000): Opcode 0x10 + length
//! - **Terminator**: 0x11, 0x00, 0x00
//!
//! Based on the reference `DwgLZ77AC18Compressor`.

/// LZ77 compressor for the AC18 DWG format variant.
///
/// Uses a hash table of size 0x8000 for finding backward matches.
pub struct DwgLZ77AC18Compressor {
    /// Hash table mapping 4-byte hashes to source positions
    block: Vec<i32>,
}

impl DwgLZ77AC18Compressor {
    /// Create a new compressor instance.
    pub fn new() -> Self {
        Self {
            block: vec![-1; 0x8000],
        }
    }

    /// Reset the hash table for a new compression run.
    fn restart_block(&mut self) {
        for entry in self.block.iter_mut() {
            *entry = -1;
        }
    }

    /// Compress source data into the DWG LZ77 AC18 format.
    ///
    /// # Arguments
    /// - `source`: The uncompressed input data
    /// - `offset`: Starting offset within source
    /// - `total_size`: Number of bytes to compress from offset
    ///
    /// # Returns
    /// Compressed byte vector
    pub fn compress(&mut self, source: &[u8], offset: usize, total_size: usize) -> Vec<u8> {
        self.restart_block();

        let mut dest: Vec<u8> = Vec::with_capacity(total_size);

        let initial_offset = offset;
        let total_offset = initial_offset + total_size;
        let mut curr_offset = initial_offset;
        let mut curr_position = initial_offset + 4;

        let mut compression_offset: usize = 0;
        let mut match_pos: usize = 0;

        while curr_position < total_offset.saturating_sub(0x13) {
            let (found, chunk_offset, chunk_match_pos) =
                self.compress_chunk(source, initial_offset, total_offset, curr_position);

            if !found {
                curr_position += 1;
                continue;
            }

            let mask = curr_position - curr_offset;

            if compression_offset != 0 {
                Self::apply_mask(&mut dest, match_pos, compression_offset, mask);
            }

            Self::write_literal_length(&mut dest, source, curr_offset, mask);
            curr_position += chunk_offset;
            curr_offset = curr_position;
            compression_offset = chunk_offset;
            match_pos = chunk_match_pos;
        }

        // Handle remaining literal bytes
        let literal_length = total_offset - curr_offset;

        if compression_offset != 0 {
            Self::apply_mask(&mut dest, match_pos, compression_offset, literal_length);
        }

        Self::write_literal_length(&mut dest, source, curr_offset, literal_length);

        // Terminator: 0x11, 0x00, 0x00
        dest.push(0x11);
        dest.push(0x00);
        dest.push(0x00);

        dest
    }

    /// Write a variable-length encoding of `len`.
    fn write_len(dest: &mut Vec<u8>, mut len: usize) {
        debug_assert!(len > 0);
        while len > 0xFF {
            len -= 0xFF;
            dest.push(0);
        }
        dest.push(len as u8);
    }

    /// Write an opcode with optional extended length encoding.
    fn write_opcode(dest: &mut Vec<u8>, opcode: u8, compression_offset: usize, threshold: usize) {
        debug_assert!(compression_offset > 0);
        if compression_offset <= threshold {
            dest.push(opcode | (compression_offset as u8 - 2));
        } else {
            dest.push(opcode);
            Self::write_len(dest, compression_offset - threshold);
        }
    }

    /// Write literal bytes with a length prefix.
    fn write_literal_length(dest: &mut Vec<u8>, source: &[u8], start: usize, length: usize) {
        if length == 0 {
            return;
        }

        if length > 3 {
            Self::write_opcode(dest, 0, length - 1, 0x11);
        }

        for i in 0..length {
            dest.push(source[start + i]);
        }
    }

    /// Write a match reference (back-pointer + length).
    fn apply_mask(
        dest: &mut Vec<u8>,
        mut match_position: usize,
        compression_offset: usize,
        mask: usize,
    ) {
        let curr: u8;
        let next: u8;

        if compression_offset >= 0x0F || match_position > 0x400 {
            if match_position <= 0x4000 {
                match_position -= 1;
                // compressed_bytes = next Long Compression Offset + 0x21
                Self::write_opcode(dest, 0x20, compression_offset, 0x21);
            } else {
                match_position -= 0x4000;
                // compressed_bytes = next Long Compression Offset + 9
                Self::write_opcode(
                    dest,
                    0x10 | ((match_position >> 11) as u8 & 8),
                    compression_offset,
                    0x09,
                );
            }

            // offset = (first_byte >> 2) | (read_byte() << 6)
            let mut c = ((match_position & 0xFF) << 2) as u8;
            next = (match_position >> 6) as u8;

            if mask < 4 {
                c |= mask as u8;
            }
            curr = c;
        } else {
            match_position -= 1;
            // compressed_bytes = ((opcode1 & 0xF0) >> 4) - 1
            let mut c =
                ((compression_offset + 1) << 4) as u8 | ((match_position & 0x03) << 2) as u8;
            next = (match_position >> 2) as u8;

            if mask < 4 {
                c |= mask as u8;
            }
            curr = c;
        }

        dest.push(curr);
        dest.push(next);
    }

    /// Try to find a matching sequence at the current position.
    ///
    /// Returns (found, match_length, match_distance) where:
    /// - `found`: Whether a match of 3+ bytes was found
    /// - `match_length`: Length of the match
    /// - `match_distance`: Distance from current position to match
    fn compress_chunk(
        &mut self,
        source: &[u8],
        initial_offset: usize,
        total_offset: usize,
        curr_position: usize,
    ) -> (bool, usize, usize) {
        // Compute hash from 4 bytes at current position
        let v1 = (source[curr_position + 3] as i32) << 6;
        let v2 = v1 ^ source[curr_position + 2] as i32;
        let v3 = (v2 << 5) ^ source[curr_position + 1] as i32;
        let v4 = (v3 << 5) ^ source[curr_position] as i32;
        let mut value_index = ((v4.wrapping_add(v4 >> 5)) & 0x7FFF) as usize;

        let mut value = self.block[value_index];
        let mut match_pos = if value >= 0 {
            curr_position.wrapping_sub(value as usize)
        } else {
            usize::MAX
        };

        if value >= initial_offset as i32 && match_pos <= 0xBFFF {
            // Try secondary hash if primary doesn't match well
            if match_pos > 0x400 && source[curr_position + 3] != source[value as usize + 3] {
                value_index = (value_index & 0x7FF) ^ 0b100000000011111;
                value = self.block[value_index];
                match_pos = if value >= 0 {
                    curr_position.wrapping_sub(value as usize)
                } else {
                    usize::MAX
                };

                if value < initial_offset as i32
                    || match_pos > 0xBFFF
                    || (match_pos > 0x400
                        && source[curr_position + 3] != source[value as usize + 3])
                {
                    self.block[value_index] = curr_position as i32;
                    return (false, 0, 0);
                }
            }

            // Verify at least 3 bytes match
            let v = value as usize;
            if source[curr_position] == source[v]
                && source[curr_position + 1] == source[v + 1]
                && source[curr_position + 2] == source[v + 2]
            {
                // Count total match length
                let mut offset = 3usize;
                let mut index = v + 3;
                let mut curr_off = curr_position + 3;
                while curr_off < total_offset && source[index] == source[curr_off] {
                    offset += 1;
                    index += 1;
                    curr_off += 1;
                }

                self.block[value_index] = curr_position as i32;
                return (offset >= 3, offset, match_pos);
            }
        }

        self.block[value_index] = curr_position as i32;
        (false, 0, 0)
    }
}

impl Default for DwgLZ77AC18Compressor {
    fn default() -> Self {
        Self::new()
    }
}

// ════════════════════════════════════════════════════════════════════════════
//  Tests
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compress_empty() {
        let mut comp = DwgLZ77AC18Compressor::new();
        let data = vec![0u8; 4]; // Minimum: needs at least 4 bytes
        let result = comp.compress(&data, 0, data.len());
        // Should at least have the terminator
        assert!(result.len() >= 3);
        let len = result.len();
        assert_eq!(&result[len - 3..], &[0x11, 0x00, 0x00]);
    }

    #[test]
    fn test_compress_small_data() {
        let mut comp = DwgLZ77AC18Compressor::new();
        let data = b"Hello, World! This is a test.";
        let result = comp.compress(data, 0, data.len());
        // Should end with terminator
        let len = result.len();
        assert_eq!(&result[len - 3..], &[0x11, 0x00, 0x00]);
    }

    #[test]
    fn test_compress_repetitive_data() {
        let mut comp = DwgLZ77AC18Compressor::new();
        // Repetitive data should compress well
        let mut data = Vec::new();
        for _ in 0..100 {
            data.extend_from_slice(b"ABCDEFGH");
        }
        let result = comp.compress(&data, 0, data.len());
        // Compressed should be smaller than original for repetitive data
        assert!(
            result.len() < data.len(),
            "Compressed {} >= original {}",
            result.len(),
            data.len()
        );
    }

    #[test]
    fn test_compress_zeros() {
        let mut comp = DwgLZ77AC18Compressor::new();
        let data = vec![0u8; 1000];
        let result = comp.compress(&data, 0, data.len());
        // All zeros should compress very well
        assert!(result.len() < data.len() / 2);
    }

    #[test]
    fn test_compress_with_offset() {
        let mut comp = DwgLZ77AC18Compressor::new();
        let mut data = vec![0xFFu8; 10]; // Prefix garbage
        data.extend_from_slice(&vec![0u8; 100]); // Actual data
        let result = comp.compress(&data, 10, 100);
        let len = result.len();
        assert_eq!(&result[len - 3..], &[0x11, 0x00, 0x00]);
    }

    #[test]
    fn test_terminator_always_present() {
        let mut comp = DwgLZ77AC18Compressor::new();
        for size in [4, 10, 50, 100, 500] {
            let data = vec![42u8; size];
            let result = comp.compress(&data, 0, data.len());
            let len = result.len();
            assert_eq!(
                &result[len - 3..],
                &[0x11, 0x00, 0x00],
                "Missing terminator for size {}",
                size
            );
        }
    }
}
