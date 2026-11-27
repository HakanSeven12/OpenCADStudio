//! The lines, arrows and fills a dimension or leader draws, built before any
//! of it is tessellated for the screen.

use glam::Vec3;

pub(crate) struct DimGeom {
    /// Boundary between the angular extension lines, including when either is suppressed.
    pub(crate) ext2_start: Option<usize>,
    pub(crate) ext_lines: Vec<[f32; 3]>,
    pub(crate) dim_lines: Vec<[f32; 3]>,
    /// Arrowhead / tick outlines of a dimension: drawn solid, never in the
    /// dimension line's dashes.
    pub(crate) arrow_lines: Vec<[f32; 3]>,
    pub(crate) arrow_fill: Vec<[f32; 3]>,
    /// World-space point that `ext_lines` / `dim_lines` / `arrow_lines` /
    /// `arrow_fill` were built relative to. Dimension tessellation shifts the
    /// definition data to a nearby origin so its f32 geometry keeps
    /// sub-millimetre precision at UTM scale; consumers that need world-space
    /// points (explode) add this back. Zero for leader and multileader
    /// arrowheads, whose points are already world-space f32.
    pub(crate) origin: document::types::Vector3,
}

#[derive(Clone)]
pub(crate) enum ArrowKind {
    None,
    Triangle {
        size: f32,
        filled: bool,
        size_mul: f32,
    },
    /// `width` is the stroke's full width: 0 draws a hairline (Oblique,
    /// DIMTSZ), more draws a filled band (Architectural tick).
    Tick {
        size: f32,
        width: f32,
    },
    Open {
        size: f32,
        half_angle: f32,
    },
    Dot {
        size: f32,
        filled: bool,
    },
    /// "Origin indicator": an open circle of diameter `size` around the tip
    /// and a stroke from the tip back along the leader. "Origin indicator 2"
    /// (`inner`) adds a circle half that size, and its stroke starts at the
    /// outer circle.
    Origin {
        size: f32,
        inner: bool,
    },
    Box_ {
        size: f32,
        filled: bool,
    },
    Datum {
        size: f32,
        filled: bool,
    },
    Custom {
        size: f32,
        lines: Vec<[f32; 3]>,
        fill: Vec<[f32; 3]>,
    },
}

pub(crate) fn add_polyline(points: &mut Vec<[f32; 3]>, polyline: &[Vec3]) {
    if polyline.len() < 2 {
        return;
    }
    if !points.is_empty() {
        points.push([f32::NAN, f32::NAN, f32::NAN]);
    }
    points.extend(polyline.iter().map(|p| [p.x, p.y, p.z]));
}

pub(crate) fn add_segment(points: &mut Vec<[f32; 3]>, a: Vec3, b: Vec3) {
    if !points.is_empty() {
        points.push([f32::NAN, f32::NAN, f32::NAN]);
    }
    points.push([a.x, a.y, a.z]);
    points.push([b.x, b.y, b.z]);
}

