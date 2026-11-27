//! How a block reference's attributes draw: each one tessellated in the
//! reference's style, scale and array cells.

use crate::entities::insert::*;
use crate::scene::cache::block_cache;
use crate::scene::convert::tessellate;
use crate::scene::view::render;
use crate::scene::WireModel;
use document::Handle;
use document::Vector3;
use document::types::Transform;

pub(crate) fn append_insert_attribute_wires(
    wires: &mut Vec<WireModel>,
    document: &document::Document,
    ins: &document::entities::Insert,
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
    annotation_scale_handle: Option<Handle>,
) {
    let attributes = insert_attribute_entities(
        document,
        ins,
        anno_scale,
        annotation_scale_handle,
        BlockScalePolicy::FromInsert,
    );
    if attributes.is_empty() {
        return;
    }
    for offset in crate::entities::block_ref::array_offsets(ins) {
        let delta = crate::entities::block_ref::insert_instance_translation_delta(
            document,
            ins,
            offset,
            anno_scale,
            BlockScalePolicy::FromInsert,
        );
        for source in &attributes {
            let mut attr_entity = source.clone();
            if delta != Vector3::ZERO {
                attr_entity.apply_transform(&Transform::from_translation(delta));
            }
            let attr = attr_entity.common();
            let (sub_color, sub_plen, sub_pat, sub_lw_px, own_aci) =
                render::render_style_for_block_sub(
                    document,
                    &attr_entity,
                    ins_color,
                    ins_pat_len,
                    ins_pat,
                    ins_lw_px,
                    ins_layer,
                );
            // A ByBlock attribute plots by the insert's ACI, a ByLayer one on
            // layer 0 by the insert layer's, so a CTB maps it like the block's
            // own children (#3747).
            let has_book_color = render::has_resolved_book_color_common(document, attr);
            let sub_aci = if !has_book_color && attr.color == document::types::Color::ByBlock {
                render::render_style_for_common_viewport(document, &ins.common, None).4
            } else if !has_book_color
                && render::is_effective_layer_zero(&attr.layer)
                && attr.color == document::types::Color::ByLayer
            {
                crate::entities::block_ref::layer_aci(document, &ins.common.layer)
            } else {
                own_aci
            };
            let sub_color = render::adapt_to_bg(sub_color, bg_color);
            let sub_color = if is_xref && !sel {
                block_cache::xref_fade(sub_color, bg_color)
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
            let sub_aabb = crate::entities::bounds::entity_aabb(&attr_entity);
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
            let effective_layer = if render::is_effective_layer_zero(&attr.layer)
                && !render::is_effective_layer_zero(&ins.common.layer)
            {
                ins.common.layer.as_str()
            } else {
                attr.layer.as_str()
            };
            for w in &mut attr_wires {
                w.name = insert_handle.value().to_string();
                w.effective_layer = Some(std::sync::Arc::from(effective_layer));
                w.aci = sub_aci;
                w.aabb = sub_aabb;
                w.plot_visible &= attr_plottable;
                // Over every member of the block, its wipeouts included.
                w.depth_override = Some(INSERT_ATTRIBUTE_RANK);
            }
            wires.extend(attr_wires);
        }
    }
}
