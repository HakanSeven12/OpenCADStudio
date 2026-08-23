// CPU-side hatch fill data. The kernel triangulates the boundary and the GPU
// evaluates the pattern inside that mesh.

use std::sync::Arc;

use cadkernel::geom2d::{
    inside_spans, Curve as KernelCurve, Line as KernelLine, Tolerance, XLine as KernelXLine,
};

pub const MAX_HATCH_BOUNDARY_VERTS: usize = 1024;

/// How near two crossings along a pattern line must be to count as one.
///
/// The boundary reaching here is already a chord approximation of the drawn
/// curves, so this only has to be finer than that sampling and coarser than
/// the rounding in it.
const CLIP_TOLERANCE_LINEAR: f64 = 1.0e-6;

/// One line family from a PAT-format hatch pattern.
///
/// Format mirrors the standard PAT line format:
///   `angle_deg, x0, y0, dx, dy [, dash1, dash2, ...]`
///
/// The perpendicular spacing between adjacent parallel lines is:
///   `| -dx * sin(angle) + dy * cos(angle) |`
#[derive(Clone, Debug)]
pub struct PatFamily {
    /// Line direction in degrees.
    pub angle_deg: f32,
    /// Origin of the first line in this family.
    pub x0: f32,
    pub y0: f32,
    /// Step vector to the next parallel line.
    pub dx: f32,
    pub dy: f32,
    /// Dash/gap sequence: positive = dash length, negative = gap length.
    /// Empty = solid (no dash pattern).
    pub dashes: Vec<f32>,
}

/// The standard gradient fill shapes. Spherical / Hemispherical run from the
/// boundary centre outward; the others run along `angle_deg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GradientKind {
    Linear,
    Cylinder,
    Spherical,
    Hemispherical,
    Curved,
}

impl GradientKind {
    /// User-facing gradient definitions in their standard persisted order.
    /// Inverted definitions are real named patterns rather than a separate
    /// property, so Properties and the draw command share this single list.
    pub const CHOICES: [(GradientKind, bool); 9] = [
        (GradientKind::Linear, false),
        (GradientKind::Cylinder, false),
        (GradientKind::Cylinder, true),
        (GradientKind::Spherical, false),
        (GradientKind::Hemispherical, false),
        (GradientKind::Curved, false),
        (GradientKind::Spherical, true),
        (GradientKind::Hemispherical, true),
        (GradientKind::Curved, true),
    ];

    /// Radial fills shade from the boundary centre outward.
    pub fn radial(self) -> bool {
        matches!(self, GradientKind::Spherical | GradientKind::Hemispherical)
    }

    pub fn choice_label(self, inverted: bool) -> &'static str {
        match (self, inverted) {
            (GradientKind::Linear, _) => "Linear",
            (GradientKind::Cylinder, false) => "Cylindrical",
            (GradientKind::Cylinder, true) => "Inverted cylindrical",
            (GradientKind::Spherical, false) => "Spherical",
            (GradientKind::Spherical, true) => "Inverted spherical",
            (GradientKind::Hemispherical, false) => "Hemispherical",
            (GradientKind::Hemispherical, true) => "Inverted hemispherical",
            (GradientKind::Curved, false) => "Curved",
            (GradientKind::Curved, true) => "Inverted curved",
        }
    }

    pub fn from_choice_label(label: &str) -> Option<(Self, bool)> {
        Self::CHOICES
            .iter()
            .copied()
            .find(|(kind, inverted)| kind.choice_label(*inverted).eq_ignore_ascii_case(label))
    }

    /// Parse the DXF gradient name (`LINEAR`, `INVCYLINDER`, …) into the kind
    /// plus its inverted flag. Unknown names read as Linear.
    pub fn from_name(name: &str) -> (Self, bool) {
        let n = name.trim().to_ascii_uppercase();
        let invert = n.starts_with("INV");
        let base = n.trim_start_matches("INV");
        let kind = if base.contains("CYL") {
            GradientKind::Cylinder
        } else if base.contains("HEMI") {
            GradientKind::Hemispherical
        } else if base.contains("SPHER") {
            GradientKind::Spherical
        } else if base.contains("CURV") {
            GradientKind::Curved
        } else {
            GradientKind::Linear
        };
        (kind, invert)
    }

    /// The DXF gradient name. Linear has no INV variant in the standard set —
    /// an inverted linear is persisted by swapping the colour stops instead.
    pub fn dxf_name(self, invert: bool) -> &'static str {
        match (self, invert) {
            (GradientKind::Linear, _) => "LINEAR",
            (GradientKind::Cylinder, false) => "CYLINDER",
            (GradientKind::Cylinder, true) => "INVCYLINDER",
            (GradientKind::Spherical, false) => "SPHERICAL",
            (GradientKind::Spherical, true) => "INVSPHERICAL",
            (GradientKind::Hemispherical, false) => "HEMISPHERICAL",
            (GradientKind::Hemispherical, true) => "INVHEMISPHERICAL",
            (GradientKind::Curved, false) => "CURVED",
            (GradientKind::Curved, true) => "INVCURVED",
        }
    }

    /// Shape selector for the GPU shader (`grad_kind` low bits).
    pub fn shader_kind(self) -> u32 {
        match self {
            GradientKind::Linear => 0,
            GradientKind::Cylinder => 1,
            GradientKind::Spherical => 2,
            GradientKind::Hemispherical => 3,
            GradientKind::Curved => 4,
        }
    }
}

