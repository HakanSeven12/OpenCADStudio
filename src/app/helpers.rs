use acadrust::tables::Ucs;

// ── Coordinate parsing ─────────────────────────────────────────────────────

/// How a typed coordinate should be interpreted relative to the last
/// input point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CoordKind {
    /// `@x,y` prefix — offset from the last input point.
    Relative,
    /// `#x,y` prefix — world/UCS absolute, overriding DYN.
    Absolute,
    /// No prefix — the caller decides (DYN on → relative, off → absolute).
    Default,
}

/// Parse a typed coordinate string into a local Vec3 plus its interpretation.
/// Accepts Cartesian, polar, cylindrical and spherical forms:
/// `x,y`, `x,y,z`, `distance<angle`, `distance<angle,z`, and
/// `distance<angle<elevation`.
/// A leading `@` marks the value relative to the last point; a leading
/// `#` forces absolute. Separators: comma or semicolon.
///
/// Each number is read by the same pair that writes it, so anything the
/// drawing displays can be typed straight back: `5'-6 1/2"`, `9 1/2`,
/// `@72'8"<n45d20'6"e`. Angles after `<` are directions, and so are counted
/// from the drawing's zero and run the way it says.
pub(crate) fn parse_coord(text: &str) -> Option<(glam::DVec3, CoordKind)> {
    use crate::entities::common::{parse_direction, parse_length};
    let trimmed = text.trim();
    let (kind, rest) = if let Some(r) = trimmed.strip_prefix('@') {
        (CoordKind::Relative, r)
    } else if let Some(r) = trimmed.strip_prefix('#') {
        (CoordKind::Absolute, r)
    } else {
        (CoordKind::Default, trimmed)
    };
    if let Some((distance, angles)) = rest.split_once('<') {
        let distance = parse_length(distance)?;
        if let Some((azimuth, elevation)) = angles.split_once('<') {
            let azimuth = parse_direction(azimuth)?;
            // The rise out of the plane is a size, not a compass direction, so
            // it is not counted from the drawing's zero.
            let elevation = crate::entities::common::parse_angle(elevation)?;
            let horizontal = distance * elevation.cos();
            return Some((
                glam::DVec3::new(
                    horizontal * azimuth.cos(),
                    horizontal * azimuth.sin(),
                    distance * elevation.sin(),
                ),
                kind,
            ));
        }
        // A cylindrical `distance<angle,z` splits on the comma — but a
        // surveyor's bearing has none, so nothing here can eat one.
        let (angle, z) = match angles.split_once(|c| c == ',' || c == ';') {
            Some((angle, z)) => (parse_direction(angle)?, parse_length(z)?),
            None => (parse_direction(angles)?, 0.0),
        };
        return Some((
            glam::DVec3::new(distance * angle.cos(), distance * angle.sin(), z),
            kind,
        ));
    }
    let parts: Vec<f64> = rest
        .split(|c| c == ',' || c == ';')
        .map(parse_length)
        .collect::<Option<Vec<f64>>>()?;
    match parts.as_slice() {
        [x, y] => Some((glam::DVec3::new(*x, *y, 0.0), kind)),
        [x, y, z] => Some((glam::DVec3::new(*x, *y, *z), kind)),
        _ => None,
    }
}

#[cfg(test)]
mod coordinate_parsing_tests {
    use super::*;

    #[test]
    fn parses_all_coordinate_forms() {
        let close = |a: glam::DVec3, b: glam::DVec3| (a - b).length() < 1e-9;
        assert!(close(parse_coord("1,2").unwrap().0, glam::dvec3(1.0, 2.0, 0.0)));
        assert!(close(parse_coord("1,2,3").unwrap().0, glam::dvec3(1.0, 2.0, 3.0)));
        assert!(close(parse_coord("10<90").unwrap().0, glam::dvec3(0.0, 10.0, 0.0)));
        assert!(close(parse_coord("10<90,4").unwrap().0, glam::dvec3(0.0, 10.0, 4.0)));
        assert!(close(parse_coord("10<0<30").unwrap().0, glam::dvec3(5.0 * 3.0_f64.sqrt(), 0.0, 5.0)));
        assert_eq!(parse_coord("@10<0").unwrap().1, CoordKind::Relative);
        assert_eq!(parse_coord("#1,2").unwrap().1, CoordKind::Absolute);
    }
}

