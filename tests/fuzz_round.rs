//! FUZZ/property round: randomized inputs checking invariants of the
//! web-export pipeline targets, with a hand-rolled seeded xorshift PRNG
//! (no new dependencies). Method: generate hostile random inputs, run the
//! real code (or a verbatim replica when the item is `pub(crate)`/private,
//! the same precedent as `tests/web_recovered_name_redteam.rs`), and check
//! invariants with an INDEPENDENT oracle (own ZIP parser, own CRC-32,
//! exact-integer arithmetic, spec re-derivations).
//!
//! Reachability audit (branch web-pdf-export, HEAD d5b3f1d3):
//! - `OpenCADStudio::io::zip::{build_zip, ZipEntry}` — `pub mod zip` under
//!   `pub mod io`: REACHABLE, fuzzed directly.
//! - `scene::model::image_model::{tier_for, downsample_to_cap}` — private
//!   `fn`s (module is pub, functions are not): UNREACHABLE from integration
//!   tests, SKIPPED per round instructions (see the note test at the bottom).
//! - `app::record_api::json_values_match` — private fn in a private module:
//!   replicated VERBATIM (marked below) and property-tested.
//! - `io::web_link::parse_link` — `pub(crate) mod web_link`: replicated
//!   VERBATIM and property-tested.
//! - `io::web_autosave::{recovered_name, cap_evictions, newer_than_saved}` —
//!   `pub(crate) mod web_autosave`: replicated VERBATIM and property-tested.
//!
//! Seeds: four fixed seeds by default; set OCS_FUZZ_SEED (hex) to run a
//! single seed and OCS_FUZZ_ITERS to override iteration counts.

#![cfg(not(target_arch = "wasm32"))]
#![allow(dead_code)]

use std::cmp::Ordering;
use std::io::Read;
use std::panic::{catch_unwind, AssertUnwindSafe};

use flate2::read::DeflateDecoder;
use serde_json::{Number, Value};
use OpenCADStudio::io::zip::{build_zip, ZipEntry};

// ── Hand-rolled PRNG (xorshift64*), no new dependencies ─────────────────────

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        // xorshift64* requires a non-zero state.
        Rng((seed | 1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `0..n` (modulo bias is fine for fuzzing).
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }

    /// Uniform in `lo..=hi` (only for small ranges).
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(hi >= lo);
        lo + self.below(hi - lo + 1)
    }

    fn bool(&mut self) -> bool {
        self.next() & 1 == 0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }

    /// Any f64 by bit pattern (NaN / ±inf included).
    fn any_f64(&mut self) -> f64 {
        f64::from_bits(self.next())
    }
}

/// Default multi-seed set; OCS_FUZZ_SEED (hex u64) narrows to one seed.
fn seeds() -> Vec<u64> {
    match std::env::var("OCS_FUZZ_SEED") {
        Ok(raw) => vec![u64::from_str_radix(raw.trim_start_matches("0x"), 16)
            .unwrap_or_else(|_| panic!("OCS_FUZZ_SEED must be hex u64: {raw:?}"))],
        Err(_) => vec![
            0x243F_6A88_85A3_08D3,
            0x1319_8A2E_0370_7344,
            0xA409_3822_299F_31D0,
            0x082E_FA98_EC4E_6C89,
        ],
    }
}

/// Iteration count override; defaults to `default`.
fn iters(default: usize) -> usize {
    std::env::var("OCS_FUZZ_ITERS")
        .ok()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(default)
}

fn display_truncated(name: &str) -> String {
    let prefix: String = name.chars().take(48).collect();
    if name.chars().count() > 48 {
        format!("{prefix:?}…[{} chars]", name.chars().count())
    } else {
        format!("{prefix:?}")
    }
}

// ── Independent ZIP machinery (spec re-derivation, not their test code) ─────

const EOCD_SIG: [u8; 4] = [0x50, 0x4B, 0x05, 0x06];
const CENTRAL_SIG: [u8; 4] = [0x50, 0x4B, 0x01, 0x02];
const LOCAL_SIG: [u8; 4] = [0x50, 0x4B, 0x03, 0x04];
const FLAG_UTF8: u16 = 0x0800;

