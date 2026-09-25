// Underlay entity — PDF/DWF/DGN reference.
//
// Render: the visible outline (clip polygon, or the page frame) with a pick
//         surface over it; a cross at the insertion when the page is unknown.
// Grips:  insertion point + clip boundary vertices.
// Props:  General → Underlay Adjust → Geometry → Misc, as the reference lists
//         a PDF underlay. Rows that need the definition (name, page, path,
//         width, height) are filled in by the Properties panel.

use codec::entities::{Underlay, UnderlayDisplayFlags};
use crate::t;
use glam::DVec3;

use crate::command::EntityTransform;
use crate::entities::common::{center_grip, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn v3(v: &codec::types::Vector3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Small cross marker at the insertion point (used when the page is unknown).
fn cross_wire(origin: [f64; 3], size: f64) -> Vec<[f64; 3]> {
    let [ox, oy, oz] = origin;
    vec![
        [ox - size, oy, oz],
        [ox + size, oy, oz],
        [f64::NAN; 3],
        [ox, oy - size, oz],
        [ox, oy + size, oz],
    ]
}

/// The underlay's definition, when the handle resolves to one.
pub(crate) fn definition<'a>(
    u: &Underlay,
    document: &'a codec::CadDocument,
) -> Option<&'a codec::entities::UnderlayDefinition> {
    match document.objects.get(&u.definition_handle) {
        Some(codec::objects::ObjectType::UnderlayDefinition(d)) => Some(d),
        _ => None,
    }
}

/// Page number of a definition ("1" when unset).
pub(crate) fn page_of(def: &codec::entities::UnderlayDefinition) -> &str {
    if def.page_name.trim().is_empty() {
        "1"
    } else {
        def.page_name.trim()
    }
}

/// Size of the referenced page in underlay units (page inches), when the
/// definition resolves and its PDF page can be read.
pub(crate) fn page_size(u: &Underlay, document: &codec::CadDocument) -> Option<(f64, f64)> {
    let def = definition(u, document)?;
    if !matches!(def.underlay_type, codec::entities::UnderlayType::Pdf) {
        return None;
    }
    crate::scene::model::pdf_raster::page_size_inches(&def.file_path, page_of(def))
}

/// The name a definition shows: its own name, or "<file> - <page>" as the
/// reference names a PDF definition.
pub(crate) fn definition_display_name(def: &codec::entities::UnderlayDefinition) -> String {
    if !def.name.trim().is_empty() {
        return def.name.clone();
    }
    let stem = std::path::Path::new(&def.file_path.replace('\\', "/"))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    format!("{stem} - {}", page_of(def))
}

/// A stored path as the platform writes it (backslashes on Windows).
pub(crate) fn display_path(path: &str) -> String {
    if cfg!(windows) {
        path.replace('/', "\\")
    } else {
        path.to_string()
    }
}

/// Clip polygon in underlay units (page inches). Two stored vertices are the
/// opposite corners of a rectangle.
pub(crate) fn clip_polygon_local(u: &Underlay) -> Vec<[f64; 2]> {
    let v = &u.clip_boundary_vertices;
    if v.len() == 2 {
        let (a, b) = (&v[0], &v[1]);
        return vec![[a.x, a.y], [b.x, a.y], [b.x, b.y], [a.x, b.y]];
    }
    v.iter().map(|p| [p.x, p.y]).collect()
}

/// Underlay units → world, through scale, rotation and insertion.
pub(crate) fn local_to_world(u: &Underlay, p: [f64; 2]) -> [f64; 3] {
    let (c, s) = (u.rotation.cos(), u.rotation.sin());
    let x = p[0] * u.x_scale;
    let y = p[1] * u.y_scale;
    let o = &u.insertion_point;
    [o.x + x * c - y * s, o.y + x * s + y * c, o.z]
}

/// Whether a clip boundary limits what is shown.
pub(crate) fn is_clipped(u: &Underlay) -> bool {
    u.flags.contains(UnderlayDisplayFlags::CLIPPING) && clip_polygon_local(u).len() >= 3
}