/// Hatch fill pattern.
#[derive(Clone, Debug)]
pub enum HatchPattern {
    /// Opaque solid fill.
    Solid,
    /// One or more line families (PAT format).
    Pattern(Vec<PatFamily>),
    /// Two-stop gradient from `color` to `color2`, shaped by `kind`; `invert`
    /// swaps the two stops.
    Gradient {
        angle_deg: f32,
        color2: [f32; 4],
        kind: GradientKind,
        invert: bool,
        /// 0 = centred, 1 = shifted towards the upper-left light source.
        shift: f32,
    },
}

/// GPU-side separator between disconnected boundary sub-loops. A finite
/// magnitude sentinel instead of NaN: shaders detect it with a plain
/// `abs(x) < 1e29` compare, which every driver evaluates identically,
/// unlike NaN self-comparison (#386, #416). Boundary verts are local
/// (anchor-relative), so real coordinates never approach it.
pub const GPU_BOUNDARY_SEP: f32 = 1.0e30;

#[derive(Clone, Copy, Debug)]
pub struct FillPlane {
    pub origin: [f64; 3],
    pub x_axis: [f64; 3],
    pub y_axis: [f64; 3],
}

/// A hatched region defined by a closed polygon boundary.
#[derive(Clone, Debug)]
pub struct HatchModel {
    pub render_instance: Option<super::instance_model::RenderInstance>,
    /// World XY anchor (in the same offset-relative coordinate space as
    /// the rest of the scene — `world_offset` already subtracted, but
    /// kept at f64 precision). Boundary vertices are stored as f32
    /// offsets from this anchor so that:
    ///   1) hit-test / paper_canvas can still read small-magnitude f32
    ///      coords without precision loss from the f64 → f32 cast that
    ///      would otherwise happen at large drawing extents (UTM, etc.).
    ///   2) the GPU pipeline can pre-shift the quad in hatch-local
    ///      space (so the fragment shader's `xz` varying stays small)
    ///      and add `world_origin` back inside the view_proj multiply.
    /// Reconstruct WCS-relative coords as `(world_origin.x + v.x as f64,
    /// world_origin.y + v.y as f64)`.
    pub world_origin: [f64; 2],
    /// World-XY coordinates of the boundary polygon vertices, stored as
    /// f32 offsets from `world_origin`. NaN-NaN sentinels separate
    /// disconnected paths and must be preserved un-shifted by consumers.
    /// GPU uploads rewrite them to [`GPU_BOUNDARY_SEP`] — some drivers
    /// (Intel Mesa) fold the shader-side `x == x` NaN test to `true`
    /// under fast math, which turned separators into real vertices and
    /// bled fills outside their boundary (#386, #416).
    pub boundary: Arc<Vec<[f32; 2]>>,
    /// Exact absolute-WCS boundary in f64, set only by the draw commands so a
    /// typed boundary vertex is persisted without the f32 quantization the
    /// render-side `boundary` would incur (issue #311). `None` for hatches
    /// rebuilt from a DXF entity — `add_hatch` then reconstructs the persisted
    /// vertices from `boundary` + `world_origin` instead.
    pub boundary_wcs: Option<Arc<Vec<[f64; 2]>>>,
    /// Optional 3-D placement for planar fills.
    pub fill_plane: Option<FillPlane>,
    pub fill_plane_boundary: Option<Arc<Vec<[f32; 2]>>>,
    /// Per-ring DXF role, aligned with the NaN-separated boundary paths.
    pub boundary_exterior: Option<Arc<Vec<bool>>>,
    /// Source entity handles for each boundary ring.
    pub boundary_sources: Option<Arc<Vec<Vec<acadrust::Handle>>>>,
    /// Exact boundary paths retained for persistence and editing.
    pub boundary_paths: Option<Arc<Vec<acadrust::entities::BoundaryPath>>>,
    /// Island handling used by the persisted hatch entity.
    pub style: acadrust::entities::HatchStyleType,
    /// Fill pattern.
    pub pattern: HatchPattern,
    /// Catalog name for this pattern (e.g. "ANSI31", "SOLID", "LINEAR").
    /// Stored so `add_hatch()` can write the correct name to the DXF entity.
    pub name: String,
    /// RGBA color in [0,1].
    pub color: [f32; 4],
    /// Effective indexed color used by plot style tables; 0 means RGB/default.
    pub aci: u8,
    /// Effective display lineweight used by pattern strokes. PDF export converts
    /// this back to a physical thickness unless a plot style overrides it.
    pub line_weight_px: f32,
    /// Pattern rotation offset in radians (from DXF `pattern_angle`).
    /// Applied on top of each family's base angle at render time.
    pub angle_offset: f32,
    /// Pattern scale multiplier (from DXF `pattern_scale`).
    pub scale: f32,
    /// Normalized draw-order depth in (0,1); higher draws on top. Fed to the
    /// hatch pipeline as a small clip-z bias so this fill orders correctly
    /// against other entity types. 0.0 for transient/preview hatches.
    pub draw_depth: f32,
}

