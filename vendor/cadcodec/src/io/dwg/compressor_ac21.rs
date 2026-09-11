//! LZ77 compressor for DWG AC1021 (R2007) format
//!
//! AC1021 uses a different LZ77 variant than AC1018. The opcode encoding
//! and literal copy semantics differ from the AC18 compressor (see spec §5.10).
//!
//! ## Key differences from AC18
//!
//! 1. **Byte reordering** — literal copies use non-trivial byte permutations
//!    for each sub-32-byte length (31 unique patterns) and reversed 8-byte
//!    group order for 32-byte chunks.
//! 2. **Opcode format** — four instruction types based on the upper nibble:
//!    - `0x0_`: Long match (length 19–50, offset 1–4096)
//!    - `0x1_`: Short match (length 3–18, offset 1–8192)
//!    - `0x2_`: Extended match (16-bit offset, variable length)
//!    - `0x3_`–`0xE_`: Compact match (length 3–14, offset 1–512)
//! 3. **Trailing literal count** — each match opcode encodes a 3-bit count
//!    (0–7) of literal bytes that immediately follow the match.
//! 4. **Literal run length** — encoded as `(count - 8)` with continuation
//!    bytes for counts ≥ 23.
//!
//! ## Stream structure (spec §5.10)
//!
//! ```text
//! [literal_length_byte] [reordered_literals]
//! [match_opcode...] [short_literals (0-7)]
//! [match_opcode...] [short_literals (0-7)]
//! ...
//! [literal_length_byte (0x00-0x0F)] [reordered_literals (8+)]
//! ...
//! ```
//!
//! The stream alternates between literal runs (minimum 8 bytes, with byte
//! reordering) and sequences of back-reference matches. Each match's trailing
//! literal count determines whether the next operation is another match
//! (count 1–7 → short literal then match) or a long literal run (count 0 →
//! next byte is a literal length opcode with upper nibble 0).

/// Compress data using the AC1021 LZ77 variant.
///
/// The output can be decompressed by [`super::decompressor_ac21::decompress_ac21`].
///
/// # Arguments
/// * `source` - Uncompressed input data
///
/// # Returns
/// Compressed byte vector
pub fn compress_ac21(source: &[u8]) -> Vec<u8> {
    if source.is_empty() {
        // Empty input: emit a literal length of 8 with no actual data.
        // The decompressor will read the opcode and immediately exit.
        return vec![0x00];
    }

    let mut compressor = Ac21Compressor::new(source);
    compressor.compress()
}

// ---------------------------------------------------------------------------
// Inverse byte-reordering tables
// ---------------------------------------------------------------------------
//
// The decompressor reorders literal bytes during `copy_literal` / `copy_n_reordered`.
// The compressor must apply the *inverse* permutation so that after the decompressor
// applies its reordering, the original byte order is restored.
//
// For each length 1–31, `REORDER_INV[length]` maps: given decompressed index `d`,
// `REORDER_INV[length][d]` is the compressed index where byte `d` must be written.
//
// For 32-byte blocks the reordering is a fixed 8-byte-group reversal (handled
// separately in `reorder_literal_block`).

/// Build the inverse permutation for a length-`n` literal copy.
///
/// We simulate the decompressor's `copy_n_reordered` on an identity permutation
/// [0, 1, 2, …, n-1] to determine which compressed index maps to which
/// decompressed index.  Then we invert that mapping.
fn build_inverse_permutation(n: usize) -> [u8; 32] {
    // Forward permutation: forward[d] = c means compressed byte c goes to decompressed pos d.
    let mut forward = [0u8; 32];

    // Simulate the decompressor's copy: src[si + offset] → dst[di + offset]
    // We treat src as identity [0..n-1] and record where each goes.
    // The copy_Nb functions operate as:
    //   copy_1b(si, di): dst[di] = src[si]
    //   copy_2b(si, di): dst[di] = src[si+1], dst[di+1] = src[si]  (byte-swapped)
    //   copy_3b(si, di): dst[di] = src[si+2], dst[di+1] = src[si+1], dst[di+2] = src[si]
    //   copy_4b(si, di): straight copy of 4 bytes
    //   copy_8b(si, di): straight copy of 8 bytes (two copy_4b)
    //   copy_16b(si, di): copy_8b(si+8, di); copy_8b(si, di+8)  (swap halves)

    // Helper closures that simulate the decompressor's copy functions
    // and record the mapping.
    fn assign_1(fwd: &mut [u8; 32], si: usize, di: usize) {
        fwd[di] = si as u8;
    }
    fn assign_2(fwd: &mut [u8; 32], si: usize, di: usize) {
        // copy_2b byte-swaps
        fwd[di] = (si + 1) as u8;
        fwd[di + 1] = si as u8;
    }
    fn assign_3(fwd: &mut [u8; 32], si: usize, di: usize) {
        // copy_3b reverses
        fwd[di] = (si + 2) as u8;
        fwd[di + 1] = (si + 1) as u8;
        fwd[di + 2] = si as u8;
    }
    fn assign_4(fwd: &mut [u8; 32], si: usize, di: usize) {
        for i in 0..4 {
            fwd[di + i] = (si + i) as u8;
        }
    }
    fn assign_8(fwd: &mut [u8; 32], si: usize, di: usize) {
        assign_4(fwd, si, di);
        assign_4(fwd, si + 4, di + 4);
    }
    fn assign_16(fwd: &mut [u8; 32], si: usize, di: usize) {
        // copy_16b swaps two 8-byte halves
        assign_8(fwd, si + 8, di);
        assign_8(fwd, si, di + 8);
    }

    match n {
        0 => {}
        1 => assign_1(&mut forward, 0, 0),
        2 => assign_2(&mut forward, 0, 0),
        3 => assign_3(&mut forward, 0, 0),
        4 => assign_4(&mut forward, 0, 0),
        5 => {
            assign_1(&mut forward, 4, 0);
            assign_4(&mut forward, 0, 1);
        }
        6 => {
            assign_1(&mut forward, 5, 0);
            assign_4(&mut forward, 1, 1);
            assign_1(&mut forward, 0, 5);
        }
        7 => {
            assign_2(&mut forward, 5, 0);
            assign_4(&mut forward, 1, 2);
            assign_1(&mut forward, 0, 6);
        }
        8 => assign_8(&mut forward, 0, 0),
        9 => {
            assign_1(&mut forward, 8, 0);
            assign_8(&mut forward, 0, 1);
        }
        10 => {
            assign_1(&mut forward, 9, 0);
            assign_8(&mut forward, 1, 1);
            assign_1(&mut forward, 0, 9);
        }
        11 => {
            assign_2(&mut forward, 9, 0);
            assign_8(&mut forward, 1, 2);
            assign_1(&mut forward, 0, 10);
        }
        12 => {
            assign_4(&mut forward, 8, 0);
            assign_8(&mut forward, 0, 4);
        }
        13 => {
            assign_1(&mut forward, 12, 0);
            assign_4(&mut forward, 8, 1);
            assign_8(&mut forward, 0, 5);
        }
        14 => {
            assign_1(&mut forward, 13, 0);
            assign_4(&mut forward, 9, 1);
            assign_8(&mut forward, 1, 5);
            assign_1(&mut forward, 0, 13);
        }
        15 => {
            assign_2(&mut forward, 13, 0);
            assign_4(&mut forward, 9, 2);
            assign_8(&mut forward, 1, 6);
            assign_1(&mut forward, 0, 14);
        }
        16 => assign_16(&mut forward, 0, 0),
        17 => {
            assign_8(&mut forward, 9, 0);
            assign_1(&mut forward, 8, 8);
            assign_8(&mut forward, 0, 9);
        }
        18 => {
            assign_1(&mut forward, 17, 0);
            assign_16(&mut forward, 1, 1);
            assign_1(&mut forward, 0, 17);
        }
        19 => {
            assign_3(&mut forward, 16, 0);
            assign_16(&mut forward, 0, 3);
        }
        20 => {
            assign_4(&mut forward, 16, 0);
            assign_8(&mut forward, 8, 4);
            assign_8(&mut forward, 0, 12);
        }
        21 => {
            assign_1(&mut forward, 20, 0);
            assign_4(&mut forward, 16, 1);
            assign_8(&mut forward, 8, 5);
            assign_8(&mut forward, 0, 13);
        }
        22 => {
            assign_2(&mut forward, 20, 0);
            assign_4(&mut forward, 16, 2);
            assign_8(&mut forward, 8, 6);
            assign_8(&mut forward, 0, 14);
        }
        23 => {
            assign_3(&mut forward, 20, 0);
            assign_4(&mut forward, 16, 3);
            assign_8(&mut forward, 8, 7);
            assign_8(&mut forward, 0, 15);
        }
        24 => {
            assign_8(&mut forward, 16, 0);
            assign_16(&mut forward, 0, 8);
        }
        25 => {
            assign_8(&mut forward, 17, 0);
            assign_1(&mut forward, 16, 8);
            assign_16(&mut forward, 0, 9);
        }
        26 => {
            assign_1(&mut forward, 25, 0);
            assign_8(&mut forward, 17, 1);
            assign_1(&mut forward, 16, 9);
            assign_16(&mut forward, 0, 10);
        }
        27 => {
            assign_2(&mut forward, 25, 0);
            assign_8(&mut forward, 17, 2);
            assign_1(&mut forward, 16, 10);
            assign_16(&mut forward, 0, 11);
        }
        28 => {
            assign_4(&mut forward, 24, 0);
            assign_8(&mut forward, 16, 4);
            assign_8(&mut forward, 8, 12);
            assign_8(&mut forward, 0, 20);
        }
        29 => {
            assign_1(&mut forward, 28, 0);
            assign_4(&mut forward, 24, 1);
            assign_8(&mut forward, 16, 5);
            assign_8(&mut forward, 8, 13);
            assign_8(&mut forward, 0, 21);
        }
        30 => {
            assign_2(&mut forward, 28, 0);
            assign_4(&mut forward, 24, 2);
            assign_8(&mut forward, 16, 6);
            assign_8(&mut forward, 8, 14);
            assign_8(&mut forward, 0, 22);
        }
        31 => {
            assign_1(&mut forward, 30, 0);
            assign_4(&mut forward, 26, 1);
            assign_8(&mut forward, 18, 5);
            assign_8(&mut forward, 10, 13);
            assign_8(&mut forward, 2, 21);
            assign_2(&mut forward, 0, 29);
        }
        _ => unreachable!(),
    }

    // Invert: inverse[d] = c  ⟹  forward[d] = c  already has the right form
    // Actually forward[d] = c means "decompressed position d gets compressed byte c".
    // For compression we want: "given decompressed position d, where to put it in
    // compressed output?" → inverse[d] = the compressed position.
    //
    // forward[d] = c means decompressor reads src[c] and writes dst[d].
    // So for compression: compressed[c] = decompressed[d]  →  inv[d] = c.
    // But forward[d] = c already gives us what we want: inv[d] = forward[d].
    //
    // Wait — forward[d] is the SOURCE index that gets written to DEST index d.
    // So decompressed[d] = compressed[forward[d]].
    // For compression: compressed[forward[d]] = uncompressed[d].
    // We want: given uncompressed index d, what compressed index to write to?
    // Answer: forward[d].
    //
    // So the inverse permutation IS forward itself — forward[d] is the compressed
    // position where uncompressed byte d should be placed.

    forward
}

