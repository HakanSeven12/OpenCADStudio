//! Light entity display glyph (point / spot / distant).
//!
//! Lights are nonprint helper entities: the DWG reader decodes the source
//! position and aim point (see `acadrust`'s `Light`); here we synthesize a
//! wireframe glyph so the light is visible and selectable in the editor, the
//! same way AutoCAD shows a light symbol.
//!
//! The glyph is sized in **screen space** (a roughly constant pixel size across
//! zoom) via [`relative_render`], falling back to a small fixed world size when
//! no world-per-pixel factor is available (e.g. snapshot tessellation).

use acadrust::entities::Light;
use acadrust::EntityType;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{
    center_grip, edit_angle_prop, edit_prop as edit, ro_prop, square_grip,
};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;

/// On-screen glyph radius in pixels for the screen-relative path.
const GLYPH_PX: f64 = 16.0;
/// Fixed world-space glyph radius used when no zoom factor is available.
const FIXED_WORLD: f64 = 2.0;

// ── vector helpers (f64, absolute world space) ─────────────────────────────

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-12 {
        [0.0, 0.0, 0.0]
    } else {
        [a[0] / l, a[1] / l, a[2] / l]
    }
}

fn pos(l: &Light) -> [f64; 3] {
    [l.position.x, l.position.y, l.position.z]
}
fn tgt(l: &Light) -> [f64; 3] {
    [l.target.x, l.target.y, l.target.z]
}

/// Unit aim direction (position → target), defaulting to straight down when
/// the target coincides with the source (point lights carry no meaningful aim).
fn aim(l: &Light) -> [f64; 3] {
    let d = norm(sub(tgt(l), pos(l)));
    if d == [0.0, 0.0, 0.0] {
        [0.0, 0.0, -1.0]
    } else {
        d
    }
}

/// Two unit axes spanning the plane perpendicular to `a`.
fn perp_basis(a: [f64; 3]) -> ([f64; 3], [f64; 3]) {
    let up = if a[2].abs() > 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 0.0, 1.0]
    };
    let u = norm(cross(up, a));
    let v = cross(a, u);
    (u, v)
}

const NAN: [f64; 3] = [f64::NAN, f64::NAN, f64::NAN];

/// Append a disconnected segment, NaN-separated from prior geometry.
fn push_seg(out: &mut Vec<[f64; 3]>, a: [f64; 3], b: [f64; 3]) {
    if !out.is_empty() {
        out.push(NAN);
    }
    out.push(a);
    out.push(b);
}

/// Append a closed ring of radius `r` in the (u, v) plane about `c`.
fn push_ring(out: &mut Vec<[f64; 3]>, c: [f64; 3], u: [f64; 3], v: [f64; 3], r: f64, n: usize) {
    if !out.is_empty() {
        out.push(NAN);
    }
    for i in 0..=n {
        let a = i as f64 * std::f64::consts::TAU / n as f64;
        let (ca, sa) = (a.cos() * r, a.sin() * r);
        out.push([
            c[0] + u[0] * ca + v[0] * sa,
            c[1] + u[1] * ca + v[1] * sa,
            c[2] + u[2] * ca + v[2] * sa,
        ]);
    }
}

