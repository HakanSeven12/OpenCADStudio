/// Tag for pre-baked snap candidates stored inside a WireModel.
/// Kept separate from `snap::SnapType` to avoid circular module dependencies.
#[derive(Clone, Copy, Debug)]
pub enum SnapHint {
    /// Geometric center of a circle, arc, or ellipse.
    Center,
    /// Point entity location.
    Node,
    /// 0 / 90 / 180 / 270 ° point on a circle/arc (within arc span).
    Quadrant,
    /// Insertion point of text or block.
    Insertion,
    /// Midpoint of a curve that has one well-defined midpoint (an arc's
    /// arc-length centre, a spline's `t = 0.5`). Lines / polylines do
    /// not use this — their midpoints are derived from `key_vertices`.
    Midpoint,
    /// An end of a curve that is not a chain of straight segments — an arc,
    /// an elliptical arc, a spline.
    ///
    /// Such a curve cannot put its ends in `key_vertices`: consecutive
    /// entries there are taken to be joined by a straight segment, so an
    /// arc's two ends would also produce a midpoint snap out on the chord,
    /// which is not on the geometry.
    Endpoint,
}

/// Geometric primitive used by the tangent-snap engine.
#[derive(Clone, Debug)]
pub enum TangentGeom {
    /// Infinite line through these two world-space points.
    Line { p1: [f32; 3], p2: [f32; 3] },
    /// Complete circle.
    Circle { center: [f32; 3], radius: f32 },
    /// Complete circle in an arbitrary world-space plane.
    PlanarCircle {
        center: [f64; 3],
        axis_x: [f64; 3],
        axis_y: [f64; 3],
        radius: f64,
    },
    /// Bounded circular arc in its world-space plane.
    Arc {
        center: [f64; 3],
        axis_x: [f64; 3],
        axis_y: [f64; 3],
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
}

/// Viewport-relative display transform for a point marker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointMarker {
    pub origin: glam::DVec3,
    pub normal: glam::DVec3,
    pub axis_x: glam::DVec3,
    pub axis_y: glam::DVec3,
    pub viewport_percent: f32,
}

impl PointMarker {
    pub fn resolve(self, point: glam::DVec3, view_height: f64) -> glam::DVec3 {
        let normal = self.normal.normalize_or(glam::DVec3::Z);
        let delta = point - self.origin;
        let axial = normal * delta.dot(normal);
        let scale = self.viewport_percent.max(0.0) as f64 * 0.01 * view_height.max(0.0);
        self.origin + axial + (delta - axial) * scale
    }
}