/// Page frame in world space (CCW from the insertion).
fn page_quad(u: &Underlay, document: &codec::CadDocument) -> Option<[[f64; 3]; 4]> {
    let (w, h) = page_size(u, document)?;
    Some([
        local_to_world(u, [0.0, 0.0]),
        local_to_world(u, [w, 0.0]),
        local_to_world(u, [w, h]),
        local_to_world(u, [0.0, h]),
    ])
}

/// Local-space extent of what is shown: the clip polygon's box when clipped
/// to its inside, else the page.
fn shown_local_extent(u: &Underlay, document: &codec::CadDocument) -> Option<(f64, f64)> {
    if is_clipped(u) && !u.clip_inverted {
        let clip = clip_polygon_local(u);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for p in &clip {
            lo = [lo[0].min(p[0]), lo[1].min(p[1])];
            hi = [hi[0].max(p[0]), hi[1].max(p[1])];
        }
        return Some((hi[0] - lo[0], hi[1] - lo[1]));
    }
    page_size(u, document)
}

/// Width and height the Properties panel shows: the shown extent times the
/// scale (the clip's size once clipped, as the reference reports it).
pub(crate) fn shown_size(u: &Underlay, document: &codec::CadDocument) -> Option<(f64, f64)> {
    shown_local_extent(u, document).map(|(w, h)| (w * u.x_scale.abs(), h * u.y_scale.abs()))
}

/// World bounds of the underlay: the clip polygon when clipped to its
/// inside, else the page frame. `None` when the page is unknown.
pub(crate) fn world_bounds(
    u: &Underlay,
    document: &codec::CadDocument,
) -> Option<([f64; 3], [f64; 3])> {
    let outline: Vec<[f64; 3]> = if is_clipped(u) && !u.clip_inverted {
        clip_polygon_local(u).into_iter().map(|p| local_to_world(u, p)).collect()
    } else {
        page_quad(u, document)?.to_vec()
    };
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for p in outline {
        for k in 0..3 {
            min[k] = min[k].min(p[k]);
            max[k] = max[k].max(p[k]);
        }
    }
    Some((min, max))
}

/// A number as the reference Properties palette shows underlay values: up to
/// four decimals, trailing zeros dropped.
pub(crate) fn plain_number(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    if text == "-0" { "0".to_string() } else { text }
}

fn number_row(label: &str, field: &'static str, value: f64) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::EditText(plain_number(value)),
    }
}

fn yes_no(label: &str, field: &'static str, flag: bool) -> Property {
    Property {
        label: label.into(),
        field,
        value: PropValue::Choice {
            selected: if flag { t!("Yes") } else { t!("No") }.into_owned(),
            options: vec![t!("Yes").into_owned(), t!("No").into_owned()],
        },
    }
}

/// A Yes/No list value (localized label, the English word, or a toggle).
fn parse_flag(value: &str, current: bool) -> bool {
    let v = value.trim();
    if v == "toggle" {
        return !current;
    }
    v == t!("Yes").as_ref() || v.eq_ignore_ascii_case("yes") || v.eq_ignore_ascii_case("true")
}

/// Checks a value typed into an underlay row before it is written. The
/// messages are the reference's.
pub(crate) fn validate_property(field: &str, value: &str) -> Result<(), &'static str> {
    let number = || value.trim().parse::<f64>().ok();
    match field {
        "ul_contrast" | "ul_fade" => match number() {
            Some(v) if (0.0..=100.0).contains(&v) && v.fract() == 0.0 => Ok(()),
            _ => Err("Value must be an integer between 0 and 100"),
        },
        "ul_scale" | "ul_width" | "ul_height" => match number() {
            Some(v) if v > 0.0 => Ok(()),
            _ => Err("Value must be positive and nonzero."),
        },
        _ => Ok(()),
    }
}

