// WBLOCK — write a block (or selected entities) to an external DWG/DXF file.
//
// Two modes:
//   block name  → copies the named block definition to a new document
//   *           → copies currently selected model-space entities

use crate::t;
use codec::{Document, EntityType};

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

/// Build a standalone `Document` containing the named block's entities
/// extracted into model space.
///
/// Returns `Err` if the block is not found or has no entities.
pub fn extract_block_to_doc(src: &Document, block_name: &str) -> Result<Document, String> {
    let mut out = Document::new();
    extract_block_into(src, block_name, &mut out)?;
    Ok(out)
}

/// Extract the block's entities into `out` (a fresh document or a loaded
/// template whose tables/styles should survive — Catalog §2.1).
pub fn extract_block_into(
    src: &Document,
    block_name: &str,
    out: &mut Document,
) -> Result<(), String> {
    let br = src
        .block_records
        .get(block_name)
        .ok_or_else(|| t!("Block \"%{name}\" not found.", name = block_name).into_owned())?;

    let handles = br.entity_handles.clone();
    if handles.is_empty() {
        return Err(t!("Block \"%{name}\" has no entities.", name = block_name).into_owned());
    }

    copy_with_dependencies(src, &handles, out, None);

    if out.entities().count() == 0 {
        return Err(t!(
            "Block \"%{name}\" produced no exportable entities.",
            name = block_name
        )
        .into_owned());
    }

    out.header.insertion_units = src.header.insertion_units;
    inherit_source_header(src, out);
    Ok(())
}

/// A WBLOCK output inherits the source drawing's measurement system and save
/// dates, and its extents are those of the geometry it actually holds.
fn inherit_source_header(src: &Document, out: &mut Document) {
    out.header.measurement = src.header.measurement;
    // A source that has been saved carries the drawing's creation stamp;
    // a template base keeps its own when the source has none.
    if src.header.create_date_julian != 0.0 || src.header.universal_create_date_julian != 0.0 {
        out.header.create_date_julian = src.header.create_date_julian;
        out.header.universal_create_date_julian = src.header.universal_create_date_julian;
    }
    // The export is written now: its update stamp is the source's save stamp,
    // which an unsaved source and a template base do not have.
    if src.header.update_date_julian != 0.0 || src.header.universal_update_date_julian != 0.0 {
        out.header.update_date_julian = src.header.update_date_julian;
        out.header.universal_update_date_julian = src.header.universal_update_date_julian;
    }
    recompute_model_space_extents(out);
}

/// Handles of the entities placed in model space; entities that belong to a
/// block definition travel through the references that use them.
fn model_space_handles(out: &Document) -> Vec<codec::Handle> {
    let model_space = out.header.model_space_block_handle;
    out.entities()
        .filter(|entity| {
            let owner = entity.common().owner_handle;
            owner.is_null() || owner == model_space
        })
        .map(|entity| entity.common().handle)
        .collect()
}

/// EXTMIN / EXTMAX as the geometry in model space actually measures, with
/// references measured through their block definitions.
fn recompute_model_space_extents(out: &mut Document) {
    let handles = model_space_handles(out);
    if let Some(bounds) = crate::app::helpers::entities_bounds(out, &handles) {
        out.header.model_space_extents_min = bounds.min;
        out.header.model_space_extents_max = bounds.max;
    }
}

/// Build a standalone `Document` from an explicit list of entity handles
/// (the "selected entities" mode, `*`).
pub fn extract_entities_to_doc(
    src: &Document,
    handles: &[codec::Handle],
) -> Result<Document, String> {
    let mut out = Document::new();
    extract_entities_into(src, handles, &mut out)?;
    Ok(out)
}

/// Extract the listed entities into `out` (fresh document or template base).
pub fn extract_entities_into(
    src: &Document,
    handles: &[codec::Handle],
    out: &mut Document,
) -> Result<(), String> {
    if handles.is_empty() {
        return Err(t!("No entities selected for WBLOCK.").into_owned());
    }

    copy_with_dependencies(src, handles, out, None);

    if out.entities().count() == 0 {
        return Err(t!("None of the selected entities could be exported.").into_owned());
    }

    out.header.insertion_units = src.header.insertion_units;
    inherit_source_header(src, out);
    Ok(())
}

/// Copy entities into `out` the way a cross-drawing paste does: fresh handles,
/// plus the layers, linetypes, styles, block definitions and baked dimension
/// blocks they use — without those an exported INSERT points at nothing and
/// the file opens empty or needs recovery. (#1111)
fn copy_with_dependencies(
    src: &Document,
    handles: &[codec::Handle],
    out: &mut Document,
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
    crate::app::paste_entities_kernel(
        out,
        &entities,
        &deps,
        translate,
        crate::app::PasteSpace::Model,
    );
}

