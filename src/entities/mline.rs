use acadrust::entities::MLine;
use crate::t;

use crate::command::EntityTransform;
use crate::entities::common::{edit_prop as edit, ro_prop as ro, square_grip};
use crate::entities::traits::{Grippable, PropertyEditable, Transformable, RenderConvertible};
use crate::scene::convert::acad_to_render::{RenderEntity, RenderObject};
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::SnapHint;

/// One drawn line of a multiline: the polyline for a single style element (or
/// an end cap), tagged with the element's colour and linetype so the
/// tessellator can colour-bin and dash each one independently.
pub struct MLineLine {
    pub points: Vec<[f64; 3]>,
    pub color: acadrust::types::Color,
    pub linetype: String,
}

/// Resolve a multiline into its per-element parallel lines in WCS.
///
/// Geometry comes from the referenced MLINESTYLE (element offsets, the
/// justification shift and the entity scale) rather than a fixed ±scale/2 guess,
/// so a custom style's offsets, colours and linetypes render the way the drawing
/// intends. Falls back to a ±0.5 two-line layout only when no MLINESTYLE can be
/// resolved (e.g. the style object is missing).
pub fn mline_lines(m: &MLine, document: &acadrust::CadDocument) -> Vec<MLineLine> {
    use acadrust::entities::{MLineFlags, MLineJustification};
    use acadrust::objects::ObjectType;
    use acadrust::types::Color;

    if m.vertices.is_empty() {
        return Vec::new();
    }

    // MLINESTYLE lookup: prefer the hard-pointer handle, fall back to the name.
    let style = m
        .style_handle
        .and_then(|h| match document.objects.get(&h) {
            Some(ObjectType::MLineStyle(s)) => Some(s),
            _ => None,
        })
        .or_else(|| {
            document.objects.values().find_map(|o| match o {
                ObjectType::MLineStyle(s) if s.name.eq_ignore_ascii_case(&m.style_name) => Some(s),
                _ => None,
            })
        });

    // (offset, colour, linetype) per element.
    let elems: Vec<(f64, Color, String)> = match style {
        Some(s) if !s.elements.is_empty() => s
            .elements
            .iter()
            .map(|e| (e.offset, e.color, e.linetype.clone()))
            .collect(),
        _ => vec![
            (0.5, Color::ByLayer, "ByLayer".to_string()),
            (-0.5, Color::ByLayer, "ByLayer".to_string()),
        ],
    };

    // Justification shifts every element so the picked path runs along the top /
    // centre / bottom element of the style. (Only the fallback path needs it —
    // stored vertex parameters already bake justification in.)
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (o, _, _) in &elems {
        lo = lo.min(*o);
        hi = hi.max(*o);
    }
    let shift = match m.justification {
        MLineJustification::Top => -hi,
        MLineJustification::Bottom => -lo,
        MLineJustification::Zero => 0.0,
    };

    let scale = m.scale_factor;
    let closed = m.flags.contains(MLineFlags::CLOSED);
    let n = m.vertices.len();

    // The element's miter-space offset at vertex `vi`: the stored per-vertex
    // parameter[0] when present — it is measured ALONG THE MITER, so corner
    // vertices already carry the 1/cos(θ/2) miter lengthening and the
    // justification shift (using the flat style offset instead pinched the
    // channel at every corner and made the ends look flared). The style
    // offset × scale is only the fallback for files without parameters.
    let elem_off = |vi: usize, ei: usize| -> f64 {
        m.vertices[vi]
            .segments
            .get(ei)
            .and_then(|sg| sg.parameters.first().copied())
            .unwrap_or_else(|| (elems[ei].0 + shift) * scale)
    };
    let off_pt = |vi: usize, d: f64| -> [f64; 3] {
        let v = &m.vertices[vi];
        [
            v.position.x + v.miter.x * d,
            v.position.y + v.miter.y * d,
            v.position.z + v.miter.z * d,
        ]
    };

    let mut out: Vec<MLineLine> = Vec::with_capacity(elems.len() + 2);
    for (ei, (_, color, linetype)) in elems.iter().enumerate() {
        let mut pts: Vec<[f64; 3]> = Vec::new();
        // Walk each segment (vi → vi+1, wrapping when closed); the vertex's
        // parameters[1..] are draw-toggle distances along the element line —
        // this is how crossing/merged multilines store their gaps. An odd
        // toggle count leaves the final run open to the segment end.
        let seg_count = if closed { n } else { n.saturating_sub(1) };
        let mut pen_at_end = false;
        for k in 0..seg_count {
            let vi = k;
            let wi = (k + 1) % n;
            let a = off_pt(vi, elem_off(vi, ei));
            let b = off_pt(wi, elem_off(wi, ei));
            let seg = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let len = (seg[0] * seg[0] + seg[1] * seg[1] + seg[2] * seg[2]).sqrt();
            if len < 1e-12 {
                continue;
            }
            let dirn = [seg[0] / len, seg[1] / len, seg[2] / len];
            let at = |t: f64| -> [f64; 3] {
                [a[0] + dirn[0] * t, a[1] + dirn[1] * t, a[2] + dirn[2] * t]
            };
            let toggles: &[f64] = m
                .vertices[vi]
                .segments
                .get(ei)
                .map(|sg| sg.parameters.get(1..).unwrap_or(&[]))
                .unwrap_or(&[]);
            // Build (start, end) runs from the toggle list.
            let mut runs: Vec<(f64, f64)> = Vec::new();
            if toggles.is_empty() {
                runs.push((0.0, len));
            } else {
                let mut i = 0;
                while i < toggles.len() {
                    let start = toggles[i].clamp(0.0, len);
                    let end = toggles
                        .get(i + 1)
                        .copied()
                        .unwrap_or(len)
                        .clamp(0.0, len);
                    if end - start > 1e-9 {
                        runs.push((start, end));
                    }
                    i += 2;
                }
            }
            for (ri, (t0, t1)) in runs.iter().enumerate() {
                let continuous = pen_at_end && ri == 0 && *t0 <= 1e-9 && !pts.is_empty();
                if !continuous {
                    if !pts.is_empty() {
                        pts.push([f64::NAN; 3]);
                    }
                    pts.push(at(*t0));
                }
                pts.push(at(*t1));
            }
            pen_at_end = runs.last().is_some_and(|(_, t1)| (len - t1).abs() <= 1e-9);
        }
        if pts.len() < 2 {
            continue;
        }
        out.push(MLineLine {
            points: pts,
            color: *color,
            linetype: linetype.clone(),
        });
    }

    // End caps: a segment across the full style width at the first / last vertex,
    // drawn only when the style requests square caps, the style has width, and
    // the multiline is open.
    if let Some(s) = style {
        if !closed && n >= 2 {
            let mut cap = |vi: usize| {
                let mut dlo = f64::INFINITY;
                let mut dhi = f64::NEG_INFINITY;
                for ei in 0..elems.len() {
                    let d = elem_off(vi, ei);
                    dlo = dlo.min(d);
                    dhi = dhi.max(d);
                }
                if (dhi - dlo).abs() > 1e-9 {
                    out.push(MLineLine {
                        points: vec![off_pt(vi, dlo), off_pt(vi, dhi)],
                        color: Color::ByLayer,
                        linetype: "ByLayer".to_string(),
                    });
                }
            };
            if s.flags.start_square_cap {
                cap(0);
            }
            if s.flags.end_square_cap {
                cap(n - 1);
            }
        }
    }

    out
}