/// Turns a Width / Height write into the scale that produces it, since the
/// size itself is the page (or clip) size times the scale.
pub(crate) fn size_to_scale(
    u: &Underlay,
    document: &codec::CadDocument,
    field: &str,
    value: &str,
) -> Option<String> {
    let v = value.trim().parse::<f64>().ok()?;
    let (w, h) = shown_local_extent(u, document)?;
    let base = match field {
        "ul_width" => w,
        "ul_height" => h,
        _ => return None,
    };
    (base > 0.0).then(|| (v / base).to_string())
}

// ── RenderConvertible ──────────────────────────────────────────────────────────

impl RenderConvertible for Underlay {
    fn to_render(&self, document: &codec::CadDocument) -> Option<RenderEntity> {
        let origin = v3(&self.insertion_point);
        let insertion_snap = (
            DVec3::new(self.insertion_point.x, self.insertion_point.y, self.insertion_point.z),
            SnapHint::Insertion,
        );
        let quad = page_quad(self, document);
        let clip: Vec<[f64; 3]> = if is_clipped(self) {
            clip_polygon_local(self)
                .into_iter()
                .map(|p| local_to_world(self, p))
                .collect()
        } else {
            Vec::new()
        };

        // The outline is the clip boundary when clipped (plus the page frame
        // for an outside clip), else the page frame.
        let mut pts: Vec<[f64; 3]> = Vec::new();
        let ring = |poly: &[[f64; 3]], pts: &mut Vec<[f64; 3]>| {
            if !pts.is_empty() {
                pts.push([f64::NAN; 3]);
            }
            pts.extend_from_slice(poly);
            pts.push(poly[0]);
        };
        if !clip.is_empty() {
            ring(&clip, &mut pts);
            if self.clip_inverted {
                if let Some(q) = quad {
                    ring(&q, &mut pts);
                }
            }
        } else if let Some(q) = quad {
            ring(&q, &mut pts);
        }
        if pts.is_empty() {
            return Some(RenderEntity {
                pick_tris: Vec::new(),
                object: RenderObject::Lines(cross_wire(origin, 1.0)),
                snap_pts: vec![insertion_snap],
                tangent_geoms: vec![],
                key_vertices: vec![origin],
                fill_tris: vec![],
            });
        }

        // Pick surface over the shown area so a click inside selects it.
        let pick_ring: Vec<[f64; 3]> = if !clip.is_empty() && !self.clip_inverted {
            clip.clone()
        } else {
            quad.map(|q| q.to_vec()).unwrap_or_default()
        };
        let pick_tris = crate::entities::mesh::triangulate_planar(&pick_ring);
        let mut snap_pts = vec![insertion_snap];
        snap_pts.extend(crate::scene::model::pdf_vector::underlay_snap_points(self, document));
        Some(RenderEntity {
            pick_tris,
            object: RenderObject::Lines(pts),
            snap_pts,
            tangent_geoms: vec![],
            key_vertices: pick_ring,
            fill_tris: vec![],
        })
    }
}

// ── Grippable ─────────────────────────────────────────────────────────────────