/// Pre-computed inverse permutation tables for lengths 0–31.
///
/// `REORDER_INV[n][d]` = compressed index where uncompressed byte at position
/// `d` should be written during literal compression.
fn reorder_tables() -> [[u8; 32]; 32] {
    let mut tables = [[0u8; 32]; 32];
    for n in 0..32 {
        tables[n] = build_inverse_permutation(n);
    }
    tables
}

/// Apply inverse byte reordering for literal data before writing to compressed stream.
///
/// For 32-byte chunks: reverses the 4 groups of 8 bytes.
/// For sub-32 chunks: uses the pre-computed inverse permutation table.
#[cfg(test)]
#[allow(dead_code)]
fn reorder_literal_block(src: &[u8], dst: &mut [u8], tables: &[[u8; 32]; 32]) {
    let mut offset = 0;
    let len = src.len();

    // Process full 32-byte blocks
    while offset + 32 <= len {
        // Decompressor reads: c[24..28]→d[0..4], c[28..32]→d[4..8], ...
        // So compressor: d[0..4]→c[24..28], d[4..8]→c[28..32], etc.
        let co = dst.len() - (len - offset); // compressed output offset = same as src offset
        let si = offset;
        // d[24..28] → c[0..4]
        dst[co..co + 4].copy_from_slice(&src[si + 24..si + 28]);
        // d[28..32] → c[4..8]
        dst[co + 4..co + 8].copy_from_slice(&src[si + 28..si + 32]);
        // d[16..20] → c[8..12]
        dst[co + 8..co + 12].copy_from_slice(&src[si + 16..si + 20]);
        // d[20..24] → c[12..16]
        dst[co + 12..co + 16].copy_from_slice(&src[si + 20..si + 24]);
        // d[8..12] → c[16..20]
        dst[co + 16..co + 20].copy_from_slice(&src[si + 8..si + 12]);
        // d[12..16] → c[20..24]
        dst[co + 20..co + 24].copy_from_slice(&src[si + 12..si + 16]);
        // d[0..4] → c[24..28]
        dst[co + 24..co + 28].copy_from_slice(&src[si..si + 4]);
        // d[4..8] → c[28..32]
        dst[co + 28..co + 32].copy_from_slice(&src[si + 4..si + 8]);
        offset += 32;
    }

    // Process remainder (1–31 bytes) using inverse permutation table
    let remaining = len - offset;
    if remaining > 0 {
        let table = &tables[remaining];
        let co = dst.len() - remaining;
        for d in 0..remaining {
            dst[co + table[d] as usize] = src[offset + d];
        }
    }
}

// ---------------------------------------------------------------------------
// Opcode encoders
// ---------------------------------------------------------------------------

/// Opcode encoding result: the bytes to emit and the number of bytes used.
struct MatchEncoding {
    /// Opcode bytes (variable length, 2–5 bytes)
    bytes: [u8; 5],
    /// Number of valid bytes in `bytes`
    len: u8,
}

/// Encode a compact match (upper nibble 3–14).
///
/// - Length: 3–14
/// - Offset: 1–512
/// - Total bytes: 2 (opcode + next_op)
///
/// Spec §5.10: `length = opcode >> 4`, `offset = (opcode & 0x0F) + ((next_op & 0xF8) << 1) + 1`
fn encode_compact(length: u32, offset: u32, trailing_lit: u32) -> MatchEncoding {
    debug_assert!((3..=14).contains(&length));
    debug_assert!((1..=512).contains(&offset));
    debug_assert!(trailing_lit <= 7);

    let o = offset - 1;
    let opcode = (length << 4) | (o & 0x0F);
    let next_op = ((o >> 4) << 3) | (trailing_lit & 0x07);

    MatchEncoding {
        bytes: [opcode as u8, next_op as u8, 0, 0, 0],
        len: 2,
    }
}

