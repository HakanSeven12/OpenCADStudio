// WBLOCK — write a block (or selected entities) to an external DWG/DXF file.
//
// Two modes:
//   block name  → copies the named block definition to a new document
//   *           → copies currently selected model-space entities

use codec::{CadDocument, EntityType};
use crate::t;

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
    let mut out = CadDocument::new();
    extract_block_into(src, block_name, &mut out)?;
    Ok(out)
}

/// Extract the block's entities into `out` (a fresh document or a loaded
/// template whose tables/styles should survive — Catalog §2.1).
pub fn extract_block_into(
    src: &CadDocument,
    block_name: &str,
    out: &mut CadDocument,
) -> Result<(), String> {
    let br = src
        .block_records
        .get(block_name)
        .ok_or_else(|| t!("Block \"%{name}\" not found.", name = block_name).into_owned())?;

    let handles = br.entity_handles.clone();
    if handles.is_empty() {
        return Err(t!(
            "Block \"%{name}\" has no entities.",
            name = block_name
        )
        .into_owned());
    }

    copy_with_dependencies(src, &handles, out, None);

    if out.entities().count() == 0 {
        return Err(t!(
            "Block \"%{name}\" produced no exportable entities.",
            name = block_name
        )
        .into_owned());
    }

    Ok(())
}

/// Build a standalone `CadDocument` from an explicit list of entity handles
/// (the "selected entities" mode, `*`).
pub fn extract_entities_to_doc(
    src: &CadDocument,
    handles: &[codec::Handle],
) -> Result<CadDocument, String> {
    let mut out = CadDocument::new();
    extract_entities_into(src, handles, &mut out)?;
    Ok(out)
}

/// Extract the listed entities into `out` (fresh document or template base).
pub fn extract_entities_into(
    src: &CadDocument,
    handles: &[codec::Handle],
    out: &mut CadDocument,
) -> Result<(), String> {
    if handles.is_empty() {
        return Err(t!("No entities selected for WBLOCK.").into_owned());
    }

    copy_with_dependencies(src, handles, out, None);

    if out.entities().count() == 0 {
        return Err(t!("None of the selected entities could be exported.").into_owned());
    }

    Ok(())
}

/// Copy entities into `out` the way a cross-drawing paste does: fresh handles,
/// plus the layers, linetypes, styles, block definitions and baked dimension
/// blocks they use — without those an exported INSERT points at nothing and
/// the file opens empty or needs recovery. (#1111)
fn copy_with_dependencies(
    src: &CadDocument,
    handles: &[codec::Handle],
    out: &mut CadDocument,
    translate: Option<&crate::command::EntityTransform>,
) {
    let handles: Vec<codec::Handle> = handles
        .iter()
        .copied()
        .filter(|&handle| {
            !matches!(
                src.get_entity(handle),
                Some(EntityType::Block(_) | EntityType::BlockEnd(_))
            )
        })
        .collect();
    let (entities, deps) = crate::app::copy_to_clipboard_kernel(src, &handles);
    crate::app::paste_entities_kernel(out, &entities, &deps, translate);
}

/// Translate every entity of `out` so the overall bounds minimum lands on
/// the origin — SPM.ACAD's clone flow normalizes the new drawing to 0,0,0
/// after the copy (Catalog §2.1/CF-01.2). Opt-in per request; the
/// interactive WBLOCK export keeps the source coordinates.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn normalize_to_origin(out: &mut CadDocument) {
    let mut min = [f64::INFINITY; 3];
    for entity in out.entities() {
        let (lo, _) = crate::scene::convert::tess::entity_bounds(entity);
        for axis in 0..3 {
            if lo[axis] < min[axis] {
                min[axis] = lo[axis];
            }
        }
    }
    if min.iter().any(|v| !v.is_finite()) {
        return;
    }
    let shift = crate::command::EntityTransform::Affine(codec::types::Transform::from_translation(
        codec::types::Vector3::new(-min[0], -min[1], -min[2]),
    ));
    for entity in out.entities_mut() {
        crate::scene::view::dispatch::apply_transform(entity, &shift);
    }
}

/// One-shot point picker for the Write Block dialog's "Pick point" button.
pub struct WblockPickBasePointCommand;

impl crate::command::CadCommand for WblockPickBasePointCommand {
    fn name(&self) -> &'static str {
        "WBLOCK"
    }

    fn prompt(&self) -> String {
        crate::t!("WBLOCK  Specify insertion base point:").into_owned()
    }

    fn on_point(&mut self, pt: glam::DVec3) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch(format!("WBLOCK_POINT_PICKED {} {} {}", pt.x, pt.y, pt.z))
    }

    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch("WBLOCK_POINT_CANCELLED".to_string())
    }

    fn on_escape(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch("WBLOCK_POINT_CANCELLED".to_string())
    }
}

/// Build a standalone `CadDocument` from an explicit list of entity handles,
/// applying `base_point` translation and `unit` insertion units.
pub fn extract_entities_to_doc_with_base(
    src: &CadDocument,
    handles: &[codec::Handle],
    base_point: glam::DVec3,
    unit: i16,
) -> Result<CadDocument, String> {
    let mut out = CadDocument::new();
    out.header.insertion_units = unit;
    if handles.is_empty() {
        return Err(t!("No entities selected for WBLOCK.").into_owned());
    }
    let shift = crate::command::EntityTransform::Translate(-base_point);
    copy_with_dependencies(src, handles, &mut out, Some(&shift));
    if out.entities().count() == 0 {
        return Err(t!("None of the selected entities could be exported.").into_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::Line;
    use codec::types::Vector3;

    #[test]
    fn test_extract_entities_with_base_point_and_units() {
        let mut doc = CadDocument::new();
        let mut line = Line::new();
        line.start = Vector3::new(10.0, 20.0, 0.0);
        line.end = Vector3::new(30.0, 40.0, 0.0);
        let h = doc.add_entity(EntityType::Line(line)).unwrap();

        let out = extract_entities_to_doc_with_base(
            &doc,
            &[h],
            glam::DVec3::new(10.0, 20.0, 0.0),
            4, // Millimeters
        )
        .expect("extract should succeed");

        assert_eq!(out.header.insertion_units, 4);
        assert_eq!(out.entities().count(), 1);
        let e = out.entities().next().unwrap();
        if let EntityType::Line(l) = e {
            assert!((l.start.x - 0.0).abs() < 1e-6);
            assert!((l.start.y - 0.0).abs() < 1e-6);
            assert!((l.end.x - 20.0).abs() < 1e-6);
            assert!((l.end.y - 20.0).abs() < 1e-6);
        } else {
            panic!("Expected Line entity");
        }
    }

    #[test]
    fn test_extract_block_to_doc() {
        let mut doc = CadDocument::new();
        let mut line = Line::new();
        line.start = Vector3::new(1.0, 2.0, 0.0);
        line.end = Vector3::new(3.0, 4.0, 0.0);
        let h = doc.add_entity(EntityType::Line(line)).unwrap();

        let mut block_record = codec::tables::BlockRecord::new("MY_BLOCK".to_string());
        block_record.entity_handles.push(h);
        let _ = doc.block_records.add(block_record);

        let out = extract_block_to_doc(&doc, "MY_BLOCK").expect("block extract should succeed");
        assert_eq!(out.entities().count(), 1);

        assert!(extract_block_to_doc(&doc, "NON_EXISTENT").is_err());
    }
}