impl Grippable for Underlay {
    fn grips(&self) -> Vec<GripDef> {
        let origin = glam::DVec3::new(
            self.insertion_point.x,
            self.insertion_point.y,
            self.insertion_point.z,
        );
        let mut grips = vec![square_grip(0, origin)];

        if !self.clip_boundary_vertices.is_empty() {
            let world_verts = self.world_clip_boundary();
            for (i, v) in world_verts.iter().enumerate() {
                grips.push(center_grip(i + 1, glam::DVec3::new(v.x, v.y, v.z)));
            }
        }

        grips
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if grip_id == 0 {
            // Insertion point grip.
            match apply {
                GripApply::Translate(d) => {
                    self.insertion_point.x += d.x as f64;
                    self.insertion_point.y += d.y as f64;
                    self.insertion_point.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    self.insertion_point.x = p.x as f64;
                    self.insertion_point.y = p.y as f64;
                    self.insertion_point.z = p.z as f64;
                }
            }
        } else {
            // Clip boundary vertex grip (grip_id = vertex_index + 1).
            let idx = grip_id - 1;
            if idx >= self.clip_boundary_vertices.len() {
                return;
            }
            // Clip boundary vertices are in local (underlay) space.
            // We need to invert the world transform to apply the grip.
            let cos_r = self.rotation.cos();
            let sin_r = self.rotation.sin();
            let new_world = match apply {
                GripApply::Absolute(p) => {
                    // world → local: translate, un-rotate, un-scale
                    let wx = p.x as f64 - self.insertion_point.x;
                    let wy = p.y as f64 - self.insertion_point.y;
                    let lx = (wx * cos_r + wy * sin_r) / self.x_scale.max(1e-10);
                    let ly = (-wx * sin_r + wy * cos_r) / self.y_scale.max(1e-10);
                    (lx, ly)
                }
                GripApply::Translate(d) => {
                    let v = &self.clip_boundary_vertices[idx];
                    let wx = d.x as f64 / self.x_scale.max(1e-10);
                    let wy = d.y as f64 / self.y_scale.max(1e-10);
                    let lx = wx * cos_r + wy * sin_r;
                    let ly = -wx * sin_r + wy * cos_r;
                    (v.x + lx, v.y + ly)
                }
            };
            self.clip_boundary_vertices[idx].x = new_world.0;
            self.clip_boundary_vertices[idx].y = new_world.1;
        }
    }
}

// ── PropertyEditable ──────────────────────────────────────────────────────────

impl PropertyEditable for Underlay {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let show = self.flags.contains(UnderlayDisplayFlags::ON);
        let clipping = self.flags.contains(UnderlayDisplayFlags::CLIPPING);
        let monochrome = self.flags.contains(UnderlayDisplayFlags::MONOCHROME);
        let adjust_bg = self.flags.contains(UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND);

        vec![
            PropSection {
                title: t!("Underlay Adjust").into_owned(),
                props: vec![
                    number_row(t!("Contrast").as_ref(), "ul_contrast", self.contrast as f64),
                    number_row(t!("Fade").as_ref(), "ul_fade", self.fade as f64),
                    yes_no(t!("Monochrome").as_ref(), "ul_mono", monochrome),
                    yes_no(t!("Adjust colors for background").as_ref(), "ul_adjust_bg", adjust_bg),
                ],
            },
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    number_row(t!("Position X").as_ref(), "ul_ix", self.insertion_point.x),
                    number_row(t!("Position Y").as_ref(), "ul_iy", self.insertion_point.y),
                    number_row(t!("Position Z").as_ref(), "ul_iz", self.insertion_point.z),
                    number_row(t!("Rotation").as_ref(), "ul_rot", self.rotation.to_degrees()),
                    // Filled from the page size by the Properties panel.
                    number_row(t!("Width").as_ref(), "ul_width", 0.0),
                    number_row(t!("Height").as_ref(), "ul_height", 0.0),
                    number_row(t!("Scale").as_ref(), "ul_scale", self.x_scale),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    // Name, page and path live on the UnderlayDefinition and
                    // are filled in by the Properties panel.
                    ro(t!("Name").as_ref(), "ul_name", String::new()),
                    ro(t!("Page number").as_ref(), "ul_page", String::new()),
                    ro(t!("Saved Path").as_ref(), "ul_path", String::new()),
                    yes_no(t!("Show underlay").as_ref(), "ul_on", show),
                    yes_no(t!("Show clipped").as_ref(), "ul_clip", clipping),
                    ro(
                        t!("Layer display overrides").as_ref(),
                        "ul_layers",
                        if crate::scene::model::pdf_layers::hidden_layers(self).is_empty() {
                            t!("None").into_owned()
                        } else {
                            t!("Applied").into_owned()
                        },
                    ),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "ul_mono" => {
                let on = parse_flag(value, self.flags.contains(UnderlayDisplayFlags::MONOCHROME));
                self.set_monochrome(on);
                return;
            }
            "ul_adjust_bg" => {
                let on = parse_flag(
                    value,
                    self.flags.contains(UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND),
                );
                self.flags.set(UnderlayDisplayFlags::ADJUST_FOR_BACKGROUND, on);
                return;
            }
            "ul_on" => {
                let on = parse_flag(value, self.flags.contains(UnderlayDisplayFlags::ON));
                self.set_on(on);
                return;
            }
            "ul_clip" => {
                let on = parse_flag(value, self.flags.contains(UnderlayDisplayFlags::CLIPPING));
                self.flags.set(UnderlayDisplayFlags::CLIPPING, on);
                return;
            }
            _ => {}
        }
        if validate_property(field, value).is_err() {
            return;
        }
        let Some(v) = crate::entities::common::parse_f64(value) else {
            return;
        };
        match field {
            "ul_ix" => self.insertion_point.x = v,
            "ul_iy" => self.insertion_point.y = v,
            "ul_iz" => self.insertion_point.z = v,
            "ul_scale" => self.set_scale(v),
            "ul_rot" => self.rotation = v.to_radians(),
            "ul_contrast" => self.set_contrast(v as u8),
            "ul_fade" => self.set_fade(v as u8),
            _ => {}
        }
    }
}

