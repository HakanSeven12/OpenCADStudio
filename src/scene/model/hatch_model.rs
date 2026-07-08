// HatchModel — CPU-side hatch fill data; rendered entirely on the GPU.
use std::sync::Arc;
//
// The boundary is a closed polygon in world XY coordinates.
// The GPU fragment shader performs point-in-polygon and hatch-line tests so
// no line geometry is ever tessellated on the CPU.

pub const MAX_HATCH_BOUNDARY_VERTS: usize = 1024;

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

/// Hatch fill pattern.
#[derive(Clone, Debug)]
pub enum HatchPattern {
    /// Opaque solid fill.
    Solid,
    /// One or more line families (PAT format).
    Pattern(Vec<PatFamily>),
    /// Linear gradient from `color` to `color2` along `angle_deg`.
    Gradient { angle_deg: f32, color2: [f32; 4] },
}

/// A hatched region defined by a closed polygon boundary.
#[derive(Clone, Debug)]
pub struct HatchModel {
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
    pub boundary: Arc<Vec<[f32; 2]>>,
    /// Fill pattern.
    pub pattern: HatchPattern,
    /// Catalog name for this pattern (e.g. "ANSI31", "SOLID", "LINEAR").
    /// Stored so `add_hatch()` can write the correct name to the DXF entity.
    pub name: String,
    /// RGBA color in [0,1].
    pub color: [f32; 4],
    /// Pattern rotation offset in radians (from DXF `pattern_angle`).
    /// Applied on top of each family's base angle at render time.
    pub angle_offset: f32,
    /// Pattern scale multiplier (from DXF `pattern_scale`).
    pub scale: f32,
    /// Optional world-space XY rect [x0, y0, x1, y1] for paper-space
    /// viewport clipping. When `Some`, the pipeline translates it into
    /// a per-frame pixel scissor and clips this hatch's draw call to it,
    /// preventing viewport-projected content from bleeding past the
    /// viewport frame. Mirrors `WireModel.vp_scissor`.
    pub vp_scissor: Option<[f32; 4]>,
    /// Normalized draw-order depth in (0,1); higher draws on top. Fed to the
    /// hatch pipeline as a small clip-z bias so this fill orders correctly
    /// against other entity types. 0.0 for transient/preview hatches.
    pub draw_depth: f32,
}

impl HatchModel {
    /// True when the pattern has more line families than the GPU shader holds.
    pub fn needs_wire_offload(&self) -> bool {
        matches!(
            &self.pattern,
            HatchPattern::Pattern(f)
                if f.len() > crate::scene::pipeline::hatch_gpu::MAX_FAMILIES
        )
    }

