//! Georeference of a raster file, from GeoTIFF model tags or a world file
//! beside it (`.tfw`, `.jgw`, `.pgw`, `.tifw`, `.wld` …). Both reduce to the
//! world-file affine `X = a·col + b·row + c`, `Y = d·col + e·row + f`, where
//! `(col, row)` is a pixel centre counted from the upper-left pixel.

use glam::DVec3;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// World-file terms in file order: `[a, d, b, e, c, f]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Georef(pub [f64; 6]);

impl Georef {
    /// Lower-left image corner and the per-pixel u (right) and v (up)
    /// vectors, as a raster image entity stores them.
    pub fn placement(&self, height_px: u32) -> (DVec3, DVec3, DVec3) {
        let [a, d, b, e, c, f] = self.0;
        let row = height_px as f64 - 0.5;
        let corner = DVec3::new(c - 0.5 * a + row * b, f - 0.5 * d + row * e, 0.0);
        (corner, DVec3::new(a, d, 0.0), DVec3::new(-b, -e, 0.0))
    }

    fn valid(self) -> Option<Self> {
        let [a, d, b, e, ..] = self.0;
        (self.0.iter().all(|v| v.is_finite()) && (a * e - b * d).abs() > 0.0).then_some(self)
    }
}

/// The georeference stored in the image itself, else in its world file.
pub fn find(image: &str) -> Option<Georef> {
    let path = Path::new(image);
    geotiff(path)
        .ok()
        .flatten()
        .and_then(Georef::valid)
        .or_else(|| world_file(path))
}

fn world_file(image: &Path) -> Option<Georef> {
    let ext = image.extension()?.to_str()?.to_ascii_lowercase();
    let short = format!("{}{}w", ext.chars().next()?, ext.chars().last()?);
    for name in [short, format!("{ext}w"), "wld".to_string()] {
        for name in [name.clone(), name.to_ascii_uppercase()] {
            let Ok(text) = std::fs::read_to_string(image.with_extension(&name)) else {
                continue;
            };
            let terms: Vec<f64> = text
                .split_whitespace()
                .take(6)
                .map(str::parse)
                .collect::<Result<_, _>>()
                .ok()?;
            return Georef(terms.try_into().ok()?).valid();
        }
    }
    None
}

const MODEL_PIXEL_SCALE: u16 = 33550;
const MODEL_TIEPOINT: u16 = 33922;
const MODEL_TRANSFORMATION: u16 = 34264;
const TYPE_DOUBLE: u16 = 12;

struct TiffReader<R> {
    r: R,
    little_endian: bool,
    big_tiff: bool,
}

impl<R: Read + Seek> TiffReader<R> {
    fn bytes<const N: usize>(&mut self) -> std::io::Result<[u8; N]> {
        let mut b = [0u8; N];
        self.r.read_exact(&mut b)?;
        if !self.little_endian {
            b.reverse();
        }
        Ok(b)
    }
    fn u16(&mut self) -> std::io::Result<u16> {
        Ok(u16::from_le_bytes(self.bytes()?))
    }
    fn u32(&mut self) -> std::io::Result<u32> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> std::io::Result<u64> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    /// An offset or count whose width depends on classic vs BigTIFF.
    fn word(&mut self) -> std::io::Result<u64> {
        if self.big_tiff {
            self.u64()
        } else {
            self.u32().map(u64::from)
        }
    }
}