impl HatchModel {
    pub(crate) fn gradient_frame(
        &self,
        angle_deg: f32,
        shift: f32,
    ) -> Option<cadkernel::geom2d::GradientFrame> {
        let boundary: Vec<[f64; 2]> = self
            .boundary
            .iter()
            .filter(|point| point[0].is_finite() && point[1].is_finite())
            .map(|point| [point[0] as f64, point[1] as f64])
            .collect();
        cadkernel::geom2d::gradient_frame(
            &boundary,
            (angle_deg as f64).to_radians(),
            shift as f64,
            Tolerance::default(),
        )
    }

    /// Indexed local-space fill mesh with even-odd loop containment.
    pub(crate) fn fill_mesh(&self) -> (Vec<[f32; 2]>, Vec<u32>) {
        let mut rings = Vec::new();
        let mut ring = Vec::new();
        for &[x, y] in self.boundary.iter() {
            if x.is_finite() && y.is_finite() {
                ring.push([x as f64, y as f64]);
            } else if ring.len() >= 3 {
                rings.push(std::mem::take(&mut ring));
            } else {
                ring.clear();
            }
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }

        let (points, triangles) = cadkernel::geom2d::triangulate_rings(&rings);
        let vertices = points
            .into_iter()
            .map(|[x, y]| [x as f32, y as f32])
            .collect();
        let mut indices = Vec::with_capacity(triangles.len() * 3);
        for triangle in triangles {
            for index in triangle {
                let Ok(index) = u32::try_from(index) else {
                    return (Vec::new(), Vec::new());
                };
                indices.push(index);
            }
        }
        (vertices, indices)
    }

    /// CPU-side rasteriser for `HatchPattern::Pattern` — produces the line
    /// segments inside the boundary so non-GPU consumers (PDF export,
    /// `paper_canvas`, print preview) can draw the actual pattern instead
    /// of just the outline.
    ///
    /// Coordinate frame: each emitted segment is absolute WCS in f64, i.e.
    /// `world_origin + boundary[i]` resolved. Solid / gradient hatches return an
    /// empty vec — callers fall back to their solid-fill path.
    ///
    /// The whole rasterisation is f64 because boundaries and family origins are
    /// resolved in absolute WCS. Narrowing them first at UTM magnitudes would
    /// quantise both the generated lines and the `edge − line` intersections.
    pub fn pattern_segments(&self) -> Vec<[[f64; 2]; 2]> {
        self.pattern_segments_with_dot_length(None)
    }

    /// Plot variant of [`Self::pattern_segments`] that preserves PAT
    /// zero-length dash entries as coincident endpoints. The PDF exporter can
    /// then paint a round point; turning them into short segments here makes
    /// point-only patterns such as AR-CONC visibly look like tiny lines.
    pub(crate) fn pattern_segments_for_plot(&self) -> Vec<[[f64; 2]; 2]> {
        self.pattern_segments_with_dot_length(Some(0.0))
    }

