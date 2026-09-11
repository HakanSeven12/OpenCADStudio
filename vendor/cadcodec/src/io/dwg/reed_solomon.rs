//! Reed-Solomon encoding and decoding for DWG AC1021 (R2007) format.
//!
//! The AC1021 file format uses Reed-Solomon RS(255, k) codes over GF(2^8) to
//! provide error-resilient encoding of the file header and page data.
//!
//! Two RS configurations are used (spec §5.13):
//!
//! | Config | (n, k) | Parity | Primitive polynomial | Use |
//! |--------|--------|--------|---------------------|-----|
//! | Data pages | (255, 251) | 4 | x⁸+x⁴+x³+x²+1 = 0x11D | Per-page data encoding |
//! | System pages | (255, 239) | 16 | x⁸+x⁶+x⁵+x³+1 = 0x169 | File header, page map, section map |
//!
//! On decode, data is de-interleaved and parity symbols are discarded (no
//! error correction is performed by our reader).
//!
//! On encode, real GF(2^8) parity bytes are computed so that other readers
//! (AutoCAD, ODA) can verify and error-correct the data.

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// RS codeword length (n) — always 255 for DWG.
pub const RS_N: usize = 255;

/// RS(255, 251) configuration for data section pages.
/// Primitive polynomial: x⁸ + x⁴ + x³ + x² + 1  (coefficients 1,0,1,1,1,0,0,0 from x⁰).
pub const RS_DATA_PRIM_POLY: u16 = 0x11D;
/// Data bytes per codeword for data pages.
pub const RS_DATA_K: usize = 251;
/// Parity bytes per codeword for data pages.
pub const RS_DATA_PARITY: usize = RS_N - RS_DATA_K; // 4

/// RS(255, 239) configuration for system pages / file header.
/// Primitive polynomial: x⁸ + x⁶ + x⁵ + x³ + 1  (coefficients 1,0,0,1,0,1,1,0 from x⁰).
pub const RS_SYSTEM_PRIM_POLY: u16 = 0x169;
/// Data bytes per codeword for system pages.
pub const RS_SYSTEM_K: usize = 239;
/// Parity bytes per codeword for system pages.
pub const RS_SYSTEM_PARITY: usize = RS_N - RS_SYSTEM_K; // 16

/// First consecutive root exponent for DWG RS encoding.
///
/// For AC1021 pages, the on-disk parity matches when FCR equals `k`
/// (`block_size`):
/// - data pages: k=251
/// - system pages: k=239
#[inline]
fn rs_fcr_for_block_size(block_size: usize) -> usize {
    block_size % RS_N
}

// ---------------------------------------------------------------------------
// GF(2^8) arithmetic
// ---------------------------------------------------------------------------

/// Lookup tables for GF(2^8) arithmetic under a given primitive polynomial.
struct GfTables {
    /// `exp[i] = α^i` for i in 0..510.  Doubled so that `exp[a + b]` is valid
    /// for any two log values a, b in 0..254 without an explicit `% 255`.
    exp: [u8; 510],
    /// `log[v] = i` where `α^i = v`.  `log[0]` is unused (0 has no logarithm).
    log: [u8; 256],
}

impl GfTables {
    /// Build exp/log tables for the given primitive polynomial.
    ///
    /// `prim_poly` is the full 9-bit polynomial including the x⁸ term
    /// (e.g. 0x11D for x⁸+x⁴+x³+x²+1).
    fn new(prim_poly: u16) -> Self {
        let mut exp = [0u8; 510];
        let mut log = [0u8; 256];

        let mut x: u16 = 1; // α⁰ = 1
        for i in 0..255usize {
            exp[i] = x as u8;
            log[x as usize] = i as u8;
            x <<= 1; // multiply by α (= x in the polynomial ring)
            if x & 0x100 != 0 {
                x ^= prim_poly; // reduce modulo the primitive polynomial
            }
        }
        // Mirror the first 255 entries so that exp[a+b] works for a+b up to 508.
        for i in 255..510usize {
            exp[i] = exp[i - 255];
        }

        Self { exp, log }
    }