pub(crate) fn append_arrow(g: &mut DimGeom, tip: Vec3, dir: Vec3, arrow: &ArrowKind) {
    let dir = normalized_or(dir, Vec3::X);
    let perp = Vec3::new(-dir.y, dir.x, 0.0);
    match arrow {
        ArrowKind::None => {}
        ArrowKind::Triangle {
            size,
            filled,
            size_mul,
        } => {
            let size = *size * *size_mul;
            let base = tip + dir * size;
            // ~1:6 length:half-width ratio (≈9.5° half-angle) matches
            // the standard closed-filled block.
            let half_w = size / 6.0;
            let left = base + perp * half_w;
            let right = base - perp * half_w;
            add_segment(&mut g.dim_lines, tip, left);
            add_segment(&mut g.dim_lines, left, right);
            add_segment(&mut g.dim_lines, right, tip);
            if *filled {
                push_tri(&mut g.arrow_fill, tip, left, right);
            }
        }
        ArrowKind::Tick { size, width } => {
            // 45° oblique tick crossing the dim line at the tip; `size` is
            // the half-length used by DIMTSZ.
            let off = (dir + perp).normalize_or_zero() * *size;
            add_segment(&mut g.dim_lines, tip - off, tip + off);
            if *width > 0.0 {
                // A wide polyline's band, square at both ends.
                let side = (perp - dir).normalize_or_zero() * (*width * 0.5);
                let (a, b) = (tip - off - side, tip - off + side);
                let (c, d) = (tip + off + side, tip + off - side);
                push_tri(&mut g.arrow_fill, a, b, c);
                push_tri(&mut g.arrow_fill, a, c, d);
            }
        }
        ArrowKind::Open { size, half_angle } => {
            let base = tip + dir * *size;
            let half_w = *size * half_angle.tan();
            let left = base + perp * half_w;
            let right = base - perp * half_w;
            add_segment(&mut g.dim_lines, tip, left);
            add_segment(&mut g.dim_lines, tip, right);
        }
        ArrowKind::Dot { size, filled } => {
            let r = *size * 0.5;
            const N: usize = 16;
            let mut ring: Vec<Vec3> = Vec::with_capacity(N + 1);
            for i in 0..=N {
                let a = i as f32 * std::f32::consts::TAU / N as f32;
                ring.push(tip + Vec3::new(a.cos() * r, a.sin() * r, 0.0));
            }
            add_polyline(&mut g.dim_lines, &ring);
            if *filled {
                for i in 0..N {
                    push_tri(&mut g.arrow_fill, tip, ring[i], ring[i + 1]);
                }
            }
        }
        ArrowKind::Origin { size, inner } => {
            // The "_Origin" / "_Origin2" blocks at unit size: an open circle
            // of radius 0.5 (plus 0.25 for "_Origin2") around the tip and a
            // stroke back along the leader to 1.0, starting at the centre
            // ("_Origin") or at the outer circle ("_Origin2").
            const N: usize = 16;
            let radii: &[f32] = if *inner { &[0.5, 0.25] } else { &[0.5] };
            for &k in radii {
                let r = *size * k;
                let ring: Vec<Vec3> = (0..=N)
                    .map(|i| {
                        let a = i as f32 * std::f32::consts::TAU / N as f32;
                        tip + Vec3::new(a.cos() * r, a.sin() * r, 0.0)
                    })
                    .collect();
                add_polyline(&mut g.dim_lines, &ring);
            }
            let start = if *inner { *size * 0.5 } else { 0.0 };
            add_segment(&mut g.dim_lines, tip + dir * start, tip + dir * *size);
        }
        ArrowKind::Box_ { size, filled } => {
            let half = *size * 0.5;
            let p1 = tip - dir * half - perp * half;
            let p2 = tip + dir * half - perp * half;
            let p3 = tip + dir * half + perp * half;
            let p4 = tip - dir * half + perp * half;
            add_segment(&mut g.dim_lines, p1, p2);
            add_segment(&mut g.dim_lines, p2, p3);
            add_segment(&mut g.dim_lines, p3, p4);
            add_segment(&mut g.dim_lines, p4, p1);
            if *filled {
                push_tri(&mut g.arrow_fill, p1, p2, p3);
                push_tri(&mut g.arrow_fill, p1, p3, p4);
            }
        }
        ArrowKind::Datum { size, filled } => {
            // Right-pointing triangle with the base perpendicular to the dim
            // line at the tip and the apex along +dir.
            let half = *size * 0.5;
            let base_a = tip + perp * half;
            let base_b = tip - perp * half;
            let apex = tip + dir * *size;
            add_segment(&mut g.dim_lines, base_a, apex);
            add_segment(&mut g.dim_lines, apex, base_b);
            add_segment(&mut g.dim_lines, base_b, base_a);
            if *filled {
                push_tri(&mut g.arrow_fill, base_a, apex, base_b);
            }
        }
        ArrowKind::Custom { size, lines, fill } => {
            let transform = |point: &[f32; 3]| {
                tip - dir * (point[0] * *size) - perp * (point[1] * *size)
                    + Vec3::Z * (point[2] * *size)
            };
            if !lines.is_empty()
                && !g.dim_lines.is_empty()
                && !g.dim_lines.last().is_some_and(|point| point[0].is_nan())
            {
                g.dim_lines.push([f32::NAN; 3]);
            }
            for point in lines {
                if point[0].is_nan() {
                    if !g.dim_lines.last().is_some_and(|point| point[0].is_nan()) {
                        g.dim_lines.push([f32::NAN; 3]);
                    }
                    continue;
                }
                let point = transform(point);
                g.dim_lines.push([point.x, point.y, point.z]);
            }
            for triangle in fill.chunks_exact(3) {
                push_tri(
                    &mut g.arrow_fill,
                    transform(&triangle[0]),
                    transform(&triangle[1]),
                    transform(&triangle[2]),
                );
            }
        }
    }
}

pub(crate) fn push_tri(out: &mut Vec<[f32; 3]>, a: Vec3, b: Vec3, c: Vec3) {
    out.push([a.x, a.y, a.z]);
    out.push([b.x, b.y, b.z]);
    out.push([c.x, c.y, c.z]);
}