/// A 1-D entity (line, arc, polyline) represented as an ordered set of
/// world-space points rendered as a quad strip (TriangleList).
///
/// Linetype is encoded as a GPU-side dash pattern so the CPU never needs to
/// split wires into per-dash segments.  `pattern_length = 0.0` means solid.
#[derive(Clone, Debug)]
pub struct WireModel {
    /// Unique identifier — the handle value as a decimal string.
    pub name: String,
    /// Ordered world-space positions forming a strip of quads. Each entry is
    /// the "high" half of a double-single f32 pair; [`points_low`] carries the
    /// matching residual so the shader can reconstruct the f64 source.
    pub points: Vec<[f32; 3]>,
    /// Low-bit residual paired index-for-index with [`points`]. Empty means
    /// "all-zero residual" (interactive draw / preview wires whose coordinates
    /// don't need sub-f32 precision). Tessellation from CAD f64 fills it.
    pub points_low: Vec<[f32; 3]>,
    /// Point-marker scaling shared by the GPU, picking, and plotting paths.
    pub point_marker: Option<PointMarker>,
    /// RGBA colour in [0, 1].
    pub color: [f32; 4],
    /// Whether this wire is currently selected.
    #[allow(dead_code)]
    pub selected: bool,
    /// Total length of one pattern repeat (world units).  0 = solid line.
    pub pattern_length: f32,
    /// Up to 8 pattern elements: positive = dash length, negative = gap length.
    /// Unused slots must be 0.0 (acts as end-of-pattern sentinel in shader).
    pub pattern: [f32; 8],
    /// Rendered line width in screen pixels (half-width = line_weight_px / 2).
    pub line_weight_px: f32,
    /// World-space band width (drawing units). `0.0` = a normal wire whose
    /// width comes from `line_weight_px` (screen pixels). Non-zero = a wide
    /// polyline: the shader expands this centre-line to `world_width` world
    /// units (scaling with zoom) so the band IS the wire — the linetype dash
    /// pattern then applies to the band instead of a separate hatch fill.
    pub world_width: f32,
    /// Per-point full band width (drawing units), aligned index-for-index with
    /// [`points`], for a polyline whose width VARIES (a taper). Empty = a
    /// constant band of `world_width`. The wire shader reads the two endpoint
    /// widths of each segment and interpolates, so the band tapers smoothly.
    pub taper_widths: Vec<f32>,
    /// ACI color index (1-255).  0 means true-color or unknown (no CTB lookup).
    pub aci: u8,
    /// Pre-baked snap candidates (Center, Node, Quadrant, Insertion).
    pub snap_pts: Vec<(glam::DVec3, SnapHint)>,
    /// Per-segment tangent geometry for Tangent snap.
    /// Line/Arc entities: 1 entry.  LwPolyline: 1 entry per segment.
    pub tangent_geoms: Vec<TangentGeom>,
    /// True polyline vertices used for Endpoint/Midpoint snap.
    /// Non-empty only for entities with distinct vertex positions (Line, LwPolyline).
    /// Empty for tessellated curves (Circle, Arc, Ellipse) which use snap_pts instead.
    pub key_vertices: Vec<[f64; 3]>,
    /// World-space 2-D bounding box [min_x, min_y, max_x, max_y].
    /// Set from acadrust `bounding_box()` in `tessellate_entity()`.
    /// Preview / interim wires use `UNBOUNDED_AABB` so they are never pre-rejected
    /// by the snap world-space filter.
    pub aabb: [f32; 4],
    /// When false the linetype pattern restarts at each NaN-separated segment
    /// (DXF PLINEGEN=0).  When true the pattern runs continuously (PLINEGEN=1).
    pub plinegen: bool,
    /// DGN line-style marker. When false (every standard linetype) the dash
    /// pattern uses the normal phase: A-type end alignment for dash-first
    /// patterns, else centred. When true (DGN pipe walls) the pattern is drawn
    /// from the START vertex with continuous phase and no A-type end forcing —
    /// DGN line styles are not end-aligned.
    pub dash_from_start: bool,
    /// Shared "A"-type end-dash length for MLINE elements. `Some(len)` makes the
    /// dash shader use `len` as the begin/end solid-dash length for EVERY
    /// parallel element (derived once from the multiline's centre-line length),
    /// while `align_total` stays each element's own length — so all elements
    /// share one interior phase (perpendicular dashes line up) yet each still
    /// ends on a dash. `None` (the default) = per-wire A-type / from-start.
    pub dash_align_end: Option<f32>,
    /// Pre-triangulated solid fill: flat vertex list, 3 per triangle (world-offset applied).
    /// Non-empty only for PolyfaceMesh / PolygonMesh entities.
    pub fill_tris: Vec<[f32; 3]>,
    /// Low residual paired with [`fill_tris`] (double-single). Empty = all-zero;
    /// tessellation from CAD f64 fills it so fills stay precise at UTM scale.
    pub fill_tris_low: Vec<[f32; 3]>,
    /// Pre-triangulated pick-only geometry: flat vertex list, 3 per triangle.
    /// Hit-testing treats these as solid; this wire's own draw never uses them.
    ///
    /// Carries the interior of things whose surface the cursor would otherwise
    /// fall through, because [`points`] only bounds them:
    ///
    /// - An entity extruded by a DXF thickness (code 39) — the swept wall
    ///   between each base segment and its extruded copy. Drawn as its four
    ///   wireframe edges, with nothing in between to pick.
    /// - A wide polyline (codes 43 / 40 / 41) — the solid band. It *is* drawn,
    ///   but by the hatch pipeline off `wide_fills`, so the wire that carries
    ///   its centreline has no fill of its own to hit-test.
    ///
    /// Separate from [`fill_tris`] because that channel reaches the GPU: a
    /// thickness wall put there would render shaded and change how every such
    /// drawing looks, and a polyline band would be drawn a second time on top
    /// of the hatch that already draws it.
    pub pick_tris: Vec<[f32; 3]>,
    /// Low residual paired with [`pick_tris`] (double-single). Empty = all-zero.
    pub pick_tris_low: Vec<[f32; 3]>,
    /// SDF glyph quads for this entity's text (TEXT / MTEXT / dimension text /
    /// block-internal text). Non-empty only when SDF text is enabled and this
    /// wire carries a text run. Rides with the wire so it is cached by the
    /// tess memo, cloned on hit, and transformed by the block-expand loop
    /// exactly like `points` — no separate collector pass. The renderer
    /// gathers these across all wires into the text vertex buffer.
    pub text_verts: Vec<crate::scene::pipeline::text_gpu::TextVertex>,
    /// Block-local composed draw-order offset in (-1, 1) for a wire that must
    /// order against its *siblings inside the block* — currently wide-polyline
    /// bands, whose solid area would otherwise cover later-drawn siblings.
    /// `None` (all other wires) = the whole-insert depth resolved from `name`.
    /// `Some(local)` composes at draw time: `insert_depth + local * insert_half`,
    /// mirroring how exploded block fills seed their depth, so bands and fills
    /// from the same block interleave by the block's internal draw order.
    pub depth_override: Option<f32>,
    /// Whether this wire is drawn in the normal viewport pass.
    pub display_visible: bool,
    /// Whether this wire is included in plotted output.
    pub plot_visible: bool,
    /// `true` when [`fill_tris`] is a real 3-D surface (PolyfaceMesh /
    /// PolygonMesh face) that must render with hidden-surface depth and only in
    /// shaded modes. `false` for a flat 2-D overlay fill (SOLID arrowhead,
    /// dimension / MultiLeader text background, greek-LOD text) that draws in
    /// every view mode. The render pass can't infer this from `fill_tris_low`
    /// alone — a 2-D fill at UTM scale carries a low residual too.
    pub fill_is_3d: bool,
    /// `true` only for a planar SOLID entity's interior. Wireframe 3D omits
    /// this fill while preserving its perimeter and every other 2-D overlay.
    pub fill_is_2d_solid: bool,
    /// Shared block-render source and this placement's translation.
    pub render_instance: Option<super::instance_model::RenderInstance>,
}

