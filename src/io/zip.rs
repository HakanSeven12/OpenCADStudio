//! Minimal in-memory ZIP writer.
//!
//! The web ARCHIVE/ETRANSMIT packages a transmittal (drawing + registered
//! dependencies) into one browser download; ZIP is the container every
//! recipient can open. This writer emits the classic (pre-zip64) format
//! directly — local file headers, a central directory, and the end record —
//! with CRC32 and DEFLATE from the `flate2` crate the tree already depends
//! on, so no new dependency is pulled in for a handful of fixed-size
//! little-endian records.
//!
//! Constraints of the classic format are enforced with errors instead of
//! wrapping: more than 65535 entries, an entry over 4 GiB, or an archive
//! over 4 GiB cannot be represented and refuse to build. Browser-download
//! transmittals are far below every limit.
//!
//! Every entry is stamped with the same fixed DOS timestamp and flat,
//! separator-free name, so building the same entries twice yields
//! byte-identical output.

use std::io::Write;

use flate2::write::DeflateEncoder;
use flate2::Compression;

/// Local file header signature ("PK\x03\x04").
const LOCAL_SIG: u32 = 0x0403_4b50;
/// Central directory file header signature ("PK\x01\x02").
const CENTRAL_SIG: u32 = 0x0201_4b50;
/// End of central directory record signature ("PK\x05\x06").
const END_SIG: u32 = 0x0605_4b50;

/// Compression method 0: bytes stored verbatim.
const METHOD_STORE: u16 = 0;
/// Compression method 8: raw DEFLATE (no zlib wrapper).
const METHOD_DEFLATE: u16 = 8;

/// Version needed to extract (2.0 — the deflate minimum) and the version
/// that "made" the archive (2.0, MS-DOS host so no external-attribute
/// permissions are implied). One constant serves both fields.
const VERSION_2_0: u16 = 20;

/// General-purpose flag 0x0800: entry names are UTF-8.
const FLAG_UTF8: u16 = 0x0800;

/// Timestamp stamped on every entry: 2026-01-01 00:00:00 in DOS packed
/// form. Fixed rather than taken from the clock so archives are
/// deterministic (and testable) byte for byte.
const DOS_TIME: u16 = 0; // 00:00:00
const DOS_DATE: u16 = (2026 - 1980) << 9 | 1 << 5 | 1; // 2026-01-01

/// One file to package: a flat name and its bytes.
#[derive(Clone, Debug)]
pub struct ZipEntry {
    pub name: String,
    pub data: Vec<u8>,
}

impl ZipEntry {
    pub fn new(name: impl Into<String>, data: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            data,
        }
    }
}