/// Encode a short match (upper nibble 1).
///
/// - Length: 3–18
/// - Offset: 1–8192
/// - Total bytes: 3 (opcode + byte1 + next_op)
///
/// Spec §5.10: `length = (opcode & 0x0F) + 3`, `offset = byte1 + ((next_op & 0xF8) << 5) + 1`
fn encode_short(length: u32, offset: u32, trailing_lit: u32) -> MatchEncoding {
    debug_assert!((3..=18).contains(&length));
    debug_assert!((1..=8192).contains(&offset));
    debug_assert!(trailing_lit <= 7);

    let o = offset - 1;
    let opcode = 0x10 | ((length - 3) & 0x0F);
    let byte1 = o & 0xFF;
    let next_op = (((o >> 8) & 0x1F) << 3) | (trailing_lit & 0x07);

    MatchEncoding {
        bytes: [opcode as u8, byte1 as u8, next_op as u8, 0, 0],
        len: 3,
    }
}

/// Encode a long match (upper nibble 0).
///
/// - Length: 19–50
/// - Offset: 1–4096
/// - Total bytes: 3 (opcode + byte1 + next_op)
///
/// Spec §5.10: `length = (opcode & 0x0F) + 0x13 + (next_op >> 3 & 0x10)`,
///             `offset = byte1 + ((next_op & 0x78) << 5) + 1`
fn encode_long(length: u32, offset: u32, trailing_lit: u32) -> MatchEncoding {
    debug_assert!((19..=50).contains(&length));
    debug_assert!((1..=4096).contains(&offset));
    debug_assert!(trailing_lit <= 7);

    let o = offset - 1;
    let len_base = length - 19;
    let opcode = len_base & 0x0F;
    let byte1 = o & 0xFF;
    // Bit 7 of next_op carries the extra length bit (adds 16 if set)
    let extra_bit = if len_base >= 16 { 0x80u32 } else { 0 };
    let next_op = extra_bit | (((o >> 8) & 0x0F) << 3) | (trailing_lit & 0x07);

    MatchEncoding {
        bytes: [opcode as u8, byte1 as u8, next_op as u8, 0, 0],
        len: 3,
    }
}

/// Encode an extended match (upper nibble 2, bit 3 clear).
///
/// - Length: 0–255
/// - Offset: 0–65535 (16-bit, no +1)
/// - Total bytes: 4 (opcode + byte1 + byte2 + next_op)
///
/// Spec §5.10: `offset = byte1 | (byte2 << 8)`, `length = (opcode & 7) + (next_op & 0xF8)`
fn encode_extended(length: u32, offset: u32, trailing_lit: u32) -> MatchEncoding {
    debug_assert!(length <= 255);
    debug_assert!(offset <= 0xFFFF);
    debug_assert!(trailing_lit <= 7);

    let opcode = 0x20 | (length & 0x07);
    let byte1 = offset & 0xFF;
    let byte2 = (offset >> 8) & 0xFF;
    let next_op = (length & 0xF8) | (trailing_lit & 0x07);

    MatchEncoding {
        bytes: [opcode as u8, byte1 as u8, byte2 as u8, next_op as u8, 0],
        len: 4,
    }
}

/// Encode an extended-long match (upper nibble 2, bit 3 set).
///
/// - Length: 256–65791
/// - Offset: 1–65536 (16-bit + 1)
/// - Total bytes: 5 (opcode + byte1 + byte2 + byte3 + next_op)
///
/// Spec §5.10: `offset = (byte1 | (byte2 << 8)) + 1`,
///             `length = (opcode & 7) + (byte3 << 3) + ((next_op & 0xF8) << 8) + 0x100`
fn encode_extended_long(length: u32, offset: u32, trailing_lit: u32) -> MatchEncoding {
    debug_assert!((256..=65791).contains(&length));
    debug_assert!((1..=65536).contains(&offset));
    debug_assert!(trailing_lit <= 7);

    let o = offset - 1;
    let len_adj = length - 0x100;
    let opcode = 0x28 | (len_adj & 0x07);
    let byte1 = o & 0xFF;
    let byte2 = (o >> 8) & 0xFF;
    let byte3 = (len_adj >> 3) & 0xFF;
    let next_op = (((len_adj >> 11) & 0x1F) << 3) | (trailing_lit & 0x07);

    MatchEncoding {
        bytes: [
            opcode as u8,
            byte1 as u8,
            byte2 as u8,
            byte3 as u8,
            next_op as u8,
        ],
        len: 5,
    }
}

/// Choose the most compact opcode encoding for a given match (length, offset).
fn encode_match(length: u32, offset: u32, trailing_lit: u32) -> MatchEncoding {
    // Prefer compact (2 bytes) > short/long (3 bytes) > extended (4 bytes) > extended-long (5 bytes)
    let enc = if length >= 3 && length <= 14 && offset >= 1 && offset <= 512 {
        encode_compact(length, offset, trailing_lit)
    } else if length >= 19 && length <= 50 && offset >= 1 && offset <= 4096 {
        encode_long(length, offset, trailing_lit)
    } else if length >= 3 && length <= 18 && offset >= 1 && offset <= 8192 {
        encode_short(length, offset, trailing_lit)
    } else if length <= 255 && offset <= 0xFFFF {
        // Extended match: offset has no +1, so offset 0 is technically valid
        // but we only call this with offset >= 1
        encode_extended(length, offset, trailing_lit)
    } else if length >= 256 && offset >= 1 && offset <= 65536 {
        encode_extended_long(length, offset, trailing_lit)
    } else {
        // Fallback: clamp to extended-long maximum
        let clamped_len = length.min(65791);
        let clamped_off = offset.min(65536).max(1);
        encode_extended_long(clamped_len, clamped_off, trailing_lit)
    };

    // Verify encode→decode roundtrip in debug mode
    #[cfg(debug_assertions)]
    {
        let (dec_len, dec_off, dec_trail) = verify_decode_match(&enc.bytes[..enc.len as usize]);
        debug_assert_eq!(
            dec_len, length,
            "encode_match roundtrip length mismatch: encoded {}, decoded {} (offset={}, trail={})",
            length, dec_len, offset, trailing_lit
        );
        debug_assert_eq!(
            dec_off, offset,
            "encode_match roundtrip offset mismatch: encoded {}, decoded {} (length={}, trail={})",
            offset, dec_off, length, trailing_lit
        );
        debug_assert_eq!(
            dec_trail, trailing_lit,
            "encode_match roundtrip trailing_lit mismatch: encoded {}, decoded {} (length={}, offset={})",
            trailing_lit, dec_trail, length, offset
        );
    }

    enc
}

// ---------------------------------------------------------------------------
// Literal length encoding (inverse of `read_literal_length` in decompressor)
// ---------------------------------------------------------------------------