// ── Transformable ─────────────────────────────────────────────────────────────

impl Transformable for Underlay {
    fn apply_transform(&mut self, t: &EntityTransform) {
        use crate::scene::view::transform::reflect_xy_point;
        match t {
            EntityTransform::Translate(d) => {
                self.insertion_point.x += d.x as f64;
                self.insertion_point.y += d.y as f64;
                self.insertion_point.z += d.z as f64;
            }
            EntityTransform::Mirror { p1, p2, working_normal } => {
                if !working_normal.normalize_or(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-10) {
                    codec::Entity::apply_transform(
                        self,
                        &crate::scene::view::transform::reflection_about_working_line(
                            *p1,
                            *p2,
                            *working_normal,
                        ),
                    );
                    return;
                }
                reflect_xy_point(
                    &mut self.insertion_point.x,
                    &mut self.insertion_point.y,
                    *p1,
                    *p2,
                );
                // Reflect rotation angle.
                let dx = (p2.x - p1.x) as f64;
                let dy = (p2.y - p1.y) as f64;
                let axis_angle = dy.atan2(dx);
                self.rotation = 2.0 * axis_angle - self.rotation;
            }
            EntityTransform::Scale { center, factor } => {
                let bx = center.x as f64;
                let by = center.y as f64;
                let bz = center.z as f64;
                let f = *factor as f64;
                self.insertion_point.x = bx + (self.insertion_point.x - bx) * f;
                self.insertion_point.y = by + (self.insertion_point.y - by) * f;
                self.insertion_point.z = bz + (self.insertion_point.z - bz) * f;
                self.x_scale *= f;
                self.y_scale *= f;
                self.z_scale *= f;
            }
            EntityTransform::Rotate { center, axis, angle_rad } => {
                if !axis.normalize_or(DVec3::Z).abs_diff_eq(DVec3::Z, 1e-10) {
                    crate::scene::view::transform::apply_standard_transform(
                        self,
                        *center,
                        *axis,
                        *angle_rad,
                    );
                    return;
                }
                let bx = center.x as f64;
                let by = center.y as f64;
                let a = *angle_rad as f64;
                let cos_a = a.cos();
                let sin_a = a.sin();
                let dx = self.insertion_point.x - bx;
                let dy = self.insertion_point.y - by;
                self.insertion_point.x = bx + dx * cos_a - dy * sin_a;
                self.insertion_point.y = by + dx * sin_a + dy * cos_a;
                self.rotation += a;
            }
            EntityTransform::Affine(transform) => {
                codec::Entity::apply_transform(self, transform);
            }
        }
    }
}