    /// CPU-side rasteriser for `HatchPattern::Pattern` — produces the line
    /// segments inside the boundary so non-GPU consumers (PDF export,
    /// `paper_canvas`, print preview) can draw the actual pattern instead
    /// of just the outline.
    ///
    /// Coordinate frame: each emitted segment is in the same
    /// boundary-relative WCS that callers already use, i.e. `world_origin +
    /// boundary[i]`. Solid / gradient hatches return an empty vec —
    /// callers fall back to their solid-fill path.
    pub fn pattern_segments(&self) -> Vec<[[f32; 2]; 2]> {
        let HatchPattern::Pattern(families) = &self.pattern else {
            return Vec::new();
        };
        if self.boundary.is_empty() || families.is_empty() {
            return Vec::new();
        }
        let ox = self.world_origin[0] as f32;
        let oy = self.world_origin[1] as f32;

        // ── Build edge list from boundary, splitting on NaN sentinels.
        //    Each sub-path is closed (last → first edge) so even-odd
        //    inside-tests work for islands / holes.
        let mut edges: Vec<([f32; 2], [f32; 2])> = Vec::new();
        let mut sub_start: Option<[f32; 2]> = None;
        let mut prev: Option<[f32; 2]> = None;
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
            let pt = [bx + ox, by + oy];
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

        // ── AABB of the boundary in world coords.
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for &(a, b) in &edges {
            for [x, y] in [a, b] {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }

        let scale = self.scale.max(1e-6);
        let angle_offset = self.angle_offset;
        let mut segments: Vec<[[f32; 2]; 2]> = Vec::new();

        // Hard cap to keep pathological patterns / huge boundaries bounded.
        const MAX_LINES_PER_FAMILY: i32 = 4096;
        const MAX_SEGMENTS_TOTAL: usize = 200_000;

        let cos_off = angle_offset.cos();
        let sin_off = angle_offset.sin();
        for family in families {
            let angle = family.angle_deg.to_radians() + angle_offset;
            let cos_a = angle.cos();
            let sin_a = angle.sin();
            // PAT local frame: dx = along-line phase, dy = perpendicular
            // spacing. Lines step in world by k · (dx, dy)_local rotated
            // into the family's frame.
            let step_x = (family.dx * cos_a - family.dy * sin_a) * scale;
            let step_y = (family.dx * sin_a + family.dy * cos_a) * scale;
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
            let origin = [
                (family.x0 * cos_off - family.y0 * sin_off) * scale,
                (family.x0 * sin_off + family.y0 * cos_off) * scale,
            ];
            let mut p_min = f32::INFINITY;
            let mut p_max = f32::NEG_INFINITY;
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
            let mut k_lo = (p_min / step_perp).floor() as i32 - 1;
            let mut k_hi = (p_max / step_perp).ceil() as i32 + 1;
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

            let period: f32 = family.dashes.iter().map(|d| d.abs()).sum::<f32>() * scale;
            let has_dashes = !family.dashes.is_empty() && period > 1e-6;

            for k in k_lo..=k_hi {
                if segments.len() >= MAX_SEGMENTS_TOTAL {
                    return segments;
                }
                let kf = k as f32;
                let lx = origin[0] + kf * step_x;
                let ly = origin[1] + kf * step_y;

                // Intersect line P(t) = L + t·(cos_a, sin_a) against each
                // boundary edge; collect t-values where the edge actually
                // crosses (s ∈ [0,1]).
                let mut ts: Vec<f32> = Vec::with_capacity(8);
                for &(a, b) in &edges {
                    let ex = b[0] - a[0];
                    let ey = b[1] - a[1];
                    let det = ex * sin_a - ey * cos_a; // = sin_a·ex − cos_a·ey
                    if det.abs() < 1e-9 {
                        continue;
                    }
                    let rx = a[0] - lx;
                    let ry = a[1] - ly;
                    let t = (ex * ry - ey * rx) / det;
                    let s = (cos_a * ry - sin_a * rx) / det;
                    if s >= 0.0 && s <= 1.0 {
                        ts.push(t);
                    }
                }
                if ts.len() < 2 {
                    continue;
                }
                ts.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                // De-duplicate near-coincident hits (line clipping a vertex).
                ts.dedup_by(|a, b| (*a - *b).abs() < 1e-5);
                if ts.len() < 2 {
                    continue;
                }

                // Even-odd: emit segments between consecutive pairs.
                for pair in ts.chunks_exact(2) {
                    let t0 = pair[0];
                    let t1 = pair[1];
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
                        // a dot — rendered as a short mark so dot patterns
                        // (e.g. DOTS) are visible instead of drawing nothing.
                        let n = family.dashes.len();
                        let dot_len = (period * 0.06).max(1e-3);
                        let phase = t0.rem_euclid(period);
                        // Start at the period boundary at or before t0; the
                        // span clip below drops anything before t0.
                        let mut seg_t = t0 - phase;
                        let mut idx = 0usize;
                        let max_iters = (((t1 - t0) / period).ceil() as usize + 2) * n + 8;
                        let mut iters = 0usize;
                        while seg_t < t1 && iters < max_iters {
                            let d = family.dashes[idx];
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
                                // Short centered mark for the dot.
                                let a = (seg_t - dot_len * 0.5).max(t0);
                                let b = (seg_t + dot_len * 0.5).min(t1);
                                if b > a {
                                    segments.push([
                                        [lx + a * cos_a, ly + a * sin_a],
                                        [lx + b * cos_a, ly + b * sin_a],
                                    ]);
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
