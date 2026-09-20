// WBLOCK — write a block (or selected entities) to an external DWG/DXF file.
//
// Two modes:
//   block name  → copies the named block definition to a new document
//   *           → copies currently selected model-space entities

use crate::t;
use acadrust::{CadDocument, EntityType};

use crate::modules::{IconKind, ModuleEvent, ToolDef};

#[allow(dead_code)]
pub fn tool() -> ToolDef {
    ToolDef {
        id: "WBLOCK",
        label: "Write Block",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg")),
        event: ModuleEvent::Command("WBLOCK".to_string()),
    }
}

/// Build a standalone `CadDocument` containing the named block's entities
/// extracted into model space.
///
/// Returns `Err` if the block is not found or has no entities.
pub fn extract_block_to_doc(src: &CadDocument, block_name: &str) -> Result<CadDocument, String> {
    let br = src
        .block_records
        .get(block_name)
        .ok_or_else(|| t!("Block \"%{name}\" not found.", name = block_name).into_owned())?;

    let handles = br.entity_handles.clone();
    if handles.is_empty() {
        return Err(t!("Block \"%{name}\" has no entities.", name = block_name).into_owned());
    }

    let mut out = CadDocument::new();
    // Copy layers referenced by the block entities.
    for h in &handles {
        if let Some(e) = src.get_entity(*h) {
            let layer = e.common().layer.clone();
            if !layer.is_empty() && !layer.eq("0") && out.layers.get(&layer).is_none() {
                if let Some(src_layer) = src.layers.get(&layer) {
                    let _ = out.layers.add(src_layer.clone());
                }
            }
        }
    }

    for h in handles {
        if let Some(entity) = src.get_entity(h) {
            if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                continue;
            }
            let mut clone = entity.clone();
            clone.common_mut().handle = acadrust::types::Handle::NULL;
            clone.common_mut().owner_handle = acadrust::types::Handle::NULL;
            let _ = out.add_entity(clone);
        }
    }

    if out.entities().count() == 0 {
        return Err(t!(
            "Block \"%{name}\" produced no exportable entities.",
            name = block_name
        )
        .into_owned());
    }

    Ok(out)
}

/// Build a standalone `CadDocument` from an explicit list of entity handles
/// (the "selected entities" mode, `*`).
pub fn extract_entities_to_doc(
    src: &CadDocument,
    handles: &[acadrust::Handle],
) -> Result<CadDocument, String> {
    if handles.is_empty() {
        return Err(t!("No entities selected for WBLOCK.").into_owned());
    }

    let mut out = CadDocument::new();

    for &h in handles {
        if let Some(entity) = src.get_entity(h) {
            if matches!(entity, EntityType::Block(_) | EntityType::BlockEnd(_)) {
                continue;
            }
            // Copy layer definition.
            let layer = entity.common().layer.clone();
            if !layer.is_empty() && !layer.eq("0") && out.layers.get(&layer).is_none() {
                if let Some(src_layer) = src.layers.get(&layer) {
                    let _ = out.layers.add(src_layer.clone());
                }
            }
            let mut clone = entity.clone();
            clone.common_mut().handle = acadrust::types::Handle::NULL;
            clone.common_mut().owner_handle = acadrust::types::Handle::NULL;
            let _ = out.add_entity(clone);
        }
    }

    if out.entities().count() == 0 {
        return Err(t!("None of the selected entities could be exported.").into_owned());
    }

    Ok(out)
}