/// Build a complete ZIP archive from `entries`, in order.
///
/// Each entry is DEFLATE-compressed and kept only when that actually
/// shrinks it (drawings compress well; PNG/PDF payloads are stored instead
/// of being pointlessly re-deflated) — the same choice mainstream writers
/// make. Duplicate names are written as given; callers that want one file
/// per name dedupe before building.
pub fn build_zip(entries: &[ZipEntry]) -> Result<Vec<u8>, String> {
    let count = u16::try_from(entries.len())
        .map_err(|_| format!("too many entries: {}", entries.len()))?;
    // 0xFFFF/0xFFFFFFFF are the classic-format ZIP64 sentinels; writing
    // them without a ZIP64 EOCD would send strict readers hunting for one.
    if count == u16::MAX {
        return Err("too many entries (ZIP64 sentinel)".to_string());
    }
    let mut out: Vec<u8> = Vec::new();
    let mut central: Vec<u8> = Vec::new();
    for entry in entries {
        let name = validate_name(&entry.name)?;
        let name_len =
            u16::try_from(name.len()).map_err(|_| format!("entry name too long: {}", name.len()))?;

        let mut crc = flate2::Crc::new();
        crc.update(&entry.data);
        let crc = crc.sum();

        let deflated = deflate(&entry.data);
        let (method, payload): (u16, &[u8]) = if deflated.len() < entry.data.len() {
            (METHOD_DEFLATE, &deflated)
        } else {
            (METHOD_STORE, &entry.data)
        };
        let compressed_size = u32::try_from(payload.len())
            .map_err(|_| format!("entry {} exceeds the 4 GiB classic-ZIP limit", name))?;
        let uncompressed_size = u32::try_from(entry.data.len())
            .map_err(|_| format!("entry {} exceeds the 4 GiB classic-ZIP limit", name))?;
        let local_offset = u32::try_from(out.len())
            .map_err(|_| "archive exceeds the 4 GiB classic-ZIP limit".to_string())?;
        // 0xFFFFFFFF is the classic-format ZIP64 sentinel; writing it without
        // a ZIP64 EOCD sends strict readers hunting for one that is absent.
        if local_offset == u32::MAX
            || compressed_size == u32::MAX
            || uncompressed_size == u32::MAX
        {
            return Err("archive exceeds the classic-ZIP limit".to_string());
        }

        // Local file header — immediately followed by name and payload.
        out.extend_from_slice(&LOCAL_SIG.to_le_bytes());
        out.extend_from_slice(&VERSION_2_0.to_le_bytes());
        out.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&DOS_TIME.to_le_bytes());
        out.extend_from_slice(&DOS_DATE.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&compressed_size.to_le_bytes());
        out.extend_from_slice(&uncompressed_size.to_le_bytes());
        out.extend_from_slice(&name_len.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(payload);

        // Central directory record for the entry just written.
        central.extend_from_slice(&CENTRAL_SIG.to_le_bytes());
        central.extend_from_slice(&VERSION_2_0.to_le_bytes()); // version made by
        central.extend_from_slice(&VERSION_2_0.to_le_bytes()); // version needed
        central.extend_from_slice(&FLAG_UTF8.to_le_bytes());
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&DOS_TIME.to_le_bytes());
        central.extend_from_slice(&DOS_DATE.to_le_bytes());
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&compressed_size.to_le_bytes());
        central.extend_from_slice(&uncompressed_size.to_le_bytes());
        central.extend_from_slice(&name_len.to_le_bytes());
        central.extend_from_slice(&0u16.to_le_bytes()); // extra field length
        central.extend_from_slice(&0u16.to_le_bytes()); // comment length
        central.extend_from_slice(&0u16.to_le_bytes()); // disk number start
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&local_offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }

    let central_offset = u32::try_from(out.len())
        .map_err(|_| "archive exceeds the 4 GiB classic-ZIP limit".to_string())?;
    let central_size = u32::try_from(central.len())
        .map_err(|_| "archive exceeds the 4 GiB classic-ZIP limit".to_string())?;
    if central_offset == u32::MAX || central_size == u32::MAX {
        return Err("archive exceeds the classic-ZIP limit".to_string());
    }
    out.extend_from_slice(&central);

    // End of central directory record — no archive comment.
    out.extend_from_slice(&END_SIG.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // this disk number
    out.extend_from_slice(&0u16.to_le_bytes()); // central directory start disk
    out.extend_from_slice(&count.to_le_bytes()); // entries on this disk
    out.extend_from_slice(&count.to_le_bytes()); // entries total
    out.extend_from_slice(&central_size.to_le_bytes());
    out.extend_from_slice(&central_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment length
    Ok(out)
}

/// Entry names must be flat file names: non-empty, free of path separators
/// and NUL, not a directory dot-name. Keeping the archive flat mirrors the
/// native transmittal folder (drawing and dependencies side by side) and
/// rules out path traversal on extraction.
fn validate_name(name: &str) -> Result<&str, String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', '\0'])
        || name.len() > u16::MAX as usize
    {
        return Err(format!("invalid ZIP entry name: {name:?}"));
    }
    Ok(name)
}