    /// Multiply two field elements.
    #[inline]
    fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        self.exp[self.log[a as usize] as usize + self.log[b as usize] as usize]
    }
}

// ---------------------------------------------------------------------------
// Generator polynomial
// ---------------------------------------------------------------------------

/// Compute the RS generator polynomial of degree `nsym`:
///
///   g(x) = ∏(x − α^i)  for i = fcr .. fcr + nsym − 1
///
/// Returns `nsym + 1` coefficients in descending-degree order:
///   gen[0] = 1 (coefficient of x^nsym), gen[nsym] = constant term.
fn rs_generator_poly(nsym: usize, fcr: usize, gf: &GfTables) -> Vec<u8> {
    // Start with g(x) = 1  →  [1]
    let mut gen = vec![0u8; nsym + 1];
    gen[0] = 1;

    for i in 0..nsym {
        // Multiply g(x) by (x − α^(fcr + i)).
        // In GF(2), subtraction = addition, so (x − α^k) = (x + α^k).
        let alpha_i = gf.exp[(fcr + i) % RS_N];
        // Multiply in-place, processing from high degree down.
        for j in (1..=i + 1).rev() {
            gen[j] = gen[j] ^ gf.mul(gen[j - 1], alpha_i);
        }
        // gen[0] stays 1 (monic).
    }

    gen
}

// ---------------------------------------------------------------------------
// Single-block RS encoding
// ---------------------------------------------------------------------------

/// Encode a single data block, appending `nsym` parity bytes.
///
/// `data` must have at most `255 - nsym` bytes.
/// Returns a 255-byte codeword: `[data (padded to k) | parity (nsym)]`.
fn rs_encode_block(data: &[u8], nsym: usize, gen: &[u8], gf: &GfTables) -> [u8; RS_N] {
    let k = RS_N - nsym;
    debug_assert!(
        data.len() <= k,
        "data block too large for RS({}, {})",
        RS_N,
        k
    );
    debug_assert_eq!(gen.len(), nsym + 1);

    // Working buffer: data in the first k positions, parity in the last nsym.
    let mut cw = [0u8; RS_N];
    cw[..data.len()].copy_from_slice(data);
    // Remaining bytes of data portion (if data.len() < k) stay zero — this is
    // equivalent to padding the message with zeros.

    // Systematic encoding via polynomial long division.
    // We compute  r(x) = m(x) · x^nsym  mod  g(x),
    // where m(x) has coefficients cw[0..k] (high-degree first).
    //
    // gen is stored in descending-degree order: gen[0]=1, gen[nsym]=constant.
    for i in 0..k {
        let feedback = cw[i] ^ cw[k]; // cw[k] is parity[0] (shift register head)
                                      // Shift the parity register left and fold in the generator.
        if feedback != 0 {
            for j in 0..nsym - 1 {
                cw[k + j] = cw[k + j + 1] ^ gf.mul(feedback, gen[j + 1]);
            }
            cw[k + nsym - 1] = gf.mul(feedback, gen[nsym]);
        } else {
            for j in 0..nsym - 1 {
                cw[k + j] = cw[k + j + 1];
            }
            cw[k + nsym - 1] = 0;
        }
    }

    // Restore original data bytes (the shift register loop modified them
    // only if we treated parity as part of the data, but here we kept them
    // separate so they are already correct).
    cw[..data.len()].copy_from_slice(data);

    cw
}

// ---------------------------------------------------------------------------
// Interleaved RS encode (public API)
// ---------------------------------------------------------------------------

