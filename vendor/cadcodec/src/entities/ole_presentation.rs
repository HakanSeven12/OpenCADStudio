// OLE2FRAME presentation extraction.
//
// The entity's binary data is a small frame header followed by an OLE
// compound file (CFB). The picture lives in one of its streams:
//   - `\x01Ole10Native` / `CONTENTS` / `Package` — the native object data;
//     for static / Paint-style objects a raster (BMP/PNG/JPEG/GIF) or a
//     metafile.
//   - `\x02OlePres000..2` — the cached presentation: an MS-OLEDS
//     clipboard-format header (CF_ENHMETAFILE / CF_METAFILEPICT / CF_DIB)
//     followed by the picture bytes. Office writes CF_ENHMETAFILE bodies as a
//     memory WMF whose META_ESCAPE "WMFC" comment records carry the original
//     EMF in chunks; those are reassembled here.
//
// Streams sit in FAT sector chains, so the payload is NOT contiguous in the
// blob — extracting anything from it requires walking the compound file.
// This module only *classifies and extracts* bytes; rasterizing them is the
// consumer's concern (it needs image codecs and fonts a file-format library
// shouldn't carry).

/// A picture extracted from an OLE2FRAME data blob, tagged with how the
/// bytes should be interpreted.
#[derive(Clone, Debug, PartialEq)]
pub enum OlePresentation {
    /// A complete raster file: BMP ("BM" + BITMAPFILEHEADER), PNG, JPEG or
    /// GIF — feed to any image decoder.
    Raster(Vec<u8>),
    /// A headerless DIB (BITMAPINFO + bits), as CF_DIB stores it.
    Dib(Vec<u8>),
    /// An enhanced metafile (EMF) record stream.
    Emf(Vec<u8>),
    /// A Windows metafile: placeable (0x9AC6CDD7 header) or standard.
    Wmf(Vec<u8>),
}

impl crate::entities::Ole2Frame {
    /// Extract the embedded picture from this frame's structured storage.
    pub fn presentation(&self) -> Option<OlePresentation> {
        extract_presentation(&self.encoded_payload())
    }
}

/// Extract the picture from a raw OLE2FRAME data blob (frame header + CFB).
pub fn extract_presentation(data: &[u8]) -> Option<OlePresentation> {
    if let Some(cfb) = find_cfb(data).and_then(Cfb::parse) {
        // Native raster first — it's the full-quality object; the
        // presentation stream is only a preview of it.
        for name in ["\u{1}Ole10Native", "CONTENTS", "Package"] {
            if let Some(stream) = cfb.stream(name) {
                let body = native_body(name, &stream);
                if is_raster(body) {
                    return Some(OlePresentation::Raster(body.to_vec()));
                }
            }
        }
        // Cached presentation streams.
        for name in ["\u{2}OlePres000", "\u{2}OlePres001", "\u{2}OlePres002"] {
            if let Some(stream) = cfb.stream(name) {
                if let Some(p) = decode_olepres(&stream) {
                    return Some(p);
                }
            }
        }
        // Native metafile content (static metafile objects).
        for name in ["CONTENTS", "\u{1}Ole10Native"] {
            if let Some(stream) = cfb.stream(name) {
                if let Some(p) = classify_metafile(native_body(name, &stream)) {
                    return Some(p);
                }
            }
        }
    }
    // Legacy fallback: scan the raw blob for a self-consistent BMP file.
    // Catches R14-era blobs that store the DIB outside a compound file.
    scan_bmp(data).map(OlePresentation::Raster)
}

/// Strip the `\x01Ole10Native` u32 length prefix; other streams pass through.
fn native_body<'a>(name: &str, stream: &'a [u8]) -> &'a [u8] {
    if name == "\u{1}Ole10Native" && stream.len() > 4 {
        &stream[4..]
    } else {
        stream
    }
}

/// Locate the CFB signature within the blob (it sits after the OLE frame
/// header, whose exact size varies by writer).
fn find_cfb(data: &[u8]) -> Option<&[u8]> {
    const MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    let pos = data.windows(8).take(4096).position(|w| w == MAGIC)?;
    Some(&data[pos..])
}

/// Does the body start like a self-contained raster file?
fn is_raster(body: &[u8]) -> bool {
    body.len() >= 8
        && (body.starts_with(b"BM")
            || body.starts_with(&[0x89, b'P', b'N', b'G'])
            || body.starts_with(&[0xFF, 0xD8, 0xFF])
            || body.starts_with(b"GIF8"))
}