// ── UCS ↔ WCS converter ─────────────────────────────────────────────────────

/// The single bridge between WCS (how geometry and the file are stored) and the
/// active UCS (the coordinate system the user works in). Build one from the
/// tab's active UCS via [`DocumentTab::ucs_xform`](super::DocumentTab); the
/// `None` UCS yields the identity (plain WCS).
///
/// Every system that has to speak UCS — the coordinate readout, typed input,
/// the UCS icon, snap/ortho, the ViewCube — goes through this one type instead
/// of re-deriving the axis math. Axes are orthonormal, so the inverse rotation
/// is just the transpose (the dot products in `to_ucs`); no matrix inversion.
#[derive(Clone, Copy)]
pub(super) struct UcsXform {
    origin: glam::DVec3,
    x: glam::DVec3,
    y: glam::DVec3,
    z: glam::DVec3,
}

impl UcsXform {
    /// Plain WCS — no active UCS.
    pub(super) fn identity() -> Self {
        Self {
            origin: glam::DVec3::ZERO,
            x: glam::DVec3::X,
            y: glam::DVec3::Y,
            z: glam::DVec3::Z,
        }
    }

    pub(super) fn from_ucs(ucs: &Ucs) -> Self {
        let v = |a: acadrust::types::Vector3| glam::DVec3::new(a.x, a.y, a.z);
        let x = v(ucs.x_axis).normalize_or(glam::DVec3::X);
        let raw_y = v(ucs.y_axis).normalize_or(glam::DVec3::Y);
        let fallback_z = if x.dot(glam::DVec3::Z).abs() < 0.999 {
            glam::DVec3::Z
        } else {
            glam::DVec3::Y
        };
        let z = x.cross(raw_y).normalize_or(x.cross(fallback_z).normalize());
        let y = z.cross(x).normalize();
        Self { origin: v(ucs.origin), x, y, z }
    }

    pub(super) fn from_active(ucs: Option<&Ucs>) -> Self {
        ucs.map(Self::from_ucs).unwrap_or_else(Self::identity)
    }

    /// True when this is plain WCS — lets callers skip the conversion.
    pub(super) fn is_identity(&self) -> bool {
        self.origin == glam::DVec3::ZERO
            && self.x == glam::DVec3::X
            && self.y == glam::DVec3::Y
            && self.z == glam::DVec3::Z
    }

    /// UCS point → WCS.
    pub(super) fn to_wcs(&self, p: glam::DVec3) -> glam::DVec3 {
        self.origin + self.x * p.x + self.y * p.y + self.z * p.z
    }

    /// WCS point → UCS.
    pub(super) fn to_ucs(&self, p: glam::DVec3) -> glam::DVec3 {
        let d = p - self.origin;
        glam::DVec3::new(d.dot(self.x), d.dot(self.y), d.dot(self.z))
    }

    /// UCS direction → WCS (rotation only, no origin shift).
    pub(super) fn vec_to_wcs(&self, v: glam::DVec3) -> glam::DVec3 {
        self.x * v.x + self.y * v.y + self.z * v.z
    }

    /// WCS direction → UCS (rotation only, no origin shift).
    pub(super) fn vec_to_ucs(&self, v: glam::DVec3) -> glam::DVec3 {
        glam::DVec3::new(v.dot(self.x), v.dot(self.y), v.dot(self.z))
    }

    /// `(origin, x, y, z)` axes in WCS — for drawing the UCS icon.
    pub(super) fn axes(&self) -> (glam::DVec3, glam::DVec3, glam::DVec3, glam::DVec3) {
        (self.origin, self.x, self.y, self.z)
    }