/// Raw DEFLATE stream (what method 8 stores — no zlib header/checksum).
/// Infallible in practice: the encoder writes into a `Vec`, which cannot
/// fail; on an impossible error the uncompressed bytes are kept so the
/// caller still receives a usable payload.
fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    if encoder.write_all(data).is_err() {
        return data.to_vec();
    }
    encoder.finish().unwrap_or_else(|_| data.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    /// The CRC-32 (IEEE 802.3) check value for "123456789" — the standard
    /// reference every CRC implementation is verified against.
    const CRC_CHECK_VALUE: u32 = 0xCBF4_3926;

    /// One entry as read back from the central directory.
    struct CentralEntry {
        name: String,
        method: u16,
        crc: u32,
        compressed_size: u32,
        uncompressed_size: u32,
        local_offset: u32,
    }

    fn u16_at(zip: &[u8], at: usize) -> u16 {
        u16::from_le_bytes([zip[at], zip[at + 1]])
    }
    fn u32_at(zip: &[u8], at: usize) -> u32 {
        u32::from_le_bytes([zip[at], zip[at + 1], zip[at + 2], zip[at + 3]])
    }

    /// Parse the end record (always the last 22 bytes — the writer never
    /// emits a comment) and walk the central directory it points at.
    fn parse_central_directory(zip: &[u8]) -> (Vec<CentralEntry>, u16, u32, u32) {
        assert!(zip.len() >= 22, "archive must hold an end record");
        let eocd = zip.len() - 22;
        assert_eq!(&zip[eocd..eocd + 4], &END_SIG.to_le_bytes());
        assert_eq!(u16_at(zip, eocd + 4), 0, "disk number");
        assert_eq!(u16_at(zip, eocd + 6), 0, "central directory start disk");
        let on_disk = u16_at(zip, eocd + 8);
        let total = u16_at(zip, eocd + 10);
        let size = u32_at(zip, eocd + 12);
        let offset = u32_at(zip, eocd + 16);
        assert_eq!(u16_at(zip, eocd + 20), 0, "comment length");
        assert_eq!(on_disk, total, "single-disk archive");

        let mut entries = Vec::with_capacity(total as usize);
        let mut cursor = offset as usize;
        for _ in 0..total {
            assert_eq!(&zip[cursor..cursor + 4], &CENTRAL_SIG.to_le_bytes());
            let method = u16_at(zip, cursor + 10);
            let crc = u32_at(zip, cursor + 16);
            let compressed_size = u32_at(zip, cursor + 20);
            let uncompressed_size = u32_at(zip, cursor + 24);
            let name_len = u16_at(zip, cursor + 28) as usize;
            assert_eq!(u16_at(zip, cursor + 30), 0, "extra length");
            assert_eq!(u16_at(zip, cursor + 32), 0, "comment length");
            assert_eq!(u16_at(zip, cursor + 34), 0, "disk number start");
            let local_offset = u32_at(zip, cursor + 42);
            let name = String::from_utf8(zip[cursor + 46..cursor + 46 + name_len].to_vec())
                .expect("entry names are UTF-8");
            entries.push(CentralEntry {
                name,
                method,
                crc,
                compressed_size,
                uncompressed_size,
                local_offset,
            });
            cursor += 46 + name_len;
        }
        assert_eq!(cursor - offset as usize, size as usize, "recorded size");
        assert_eq!(cursor, eocd, "central directory ends at the end record");
        (entries, total, size, offset)
    }

    /// Read one entry's payload back through its local header, inflating
    /// method-8 entries — the round trip an extractor performs.
    fn read_entry_payload(zip: &[u8], entry: &CentralEntry) -> Vec<u8> {
        let at = entry.local_offset as usize;
        assert_eq!(&zip[at..at + 4], &LOCAL_SIG.to_le_bytes());
        let name_len = u16_at(zip, at + 26) as usize;
        assert_eq!(u16_at(zip, at + 28), 0, "extra length");
        let name = String::from_utf8(zip[at + 30..at + 30 + name_len].to_vec()).unwrap();
        assert_eq!(name, entry.name, "local and central names agree");
        let start = at + 30 + name_len;
        let payload = &zip[start..start + entry.compressed_size as usize];
        match entry.method {
            METHOD_STORE => payload.to_vec(),
            METHOD_DEFLATE => {
                let mut inflated = Vec::new();
                DeflateDecoder::new(payload)
                    .read_to_end(&mut inflated)
                    .expect("deflate payload inflates");
                inflated
            }
            other => panic!("unexpected method {other}"),
        }
    }

    /// Deterministic poorly-compressible bytes (an LCG keystream) so the
    /// STORE branch is exercised without a fixture file.
    fn incompressible(len: usize) -> Vec<u8> {
        let mut state = 0x2545_F491_u32;
        (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn empty_archive_is_a_bare_end_record() {
        let zip = build_zip(&[]).expect("empty archive builds");
        assert_eq!(zip.len(), 22);
        let (entries, total, size, offset) = parse_central_directory(&zip);
        assert_eq!(entries.len(), 0);
        assert_eq!(total, 0);
        assert_eq!(size, 0);
        assert_eq!(offset, 0);
    }

    #[test]
    fn central_directory_round_trips_known_contents() {
        let drawing = b"0\nSECTION\n2\nENTITIES\n0\nENDSEC\n0\nEOF\n".repeat(40);
        let entries = vec![
            ZipEntry::new("plan.dwg", drawing.clone()),
            ZipEntry::new("site.png", incompressible(512)),
            ZipEntry::new("notes.txt", Vec::new()),
        ];
        let zip = build_zip(&entries).expect("archive builds");

        let (central, total, size, offset) = parse_central_directory(&zip);
        assert_eq!(total as usize, entries.len());
        assert_eq!(central.len(), entries.len());
        assert!(size > 0 && (offset as usize) < zip.len());

        let names: Vec<&str> = central.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["plan.dwg", "site.png", "notes.txt"]);

        for (entry, parsed) in entries.iter().zip(&central) {
            let payload = read_entry_payload(&zip, parsed);
            assert_eq!(payload, entry.data, "{} round-trips", entry.name);
            assert_eq!(parsed.uncompressed_size as usize, entry.data.len());
            let mut crc = flate2::Crc::new();
            crc.update(&entry.data);
            assert_eq!(parsed.crc, crc.sum(), "{} CRC matches flate2", entry.name);
            assert!(parsed.compressed_size as usize <= entry.data.len());
        }

        // The repetitive drawing DEFLATES; the keystream image does not.
        assert_eq!(central[0].method, METHOD_DEFLATE);
        assert!(central[0].compressed_size < central[0].uncompressed_size);
        assert_eq!(central[1].method, METHOD_STORE);
        assert_eq!(central[1].compressed_size, central[1].uncompressed_size);
        // An empty entry has nothing to shrink: stored, zero sizes.
        assert_eq!(central[2].method, METHOD_STORE);
        assert_eq!(central[2].compressed_size, 0);
        assert_eq!(central[2].uncompressed_size, 0);
    }

    #[test]
    fn stored_crc_matches_the_reference_check_value() {
        let zip = build_zip(&[ZipEntry::new("check.bin", b"123456789".to_vec())])
            .expect("archive builds");
        let (central, _, _, _) = parse_central_directory(&zip);
        assert_eq!(central[0].crc, CRC_CHECK_VALUE);
    }

    #[test]
    fn local_offsets_and_determinism_hold() {
        let entries = vec![
            ZipEntry::new("a.dwg", incompressible(64)),
            ZipEntry::new("b.png", b"same bytes".to_vec()),
        ];
        let zip = build_zip(&entries).expect("archive builds");
        let (central, _, _, _) = parse_central_directory(&zip);
        // Offsets advance past each header+payload, first entry at zero.
        assert_eq!(central[0].local_offset, 0);
        assert!(central[1].local_offset > central[0].local_offset);
        // Identical input rebuilds byte-identical output (fixed timestamp).
        assert_eq!(build_zip(&entries).unwrap(), zip);
    }

    #[test]
    fn utf8_names_round_trip() {
        let entries = vec![ZipEntry::new("πλάνη.dwg", "γ".repeat(100).into_bytes())];
        let zip = build_zip(&entries).expect("archive builds");
        let (central, _, _, _) = parse_central_directory(&zip);
        assert_eq!(central[0].name, "πλάνη.dwg");
        assert_eq!(read_entry_payload(&zip, &central[0]), entries[0].data);
    }

    #[test]
    fn invalid_names_are_rejected() {
        let long = "x".repeat(u16::MAX as usize + 1);
        for name in ["", ".", "..", "a/b", "a\\b", "a\0b", &long] {
            let err = build_zip(&[ZipEntry::new(name, Vec::new())])
                .expect_err("invalid name must be rejected");
            assert!(err.contains("invalid ZIP entry name"), "{name:?}: {err}");
        }
    }

    #[test]
    fn entry_count_over_the_classic_limit_is_rejected() {
        let entries: Vec<ZipEntry> = (0..=u16::MAX as usize + 1)
            .map(|i| ZipEntry::new(format!("f{i}"), Vec::new()))
            .collect();
        let err = build_zip(&entries).expect_err("too many entries must be rejected");
        assert!(err.contains("too many entries"), "{err}");
    }
}