/// Encode a literal run length into the compressed stream.
///
/// The decompressor's `read_literal_length` computes: `length = opcode + 8`.
/// For `opcode == 0x0F` (length 23), it reads continuation bytes.
///
/// The opcode byte MUST have upper nibble 0 (values 0x00–0x0F) so that
/// the match-chain loop in the decompressor correctly exits and treats
/// it as a literal length opcode.
/// Decode a match opcode (for verification). Returns (length, offset, trailing_lit).
#[cfg(debug_assertions)]
fn verify_decode_match(bytes: &[u8]) -> (u32, u32, u32) {
    let mut si: u32 = 0;
    let mut op = bytes[si as usize] as u32;
    si += 1;
    let mut length: u32;
    let mut offset: u32;

    match op >> 4 {
        0 => {
            length = (op & 0x0F) + 0x13;
            offset = bytes[si as usize] as u32;
            si += 1;
            op = bytes[si as usize] as u32;
            #[allow(unused_assignments)]
            {
                si += 1;
            }
            length = (op >> 3 & 0x10) + length;
            offset = ((op & 0x78) << 5) + 1 + offset;
        }
        1 => {
            length = (op & 0x0F) + 3;
            offset = bytes[si as usize] as u32;
            si += 1;
            op = bytes[si as usize] as u32;
            #[allow(unused_assignments)]
            {
                si += 1;
            }
            offset = ((op & 0xF8) << 5) + 1 + offset;
        }
        2 => {
            offset = bytes[si as usize] as u32;
            si += 1;
            offset = ((bytes[si as usize] as u32) << 8 & 0xFF00) | offset;
            si += 1;
            length = op & 7;
            if (op & 8) == 0 {
                op = bytes[si as usize] as u32;
                #[allow(unused_assignments)]
                {
                    si += 1;
                }
                length = (op & 0xF8) + length;
            } else {
                offset += 1;
                length = ((bytes[si as usize] as u32) << 3) + length;
                si += 1;
                op = bytes[si as usize] as u32;
                #[allow(unused_assignments)]
                {
                    si += 1;
                }
                length = ((op & 0xF8) << 8) + length + 0x100;
            }
        }
        _ => {
            length = op >> 4;
            offset = op & 0x0F;
            op = bytes[si as usize] as u32;
            #[allow(unused_assignments)]
            {
                si += 1;
            }
            offset = ((op & 0xF8) << 1) + offset + 1;
        }
    }

    let trailing_lit = op & 0x07;
    (length, offset, trailing_lit)
}

