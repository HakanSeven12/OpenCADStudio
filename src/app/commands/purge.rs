//! PURGE's choice of what can go: one pass over a drawing, shared by the
//! command and the Purge dialog.

use crate::app::style_ops::{style_in_use, StyleKind};
use document::objects::ObjectType;
use document::{Document, EntityType, Handle};

/// The kinds PURGE removes, in the order `purge_pass` reports them.
pub(crate) const KINDS: usize = 12;
pub(crate) const LAYERS: usize = 0;
pub(crate) const TEXT_STYLES: usize = 1;
pub(crate) const LINETYPES: usize = 2;
pub(crate) const BLOCKS: usize = 3;
pub(crate) const DIM_STYLES: usize = 4;
pub(crate) const MLEADER_STYLES: usize = 5;
pub(crate) const TABLE_STYLES: usize = 6;
pub(crate) const MLINE_STYLES: usize = 7;
pub(crate) const GROUPS: usize = 8;
pub(crate) const MATERIALS: usize = 9;
/// Entities, not definitions: named `TYPE HANDLE` (`LINE 2A`).
pub(crate) const ZERO_LENGTH: usize = 10;
pub(crate) const EMPTY_TEXT: usize = 11;

/// The style kind a style purge kind removes through the style managers'
/// shared storage.
pub(crate) fn style_kind(kind: usize) -> Option<StyleKind> {
    Some(match kind {
        DIM_STYLES => StyleKind::Dim,
        MLEADER_STYLES => StyleKind::MLeader,
        TABLE_STYLES => StyleKind::Table,
        MLINE_STYLES => StyleKind::MLine,
        _ => return None,
    })
}

/// The handle in an entity item's `TYPE HANDLE` name.
pub(crate) fn item_handle(name: &str) -> Option<Handle> {
    let hex = name.rsplit(' ').next()?;
    u64::from_str_radix(hex, 16).ok().map(Handle::new)
}