    pub(super) fn working_plane(&self) -> crate::command::WorkingPlane {
        crate::command::WorkingPlane::new(self.origin, self.x, self.y)
    }

    /// UCS→world rotation matrix (columns = UCS axes). For consumers that take
    /// a `Mat4` rotation directly (ViewCube, OTRACK ray directions). The GPU /
    /// screen layer is f32, so the axes downcast here at the boundary.
    pub(super) fn rotation_mat(&self) -> glam::Mat4 {
        glam::Mat4::from_cols(
            self.x.as_vec3().extend(0.0),
            self.y.as_vec3().extend(0.0),
            self.z.as_vec3().extend(0.0),
            glam::Vec4::W,
        )
    }

    /// Full UCS-local → WCS transform, using `origin` as local zero while
    /// retaining this UCS's orthonormal axes.
    pub(super) fn to_wcs_transform_at(
        &self,
        origin: glam::DVec3,
    ) -> acadrust::types::Transform {
        use acadrust::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.y.x, self.z.x, origin.x],
                [self.x.y, self.y.y, self.z.y, origin.y],
                [self.x.z, self.y.z, self.z.z, origin.z],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    /// Full WCS → UCS-local transform, using `origin` as the local zero.
    pub(super) fn to_ucs_transform_at(
        &self,
        origin: glam::DVec3,
    ) -> acadrust::types::Transform {
        use acadrust::types::{Matrix4, Transform};
        Transform::from_matrix(Matrix4 {
            m: [
                [self.x.x, self.x.y, self.x.z, -origin.dot(self.x)],
                [self.y.x, self.y.y, self.y.z, -origin.dot(self.y)],
                [self.z.x, self.z.y, self.z.z, -origin.dot(self.z)],
                [0.0, 0.0, 0.0, 1.0],
            ],
        })
    }

    /// Convert from the represented UCS into its canonical local frame.
    pub(super) fn to_ucs_transform(&self) -> acadrust::types::Transform {
        self.to_ucs_transform_at(self.origin)
    }
}

// ── UCS ↔ WCS transforms (thin wrappers over `UcsXform`) ────────────────────

/// Rotate a UCS-local offset into WCS without applying the origin
/// translation — used for relative coordinate entry, where only the
/// axis orientation matters, not the UCS origin.
pub(super) fn ucs_rotate_vec(offset: glam::DVec3, ucs: &Ucs) -> glam::DVec3 {
    UcsXform::from_ucs(ucs).vec_to_wcs(offset)
}

/// Convert a point from UCS local coordinates to WCS.
pub(super) fn ucs_to_wcs(pt: glam::DVec3, ucs: &Ucs) -> glam::DVec3 {
    UcsXform::from_ucs(ucs).to_wcs(pt)
}

/// Return the normalised Z axis of a UCS (cross product of X and Y axes).
pub(super) fn ucs_z_axis(ucs: &Ucs) -> glam::DVec3 {
    UcsXform::from_ucs(ucs).axes().3
}

/// Build a UCS whose Z axis is `normal`, anchored at `origin`.
///
/// X and Y come from the arbitrary-axis algorithm in
/// [`crate::scene::view::transform::ocs_axes`], the same frame entities get
/// from their extrusion direction, so the same face always yields the same X
/// axis. Returns `None` when `normal` is degenerate and defines no plane.
pub(super) fn ucs_from_normal(origin: glam::DVec3, normal: glam::DVec3) -> Option<Ucs> {
    let z = normal.normalize_or_zero();
    if z.length_squared() < 1e-12 {
        return None;
    }
    let ((xx, xy, xz), (yx, yy, yz)) = crate::scene::view::transform::ocs_axes((z.x, z.y, z.z));
    let mut ucs = Ucs::new("*ACTIVE*");
    ucs.origin = acadrust::types::Vector3::new(origin.x, origin.y, origin.z);
    ucs.x_axis = acadrust::types::Vector3::new(xx, xy, xz);
    ucs.y_axis = acadrust::types::Vector3::new(yx, yy, yz);
    Some(ucs)
}