/// Table-driven CRC-32 (IEEE 802.3, reflected poly 0xEDB88320), written from
/// the spec so the archive's flate2 CRC is checked by an independent oracle.
fn crc32_ieee(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc = table[((crc ^ byte as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

fn u16le(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|w| u16::from_le_bytes([w[0], w[1]]))
        .ok_or_else(|| format!("u16 read out of bounds at {at}"))
}

fn u32le(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|w| u32::from_le_bytes([w[0], w[1], w[2], w[3]]))
        .ok_or_else(|| format!("u32 read out of bounds at {at}"))
}

/// Scan backward (max comment window) for the end-of-central-directory
/// signature, like an extractor does.
fn find_eocd(zip: &[u8]) -> Option<usize> {
    if zip.len() < 22 {
        return None;
    }
    let last = zip.len() - 22;
    let low = zip.len().saturating_sub(22 + 65_535);
    (low..=last).rev().find(|&pos| zip[pos..pos + 4] == EOCD_SIG)
}

/// Independent full-archive validation of `build_zip` output against the
/// entries it was asked to package. Returns every violation found.
fn validate_archive(zip: &[u8], entries: &[ZipEntry]) -> Result<(), Vec<String>> {
    let mut errs = Vec::new();

    // End record: found, last, comment-free, single-disk, counts sane.
    let Some(eocd) = find_eocd(zip) else {
        errs.push("EOCD signature not found scanning backward".into());
        return Err(errs);
    };
    if eocd + 22 != zip.len() {
        errs.push(format!("EOCD at {eocd} but archive ends at {}", zip.len()));
    }
    for (field, offset) in [("disk", 4usize), ("cd start disk", 6)] {
        if let Ok(v) = u16le(zip, eocd + offset) {
            if v != 0 {
                errs.push(format!("EOCD {field} = {v}, expected 0"));
            }
        }
    }
    let (on_disk, total) = (
        u16le(zip, eocd + 8).unwrap_or(u16::MAX),
        u16le(zip, eocd + 10).unwrap_or(u16::MAX),
    );
    if on_disk != total {
        errs.push(format!("EOCD counts differ: {on_disk} on disk vs {total} total"));
    }
    if total as usize != entries.len() {
        errs.push(format!(
            "EOCD entry count {total} != {} requested entries",
            entries.len()
        ));
    }
    if let Ok(comment) = u16le(zip, eocd + 20) {
        if comment != 0 {
            errs.push(format!("EOCD comment length {comment}, expected 0"));
        }
    }
    let cd_size = u32le(zip, eocd + 12).unwrap_or(u32::MAX);
    let cd_offset = u32le(zip, eocd + 16).unwrap_or(u32::MAX);

    // Central directory walk.
    struct Central {
        name: Vec<u8>,
        method: u16,
        crc: u32,
        csize: u32,
        usize_: u32,
        local_offset: u32,
    }
    let mut centrals = Vec::with_capacity(entries.len());
    let mut cursor = cd_offset as usize;
    if cd_offset as usize > zip.len() {
        errs.push(format!("central directory offset {cd_offset} beyond archive"));
        return Err(errs);
    }
    for index in 0..total as usize {
        if zip.get(cursor..cursor + 4) != Some(&CENTRAL_SIG[..]) {
            errs.push(format!(
                "central[{index}] at {cursor}: signature is not PK\\x01\\x02"
            ));
            return Err(errs);
        }
        let made = u16le(zip, cursor + 4).unwrap_or(u16::MAX);
        let needed = u16le(zip, cursor + 6).unwrap_or(u16::MAX);
        let flags = u16le(zip, cursor + 8).unwrap_or(u16::MAX);
        let method = u16le(zip, cursor + 10).unwrap_or(u16::MAX);
        let crc = u32le(zip, cursor + 16).unwrap_or(u32::MAX);
        let csize = u32le(zip, cursor + 20).unwrap_or(u32::MAX);
        let usize_ = u32le(zip, cursor + 24).unwrap_or(u32::MAX);
        let name_len = u16le(zip, cursor + 28).unwrap_or(u16::MAX) as usize;
        let extra_len = u16le(zip, cursor + 30).unwrap_or(u16::MAX);
        let comment_len = u16le(zip, cursor + 32).unwrap_or(u16::MAX);
        let disk_start = u16le(zip, cursor + 34).unwrap_or(u16::MAX);
        let local_offset = u32le(zip, cursor + 42).unwrap_or(u32::MAX);
        if made != 20 || needed != 20 {
            errs.push(format!("central[{index}] version fields {made}/{needed}, expected 20/20"));
        }
        if flags != FLAG_UTF8 {
            errs.push(format!("central[{index}] flags {flags:#x}, expected {FLAG_UTF8:#x}"));
        }
        if method != 0 && method != 8 {
            errs.push(format!("central[{index}] method {method}, expected 0 or 8"));
        }
        if extra_len != 0 || comment_len != 0 || disk_start != 0 {
            errs.push(format!(
                "central[{index}] extra/comment/disk {extra_len}/{comment_len}/{disk_start}, expected 0/0/0"
            ));
        }
        let name_at = cursor + 46;
        let Some(name) = zip.get(name_at..name_at + name_len) else {
            errs.push(format!("central[{index}] name runs past the archive"));
            return Err(errs);
        };
        match entries.get(index) {
            Some(entry) if entry.name.as_bytes() == name => {}
            Some(entry) => errs.push(format!(
                "central[{index}] name {:?} != requested {:?}",
                String::from_utf8_lossy(name),
                display_truncated(&entry.name)
            )),
            None => errs.push(format!("central[{index}] has no matching requested entry")),
        }
        centrals.push(Central {
            name: name.to_vec(),
            method,
            crc,
            csize,
            usize_,
            local_offset,
        });
        cursor += 46 + name_len;
    }
    if cursor != cd_offset as usize + cd_size as usize {
        errs.push(format!(
            "central walk consumed {cursor} bytes, EOCD says size {cd_size} at offset {cd_offset}"
        ));
    }
    if cursor != eocd {
        errs.push(format!(
            "central directory ends at {cursor}, EOCD sits at {eocd}"
        ));
    }

    // Local headers, payloads, sizes, CRC, layout — the extractor round trip.
    let mut expected_offset = 0usize;
    for (index, central) in centrals.iter().enumerate() {
        let at = central.local_offset as usize;
        if at != expected_offset {
            errs.push(format!(
                "local[{index}] offset {at}, expected back-to-back layout offset {expected_offset}"
            ));
        }
        if zip.get(at..at + 4) != Some(&LOCAL_SIG[..]) {
            errs.push(format!("local[{index}] at {at}: signature is not PK\\x03\\x04"));
            expected_offset = at.saturating_add(1);
            continue;
        }
        let needed = u16le(zip, at + 4).unwrap_or(u16::MAX);
        let flags = u16le(zip, at + 6).unwrap_or(u16::MAX);
        let method = u16le(zip, at + 8).unwrap_or(u16::MAX);
        let crc = u32le(zip, at + 14).unwrap_or(u32::MAX);
        let csize = u32le(zip, at + 18).unwrap_or(u32::MAX);
        let usize_ = u32le(zip, at + 22).unwrap_or(u32::MAX);
        let name_len = u16le(zip, at + 26).unwrap_or(u16::MAX) as usize;
        let extra_len = u16le(zip, at + 28).unwrap_or(u16::MAX);
        if needed != 20 || flags != FLAG_UTF8 || extra_len != 0 {
            errs.push(format!(
                "local[{index}] version/flags/extra {needed}/{flags:#x}/{extra_len}"
            ));
        }
        if method != central.method || crc != central.crc || csize != central.csize
            || usize_ != central.usize_
        {
            errs.push(format!(
                "local[{index}] disagrees with central: method {method}/{} crc {crc:#x}/{:#x} sizes {csize}/{}, {usize_}/{}",
                central.method, central.crc, central.csize, central.usize_
            ));
        }
        let name_at = at + 30;
        if zip.get(name_at..name_at + name_len) != Some(&central.name[..]) {
            errs.push(format!("local[{index}] name disagrees with central name"));
        }
        let data_start = name_at + name_len;
        let Some(payload) = zip.get(data_start..data_start + csize as usize) else {
            errs.push(format!("local[{index}] payload runs past the archive"));
            expected_offset = data_start;
            continue;
        };
        let Some(entry) = entries.get(index) else {
            expected_offset = data_start + csize as usize;
            continue;
        };
        if usize_ as usize != entry.data.len() {
            errs.push(format!(
                "local[{index}] uncompressed size {usize_} != {} actual bytes",
                entry.data.len()
            ));
        }
        if crc32_ieee(&entry.data) != crc {
            errs.push(format!(
                "local[{index}] CRC {crc:#x} != independently computed {:#x}",
                crc32_ieee(&entry.data)
            ));
        }
        match method {
            0 => {
                if payload != entry.data.as_slice() {
                    errs.push(format!("local[{index}] STORE payload differs from input bytes"));
                }
                if csize as usize != entry.data.len() {
                    errs.push(format!(
                        "local[{index}] STORE compressed size {csize} != stored length {}",
                        entry.data.len()
                    ));
                }
            }
            8 => {
                let mut inflated = Vec::new();
                match DeflateDecoder::new(payload).read_to_end(&mut inflated) {
                    Ok(_) => {
                        if inflated != entry.data {
                            errs.push(format!(
                                "local[{index}] DEFLATE round trip differs ({} bytes back vs {} in)",
                                inflated.len(),
                                entry.data.len()
                            ));
                        }
                    }
                    Err(error) => errs.push(format!(
                        "local[{index}] DEFLATE payload does not inflate: {error}"
                    )),
                }
                if (csize as usize) >= entry.data.len().max(1) {
                    errs.push(format!(
                        "local[{index}] DEFLATE chosen but compressed size {csize} >= uncompressed {}",
                        entry.data.len()
                    ));
                }
            }
            other => errs.push(format!("local[{index}] unexpected method {other}")),
        }
        expected_offset = data_start + csize as usize;
    }
    if !centrals.is_empty() && expected_offset != cd_offset as usize {
        errs.push(format!(
            "last payload ends at {expected_offset} but central directory starts at {cd_offset}"
        ));
    }

    if errs.is_empty() {
        Ok(())
    } else {
        Err(errs)
    }
}

/// The documented flat-name rule (src/io/zip.rs `validate_name`).
fn is_valid_zip_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0'])
        && name.len() <= u16::MAX as usize
}

// ── Hostile input generators ─────────────────────────────────────────────────

const HOSTILE_ASCII: &[char] = &[
    '\0', '/', '\\', '.', ':', '*', '?', '"', '<', '>', '|', ' ', '%', '+', '#', '&', ';', '=',
    '-', '_', 'a', 'Z', '0',
];
const UNICODE_POOL: &[char] = &[
    'π', 'λ', 'а', '文', '図', '子', '😀', 'é', 'ñ', 'א', 'ا', '〉', 'ำ', 'é', 'ø', '\u{0301}',
    '\u{200B}', '\u{00AD}', '\u{200F}', '\u{202E}',
];

fn random_zip_name(rng: &mut Rng) -> String {
    match rng.below(20) {
        0..=3 => (0..rng.range(1, 12))
            .map(|_| (b'a' + rng.below(26) as u8) as char)
            .collect(),
        4..=6 => (0..rng.range(1, 10))
            .map(|_| *rng.pick(HOSTILE_ASCII))
            .collect(),
        7..=8 => (0..rng.range(1, 8)).map(|_| *rng.pick(UNICODE_POOL)).collect(),
        9 => {
            let mut s = String::new();
            for _ in 0..rng.range(1, 15) {
                let pool = if rng.bool() { HOSTILE_ASCII } else { UNICODE_POOL };
                s.push(*rng.pick(pool));
            }
            s
        }
        10..=12 => (*rng.pick(&[
            "", ".", "..", "...", "a.", ".a", "a..b", "a/b", "a\\b", "/", "\\", "a/b\\c", ".dwg",
            ".dxf", "x.dwg", "plan.dwg", "site.png", "con", "NUL", "notes.txt", "plan.sv$",
        ]))
        .to_string(),
        13 => {
            // Byte-length boundary names; multi-byte chars decouple bytes/chars.
            match rng.below(4) {
                0 => "x".repeat(rng.range(65_533, 65_538) as usize),
                1 => "é".repeat(rng.range(32_766, 32_770) as usize),
                2 => "文".repeat(rng.range(21_844, 21_847) as usize),
                _ => {
                    let mut s = "x".repeat(65_530);
                    s.push(*rng.pick(UNICODE_POOL));
                    s
                }
            }
        }
        14..=15 => (0..rng.range(100, 1000))
            .map(|_| *rng.pick(HOSTILE_ASCII))
            .collect(),
        _ => {
            let mut s: String = (0..rng.range(1, 10))
                .map(|_| (b'a' + rng.below(26) as u8) as char)
                .collect();
            for _ in 0..rng.range(1, 4) {
                let at = rng.below(s.chars().count() as u64 + 1) as usize;
                let sep = *rng.pick(&['.', '/', '\\', '\0']);
                s.insert(s.char_indices().nth(at).map(|(i, _)| i).unwrap_or(s.len()), sep);
            }
            s
        }
    }
}

fn random_data(rng: &mut Rng) -> Vec<u8> {
    let len = match rng.below(100) {
        0..=9 => rng.range(0, 16),
        10..=49 => rng.range(16, 1024),
        50..=89 => rng.range(1024, 8192),
        90..=98 => rng.range(8192, 32_768),
        _ => rng.range(32_768, 102_400),
    } as usize;
    match rng.below(6) {
        0 => (0..len).map(|_| rng.next() as u8).collect(),
        1 => vec![rng.next() as u8; len],
        2 => {
            let pattern: Vec<u8> = (0..rng.range(1, 64)).map(|_| rng.next() as u8).collect();
            (0..len).map(|i| pattern[i % pattern.len()]).collect()
        }
        3 => (0..len)
            .map(|_| if rng.below(20) == 0 { rng.next() as u8 } else { 0 })
            .collect(),
        4 => (0..len)
            .map(|_| if rng.bool() { b' ' } else { b'a' + rng.below(26) as u8 })
            .collect(),
        _ => {
            let mut out = Vec::with_capacity(len);
            while out.len() < len {
                let byte = rng.next() as u8;
                let run = rng.range(1, 40) as usize;
                out.extend(std::iter::repeat(byte).take(run.min(len - out.len())));
            }
            out.truncate(len);
            out
        }
    }
}

fn entry_count(rng: &mut Rng) -> u64 {
    match rng.below(10) {
        0..=1 => rng.range(0, 2),
        2..=6 => rng.range(2, 9),
        _ => rng.range(9, 51),
    }
}

// ── Target 1: io::zip build_zip (real code, REACHABLE) ───────────────────────

#[test]
fn fuzz_zip_random_entries_round_trip_and_never_panic() {
    let iterations = iters(10_000);
    let mut valid_builds = 0usize;
    let mut rejected_sets = 0usize;
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let entries: Vec<ZipEntry> = (0..entry_count(&mut rng))
                .map(|_| ZipEntry::new(random_zip_name(&mut rng), random_data(&mut rng)))
                .collect();
            let all_valid = entries.iter().all(|entry| is_valid_zip_name(&entry.name));
            let names_summary = || {
                entries
                    .iter()
                    .map(|entry| format!("{} ({}B)", display_truncated(&entry.name), entry.data.len()))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let built = catch_unwind(AssertUnwindSafe(|| build_zip(&entries)))
                .unwrap_or_else(|_| panic!("build_zip PANICKED seed={seed} iter={i} entries: {}", names_summary()));
            if all_valid {
                let zip = built.unwrap_or_else(|error| {
                    panic!("valid entry set must build seed={seed} iter={i} err={error} entries: {}", names_summary())
                });
                valid_builds += 1;
                if let Err(violations) = validate_archive(&zip, &entries) {
                    panic!(
                        "seed={seed} iter={i} archive invariant violations: [{}] entries: {}",
                        violations.join("; "),
                        names_summary()
                    );
                }
                if i % 97 == 0 {
                    let rebuilt = build_zip(&entries).expect("rebuild of a valid set");
                    assert_eq!(rebuilt, zip, "byte-determinism seed={seed} iter={i}");
                }
            } else {
                rejected_sets += 1;
                let error = built.expect_err("any invalid name must make build_zip return Err");
                assert!(
                    error.contains("invalid ZIP entry name"),
                    "seed={seed} iter={i} unexpected error {error:?} entries: {}",
                    names_summary()
                );
            }
        }
    }
    eprintln!(
        "zip fuzz done: {valid_builds} valid archives fully validated, {rejected_sets} invalid-name sets rejected"
    );
}