fn encode_literal_length(dest: &mut Vec<u8>, length: u32) {
    debug_assert!(length >= 8, "literal runs must be at least 8 bytes");

    let base = length - 8;

    if base < 0x0F {
        // Single byte: value 0x00–0x0E → length 8–22
        dest.push(base as u8);
    } else {
        // First byte is 0x0F (→ length starts at 23)
        dest.push(0x0F);
        let mut remaining = base - 0x0F; // = length - 23

        if remaining < 0xFF {
            dest.push(remaining as u8);
        } else {
            // Emit 0xFF then u16le continuation(s)
            dest.push(0xFF);
            remaining -= 0xFF; // already accounted for 0xFF in the single byte

            loop {
                if remaining < 0xFFFF {
                    dest.push((remaining & 0xFF) as u8);
                    dest.push(((remaining >> 8) & 0xFF) as u8);
                    break;
                } else {
                    dest.push(0xFF);
                    dest.push(0xFF);
                    remaining -= 0xFFFF;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Hash-chain match finder
// ---------------------------------------------------------------------------

const HASH_SIZE: usize = 1 << 15; // 32768 entries
const HASH_MASK: usize = HASH_SIZE - 1;
const MIN_MATCH: u32 = 3;
const MAX_CHAIN: usize = 64; // max chain length to check before giving up

/// 3-byte hash for match finding.
#[inline]
fn hash3(data: &[u8], pos: usize) -> usize {
    let h =
        (data[pos] as usize) * 31 * 31 + (data[pos + 1] as usize) * 31 + (data[pos + 2] as usize);
    h & HASH_MASK
}

// ---------------------------------------------------------------------------
// Main compressor
// ---------------------------------------------------------------------------

struct Ac21Compressor<'a> {
    source: &'a [u8],
    output: Vec<u8>,
    /// Hash chain: head[hash] = most recent position with that hash
    head: Vec<i32>,
    /// Hash chain: prev[pos & WINDOW_MASK] = previous position with same hash
    prev: Vec<i32>,
    /// Pre-computed reorder tables
    reorder: [[u8; 32]; 32],
}

/// Maximum back-reference offset (extended-long max is 65536)
const MAX_OFFSET: usize = 65536;
const WINDOW_MASK: usize = MAX_OFFSET - 1; // for prev[] indexing

impl<'a> Ac21Compressor<'a> {
    fn new(source: &'a [u8]) -> Self {
        Self {
            source,
            output: Vec::with_capacity(source.len()),
            head: vec![-1i32; HASH_SIZE],
            prev: vec![-1i32; MAX_OFFSET],
            reorder: reorder_tables(),
        }
    }

    /// Update hash chain with position `pos`.
    fn insert_hash(&mut self, pos: usize) {
        if pos + 2 >= self.source.len() {
            return;
        }
        let h = hash3(self.source, pos);
        self.prev[pos & WINDOW_MASK] = self.head[h];
        self.head[h] = pos as i32;
    }

    /// Find the longest match at `pos` looking backward.
    /// Returns (best_length, best_offset) or (0, 0) if no match found.
    fn find_match(&self, pos: usize) -> (u32, u32) {
        if pos + 2 >= self.source.len() {
            return (0, 0);
        }

        let h = hash3(self.source, pos);
        let mut chain_pos = self.head[h];
        let mut best_len: u32 = 0;
        let mut best_offset: u32 = 0;
        let mut chain_count = 0;

        let max_len = (self.source.len() - pos) as u32;
        // Cap match length at what the opcodes can encode (65791 for extended-long,
        // but practically we cap at something reasonable)
        let max_match = max_len.min(65791);

        while chain_pos >= 0 && chain_count < MAX_CHAIN {
            let candidate = chain_pos as usize;
            let offset = pos - candidate;

            if offset >= MAX_OFFSET || offset == 0 {
                chain_pos = self.prev[candidate & WINDOW_MASK];
                chain_count += 1;
                continue;
            }

            // Quick check: compare the byte just past current best to prune early
            if best_len >= MIN_MATCH
                && pos + (best_len as usize) < self.source.len()
                && self.source[candidate + best_len as usize]
                    != self.source[pos + best_len as usize]
            {
                chain_pos = self.prev[candidate & WINDOW_MASK];
                chain_count += 1;
                continue;
            }

            // Count matching bytes
            let mut len = 0u32;
            while len < max_match
                && self.source[candidate + len as usize] == self.source[pos + len as usize]
            {
                len += 1;
            }

            if len > best_len {
                best_len = len;
                best_offset = offset as u32;
                if len == max_match {
                    break;
                }
            }

            chain_pos = self.prev[candidate & WINDOW_MASK];
            chain_count += 1;
        }

        if best_len < MIN_MATCH {
            (0, 0)
        } else {
            (best_len, best_offset)
        }
    }

    /// Write reordered literal bytes to the output.
    fn emit_literals(&mut self, start: usize, count: usize) {
        let src = &self.source[start..start + count];
        let base = self.output.len();
        self.output.resize(base + count, 0);
        let dst = &mut self.output[base..base + count];

        // Apply inverse byte reordering
        let mut offset = 0;

        // 32-byte blocks
        while offset + 32 <= count {
            let si = offset;
            let co = offset;
            // d[24..28] → c[0..4]
            dst[co..co + 4].copy_from_slice(&src[si + 24..si + 28]);
            // d[28..32] → c[4..8]
            dst[co + 4..co + 8].copy_from_slice(&src[si + 28..si + 32]);
            // d[16..20] → c[8..12]
            dst[co + 8..co + 12].copy_from_slice(&src[si + 16..si + 20]);
            // d[20..24] → c[12..16]
            dst[co + 12..co + 16].copy_from_slice(&src[si + 20..si + 24]);
            // d[8..12] → c[16..20]
            dst[co + 16..co + 20].copy_from_slice(&src[si + 8..si + 12]);
            // d[12..16] → c[20..24]
            dst[co + 20..co + 24].copy_from_slice(&src[si + 12..si + 16]);
            // d[0..4] → c[24..28]
            dst[co + 24..co + 28].copy_from_slice(&src[si..si + 4]);
            // d[4..8] → c[28..32]
            dst[co + 28..co + 32].copy_from_slice(&src[si + 4..si + 8]);
            offset += 32;
        }

        // Remainder (1–31 bytes)
        let remaining = count - offset;
        if remaining > 0 {
            let table = &self.reorder[remaining];
            for d in 0..remaining {
                dst[offset + table[d] as usize] = src[offset + d];
            }
        }
    }

    /// Main compression entry point.
    fn compress(&mut self) -> Vec<u8> {
        let src_len = self.source.len();
        if src_len == 0 {
            return vec![0x00];
        }

        // For inputs < 8 bytes, we can't use normal literal encoding
        // (the format requires literal runs ≥ 8). Emit the entire input
        // as a single literal run of exactly 8 bytes, padding the reordered
        // output with zeros. The decompressor's caller provides the correct
        // output buffer size, so the extra bytes are harmless.
        if src_len < 8 {
            encode_literal_length(&mut self.output, 8);
            // Pad source to 8 bytes for reordering
            let mut padded = [0u8; 8];
            padded[..src_len].copy_from_slice(self.source);
            let base = self.output.len();
            self.output.resize(base + 8, 0);
            let dst = &mut self.output[base..base + 8];
            // Length 8 has identity permutation — straight copy
            dst.copy_from_slice(&padded);
            return std::mem::take(&mut self.output);
        }

        let mut pos: usize = 0;
        // Track whether the last emission was a match (for trailing literal patching)
        let mut last_was_match = false;

        // The stream must start with a literal run (minimum 8 bytes).
        // find_first_match_after_min_literal inserts hashes for all
        // positions it scans, so we don't re-insert them afterward.
        let first_literal_end = self.find_first_match_after_min_literal(pos);

        // Emit initial literal run
        let literal_count = first_literal_end.min(src_len);
        encode_literal_length(&mut self.output, literal_count as u32);
        self.emit_literals(pos, literal_count);
        // Hashes already inserted by find_first_match_after_min_literal
        pos = literal_count;

        // Main compression loop
        while pos < src_len {
            // Try to find a match at current position
            let (match_len, match_off) = self.find_match(pos);

            if match_len < MIN_MATCH || pos + (match_len as usize) > src_len {
                // No match found — accumulate literals until we find a match
                // or exhaust input.
                //
                // AC1021 format requires strict alternation:
                //   literal (≥8) → match_chain → literal (≥8) → match_chain → …
                // A literal-length opcode signifies the start of a new literal run,
                // and the decompressor enters match-processing mode immediately
                // after consuming the literal bytes.  Two consecutive literals
                // without an intervening match would cause the decompressor to
                // misinterpret the second literal's length byte as a match opcode.
                //
                // To guarantee the invariant we only break when BOTH:
                //  (a) a match is found (ml ≥ MIN_MATCH), AND
                //  (b) enough literal bytes have accumulated (≥ 8).

                let literal_start = pos;
                self.insert_hash(pos);
                pos += 1;

                while pos < src_len {
                    let (ml, _mo) = self.find_match(pos);
                    if ml >= MIN_MATCH && (pos - literal_start) >= 8 {
                        break;
                    }
                    self.insert_hash(pos);
                    pos += 1;
                }

                let lit_len = pos - literal_start;

                if lit_len >= 8 {
                    self.emit_long_literal_run(literal_start, lit_len);
                    last_was_match = false;
                } else if pos >= src_len && last_was_match && lit_len <= 7 {
                    // End of input with < 8 unmatched bytes after a match.
                    // Use the last match's trailing literal field (1-7) to
                    // carry these bytes without needing a literal-length opcode.
                    self.patch_trailing_literal(lit_len as u32);
                    self.emit_trailing_literals(literal_start, lit_len);
                } else if pos >= src_len {
                    // End of input, no preceding match to patch.
                    // Emit as padded literal (at least 8 bytes).
                    let padded_len = lit_len.max(8);
                    encode_literal_length(&mut self.output, padded_len as u32);
                    let mut padded = vec![0u8; padded_len];
                    padded[..lit_len]
                        .copy_from_slice(&self.source[literal_start..literal_start + lit_len]);
                    // Apply reordering for the padded literal
                    let base = self.output.len();
                    self.output.resize(base + padded_len, 0);
                    let dst = &mut self.output[base..base + padded_len];
                    let table = &self.reorder[padded_len];
                    for d in 0..padded_len {
                        dst[table[d] as usize] = padded[d];
                    }
                    last_was_match = false;
                } else {
                    // Should not happen: if pos < src_len, the loop above
                    // guarantees lit_len >= 8 before breaking.
                    unreachable!(
                        "literal accumulation ended with lit_len={} < 8 and pos < src_len",
                        lit_len
                    );
                }

                continue;
            }

            // We have a match. Emit it.
            self.emit_match_sequence(pos, match_len, match_off);
            last_was_match = true;
            // Update hash for matched bytes
            let end = pos + match_len as usize;
            for i in pos..end {
                self.insert_hash(i);
            }
            pos = end;

            // After match, try to chain more matches (using trailing literal counts)
            loop {
                if pos >= src_len {
                    break;
                }

                // Look ahead for the next match
                let (next_len, next_off) = self.find_match(pos);

                if next_len >= MIN_MATCH {
                    // Chain the match directly (trailing_lit = 0)
                    self.emit_chained_match(next_len, next_off);
                    let end = pos + next_len as usize;
                    for i in pos..end {
                        self.insert_hash(i);
                    }
                    pos = end;
                    continue;
                }

                // No match at current pos. Look ahead 1-7 bytes for a match.
                let mut gap = 1usize;
                let mut found_after_gap = false;
                while gap <= 7 && pos + gap < src_len {
                    let (gl, go) = self.find_match(pos + gap);
                    if gl >= MIN_MATCH {
                        // Emit the gap bytes as trailing literals of the
                        // previous match, then chain the new match.
                        self.patch_trailing_literal(gap as u32);
                        self.emit_trailing_literals(pos, gap);
                        for i in pos..pos + gap {
                            self.insert_hash(i);
                        }
                        pos += gap;

                        // Use emit_match_sequence (not emit_chained_match)
                        // because the decompressor reads this opcode at the
                        // START of a fresh copy_decompressed_chunks call,
                        // which does NOT perform the 0xF_ → 0x0_ unmask
                        // that the inner chain loop does.
                        self.emit_match_sequence(pos, gl, go);
                        let end = pos + gl as usize;
                        for i in pos..end {
                            self.insert_hash(i);
                        }
                        pos = end;
                        found_after_gap = true;
                        break;
                    }
                    gap += 1;
                }

                if !found_after_gap {
                    // No nearby match. Check if remaining input is 1-7 bytes
                    // — if so, handle via trailing literals on the last match.
                    let remaining = src_len - pos;
                    if remaining > 0 && remaining <= 7 {
                        self.patch_trailing_literal(remaining as u32);
                        self.emit_trailing_literals(pos, remaining);
                        for i in pos..src_len {
                            self.insert_hash(i);
                        }
                        pos = src_len;
                    }
                    // Break out of match chain loop. If remaining >= 8,
                    // the main loop will emit a literal run.
                    break;
                }
            }
        }

        std::mem::take(&mut self.output)
    }

    /// Find the first position >= 8 where a match exists.
    /// Returns the position (literal end) for the initial literal run.
    fn find_first_match_after_min_literal(&mut self, start: usize) -> usize {
        let src_len = self.source.len();
        if src_len <= 8 {
            return src_len;
        }

        // Insert hashes for first 8 bytes (we need them for matching)
        for i in start..start + 8.min(src_len) {
            self.insert_hash(i);
        }

        let mut pos = start + 8;
        while pos + 2 < src_len {
            self.insert_hash(pos);
            let (ml, _) = self.find_match(pos);
            if ml >= MIN_MATCH {
                return pos;
            }
            pos += 1;
        }
        src_len
    }

    /// Emit a long literal run (≥ 8 bytes) with literal length opcode.
    fn emit_long_literal_run(&mut self, start: usize, count: usize) {
        debug_assert!(count >= 8);
        encode_literal_length(&mut self.output, count as u32);
        self.emit_literals(start, count);
    }

    /// Emit the first match in a sequence (after a literal run).
    /// The trailing literal is initially 0 (no trailing literals).
    fn emit_match_sequence(&mut self, _pos: usize, length: u32, offset: u32) {
        debug_assert!(
            offset as usize <= _pos,
            "emit_match_sequence: offset {} > pos {} (impossible back-reference)",
            offset,
            _pos
        );
        let enc = encode_match(length, offset, 0);
        self.output
            .extend_from_slice(&enc.bytes[..enc.len as usize]);
    }

    /// Emit a chained match (trailing_lit = 0, within the match chain loop).
    /// The opcode's upper nibble must not be 0 (that would break the chain).
    fn emit_chained_match(&mut self, length: u32, offset: u32) {
        let enc = encode_match(length, offset, 0);

        // If the opcode has upper nibble 0 (long match format), the decompressor's
        // chain loop would exit instead of processing it. We need to handle this:
        // - For chaining, upper nibble 0 is NOT allowed (it means "exit chain").
        // - Use the extended format (upper nibble 0x2_) which is valid in any
        //   position. Long matches (length 19–50, offset 1–4096) easily fit in
        //   the extended format (length ≤ 255, offset ≤ 65535).
        if enc.bytes[0] >> 4 == 0 {
            let ext = encode_extended(length, offset, 0);
            self.output
                .extend_from_slice(&ext.bytes[..ext.len as usize]);
        } else {
            self.output
                .extend_from_slice(&enc.bytes[..enc.len as usize]);
        }
    }

    /// Patch the trailing literal count of the last emitted match opcode.
    /// The trailing literal count is always in the low 3 bits of the last
    /// byte of the most recent match encoding.
    fn patch_trailing_literal(&mut self, count: u32) {
        debug_assert!((1..=7).contains(&count));
        if let Some(last) = self.output.last_mut() {
            // Clear low 3 bits and set new trailing literal count
            *last = (*last & 0xF8) | (count as u8 & 0x07);
        }
    }

    /// Emit trailing literal bytes (1–7) that follow a match.
    /// These bytes are NOT reordered — they follow the match opcode directly
    /// and the decompressor copies them via copy_literal with the count from
    /// the trailing literal field.
    fn emit_trailing_literals(&mut self, start: usize, count: usize) {
        debug_assert!((1..=7).contains(&count));
        // Trailing literals go through copy_literal in the decompressor,
        // which applies byte reordering. So we must apply inverse reordering.
        let src = &self.source[start..start + count];
        let base = self.output.len();
        self.output.resize(base + count, 0);
        let dst = &mut self.output[base..base + count];

        // For short counts (1-7), use the reorder table
        let table = &self.reorder[count];
        for d in 0..count {
            dst[table[d] as usize] = src[d];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::io::dwg::decompressor_ac21::decompress_ac21;

    // -----------------------------------------------------------------------
    // Inverse permutation table tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_identity_permutation_lengths_1_4_8() {
        let tables = reorder_tables();
        // Length 1: identity
        assert_eq!(tables[1][0], 0);
        // Length 4: identity
        for i in 0..4 {
            assert_eq!(tables[4][i], i as u8);
        }
        // Length 8: identity
        for i in 0..8 {
            assert_eq!(tables[8][i], i as u8);
        }
    }

    #[test]
    fn test_swap_permutation_length_2() {
        let tables = reorder_tables();
        // copy_2b byte-swaps, so inv[0]=1, inv[1]=0
        assert_eq!(tables[2][0], 1);
        assert_eq!(tables[2][1], 0);
    }

    #[test]
    fn test_reverse_permutation_length_3() {
        let tables = reorder_tables();
        // copy_3b reverses: inv[0]=2, inv[1]=1, inv[2]=0
        assert_eq!(tables[3][0], 2);
        assert_eq!(tables[3][1], 1);
        assert_eq!(tables[3][2], 0);
    }

    #[test]
    fn test_permutation_length_5() {
        let tables = reorder_tables();
        // copy_1b(si+4, di); copy_4b(si, di+1)
        // compressed[4]→d[0], compressed[0..4]→d[1..5]
        // inv[0]=4, inv[1]=0, inv[2]=1, inv[3]=2, inv[4]=3
        assert_eq!(&tables[5][..5], &[4, 0, 1, 2, 3]);
    }

    #[test]
    fn test_permutation_is_valid() {
        let tables = reorder_tables();
        // Each permutation for length n must be a valid permutation of 0..n
        for n in 1..32 {
            let perm = &tables[n][..n];
            let mut seen = vec![false; n];
            for &v in perm {
                assert!((v as usize) < n, "length {}: value {} out of range", n, v);
                assert!(!seen[v as usize], "length {}: duplicate value {}", n, v);
                seen[v as usize] = true;
            }
        }
    }

    #[test]
    fn test_reorder_roundtrip_all_lengths() {
        // For each length 1-31, verify that:
        // reorder → decompressor_copy = identity
        let tables = reorder_tables();

        for n in 1..32usize {
            // Create test data
            let original: Vec<u8> = (0..n as u8).collect();

            // Apply compressor's inverse reorder
            let mut compressed = vec![0u8; n];
            let table = &tables[n];
            for d in 0..n {
                compressed[table[d] as usize] = original[d];
            }

            // Simulate decompressor's forward copy
            let mut decompressed = vec![0u8; n.max(32)]; // pad for safety
            crate::io::dwg::decompressor_ac21::decompress_copy_n_reordered(
                &compressed,
                0,
                &mut decompressed,
                0,
                n,
            );

            assert_eq!(
                &decompressed[..n],
                &original[..],
                "reorder roundtrip failed for length {}",
                n
            );
        }
    }

    // -----------------------------------------------------------------------
    // Opcode encoding tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_encode_compact_basic() {
        // Length 3, offset 1, trailing_lit 0
        let enc = encode_compact(3, 1, 0);
        assert_eq!(enc.len, 2);
        // opcode = (3 << 4) | 0 = 0x30
        assert_eq!(enc.bytes[0], 0x30);
        // next_op = (0 << 3) | 0 = 0x00
        assert_eq!(enc.bytes[1], 0x00);
    }

    #[test]
    fn test_encode_compact_max() {
        // Length 14, offset 512, trailing_lit 7
        let enc = encode_compact(14, 512, 7);
        assert_eq!(enc.len, 2);
        let o = 511u32; // offset - 1
        let opcode = (14 << 4) | (o & 0x0F);
        assert_eq!(enc.bytes[0], opcode as u8);
        let next_op = ((o >> 4) << 3) | 7;
        assert_eq!(enc.bytes[1], next_op as u8);
    }

    #[test]
    fn test_encode_short_basic() {
        let enc = encode_short(5, 100, 2);
        assert_eq!(enc.len, 3);
        assert_eq!(enc.bytes[0], 0x10 | 2); // length-3 = 2
        assert_eq!(enc.bytes[1], 99); // offset-1 = 99
    }

    #[test]
    fn test_encode_long_basic() {
        let enc = encode_long(19, 1, 0);
        assert_eq!(enc.len, 3);
        assert_eq!(enc.bytes[0], 0x00); // len_base = 0
        assert_eq!(enc.bytes[1], 0x00); // offset-1 = 0
    }

    #[test]
    fn test_encode_long_with_extra_bit() {
        // Length 35 = 19 + 16 → requires extra bit
        let enc = encode_long(35, 1, 0);
        assert_eq!(enc.bytes[0], 0x00); // (35-19) & 0x0F = 0
        assert!(enc.bytes[2] & 0x80 != 0); // extra bit set
    }

    #[test]
    fn test_literal_length_encoding() {
        // Length 8 → byte 0x00
        let mut out = vec![];
        encode_literal_length(&mut out, 8);
        assert_eq!(out, vec![0x00]);

        // Length 22 → byte 0x0E
        let mut out = vec![];
        encode_literal_length(&mut out, 22);
        assert_eq!(out, vec![0x0E]);

        // Length 23 → 0x0F, 0x00
        let mut out = vec![];
        encode_literal_length(&mut out, 23);
        assert_eq!(out, vec![0x0F, 0x00]);

        // Length 30 → 0x0F, 0x07
        let mut out = vec![];
        encode_literal_length(&mut out, 30);
        assert_eq!(out, vec![0x0F, 0x07]);
    }

    // -----------------------------------------------------------------------
    // Roundtrip compression/decompression tests
    // -----------------------------------------------------------------------

    /// Helper: compress then decompress and verify roundtrip.
    fn roundtrip(data: &[u8]) {
        let compressed = compress_ac21(data);
        let mut decompressed = vec![0u8; data.len()];
        decompress_ac21(&compressed, 0, compressed.len() as u32, &mut decompressed);
        assert_eq!(
            &decompressed[..],
            data,
            "roundtrip failed: {} bytes compressed to {} bytes",
            data.len(),
            compressed.len()
        );
    }

    #[test]
    fn test_roundtrip_all_zeros_small() {
        roundtrip(&[0u8; 32]);
    }

    #[test]
    fn test_roundtrip_all_zeros_medium() {
        roundtrip(&[0u8; 256]);
    }

    #[test]
    fn test_roundtrip_all_zeros_large() {
        roundtrip(&[0u8; 4096]);
    }

    #[test]
    fn test_roundtrip_sequential() {
        let data: Vec<u8> = (0..=255).cycle().take(512).collect();
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_repeating_pattern() {
        let pattern = b"ABCDEFGH";
        let data: Vec<u8> = pattern.iter().copied().cycle().take(256).collect();
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_exactly_8_bytes() {
        roundtrip(&[1, 2, 3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn test_roundtrip_exactly_32_bytes() {
        let data: Vec<u8> = (0..32).collect();
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_33_bytes() {
        let data: Vec<u8> = (0..33).collect();
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_metadata_size() {
        // File header metadata is 0x110 = 272 bytes
        let data: Vec<u8> = (0..272).map(|i| (i & 0xFF) as u8).collect();
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_random_ish() {
        // Pseudo-random data (deterministic)
        let mut data = vec![0u8; 1024];
        let mut state: u32 = 0xDEADBEEF;
        for byte in data.iter_mut() {
            state = state.wrapping_mul(1103515245).wrapping_add(12345);
            *byte = (state >> 16) as u8;
        }
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_highly_compressible() {
        // Long run of same byte (tests long matches)
        let data = vec![0xAA; 8192];
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_mixed_patterns() {
        let mut data = Vec::with_capacity(2048);
        // Repeating section
        for _ in 0..10 {
            data.extend_from_slice(b"Hello, World! ");
        }
        // Zeros
        data.extend_from_slice(&[0u8; 200]);
        // Sequential
        for i in 0u8..=255 {
            data.push(i);
        }
        // More repeating
        for _ in 0..20 {
            data.extend_from_slice(&[1, 2, 3, 4]);
        }
        roundtrip(&data);
    }

    #[test]
    fn test_roundtrip_small_sizes() {
        // Test sizes 8 through 64
        for size in 8..=64 {
            let data: Vec<u8> = (0..size).map(|i| (i * 7 + 3) as u8).collect();
            roundtrip(&data);
        }
    }

    #[test]
    fn test_compression_ratio_zeros() {
        let data = vec![0u8; 4096];
        let compressed = compress_ac21(&data);
        // Zeros should compress very well
        assert!(
            compressed.len() < data.len() / 2,
            "expected significant compression for zeros: {} -> {}",
            data.len(),
            compressed.len()
        );
    }

    #[test]
    fn test_compression_ratio_repeating() {
        let pattern = b"ABCD";
        let data: Vec<u8> = pattern.iter().copied().cycle().take(4096).collect();
        let compressed = compress_ac21(&data);
        assert!(
            compressed.len() < data.len() / 2,
            "expected significant compression for repeating pattern: {} -> {}",
            data.len(),
            compressed.len()
        );
    }

    // ---- Regression tests for fixed bugs ----

    /// Regression: match followed by < 8 non-matching bytes at end of input.
    /// Previously the backtrack logic re-emitted bytes at incorrect positions.
    #[test]
    fn test_roundtrip_trailing_short_literal_after_match() {
        // 16 bytes: 8 unique, then 3-byte match (offset 8), then 5 unique bytes.
        let data = vec![1, 2, 3, 10, 20, 30, 40, 50, 1, 2, 3, 99, 98, 97, 96, 95];
        roundtrip(&data);
    }

    /// Regression: match followed by exactly 1 non-matching byte at end.
    #[test]
    fn test_roundtrip_trailing_1_byte_after_match() {
        let data = vec![1, 2, 3, 10, 20, 30, 40, 50, 1, 2, 3, 99];
        roundtrip(&data);
    }

    /// Regression: match followed by exactly 7 non-matching bytes at end.
    #[test]
    fn test_roundtrip_trailing_7_bytes_after_match() {
        let data = vec![
            1, 2, 3, 10, 20, 30, 40, 50, 1, 2, 3, 99, 98, 97, 96, 95, 94, 93,
        ];
        roundtrip(&data);
    }

    /// Regression: input < 8 bytes should not panic.
    #[test]
    fn test_roundtrip_very_small_inputs() {
        for size in 1..8 {
            let data: Vec<u8> = (0..size).map(|i| (i * 3 + 7) as u8).collect();
            let compressed = compress_ac21(&data);
            let mut output = vec![0u8; size as usize];
            decompress_ac21(&compressed, 0, compressed.len() as u32, &mut output);
            assert_eq!(&output, &data, "roundtrip failed for {}-byte input", size);
        }
    }

    /// Regression: empty input should produce minimal valid output.
    #[test]
    fn test_roundtrip_empty_input() {
        let compressed = compress_ac21(&[]);
        assert_eq!(compressed, vec![0x00]);
    }

    /// Regression: multiple matches each followed by short gaps, with
    /// a short tail at end of input.
    #[test]
    fn test_roundtrip_multiple_matches_short_tail() {
        // Create data with multiple 3-byte matches and short gaps
        let mut data = vec![0u8; 40];
        // Unique initial literal
        for i in 0..8 {
            data[i] = (i * 11 + 5) as u8;
        }
        // Match at pos 8 (matches pos 0-2)
        data[8] = data[0];
        data[9] = data[1];
        data[10] = data[2];
        // 2 unique bytes
        data[11] = 200;
        data[12] = 201;
        // Match at pos 13 (matches pos 3-5)
        data[13] = data[3];
        data[14] = data[4];
        data[15] = data[5];
        // 3 unique trailing bytes
        data[16] = 210;
        data[17] = 211;
        data[18] = 212;
        // Fill rest with unique data
        for i in 19..40 {
            data[i] = (i * 13 + 1) as u8;
        }

        roundtrip(&data);
    }

    /// Test that small sizes from 8..=15 roundtrip correctly
    /// (these are likely to have matches followed by < 8 trailing bytes).
    #[test]
    fn test_roundtrip_sizes_8_through_20_with_repeats() {
        for size in 8..=20 {
            // Data with some repeating trigrams to force matches
            let mut data = Vec::with_capacity(size);
            for i in 0..size {
                data.push(((i % 5) * 3 + 1) as u8);
            }
            roundtrip(&data);
        }
    }

    /// Regression: long match (len 19-50, offset 1-4096) immediately after
    /// a trailing-literal gap.  Previously emit_chained_match applied the
    /// 0xF0 prefix, but the decompressor reads this opcode at the START of
    /// a fresh copy_decompressed_chunks call which does NOT unmask 0xF_→0x0_.
    #[test]
    fn test_roundtrip_long_match_after_trailing_gap() {
        // Build data:
        //  [8 unique bytes]                     → initial literal
        //  [3 bytes matching offset 8]          → compact match (chain start)
        //  [2 unique bytes]                     → trailing literal gap
        //  [25 bytes matching from earlier]      → long match (len=25, needs upper nibble 0)
        let mut data = vec![0u8; 128];
        // Initial unique literal (8 bytes)
        for i in 0..8 {
            data[i] = (i as u8).wrapping_mul(37).wrapping_add(100);
        }
        // First match: 3 bytes at offset 8 (compact)
        data[8] = data[0];
        data[9] = data[1];
        data[10] = data[2];
        // 2-byte gap (trailing literals)
        data[11] = 0xAA;
        data[12] = 0xBB;
        // Long match: 25 bytes repeating a pattern with offset ≤ 4096
        // Place a 25-byte pattern starting at position 13, that matches
        // position 50 with offset 37.
        for i in 13..38 {
            data[i] = (i as u8).wrapping_mul(19).wrapping_add(7);
        }
        // Copy that pattern at position 50 (offset = 37, well within 4096)
        for i in 0..25 {
            data[50 + i] = data[13 + i];
        }
        // Fill the gap (38..50) with unique data
        for i in 38..50 {
            data[i] = (i as u8).wrapping_mul(53).wrapping_add(200);
        }
        // Fill the rest with unique data
        for i in 75..128 {
            data[i] = (i as u8).wrapping_mul(41).wrapping_add(11);
        }
        roundtrip(&data);
    }

    /// Test encode_match handles all offset/length boundaries correctly.
    #[test]
    fn test_encode_decode_opcode_parity() {
        // Helper: simulate decoder on encoded bytes
        fn decode_match(bytes: &[u8]) -> (u32, u32, u32) {
            let mut source_index: u32 = 1; // skip first byte (opcode)
            let mut op_code = bytes[0] as u32;
            let mut length: u32;
            let mut source_offset: u32;

            // Inline read_instructions logic
            match op_code >> 4 {
                0 => {
                    length = (op_code & 0x0F) + 0x13;
                    source_offset = bytes[source_index as usize] as u32;
                    source_index += 1;
                    op_code = bytes[source_index as usize] as u32;
                    length = (op_code >> 3 & 0x10) + length;
                    source_offset = ((op_code & 0x78) << 5) + 1 + source_offset;
                }
                1 => {
                    length = (op_code & 0x0F) + 3;
                    source_offset = bytes[source_index as usize] as u32;
                    source_index += 1;
                    op_code = bytes[source_index as usize] as u32;
                    source_offset = ((op_code & 0xF8) << 5) + 1 + source_offset;
                }
                2 => {
                    source_offset = bytes[source_index as usize] as u32;
                    source_index += 1;
                    source_offset =
                        ((bytes[source_index as usize] as u32) << 8 & 0xFF00) | source_offset;
                    source_index += 1;
                    let base_len = op_code & 7;
                    if (op_code & 8) == 0 {
                        op_code = bytes[source_index as usize] as u32;
                        length = (op_code & 0xF8) + base_len;
                    } else {
                        source_offset += 1;
                        let hi = (bytes[source_index as usize] as u32) << 3;
                        source_index += 1;
                        op_code = bytes[source_index as usize] as u32;
                        length = ((op_code & 0xF8) << 8) + hi + base_len + 0x100;
                    }
                }
                _ => {
                    length = op_code >> 4;
                    source_offset = op_code & 0x0F;
                    op_code = bytes[source_index as usize] as u32;
                    source_offset = ((op_code & 0xF8) << 1) + source_offset + 1;
                }
            }
            let trailing_lit = op_code & 0x07;
            (length, source_offset, trailing_lit)
        }

        // Compact edge cases
        for &(len, off, tl) in &[(3, 1, 0), (14, 512, 7), (7, 256, 3)] {
            let enc = encode_compact(len, off, tl);
            let (dl, do_, dt) = decode_match(&enc.bytes[..enc.len as usize]);
            assert_eq!(
                (dl, do_, dt),
                (len, off, tl),
                "compact({},{},{})",
                len,
                off,
                tl
            );
        }
        // Short edge cases
        for &(len, off, tl) in &[(3, 1, 0), (18, 8192, 7), (10, 4000, 5)] {
            let enc = encode_short(len, off, tl);
            let (dl, do_, dt) = decode_match(&enc.bytes[..enc.len as usize]);
            assert_eq!(
                (dl, do_, dt),
                (len, off, tl),
                "short({},{},{})",
                len,
                off,
                tl
            );
        }
        // Long edge cases
        for &(len, off, tl) in &[(19, 1, 0), (34, 1, 0), (35, 1, 0), (50, 4096, 7)] {
            let enc = encode_long(len, off, tl);
            let (dl, do_, dt) = decode_match(&enc.bytes[..enc.len as usize]);
            assert_eq!(
                (dl, do_, dt),
                (len, off, tl),
                "long({},{},{})",
                len,
                off,
                tl
            );
        }
        // Extended edge cases
        for &(len, off, tl) in &[(0, 0, 0), (255, 65535, 7), (100, 30000, 4)] {
            let enc = encode_extended(len, off, tl);
            let (dl, do_, dt) = decode_match(&enc.bytes[..enc.len as usize]);
            assert_eq!(
                (dl, do_, dt),
                (len, off, tl),
                "extended({},{},{})",
                len,
                off,
                tl
            );
        }
        // Extended-long edge cases
        for &(len, off, tl) in &[(256, 1, 0), (65791, 65536, 7), (1000, 32768, 3)] {
            let enc = encode_extended_long(len, off, tl);
            let (dl, do_, dt) = decode_match(&enc.bytes[..enc.len as usize]);
            assert_eq!(
                (dl, do_, dt),
                (len, off, tl),
                "ext-long({},{},{})",
                len,
                off,
                tl
            );
        }
    }

    /// Test literal length encode/decode parity for all edge cases.
    #[test]
    fn test_literal_length_parity() {
        fn decode_literal_length(bytes: &[u8]) -> u32 {
            let mut si: u32 = 1;
            let op_code = bytes[0] as u32;
            let mut length: u32 = op_code + 8;
            if length == 0x17 {
                let mut n = bytes[si as usize] as u32;
                si += 1;
                length += n;
                if n == 0xFF {
                    loop {
                        n = bytes[si as usize] as u32;
                        si += 1;
                        n |= (bytes[si as usize] as u32) << 8;
                        si += 1;
                        length += n;
                        if n != 0xFFFF {
                            break;
                        }
                    }
                }
            }
            length
        }

        for &len in &[8, 9, 15, 22, 23, 30, 100, 277, 278, 279, 534, 65814] {
            let mut encoded = vec![];
            encode_literal_length(&mut encoded, len);
            let decoded = decode_literal_length(&encoded);
            assert_eq!(decoded, len, "literal length {} roundtrip failed", len);
        }
    }
}