/// Build a UCS with `origin` and axes rotated by `angle_z_rad` around the Z axis.
pub(super) fn ucs_rotated_z(origin: glam::DVec3, angle_z: f32) -> Ucs {
    let cos = angle_z.cos() as f64;
    let sin = angle_z.sin() as f64;
    let mut ucs = Ucs::new("*ACTIVE*");
    ucs.origin = acadrust::types::Vector3::new(origin.x, origin.y, origin.z);
    ucs.x_axis = acadrust::types::Vector3::new(cos, sin, 0.0);
    ucs.y_axis = acadrust::types::Vector3::new(-sin, cos, 0.0);
    ucs
}

// ── Drawing constraint helpers ─────────────────────────────────────────────

/// The two live drafting directions in degrees inside the active UCS plane.
pub(super) fn drafting_angles(
    isometric: bool,
    iso_plane: super::settings::IsoPlane,
    snap_angle_deg: f32,
) -> [f64; 2] {
    let base = if isometric {
        iso_plane.angles()
    } else {
        [0.0, 90.0]
    };
    base.map(|angle| angle + snap_angle_deg as f64)
}

/// Convert the live drafting directions into world-space axes.
pub(super) fn drafting_axes(
    x: glam::DVec3,
    y: glam::DVec3,
    z: glam::DVec3,
    isometric: bool,
    iso_plane: super::settings::IsoPlane,
    snap_angle_deg: f32,
) -> (glam::DVec3, glam::DVec3, glam::DVec3) {
    let [a, b] = drafting_angles(isometric, iso_plane, snap_angle_deg);
    let direction = |degrees: f64| {
        let radians = degrees.to_radians();
        (x * radians.cos() + y * radians.sin()).normalize_or(x)
    };
    (direction(a), direction(b), z.normalize_or(x.cross(y)))
}

/// Constrain `pt` to the nearest live drafting direction from `base`.
pub(super) fn drafting_constrain(
    pt: glam::DVec3,
    base: glam::DVec3,
    xf: &UcsXform,
    isometric: bool,
    iso_plane: super::settings::IsoPlane,
    snap_angle_deg: f32,
) -> glam::DVec3 {
    let p = xf.to_ucs(pt);
    let b = xf.to_ucs(base);
    let delta = glam::DVec2::new(p.x - b.x, p.y - b.y);
    let [a, c] = drafting_angles(isometric, iso_plane, snap_angle_deg).map(|degrees| {
        let radians = degrees.to_radians();
        glam::DVec2::new(radians.cos(), radians.sin())
    });
    let direction = if delta.dot(a).abs() >= delta.dot(c).abs() { a } else { c };
    let projected = direction * delta.dot(direction);
    let c = glam::DVec3::new(b.x + projected.x, b.y + projected.y, p.z);
    xf.to_wcs(c)
}

/// Constrain `pt` to the nearest polar angle multiple from `base`, measured in
/// the active UCS plane (identity `xf` = world XY, Z-up).
pub(super) fn polar_constrain(
    pt: glam::DVec3,
    base: glam::DVec3,
    step_deg: f32,
    xf: &UcsXform,
) -> glam::DVec3 {
    let p = xf.to_ucs(pt);
    let b = xf.to_ucs(base);
    let dx = p.x - b.x;
    let dy = p.y - b.y;
    let dist = (dx * dx + dy * dy).sqrt();
    if dist < 1e-6 {
        return pt;
    }
    let step = (step_deg as f64).to_radians();
    let angle = dy.atan2(dx);
    let snapped = (angle / step).round() * step;
    xf.to_wcs(glam::DVec3::new(
        b.x + dist * snapped.cos(),
        b.y + dist * snapped.sin(),
        p.z,
    ))
}

