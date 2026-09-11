//! ASCII STL parser.
//!
//! ASCII STL format:
//! ```text
//! solid name
//!   facet normal ni nj nk
//!     outer loop
//!       vertex v1x v1y v1z
//!       vertex v2x v2y v2z
//!       vertex v3x v3y v3z
//!     endloop
//!   endfacet
//! endsolid name
//! ```

use std::io::{BufRead, BufReader, Read};

use crate::error::{DxfError, Result};

use super::{StlMesh, StlTriangle};

/// Parse an ASCII STL stream.
pub fn parse_ascii_stl<R: Read>(reader: R) -> Result<StlMesh> {
    let buf = BufReader::new(reader);
    let mut lines = buf.lines();

    // First line: "solid [name]"
    let first_line = lines
        .next()
        .ok_or_else(|| DxfError::ImportError("Empty STL file".into()))?
        .map_err(|e| DxfError::ImportError(format!("Failed to read STL: {}", e)))?;

    let name = first_line
        .trim()
        .strip_prefix("solid")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let name = if name.is_empty() {
        "stl_import".to_string()
    } else {
        name
    };

    let mut triangles = Vec::new();

    // Collect remaining lines, trimmed and lowercased for keyword matching
    let mut remaining: Vec<String> = Vec::new();
    for line in lines {
        let line =
            line.map_err(|e| DxfError::ImportError(format!("Failed to read STL line: {}", e)))?;
        let trimmed = line.trim().to_string();
        if !trimmed.is_empty() {
            remaining.push(trimmed);
        }
    }

    let mut i = 0;
    while i < remaining.len() {
        let lower = remaining[i].to_ascii_lowercase();

        if lower.starts_with("endsolid") {
            break;
        }

        if lower.starts_with("facet normal") {
            let normal = parse_normal(&remaining[i])?;

            // Expect "outer loop"
            i += 1;
            if i >= remaining.len() || !remaining[i].to_ascii_lowercase().starts_with("outer loop")
            {
                return Err(DxfError::ImportError(
                    "Expected 'outer loop' after 'facet normal'".into(),
                ));
            }

            // Read 3 vertices
            let mut vertices = [[0.0f64; 3]; 3];
            for v in &mut vertices {
                i += 1;
                if i >= remaining.len() {
                    return Err(DxfError::ImportError(
                        "Unexpected end of file while reading vertices".into(),
                    ));
                }
                *v = parse_vertex(&remaining[i])?;
            }

            // Expect "endloop"
            i += 1;
            if i >= remaining.len() || !remaining[i].to_ascii_lowercase().starts_with("endloop") {
                return Err(DxfError::ImportError("Expected 'endloop'".into()));
            }

            // Expect "endfacet"
            i += 1;
            if i >= remaining.len() || !remaining[i].to_ascii_lowercase().starts_with("endfacet") {
                return Err(DxfError::ImportError("Expected 'endfacet'".into()));
            }

            triangles.push(StlTriangle {
                normal,
                vertices,
                color: None,
            });
        }

        i += 1;
    }

    Ok(StlMesh { name, triangles })
}

fn parse_normal(line: &str) -> Result<[f64; 3]> {
    // "facet normal nx ny nz"
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 5 {
        return Err(DxfError::ImportError(format!(
            "Invalid facet normal line: '{}'",
            line
        )));
    }
    Ok([
        parse_f64(parts[2], line)?,
        parse_f64(parts[3], line)?,
        parse_f64(parts[4], line)?,
    ])
}

fn parse_vertex(line: &str) -> Result<[f64; 3]> {
    // "vertex x y z"
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 4 {
        return Err(DxfError::ImportError(format!(
            "Invalid vertex line: '{}'",
            line
        )));
    }
    if !parts[0].eq_ignore_ascii_case("vertex") {
        return Err(DxfError::ImportError(format!(
            "Expected 'vertex', got: '{}'",
            parts[0]
        )));
    }
    Ok([
        parse_f64(parts[1], line)?,
        parse_f64(parts[2], line)?,
        parse_f64(parts[3], line)?,
    ])
}

fn parse_f64(s: &str, context: &str) -> Result<f64> {
    s.parse::<f64>()
        .map_err(|_| DxfError::ImportError(format!("Invalid float '{}' in line: '{}'", s, context)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const SAMPLE_ASCII_STL: &str = r#"solid test
  facet normal 0 0 1
    outer loop
      vertex 0 0 0
      vertex 1 0 0
      vertex 0 1 0
    endloop
  endfacet
  facet normal 0 0 1
    outer loop
      vertex 1 0 0
      vertex 1 1 0
      vertex 0 1 0
    endloop
  endfacet
endsolid test
"#;

    #[test]
    fn test_parse_ascii_two_triangles() {
        let cursor = Cursor::new(SAMPLE_ASCII_STL.as_bytes());
        let mesh = parse_ascii_stl(cursor).unwrap();
        assert_eq!(mesh.name, "test");
        assert_eq!(mesh.triangles.len(), 2);
        assert_eq!(mesh.triangles[0].vertices[0], [0.0, 0.0, 0.0]);
        assert_eq!(mesh.triangles[0].vertices[2], [0.0, 1.0, 0.0]);
    }

    #[test]
    fn test_parse_ascii_empty_solid() {
        let stl = "solid empty\nendsolid empty\n";
        let cursor = Cursor::new(stl.as_bytes());
        let mesh = parse_ascii_stl(cursor).unwrap();
        assert_eq!(mesh.name, "empty");
        assert_eq!(mesh.triangles.len(), 0);
    }

    #[test]
    fn test_parse_ascii_scientific_notation() {
        let stl = "solid sci\n\
            facet normal 0.0e+00 0.0e+00 1.0e+00\n\
            outer loop\n\
            vertex 1.5e+01 2.3e-02 0.0e+00\n\
            vertex 1.0e+00 0.0e+00 0.0e+00\n\
            vertex 0.0e+00 1.0e+00 0.0e+00\n\
            endloop\n\
            endfacet\n\
            endsolid sci\n";
        let cursor = Cursor::new(stl.as_bytes());
        let mesh = parse_ascii_stl(cursor).unwrap();
        assert_eq!(mesh.triangles.len(), 1);
        assert!((mesh.triangles[0].vertices[0][0] - 15.0).abs() < 1e-10);
        assert!((mesh.triangles[0].vertices[0][1] - 0.023).abs() < 1e-10);
    }
}
