// Indexed scan (.rcs, "ADOCT" version 3) point reader.
//
// Layout (little-endian):
//   0x10  3 × f64  scan translation T
//   0x28  3 × f64  scan rotation
//   0x40  3 × f64  scan scale
//   0xE0  3 × u8   has RGB, has normals, has intensity
//   0x129 u32      section count, then 24-byte entries at 0x12D:
//                  u64 type, u64 offset, u64 size
// Section 2 is the node directory: u64 node count, then 368-byte records
// (cube min at +0, levels +96, point count +100, block size +104, per-level
// cell counts +108 and point counts +236). Section 1 holds the node blocks
// back to back: the node's octree index (u32 entries), then 16-byte point
// records — a u64 with three 18-bit millimetre offsets from the node cube's
// min corner, the intensity byte, then blue, green, red.
// The octree index is only needed for spatial queries: every record carries
// its own position.

/// One scan, in its own (untransformed) frame.
pub struct Scan {
    /// Point positions before the scan transform.
    pub local: Vec<[f64; 3]>,
    pub colors: Vec<[u8; 3]>,
    pub intensity: Vec<u8>,
    /// Unit normals (scan frame).
    pub normals: Vec<[f32; 3]>,
    pub has_rgb: bool,
    pub has_normals: bool,
    pub has_intensity: bool,
    pub translation: [f64; 3],
    pub rotation: [f64; 3],
    pub scale: [f64; 3],
    /// The scan's bounds as its header records them (local frame).
    pub bounds: [[f64; 3]; 2],
    /// The scan's identifier ("{…}"), as its project and a cloud's hidden
    /// scans name it.
    pub id: String,
}

const NODE_RECORD: usize = 368;

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn f64_at(b: &[u8], at: usize) -> Option<f64> {
    u64_at(b, at).map(f64::from_bits).filter(|v| v.is_finite())
}

fn vec3_at(b: &[u8], at: usize) -> Option<[f64; 3]> {
    Some([f64_at(b, at)?, f64_at(b, at + 8)?, f64_at(b, at + 16)?])
}

/// A normal from its cube-map code: six faces of 52 × 52 equal-angle
/// cells. The face gives the main axis and its sign; the cell the other two
/// components, which flip with the face.
fn normal(code: u16) -> [f32; 3] {
    let code = usize::from(code);
    if code >= 6 * 52 * 52 {
        return [0.0, 0.0, 1.0];
    }
    let (face, cell) = (code / 2704, code % 2704);
    let tan = |i: usize| (((i as f64 + 0.5) / 26.0 - 1.0) * std::f64::consts::FRAC_PI_4).tan();
    let axis = face >> 1;
    let (b, c) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let sign = if face % 2 == 0 { 1.0 } else { -1.0 };
    let mut n = [0.0f64; 3];
    n[axis] = sign;
    n[b] = sign * tan(cell % 52);
    n[c] = sign * tan(cell / 52);
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    n.map(|v| (v / length) as f32)
}

/// The identifier the header carries after its flags, NUL-terminated.
fn header_id(b: &[u8]) -> String {
    let tail = b.get(0xE3..b.len().min(0xE3 + 64)).unwrap_or_default();
    let text = &tail[..tail.iter().position(|c| *c == 0).unwrap_or(tail.len())];
    match std::str::from_utf8(text) {
        Ok(id) if id.starts_with('{') && id.ends_with('}') => id.to_string(),
        _ => String::new(),
    }
}

pub fn decode(b: &[u8]) -> Option<Scan> {
    if b.get(0..5)? != b"ADOCT" {
        return None;
    }
    let translation = vec3_at(b, 0x10)?;
    let rotation = vec3_at(b, 0x28)?;
    let scale = vec3_at(b, 0x40)?;
    let has_rgb = *b.get(0xE0)? != 0;
    let bounds = [vec3_at(b, 0xA0)?, vec3_at(b, 0xB8)?];
    let has_normals = *b.get(0xE1)? != 0;
    let has_intensity = *b.get(0xE2)? != 0;

    let sections = u32_at(b, 0x129)? as usize;
    let (mut directory, mut data) = (None, None);
    for k in 0..sections.min(64) {
        let at = 0x12D + k * 24;
        let offset = usize::try_from(u64_at(b, at + 8)?).ok()?;
        match u64_at(b, at)? {
            1 => data = Some(offset),
            2 => directory = Some(offset),
            _ => {}
        }
    }
    let (directory, mut block) = (directory?, data?);
    let nodes = usize::try_from(u64_at(b, directory)?).ok()?;
    // Every node needs its directory record; a count past the file is corrupt.
    if nodes.checked_mul(NODE_RECORD)? > b.len() {
        return None;
    }

    let mut scan = Scan {
        local: Vec::new(),
        colors: Vec::new(),
        intensity: Vec::new(),
        normals: Vec::new(),
        has_rgb,
        has_normals,
        has_intensity,
        translation,
        rotation,
        scale,
        bounds,
        id: header_id(b),
    };
    for node in 0..nodes {
        let record = directory + 8 + node * NODE_RECORD;
        let cube_min = vec3_at(b, record)?;
        let points = u32_at(b, record + 100)? as usize;
        let size = u32_at(b, record + 104)? as usize;
        let mut entries = points;
        for level in 0..32 {
            entries = entries.checked_add(u32_at(b, record + 108 + level * 4)? as usize)?;
        }
        if entries.checked_mul(4)?.checked_add(points.checked_mul(16)?)? != size {
            return None;
        }
        let records = block + entries * 4;
        let end = block.checked_add(size)?;
        let bytes = b.get(records..end)?;
        scan.local.reserve(points);
        scan.colors.reserve(points);
        scan.intensity.reserve(points);
        for rec in bytes.chunks_exact(16) {
            let v = u64::from_le_bytes(rec[0..8].try_into().ok()?);
            let q = |shift: u32| ((v >> shift) & 0x3FFFF) as f64 * 1e-3;
            scan.local.push([cube_min[0] + q(0), cube_min[1] + q(18), cube_min[2] + q(36)]);
            scan.intensity.push(rec[8]);
            scan.colors.push([rec[11], rec[10], rec[9]]);
            scan.normals.push(normal(u16::from_le_bytes([rec[12], rec[13]])));
        }
        block = end;
    }
    Some(scan)
}