/// Encode data with interleaved Reed-Solomon, producing output that
/// [`reed_solomon_decode`] can decode.
///
/// # Layout
///
/// * Input `data`: up to `factor × block_size` bytes of source data.
///   If shorter, the last sub-stream is zero-padded to `block_size`.
/// * Output `buffer`: exactly `factor × 255` bytes of RS-encoded,
///   interleaved codewords.
///
/// Each sub-stream of `block_size` data bytes is RS-encoded to produce a
/// 255-byte codeword.  The codewords are then interleaved at stride `factor`:
/// byte `i` of sub-stream `j` is written to `buffer[i * factor + j]`.
///
/// # Parameters
///
/// * `data`       — source data (at most `factor * block_size` bytes)
/// * `buffer`     — output buffer, must be exactly `factor * 255` bytes
/// * `factor`     — number of interleaved sub-streams
/// * `block_size` — RS data size k (239 for system pages, 251 for data pages)
/// * `prim_poly`  — GF(2^8) primitive polynomial (use [`RS_SYSTEM_PRIM_POLY`]
///                  or [`RS_DATA_PRIM_POLY`])
///
/// # Panics
///
/// Panics if `buffer.len() != factor * 255` or `data.len() > factor * block_size`.
pub fn reed_solomon_encode(
    data: &[u8],
    buffer: &mut [u8],
    factor: usize,
    block_size: usize,
    prim_poly: u16,
) {
    let nsym = RS_N - block_size;
    assert_eq!(
        buffer.len(),
        factor * RS_N,
        "buffer must be factor({}) × 255 = {} bytes, got {}",
        factor,
        factor * RS_N,
        buffer.len()
    );
    assert!(
        data.len() <= factor * block_size,
        "data length {} exceeds factor({}) × block_size({}) = {}",
        data.len(),
        factor,
        block_size,
        factor * block_size
    );

    let gf = GfTables::new(prim_poly);
    let fcr = rs_fcr_for_block_size(block_size);
    let gen = rs_generator_poly(nsym, fcr, &gf);

    // Zero the output buffer.
    buffer.fill(0);

    for s in 0..factor {
        // Extract (or zero-pad) the s-th sub-stream.
        let src_start = s * block_size;
        let src_end = (src_start + block_size).min(data.len());
        let chunk = if src_start < data.len() {
            &data[src_start..src_end]
        } else {
            &[] as &[u8]
        };

        // RS-encode the sub-stream → 255-byte codeword.
        let codeword = rs_encode_block(chunk, nsym, &gen, &gf);

        // Interleave: byte i of sub-stream s → buffer[i * factor + s].
        for (i, &b) in codeword.iter().enumerate() {
            buffer[i * factor + s] = b;
        }
    }
}

// ---------------------------------------------------------------------------
// Decode (existing — de-interleave only)
// ---------------------------------------------------------------------------