    fn pattern_segments_with_dot_length(
        &self,
        plot_dot_length: Option<f64>,
    ) -> Vec<[[f64; 2]; 2]> {
        let HatchPattern::Pattern(families) = &self.pattern else {
            return Vec::new();
        };
        if self.boundary.is_empty() || families.is_empty() {
            return Vec::new();
        }
        let ox = self.world_origin[0];
        let oy = self.world_origin[1];

        // ── Build edge list from boundary, splitting on NaN sentinels.
        //    Each sub-path is closed (last → first edge) so even-odd
        //    inside-tests work for islands / holes.
        let mut edges: Vec<([f64; 2], [f64; 2])> = Vec::new();
        let mut sub_start: Option<[f64; 2]> = None;
        let mut prev: Option<[f64; 2]> = None;
        for &[bx, by] in self.boundary.iter() {
            if bx.is_nan() || by.is_nan() {
                if let (Some(s), Some(p)) = (sub_start, prev) {
                    if (s[0] - p[0]).abs() > 1e-6 || (s[1] - p[1]).abs() > 1e-6 {
                        edges.push((p, s));
                    }
                }
                sub_start = None;
                prev = None;
                continue;
            }
            let pt = [bx as f64 + ox, by as f64 + oy];
            match (sub_start, prev) {
                (None, _) => {
                    sub_start = Some(pt);
                    prev = Some(pt);
                }
                (Some(_), Some(p)) => {
                    edges.push((p, pt));
                    prev = Some(pt);
                }
                _ => {}
            }
        }
        if let (Some(s), Some(p)) = (sub_start, prev) {
            if (s[0] - p[0]).abs() > 1e-6 || (s[1] - p[1]).abs() > 1e-6 {
                edges.push((p, s));
            }
        }
        if edges.is_empty() {
            return Vec::new();
        }
        // The boundary as kernel curves, built once for every pattern line to
        // be clipped against.
        let clip_tolerance = Tolerance::new(CLIP_TOLERANCE_LINEAR);
        let boundary: Vec<KernelCurve> = edges
            .iter()
            .map(|(a, b)| KernelCurve::Line(KernelLine { start: *a, end: *b }))
            .collect();

        // ── AABB of the boundary in world coords.
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for &(a, b) in &edges {
            for [x, y] in [a, b] {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }

        let scale = self.scale.max(1e-6) as f64;
        let angle_offset = self.angle_offset as f64;
        let mut segments: Vec<[[f64; 2]; 2]> = Vec::new();

        // Hard cap to keep pathological patterns / huge boundaries bounded.
        // `k` is i64: at UTM with a fine spacing the index legitimately reaches
        // ~1e7 and an out-of-range `as i32` saturates silently, collapsing the
        // range to a bogus run of lines from i32::MIN.
        const MAX_LINES_PER_FAMILY: i64 = 4096;
        const MAX_SEGMENTS_TOTAL: usize = 200_000;

        let cos_off = angle_offset.cos();
        let sin_off = angle_offset.sin();
        for family in families {
            let angle = (family.angle_deg as f64).to_radians() + angle_offset;
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            // PAT local frame: dx = along-line phase, dy = perpendicular
            // spacing. Lines step in world by k · (dx, dy)_local rotated
            // into the family's frame.
            let (fdx, fdy) = (family.dx as f64, family.dy as f64);
            let step_x = (fdx * cos_a - fdy * sin_a) * scale;
            let step_y = (fdx * sin_a + fdy * cos_a) * scale;
            let perp_x = -sin_a;
            let perp_y = cos_a;
            let step_perp = step_x * perp_x + step_y * perp_y;
            if step_perp.abs() < 1e-6 {
                continue; // degenerate spacing
            }

            // k range: project AABB corners onto perp direction relative
            // to the family's origin and divide by signed perp step. The
            // pattern origin is rotated by `angle_offset` and scaled —
            // same convention as the GPU shader, so PAT patterns whose
            // `x0/y0` are non-zero (e.g. brick offsets) line up with the
            // on-screen render.
            let (fx0, fy0) = (family.x0 as f64, family.y0 as f64);
            let origin = [
                ox + (fx0 * cos_off - fy0 * sin_off) * scale,
                oy + (fx0 * sin_off + fy0 * cos_off) * scale,
            ];
            let mut p_min = f64::INFINITY;
            let mut p_max = f64::NEG_INFINITY;
            for &[cx, cy] in &[
                [min_x, min_y],
                [max_x, min_y],
                [min_x, max_y],
                [max_x, max_y],
            ] {
                let p = (cx - origin[0]) * perp_x + (cy - origin[1]) * perp_y;
                p_min = p_min.min(p);
                p_max = p_max.max(p);
            }
            let mut k_lo = (p_min / step_perp).floor() as i64 - 1;
            let mut k_hi = (p_max / step_perp).ceil() as i64 + 1;
            if k_lo > k_hi {
                std::mem::swap(&mut k_lo, &mut k_hi);
            }
            // Cap the line COUNT (span), not the absolute index. A hatch far
            // from the pattern origin (0,0) — e.g. a fine-spaced fill at large
            // drawing coordinates — has large-magnitude k at both ends but a
            // small span. Clamping the absolute index to ±MAX_LINES_PER_FAMILY
            // would invert the range (k_lo > k_hi) and emit nothing, silently
            // dropping the whole fill.
            if k_hi.saturating_sub(k_lo) > MAX_LINES_PER_FAMILY {
                k_hi = k_lo.saturating_add(MAX_LINES_PER_FAMILY);
            }

            let period: f64 = family.dashes.iter().map(|d| d.abs() as f64).sum::<f64>() * scale;
            let has_dashes = !family.dashes.is_empty() && period > 1e-6;

            for k in k_lo..=k_hi {
                if segments.len() >= MAX_SEGMENTS_TOTAL {
                    return segments;
                }
                let kf = k as f64;
                let lx = origin[0] + kf * step_x;
                let ly = origin[1] + kf * step_y;

                // The stretches of this pattern line that fall inside the
                // boundary. The direction is a unit vector, so the kernel's
                // parameter is distance along the line — the same `t` the
                // dash walk below steps in.
                //
                // Each span is tested for containment rather than taken from
                // alternating crossings. A line that grazes a corner crosses
                // twice in one place, and an alternating walk inverts from
                // there on: the fill lands in the holes and the solid comes
                // out empty.
                let spans = inside_spans(
                    &boundary,
                    &KernelCurve::XLine(KernelXLine {
                        base: [lx, ly],
                        direction: [cos_a, sin_a],
                    }),
                    clip_tolerance,
                );

                for span in spans {
                    let t0 = span[0];
                    let t1 = span[1];
                    if t1 - t0 < 1e-6 {
                        continue;
                    }
                    if !has_dashes {
                        let p0 = [lx + t0 * cos_a, ly + t0 * sin_a];
                        let p1 = [lx + t1 * cos_a, ly + t1 * sin_a];
                        segments.push([p0, p1]);
                    } else {
                        // Walk the dash sequence along this clipped span with
                        // absolute phase (so the pattern aligns across spans,
                        // matching the GPU shader). Positive entries are
                        // dashes, negative are gaps, and a zero-length entry is
                        // a round-capped point.
                        let n = family.dashes.len();
                        let dot_len = plot_dot_length
                            .unwrap_or_else(|| (period * 0.06).max(1e-3));
                        let phase = t0.rem_euclid(period);
                        // Start at the period boundary at or before t0; the
                        // span clip below drops anything before t0.
                        let mut seg_t = t0 - phase;
                        let mut idx = 0usize;
                        let max_iters = (((t1 - t0) / period).ceil() as usize + 2) * n + 8;
                        let mut iters = 0usize;
                        while seg_t < t1 && iters < max_iters {
                            let d = family.dashes[idx] as f64;
                            let dl = d.abs() * scale;
                            if d > 0.0 {
                                let a = seg_t.max(t0);
                                let b = (seg_t + dl).min(t1);
                                if b > a {
                                    segments.push([
                                        [lx + a * cos_a, ly + a * sin_a],
                                        [lx + b * cos_a, ly + b * sin_a],
                                    ]);
                                }
                            } else if d == 0.0 && seg_t >= t0 - 1e-6 && seg_t <= t1 + 1e-6 {
                                if plot_dot_length == Some(0.0) {
                                    let point = [lx + seg_t * cos_a, ly + seg_t * sin_a];
                                    segments.push([point, point]);
                                } else {
                                    let a = (seg_t - dot_len * 0.5).max(t0);
                                    let b = (seg_t + dot_len * 0.5).min(t1);
                                    if b > a {
                                        segments.push([
                                            [lx + a * cos_a, ly + a * sin_a],
                                            [lx + b * cos_a, ly + b * sin_a],
                                        ]);
                                    }
                                }
                            }
                            seg_t += dl;
                            idx = (idx + 1) % n;
                            iters += 1;
                            if segments.len() >= MAX_SEGMENTS_TOTAL {
                                return segments;
                            }
                        }
                    }
                }
            }
        }
        segments
    }
}