#[test]
fn zip_boundary_conditions_deterministic() {
    // My CRC oracle matches the standard reference check value.
    assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);

    // Entry-count sentinel: u16::MAX itself is the ZIP64 sentinel.
    let entries: Vec<ZipEntry> = (0..65_534)
        .map(|i| ZipEntry::new(format!("f{i}"), Vec::new()))
        .collect();
    let zip = build_zip(&entries).expect("65534 (u16::MAX-1) entries build");
    let eocd = zip.len() - 22;
    assert_eq!(u16le(&zip, eocd + 8).unwrap(), 65_534);
    assert_eq!(u16le(&zip, eocd + 10).unwrap(), 65_534);
    validate_archive(&zip, &entries).expect("65534-entry archive validates");
    for count in [65_535usize, 65_536] {
        let entries: Vec<ZipEntry> = (0..count)
            .map(|i| ZipEntry::new(format!("f{i}"), Vec::new()))
            .collect();
        let error = build_zip(&entries).expect_err("count at/over the sentinel must Err");
        assert!(error.contains("too many entries"), "{error}");
    }

    // Name byte-length boundary (validate_name measures bytes).
    for (name, ok) in [
        ("x".repeat(65_535), true),
        ("x".repeat(65_536), false),
        ("é".repeat(32_767), true), // 65534 bytes
        ("é".repeat(32_768), false), // 65536 bytes
        ("文".repeat(21_845), true), // 65535 bytes
        ("文".repeat(21_846), false), // 65538 bytes
    ] {
        let result = build_zip(&[ZipEntry::new(name.clone(), b"data".to_vec())]);
        assert_eq!(
            result.is_ok(),
            ok,
            "name of {} bytes ({} chars) built={} want {ok}",
            name.len(),
            name.chars().count(),
            result.is_ok()
        );
        if let Ok(zip) = result {
            validate_archive(&zip, &[ZipEntry::new(name, b"data".to_vec())])
                .expect("boundary-length valid archive validates");
        }
    }
}