impl WireModel {
    pub const WHITE: [f32; 4] = [1.00, 1.00, 1.00, 1.0];
    pub const CYAN: [f32; 4] = [0.25, 0.85, 1.00, 1.0];
    pub const SELECTED: [f32; 4] = [0.15, 0.55, 1.00, 1.0];
    /// Rollover (hover) highlight — orange, distinct from the blue selection.
    pub const HOVER: [f32; 4] = [0.95, 0.55, 0.10, 1.0];
    /// Sentinel AABB that never rejects any snap query.
    pub const UNBOUNDED_AABB: [f32; 4] = [
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
        f32::INFINITY,
        f32::INFINITY,
    ];

    /// Double-single split: `high + low ≈ v` to ~f64 precision in two f32s.
    /// Matches the renderer's relative-to-eye reconstruction.
    #[inline]
    pub fn split_ds(v: f64) -> (f32, f32) {
        let high = v as f32;
        (high, (v - high as f64) as f32)
    }

    /// Create a solid preview wire from f64 points, filling the double-single
    /// `points_low` buffer so the line stays precise at UTM-scale coordinates.
    /// Rubber-band previews built straight from f32 absolute points jitter
    /// ~0.5 m at UTM because the wire pass is relative-to-eye and expects the
    /// low residual; this keeps the preview glued to the cursor.
    pub fn solid_f64(name: String, points: Vec<[f64; 3]>, color: [f32; 4], selected: bool) -> Self {
        let mut hi = Vec::with_capacity(points.len());
        let mut lo = Vec::with_capacity(points.len());
        for [x, y, z] in points {
            let (hx, lx) = Self::split_ds(x);
            let (hy, ly) = Self::split_ds(y);
            let (hz, lz) = Self::split_ds(z);
            hi.push([hx, hy, hz]);
            lo.push([lx, ly, lz]);
        }
        let mut w = Self::solid(name, hi, color, selected);
        w.points_low = lo;
        w
    }

    /// Create a solid wire (no dash pattern, 1px weight).
    pub fn solid(name: String, points: Vec<[f32; 3]>, color: [f32; 4], selected: bool) -> Self {
        Self {
            point_marker: None,
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            text_verts: Vec::new(),
            name,
            points,
            points_low: Vec::new(),
            color,
            selected,
            aci: 0,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: vec![],
            tangent_geoms: vec![],
            key_vertices: vec![],
            aabb: Self::UNBOUNDED_AABB,
            plinegen: true,
            dash_from_start: false,
            dash_align_end: None,
            fill_tris: vec![],
            fill_tris_low: Vec::new(),
        }
    }

