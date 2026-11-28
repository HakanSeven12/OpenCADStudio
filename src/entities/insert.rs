use document::entities::Insert;
use document::types::{Matrix3, Transform, Vector3};
use document::{Document, EntityType, Handle};
use glam::Vec3;

use crate::entities::common::{
    edit_angle_prop as edit_angle, edit_prop as edit, parse_f64, ro_prop as ro, square_grip,
};
use crate::entities::traits::Grippable;
use crate::entities::transform::EntityTransform;

use crate::entities::object::{GripApply, GripDef, PropSection, PropValue, Property};
use document::objects::BlockBasePointParameter;
use document::objects::DynamicBlockData;
use document::objects::ObjectType;

/// Grip ids from here up address the INSERT's attributes — `BASE + i` is
/// `attributes[i]`. Grip 0 stays the block's own insertion point.
pub(crate) const ATTRIBUTE_GRIP_BASE: usize = 1;

/// Whether an attribute is one the user can pick up and reposition. A constant
/// attribute belongs to the block definition rather than this insert, and a
/// locked position cannot be dragged. Visibility is a separate question: an
/// invisible attribute that ATTMODE 2 displays is grippable while it shows.
fn attribute_is_movable(att: &document::entities::AttributeEntity) -> bool {
    !att.flags.constant && !att.lock_position && !att.flags.locked_position
}

/// A dragged attribute grip, mapped back to the attribute's stored position.
/// The grip is placed on the displayed attribute, which an annotative insert
/// scales about its insertion point; the target has to be unscaled the same
/// way or the attribute lands `scale` times too far and the grip jumps.
pub(crate) fn unscale_attribute_grip(
    insert: &Insert,
    annotation_scale: f32,
    grip_id: usize,
    apply: GripApply,
) -> GripApply {
    if grip_id < ATTRIBUTE_GRIP_BASE {
        return apply;
    }
    let block_scale = insert_attribute_block_scale(
        insert,
        annotation_scale,
        BlockScalePolicy::FromInsert,
    );
    if (block_scale - 1.0).abs() <= 1.0e-6 {
        return apply;
    }
    let origin = glam::DVec3::new(
        insert.insert_point.x,
        insert.insert_point.y,
        insert.insert_point.z,
    );
    match apply {
        GripApply::Absolute(target) => {
            GripApply::Absolute(origin + (target - origin) / block_scale)
        }
        GripApply::Translate(delta) => GripApply::Translate(delta / block_scale),
    }
}

fn attribute_is_render_visible(
    document: &Document,
    visibility: i16,
    attribute: &document::entities::AttributeEntity,
) -> bool {
    let layer_visible = document
        .layers
        .get(&attribute.common.layer)
        .is_none_or(|layer| !layer.flags.off && !layer.flags.frozen);
    // ATTMODE controls the ATTRIB's own invisible flag, but the common entity
    // visibility flag still hides the entity independently.
    layer_visible
        && visibility != 0
        && !attribute.common.invisible
        && (visibility == 2 || !attribute.flags.invisible)
}

fn insert_attribute_block_scale(
    insert: &Insert,
    annotation_scale: f32,
    scale_policy: BlockScalePolicy,
) -> f64 {
    if scale_policy == BlockScalePolicy::FromInsert
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
    attribute: &mut document::entities::AttributeEntity,
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
    if let Some(mtext) = attribute.embedded_mtext.as_mut() {
        mtext.rectangle_width *= block_scale;
    }
    attribute.insertion_point = scale_about(attribute.insertion_point);
    attribute.alignment_point = scale_about(attribute.alignment_point);
}

/// The insertion point grip drags the whole reference, attributes included.
fn insert_point_grip(world: Vector3) -> GripDef {
    GripDef {
        is_midpoint: true,
        ..square_grip(0, glam::DVec3::new(world.x, world.y, world.z))
    }
}

