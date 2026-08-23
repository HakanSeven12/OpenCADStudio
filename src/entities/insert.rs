use acadrust::entities::Insert;
use acadrust::types::{Matrix3, Transform, Vector3};
use acadrust::{EntityType, Handle};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{edit_angle_prop as edit_angle, edit_prop as edit, parse_f64, ro_prop as ro, square_grip};

use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::WireModel;
use crate::scene::cache::block_cache;
use crate::scene::convert::tessellate;
use crate::scene::view::render;

fn grips(ins: &Insert) -> Vec<GripDef> {
    // `insert_point` is in the OCS defined by `normal`; the grip must sit at
    // the world placement, so map it through the OCS. Identity for +Z.
    let w = Matrix3::arbitrary_axis(ins.normal) * ins.insert_point;
    vec![square_grip(0, glam::DVec3::new(w.x, w.y, w.z))]
}

fn properties(ins: &Insert) -> Vec<PropSection> {
    let annotative = ins
        .common
        .extended_data
        .get_record("AcAnnotativeData")
        .is_some();
    let attrs: Vec<Property> = ins
        .attributes
        .iter()
        .map(|a| Property {
            label: a.tag.clone(),
            field: "attr",
            value: PropValue::AttrText {
                tag: a.tag.clone(),
                value: a.value.clone(),
            },
        })
        .collect();
    let mut sections = vec![
        PropSection {
            title: "Geometry".into(),
            props: vec![
                edit("Position X", "ins_x", ins.insert_point.x),
                edit("Position Y", "ins_y", ins.insert_point.y),
                edit("Position Z", "ins_z", ins.insert_point.z),
                edit("Scale X", "x_scale", ins.x_scale()),
                edit("Scale Y", "y_scale", ins.y_scale()),
                edit("Scale Z", "z_scale", ins.z_scale()),
            ],
        },
        PropSection {
            title: "Misc".into(),
            props: vec![
                ro("Name", "block", ins.block_name.clone()),
                edit_angle("Rotation", "rotation", ins.rotation.to_degrees()),
                ro(
                    "Annotative",
                    "annotative",
                    if annotative { "Yes" } else { "No" },
                ),
                ro("Block Unit", "block_unit", String::new()),
                ro("Unit factor", "unit_factor", String::new()),
            ],
        },
    ];
    // The Attributes group appears only when the block actually has attributes.
    if !attrs.is_empty() {
        sections.push(PropSection {
            title: "Attributes".into(),
            props: attrs,
        });
    }
    sections
}

fn apply_geom_prop(ins: &mut Insert, field: &str, value: &str) {
    let Some(v) = parse_f64(value) else {
        return;
    };
    match field {
        "ins_x" | "ins_y" | "ins_z" => {
            // Move the attributes by the same world delta as the insertion
            // point so they follow the block instead of staying put (#255).
            let ocs = Matrix3::arbitrary_axis(ins.normal);
            let old_world = ocs * ins.insert_point;
            match field {
                "ins_x" => ins.insert_point.x = v,
                "ins_y" => ins.insert_point.y = v,
                _ => ins.insert_point.z = v,
            }
            let delta = ocs * ins.insert_point - old_world;
            for att in &mut ins.attributes {
                acadrust::Entity::translate(att, delta);
            }
        }
        // Single Scale row shown while "Uniform scale" is checked (#427).
        "u_scale" => {
            ins.set_x_scale(v);
            ins.set_y_scale(v);
            ins.set_z_scale(v);
        }
        "x_scale" => ins.set_x_scale(v),
        "y_scale" => ins.set_y_scale(v),
        "z_scale" => ins.set_z_scale(v),
        "rotation" => ins.rotation = v.to_radians(),
        _ => {}
    }
}

fn apply_grip(ins: &mut Insert, _grip_id: usize, apply: GripApply) {
    // The grip works in world space, but `insert_point` is stored in the OCS
    // defined by `normal`. Round-trip through the OCS so dragging a block
    // whose extrusion direction isn't +Z moves along world axes. Identity OCS
    // for a +Z normal, so this matches the old direct assignment there.
    let ocs = Matrix3::arbitrary_axis(ins.normal);
    let old_world = ocs * ins.insert_point;
    let world = match apply {
        GripApply::Absolute(p) => Vector3::new(p.x as f64, p.y as f64, p.z as f64),
        GripApply::Translate(d) => old_world + Vector3::new(d.x as f64, d.y as f64, d.z as f64),
    };
    ins.insert_point = ocs.transpose() * world;
    // Attributes sit in world space beside the block, so move them by the same
    // world delta — otherwise a grip-drag leaves the attribute text behind (#255).
    let delta = world - old_world;
    for att in &mut ins.attributes {
        acadrust::Entity::translate(att, delta);
    }
}