/// Return the polar-constrained point while its ray is engaged.
pub(super) fn polar_constrain_if_near(
    pt: glam::DVec3,
    base: glam::DVec3,
    step_deg: f32,
    view_rot: glam::Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    tol_px: f32,
    xf: &UcsXform,
) -> Option<glam::DVec3> {
    let snapped = polar_constrain(pt, base, step_deg, xf);
    let to_screen = |w: glam::DVec3| {
        let ndc = view_rot.project_point3((w - eye).as_vec3());
        (
            (ndc.x + 1.0) * 0.5 * bounds.width,
            (1.0 - ndc.y) * 0.5 * bounds.height,
        )
    };
    let (cx, cy) = to_screen(pt);
    let (sx, sy) = to_screen(snapped);
    (((cx - sx).powi(2) + (cy - sy).powi(2)).sqrt() <= tol_px).then_some(snapped)
}

/// Constrain near an engaged polar ray; otherwise keep the cursor free.
pub(super) fn polar_constrain_near(
    pt: glam::DVec3,
    base: glam::DVec3,
    step_deg: f32,
    view_rot: glam::Mat4,
    eye: glam::DVec3,
    bounds: iced::Rectangle,
    tol_px: f32,
    xf: &UcsXform,
) -> glam::DVec3 {
    polar_constrain_if_near(pt, base, step_deg, view_rot, eye, bounds, tol_px, xf)
        .unwrap_or(pt)
}

/// Hard axis lock (#312): the locked ray's direction — the nearest polar
/// increment (or ortho axis) from `base` toward `cursor`, in the active UCS
/// plane. `None` when the cursor sits on the base point.
pub(super) fn axis_lock_capture(
    cursor: glam::DVec3,
    base: glam::DVec3,
    polar: bool,
    step_deg: f32,
    xf: &UcsXform,
    isometric: bool,
    iso_plane: super::settings::IsoPlane,
    snap_angle_deg: f32,
) -> Option<glam::DVec3> {
    let p = xf.to_ucs(cursor);
    let b = xf.to_ucs(base);
    let dx = p.x - b.x;
    let dy = p.y - b.y;
    if dx.hypot(dy) < 1e-9 {
        return None;
    }
    let ang = if polar {
        let base = (snap_angle_deg as f64).to_radians();
        let step = (step_deg as f64).to_radians();
        ((dy.atan2(dx) - base) / step).round() * step + base
    } else {
        let delta = glam::DVec2::new(dx, dy);
        let [a, b] = drafting_angles(isometric, iso_plane, snap_angle_deg).map(|degrees| {
            let radians = degrees.to_radians();
            glam::DVec2::new(radians.cos(), radians.sin())
        });
        let direction = if delta.dot(a).abs() >= delta.dot(b).abs() { a } else { b };
        let direction = if delta.dot(direction) < 0.0 { -direction } else { direction };
        direction.y.atan2(direction.x)
    };
    let dir_ucs = glam::DVec3::new(ang.cos(), ang.sin(), 0.0);
    let dir = xf.to_wcs(b + dir_ucs) - xf.to_wcs(b);
    (dir.length_squared() > 1e-12).then(|| dir.normalize())
}

/// Project `pt` onto the locked ray through `base` — the hard lock applies to
/// EVERYTHING, including an osnap hit, so a snap far off-axis contributes only
/// its along-axis component (#312).
pub(super) fn axis_lock_apply(
    pt: glam::DVec3,
    base: glam::DVec3,
    dir: glam::DVec3,
) -> glam::DVec3 {
    base + dir * (pt - base).dot(dir)
}

// ── Clipboard / selection helpers ──────────────────────────────────────────