/// GeoTIFF model tags of the first image directory: a ModelTransformation,
/// or a ModelPixelScale with its first ModelTiepoint. `Ok(None)` for a file
/// that is not a TIFF or carries neither.
// ponytail: tiepoints are read as PixelIsArea; a PixelIsPoint raster
// (GeoKey 1025 = 2) lands half a pixel off until the key directory is read.
fn geotiff(path: &Path) -> std::io::Result<Option<Georef>> {
    let mut r = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut order = [0u8; 2];
    r.read_exact(&mut order)?;
    let little_endian = match &order {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(None),
    };
    let mut t = TiffReader {
        r,
        little_endian,
        big_tiff: false,
    };
    match t.u16()? {
        42 => {}
        43 => {
            t.big_tiff = true;
            t.u32()?; // offset size (8) and a reserved zero
        }
        _ => return Ok(None),
    }
    let ifd = t.word()?;
    t.r.seek(SeekFrom::Start(ifd))?;
    let count = if t.big_tiff {
        t.u64()?
    } else {
        t.u16()? as u64
    };
    let inline = if t.big_tiff { 8 } else { 4 };
    // (tag, value count, position of the value field)
    let mut wanted = Vec::new();
    for _ in 0..count.min(4096) {
        let tag = t.u16()?;
        let kind = t.u16()?;
        let n = t.word()?;
        let at = t.r.stream_position()?;
        if kind == TYPE_DOUBLE
            && matches!(
                tag,
                MODEL_PIXEL_SCALE | MODEL_TIEPOINT | MODEL_TRANSFORMATION
            )
        {
            wanted.push((tag, n, at));
        }
        t.r.seek(SeekFrom::Start(at + inline))?;
    }
    let mut values = |tag: u16| -> std::io::Result<Vec<f64>> {
        let Some(&(_, n, at)) = wanted.iter().find(|w| w.0 == tag) else {
            return Ok(Vec::new());
        };
        t.r.seek(SeekFrom::Start(at))?;
        if n * 8 > inline {
            let offset = t.word()?;
            t.r.seek(SeekFrom::Start(offset))?;
        }
        (0..n.min(16))
            .map(|_| t.u64().map(f64::from_bits))
            .collect()
    };
    let m = values(MODEL_TRANSFORMATION)?;
    if m.len() >= 8 {
        // Raster space here is pixel corners; shift to the first pixel centre.
        let c = m[3] + 0.5 * (m[0] + m[1]);
        let f = m[7] + 0.5 * (m[4] + m[5]);
        return Ok(Some(Georef([m[0], m[4], m[1], m[5], c, f])));
    }
    let scale = values(MODEL_PIXEL_SCALE)?;
    let tie = values(MODEL_TIEPOINT)?;
    if scale.len() < 2 || tie.len() < 6 {
        return Ok(None);
    }
    let (sx, sy) = (scale[0], scale[1]);
    let (i, j, x, y) = (tie[0], tie[1], tie[3], tie[4]);
    let c = x + (0.5 - i) * sx;
    let f = y - (0.5 - j) * sy;
    Ok(Some(Georef([sx, 0.0, 0.0, -sy, c, f])))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world file and the equivalent GeoTIFF tags place the image alike.
    #[test]
    fn world_file_and_geotiff_tags_agree() {
        let dir = std::env::temp_dir().join(format!("ocs_georef_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 2 m pixels, upper-left pixel centre at (500001, 4000099).
        let jpg = dir.join("site.jpg");
        std::fs::write(dir.join("site.jgw"), "2\n0\n0\n-2\n500001\n4000099\n").unwrap();
        let from_wld = find(jpg.to_str().unwrap()).expect("world file");

        // Big-endian TIFF, one IFD: pixel scale (2,2,0) and tiepoint
        // raster (0,0) → model (500000, 4000100).
        let mut tif: Vec<u8> = b"MM\0\x2a\0\0\0\x08".to_vec();
        tif.extend(2u16.to_be_bytes());
        let scale_at = 8 + 2 + 2 * 12 + 4;
        let tie_at = scale_at + 3 * 8;
        for (tag, n, at) in [
            (MODEL_PIXEL_SCALE, 3u32, scale_at),
            (MODEL_TIEPOINT, 6, tie_at),
        ] {
            tif.extend(tag.to_be_bytes());
            tif.extend(TYPE_DOUBLE.to_be_bytes());
            tif.extend(n.to_be_bytes());
            tif.extend((at as u32).to_be_bytes());
        }
        tif.extend(0u32.to_be_bytes());
        for v in [2.0f64, 2.0, 0.0, 0.0, 0.0, 0.0, 500000.0, 4000100.0, 0.0] {
            tif.extend(v.to_bits().to_be_bytes());
        }
        let tif_path = dir.join("site.tif");
        std::fs::write(&tif_path, tif).unwrap();
        let from_tags = find(tif_path.to_str().unwrap()).expect("geotiff tags");
        std::fs::remove_dir_all(&dir).ok();

        assert_eq!(from_wld, from_tags);
        // 50 px tall: lower-left corner 100 m below the upper edge.
        let (corner, u, v) = from_wld.placement(50);
        assert_eq!(corner, DVec3::new(500000.0, 4000000.0, 0.0));
        assert_eq!(
            (u, v),
            (DVec3::new(2.0, 0.0, 0.0), DVec3::new(0.0, 2.0, 0.0))
        );
    }
}