/// Returns the text position of a dimension in DXF world-space (f64, no offset applied).
/// Used when building a synthetic Text entity so tessellate() can apply world_offset itself.
/// When the saved `text_middle_point` is zero (no explicit point was written),
/// computes a fallback from the dim geometry and applies DIMTAD/DIMGAP.
pub(crate) fn normalized_or(v: Vec3, fallback: Vec3) -> Vec3 {
    if v.length_squared() <= 1e-12 {
        fallback
    } else {
        v.normalize()
    }
}

/// Canonical block-name → arrow-kind mapping shared by the renderer and the
/// dimstyle preview so both agree on what a named arrowhead looks like.
/// Unknown / empty names fall back to the standard filled triangle.
pub(crate) fn arrow_from_block_name(name: Option<&str>, dimasz: f32) -> ArrowKind {
    name.and_then(|name| builtin_arrow_from_block_name(name, dimasz))
        .unwrap_or(ArrowKind::Triangle {
            size: dimasz,
            filled: true,
            size_mul: 1.0,
        })
}

pub(crate) fn builtin_arrow_from_block_name(name: &str, dimasz: f32) -> Option<ArrowKind> {
    // Built-in arrow block names may carry a leading underscore. Normalize it
    // before matching the canonical names.
    let n = name.trim().trim_start_matches('_').to_ascii_uppercase();
    match n.as_str() {
        "" | "CLOSEDFILLED" => Some(ArrowKind::Triangle {
            size: dimasz,
            filled: true,
            size_mul: 1.0,
        }),
        "CLOSED" | "CLOSEDBLANK" => Some(ArrowKind::Triangle {
            size: dimasz,
            filled: false,
            size_mul: 1.0,
        }),
        // `_Small` is the reference's "Dot small blank": a circle half the
        // arrow size across.
        "SMALL" => Some(ArrowKind::Dot {
            size: dimasz * 0.5,
            filled: false,
        }),
        "OPEN" => Some(ArrowKind::Open {
            size: dimasz,
            half_angle: 9.5_f32.to_radians(),
        }),
        "OPEN30" => Some(ArrowKind::Open {
            size: dimasz,
            half_angle: 15.0_f32.to_radians(),
        }),
        "OPEN90" => Some(ArrowKind::Open {
            size: dimasz,
            half_angle: 45.0_f32.to_radians(),
        }),
        "DOT" => Some(ArrowKind::Dot {
            size: dimasz,
            filled: true,
        }),
        "DOTSMALL" => Some(ArrowKind::Dot {
            size: dimasz * 0.5,
            filled: true,
        }),
        "DOTBLANK" => Some(ArrowKind::Dot {
            size: dimasz,
            filled: false,
        }),
        "DOTSMALLBLANK" => Some(ArrowKind::Dot {
            size: dimasz * 0.5,
            filled: false,
        }),
        "ORIGIN" | "ORIGININDICATOR" => Some(ArrowKind::Origin {
            size: dimasz,
            inner: false,
        }),
        "ORIGIN2" | "ORIGININDICATOR2" => Some(ArrowKind::Origin {
            size: dimasz,
            inner: true,
        }),
        // `ArrowKind::Tick` draws the stroke `size` to either side of the tip
        // (total 2·size — its `size` is a half-length, matching DIMTSZ). The
        // block's stroke runs (-0.5,-0.5)→(0.5,0.5) scaled by DIMASZ, so its
        // half-length is DIMASZ/√2, not DIMASZ/2 — halving drew every
        // oblique stroke √2 short of the file's own. (#898)
        "OBLIQUE" => Some(ArrowKind::Tick {
            size: dimasz * std::f32::consts::FRAC_1_SQRT_2,
            width: 0.0,
        }),
        // `_ArchTick` is the same stroke as an LWPOLYLINE of constant width
        // 0.15 (`_Oblique` is a LINE), so it scales to 0.15·DIMASZ.
        "ARCHTICK" => Some(ArrowKind::Tick {
            size: dimasz * std::f32::consts::FRAC_1_SQRT_2,
            width: dimasz * 0.15,
        }),
        "BOXFILLED" => Some(ArrowKind::Box_ {
            size: dimasz,
            filled: true,
        }),
        "BOXBLANK" | "BOX" => Some(ArrowKind::Box_ {
            size: dimasz,
            filled: false,
        }),
        "DATUMFILLED" | "DATUMTRIANGLEFILLED" => Some(ArrowKind::Datum {
            size: dimasz,
            filled: true,
        }),
        "DATUMBLANK" | "DATUMTRIANGLE" => Some(ArrowKind::Datum {
            size: dimasz,
            filled: false,
        }),
        "NONE" => Some(ArrowKind::None),
        _ => None,
    }
}