/// Copy/paste anchor: the lower-left corner of the bounding box that encloses
/// every copied entity. Standard clipboard behaviour puts this corner under the
/// cursor at paste time, so the whole selection drops down-and-right of the pick.
///
/// Unions each entity's own bounding box (O(entities)) rather than averaging
/// every tessellated wire vertex, which cost O(total geometry) and stalled a
/// whole-drawing copy. The per-entity `min` corners give the exact enclosing
/// box's lower-left.
pub(super) fn entities_lower_left_by_bbox(
    doc: &acadrust::CadDocument,
    handles: &[acadrust::Handle],
) -> glam::DVec3 {
    let mut min = glam::DVec3::splat(f64::INFINITY);
    let mut any = false;
    for &h in handles {
        let Some(e) = doc.get_entity(h) else { continue };
        let bb = e.as_entity().bounding_box();
        let lo = glam::DVec3::new(bb.min.x, bb.min.y, bb.min.z);
        if lo.x.is_finite() && lo.y.is_finite() && lo.z.is_finite() {
            min = min.min(lo);
            any = true;
        }
    }
    if any {
        min
    } else {
        glam::DVec3::ZERO
    }
}

/// Generate the next available auto group name ("*A1", "*A2", …).
pub(super) fn next_group_auto_name(scene: &crate::scene::Scene) -> String {
    let existing: rustc_hash::FxHashSet<String> =
        scene.groups().map(|g| g.name.clone()).collect();
    for n in 1..=9999 {
        let name = format!("*A{n}");
        if !existing.contains(&name) {
            return name;
        }
    }
    "*A".to_string()
}

// ── Entity type labels ─────────────────────────────────────────────────────

pub(super) fn entity_type_label(entity: &acadrust::EntityType) -> String {
    crate::t!(crate::entities::names::ui_name_or_class(entity)).into_owned()
}

pub(super) fn entity_type_key(entity: &acadrust::EntityType) -> String {
    use acadrust::EntityType::*;
    match entity {
        Point(_) => "point",
        Line(_) => "line",
        Circle(_) => "circle",
        Arc(_) => "arc",
        Ellipse(_) => "ellipse",
        Spline(_) => "spline",
        Helix(_) => "helix",
        LwPolyline(_) | Polyline(_) => "pline",
        Polyline2D(_) => "pline2d",
        Polyline3D(_) => "pline3d",
        PolyfaceMesh(_) => "polyface",
        PolygonMesh(_) => "polymesh",
        Text(_) => "text",
        MText(_) => "mtext",
        Dimension(_) => "dimension",
        Leader(_) => "leader",
        MultiLeader(_) => "multileader",
        Tolerance(_) => "tolerance",
        Insert(_) => "insert",
        Block(_) => "block",
        BlockEnd(_) => "blockend",
        Hatch(_) => "hatch",
        Solid(_) => "solid",
        Face3D(_) => "face3d",
        Solid3D(_) => "solid3d",
        Region(_) => "region",
        Body(_) => "body",
        Surface(_) => "surface",
        Mesh(_) => "mesh",
        Ray(_) => "ray",
        XLine(_) => "xline",
        MLine(_) => "mline",
        Viewport(_) => "viewport",
        RasterImage(_) => "rasterimage",
        Wipeout(_) => "wipeout",
        Underlay(_) => "underlay",
        Shape(_) => "shape",
        Table(_) => "table",
        AttributeDefinition(_) => "attdef",
        AttributeEntity(_) => "attrib",
        Ole2Frame(_) => "ole2frame",
        Light(_) => "light",
        SectionSymbol(_) => "sectionsymbol",
        ViewBorder(_) => "viewborder",
        Extended(entity) => return entity.class_name().to_ascii_lowercase(),
        Seqend(_) => "seqend",
        Unknown(_) => "unknown",
    }
    .to_string()
}

pub(super) fn title_case_word(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => {
            let mut out = first.to_uppercase().collect::<String>();
            out.push_str(chars.as_str());
            out
        }
        None => String::new(),
    }
}

// ── Window icon ────────────────────────────────────────────────────────────