impl RenderConvertible for MLine {
    fn to_render(&self, document: &acadrust::CadDocument) -> Option<RenderEntity> {
        if self.vertices.is_empty() {
            return None;
        }

        // NaN-separated flat list of every element line (single-colour path used
        // by pick and the edit commands; the coloured render is built in
        // `tessellate`, which special-cases MLINE).
        let lines = mline_lines(self, document);
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            if i > 0 {
                pts.push([f64::NAN; 3]);
            }
            pts.extend_from_slice(&l.points);
        }

        let key_verts: Vec<[f64; 3]> = self
            .vertices
            .iter()
            .map(|v| [v.position.x, v.position.y, v.position.z])
            .collect();

        let snap_pts = self
            .vertices
            .iter()
            .map(|v| {
                (
                    glam::DVec3::new(v.position.x, v.position.y, v.position.z),
                    SnapHint::Node,
                )
            })
            .collect();

        Some(RenderEntity {
            pick_tris: Vec::new(),
            object: RenderObject::Lines(pts),
            snap_pts,
            tangent_geoms: vec![],
            key_vertices: key_verts,
            fill_tris: vec![],
        })
    }
}

impl Grippable for MLine {
    fn grips(&self) -> Vec<GripDef> {
        self.vertices
            .iter()
            .enumerate()
            .map(|(i, v)| {
                square_grip(
                    i,
                    glam::DVec3::new(v.position.x, v.position.y, v.position.z),
                )
            })
            .collect()
    }

    fn apply_grip(&mut self, grip_id: usize, apply: GripApply) {
        if let Some(v) = self.vertices.get_mut(grip_id) {
            match apply {
                GripApply::Translate(d) => {
                    v.position.x += d.x as f64;
                    v.position.y += d.y as f64;
                    v.position.z += d.z as f64;
                }
                GripApply::Absolute(p) => {
                    v.position.x = p.x as f64;
                    v.position.y = p.y as f64;
                    v.position.z = p.z as f64;
                }
            }
        }
    }

    fn grip_menu(&self, _grip_id: usize) -> Vec<crate::scene::model::object::GripMenuItem> {
        use crate::scene::model::object::{GripMenuAction, GripMenuItem};
        vec![
            GripMenuItem {
                label: "Stretch",
                action: GripMenuAction::Stretch,
            },
            GripMenuItem {
                label: "Add Vertex",
                action: GripMenuAction::AddVertex,
            },
            GripMenuItem {
                label: "Remove Vertex",
                action: GripMenuAction::RemoveVertex,
            },
        ]
    }