/// What one PURGE pass removes from `doc`, per kind, for the kinds `wanted`
/// turns on. Removing blocks can free what only they used, so a full purge
/// repeats passes until one finds nothing. `active_mleader` is the session's
/// current multileader style, which is kept.
pub(crate) fn purge_pass(
    doc: &Document,
    open_edits: &[Handle],
    wanted: [bool; KINDS],
    active_mleader: &str,
) -> [Vec<String>; KINDS] {
    let [do_layers, do_styles, do_lts, do_blocks, ..] = wanted;
    // Xref-dependent records (`Xref|Name`) belong to the xref and come back
    // with its next reload, so they are never purged, also when the xref did
    // not resolve and nothing in the host draws on them (#1929). Blocks get
    // the same rule in `block_service::protected_reason`.
    let xref_dependent = |name: &str, flagged: bool| flagged || name.contains('|');
    let insert_attributes = || {
        doc
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Insert(ins) => Some(ins.attributes.iter()),
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
            document::EntityType::Text(t) => Some(t.style.clone()),
            document::EntityType::MText(t) => Some(t.style.clone()),
            document::EntityType::AttributeDefinition(a) => Some(a.text_style.clone()),
            document::EntityType::AttributeEntity(a) => Some(a.text_style.clone()),
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
    let linetype_styles: rustc_hash::FxHashSet<document::Handle> = doc
        .line_types
        .iter()
        .flat_map(|lt| lt.elements.iter())
        .filter_map(|element| element.complex.as_ref())
        .map(|complex| complex.style_handle)
        .collect();
    // Multileaders and their styles, and table styles, reach
    // their text styles by handle (a table style also by name).
    let object_text_styles: rustc_hash::FxHashSet<document::Handle> = {
        let leaders = doc.entities().filter_map(|e| match e {
            document::EntityType::MultiLeader(m) => Some(m),
            _ => None,
        });
        leaders
            .flat_map(|m| [m.text_style_handle, m.context.text_style_handle])
            .chain(doc.objects.values().flat_map(|o| {
                match o {
                    document::objects::ObjectType::MultiLeaderStyle(style) => {
                        vec![style.text_style_handle]
                    }
                    document::objects::ObjectType::TableStyle(style) => [
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
            document::objects::ObjectType::TableStyle(style) => Some(style),
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
    let dimstyle_linetypes: rustc_hash::FxHashSet<document::Handle> = doc
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
    let mut lists: [Vec<String>; KINDS] = Default::default();
    lists[LAYERS] = layer_remove;
    lists[TEXT_STYLES] = style_remove;
    lists[LINETYPES] = lt_remove;
    lists[BLOCKS] = block_remove;

    // Styles: what the style managers would let you delete.
    let keep = |name: &str| name.eq_ignore_ascii_case("Standard") || name.contains('|');
    if wanted[DIM_STYLES] {
        let names: Vec<String> = doc.dim_styles.iter().map(|s| s.name.clone()).collect();
        lists[DIM_STYLES] = names
            .iter()
            .filter(|name| {
                // A style's overrides (`NAME$0`) go first; it follows on the
                // next pass.
                let children = format!("{}$", name.to_uppercase());
                !keep(name)
                    && !names.iter().any(|n| n.to_uppercase().starts_with(&children))
                    && !style_in_use(doc, StyleKind::Dim, name, active_mleader)
            })
            .cloned()
            .collect();
    }
    for kind in [MLEADER_STYLES, TABLE_STYLES, MLINE_STYLES] {
        if !wanted[kind] {
            continue;
        }
        let style = style_kind(kind).expect("style kind");
        lists[kind] = doc
            .objects
            .values()
            .filter_map(|o| match (kind, o) {
                (MLEADER_STYLES, ObjectType::MultiLeaderStyle(s)) => Some(s.name.clone()),
                (TABLE_STYLES, ObjectType::TableStyle(s)) => Some(s.name.clone()),
                (MLINE_STYLES, ObjectType::MLineStyle(s)) => Some(s.name.clone()),
                _ => None,
            })
            .filter(|name| !keep(name) && !style_in_use(doc, style, name, active_mleader))
            .collect();
    }

    // Groups with no member left in the drawing.
    if wanted[GROUPS] {
        lists[GROUPS] = doc
            .objects
            .values()
            .filter_map(|o| match o {
                ObjectType::Group(g)
                    if g.entities.iter().all(|h| doc.get_entity(*h).is_none()) =>
                {
                    Some(g.name.clone())
                }
                _ => None,
            })
            .collect();
    }

    // Materials no object or layer uses; the three built-in ones stay.
    if wanted[MATERIALS] {
        let used: rustc_hash::FxHashSet<Handle> = doc
            .entities()
            .filter_map(|e| e.common().material_handle)
            .chain(doc.layers.iter().map(|l| l.material))
            .collect();
        lists[MATERIALS] = doc
            .objects
            .values()
            .filter_map(|o| match o {
                ObjectType::Material(m)
                    if !used.contains(&m.handle)
                        && !["Global", "ByLayer", "ByBlock"]
                            .iter()
                            .any(|b| b.eq_ignore_ascii_case(&m.name)) =>
                {
                    Some(m.name.clone())
                }
                _ => None,
            })
            .collect();
    }

    // Zero-length geometry and empty text, on a model or sheet layout.
    if wanted[ZERO_LENGTH] || wanted[EMPTY_TEXT] {
        let spaces: rustc_hash::FxHashSet<Handle> = doc
            .block_records
            .iter()
            .filter(|br| matches!(doc.objects.get(&br.layout), Some(ObjectType::Layout(_))))
            .map(|br| br.handle)
            .collect();
        const TOL: f64 = 1e-10;
        for e in doc.entities().filter(|e| spaces.contains(&e.common().owner_handle)) {
            let (kind, label) = match e {
                EntityType::Line(l) if (l.end - l.start).length() <= TOL => (ZERO_LENGTH, "LINE"),
                EntityType::Arc(a) if a.radius.abs() <= TOL => (ZERO_LENGTH, "ARC"),
                EntityType::Circle(c) if c.radius.abs() <= TOL => (ZERO_LENGTH, "CIRCLE"),
                EntityType::LwPolyline(p)
                    if p.vertices.windows(2).all(|w| {
                        (w[1].location - w[0].location).length() <= TOL
                    }) =>
                {
                    (ZERO_LENGTH, "LWPOLYLINE")
                }
                EntityType::Text(t) if t.value.trim().is_empty() => (EMPTY_TEXT, "TEXT"),
                EntityType::MText(t) if t.value.trim().is_empty() => (EMPTY_TEXT, "MTEXT"),
                _ => continue,
            };
            if wanted[kind] {
                lists[kind].push(format!("{label} {:X}", e.common().handle));
            }
        }
    }
    lists
}