// ── Target 2: image_model tier_for/downsample_to_cap — SKIPPED (private) ────

#[test]
fn note_image_model_tier_fns_are_private_and_skipped() {
    // tier_for / tier_for_native / downsample_to_cap in
    // src/scene/model/image_model.rs are plain private `fn`s (the module is
    // `pub`, the functions are not, and there is no #[cfg(test)] pub seam or
    // pub re-export), so an integration test under tests/ cannot reach them.
    // Per this round's instructions they are SKIPPED, not replicated.
}

// ── Target 3: record_api::json_values_match — VERBATIM replica ───────────────

// Verbatim copy of src/app/record_api.rs:1112-1152 (`json_values_match`),
// private fn in a private module, invisible to integration tests.
/// A whole f64 equals an i64 under the same round-trip rule as the u64
/// side (see [`whole_float_equals_u64`]); the bounds are strict because
/// `i64::MIN as f64` and `i64::MAX as f64` land exactly ON the boundary
/// powers of two, where the cast back saturates.
fn whole_float_equals_i64(float: f64, int: i64) -> bool {
    if float.fract() != 0.0 || float < -9_223_372_036_854_775_808.0 || float >= 9_223_372_036_854_775_808.0
    {
        return false;
    }
    float as i64 == int
}

fn json_values_match(stored: &Value, expected: &Value) -> bool {
    match (stored, expected) {
        (Value::Number(stored), Value::Number(expected)) => {
            // Integers compare exactly (a u64 beyond f64 precision must not
            // round-trip-match a float); mixed integer/float matches only
            // when the float is whole and equals the integer.
            match (stored.as_u64(), expected.as_u64()) {
                (Some(stored), Some(expected)) => stored == expected,
                (Some(stored), None) => match expected.as_f64() {
                    Some(expected) => whole_float_equals_u64(expected, stored),
                    None => false,
                },
                (None, Some(expected)) => match stored.as_f64() {
                    Some(stored) => whole_float_equals_u64(stored, expected),
                    None => false,
                },
                _ => match (stored.as_i64(), expected.as_i64()) {
                    (Some(stored), Some(expected)) => stored == expected,
                    (Some(stored), None) => match expected.as_f64() {
                        Some(expected) => whole_float_equals_i64(expected, stored),
                        None => false,
                    },
                    (None, Some(expected)) => match stored.as_f64() {
                        Some(stored) => whole_float_equals_i64(stored, expected),
                        None => false,
                    },
                    // Both floats (or out-of-range): plain f64 equality —
                    // the only remaining representation.
                    _ => stored == expected,
                },
            }
        }
        _ => stored == expected,
    }
}

fn whole_float_equals_u64(float: f64, int: u64) -> bool {
    if float.fract() != 0.0 || float < 0.0 || float >= 18_446_744_073_709_551_616.0 {
        return false;
    }
    float as u64 == int
}

const U64_BOUNDARIES: &[u64] = &[
    0,
    1,
    9007199254740992,   // 2^53
    9007199254740993,   // 2^53+1 (first int not exactly representable as f64)
    9223372036854775807,   // i64::MAX
    9223372036854775808,   // 2^63
    9223372036854775809,   // 2^63+1
    18446744073709551615,   // u64::MAX
];
const F64_BOUNDARIES: &[f64] = &[
    0.0,
    -0.0,
    1.0,
    -1.0,
    0.5,
    9007199254740992.0,       // 2^53
    9007199254740993.0,       // not representable; rounds to 2^53+2
    9223372036854775808.0,    // 2^63
    18446744073709551616.0,   // 2^64
    1e300,
    -1e300,
    1e-300,
    f64::MIN,
    f64::MAX,
];

fn finite_f64(rng: &mut Rng) -> f64 {
    loop {
        let f = rng.any_f64();
        if f.is_finite() {
            return f;
        }
    }
}

fn random_number(rng: &mut Rng) -> Number {
    match rng.below(9) {
        0 => Number::from(rng.range(0, 1000)),
        1 => Number::from(*rng.pick(U64_BOUNDARIES)),
        2 => Number::from(rng.next()),
        3 => Number::from(match rng.below(6) {
            0 => 0i64,
            1 => -1,
            2 => -9007199254740993, // -(2^53+1)
            3 => i64::MIN,
            4 => i64::MAX,
            _ => rng.next() as i64,
        }),
        4 => Number::from_f64(rng.next() as f64).filter(|n| n.is_f64()).unwrap_or_else(|| Number::from(0)),
        5 => Number::from_f64(*rng.pick(F64_BOUNDARIES)).unwrap(),
        6 => Number::from_f64(finite_f64(rng)).unwrap(),
        7 => Number::from_f64((rng.below(10_000) as f64) / 7.0).unwrap(),
        _ => Number::from_f64(-finite_f64(rng)).unwrap(),
    }
}

fn mutated_number(rng: &mut Rng, n: &Number) -> Number {
    if let Some(u) = n.as_u64() {
        match rng.below(5) {
            0 => Number::from(u.saturating_add(1)),
            1 => Number::from(u.saturating_sub(1)),
            2 => Number::from_f64(u as f64).unwrap(),
            3 => Number::from_f64((u as f64) + 0.5).unwrap(),
            _ => Number::from_f64(-(u as f64)).unwrap(),
        }
    } else if let Some(i) = n.as_i64() {
        match rng.below(5) {
            0 => Number::from(i.saturating_add(1)),
            1 => Number::from(i.saturating_sub(1)),
            2 => Number::from_f64(i as f64).unwrap(),
            3 => Number::from_f64((i as f64) + 0.5).unwrap(),
            _ => Number::from_f64(-(i as f64)).unwrap(),
        }
    } else if let Some(f) = n.as_f64() {
        match rng.below(5) {
            0 => Number::from_f64(f + 1.0).unwrap_or_else(|| Number::from(0)),
            1 => Number::from_f64(-f).unwrap_or_else(|| Number::from(0)),
            2 => Number::from_f64(f * 2.0).unwrap_or_else(|| Number::from(0)),
            3 => Number::from(f.trunc() as u64),
            _ => Number::from((f.trunc() as i64).saturating_add(1)),
        }
    } else {
        random_number(rng)
    }
}