/// Translate every entity of `out` so the overall bounds minimum lands on
/// the origin — SPM.ACAD's clone flow normalizes the new drawing to 0,0,0
/// after the copy (Catalog §2.1/CF-01.2). Opt-in per request; the
/// interactive WBLOCK export keeps the source coordinates.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn normalize_to_origin(out: &mut Document) {
    let model_space = out.header.model_space_block_handle;
    let handles = model_space_handles(out);
    let Some(bounds) = crate::app::helpers::entities_bounds(out, &handles) else {
        return;
    };
    let min = bounds.min;
    if !(min.x.is_finite() && min.y.is_finite() && min.z.is_finite()) {
        return;
    }
    let shift = crate::command::EntityTransform::Affine(codec::types::Transform::from_translation(
        codec::types::Vector3::new(-min.x, -min.y, -min.z),
    ));
    // Only what sits in model space moves; a definition's own entities keep
    // their coordinates and the reference carries them.
    for entity in out.entities_mut() {
        let owner = entity.common().owner_handle;
        if !owner.is_null() && owner != model_space {
            continue;
        }
        crate::scene::view::dispatch::apply_transform(entity, &shift);
    }
    recompute_model_space_extents(out);
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
        crate::command::CmdResult::Dispatch(format!(
            "WBLOCK_POINT_PICKED {} {} {}",
            pt.x, pt.y, pt.z
        ))
    }

    fn on_enter(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch("WBLOCK_POINT_CANCELLED".to_string())
    }

    fn on_escape(&mut self) -> crate::command::CmdResult {
        crate::command::CmdResult::Dispatch("WBLOCK_POINT_CANCELLED".to_string())
    }
}

