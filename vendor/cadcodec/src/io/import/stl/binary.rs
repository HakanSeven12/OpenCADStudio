//! Binary STL parser.
//!
//! Binary STL format:
//! - 80 bytes: header (ignored, must not start with "solid")
//! - 4 bytes:  number of triangles (u32 LE)
//! - Per triangle (50 bytes each):
//!   - 12 bytes: normal vector (3 × f32 LE)
//!   - 36 bytes: 3 vertices (3 × 3 × f32 LE)
//!   - 2 bytes:  attribute byte count (u16 LE, usually 0)

use std::io::{Read, Seek, SeekFrom};

use byteorder::{LittleEndian, ReadBytesExt};

use crate::error::{DxfError, Result};

use super::{StlMesh, StlTriangle};

/// Parse a binary STL stream.
///
/// The reader must be positioned at the start of the file.
pub fn parse_binary_stl<R: Read + Seek>(reader: &mut R) -> Result<StlMesh> {
    // Read 80-byte header
    let mut header = [0u8; 80];
    reader
        .read_exact(&mut header)
        .map_err(|e| DxfError::ImportError(format!("Failed to read STL header: {}", e)))?;

    // Extract a name from the header (everything up to first NUL or non-printable)
    let name = extract_header_name(&header);

    // Read triangle count
    let num_triangles = reader
        .read_u32::<LittleEndian>()
        .map_err(|e| DxfError::ImportError(format!("Failed to read triangle count: {}", e)))?
        as usize;

    // Validate: file size should be 84 + 50 * num_triangles
    if let Ok(file_len) = reader.seek(SeekFrom::End(0)) {
        let expected = 84 + 50 * num_triangles as u64;
        if file_len < expected {
            return Err(DxfError::ImportError(format!(
                "Binary STL file too small: expected at least {} bytes for {} triangles, got {}",
                expected, num_triangles, file_len
            )));
        }
        reader
            .seek(SeekFrom::Start(84))
            .map_err(|e| DxfError::ImportError(format!("Failed to seek past header: {}", e)))?;
    }

    let mut triangles = Vec::with_capacity(num_triangles);

    for i in 0..num_triangles {
        let tri = read_triangle(reader)
            .map_err(|e| DxfError::ImportError(format!("Failed to read triangle {}: {}", i, e)))?;
        triangles.push(tri);
    }

    Ok(StlMesh { name, triangles })
}

fn read_triangle<R: Read>(reader: &mut R) -> Result<StlTriangle> {
    let normal = [
        reader.read_f32::<LittleEndian>().map_err(io_err)? as f64,
        reader.read_f32::<LittleEndian>().map_err(io_err)? as f64,
        reader.read_f32::<LittleEndian>().map_err(io_err)? as f64,
    ];

    let mut vertices = [[0.0f64; 3]; 3];
    for v in &mut vertices {
        v[0] = reader.read_f32::<LittleEndian>().map_err(io_err)? as f64;
        v[1] = reader.read_f32::<LittleEndian>().map_err(io_err)? as f64;
        v[2] = reader.read_f32::<LittleEndian>().map_err(io_err)? as f64;
    }

    let attribute = reader.read_u16::<LittleEndian>().map_err(io_err)?;

    // Some STL writers encode colour in the attribute field (VisCAM / SolidView).
    // Bit 15 set → bits 0-4 = blue, 5-9 = green, 10-14 = red (5-bit each).
    let color = if attribute & 0x8000 != 0 {
        let b = ((attribute & 0x001F) as f32 / 31.0 * 255.0) as u8;
        let g = (((attribute >> 5) & 0x001F) as f32 / 31.0 * 255.0) as u8;
        let r = (((attribute >> 10) & 0x001F) as f32 / 31.0 * 255.0) as u8;
        Some((r, g, b))
    } else {
        None
    };

    Ok(StlTriangle {
        normal,
        vertices,
        color,
    })
}

fn io_err(e: std::io::Error) -> DxfError {
    DxfError::ImportError(format!("STL binary read error: {}", e))
}

fn extract_header_name(header: &[u8; 80]) -> String {
    let end = header
        .iter()
        .position(|&b| b == 0 || b < 0x20)
        .unwrap_or(80);
    let s = String::from_utf8_lossy(&header[..end]);
    let trimmed = s.trim();
    if trimmed.is_empty() {
        "stl_import".to_string()
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Creates a minimal binary STL with one triangle.
    fn make_binary_stl(triangles: &[StlTriangle]) -> Vec<u8> {
        let mut buf = Vec::new();
        // 80-byte header
        let header = b"test binary stl";
        buf.extend_from_slice(header);
        buf.extend_from_slice(&vec![0u8; 80 - header.len()]);
        // triangle count
        buf.extend_from_slice(&(triangles.len() as u32).to_le_bytes());
        for tri in triangles {
            for &n in &tri.normal {
                buf.extend_from_slice(&(n as f32).to_le_bytes());
            }
            for v in &tri.vertices {
                for &c in v {
                    buf.extend_from_slice(&(c as f32).to_le_bytes());
                }
            }
            buf.extend_from_slice(&0u16.to_le_bytes());
        }
        buf
    }

    #[test]
    fn test_parse_single_triangle() {
        let tri = StlTriangle {
            normal: [0.0, 0.0, 1.0],
            vertices: [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            color: None,
        };
        let data = make_binary_stl(&[tri]);
        let mut cursor = Cursor::new(data);
        let mesh = parse_binary_stl(&mut cursor).unwrap();
        assert_eq!(mesh.triangles.len(), 1);
        assert_eq!(mesh.triangles[0].vertices[1], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn test_parse_empty() {
        let data = make_binary_stl(&[]);
        let mut cursor = Cursor::new(data);
        let mesh = parse_binary_stl(&mut cursor).unwrap();
        assert_eq!(mesh.triangles.len(), 0);
    }

    #[test]
    fn test_color_attribute() {
        let mut data = Vec::new();
        // header
        data.extend_from_slice(&[0u8; 80]);
        // 1 triangle
        data.extend_from_slice(&1u32.to_le_bytes());
        // normal
        for _ in 0..3 {
            data.extend_from_slice(&0.0f32.to_le_bytes());
        }
        // vertices
        for _ in 0..9 {
            data.extend_from_slice(&1.0f32.to_le_bytes());
        }
        // attribute with colour: bit 15 set, R=31, G=0, B=0
        let attr: u16 = 0x8000 | (31 << 10);
        data.extend_from_slice(&attr.to_le_bytes());

        let mut cursor = Cursor::new(data);
        let mesh = parse_binary_stl(&mut cursor).unwrap();
        assert_eq!(mesh.triangles[0].color, Some((255, 0, 0)));
    }
}