fn random_json_value(rng: &mut Rng, depth: u64) -> Value {
    if depth == 0 {
        return match rng.below(5) {
            0..=3 => Value::Number(random_number(rng)),
            4 => Value::Bool(rng.bool()),
            _ => Value::Null,
        };
    }
    match rng.below(10) {
        0..=4 => Value::Number(random_number(rng)),
        5 => Value::String(
            (0..rng.range(0, 12))
                .map(|_| *rng.pick(UNICODE_POOL))
                .collect(),
        ),
        6 => Value::Bool(rng.bool()),
        7 => Value::Null,
        8 => Value::Array((0..rng.range(0, 4)).map(|_| random_json_value(rng, depth - 1)).collect()),
        _ => {
            let mut map = serde_json::Map::new();
            for k in 0..rng.range(0, 4) {
                map.insert(format!("k{k}"), random_json_value(rng, depth - 1));
            }
            Value::Object(map)
        }
    }
}

#[test]
fn fuzz_json_match_is_symmetric_and_reflexive() {
    let iterations = iters(30_000);
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let a = random_json_value(&mut rng, 3);
            let b = if rng.bool() {
                random_json_value(&mut rng, 3)
            } else if let Value::Number(n) = &a {
                Value::Number(mutated_number(&mut rng, n))
            } else {
                a.clone()
            };
            assert!(
                json_values_match(&a, &a),
                "reflexivity seed={seed} iter={i}: {a:?}"
            );
            let forward = json_values_match(&a, &b);
            let backward = json_values_match(&b, &a);
            assert_eq!(
                forward, backward,
                "SYMMETRY violation seed={seed} iter={i}: match({a:?}, {b:?})={forward} but match(b,a)={backward}"
            );
        }
    }
}

/// Exact-arithmetic oracle: the value a JSON number denotes, as an integer
/// when it IS an integer (f64 whole values convert exactly to i128).
fn exact_int(n: &Number) -> Option<i128> {
    if let Some(u) = n.as_u64() {
        return Some(u as i128);
    }
    if let Some(i) = n.as_i64() {
        return Some(i as i128);
    }
    let f = n.as_f64()?;
    if !f.is_finite() || f.fract() != 0.0 || f.abs() >= 1e30 {
        return None;
    }
    Some(f as i128)
}

fn exact_number_eq(a: &Number, b: &Number) -> bool {
    match (exact_int(a), exact_int(b)) {
        (Some(x), Some(y)) => x == y,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => a == b,
        },
    }
}

fn fmt_number(n: &Number) -> String {
    if let Some(u) = n.as_u64() {
        format!("json!({u}) [u64 {u}]")
    } else if let Some(i) = n.as_i64() {
        format!("json!({i}) [i64 {i}]")
    } else if let Some(f) = n.as_f64() {
        let exact = if f.fract() == 0.0 && f.abs() < 1e30 {
            format!(", exactly {}", f as i128)
        } else {
            String::new()
        };
        format!("json!({f:?}) [f64 {f:?}{exact}]")
    } else {
        format!("{n:?}")
    }
}

#[test]
fn fuzz_json_match_numbers_agree_with_exact_arithmetic() {
    // EXPECTED TO FAIL while the finding stands: the mixed int/float branches
    // compare `stored as f64 == expected`, so any u64/i64 beyond f64's 53-bit
    // mantissa round-trips onto a DIFFERENT whole float and still matches —
    // the exact round-trip the function's own doc comment forbids.
    let iterations = iters(30_000);
    let mut violations: Vec<(Number, Number)> = Vec::new();
    let mut first_context: Option<(u64, usize)> = None;
    'seeded: for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let a = random_number(&mut rng);
            let b = if rng.bool() {
                random_number(&mut rng)
            } else {
                mutated_number(&mut rng, &a)
            };
            let matched = json_values_match(&Value::Number(a.clone()), &Value::Number(b.clone()));
            let exact = exact_number_eq(&a, &b);
            if matched && !exact {
                violations.push((a, b));
                if first_context.is_none() {
                    first_context = Some((seed, i));
                }
                break 'seeded;
            }
            let fa = fmt_number(&a);
            let fb = fmt_number(&b);
            assert_eq!(
                matched,
                exact,
                "seed={seed} iter={i}: match({fa}, {fb}) == {matched} but exact equality is {exact}"
            );
        }
    }
    // Minimize: prefer the curated smallest boundary repros, else shrink the
    // random find to the first violating pair and report it verbatim.
    let curated = [
        (Number::from(9223372036854775809u64), 9223372036854775808.0f64), // 2^63+1 vs 2^63
        (Number::from(9007199254740993u64), 9007199254740992.0f64),       // 2^53+1 vs 2^53
        (Number::from(u64::MAX), 18446744073709551616.0f64),              // 2^64-1 vs 2^64
        (Number::from(-9223372036854775806i64), -9223372036854775808.0f64), // -(2^63-2) vs -2^63
    ];
    let minimized = curated
        .iter()
        .find(|(a, b)| {
            let bf = Number::from_f64(*b).unwrap();
            json_values_match(&Value::Number(a.clone()), &Value::Number(bf.clone()))
                && !exact_number_eq(a, &bf)
        })
        .map(|(a, b)| (a.clone(), Number::from_f64(*b).unwrap()))
        .or_else(|| violations.first().cloned());
    if let Some((a, b)) = minimized {
        let origin = first_context
            .map(|(seed, i)| format!("first random hit at seed={seed} iter={i}"))
            .unwrap_or_else(|| "curated boundary probe".to_string());
        panic!(
            "REAL BUG (replica of private record_api::json_values_match, src/app/record_api.rs:1112):\n\
             json_values_match({}, {}) == true but the two numbers are NOT numerically equal.\n\
             Root cause: the mixed integer/float arms compare `stored as f64 == expected`, so every\n\
             integer beyond f64's 53-bit mantissa rounds onto a neighbouring whole float and still\n\
             matches — the exact round-trip match the function's own doc comment forbids\n\
             (\"a u64 beyond f64 precision must not round-trip-match a float\"). A compare-and-set\n\
             `expected` gate can therefore pass against a different value. ({origin})",
            fmt_number(&a),
            fmt_number(&b),
        );
    }
}

// ── Target 4: web_link::parse_link — VERBATIM replica ────────────────────────

// Verbatim copy of src/io/web_link.rs:22-98 + private helpers 121-192
// (`pub(crate) mod web_link`, invisible to integration tests).