fn apply_transform(ins: &mut Insert, t: &EntityTransform) {
    crate::scene::view::transform::apply_standard_entity_transform(ins, t, |entity, p1, p2| {
        let dx = (p2.x - p1.x) as f64;
        let dy = (p2.y - p1.y) as f64;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 {
            return;
        }

        let ux = dx / len;
        let uy = dy / len;
        let mirror = acadrust::types::Matrix4 {
            m: [
                [2.0 * ux * ux - 1.0, 2.0 * ux * uy, 0.0, 0.0],
                [2.0 * ux * uy, 2.0 * uy * uy - 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let t = Transform::from_translation(Vector3::new(-(p1.x as f64), -(p1.y as f64), 0.0))
            .then(&Transform::from_matrix(mirror))
            .then(&Transform::from_translation(Vector3::new(
                p1.x as f64,
                p1.y as f64,
                0.0,
            )));
        acadrust::Entity::apply_transform(entity, &t);
    });
}

crate::impl_entity_basics!(Insert);

impl crate::entities::traits::FallbackTess for Insert {
    fn fallback_geometry(
        &self,
    ) -> crate::scene::convert::tess_util::FallbackGeometry {
        let (ipx, ipy, ipz) = (
            self.insert_point.x,
            self.insert_point.y,
            self.insert_point.z,
        );
        let ip = Vec3::new(ipx as f32, ipy as f32, ipz as f32);
        let s = 0.1_f64;
        let pts = vec![
            [ipx - s, ipy, ipz],
            [ipx + s, ipy, ipz],
            [ipx, ipy - s, ipz],
            [ipx, ipy + s, ipz],
        ];
        (
            pts,
            vec![(ip, crate::scene::model::wire_model::SnapHint::Insertion)],
            vec![],
            vec![],
        )
    }
}
pub(crate) fn append_insert_attribute_wires(
    wires: &mut Vec<WireModel>,
    document: &acadrust::CadDocument,
    ins: &acadrust::entities::Insert,
    insert_handle: Handle,
    sel: bool,
    ins_color: [f32; 4],
    ins_pat_len: f32,
    ins_pat: [f32; 8],
    ins_lw_px: f32,
    ins_layer: render::InheritStyle,
    ins_layer_plottable: bool,
    bg_color: [f32; 4],
    is_xref: bool,
    pslt_factor: f32,
    anno_scale: f32,
) {
    if ins.attributes.is_empty() {
        return;
    }
    // ATTMODE (header.attribute_visibility):
    //   0 = Off    — every attribute hidden
    //   1 = Normal — honour per-attribute `invisible` flag (default)
    //   2 = On     — every attribute forced visible, ignoring its flag
    let attmode = document.header.attribute_visibility;
    if attmode == 0 {
        return;
    }
    // Attributes inherit the annotative block scale. Pre-scale them here so
    // text tessellation stays at a neutral scale.
    let annotative = ins
        .common
        .extended_data
        .get_record("AcAnnotativeData")
        .is_some();
    let block_scale = if annotative { anno_scale as f64 } else { 1.0 };
    let ip = ins.insert_point;
    let scale_about = |q: acadrust::types::Vector3| {
        acadrust::types::Vector3::new(
            ip.x + (q.x - ip.x) * block_scale,
            ip.y + (q.y - ip.y) * block_scale,
            ip.z + (q.z - ip.z) * block_scale,
        )
    };
    for attr in &ins.attributes {
        let per_attr_hidden = attr.common.invisible || attr.flags.invisible;
        if attmode == 1 && per_attr_hidden {
            continue;
        }
        let attr_entity = EntityType::AttributeEntity({
            let mut a = attr.clone();
            if (block_scale - 1.0).abs() > 1e-6 {
                a.height *= block_scale;
                a.insertion_point = scale_about(a.insertion_point);
                a.alignment_point = scale_about(a.alignment_point);
            }
            a
        });
        let (sub_color, sub_plen, sub_pat, sub_lw_px, sub_aci) = render::render_style_for_block_sub(
            document,
            &attr_entity,
            ins_color,
            ins_pat_len,
            ins_pat,
            ins_lw_px,
            ins_layer,
        );
        let sub_color = render::adapt_to_bg(sub_color, bg_color);
        let sub_color = if is_xref && !sel {
            block_cache::fade_toward_bg(sub_color, bg_color)
        } else {
            sub_color
        };
        let attr_plottable = if render::is_effective_layer_zero(&attr.common.layer) {
            ins_layer_plottable
        } else {
            document
                .layers
                .get(&attr.common.layer)
                .map(|layer| layer.is_plottable)
                .unwrap_or(true)
        };
        let attr_plottable = ins_layer_plottable && attr_plottable;
        let sub_aabb = crate::scene::entity_aabb(&attr_entity);
        let mut attr_wires = tessellate::tessellate(
            document,
            insert_handle,
            &attr_entity,
            sel,
            sub_color,
            sub_plen * pslt_factor,
            sub_pat.map(|v| v * pslt_factor),
            sub_lw_px,
            // Neutral: the attribute is already scaled as part of the block unit
            // above, so the text path must not scale it a second time.
            1.0,
            None,
            None,
            bg_color,
            false,
        );
        // Use the INSERT's handle so selection / picking groups attribute
        // text with the parent insert instead of treating it as a stray text.
        for w in &mut attr_wires {
            w.name = insert_handle.value().to_string();
            w.aci = sub_aci;
            w.aabb = sub_aabb;
            w.plot_visible &= attr_plottable;
        }
        wires.extend(attr_wires);
    }
}