/// Classify a metafile body. A memory WMF that is only a "WMFC" wrapper
/// around an embedded EMF yields the reassembled EMF instead.
fn classify_metafile(body: &[u8]) -> Option<OlePresentation> {
    if body.len() < 22 {
        return None;
    }
    let key = u32::from_le_bytes(body[0..4].try_into().unwrap());
    if key == 1 && body.len() >= 44 && &body[40..44] == b" EMF" {
        return Some(OlePresentation::Emf(body.to_vec()));
    }
    let is_placeable = key == 0x9AC6_CDD7;
    let (ty, hs) = if is_placeable && body.len() >= 26 {
        (
            u16::from_le_bytes(body[22..24].try_into().unwrap()),
            u16::from_le_bytes(body[24..26].try_into().unwrap()),
        )
    } else {
        (
            u16::from_le_bytes(body[0..2].try_into().unwrap()),
            u16::from_le_bytes(body[2..4].try_into().unwrap()),
        )
    };
    if (ty == 1 || ty == 2) && hs == 9 {
        if let Some(emf) = extract_embedded_emf(body) {
            return Some(OlePresentation::Emf(emf));
        }
        return Some(OlePresentation::Wmf(body.to_vec()));
    }
    None
}

/// Decode an OlePres stream (MS-OLEDS): clipboard-format header, target
/// device, aspect/extents, then the picture bytes.
fn decode_olepres(d: &[u8]) -> Option<OlePresentation> {
    if d.len() < 24 {
        return None;
    }
    let marker = u32::from_le_bytes(d[0..4].try_into().unwrap());
    let mut off = 4usize;
    let format = if marker == 0xFFFF_FFFF || marker == 0xFFFF_FFFE {
        let f = u32::from_le_bytes(d.get(off..off + 4)?.try_into().unwrap());
        off += 4;
        f
    } else if marker == 0 {
        0 // no format — sniff the body below
    } else {
        // Length-prefixed ANSI clipboard-format name; skip it.
        off = off.checked_add(marker as usize)?;
        0
    };
    // TargetDeviceSize: 4 means "no target device"; larger values embed one.
    let tds = u32::from_le_bytes(d.get(off..off + 4)?.try_into().unwrap()) as usize;
    off += 4;
    if tds > 4 {
        off = off.checked_add(tds - 4)?;
    }
    // Aspect, lindex, advf, reserved1, width, height (HIMETRIC), size.
    off = off.checked_add(24)?;
    let size = u32::from_le_bytes(d.get(off..off + 4)?.try_into().unwrap()) as usize;
    off += 4;
    let body = d.get(off..off.checked_add(size)?.min(d.len()))?;
    if body.is_empty() {
        return None;
    }
    match format {
        // CF_ENHMETAFILE — a raw EMF, or a WMF "WMFC" wrapper around one.
        14 => classify_metafile(body),
        // CF_METAFILEPICT — an 8-byte METAFILEPICT header, then the WMF.
        3 => {
            let wmf = if body.len() > 12 {
                let ty = u16::from_le_bytes(body[8..10].try_into().unwrap());
                let hs = u16::from_le_bytes(body[10..12].try_into().unwrap());
                if (ty == 1 || ty == 2) && hs == 9 {
                    &body[8..]
                } else {
                    body
                }
            } else {
                body
            };
            classify_metafile(wmf)
        }
        // CF_DIB — headerless BITMAPINFO + bits.
        8 => Some(OlePresentation::Dib(body.to_vec())),
        _ => {
            if is_raster(body) {
                Some(OlePresentation::Raster(body.to_vec()))
            } else {
                classify_metafile(body)
            }
        }
    }
}

/// Scan a blob for a plausible embedded BMP file and return its bytes.
fn scan_bmp(data: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0usize;
    while i + 54 <= data.len() {
        if &data[i..i + 2] != b"BM" {
            i += 1;
            continue;
        }
        let file_size = u32::from_le_bytes(data[i + 2..i + 6].try_into().unwrap()) as usize;
        let dib_size = u32::from_le_bytes(data[i + 14..i + 18].try_into().unwrap());
        // A plausible BITMAPFILEHEADER points at a known DIB-header size and
        // a sane pixel-data offset within the remaining bytes.
        let px_off = u32::from_le_bytes(data[i + 10..i + 14].try_into().unwrap()) as usize;
        if matches!(dib_size, 12 | 40 | 52 | 56 | 64 | 108 | 124)
            && px_off >= 26
            && px_off < data.len() - i
            && file_size >= 54
        {
            let end = if i + file_size <= data.len() {
                i + file_size
            } else {
                data.len()
            };
            return Some(data[i..end].to_vec());
        }
        i += 1;
    }
    None
}