#[derive(Debug, Clone, PartialEq, Eq)]
enum LinkTarget {
    Url { url: String, name: String },
    RecentName(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkRefusal {
    Scheme,
    BadUrl,
    NotDrawing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ParsedLink {
    Absent,
    Target(LinkTarget),
    Ignored(LinkRefusal, String),
}

fn parse_link(search: &str) -> ParsedLink {
    let search = search.strip_prefix('?').unwrap_or(search);
    // `form_urlencoded` yields `Cow<str>` pairs in order; the FIRST `file`
    // param decides — a duplicated one must not turn into two opens.
    let value = url::form_urlencoded::parse(search.as_bytes())
        .find(|(key, _)| key == "file")
        .map(|(_, value)| value.into_owned());
    let Some(value) = value else {
        return ParsedLink::Absent;
    };
    let value = value.trim();
    if value.is_empty() {
        return ParsedLink::Absent;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let parsed = match url::Url::parse(value) {
            Ok(parsed) => parsed,
            Err(_) => return ParsedLink::Ignored(LinkRefusal::BadUrl, value.to_string()),
        };
        return match trailing_drawing_name(&parsed) {
            Some(name) => ParsedLink::Target(LinkTarget::Url {
                url: parsed.to_string(),
                name,
            }),
            None => ParsedLink::Ignored(LinkRefusal::NotDrawing, value.to_string()),
        };
    }
    if let Some(_scheme) = scheme_prefix(value) {
        // Never dereference: the gate the remote-image fetch applies, so a
        // `file:`/`javascript:`/drive-letter value dies here, warned only.
        return ParsedLink::Ignored(LinkRefusal::Scheme, value.to_string());
    }
    ParsedLink::Target(LinkTarget::RecentName(value.to_string()))
}

fn scheme_prefix(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let end = bytes.iter().position(|&byte| byte == b':')?;
    let scheme = &value[..end];
    let valid = !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
    valid.then(|| scheme.to_string())
}

fn trailing_drawing_name(url: &url::Url) -> Option<String> {
    let segment = percent_decode(url.path().rsplit('/').next().unwrap_or_default());
    if !is_drawing_name(&segment) {
        return None;
    }
    Some(segment)
}

fn is_drawing_name(name: &str) -> bool {
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "dwg" | "dxf"
    )
}

fn percent_decode(value: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%' && index + 2 < bytes.len())
            .then(|| (hex(bytes[index + 1]), hex(bytes[index + 2])));
        match escape {
            Some((Some(high), Some(low))) => {
                out.push(high * 16 + low);
                index += 3;
            }
            _ => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Independent re-derivation of the RFC 3986 scheme shape for the oracle.
fn spec_scheme(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let end = bytes.iter().position(|&byte| byte == b':')?;
    let scheme = &value[..end];
    let valid = !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
    valid.then(|| scheme.to_string())
}

/// Percent-encode every byte so the decoded param value is exactly `value`.
fn enc_all(value: &str) -> String {
    value
        .bytes()
        .map(|byte| format!("%{byte:02X}"))
        .collect()
}

fn gen_link_value(rng: &mut Rng) -> String {
    let mut value = String::new();
    match rng.below(10) {
        0 => value.push_str(*rng.pick(&[
            "http://", "https://", "HTTP://", "Https://", "http:", "https:/", "ftp://",
            "file:///", "javascript:", "data:", "blob:", "c:", "C:\\", "x+1-2.3:", "1http://",
            "mailto:a@b",
        ])),
        1 => value.push_str(*rng.pick(&[
            "example.com", "example.com/", "example.com/a/b/", "a/b/c.dwg", "p.dwg", "p.DXF",
            "p.dxf", "p.txt", "p.dwg.txt", "site.dwg?v=1#frag", "..dwgx", "%2e%2e", "DWG",
            "my plan", "çizim.dwg", "", "/", "//", "a b.dwg",
        ])),
        2 => {
            for _ in 0..rng.range(0, 8) {
                value.push(*rng.pick(UNICODE_POOL));
            }
        }
        3 => {
            for _ in 0..rng.range(0, 10) {
                value.push(*rng.pick(HOSTILE_ASCII));
            }
        }
        4 => value.push_str(&"a".repeat(rng.range(1, 300) as usize)),
        5 => {
            value.push_str("http://example.com/");
            value.push_str(&"d".repeat(rng.range(1, 64) as usize));
            value.push_str(*rng.pick(&[".dwg", ".dxf", ".pdf", "", ".DWG"]));
        }
        _ => {
            for _ in 0..rng.range(0, 20) {
                let pool = if rng.bool() { HOSTILE_ASCII } else { UNICODE_POOL };
                value.push(*rng.pick(pool));
            }
        }
    }
    match rng.below(6) {
        0 => format!(" {value}"),
        1 => format!("{value} "),
        2 => format!("\t{value}\n"),
        3 => format!("\u{00a0}{value}"),
        _ => value,
    }
}

fn gen_raw_search(rng: &mut Rng) -> String {
    let len = if rng.below(100) == 0 {
        rng.range(10_000, 100_000)
    } else {
        rng.range(0, 2_000)
    } as usize;
    let pool: Vec<char> = HOSTILE_ASCII
        .iter()
        .chain(UNICODE_POOL.iter())
        .chain(['?', '%', '+', '&', '=', '#'].iter())
        .copied()
        .collect();
    (0..len).map(|_| *rng.pick(&pool)).collect()
}

#[test]
fn fuzz_parse_link_never_panics_on_random_search_strings() {
    let iterations = iters(20_000);
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let search = gen_raw_search(&mut rng);
            let probed = format!("?{}", search);
            for input in [&search[..], &probed[..]] {
                let outcome = catch_unwind(AssertUnwindSafe(|| parse_link(input)));
                assert!(
                    outcome.is_ok(),
                    "parse_link PANICKED seed={seed} iter={i} on {:?} ({} chars)",
                    &input[..input.len().min(80)],
                    input.len()
                );
            }
        }
    }
}

#[test]
fn fuzz_parse_link_first_file_param_wins() {
    let iterations = iters(15_000);
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let v1 = gen_link_value(&mut rng);
            let v2 = gen_link_value(&mut rng);
            let search = format!(
                "file={}&z=1&file={}&w={}",
                enc_all(&v1),
                enc_all(&v2),
                enc_all(&v2)
            );
            let with_prefix = format!("?{search}");
            let expected = parse_link(&format!("file={}", enc_all(&v1)));
            assert_eq!(
                parse_link(&search),
                expected,
                "first-wins seed={seed} iter={i} v1={:?} v2={:?}",
                display_truncated(&v1),
                display_truncated(&v2)
            );
            assert_eq!(
                parse_link(&with_prefix),
                expected,
                "leading-? invariance seed={seed} iter={i}"
            );
        }
    }
    // An empty FIRST value is absent, not skipped (their own unit pinned
    // behavior — the fuzz must agree on every generated second value).
    assert_eq!(
        parse_link("?file=&file=plan.dwg"),
        ParsedLink::Absent,
        "empty first value must win"
    );
    // Only the exact lowercase key opens.
    assert_eq!(parse_link("?File=plan.dwg"), ParsedLink::Absent);
}

#[test]
fn fuzz_parse_link_scheme_gate_is_consistent() {
    let iterations = iters(15_000);
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let value = gen_link_value(&mut rng);
            let got = parse_link(&format!("?file={}", enc_all(&value)));
            let trimmed = value.trim().to_string();
            let context = format!(
                "seed={seed} iter={i} value={:?}",
                display_truncated(&value)
            );
            if trimmed.is_empty() {
                assert_eq!(got, ParsedLink::Absent, "empty-after-trim {context}");
                continue;
            }
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("http://") || lower.starts_with("https://") {
                match &got {
                    ParsedLink::Target(LinkTarget::RecentName(_)) => panic!(
                        "http(s) value must never become a recent-name lookup: {context}"
                    ),
                    ParsedLink::Ignored(LinkRefusal::Scheme, _) => panic!(
                        "http(s) value must never take the foreign-scheme refusal: {context}"
                    ),
                    ParsedLink::Target(LinkTarget::Url { name, .. }) => {
                        let ext = name.rsplit('.').next().unwrap_or_default().to_ascii_lowercase();
                        assert!(
                            matches!(ext.as_str(), "dwg" | "dxf"),
                            "extracted name {name:?} fails the drawing-extension rule: {context}"
                        );
                    }
                    ParsedLink::Ignored(_, raw) => assert_eq!(
                        raw, &trimmed,
                        "refusal must echo the trimmed value: {context}"
                    ),
                    ParsedLink::Absent => panic!("non-empty value can never be absent: {context}"),
                }
            } else if spec_scheme(&trimmed).is_some() {
                assert_eq!(
                    got,
                    ParsedLink::Ignored(LinkRefusal::Scheme, trimmed),
                    "non-http scheme must be refused as Scheme {context}"
                );
            } else {
                assert_eq!(
                    got,
                    ParsedLink::Target(LinkTarget::RecentName(trimmed)),
                    "scheme-free value must be a recent-name target exactly {context}"
                );
            }
        }
    }
}