    /// Return a clone with every point translated by `delta`.
    pub fn translated(&self, delta: glam::Vec3) -> Self {
        let mut out = self.clone();
        out.name = format!("preview_{}", self.name);
        out.color = Self::CYAN;
        out.selected = false;
        map_points(&mut out.points, &mut out.points_low, |point| {
            point + delta.as_dvec3()
        });
        if let Some(marker) = &mut out.point_marker {
            marker.origin += delta.as_dvec3();
        }
        if !out.text_verts.is_empty() {
            let (dx, dy, dz) = (delta.x as f64, delta.y as f64, delta.z as f64);
            out.text_verts =
                map_text_verts(&self.text_verts, |x, y, z| (x + dx, y + dy, z + dz));
        }
        out
    }

    /// Return a clone with every point rotated around `center` by `angle_rad`.
    pub fn rotated(&self, center: glam::Vec3, angle_rad: f32) -> Self {
        self.rotated_about_axis(center, glam::Vec3::Z, angle_rad)
    }

    /// Return a clone rotated about an arbitrary world-space axis.
    pub fn rotated_about_axis(
        &self,
        center: glam::Vec3,
        axis: glam::Vec3,
        angle_rad: f32,
    ) -> Self {
        let rotation = glam::Quat::from_axis_angle(axis.normalize_or_zero(), angle_rad);
        let (s, c) = angle_rad.sin_cos();
        let mut out = self.clone();
        out.name = format!("preview_{}", self.name);
        out.color = Self::CYAN;
        out.selected = false;
        for p in &mut out.points {
            let mapped = center + rotation * (glam::Vec3::from_array(*p) - center);
            *p = mapped.to_array();
        }
        for p in &mut out.points_low {
            *p = (rotation * glam::Vec3::from_array(*p)).to_array();
        }
        if let Some(marker) = &mut out.point_marker {
            let center = center.as_dvec3();
            let rotation = glam::DQuat::from_axis_angle(
                axis.as_dvec3().normalize_or(glam::DVec3::Z),
                angle_rad as f64,
            );
            marker.origin = center + rotation * (marker.origin - center);
            marker.normal = (rotation * marker.normal).normalize_or(glam::DVec3::Z);
            marker.axis_x = (rotation * marker.axis_x).normalize_or(glam::DVec3::X);
            marker.axis_y = (rotation * marker.axis_y).normalize_or(glam::DVec3::Y);
        }
        if !out.text_verts.is_empty() {
            let center = center.as_dvec3();
            let axis = axis.as_dvec3().normalize_or_zero();
            let (s, c) = (s as f64, c as f64);
            out.text_verts = map_text_verts(&self.text_verts, |x, y, z| {
                let v = glam::DVec3::new(x, y, z) - center;
                let mapped = center
                    + v * c
                    + axis.cross(v) * s
                    + axis * axis.dot(v) * (1.0 - c);
                (mapped.x, mapped.y, mapped.z)
            });
        }
        out
    }

    /// Return a clone with every point uniformly scaled from `center` by `factor`.
    pub fn scaled(&self, center: glam::Vec3, factor: f32) -> Self {
        let mut out = self.clone();
        out.name = format!("preview_{}", self.name);
        out.color = Self::CYAN;
        out.selected = false;
        if let Some(marker) = out.point_marker {
            let center = center.as_dvec3();
            let normal = marker.normal.normalize_or(glam::DVec3::Z);
            let target_origin = center + (marker.origin - center) * factor as f64;
            map_points(&mut out.points, &mut out.points_low, |point| {
                let delta = point - marker.origin;
                let axial = normal * delta.dot(normal);
                target_origin + (delta - axial) + axial * factor as f64
            });
            if let Some(target) = &mut out.point_marker {
                target.origin = target_origin;
            }
        } else {
            let center = center.as_dvec3();
            map_points(&mut out.points, &mut out.points_low, |point| {
                center + (point - center) * factor as f64
            });
        }
        if !out.text_verts.is_empty() {
            let (cx, cy, cz) = (center.x as f64, center.y as f64, center.z as f64);
            let f = factor as f64;
            out.text_verts = map_text_verts(&self.text_verts, |x, y, z| {
                (cx + (x - cx) * f, cy + (y - cy) * f, cz + (z - cz) * f)
            });
        }
        out
    }

