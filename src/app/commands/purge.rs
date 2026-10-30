//! PURGE's choice of what can go: one pass over a drawing, shared by the
//! command and the Purge dialog.

use codec::{Document, Handle};

/// The layers, text styles, linetypes and blocks one PURGE pass removes from
/// `doc`, for the kinds `wanted` turns on (layers, styles, linetypes,
/// blocks). Removing blocks can free what only they used, so a full purge
/// repeats passes until one finds nothing.
pub(crate) fn purge_pass(
    doc: &Document,
    open_edits: &[Handle],
    wanted: [bool; 4],
) -> [Vec<String>; 4] {
    let [do_layers, do_styles, do_lts, do_blocks] = wanted;
    // Xref-dependent records (`Xref|Name`) belong to the xref and come back
    // with its next reload, so they are never purged, also when the xref did
    // not resolve and nothing in the host draws on them (#1929). Blocks get
    // the same rule in `block_service::protected_reason`.
    let xref_dependent = |name: &str, flagged: bool| flagged || name.contains('|');
    let insert_attributes = || {
        doc
            .entities()
            .filter_map(|e| match e {
                codec::EntityType::Insert(ins) => Some(ins.attributes.iter()),
                _ => None,
            })
            .flatten()
    };
    let used_layers: rustc_hash::FxHashSet<String> = doc
        .entities()
        .map(|e| e.common())
        .chain(insert_attributes().map(|a| &a.common))
        .filter_map(|common| {
            let name = &common.layer;
            if name.is_empty() {
                None
            } else {
                Some(name.to_uppercase())
            }
        })
        // The current layer is never purged, empty or not.
        .chain(std::iter::once(
            doc
                .header
                .current_layer_name
                .to_uppercase(),
        ))
        .collect();
    // Text styles: entity references plus every dimension
    // style's DIMTXSTY — a style only a dimstyle uses is not
    // purgeable.
    let used_text_styles: rustc_hash::FxHashSet<String> = doc
        .entities()
        .filter_map(|e| match e {
            codec::EntityType::Text(t) => Some(t.style.clone()),
            codec::EntityType::MText(t) => Some(t.style.clone()),
            codec::EntityType::AttributeDefinition(a) => Some(a.text_style.clone()),
            codec::EntityType::AttributeEntity(a) => Some(a.text_style.clone()),
            _ => None,
        })
        .chain(insert_attributes().map(|a| a.text_style.clone()))
        .chain(std::iter::once(
            doc
                .header
                .current_text_style_name
                .clone(),
        ))
        .chain(
            doc
                .dim_styles
                .iter()
                .map(|ds| ds.dimtxsty.clone()),
        )
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
        .collect();
    // Complex linetypes reach their text and shape styles by handle.
    let linetype_styles: rustc_hash::FxHashSet<codec::Handle> = doc
        .line_types
        .iter()
        .flat_map(|lt| lt.elements.iter())
        .filter_map(|element| element.complex.as_ref())
        .map(|complex| complex.style_handle)
        .collect();
    // Multileaders and their styles, and table styles, reach
    // their text styles by handle (a table style also by name).
    let object_text_styles: rustc_hash::FxHashSet<codec::Handle> = {
        let leaders = doc.entities().filter_map(|e| match e {
            codec::EntityType::MultiLeader(m) => Some(m),
            _ => None,
        });
        leaders
            .flat_map(|m| [m.text_style_handle, m.context.text_style_handle])
            .chain(doc.objects.values().flat_map(|o| {
                match o {
                    codec::objects::ObjectType::MultiLeaderStyle(style) => {
                        vec![style.text_style_handle]
                    }
                    codec::objects::ObjectType::TableStyle(style) => [
                        &style.data_row_style,
                        &style.header_row_style,
                        &style.title_row_style,
                    ]
                    .iter()
                    .map(|row| row.text_style_handle)
                    .collect(),
                    _ => vec![],
                }
            }))
            .flatten()
            .filter(|h| !h.is_null())
            .collect()
    };
    let table_style_text_styles: rustc_hash::FxHashSet<String> = doc
        .objects
        .values()
        .filter_map(|o| match o {
            codec::objects::ObjectType::TableStyle(style) => Some(style),
            _ => None,
        })
        .flat_map(|style| {
            [
                &style.data_row_style,
                &style.header_row_style,
                &style.title_row_style,
            ]
        })
        .filter(|row| !row.text_style_name.is_empty())
        .map(|row| row.text_style_name.to_uppercase())
        .collect();
    // Dimension styles reach their dimension and extension line
    // linetypes by handle.
    let dimstyle_linetypes: rustc_hash::FxHashSet<codec::Handle> = doc
        .dim_styles
        .iter()
        .flat_map(|ds| [ds.dimltex_handle, ds.dimltex1_handle, ds.dimltex2_handle])
        .filter(|h| !h.is_null())
        .collect();
    // Linetypes: entity references plus every layer's own
    // linetype — a ByLayer entity reaches it through the layer.
    let used_linetypes: rustc_hash::FxHashSet<String> = doc
        .entities()
        .map(|e| e.common())
        .chain(insert_attributes().map(|a| &a.common))
        .filter_map(|common| {
            let lt = &common.linetype;
            if lt.is_empty() || lt == "ByLayer" || lt == "ByBlock" {
                None
            } else {
                Some(lt.clone())
            }
        })
        .chain(
            doc
                .layers
                .iter()
                .map(|l| l.line_type.clone()),
        )
        .chain(std::iter::once(
            doc
                .header
                .current_linetype_name
                .clone(),
        ))
        .filter(|s| !s.is_empty())
        .map(|s| s.to_uppercase())
        .collect();

    // Build removal lists (still immutable)
    let layer_remove: Vec<String> = if do_layers {
        doc
            .layers
            .iter()
            .filter(|l| {
                l.name != "0"
                    && !xref_dependent(&l.name, l.flags.xref_dependent)
                    && !used_layers.contains(&l.name.to_uppercase())
            })
            .map(|l| l.name.clone())
            .collect()
    } else {
        vec![]
    };
    let style_remove: Vec<String> = if do_styles {
        doc
            .text_styles
            .iter()
            // Unnamed shape-file records serve complex linetypes
            // and are never listed for purging.
            .filter(|s| {
                !s.name.is_empty()
                    && !s.is_shape_file
                    && !s.name.eq_ignore_ascii_case("Standard")
                    && !xref_dependent(&s.name, s.xref_dependent)
                    && !used_text_styles.contains(&s.name.to_uppercase())
                    && !linetype_styles.contains(&s.handle)
                    && !object_text_styles.contains(&s.handle)
                    && !table_style_text_styles.contains(&s.name.to_uppercase())
            })
            .map(|s| s.name.clone())
            .collect()
    } else {
        vec![]
    };
    let lt_remove: Vec<String> = if do_lts {
        let standard = ["Continuous", "ByLayer", "ByBlock"];
        doc
            .line_types
            .iter()
            .filter(|lt| {
                !standard.iter().any(|s| s.eq_ignore_ascii_case(&lt.name))
                    && !xref_dependent(&lt.name, lt.xref_dependent)
                    && !used_linetypes.contains(&lt.name.to_uppercase())
                    && !dimstyle_linetypes.contains(&lt.handle)
            })
            .map(|lt| lt.name.clone())
            .collect()
    } else {
        vec![]
    };

    // Remove blocks nothing references any more. The shared
    // service decides what qualifies — layout/xref/dynamic
    // records and open BEDIT sessions are protected, and so is
    // every reference kind — so a purge and a delete give the
    // same answer.
    let block_remove: Vec<String> = if do_blocks {
        crate::scene::block_service::purge_candidates(doc, open_edits)
    } else {
        vec![]
    };
    [layer_remove, style_remove, lt_remove, block_remove]
}