fn grips(ins: &Insert) -> Vec<GripDef> {
    // `insert_point` is in the OCS defined by `normal`; the grip must sit at
    // the world placement, so map it through the OCS. Identity for +Z.
    let w = Matrix3::arbitrary_axis(ins.normal) * ins.insert_point;
    let mut out = vec![insert_point_grip(w)];
    // One grip per attribute, so an attribute can be moved off the place the
    // block definition put it without dragging the whole block (#1259).
    // Attributes are stored beside the block in world space — the same space
    // `apply_grip` already translates them in.
    // Without the document there is no ATTMODE to consult, so only an
    // attribute that is drawn on its own terms gets a grip here;
    // `visible_attribute_grips` is the display-aware list.
    out.extend(ins.attributes.iter().enumerate().filter_map(|(i, att)| {
        if !attribute_is_movable(att) || att.flags.invisible || att.common.invisible {
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
    document: &Document,
    insert: &Insert,
    annotation_scale: f32,
    annotation_scale_handle: Option<Handle>,
) -> Vec<GripDef> {
    let w = Matrix3::arbitrary_axis(insert.normal) * insert.insert_point;
    let mut out = vec![insert_point_grip(w)];
    let visibility = document.header.attribute_visibility;
    let block_scale = insert_attribute_block_scale(
        insert,
        annotation_scale,
        BlockScalePolicy::FromInsert,
    );
    out.extend(
        insert
            .attributes
            .iter()
            .enumerate()
            .filter_map(|(i, attribute)| {
                if !attribute_is_movable(attribute)
                    || !attribute_is_render_visible(document, visibility, attribute)
                {
                    return None;
                }
                let mut attribute = attribute.clone();
                if !crate::entities::annotative::place_attribute_for_annotation_scale(
                    document,
                    &mut attribute,
                    annotation_scale_handle,
                ) {
                    scale_attribute_for_insert(insert, &mut attribute, block_scale);
                }
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
    // The attributes appear only when the block actually has attributes,
    // under the block's name: references of different blocks share no
    // attribute group.
    if !attrs.is_empty() {
        sections.push(PropSection {
            title: format!("Block:{}", ins.block_name),
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
                document::Entity::translate(att, delta);
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
        if let Some(att) = ins
            .attributes
            .get_mut(index)
            .filter(|a| attribute_is_movable(a))
        {
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
        GripApply::Absolute(p) => Vector3::new(p.x, p.y, p.z),
        GripApply::Translate(d) => old_world + Vector3::new(d.x, d.y, d.z),
    };
    ins.insert_point = ocs.transpose() * world;
    // Attributes sit in world space beside the block, so move them by the same
    // world delta — otherwise a grip-drag leaves the attribute text behind (#255).
    let delta = world - old_world;
    for att in &mut ins.attributes {
        document::Entity::translate(att, delta);
    }
}

pub(crate) fn apply_transform(ins: &mut Insert, t: &EntityTransform) {
    crate::entities::transform::apply_standard_entity_transform(ins, t, |entity, p1, p2| {
        let dx = p2.x - p1.x;
        let dy = p2.y - p1.y;
        let len = (dx * dx + dy * dy).sqrt();
        if len < 1e-12 {
            return;
        }

        let ux = dx / len;
        let uy = dy / len;
        let mirror = document::types::Matrix4 {
            m: [
                [2.0 * ux * ux - 1.0, 2.0 * ux * uy, 0.0, 0.0],
                [2.0 * ux * uy, 2.0 * uy * uy - 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        };
        let t = Transform::from_translation(Vector3::new(-p1.x, -p1.y, 0.0))
            .then(&Transform::from_matrix(mirror))
            .then(&Transform::from_translation(Vector3::new(p1.x, p1.y, 0.0)));
        document::Entity::apply_transform(entity, &t);
    });
}

crate::impl_entity_basics!(Insert);

impl crate::entities::traits::FallbackTess for Insert {
    fn fallback_geometry(&self) -> crate::display::tess_util::FallbackGeometry {
        // `insert_point` is in the OCS of `normal`.
        let w = Matrix3::arbitrary_axis(self.normal) * self.insert_point;
        let (ipx, ipy, ipz) = (w.x, w.y, w.z);
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
            vec![(ip, crate::display::wire_model::SnapHint::Insertion)],
            vec![],
            vec![],
        )
    }
}
pub(crate) fn insert_attribute_entities(
    document: &document::Document,
    insert: &document::entities::Insert,
    annotation_scale: f32,
    annotation_scale_handle: Option<Handle>,
    scale_policy: BlockScalePolicy,
) -> Vec<EntityType> {
    let visibility = document.header.attribute_visibility;
    if visibility == 0 || insert.attributes.is_empty() {
        return Vec::new();
    }
    let block_scale = insert_attribute_block_scale(insert, annotation_scale, scale_policy);
    insert
        .attributes
        .iter()
        // A constant attribute is text the definition itself carries: it is the
        // same for every reference, so the definition draws it and the
        // reference must not draw it again underneath.
        .filter(|attribute| !attribute.flags.constant)
        .filter(|attribute| attribute_is_render_visible(document, visibility, attribute))
        .map(|attribute| {
            let mut attribute = attribute.clone();
            if !crate::entities::annotative::place_attribute_for_annotation_scale(
                document,
                &mut attribute,
                annotation_scale_handle,
            ) {
                scale_attribute_for_insert(insert, &mut attribute, block_scale);
            }
            EntityType::AttributeEntity(attribute)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, Hash, PartialEq, Eq)]
pub enum BlockScalePolicy {
    #[default]
    FromInsert,
    Applied,
}

/// The rank an insert's attributes are drawn at: `insert depth + rank * half`.
/// Block members draw at their own ranks, all inside (0, 2) of the insert's
/// half gap, so this one is above every member (an attribute is drawn after the
/// block's contents, over its wipeout) and still under the next sibling, which
/// starts two half gaps further on.
pub(crate) const INSERT_ATTRIBUTE_RANK: f32 = 1.95;

pub(crate) fn block_base_point_parameter(
    document: &Document,
    definition: Handle,
) -> Option<(Handle, &BlockBasePointParameter)> {
    let ObjectType::Dictionary(dictionary) = document
        .objects
        .get(&crate::entities::dictionary_owners::definition_dictionary(document, definition)?)?
    else {
        return None;
    };
    let graph = dictionary.get("ACAD_ENHANCEDBLOCK")?;
    let ObjectType::DynamicBlock(object) = document.objects.get(&graph)? else {
        return None;
    };
    let DynamicBlockData::EvaluationGraph(data) = &object.data else {
        return None;
    };
    data.nodes
        .iter()
        .find_map(|node| match document.objects.get(&node.expression) {
            Some(ObjectType::DynamicBlock(object)) if object.owner == graph => match &object.data {
                DynamicBlockData::BasePointParameter(p) => Some((object.handle, p)),
                _ => None,
            },
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use document::entities::AttributeEntity;
    use document::tables::Layer;
    use document::xdata::ExtendedDataRecord;

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

    fn document_with_layers(layers: Vec<Layer>) -> Document {
        let mut document = Document::new();
        for layer in layers {
            document.layers.add_or_replace(layer);
        }
        document
    }

    /// #1259: selecting a block showed only the insertion grip, so there was no
    /// handle to pick an attribute up by and nowhere to drop it.
    #[test]
    fn each_attribute_offers_its_own_grip() {
        let ins = block_with(vec![
            attribute("TAG_A", 3.0, 4.0),
            attribute("TAG_B", 6.0, 8.0),
        ]);

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
        let mut ins = block_with(vec![
            attribute("TAG_A", 3.0, 4.0),
            attribute("TAG_B", 6.0, 8.0),
        ]);

        let moved = grips(&ins)[1].id;
        apply_grip(
            &mut ins,
            moved,
            GripApply::Absolute(glam::DVec3::new(30.0, 40.0, 0.0)),
        );

        assert_eq!(ins.insert_point, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(
            ins.attributes[0].insertion_point,
            Vector3::new(30.0, 40.0, 0.0)
        );
        assert_eq!(
            ins.attributes[0].alignment_point,
            Vector3::new(30.0, 40.0, 0.0)
        );
        assert_eq!(
            ins.attributes[1].insertion_point,
            Vector3::new(6.0, 8.0, 0.0)
        );
    }

    #[test]
    fn justified_attribute_grip_preserves_its_text_alignment() {
        let mut attribute = attribute("ROOM", 3.0, 4.0);
        attribute.alignment_point = Vector3::new(5.0, 6.0, 0.0);
        let mut insert = block_with(vec![attribute]);
        apply_grip(
            &mut insert,
            ATTRIBUTE_GRIP_BASE,
            GripApply::Absolute(glam::DVec3::new(13.0, 14.0, 0.0)),
        );
        assert_eq!(
            insert.attributes[0].insertion_point,
            Vector3::new(13.0, 14.0, 0.0)
        );
        assert_eq!(
            insert.attributes[0].alignment_point,
            Vector3::new(15.0, 16.0, 0.0)
        );
    }

    /// The insertion grip keeps carrying the attributes along with the block,
    /// which is what the attribute grips are an alternative to, not a
    /// replacement for (#255).
    #[test]
    fn insertion_grip_still_carries_the_attributes() {
        let mut ins = block_with(vec![attribute("TAG_A", 3.0, 4.0)]);

        apply_grip(
            &mut ins,
            0,
            GripApply::Translate(glam::DVec3::new(10.0, 0.0, 0.0)),
        );

        assert_eq!(ins.insert_point, Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(
            ins.attributes[0].insertion_point,
            Vector3::new(13.0, 4.0, 0.0)
        );
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
        let mut ins = block_with(vec![
            constant,
            invisible,
            locked,
            attribute("TAG_OK", 5.0, 5.0),
        ]);

        let g = grips(&ins);
        assert_eq!(g.len(), 2, "only the movable attribute is gripped: {g:?}");
        assert_eq!(g[1].world, glam::DVec3::new(5.0, 5.0, 0.0));

        apply_grip(
            &mut ins,
            g[1].id,
            GripApply::Absolute(glam::DVec3::new(50.0, 50.0, 0.0)),
        );
        assert_eq!(
            ins.attributes[3].insertion_point,
            Vector3::new(50.0, 50.0, 0.0)
        );
        assert_eq!(
            ins.attributes[0].insertion_point,
            Vector3::new(1.0, 1.0, 0.0)
        );
    }

    #[test]
    fn a_constant_attribute_is_painted_by_the_definition_only() {
        use document::EntityType;
        let mut document = document_with_layers(Vec::new());
        document.header.attribute_visibility = 1;
        let mut constant = attribute("TAG_CONST", 1.0, 1.0);
        constant.flags.constant = true;
        let ins = block_with(vec![constant, attribute("TAG_VALUE", 2.0, 2.0)]);
        let painted: Vec<String> = insert_attribute_entities(
            &document,
            &ins,
            1.0,
            None,
            BlockScalePolicy::FromInsert,
        )
        .into_iter()
        .map(|entity| match entity {
            EntityType::AttributeEntity(attribute) => attribute.tag.clone(),
            other => panic!("expected an attribute, got {other:?}"),
        })
        .collect();
        assert_eq!(
            painted,
            vec!["TAG_VALUE".to_string()],
            "the definition already draws the constant, so the reference adds nothing"
        );
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

        let grips = visible_attribute_grips(&document, &ins, 1.0, None);

        assert_eq!(grips.len(), 2, "base grip plus only rendered attributes");
        assert_eq!(grips[1].id, ATTRIBUTE_GRIP_BASE + 3);
        assert_eq!(grips[1].world, glam::DVec3::new(4.0, 4.0, 0.0));

        document.header.attribute_visibility = 0;
        assert_eq!(
            visible_attribute_grips(&document, &ins, 1.0, None).len(),
            1,
            "ATTMODE=0 should hide every attribute grip",
        );
    }

    #[test]
    fn visible_attribute_grips_scale_annotative_attributes() {
        let document = Document::new();
        let mut insert = block_with(vec![attribute("TAG", 11.0, 10.0)]);
        insert.insert_point = Vector3::new(10.0, 10.0, 0.0);
        insert
            .common
            .extended_data
            .add_record(ExtendedDataRecord::new("AcAnnotativeData"));

        let grips = visible_attribute_grips(&document, &insert, 2.0, None);

        assert_eq!(grips.len(), 2);
        assert_eq!(grips[1].world, glam::DVec3::new(12.0, 10.0, 0.0));
    }
}

#[cfg(test)]
mod attribute_width_tests {
    use super::*;

    #[test]
    fn annotative_attribute_scales_its_wrap_width_with_its_height() {
        let insert = Insert::new("TITLE", Vector3::ZERO);
        let mut attribute = document::entities::AttributeEntity::new("TITLE".into(), "A B".into());
        let mut mtext = document::entities::MText::new();
        mtext.rectangle_width = 15.0;
        attribute.embedded_mtext = Some(Box::new(mtext));
        let ratio = 15.0 / attribute.height;
        scale_attribute_for_insert(&insert, &mut attribute, 3.0);
        assert_eq!(
            attribute.embedded_mtext.unwrap().rectangle_width / attribute.height,
            ratio
        );
    }
}