/// Build the light glyph wire (absolute world coordinates) at radius `r`.
fn light_wire(l: &Light, r: f64) -> Vec<[f64; 3]> {
    let p = pos(l);
    let mut out: Vec<[f64; 3]> = Vec::new();
    match l.light_type {
        // ── Spot (cone / flashlight) ──────────────────────────────────────
        3 => {
            let a = aim(l);
            let (u, v) = perp_basis(a);
            let len = r * 2.4;
            let base_r = r;
            let c = add(p, scale(a, len));
            // Emitter ring at the source.
            push_ring(&mut out, p, u, v, r * 0.35, 12);
            // Cone mouth + rays from the source to the rim.
            push_ring(&mut out, c, u, v, base_r, 16);
            for k in 0..4 {
                let ang = k as f64 * std::f64::consts::FRAC_PI_2;
                let rim = add(
                    c,
                    add(scale(u, ang.cos() * base_r), scale(v, ang.sin() * base_r)),
                );
                push_seg(&mut out, p, rim);
            }
        }
        // ── Distant (sun with parallel rays) ─────────────────────────────
        1 => {
            let (u, v) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
            push_ring(&mut out, p, u, v, r * 0.6, 16);
            // A few parallel rays streaming off toward −X.
            let dir = [-1.0, 0.0, 0.0];
            for k in -1..=1 {
                let off = scale(v, k as f64 * r * 0.5);
                let a = add(add(p, off), scale(u, r * 0.6));
                let b = add(a, scale(dir, r * 1.4));
                push_seg(&mut out, a, b);
            }
        }
        // ── Point (radiant sphere) ───────────────────────────────────────
        _ => {
            push_ring(&mut out, p, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], r, 20);
            push_ring(&mut out, p, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], r, 20);
            push_ring(&mut out, p, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], r, 20);
            // Short radial ticks to read as a light source.
            for k in 0..4 {
                let ang = k as f64 * std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_4;
                let d = [ang.cos(), ang.sin(), 0.0];
                push_seg(&mut out, add(p, scale(d, r)), add(p, scale(d, r * 1.5)));
            }
        }
    }
    out
}

fn build(l: &Light, r: f64) -> RenderEntity {
    let p = pos(l);
    let snap = glam::DVec3::new(p[0], p[1], p[2]);
    let mut snap_pts = vec![(snap, SnapHint::Node)];
    let mut key = vec![p];
    if l.is_spot() {
        let t = tgt(l);
        snap_pts.push((glam::DVec3::new(t[0], t[1], t[2]), SnapHint::Node));
        key.push(t);
    }
    RenderEntity {
        pick_tris: Vec::new(),
        object: RenderObject::Lines(light_wire(l, r)),
        snap_pts,
        tangent_geoms: vec![],
        key_vertices: key,
        fill_tris: vec![],
    }
}

/// Viewport-aware glyph: constant on-screen size from the world-per-pixel
/// factor. Returns `None` for a non-light entity or when no factor is
/// available (the caller then falls back to the fixed-world [`to_render`]).
pub fn relative_render(
    entity: &EntityType,
    _document: &acadrust::CadDocument,
    wpp: Option<f32>,
) -> Option<RenderEntity> {
    let EntityType::Light(l) = entity else {
        return None;
    };
    let w = wpp.filter(|w| *w > 0.0)? as f64;
    Some(build(l, w * GLYPH_PX))
}

impl RenderConvertible for Light {
    fn to_render(&self, _document: &acadrust::CadDocument) -> Option<RenderEntity> {
        Some(build(self, FIXED_WORLD))
    }
}

impl Grippable for Light {
    fn grips(&self) -> Vec<GripDef> {
        let p = pos(self);
        let mut g = vec![square_grip(0, glam::DVec3::new(p[0], p[1], p[2]))];
        if self.is_spot() {
            let t = tgt(self);
            g.push(center_grip(1, glam::DVec3::new(t[0], t[1], t[2])));
        }
        g
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        match (grip_id, apply) {
            // Grip 0 — light source: moves the whole light (aim preserved).
            (0, GripApply::Translate(d)) => {
                self.position.x += d.x as f64;
                self.position.y += d.y as f64;
                self.position.z += d.z as f64;
                self.target.x += d.x as f64;
                self.target.y += d.y as f64;
                self.target.z += d.z as f64;
            }
            (0, GripApply::Absolute(p)) => {
                let dx = p.x as f64 - self.position.x;
                let dy = p.y as f64 - self.position.y;
                let dz = p.z as f64 - self.position.z;
                self.position.x += dx;
                self.position.y += dy;
                self.position.z += dz;
                self.target.x += dx;
                self.target.y += dy;
                self.target.z += dz;
            }
            // Grip 1 — spot target: re-aims the light.
            (1, GripApply::Translate(d)) => {
                self.target.x += d.x as f64;
                self.target.y += d.y as f64;
                self.target.z += d.z as f64;
            }
            (1, GripApply::Absolute(p)) => {
                self.target.x = p.x as f64;
                self.target.y = p.y as f64;
                self.target.z = p.z as f64;
            }
            _ => {}
        }
    }
}