// ── Target 5: web_autosave — VERBATIM replica ────────────────────────────────

// Verbatim copies from src/io/web_autosave.rs:34-124 (`pub(crate)` module).

#[derive(Debug, Clone)]
struct WebAutosaveEntry {
    name: String,
    size: u64,
    modified_ms: f64,
}

const SNAPSHOT_CAP: usize = 16;

fn newer_than_saved(autosave_ms: f64, saved_ms: Option<f64>) -> bool {
    match saved_ms {
        Some(saved_ms) => autosave_ms > saved_ms,
        None => true,
    }
}

fn cap_evictions(
    mut entries: Vec<WebAutosaveEntry>,
    cap: usize,
) -> (Vec<WebAutosaveEntry>, Vec<String>) {
    entries.sort_by(|a, b| b.modified_ms.total_cmp(&a.modified_ms));
    let overflow = entries.len().saturating_sub(cap);
    let mut evicted: Vec<String> = entries
        .split_off(entries.len() - overflow)
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    evicted.reverse();
    (entries, evicted)
}

const RECOVERED_MARKER: &str = " (recovered)";

fn recovered_name(drawing: &str) -> String {
    let name = drawing.rsplit(['/', '\\']).next().unwrap_or_default();
    let (head, ext) = match name.rsplit_once('.') {
        Some((head, ext)) if !ext.is_empty() => (head, ext.to_string()),
        _ => (name, "dxf".to_string()),
    };
    let mut stem = if head.is_empty() { name } else { head };
    while let Some(stripped) = stem.strip_suffix(RECOVERED_MARKER) {
        stem = stripped;
    }
    format!("{stem}{RECOVERED_MARKER}.{ext}")
}

fn gen_autosave_name(rng: &mut Rng) -> String {
    let mut name = String::new();
    match rng.below(12) {
        0 => name.push_str(*rng.pick(&[
            "", ".", "..", "plan.dwg", "sketch.dxf", "Drawing1", "a.b.dwg", ".dxf", ".dwg",
            "plan.sv$", "plan (recovered).dwg", "plan (recovered) (recovered).dwg",
            "(recovered) plan.dwg", "plan.", "a.", " ", "con", "NUL.png",
        ])),
        1 => {
            for _ in 0..rng.range(0, 3) {
                name.push_str(RECOVERED_MARKER);
            }
        }
        2 => {
            for _ in 0..rng.range(0, 8) {
                name.push(*rng.pick(UNICODE_POOL));
            }
        }
        3 => {
            for _ in 0..rng.range(0, 10) {
                let pool = if rng.bool() { HOSTILE_ASCII } else { UNICODE_POOL };
                name.push(*rng.pick(pool));
            }
        }
        4 => name.push_str(&"x".repeat(rng.range(1, 4096) as usize)),
        _ => {
            for _ in 0..rng.range(1, 5) {
                name.push_str(*rng.pick(&[
                    "plan", "a", ".", "..", "/", "\\", "a/", "\\b", ".dwg", ".dxf", " (recovered)",
                    "図面", "π", "-", " ", "..",
                ]));
            }
        }
    }
    if rng.below(8) == 0 {
        name.push_str(".dwg");
    }
    name
}

fn minimize_string(start: &str, violates: &dyn Fn(&str) -> bool) -> String {
    // Replace non-ASCII first when that preserves the violation…
    let ascii: String = start
        .chars()
        .map(|c| if c.is_ascii() { c } else { 'a' })
        .collect();
    let mut cur = if violates(&ascii) { ascii } else { start.to_string() };
    // …then delete one character at a time while the violation survives.
    loop {
        let mut changed = false;
        let starts: Vec<usize> = cur.char_indices().map(|(i, _)| i).collect();
        for at in starts {
            let mut candidate = String::with_capacity(cur.len());
            candidate.push_str(&cur[..at]);
            let mut tail = cur[at..].chars();
            tail.next();
            candidate.extend(tail);
            if violates(&candidate) {
                cur = candidate;
                changed = true;
                break;
            }
        }
        if !changed {
            return cur;
        }
    }
}

#[test]
fn fuzz_recovered_name_is_idempotent_and_marker_never_stacks() {
    // EXPECTED TO FAIL while the finding stands: the empty-head fallback
    // (`if head.is_empty() { drawing }`, meant for dot-files like ".dxf")
    // reinstates the WHOLE unsanitized input as the stem. For a stored name
    // whose head-before-last-dot ends in a path separator (e.g. "/.dwg"),
    // the first call leaves the separator in the result while the second
    // call strips it — so the recovered name alternates across crash
    // cycles instead of being idempotent.
    let iterations = iters(20_000);
    let mut violations = 0usize;
    let mut first: Option<(u64, usize, String)> = None;
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let name = gen_autosave_name(&mut rng);
            let once = recovered_name(&name);
            let twice = recovered_name(&once);
            let structure_ok = match once.rsplit_once('.') {
                Some((head, ext)) => {
                    !ext.is_empty()
                        && head.ends_with(RECOVERED_MARKER)
                        && !head
                            .strip_suffix(RECOVERED_MARKER)
                            .unwrap()
                            .ends_with(RECOVERED_MARKER)
                }
                None => false,
            };
            if once != twice || !structure_ok {
                violations += 1;
                if first.is_none() {
                    first = Some((seed, i, name));
                }
            }
        }
    }
    let breaks = |name: &str| {
        let once = recovered_name(name);
        let twice = recovered_name(&once);
        let structure_broken = match once.rsplit_once('.') {
            Some((head, ext)) => {
                ext.is_empty()
                    || !head.ends_with(RECOVERED_MARKER)
                    || head
                        .strip_suffix(RECOVERED_MARKER)
                        .unwrap()
                        .ends_with(RECOVERED_MARKER)
            }
            None => true,
        };
        once != twice || structure_broken
    };
    let minimized = if breaks("/.dwg") {
        "/.dwg".to_string()
    } else if let Some((seed, i, name)) = first.clone() {
        eprintln!("shrinking first fuzz hit seed={seed} iter={i} input={:?}", display_truncated(&name));
        minimize_string(&name, &breaks)
    } else {
        String::new()
    };
    if !minimized.is_empty() {
        panic!(
            "REAL BUG (replica of pub(crate) src/io/web_autosave.rs `recovered_name`):\n\
             recovered_name({minimized:?}) = {:?}, but recovered_name(that) = {:?} — the name is\n\
             NOT idempotent, so the restored tab/download name alternates across crash cycles\n\
             (the marker-stability guarantee the function documents). Root cause: when the\n\
             head-before-last-dot reduces to an empty component after separator stripping\n\
             (any stored name whose head ends in '/' or '\\\\'), the `head.is_empty()` fallback\n\
             reinstates the whole UNSANITIZED input as the stem; the second application then\n\
             sees a non-empty head and strips the separator. Fuzz stats: {violations} violating\n\
             names in {iterations} iterations per seed; minimized by greedy char deletion.",
            recovered_name(&minimized),
            recovered_name(&recovered_name(&minimized)),
        );
    }
}