/// Decode Reed-Solomon interleaved data.
///
/// De-interleaves `encoded` into `buffer` by reading every `factor`-th byte
/// and copying `block_size` data bytes per stream (parity bytes are discarded).
///
/// This matches the reference `reedSolomonDecoding()` method.
///
/// # Panics
/// Panics if `buffer.len() < factor * block_size`.
pub fn reed_solomon_decode(encoded: &[u8], buffer: &mut [u8], factor: usize, block_size: usize) {
    let mut index = 0;
    let mut n = 0;
    let mut length = buffer.len();

    for _i in 0..factor {
        let mut cindex = n;
        if n < encoded.len() {
            let size = length.min(block_size);
            length -= size;
            let offset = index + size;
            while index < offset {
                buffer[index] = encoded[cindex];
                index += 1;
                cindex += factor;
            }
        }
        n += 1;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Encode data using compact sequential RS(255, block_size) format.
///
/// On-disk format for encoding=1 sections:
/// ```text
/// [original_data_bytes][parity_block_0][parity_block_1]...[parity_block_N-1]
/// ```
///
/// The last data block is padded with implicit zeros for RS parity computation,
/// but those zeros are NOT stored on disk. This produces a more compact output
/// than `reed_solomon_encode_sequential` which pads each block to `block_size`.
///
/// # Arguments
/// * `data`       — Input data of arbitrary length
/// * `block_size` — Data symbols per codeword (k), typically 251 for encoding=1
///
/// # Returns
/// Encoded byte vector of length `data.len() + ceil(data.len()/block_size) * (255 − block_size)`.
pub fn reed_solomon_encode_compact(data: &[u8], block_size: usize) -> Vec<u8> {
    let nsym = 255 - block_size;
    let gf = GfTables::new(RS_DATA_PRIM_POLY);
    let gen = rs_generator_poly(nsym, rs_fcr_for_block_size(block_size), &gf);
    let n_blocks = if data.is_empty() {
        0
    } else {
        (data.len() + block_size - 1) / block_size
    };

    let mut result = Vec::with_capacity(data.len() + n_blocks * nsym);

    // Copy original data as-is
    result.extend_from_slice(data);

    // Compute and append parity for each block
    for i in 0..n_blocks {
        let start = i * block_size;
        let end = std::cmp::min(start + block_size, data.len());
        let chunk = &data[start..end];

        // Pad to block_size for RS computation (implicit zeros for last block)
        let mut padded = vec![0u8; block_size];
        padded[..chunk.len()].copy_from_slice(chunk);

        let codeword = rs_encode_block(&padded, nsym, &gen, &gf);
        // Extract only the parity bytes (last nsym bytes of the 255-byte codeword)
        let k = RS_N - nsym;
        result.extend_from_slice(&codeword[k..]);
    }

    result
}

/// Decode compact sequential RS(255, block_size) format.
///
/// Reads the data portion from the compact format (parity bytes are discarded).
/// This is the inverse of `reed_solomon_encode_compact` — it simply extracts
/// the first `data_size` bytes from the encoded buffer.
///
/// # Arguments
/// * `encoded`   — Encoded data in compact sequential format
/// * `data_size` — Number of data bytes to extract
///
/// # Returns
/// Decoded data bytes (parity discarded).
pub fn reed_solomon_decode_compact(encoded: &[u8], data_size: usize) -> Vec<u8> {
    let mut data = vec![0u8; data_size];
    let copy_len = std::cmp::min(data_size, encoded.len());
    data[..copy_len].copy_from_slice(&encoded[..copy_len]);
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Decode tests (existing) ──────────────────────────────────────────

    #[test]
    fn test_reed_solomon_basic_deinterleave() {
        let encoded = [1, 4, 2, 5, 3, 6, 99, 99];
        let mut buffer = [0u8; 6];
        reed_solomon_decode(&encoded, &mut buffer, 2, 3);
        assert_eq!(buffer, [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn test_reed_solomon_factor3() {
        let encoded = [10, 20, 30, 11, 21, 31, 0, 0, 0];
        let mut buffer = [0u8; 6];
        reed_solomon_decode(&encoded, &mut buffer, 3, 2);
        assert_eq!(buffer, [10, 11, 20, 21, 30, 31]);
    }

    #[test]
    fn test_reed_solomon_real_world_params() {
        let encoded = vec![0xAA; 1024];
        let mut buffer = vec![0u8; 3 * 239];
        reed_solomon_decode(&encoded, &mut buffer, 3, 239);
        assert!(buffer.iter().all(|&b| b == 0xAA));
    }

    // ── GF(2^8) arithmetic tests ─────────────────────────────────────────

    #[test]
    fn test_gf_tables_0x11d() {
        let gf = GfTables::new(RS_DATA_PRIM_POLY);
        // α⁰ = 1
        assert_eq!(gf.exp[0], 1);
        // α¹ = 2
        assert_eq!(gf.exp[1], 2);
        // α⁸ should reduce: 0x100 ^ 0x11D = 0x1D
        assert_eq!(gf.exp[8], 0x1D);
        // α^255 should wrap back to α⁰ = 1 (primitive element generates the
        // full multiplicative group of order 255).
        assert_eq!(gf.exp[255], gf.exp[0]);
        // log(1) = 0
        assert_eq!(gf.log[1], 0);
        // log(2) = 1
        assert_eq!(gf.log[2], 1);
    }

    #[test]
    fn test_gf_tables_0x169() {
        let gf = GfTables::new(RS_SYSTEM_PRIM_POLY);
        assert_eq!(gf.exp[0], 1);
        assert_eq!(gf.exp[1], 2);
        // α⁸ = 0x100 ^ 0x169 = 0x69
        assert_eq!(gf.exp[8], 0x69);
        assert_eq!(gf.exp[255], gf.exp[0]);
    }

    #[test]
    fn test_gf_mul_identity() {
        let gf = GfTables::new(RS_DATA_PRIM_POLY);
        // a * 1 = a
        for a in 0..=255u8 {
            assert_eq!(gf.mul(a, 1), a);
        }
        // a * 0 = 0
        for a in 0..=255u8 {
            assert_eq!(gf.mul(a, 0), 0);
        }
    }

    #[test]
    fn test_gf_mul_commutative() {
        let gf = GfTables::new(RS_DATA_PRIM_POLY);
        for a in 1..=255u8 {
            for b in 1..=255u8 {
                assert_eq!(gf.mul(a, b), gf.mul(b, a), "a={}, b={}", a, b);
            }
        }
    }

    // ── Generator polynomial tests ───────────────────────────────────────

    #[test]
    fn test_generator_poly_degree() {
        let gf = GfTables::new(RS_DATA_PRIM_POLY);
        let gen = rs_generator_poly(RS_DATA_PARITY, RS_DATA_K, &gf); // nsym = 4
        assert_eq!(gen.len(), RS_DATA_PARITY + 1); // degree 4 → 5 coefficients
        assert_eq!(gen[0], 1); // monic
    }

    #[test]
    fn test_generator_poly_roots() {
        // Verify that α^fcr .. α^(fcr+nsym-1) are roots of g(x).
        let gf = GfTables::new(RS_DATA_PRIM_POLY);
        let nsym = RS_DATA_PARITY; // 4
        let fcr = RS_DATA_K;
        let gen = rs_generator_poly(nsym, fcr, &gf);

        for i in 0..nsym {
            let alpha_i = gf.exp[(fcr + i) % RS_N]; // root
                                                    // Evaluate g(alpha_i) using Horner's method.
            let mut val: u8 = 0;
            for &coeff in &gen {
                val = gf.mul(val, alpha_i) ^ coeff;
            }
            assert_eq!(val, 0, "α^{} should be a root of g(x)", fcr + i);
        }
    }

    #[test]
    fn test_generator_poly_roots_system() {
        let gf = GfTables::new(RS_SYSTEM_PRIM_POLY);
        let nsym = RS_SYSTEM_PARITY; // 16
        let fcr = RS_SYSTEM_K;
        let gen = rs_generator_poly(nsym, fcr, &gf);

        assert_eq!(gen.len(), nsym + 1);
        assert_eq!(gen[0], 1);

        for i in 0..nsym {
            let alpha_i = gf.exp[(fcr + i) % RS_N];
            let mut val: u8 = 0;
            for &coeff in &gen {
                val = gf.mul(val, alpha_i) ^ coeff;
            }
            assert_eq!(val, 0, "α^{} should be a root of g(x)", fcr + i);
        }
    }

    // ── Single-block encode tests ────────────────────────────────────────

    #[test]
    fn test_encode_block_codeword_is_valid() {
        // A valid codeword c(x) must satisfy c(α^i) = 0 for all roots of g(x).
        let gf = GfTables::new(RS_DATA_PRIM_POLY);
        let nsym = RS_DATA_PARITY;
        let fcr = RS_DATA_K;
        let gen = rs_generator_poly(nsym, fcr, &gf);

        let data: Vec<u8> = (1..=RS_DATA_K as u8).collect();
        let cw = rs_encode_block(&data, nsym, &gen, &gf);

        // Check each root.
        for r in 0..nsym {
            let alpha_r = gf.exp[(fcr + r) % RS_N];
            let mut syndrome: u8 = 0;
            for &byte in cw.iter() {
                syndrome = gf.mul(syndrome, alpha_r) ^ byte;
            }
            assert_eq!(syndrome, 0, "syndrome at root α^{} must be 0", fcr + r);
        }
    }

    #[test]
    fn test_encode_block_data_preserved() {
        // Systematic encoding: data bytes must appear unchanged at the start.
        let gf = GfTables::new(RS_SYSTEM_PRIM_POLY);
        let nsym = RS_SYSTEM_PARITY;
        let gen = rs_generator_poly(nsym, RS_SYSTEM_K, &gf);

        let data = vec![0x42u8; RS_SYSTEM_K];
        let cw = rs_encode_block(&data, nsym, &gen, &gf);
        assert_eq!(&cw[..RS_SYSTEM_K], &data[..]);
    }

    #[test]
    fn test_encode_block_zeros() {
        // All-zero data should produce all-zero codeword.
        let gf = GfTables::new(RS_DATA_PRIM_POLY);
        let nsym = RS_DATA_PARITY;
        let gen = rs_generator_poly(nsym, RS_DATA_K, &gf);

        let data = vec![0u8; RS_DATA_K];
        let cw = rs_encode_block(&data, nsym, &gen, &gf);
        assert!(cw.iter().all(|&b| b == 0));
    }

    // ── Encode → Decode roundtrip tests ──────────────────────────────────

    #[test]
    fn test_roundtrip_factor1_data_pages() {
        // Single RS block, RS(255,251), factor=1.
        let data = vec![0xAB; RS_DATA_K];
        let mut encoded = vec![0u8; RS_N]; // 255
        reed_solomon_encode(&data, &mut encoded, 1, RS_DATA_K, RS_DATA_PRIM_POLY);

        let mut decoded = vec![0u8; RS_DATA_K];
        reed_solomon_decode(&encoded, &mut decoded, 1, RS_DATA_K);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_roundtrip_factor3_system_pages() {
        // File header configuration: RS(255,239), factor=3.
        let data: Vec<u8> = (0..3 * RS_SYSTEM_K).map(|i| (i & 0xFF) as u8).collect();
        let mut encoded = vec![0u8; 3 * RS_N]; // 765
        reed_solomon_encode(&data, &mut encoded, 3, RS_SYSTEM_K, RS_SYSTEM_PRIM_POLY);

        let mut decoded = vec![0u8; 3 * RS_SYSTEM_K];
        reed_solomon_decode(&encoded, &mut decoded, 3, RS_SYSTEM_K);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_roundtrip_factor2_data_pages() {
        // Two RS blocks with data pages config.
        let data: Vec<u8> = (0..2 * RS_DATA_K)
            .map(|i| ((i * 7 + 13) & 0xFF) as u8)
            .collect();
        let mut encoded = vec![0u8; 2 * RS_N];
        reed_solomon_encode(&data, &mut encoded, 2, RS_DATA_K, RS_DATA_PRIM_POLY);

        let mut decoded = vec![0u8; 2 * RS_DATA_K];
        reed_solomon_decode(&encoded, &mut decoded, 2, RS_DATA_K);
        assert_eq!(decoded, data);
    }

    #[test]
    fn test_roundtrip_short_last_block() {
        // Data shorter than factor * block_size — last sub-stream is zero-padded.
        // factor=3, block_size=239, but only 500 bytes of data.
        let data: Vec<u8> = (0..500).map(|i| (i & 0xFF) as u8).collect();
        let factor = (data.len() + RS_SYSTEM_K - 1) / RS_SYSTEM_K; // 3
        assert_eq!(factor, 3);

        let mut encoded = vec![0u8; factor * RS_N];
        reed_solomon_encode(
            &data,
            &mut encoded,
            factor,
            RS_SYSTEM_K,
            RS_SYSTEM_PRIM_POLY,
        );

        // The decoder outputs factor * block_size bytes, but only the first
        // data.len() bytes are meaningful.
        let mut decoded = vec![0u8; factor * RS_SYSTEM_K];
        reed_solomon_decode(&encoded, &mut decoded, factor, RS_SYSTEM_K);

        assert_eq!(&decoded[..data.len()], &data[..]);
        // Remaining bytes should be zero (padding).
        assert!(decoded[data.len()..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_roundtrip_file_header_realistic() {
        // Simulate file header encoding/decoding parameters.
        // 717 bytes of data → RS(255,239) × 3 → 765 bytes encoded.
        // Encoded goes into a 0x400-byte page (only first 765 bytes used).
        let data: Vec<u8> = (0u16..717).map(|i| (i & 0xFF) as u8).collect();
        let mut page = vec![0u8; 0x400];

        // Encode into the first 765 bytes of the page.
        let encoded_len = 3 * RS_N; // 765
        reed_solomon_encode(
            &data,
            &mut page[..encoded_len],
            3,
            RS_SYSTEM_K,
            RS_SYSTEM_PRIM_POLY,
        );

        // Decode from the full 0x400-byte page (as the reader does).
        let mut decoded = vec![0u8; 3 * RS_SYSTEM_K]; // 717
        reed_solomon_decode(&page, &mut decoded, 3, RS_SYSTEM_K);
        assert_eq!(decoded, data);
    }

    // ── Interleaving correctness ─────────────────────────────────────────

    #[test]
    fn test_interleave_data_byte_positions() {
        // With factor=2, data byte i of stream s should be at encoded[i*2 + s].
        // Verify by checking that the data bytes (first k of each 255-byte
        // codeword) land at the expected positions.
        let stream0: Vec<u8> = (1..=RS_DATA_K as u8).collect();
        let stream1: Vec<u8> = (0..RS_DATA_K).map(|i| (200 + i) as u8).collect();

        let mut data = Vec::new();
        data.extend_from_slice(&stream0);
        data.extend_from_slice(&stream1);

        let mut encoded = vec![0u8; 2 * RS_N];
        reed_solomon_encode(&data, &mut encoded, 2, RS_DATA_K, RS_DATA_PRIM_POLY);

        // Data byte 0 of stream 0 should be at encoded[0].
        assert_eq!(encoded[0], stream0[0]);
        // Data byte 0 of stream 1 should be at encoded[1].
        assert_eq!(encoded[1], stream1[0]);
        // Data byte 1 of stream 0 should be at encoded[2].
        assert_eq!(encoded[2], stream0[1]);
        // Data byte 1 of stream 1 should be at encoded[3].
        assert_eq!(encoded[3], stream1[1]);
    }

    // ── Syndrome verification for system pages polynomial ────────────────

    #[test]
    fn test_encode_block_system_poly_syndromes() {
        let gf = GfTables::new(RS_SYSTEM_PRIM_POLY);
        let nsym = RS_SYSTEM_PARITY;
        let fcr = RS_SYSTEM_K;
        let gen = rs_generator_poly(nsym, fcr, &gf);

        // Random-ish data.
        let data: Vec<u8> = (0..RS_SYSTEM_K)
            .map(|i| ((i * 37 + 5) & 0xFF) as u8)
            .collect();
        let cw = rs_encode_block(&data, nsym, &gen, &gf);

        for r in 0..nsym {
            let alpha_r = gf.exp[(fcr + r) % RS_N];
            let mut syndrome: u8 = 0;
            for &byte in cw.iter() {
                syndrome = gf.mul(syndrome, alpha_r) ^ byte;
            }
            assert_eq!(syndrome, 0, "syndrome at root α^{} must be 0", fcr + r);
        }
    }

    // ── Cross-validation against LibreDWG's reference implementation ─────

    #[test]
    fn test_gf_exp_table_matches_libredwg_f256_power() {
        // LibreDWG's f256_power[] table (first 32 entries) for polynomial 0x169.
        // Source: LibreDWG/src/reedsolomon.c
        let libredwg_power: [u8; 32] = [
            0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x69, 0xd2, 0xcd, 0xf3, 0x8f, 0x77,
            0xee, 0xb5, 0x03, 0x06, 0x0c, 0x18, 0x30, 0x60, 0xc0, 0xe9, 0xbb, 0x1f, 0x3e, 0x7c,
            0xf8, 0x99, 0x5b, 0xb6,
        ];

        let gf = GfTables::new(RS_SYSTEM_PRIM_POLY);
        for i in 0..32 {
            assert_eq!(
                gf.exp[i], libredwg_power[i],
                "exp[{}]: ours={:#04x} vs LibreDWG={:#04x}",
                i, gf.exp[i], libredwg_power[i]
            );
        }
    }

    #[test]
    fn test_generator_poly_matches_libredwg_rsgen() {
        // LibreDWG's rsgen[] for RS(255,239) with polynomial 0x169, FCR=1.
        // Source: LibreDWG/src/reedsolomon.c
        // Their coefficients are in ascending degree order (rsgen[0]=constant term).
        let libredwg_rsgen: [u8; 17] = [
            0x6a, 0xe3, 0x63, 0x1f, 0xa1, 0x24, 0x9e, 0x44, 0x13, 0x1e, 0x2f, 0xfc, 0xfd, 0xce,
            0xa9, 0xdb, 0x01,
        ];

        let gf = GfTables::new(RS_SYSTEM_PRIM_POLY);
        let gen = rs_generator_poly(RS_SYSTEM_PARITY, 1, &gf);
        // Our gen is in descending degree order: gen[0]=1 (x^16), gen[16]=constant.
        // LibreDWG's rsgen is ascending: rsgen[0]=constant, rsgen[16]=1 (x^16).
        // So gen[i] should equal libredwg_rsgen[16 - i].
        assert_eq!(gen.len(), 17);
        for i in 0..17 {
            assert_eq!(
                gen[i],
                libredwg_rsgen[16 - i],
                "gen[{}] (x^{} coeff): ours={:#04x} vs LibreDWG rsgen[{}]={:#04x}",
                i,
                16 - i,
                gen[i],
                16 - i,
                libredwg_rsgen[16 - i]
            );
        }
    }

    #[test]
    fn test_encode_parity_matches_libredwg_convention() {
        // Verify that our encoder produces the same parity bytes as LibreDWG
        // would for a simple test vector, by checking the codeword is valid
        // under the same syndrome evaluation used by LibreDWG's rs_decode_block:
        //   synbuf[j] = evaluate(blk, 255, f256_power[j + 1])  for j=0..15
        // This evaluates the codeword polynomial at α^1, α^2, ..., α^16 (FCR=1).
        let gf = GfTables::new(RS_SYSTEM_PRIM_POLY);
        let nsym = RS_SYSTEM_PARITY;
        let gen = rs_generator_poly(nsym, 1, &gf);

        // Test vector: first 239 bytes = 0x01, 0x02, ..., 0xEF
        let data: Vec<u8> = (1..=RS_SYSTEM_K as u8).collect();
        let cw = rs_encode_block(&data, nsym, &gen, &gf);

        // Verify syndromes at α^1 through α^16 (matching LibreDWG's FCR=1 convention).
        for j in 0..16usize {
            let root = gf.exp[j + 1]; // α^(j+1)
                                      // Evaluate codeword polynomial at root (Horner's method, MSB-first).
            let mut val: u8 = 0;
            for &byte in cw.iter() {
                val = gf.mul(val, root) ^ byte;
            }
            assert_eq!(
                val,
                0,
                "LibreDWG-compatible syndrome at α^{} must be 0",
                j + 1
            );
        }
    }
}