    fn apply_grip_menu(&mut self, grip_id: usize, action: crate::scene::model::object::GripMenuAction) {
        use crate::scene::model::object::GripMenuAction as A;
        let n = self.vertices.len();
        match action {
            A::AddVertex if grip_id < n => {
                let i1 = (grip_id + 1).min(n - 1);
                if i1 == grip_id {
                    return;
                }
                let v0 = &self.vertices[grip_id];
                let v1 = &self.vertices[i1];
                let mut new_v = v0.clone();
                new_v.position.x = (v0.position.x + v1.position.x) * 0.5;
                new_v.position.y = (v0.position.y + v1.position.y) * 0.5;
                new_v.position.z = (v0.position.z + v1.position.z) * 0.5;
                self.vertices.insert(i1, new_v);
            }
            A::RemoveVertex if grip_id < n && n > 2 => {
                self.vertices.remove(grip_id);
            }
            _ => {}
        }
    }
}

impl PropertyEditable for MLine {
    fn geometry_properties(&self, _text_style_names: &[String]) -> Vec<PropSection> {
        let just_str = match self.justification {
            acadrust::entities::MLineJustification::Top => "Top",
            acadrust::entities::MLineJustification::Zero => "Zero",
            acadrust::entities::MLineJustification::Bottom => "Bottom",
        };
        let cur = self.vertices.first();
        let cur_x = cur.map(|v| v.position.x).unwrap_or(0.0);
        let cur_y = cur.map(|v| v.position.y).unwrap_or(0.0);
        let cur_z = cur.map(|v| v.position.z).unwrap_or(0.0);
        vec![
            PropSection {
                title: t!("Geometry").into_owned(),
                props: vec![
                    ro(t!("Vertex").as_ref(), "ml_vertex", if self.vertices.is_empty() { String::new() } else { "1".to_string() }),
                    edit(t!("Vertex X").as_ref(), "ml_vertex_x", cur_x),
                    edit(t!("Vertex Y").as_ref(), "ml_vertex_y", cur_y),
                    edit(t!("Vertex Z").as_ref(), "ml_vertex_z", cur_z),
                ],
            },
            PropSection {
                title: t!("Misc").into_owned(),
                props: vec![
                    Property {
                        label: t!("Style").into_owned(),
                        field: "ml_style",
                        value: PropValue::EditText(self.style_name.clone()),
                    },
                    Property {
                        label: t!("Style justification").into_owned(),
                        field: "ml_justification",
                        value: PropValue::Choice {
                            selected: just_str.to_string(),
                            options: ["Top", "Zero", "Bottom"]
                                .into_iter()
                                .map(str::to_string)
                                .collect(),
                        },
                    },
                    edit(t!("Style scale").as_ref(), "ml_scale", self.scale_factor),
                ],
            },
        ]
    }

    fn apply_geom_prop(&mut self, field: &str, value: &str) {
        match field {
            "ml_closed" => {
                let closed = if value == "toggle" {
                    !self.flags.contains(acadrust::entities::MLineFlags::CLOSED)
                } else {
                    value == "true"
                };
                self.flags
                    .set(acadrust::entities::MLineFlags::CLOSED, closed);
                return;
            }
            "ml_justification" => {
                self.justification = match value {
                    "Top" => acadrust::entities::MLineJustification::Top,
                    "Bottom" => acadrust::entities::MLineJustification::Bottom,
                    _ => acadrust::entities::MLineJustification::Zero,
                };
                return;
            }
            "ml_style" => {
                self.style_name = value.to_string();
                return;
            }
            _ => {}
        }
        let Ok(v) = value.trim().parse::<f64>() else {
            return;
        };
        match field {
            "ml_scale" if v != 0.0 => self.scale_factor = v,
            "ml_vertex_x" => {
                if let Some(vx) = self.vertices.first_mut() {
                    vx.position.x = v;
                }
            }
            "ml_vertex_y" => {
                if let Some(vx) = self.vertices.first_mut() {
                    vx.position.y = v;
                }
            }
            "ml_vertex_z" => {
                if let Some(vx) = self.vertices.first_mut() {
                    vx.position.z = v;
                }
            }
            _ => {}
        }
    }
}

impl Transformable for MLine {
    fn apply_transform(&mut self, t: &EntityTransform) {
        crate::scene::view::transform::apply_standard_entity_transform(self, t, |entity, p1, p2| {
            for v in &mut entity.vertices {
                crate::scene::view::transform::reflect_xy_point(
                    &mut v.position.x,
                    &mut v.position.y,
                    p1,
                    p2,
                );
            }
            crate::scene::view::transform::reflect_xy_point(
                &mut entity.start_point.x,
                &mut entity.start_point.y,
                p1,
                p2,
            );
        });
    }
}
