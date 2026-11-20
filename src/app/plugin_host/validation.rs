// Name, nesting and panel-id validation helpers.

use super::*;

/// True when block `outer` (transitively) contains an insert of `target`.
pub(super) fn block_nests(doc: &Document, outer: &str, target: &str, _depth: usize) -> bool {
    // Visited-set traversal (P1-13): the depth-capped recursion re-explored
    // shared sub-blocks once per referencing instance, so a document with N
    // shared instances of one block multiplied the whole subtree N times —
    // exponential on deep, heavily-shared block trees. Every block is now
    // visited at most once per top-level check (linear in the block count);
    // the depth cap stays as a belt-and-braces guard with its old
    // conservative `true`.
    fn walk(
        doc: &Document,
        outer: &str,
        target: &str,
        visited: &mut std::collections::HashSet<String>,
        depth: usize,
    ) -> bool {
        if depth > 64 {
            return true;
        }
        if !visited.insert(outer.to_ascii_uppercase()) {
            return false;
        }
        let Some(record) = doc.block_records.get(outer) else {
            return false;
        };
        record
            .entity_handles
            .iter()
            .filter_map(|h| doc.get_entity(*h))
            .any(|entity| match entity {
                EntityType::Insert(insert) => {
                    insert.block_name.eq_ignore_ascii_case(target)
                        || walk(doc, &insert.block_name, target, visited, depth + 1)
                }
                _ => false,
            })
    }
    walk(doc, outer, target, &mut std::collections::HashSet::new(), 0)
}

/// A layer name AutoCAD would accept: 1-255 characters, none of `<>/\":;?*|=\``.
pub(super) fn validated_layer_name(raw: &str) -> Result<String, String> {
    validated_symbol_name(raw, "a layer name")
}

/// A symbol-table name AutoCAD would accept (layers, styles): 1-255 characters,
/// none of `<>/\":;?*|=` or a backquote, and no control characters.
pub(super) fn validated_symbol_name(raw: &str, what: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(format!("{what} cannot be empty"));
    }
    if name.chars().count() > 255 {
        return Err(format!("{what} is limited to 255 characters"));
    }
    if let Some(bad) = name
        .chars()
        .find(|c| "<>/\\\":;?*|=`".contains(*c) || c.is_control())
    {
        return Err(format!("{what} cannot contain {bad:?}"));
    }
    Ok(name.to_owned())
}

/// Validate the entries an XRecord create or update wants to store: each needs
/// a group code the codec model admits (1..=369; 5 and 105 are reserved for
/// object handles). Refused before any change so bad input never reaches a
/// file.
pub(super) fn validated_xrecord_entries(
    entries: &[document::objects::XRecordEntry],
) -> Result<(), String> {
    for entry in entries {
        if !(1..=369).contains(&entry.code) {
            return Err(format!(
                "XRecord group code {} is out of range 1..=369",
                entry.code
            ));
        }
        if entry.code == 5 || entry.code == 105 {
            return Err(format!(
                "XRecord group code {} is reserved for the object handle",
                entry.code
            ));
        }
    }
    Ok(())
}

/// The registered panel `panel_id` names: its id, or loosely its title or
/// command.
pub(super) fn resolve_toggle_panel_id(app: &OpenCADStudio, panel_id: &str) -> Option<String> {
    if app.plugin_panel_ids.contains_key(panel_id) {
        return Some(panel_id.to_string());
    }
    let lower = panel_id.to_lowercase();
    let stripped = lower.replace([' ', '_', '-'], "");
    app.plugin_panel_ids
        .keys()
        .find(|key| {
            key.to_lowercase() == lower
                || key.replace([' ', '_', '-'], "").to_lowercase() == stripped
        })
        .cloned()
        .or_else(|| {
            app.plugin_panels
                .iter()
                .find(|(key, state)| {
                    key.to_lowercase() == lower
                        || key.replace([' ', '_', '-'], "").to_lowercase() == stripped
                        || state.def.title.to_lowercase() == lower
                        || state.def.title.replace([' ', '_', '-'], "").to_lowercase() == stripped
                        || state.def.command.as_ref().is_some_and(|command| {
                            command.to_lowercase() == lower
                                || command.replace([' ', '_', '-'], "").to_lowercase() == stripped
                        })
                })
                .map(|(key, _)| key.clone())
        })
}