/// The window icon, rasterised from the same file every other build target
/// draws its icon from.
///
/// This used to redraw the mark stroke by stroke in code, with the background
/// colour written out as a literal. The taskbar therefore kept showing whatever
/// the logo used to be, however many times the logo itself was redrawn — the
/// one icon in the application that did not come from `assets/logo.svg`.
///
/// Returns 32×32 RGBA. A logo that cannot be rendered leaves the window with
/// the platform default rather than something invented here, which would put
/// this back where it started.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn build_window_icon() -> Option<Vec<u8>> {
    const W: u32 = 32;
    static LOGO: &[u8] = include_bytes!("../../assets/logo.svg");

    let tree = resvg::usvg::Tree::from_data(LOGO, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(W, W)?;
    let size = tree.size();
    // Fit the artwork to the square without distorting it, whatever aspect the
    // logo happens to have.
    let scale = (W as f32 / size.width()).min(W as f32 / size.height());
    let transform = resvg::tiny_skia::Transform::from_translate(
        (W as f32 - size.width() * scale) / 2.0,
        (W as f32 - size.height() * scale) / 2.0,
    )
    .pre_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Some(pixmap.take())
}

#[cfg(test)]
mod ucs_from_normal_tests {
    use super::ucs_from_normal;
    use glam::DVec3;

    fn axes(ucs: &acadrust::tables::Ucs) -> (DVec3, DVec3, DVec3) {
        let x = DVec3::new(ucs.x_axis.x, ucs.x_axis.y, ucs.x_axis.z);
        let y = DVec3::new(ucs.y_axis.x, ucs.y_axis.y, ucs.y_axis.z);
        (x, y, x.cross(y))
    }

    /// The contract `UCS FACE` depends on: the plane's Z is the face normal it
    /// was handed. If this drifts, sketches tilt off the face they were
    /// started on.
    #[test]
    fn z_axis_is_the_supplied_normal() {
        for normal in [
            DVec3::Z,
            DVec3::NEG_Z,
            DVec3::X,
            DVec3::Y,
            DVec3::new(1.0, 2.0, 3.0).normalize(),
        ] {
            let ucs = ucs_from_normal(DVec3::ZERO, normal).expect("a normal defines a plane");
            let (_, _, z) = axes(&ucs);
            assert!(
                (z - normal).length() < 1e-9,
                "expected Z {normal:?}, got {z:?}"
            );
        }
    }

    /// Right-handed and unit length, or every coordinate read off the plane is
    /// skewed.
    #[test]
    fn axes_are_orthonormal() {
        for normal in [DVec3::Z, DVec3::new(-2.0, 0.5, 1.0).normalize()] {
            let ucs = ucs_from_normal(DVec3::ZERO, normal).unwrap();
            let (x, y, z) = axes(&ucs);
            for (name, axis) in [("X", x), ("Y", y), ("Z", z)] {
                assert!((axis.length() - 1.0).abs() < 1e-9, "{name} is not unit");
            }
            assert!(x.dot(y).abs() < 1e-9, "X and Y are not perpendicular");
            assert!(x.dot(z).abs() < 1e-9, "X and Z are not perpendicular");
        }
    }

    /// The 1/64 branch exists so a normal along world Z does not cross-product
    /// with itself. Both sides of that branch must still produce a usable
    /// plane.
    #[test]
    fn a_normal_along_world_z_still_yields_a_plane() {
        let ucs = ucs_from_normal(DVec3::ZERO, DVec3::Z).expect("world Z is a valid normal");
        let (x, _, _) = axes(&ucs);
        assert!(x.length() > 0.5, "X degenerated next to the world Z axis");
    }

    /// The same face must always give the same X axis, or a sketch would spin
    /// each time the UCS was rebuilt from it.
    #[test]
    fn the_same_normal_is_reproducible() {
        let normal = DVec3::new(0.3, -0.7, 0.2).normalize();
        let first = axes(&ucs_from_normal(DVec3::ZERO, normal).unwrap());
        let second = axes(&ucs_from_normal(DVec3::ZERO, normal).unwrap());
        assert!((first.0 - second.0).length() < 1e-12);
    }

    /// A zero vector defines no plane; it must be refused rather than
    /// producing a degenerate UCS that silently misplaces geometry.
    #[test]
    fn a_degenerate_normal_is_rejected() {
        assert!(ucs_from_normal(DVec3::ZERO, DVec3::ZERO).is_none());
    }
}