#[test]
fn fuzz_recovered_name_never_smuggles_path_separators() {
    // EXPECTED TO FAIL while the finding stands: only the HEAD is stripped to
    // its final path component; the EXTENSION (whatever follows the last dot)
    // is passed through verbatim, so a separator after the last dot survives
    // into the tab title / download name.
    let iterations = iters(20_000);
    let mut first_violation: Option<(u64, usize, String)> = None;
    let mut violations = 0usize;
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let name = gen_autosave_name(&mut rng);
            let result = recovered_name(&name);
            if result.contains('/') || result.contains('\\') {
                violations += 1;
                if first_violation.is_none() {
                    first_violation = Some((seed, i, name));
                }
            }
        }
    }
    let violates = |name: &str| {
        let out = recovered_name(name);
        out.contains('/') || out.contains('\\')
    };
    // The minimized repro (greedy char deletion; also try the curated probe
    // first so the report is stable across seeds).
    let minimized = if violates("a.b/c") {
        "a.b/c".to_string()
    } else if let Some((seed, i, name)) = first_violation.clone() {
        eprintln!("shrinking first fuzz hit seed={seed} iter={i} input={:?}", display_truncated(&name));
        minimize_string(&name, &violates)
    } else {
        String::new()
    };
    if !minimized.is_empty() {
        panic!(
            "REAL BUG (replica of pub(crate) src/io/web_autosave.rs `recovered_name`):\n\
             recovered_name({minimized:?}) = {:?} — the result carries a path separator into the\n\
             restored tab title / download name. Only the stem (head before the last dot) is\n\
             reduced to its final path component; the extension after the last dot passes through\n\
             verbatim, so any '/' or '\\\\' AFTER the last dot is smuggled through. The existing\n\
             red-team suite only probes separators BEFORE the last dot (tests/web_recovered_name_redteam.rs\n\
             `benign_hostile_names_round_trip_parseable`). Reachable in production via a deep link:\n\
             parse_link(\"?file=a.b%2Fc\") → RecentName(\"a.b/c\") is a legal autosave name.\n\
             Fuzz stats: {violations} violating names in {iterations} iterations per seed; minimized by greedy char deletion.",
            recovered_name(&minimized),
        );
    }
}

fn gen_mtime(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0..=6 => rng.range(0, 2_000_000_000_000) as f64,
        7 => *rng.pick(&[
            0.0,
            -0.0,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MIN,
            f64::MAX,
            1e300,
            -1e300,
        ]),
        _ => rng.any_f64(),
    }
}

fn autosave_entry(name: String, modified_ms: f64) -> WebAutosaveEntry {
    WebAutosaveEntry {
        name,
        size: 16,
        modified_ms,
    }
}

fn descending(values: &[f64]) -> bool {
    values
        .windows(2)
        .all(|w| w[0].total_cmp(&w[1]) != Ordering::Less)
}

fn ascending(values: &[f64]) -> bool {
    values
        .windows(2)
        .all(|w| w[0].total_cmp(&w[1]) != Ordering::Greater)
}

#[test]
fn fuzz_cap_evictions_partitions_newest_first_and_is_stable() {
    let iterations = iters(20_000);
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let count = rng.range(0, 41) as usize;
            let entries: Vec<WebAutosaveEntry> = (0..count)
                .map(|k| autosave_entry(format!("d{k}"), gen_mtime(&mut rng)))
                .collect();
            let cap = rng.range(0, 25) as usize;
            let input_names: Vec<String> = {
                let mut names: Vec<String> = entries.iter().map(|e| e.name.clone()).collect();
                names.sort();
                names
            };
            let (kept, evicted) = cap_evictions(entries.clone(), cap);

            // Exactly the overflow is evicted, nothing is lost or duplicated.
            assert_eq!(kept.len(), count.min(cap), "kept size seed={seed} iter={i}");
            assert_eq!(
                evicted.len(),
                count.saturating_sub(cap),
                "evicted size seed={seed} iter={i}"
            );
            let mut outcome = kept.iter().map(|e| e.name.clone()).collect::<Vec<_>>();
            outcome.extend(evicted.iter().cloned());
            outcome.sort();
            assert_eq!(
                outcome, input_names,
                "names partition the input seed={seed} iter={i}"
            );

            // Ordering: kept newest-first, evicted oldest-first.
            let kept_times: Vec<f64> = kept.iter().map(|e| e.modified_ms).collect();
            let evicted_times: Vec<f64> = {
                // Recover the evicted mtimes from the input names.
                evicted
                    .iter()
                    .map(|name| {
                        entries
                            .iter()
                            .find(|e| &e.name == name)
                            .unwrap()
                            .modified_ms
                    })
                    .collect()
            };
            assert!(descending(&kept_times), "kept newest-first seed={seed} iter={i}");
            assert!(ascending(&evicted_times), "evicted oldest-first seed={seed} iter={i}");
            if let (Some(last_kept), Some(first_evicted)) = (kept_times.last(), evicted_times.first())
            {
                assert!(
                    last_kept.total_cmp(first_evicted) != Ordering::Less,
                    "every kept snapshot outranks every evicted one seed={seed} iter={i}"
                );
            }

            // Idempotent: re-running on the survivors evicts nothing more.
            let (kept2, evicted2) = cap_evictions(kept.clone(), cap);
            assert!(evicted2.is_empty(), "cap idempotence seed={seed} iter={i}");
            assert_eq!(kept2.len(), kept.len(), "cap idempotence size seed={seed} iter={i}");

            // Deterministic: same input, same split (names + exact f64 bits).
            let normalize = |kept: &Vec<WebAutosaveEntry>, evicted: &Vec<String>| {
                (
                    kept.iter()
                        .map(|e| (e.name.clone(), e.modified_ms.to_bits()))
                        .collect::<Vec<_>>(),
                    evicted.clone(),
                )
            };
            let (kept3, evicted3) = cap_evictions(entries, cap);
            assert_eq!(
                normalize(&kept3, &evicted3),
                normalize(&kept, &evicted),
                "determinism seed={seed} iter={i}"
            );
        }
    }
}

#[test]
fn fuzz_newer_than_saved_orders_finite_times_and_never_offers_nan() {
    let iterations = iters(20_000);
    for seed in seeds() {
        let mut rng = Rng::new(seed);
        for i in 0..iterations {
            let a = gen_mtime(&mut rng);
            let b = gen_mtime(&mut rng);
            // Never panics, and mirrors numeric order exactly (`NaN > x` is
            // false, so a NaN autosave mtime is never offered).
            assert_eq!(
                newer_than_saved(a, Some(b)),
                a > b,
                "numeric order seed={seed} iter={i} a={a:?} b={b:?}"
            );
            assert!(
                !(newer_than_saved(a, Some(b)) && newer_than_saved(b, Some(a))),
                "offer decision is mutually exclusive seed={seed} iter={i}"
            );
            assert!(!newer_than_saved(a, Some(a)), "equal times never offered");
            // No saved copy at all: the snapshot is the only survivor.
            assert!(newer_than_saved(a, None), "None always offered seed={seed} iter={i}");
        }
    }
}