    /// Return a clone for a stretch preview: every point whose XY lies inside
    /// the crossing window `[win_min, win_max]` is translated by `delta`; points
    /// outside stay put. Exact for line/polyline vertices (the primary stretch
    /// targets); curve tessellation points may deform where a window edge cuts
    /// through them, matching the per-vertex nature of the operation.
    pub fn stretched(
    &self,
    win_min: glam::Vec3,
    win_max: glam::Vec3,
    delta: glam::Vec3,
) -> Self {
    self.stretched_windows(&[(win_min, win_max)], delta)
}

/// Return a clone for a multi-window STRETCH preview. A point moves exactly
/// once when it lies inside any of the crossing windows.
pub fn stretched_windows(
    &self,
    windows: &[(glam::Vec3, glam::Vec3)],
    delta: glam::Vec3,
) -> Self {
        let mut out = self.clone();
        out.name = format!("preview_{}", self.name);
        out.color = Self::CYAN;
        out.selected = false;

        let inside = |x: f32, y: f32| {
            windows.iter().any(|(win_min, win_max)| {
                x >= win_min.x
                    && x <= win_max.x
                    && y >= win_min.y
                    && y <= win_max.y
            })
        };

        if let Some(marker) = out.point_marker {
            if inside(marker.origin.x as f32, marker.origin.y as f32) {
                map_points(&mut out.points, &mut out.points_low, |point| {
                    point + delta.as_dvec3()
                });
                if let Some(target) = &mut out.point_marker {
                    target.origin += delta.as_dvec3();
                }
            }
        } else {
            for p in &mut out.points {
                if inside(p[0], p[1]) {
                    p[0] += delta.x;
                    p[1] += delta.y;
                    p[2] += delta.z;
                }
            }
        }

        if !out.text_verts.is_empty() {
            let (dx, dy, dz) =
                (delta.x as f64, delta.y as f64, delta.z as f64);

            out.text_verts = map_text_verts(&self.text_verts, |x, y, z| {
                let inside = windows.iter().any(|(win_min, win_max)| {
                    x >= win_min.x as f64
                        && x <= win_max.x as f64
                        && y >= win_min.y as f64
                        && y <= win_max.y as f64
                });

                if inside {
                    (x + dx, y + dy, z + dz)
                } else {
                    (x, y, z)
                }
            });
        }

        out
    }

    /// Return a clone mirrored across the line through `p1`→`p2`.
    pub fn mirrored(&self, p1: glam::Vec3, p2: glam::Vec3) -> Self {
        self.mirrored_in_plane(p1, p2, glam::Vec3::Z)
    }

    /// Return a clone reflected through the plane containing the picked line
    /// and the supplied working-plane normal.
    pub fn mirrored_in_plane(
        &self,
        p1: glam::Vec3,
        p2: glam::Vec3,
        working_normal: glam::Vec3,
    ) -> Self {
        let plane_normal = (p2 - p1)
            .cross(working_normal.normalize_or_zero())
            .normalize_or_zero();
        let mut out = self.clone();
        out.name = format!("preview_{}", self.name);
        out.color = Self::CYAN;
        out.selected = false;
        if plane_normal.length_squared() < 1e-12 {
            return out;
        }
        for p in &mut out.points {
            let point = glam::Vec3::from_array(*p);
            *p = (point - 2.0 * plane_normal.dot(point - p1) * plane_normal).to_array();
        }
        // World position is the double-single sum `points + points_low` (text /
        // UTM wires split it), so the residual must reflect too — as a direction
        // (linear reflection about the axis, no `p1` offset).
        for p in &mut out.points_low {
            let point = glam::Vec3::from_array(*p);
            *p = (point - 2.0 * plane_normal.dot(point) * plane_normal).to_array();
        }
        if let Some(marker) = &mut out.point_marker {
            let p1 = p1.as_dvec3();
            let normal = plane_normal.as_dvec3();
            marker.origin -= 2.0 * normal.dot(marker.origin - p1) * normal;
            marker.normal =
                (marker.normal - 2.0 * normal.dot(marker.normal) * normal)
                    .normalize_or(glam::DVec3::Z);
            marker.axis_x =
                (marker.axis_x - 2.0 * normal.dot(marker.axis_x) * normal)
                    .normalize_or(glam::DVec3::X);
            marker.axis_y =
                (marker.axis_y - 2.0 * normal.dot(marker.axis_y) * normal)
                    .normalize_or(glam::DVec3::Y);
        }
        // Glyph quads reflect wholesale (true mirror) — the caller only routes
        // text through here for MIRRTEXT-on; MIRRTEXT-off relocates via
        // `translated` so glyphs stay readable.
        if !out.text_verts.is_empty() {
            let p1 = p1.as_dvec3();
            let normal = plane_normal.as_dvec3();
            out.text_verts = map_text_verts(&self.text_verts, |x, y, z| {
                let point = glam::DVec3::new(x, y, z);
                let mapped = point - 2.0 * normal.dot(point - p1) * normal;
                (mapped.x, mapped.y, mapped.z)
            });
        }
        out
    }