// ── WMFC → EMF reassembly ───────────────────────────────────────────────────

/// Iterate WMF records as `(function, params)` slices. `off` must point at
/// the first record (past the 18-byte standard header).
struct WmfRecords<'a> {
    d: &'a [u8],
    off: usize,
}

impl<'a> Iterator for WmfRecords<'a> {
    type Item = (u16, &'a [u8]);
    fn next(&mut self) -> Option<(u16, &'a [u8])> {
        if self.off + 6 > self.d.len() {
            return None;
        }
        let size_w =
            u32::from_le_bytes(self.d[self.off..self.off + 4].try_into().unwrap()) as usize;
        let func = u16::from_le_bytes(self.d[self.off + 4..self.off + 6].try_into().unwrap());
        let size_b = size_w.checked_mul(2)?;
        if size_b < 6 || self.off + size_b > self.d.len() {
            return None;
        }
        let params = &self.d[self.off + 6..self.off + size_b];
        self.off += size_b;
        if func == 0 {
            return None; // META_EOF
        }
        Some((func, params))
    }
}

/// Reassemble an embedded EMF from META_ESCAPE_ENHANCED_METAFILE comment
/// records ("WMFC"). Returns the EMF byte stream if the wrapper carries one.
fn extract_embedded_emf(data: &[u8]) -> Option<Vec<u8>> {
    // Skip a placeable header if present.
    let data =
        if data.len() >= 22 && u32::from_le_bytes(data[0..4].try_into().unwrap()) == 0x9AC6_CDD7 {
            &data[22..]
        } else {
            data
        };
    if data.len() < 18 {
        return None;
    }
    let hs = u16::from_le_bytes(data[2..4].try_into().unwrap()) as usize;
    if hs != 9 {
        return None;
    }
    let mut emf: Vec<u8> = Vec::new();
    for (func, params) in (WmfRecords {
        d: data,
        off: hs * 2,
    }) {
        if func != 0x0626 || params.len() < 38 {
            continue;
        }
        // params: EscapeFunction u16, ByteCount u16, then the comment payload
        // (CommentIdentifier "WMFC", type, version, checksum, flags, record
        // count, CurrentRecordSize @26, remaining, total; data at 38).
        if u16::from_le_bytes(params[0..2].try_into().unwrap()) != 0x000F
            || u32::from_le_bytes(params[4..8].try_into().unwrap()) != 0x4346_4D57
        {
            continue;
        }
        let cur_size = u32::from_le_bytes(params[26..30].try_into().unwrap()) as usize;
        let end = 38usize.checked_add(cur_size)?;
        if end > params.len() {
            continue;
        }
        emf.extend_from_slice(&params[38..end]);
    }
    if emf.len() >= 88 && &emf[40..44] == b" EMF" {
        Some(emf)
    } else {
        None
    }
}

// ── Compound file (CFB) reader ──────────────────────────────────────────────

const END_OF_CHAIN: u32 = 0xFFFF_FFFE;
const FREE_SECT: u32 = 0xFFFF_FFFF;

struct DirEntry {
    name: String,
    kind: u8,
    start: u32,
    size: u64,
}

struct Cfb<'a> {
    data: &'a [u8],
    sector_size: usize,
    mini_size: usize,
    mini_cutoff: u64,
    fat: Vec<u32>,
    minifat: Vec<u32>,
    ministream: Vec<u8>,
    dir: Vec<DirEntry>,
}

