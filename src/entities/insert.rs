use acadrust::entities::Insert;
use acadrust::types::{Matrix3, Transform, Vector3};
use acadrust::{CadDocument, EntityType, Handle};
use glam::Vec3;

use crate::command::EntityTransform;
use crate::entities::common::{
    edit_angle_prop as edit_angle, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::Grippable;

use crate::scene::cache::block_cache;
use crate::scene::convert::tessellate;
use crate::scene::model::object::{GripApply, GripDef, PropSection, PropValue, Property};
use crate::scene::model::wire_model::WireModel;
use crate::scene::view::render;

/// Grip ids from here up address the INSERT's attributes — `BASE + i` is
/// `attributes[i]`. Grip 0 stays the block's own insertion point.
const ATTRIBUTE_GRIP_BASE: usize = 1;

/// Whether an attribute is one the user can pick up and reposition. A constant
/// attribute belongs to the block definition rather than this insert, and an
/// invisible one is not drawn, so neither has a grip to offer. The attribute
/// itself declines a locked position.
fn attribute_is_movable(att: &acadrust::entities::AttributeEntity) -> bool {
    !att.flags.constant && !att.flags.invisible
}

fn attribute_is_render_visible(
    document: &CadDocument,
    visibility: i16,
    attribute: &acadrust::entities::AttributeEntity,
) -> bool {
    let layer_visible = document
        .layers
        .get(&attribute.common.layer)
        .is_none_or(|layer| !layer.flags.off && !layer.flags.frozen);
    layer_visible
        && visibility != 0
        && (visibility == 2 || (!attribute.common.invisible && !attribute.flags.invisible))
}

fn insert_attribute_block_scale(
    insert: &Insert,
    annotation_scale: f32,
    scale_policy: crate::scene::BlockScalePolicy,
) -> f64 {
    if scale_policy == crate::scene::BlockScalePolicy::FromInsert
        && insert
            .common
            .extended_data
            .get_record("AcAnnotativeData")
            .is_some()
    {
        annotation_scale as f64
    } else {
        1.0
    }
}

fn scale_attribute_for_insert(
    insert: &Insert,
    attribute: &mut acadrust::entities::AttributeEntity,
    block_scale: f64,
) {
    if (block_scale - 1.0).abs() <= 1.0e-6 {
        return;
    }
    let insertion = insert.insert_point;
    let scale_about = |point: Vector3| {
        Vector3::new(
            insertion.x + (point.x - insertion.x) * block_scale,
            insertion.y + (point.y - insertion.y) * block_scale,
            insertion.z + (point.z - insertion.z) * block_scale,
        )
    };
    attribute.height *= block_scale;
    attribute.insertion_point = scale_about(attribute.insertion_point);
    attribute.alignment_point = scale_about(attribute.alignment_point);
}

fn grips(ins: &Insert) -> Vec<GripDef> {
    // `insert_point` is in the OCS defined by `normal`; the grip must sit at
    // the world placement, so map it through the OCS. Identity for +Z.
    let w = Matrix3::arbitrary_axis(ins.normal) * ins.insert_point;
    let mut out = vec![square_grip(0, glam::DVec3::new(w.x, w.y, w.z))];
    // One grip per attribute, so an attribute can be moved off the place the
    // block definition put it without dragging the whole block (#1259).
    // Attributes are stored beside the block in world space — the same space
    // `apply_grip` already translates them in.
    out.extend(ins.attributes.iter().enumerate().filter_map(|(i, att)| {
        if !attribute_is_movable(att) {
            return None;
        }
        let grip = Grippable::grips(att).into_iter().next()?;
        Some(GripDef {
            id: ATTRIBUTE_GRIP_BASE + i,
            ..grip
        })
    }));
    out
}

pub(crate) fn visible_attribute_grips(
    document: &CadDocument,
    insert: &Insert,
    annotation_scale: f32,
) -> Vec<GripDef> {
    let w = Matrix3::arbitrary_axis(insert.normal) * insert.insert_point;
    let mut out = vec![square_grip(0, glam::DVec3::new(w.x, w.y, w.z))];
    let visibility = document.header.attribute_visibility;
    let block_scale = insert_attribute_block_scale(
        insert,
        annotation_scale,
        crate::scene::BlockScalePolicy::FromInsert,
    );
    out.extend(
        insert
            .attributes
            .iter()
            .enumerate()
            .filter_map(|(i, attribute)| {
                if attribute.flags.constant
                    || attribute.lock_position
                    || attribute.flags.locked_position
                    || !attribute_is_render_visible(document, visibility, attribute)
                {
                    return None;
                }
                let mut attribute = attribute.clone();
                scale_attribute_for_insert(insert, &mut attribute, block_scale);
                let grip = Grippable::grips(&attribute).into_iter().next()?;
                Some(GripDef {
                    id: ATTRIBUTE_GRIP_BASE + i,
                    ..grip
                })
            }),
    );
    out
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

fn apply_grip(ins: &mut Insert, grip_id: usize, apply: GripApply) {
    // An attribute grip moves that attribute alone; the block stays put.
    if let Some(index) = grip_id.checked_sub(ATTRIBUTE_GRIP_BASE) {
        if let Some(att) = ins.attributes.get_mut(index).filter(|a| attribute_is_movable(a)) {
            Grippable::apply_grip(att, 0, apply);
        }
        return;
    }
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
pub(crate) fn insert_attribute_entities(
    document: &acadrust::CadDocument,
    insert: &acadrust::entities::Insert,
    annotation_scale: f32,
    scale_policy: crate::scene::BlockScalePolicy,
) -> Vec<EntityType> {
    let visibility = document.header.attribute_visibility;
    if visibility == 0 || insert.attributes.is_empty() {
        return Vec::new();
    }
    let block_scale = insert_attribute_block_scale(insert, annotation_scale, scale_policy);
    insert
        .attributes
        .iter()
        .filter(|attribute| attribute_is_render_visible(document, visibility, attribute))
        .map(|attribute| {
            let mut attribute = attribute.clone();
            scale_attribute_for_insert(insert, &mut attribute, block_scale);
            EntityType::AttributeEntity(attribute)
        })
        .collect()
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
    let attributes = insert_attribute_entities(
        document,
        ins,
        anno_scale,
        crate::scene::BlockScalePolicy::FromInsert,
    );
    for offset in crate::scene::render_graph::array_offsets(ins) {
        let delta = crate::scene::render_graph::insert_instance_translation_delta(
            document,
            ins,
            offset,
            anno_scale,
            crate::scene::BlockScalePolicy::FromInsert,
        );
        for source in &attributes {
            let mut attr_entity = source.clone();
            if delta != Vector3::ZERO {
                attr_entity.apply_transform(&Transform::from_translation(delta));
            }
            let attr = attr_entity.common();
            let (sub_color, sub_plen, sub_pat, sub_lw_px, sub_aci) =
                render::render_style_for_block_sub(
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
            let attr_plottable = if render::is_effective_layer_zero(&attr.layer) {
                ins_layer_plottable
            } else {
                document
                    .layers
                    .get(&attr.layer)
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
                1.0,
                None,
                None,
                bg_color,
                false,
            );
            for w in &mut attr_wires {
                w.name = insert_handle.value().to_string();
                w.aci = sub_aci;
                w.aabb = sub_aabb;
                w.plot_visible &= attr_plottable;
            }
            wires.extend(attr_wires);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acadrust::entities::AttributeEntity;
    use acadrust::tables::Layer;
    use acadrust::xdata::ExtendedDataRecord;

    fn attribute(tag: &str, x: f64, y: f64) -> AttributeEntity {
        let mut att = AttributeEntity::new(tag.to_string(), format!("{tag}-value"));
        att.insertion_point = Vector3::new(x, y, 0.0);
        att.alignment_point = Vector3::new(x, y, 0.0);
        att
    }

    fn block_with(attributes: Vec<AttributeEntity>) -> Insert {
        let mut ins = Insert::new("BLOCK", Vector3::new(0.0, 0.0, 0.0));
        ins.attributes = attributes;
        ins
    }

    fn document_with_layers(layers: Vec<Layer>) -> CadDocument {
        let mut document = CadDocument::new();
        for layer in layers {
            document.layers.add_or_replace(layer);
        }
        document
    }

    /// #1259: selecting a block showed only the insertion grip, so there was no
    /// handle to pick an attribute up by and nowhere to drop it.
    #[test]
    fn each_attribute_offers_its_own_grip() {
        let ins = block_with(vec![attribute("TAG_A", 3.0, 4.0), attribute("TAG_B", 6.0, 8.0)]);

        let g = grips(&ins);
        assert_eq!(g.len(), 3, "insertion point plus one grip per attribute");
        assert_eq!(g[0].world, glam::DVec3::new(0.0, 0.0, 0.0));
        assert_eq!(g[1].world, glam::DVec3::new(3.0, 4.0, 0.0));
        assert_eq!(g[2].world, glam::DVec3::new(6.0, 8.0, 0.0));
    }

    /// Dragging an attribute grip moves that attribute and nothing else — the
    /// block and its other attributes stay where they were.
    #[test]
    fn attribute_grip_moves_only_that_attribute() {
        let mut ins = block_with(vec![attribute("TAG_A", 3.0, 4.0), attribute("TAG_B", 6.0, 8.0)]);

        let moved = grips(&ins)[1].id;
        apply_grip(&mut ins, moved, GripApply::Absolute(glam::DVec3::new(30.0, 40.0, 0.0)));

        assert_eq!(ins.insert_point, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(ins.attributes[0].insertion_point, Vector3::new(30.0, 40.0, 0.0));
        assert_eq!(ins.attributes[0].alignment_point, Vector3::new(30.0, 40.0, 0.0));
        assert_eq!(ins.attributes[1].insertion_point, Vector3::new(6.0, 8.0, 0.0));
    }

    /// The insertion grip keeps carrying the attributes along with the block,
    /// which is what the attribute grips are an alternative to, not a
    /// replacement for (#255).
    #[test]
    fn insertion_grip_still_carries_the_attributes() {
        let mut ins = block_with(vec![attribute("TAG_A", 3.0, 4.0)]);

        apply_grip(&mut ins, 0, GripApply::Translate(glam::DVec3::new(10.0, 0.0, 0.0)));

        assert_eq!(ins.insert_point, Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(ins.attributes[0].insertion_point, Vector3::new(13.0, 4.0, 0.0));
    }

    /// A constant attribute belongs to the block definition and an invisible
    /// one is not drawn, so neither gets a grip — and a grip id must still
    /// address the attribute it was built from once some are skipped.
    #[test]
    fn skipped_attributes_neither_grip_nor_shift_the_others() {
        let mut constant = attribute("TAG_CONST", 1.0, 1.0);
        constant.flags.constant = true;
        let mut invisible = attribute("TAG_HIDDEN", 2.0, 2.0);
        invisible.flags.invisible = true;
        let mut locked = attribute("TAG_LOCKED", 4.0, 4.0);
        locked.lock_position = true;
        let mut ins = block_with(vec![constant, invisible, locked, attribute("TAG_OK", 5.0, 5.0)]);

        let g = grips(&ins);
        assert_eq!(g.len(), 2, "only the movable attribute is gripped: {g:?}");
        assert_eq!(g[1].world, glam::DVec3::new(5.0, 5.0, 0.0));

        apply_grip(&mut ins, g[1].id, GripApply::Absolute(glam::DVec3::new(50.0, 50.0, 0.0)));
        assert_eq!(ins.attributes[3].insertion_point, Vector3::new(50.0, 50.0, 0.0));
        assert_eq!(ins.attributes[0].insertion_point, Vector3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn visible_attribute_grips_match_render_visibility() {
        let mut off_layer = Layer::new("OFF");
        off_layer.flags.off = true;
        let mut frozen_layer = Layer::new("FROZEN");
        frozen_layer.flags.frozen = true;
        let mut document = document_with_layers(vec![off_layer, frozen_layer]);
        document.header.attribute_visibility = 1;

        let mut common_invisible = attribute("TAG_COMMON_INVISIBLE", 1.0, 1.0);
        common_invisible.common.invisible = true;
        let mut off = attribute("TAG_OFF", 2.0, 2.0);
        off.common.layer = "OFF".into();
        let mut frozen = attribute("TAG_FROZEN", 3.0, 3.0);
        frozen.common.layer = "FROZEN".into();
        let visible = attribute("TAG_VISIBLE", 4.0, 4.0);
        let ins = block_with(vec![common_invisible, off, frozen, visible]);

        let grips = visible_attribute_grips(&document, &ins, 1.0);

        assert_eq!(grips.len(), 2, "base grip plus only rendered attributes");
        assert_eq!(grips[1].id, ATTRIBUTE_GRIP_BASE + 3);
        assert_eq!(grips[1].world, glam::DVec3::new(4.0, 4.0, 0.0));

        document.header.attribute_visibility = 0;
        assert_eq!(
            visible_attribute_grips(&document, &ins, 1.0).len(),
            1,
            "ATTMODE=0 should hide every attribute grip",
        );
    }

    #[test]
    fn visible_attribute_grips_scale_annotative_attributes() {
        let document = CadDocument::new();
        let mut insert = block_with(vec![attribute("TAG", 11.0, 10.0)]);
        insert.insert_point = Vector3::new(10.0, 10.0, 0.0);
        insert
            .common
            .extended_data
            .add_record(ExtendedDataRecord::new("AcAnnotativeData"));

        let grips = visible_attribute_grips(&document, &insert, 2.0);

        assert_eq!(grips.len(), 2);
        assert_eq!(grips[1].world, glam::DVec3::new(12.0, 10.0, 0.0));
    }
}