    /// Total arc-length of this wire (sum of segment lengths).
    #[allow(dead_code)]
    pub fn length(&self) -> f32 {
        self.points
            .windows(2)
            .map(|w| {
                let dx = w[1][0] - w[0][0];
                let dy = w[1][1] - w[0][1];
                let dz = w[1][2] - w[0][2];
                (dx * dx + dy * dy + dz * dz).sqrt()
            })
            .sum()
    }

    pub fn point_world(&self, index: usize, view_height: f64) -> glam::DVec3 {
        let high = self.points[index];
        let low = self.points_low.get(index).copied().unwrap_or([0.0; 3]);
        let point = glam::DVec3::new(
            high[0] as f64 + low[0] as f64,
            high[1] as f64 + low[1] as f64,
            high[2] as f64 + low[2] as f64,
        );
        self.point_marker
            .map_or(point, |marker| marker.resolve(point, view_height))
    }
}

fn map_points(
    points: &mut [[f32; 3]],
    points_low: &mut Vec<[f32; 3]>,
    map: impl Fn(glam::DVec3) -> glam::DVec3,
) {
    let old_low = std::mem::take(points_low);
    points_low.reserve(points.len());
    for (index, high) in points.iter_mut().enumerate() {
        if !high[0].is_finite() || !high[1].is_finite() || !high[2].is_finite() {
            points_low.push([0.0; 3]);
            continue;
        }
        let low = old_low.get(index).copied().unwrap_or([0.0; 3]);
        let mapped = map(glam::DVec3::new(
            high[0] as f64 + low[0] as f64,
            high[1] as f64 + low[1] as f64,
            high[2] as f64 + low[2] as f64,
        ));
        let (hx, lx) = WireModel::split_ds(mapped.x);
        let (hy, ly) = WireModel::split_ds(mapped.y);
        let (hz, lz) = WireModel::split_ds(mapped.z);
        *high = [hx, hy, hz];
        points_low.push([lx, ly, lz]);
    }
}

/// Map every glyph vertex's double-single world position through `f`, re-
/// splitting the result. The preview transforms above move `points`, but SDF
/// glyph quads live in `text_verts` (absolute-world double-single) — so a text
/// ghost (MOVE / COPY / ROTATE / SCALE / STRETCH / MIRROR preview) must carry
/// these along or the dragged text renders frozen at its source (issue #316).
/// Paper-space viewport projection uses it for the same reason (issue #385).
pub(crate) fn map_text_verts(
    verts: &[crate::scene::pipeline::text_gpu::TextVertex],
    f: impl Fn(f64, f64, f64) -> (f64, f64, f64),
) -> Vec<crate::scene::pipeline::text_gpu::TextVertex> {
    use crate::scene::pipeline::text_gpu::split_ds;
    verts
        .iter()
        .map(|v| {
            let (nx, ny, nz) = f(
                v.pos[0] as f64 + v.pos_low[0] as f64,
                v.pos[1] as f64 + v.pos_low[1] as f64,
                v.pos[2] as f64 + v.pos_low[2] as f64,
            );
            let (xh, xl) = split_ds(nx);
            let (yh, yl) = split_ds(ny);
            let (zh, zl) = split_ds(nz);
            crate::scene::pipeline::text_gpu::TextVertex {
                pos: [xh, yh, zh],
                pos_low: [xl, yl, zl],
                ..*v
            }
        })
        .collect()
}

impl Default for WireModel {
    fn default() -> Self {
        Self {
            point_marker: None,
            text_verts: Vec::new(),
            name: String::new(),
            points: Vec::new(),
            points_low: Vec::new(),
            color: Self::WHITE,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            world_width: 0.0,
            taper_widths: Vec::new(),
            aci: 0,
            snap_pts: Vec::new(),
            tangent_geoms: Vec::new(),
            key_vertices: Vec::new(),
            aabb: Self::UNBOUNDED_AABB,
            plinegen: true,
            dash_from_start: false,
            dash_align_end: None,
            fill_tris: Vec::new(),
            fill_tris_low: Vec::new(),
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: false,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
        }
    }
}