/// Build a standalone `Document` from an explicit list of entity handles,
/// applying `base_point` translation and `unit` insertion units.
pub fn extract_entities_to_doc_with_base(
    src: &Document,
    handles: &[codec::Handle],
    base_point: glam::DVec3,
    unit: i16,
) -> Result<Document, String> {
    let mut out = Document::new();
    out.header.insertion_units = unit;
    if handles.is_empty() {
        return Err(t!("No entities selected for WBLOCK.").into_owned());
    }
    // The objects keep their coordinates; the base point becomes the new
    // drawing's insertion base (INSBASE).
    copy_with_dependencies(src, handles, &mut out, None);
    out.header.model_space_insertion_base =
        codec::types::Vector3::new(base_point.x, base_point.y, base_point.z);
    if out.entities().count() == 0 {
        return Err(t!("None of the selected entities could be exported.").into_owned());
    }
    inherit_source_header(src, &mut out);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::Line;
    use codec::types::Vector3;

    #[test]
    fn test_extract_entities_with_base_point_and_units() {
        let mut doc = Document::new();
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
        assert_eq!(
            out.header.model_space_insertion_base,
            Vector3::new(10.0, 20.0, 0.0)
        );
        let e = out.entities().next().unwrap();
        if let EntityType::Line(l) = e {
            assert!((l.start.x - 10.0).abs() < 1e-6);
            assert!((l.start.y - 20.0).abs() < 1e-6);
            assert!((l.end.x - 30.0).abs() < 1e-6);
            assert!((l.end.y - 40.0).abs() < 1e-6);
        } else {
            panic!("Expected Line entity");
        }
    }

    #[test]
    fn test_extract_block_to_doc() {
        let mut doc = Document::new();
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

    #[test]
    fn an_extracted_insert_is_measured_through_its_block_definition() {
        let mut doc = Document::new();
        let mut line = Line::new();
        line.start = Vector3::new(10.0, 20.0, 0.0);
        line.end = Vector3::new(30.0, 40.0, 0.0);
        let inner_line = doc.add_entity(EntityType::Line(line)).unwrap();
        let mut inner = codec::tables::BlockRecord::new("INNER");
        inner.entity_handles.push(inner_line);
        doc.block_records.add(inner).unwrap();

        let insert = codec::entities::Insert::new("INNER", Vector3::new(100.0, 200.0, 0.0));
        let insert_handle = doc.add_entity(EntityType::Insert(insert)).unwrap();
        let mut outer = codec::tables::BlockRecord::new("OUTER");
        outer.entity_handles.push(insert_handle);
        doc.block_records.add(outer).unwrap();

        let out = extract_block_to_doc(&doc, "OUTER").expect("block extract should succeed");

        assert_eq!(
            out.header.model_space_extents_min,
            Vector3::new(110.0, 220.0, 0.0)
        );
        assert_eq!(
            out.header.model_space_extents_max,
            Vector3::new(130.0, 240.0, 0.0)
        );
    }

    #[test]
    fn normalizing_shifts_the_model_and_not_the_definitions() {
        let mut doc = Document::new();
        let mut line = Line::new();
        line.start = Vector3::new(5.0, 5.0, 0.0);
        line.end = Vector3::new(15.0, 15.0, 0.0);
        let inner_line = doc.add_entity(EntityType::Line(line)).unwrap();
        let mut part = codec::tables::BlockRecord::new("PART");
        part.entity_handles.push(inner_line);
        doc.block_records.add(part).unwrap();

        let insert = codec::entities::Insert::new("PART", Vector3::new(100.0, 100.0, 0.0));
        let insert_handle = doc.add_entity(EntityType::Insert(insert)).unwrap();

        let mut out = extract_entities_to_doc(&doc, &[insert_handle]).expect("selection extract");
        normalize_to_origin(&mut out);

        assert_eq!(
            out.header.model_space_extents_min,
            Vector3::new(0.0, 0.0, 0.0)
        );
        assert_eq!(
            out.header.model_space_extents_max,
            Vector3::new(10.0, 10.0, 0.0)
        );
        let placed = out
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(insert) => Some(insert.insert_point),
                _ => None,
            })
            .expect("the insert survives the export");
        assert_eq!(placed, Vector3::new(-5.0, -5.0, 0.0));
    }

    #[test]
    fn an_extracted_drawing_keeps_the_source_unit() {
        let mut doc = Document::new();
        doc.header.insertion_units = 6;
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(1.0, 1.0, 0.0);
        let h = doc.add_entity(EntityType::Line(line)).unwrap();
        let mut record = codec::tables::BlockRecord::new("PART");
        record.entity_handles.push(h);
        doc.block_records.add(record).unwrap();

        let block = extract_block_to_doc(&doc, "PART").expect("block extract should succeed");
        assert_eq!(block.header.insertion_units, 6);

        let selection = extract_entities_to_doc(&doc, &[h]).expect("selection extract");
        assert_eq!(selection.header.insertion_units, 6);
    }

    #[test]
    fn a_template_base_keeps_its_dates_when_the_source_never_saved() {
        let mut doc = Document::new();
        doc.header.measurement = 1;
        let mut line = Line::new();
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(1.0, 1.0, 0.0);
        let h = doc.add_entity(EntityType::Line(line)).unwrap();
        let mut record = codec::tables::BlockRecord::new("PART");
        record.entity_handles.push(h);
        doc.block_records.add(record).unwrap();

        let mut template = Document::new();
        template.header.create_date_julian = 2_400_000.0;
        template.header.update_date_julian = 2_400_001.0;
        extract_block_into(&doc, "PART", &mut template).expect("template extract should succeed");

        assert_eq!(template.header.create_date_julian, 2_400_000.0);
        assert_eq!(template.header.update_date_julian, 2_400_001.0);
        assert_eq!(template.header.measurement, 1);
    }

    #[test]
    fn an_extracted_block_carries_the_source_header_and_its_own_extents() {
        let mut doc = Document::new();
        doc.header.measurement = 1;
        doc.header.create_date_julian = 2_460_000.5;
        doc.header.universal_create_date_julian = 2_460_000.0;
        doc.header.update_date_julian = 2_460_001.5;
        doc.header.universal_update_date_julian = 2_460_001.0;
        let mut line = Line::new();
        line.start = Vector3::new(20.0, 0.0, 0.0);
        line.end = Vector3::new(212.0, 206.0, 0.0);
        let h = doc.add_entity(EntityType::Line(line)).unwrap();
        let mut record = codec::tables::BlockRecord::new("PART");
        record.entity_handles.push(h);
        doc.block_records.add(record).unwrap();

        let out = extract_block_to_doc(&doc, "PART").expect("block extract should succeed");

        assert_eq!(out.header.measurement, 1);
        assert_eq!(out.header.create_date_julian, 2_460_000.5);
        assert_eq!(out.header.universal_create_date_julian, 2_460_000.0);
        assert_eq!(out.header.update_date_julian, 2_460_001.5);
        assert_eq!(
            out.header.model_space_extents_min,
            Vector3::new(20.0, 0.0, 0.0)
        );
        assert_eq!(
            out.header.model_space_extents_max,
            Vector3::new(212.0, 206.0, 0.0)
        );
    }

    #[test]
    fn an_extracted_selection_carries_the_source_header_and_its_own_extents() {
        let mut doc = Document::new();
        doc.header.measurement = 1;
        doc.header.create_date_julian = 2_460_000.5;
        doc.header.universal_create_date_julian = 2_460_000.0;
        let mut line = Line::new();
        line.start = Vector3::new(10.0, 20.0, 0.0);
        line.end = Vector3::new(30.0, 40.0, 0.0);
        let h = doc.add_entity(EntityType::Line(line)).unwrap();

        let out = extract_entities_to_doc(&doc, &[h]).expect("selection extract should succeed");

        assert_eq!(out.header.measurement, 1);
        assert_eq!(out.header.create_date_julian, 2_460_000.5);
        assert_eq!(out.header.universal_create_date_julian, 2_460_000.0);
        assert_eq!(
            out.header.model_space_extents_min,
            Vector3::new(10.0, 20.0, 0.0)
        );
        assert_eq!(
            out.header.model_space_extents_max,
            Vector3::new(30.0, 40.0, 0.0)
        );
    }
}