impl PropertyEditable for Light {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let kind = match self.light_type {
            1 => "Distant",
            3 => "Spot",
            _ => "Point",
        };
        let mut props = vec![
            ro_prop(t!("Name").as_ref(), "li_name", self.name.clone()),
            ro_prop(t!("Type").as_ref(), "li_type", kind),
            edit(t!("Position X").as_ref(), "li_px", self.position.x),
            edit(t!("Position Y").as_ref(), "li_py", self.position.y),
            edit(t!("Position Z").as_ref(), "li_pz", self.position.z),
        ];
        if self.light_type != 2 {
            props.push(edit(t!("Target X").as_ref(), "li_tx", self.target.x));
            props.push(edit(t!("Target Y").as_ref(), "li_ty", self.target.y));
            props.push(edit(t!("Target Z").as_ref(), "li_tz", self.target.z));
        }
        let mut sections = vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props,
            },
            PropSection {
                title: t!("Light").into_owned(),
                props: vec![
                    Property {
                        label: t!("On").into_owned(),
                        field: "li_status",
                        value: PropValue::BoolToggle {
                            field: "li_status",
                            value: self.status,
                        },
                    },
                    Property {
                        label: t!("Plot Glyph").into_owned(),
                        field: "li_plot_glyph",
                        value: PropValue::BoolToggle {
                            field: "li_plot_glyph",
                            value: self.plot_glyph,
                        },
                    },
                    ro_prop(t!("Color").as_ref(), "li_color", format!("{:?}", self.light_color)),
                    edit(t!("Intensity").as_ref(), "li_intensity", self.intensity),
                    Property {
                        label: t!("Attenuation").into_owned(),
                        field: "li_attenuation",
                        value: PropValue::Choice {
                            selected: match self.attenuation_type {
                                1 => "Inverse Linear",
                                2 => "Inverse Square",
                                _ => "None",
                            }
                            .to_string(),
                            options: vec![
                                "None".to_string(),
                                "Inverse Linear".to_string(),
                                "Inverse Square".to_string(),
                            ],
                        },
                    },
                    Property {
                        label: t!("Use Limits").into_owned(),
                        field: "li_use_limits",
                        value: PropValue::BoolToggle {
                            field: "li_use_limits",
                            value: self.use_attenuation_limits,
                        },
                    },
                    edit(t!("Start Limit").as_ref(), "li_start", self.attenuation_start_limit),
                    edit(t!("End Limit").as_ref(), "li_end", self.attenuation_end_limit),
                    edit_angle_prop(t!("Hotspot Angle").as_ref(),
                        "li_hotspot",
                        self.hotspot_angle.to_degrees(),
                    ),
                    edit_angle_prop(t!("Falloff Angle").as_ref(),
                        "li_falloff",
                        self.falloff_angle.to_degrees(),
                    ),
                ],
            },
            PropSection {
                title: t!("Shadows").into_owned(),
                props: vec![
                    Property {
                        label: t!("Cast Shadows").into_owned(),
                        field: "li_shadows",
                        value: PropValue::BoolToggle {
                            field: "li_shadows",
                            value: self.cast_shadows,
                        },
                    },
                    ro_prop(t!("Type").as_ref(), "li_shadow_type", self.shadow_type.to_string()),
                    ro_prop(t!("Map Size").as_ref(), "li_shadow_size", self.shadow_map_size.to_string()),
                    ro_prop(t!("Softness").as_ref(),
                        "li_shadow_softness",
                        self.shadow_map_softness.to_string(),
                    ),
                ],
            },
        ];
        if let Some(photo) = &self.photometric_data {
            sections.push(PropSection {
                title: t!("Photometric").into_owned(),
                props: vec![
                    ro_prop(t!("Web File").as_ref(), "li_web_file", photo.web_file.clone()),
                    ro_prop(t!("Physical Method").as_ref(),
                        "li_physical_method",
                        photo.physical_intensity_method.to_string(),
                    ),
                    edit(t!("Physical Intensity").as_ref(),
                        "li_physical_intensity",
                        photo.physical_intensity,
                    ),
                    edit(t!("Illuminance Distance").as_ref(),
                        "li_illuminance_distance",
                        photo.illuminance_distance,
                    ),
                    edit(t!("Color Temperature").as_ref(),
                        "li_color_temperature",
                        photo.lamp_color_temperature,
                    ),
                    ro_prop(t!("Lamp Preset").as_ref(),
                        "li_lamp_preset",
                        photo.lamp_color_preset.to_string(),
                    ),
                    ro_prop(t!("Shape").as_ref(),
                        "li_shape",
                        format!(
                            "{} · {:.6} × {:.6} · radius {:.6}",
                            photo.extended_light_shape,
                            photo.extended_light_length,
                            photo.extended_light_width,
                            photo.extended_light_radius
                        ),
                    ),
                    ro_prop(t!("Web").as_ref(),
                        "li_web",
                        format!(
                            "type {}; symmetry {}; flux {:.6}; angles {:?}; rotation {:.6},{:.6},{:.6}",
                            photo.web_file_type,
                            photo.web_symmetry,
                            photo.web_flux,
                            photo.web_angles,
                            photo.web_rotation.x,
                            photo.web_rotation.y,
                            photo.web_rotation.z
                        ),
                    ),
                ],
            });
        }
        sections
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "li_status" => {
                if let Ok(value) = value.parse::<bool>() {
                    self.status = value;
                }
                return;
            }
            "li_plot_glyph" => {
                if let Ok(value) = value.parse::<bool>() {
                    self.plot_glyph = value;
                }
                return;
            }
            "li_use_limits" => {
                if let Ok(value) = value.parse::<bool>() {
                    self.use_attenuation_limits = value;
                }
                return;
            }
            "li_shadows" => {
                if let Ok(value) = value.parse::<bool>() {
                    self.cast_shadows = value;
                }
                return;
            }
            "li_attenuation" => {
                self.attenuation_type = match value {
                    "Inverse Linear" => 1,
                    "Inverse Square" => 2,
                    _ => 0,
                };
                return;
            }
            _ => {}
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "li_px" => self.position.x = v,
            "li_py" => self.position.y = v,
            "li_pz" => self.position.z = v,
            "li_tx" => self.target.x = v,
            "li_ty" => self.target.y = v,
            "li_tz" => self.target.z = v,
            "li_intensity" => self.intensity = v.max(0.0),
            "li_start" => self.attenuation_start_limit = v,
            "li_end" => self.attenuation_end_limit = v,
            "li_hotspot" => self.hotspot_angle = v.to_radians(),
            "li_falloff" => self.falloff_angle = v.to_radians(),
            "li_physical_intensity" => {
                if let Some(photo) = self.photometric_data.as_mut() {
                    photo.physical_intensity = v;
                }
            }
            "li_illuminance_distance" => {
                if let Some(photo) = self.photometric_data.as_mut() {
                    photo.illuminance_distance = v;
                }
            }
            "li_color_temperature" => {
                if let Some(photo) = self.photometric_data.as_mut() {
                    photo.lamp_color_temperature = v;
                }
            }
            _ => {}
        }
    }
}

impl Transformable for Light {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(
            self,
            t,
            |entity, p1, p2| {
                crate::scene::view::transform::reflect_xy_point(
                    &mut entity.position.x,
                    &mut entity.position.y,
                    p1,
                    p2,
                );
                crate::scene::view::transform::reflect_xy_point(
                    &mut entity.target.x,
                    &mut entity.target.y,
                    p1,
                    p2,
                );
            },
        );
    }
}