impl<'a> Cfb<'a> {
    fn parse(data: &'a [u8]) -> Option<Cfb<'a>> {
        if data.len() < 512 {
            return None;
        }
        let u16f = |o: usize| u16::from_le_bytes(data[o..o + 2].try_into().unwrap());
        let u32f = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        let sector_shift = u16f(30);
        let mini_shift = u16f(32);
        if !(7..=15).contains(&sector_shift) || mini_shift as usize >= sector_shift as usize {
            return None;
        }
        let sector_size = 1usize << sector_shift;
        let mini_size = 1usize << mini_shift;
        let dir_start = u32f(48);
        let mini_cutoff = u32f(56) as u64;
        let minifat_start = u32f(60);
        let num_minifat = u32f(64) as usize;
        let difat_start = u32f(68);
        let num_difat = u32f(72) as usize;

        // DIFAT: 109 header entries plus chained DIFAT sectors.
        let mut difat: Vec<u32> = (0..109).map(|i| u32f(76 + i * 4)).collect();
        let sector = |n: u32| -> Option<&'a [u8]> {
            let off = 512usize.checked_add((n as usize).checked_mul(sector_size)?)?;
            // A truncated blob may cut the final sectors; treat a partial
            // sector as absent.
            data.get(off..off + sector_size)
        };
        let mut s = difat_start;
        for _ in 0..num_difat.min(4096) {
            if s == END_OF_CHAIN || s == FREE_SECT {
                break;
            }
            let Some(sec) = sector(s) else { break };
            let n = sector_size / 4;
            for i in 0..n - 1 {
                difat.push(u32::from_le_bytes(
                    sec[i * 4..i * 4 + 4].try_into().unwrap(),
                ));
            }
            s = u32::from_le_bytes(sec[(n - 1) * 4..n * 4].try_into().unwrap());
        }

        // FAT.
        let mut fat: Vec<u32> = Vec::new();
        for &fs in &difat {
            if fs == END_OF_CHAIN || fs == FREE_SECT {
                continue;
            }
            let Some(sec) = sector(fs) else { continue };
            for c in sec.chunks_exact(4) {
                fat.push(u32::from_le_bytes(c.try_into().unwrap()));
            }
        }
        if fat.is_empty() {
            return None;
        }

        let chain = |start: u32| -> Vec<u8> {
            let mut out = Vec::new();
            let mut n = start;
            let mut seen = 0usize;
            while n != END_OF_CHAIN && n != FREE_SECT && (n as usize) < fat.len() {
                seen += 1;
                if seen > fat.len() + 1 {
                    break; // cycle guard
                }
                match sector(n) {
                    Some(sec) => out.extend_from_slice(sec),
                    None => break,
                }
                n = fat[n as usize];
            }
            out
        };

        // Directory.
        let dirdata = chain(dir_start);
        let mut dir = Vec::new();
        for e in dirdata.chunks_exact(128) {
            let name_len = u16::from_le_bytes(e[64..66].try_into().unwrap()) as usize;
            if name_len < 2 || name_len > 64 {
                continue;
            }
            let units: Vec<u16> = e[..name_len - 2]
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes(c.try_into().unwrap()))
                .collect();
            dir.push(DirEntry {
                name: String::from_utf16_lossy(&units),
                kind: e[66],
                start: u32::from_le_bytes(e[116..120].try_into().unwrap()),
                size: u32::from_le_bytes(e[120..124].try_into().unwrap()) as u64,
            });
        }

        // Mini FAT + mini stream (the root storage's chain).
        let mut minifat: Vec<u32> = Vec::new();
        let mut n = minifat_start;
        let mut count = 0usize;
        while n != END_OF_CHAIN && n != FREE_SECT && count < num_minifat.max(1) {
            let Some(sec) = sector(n) else { break };
            for c in sec.chunks_exact(4) {
                minifat.push(u32::from_le_bytes(c.try_into().unwrap()));
            }
            n = *fat.get(n as usize).unwrap_or(&END_OF_CHAIN);
            count += 1;
        }
        let ministream = dir
            .iter()
            .find(|e| e.kind == 5)
            .map(|root| chain(root.start))
            .unwrap_or_default();

        Some(Cfb {
            data,
            sector_size,
            mini_size,
            mini_cutoff,
            fat,
            minifat,
            ministream,
            dir,
        })
    }

    fn stream(&self, name: &str) -> Option<Vec<u8>> {
        let e = self.dir.iter().find(|e| e.kind == 2 && e.name == name)?;
        let size = e.size as usize;
        let mut out = if size < self.mini_cutoff as usize {
            let mut out = Vec::with_capacity(size);
            let mut n = e.start;
            let mut seen = 0usize;
            while n != END_OF_CHAIN && n != FREE_SECT && (n as usize) < self.minifat.len() {
                seen += 1;
                if seen > self.minifat.len() + 1 {
                    break;
                }
                let off = n as usize * self.mini_size;
                match self.ministream.get(off..off + self.mini_size) {
                    Some(chunk) => out.extend_from_slice(chunk),
                    None => break,
                }
                n = self.minifat[n as usize];
            }
            out
        } else {
            let mut out = Vec::with_capacity(size);
            let mut n = e.start;
            let mut seen = 0usize;
            while n != END_OF_CHAIN && n != FREE_SECT && (n as usize) < self.fat.len() {
                seen += 1;
                if seen > self.fat.len() + 1 {
                    break;
                }
                let off = 512 + n as usize * self.sector_size;
                match self.data.get(off..off + self.sector_size) {
                    Some(sec) => out.extend_from_slice(sec),
                    None => break,
                }
                n = self.fat[n as usize];
            }
            out
        };
        if out.is_empty() {
            return None;
        }
        out.truncate(size);
        Some(out)
    }
}
