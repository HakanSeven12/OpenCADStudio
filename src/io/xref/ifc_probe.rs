//! Native acceptance tests for direct IFC slices and DWG persistence.
use super::{
    ifc_slice::{self, View},
    *,
};
use crate::{
    ifc_plan,
    scene::{ChangeKind, Scene},
};
use document::{
    entities::{Dimension, DimensionLinear, Insert, Line},
    objects::{
        AssocDimensionAssociation, AssocDimensionReference, AssociativeData, AssociativeObject,
        ObjectType,
    },
    tables::BlockRecord,
};
use std::fs;

const IFC_NAME: &str = "building.ifc";
const BLOCK: &str = "IFC_BUILDING";

fn associations(doc: &Document) -> impl Iterator<Item = &AssocDimensionAssociation> {
    doc.objects.values().filter_map(|object| {
        let ObjectType::Associative(object) = object else {
            return None;
        };
        let AssociativeData::DimensionAssociation(association) = &object.data else {
            return None;
        };
        Some(association)
    })
}

const ORIGINAL: &[u8] = ifc_plan::TWO_WALLS_FIXTURE;
const MOVED: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/ifc-xref/wall-moved.ifc"
));
const OPENING: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/ifc-xref/wall-with-opening.ifc"
));

fn attach(scene: &mut Scene, dir: &Path, view: &View) -> (Handle, Handle) {
    let doc = &mut scene.document;
    doc.header.insertion_units = 4;
    ifc_slice::ensure_app(doc);
    let mut block = BlockRecord::new(BLOCK);
    block.handle = doc.allocate_handle();
    block.units = 4;
    block.flags.is_xref = true;
    block.xref_path = IFC_NAME.to_owned();
    let block_handle = block.handle;
    doc.block_records.add(block).unwrap();
    let mut insert = Insert::new(BLOCK, Vector3::new(2000.0, 3000.0, 0.0));
    view.set_on_insert(&mut insert);
    let insert_handle = doc.add_entity(EntityType::Insert(insert)).unwrap();
    assert!(matches!(
        reload(scene, dir, block_handle).status,
        XrefStatus::Loaded
    ));
    (block_handle, insert_handle)
}

fn reload(scene: &mut Scene, dir: &Path, block: Handle) -> XrefInfo {
    let mut infos =
        resolve_xrefs_for_keys(&mut scene.document, dir, &[block].into_iter().collect());
    assert_eq!(infos.len(), 1);
    let mut observed = HashSet::default();
    let changes: Vec<_> = associations(&scene.document)
        .flat_map(|association| association.references.iter().flatten())
        .flat_map(|reference| reference.xrefs.iter().copied())
        .filter(|handle| {
            scene.document.get_entity(*handle).is_some_and(|entity| {
                entity.common().owner_handle == block && observed.insert(*handle)
            })
        })
        .map(|handle| (handle, ChangeKind::Modified))
        .collect();
    if !changes.is_empty() {
        scene.bump_entities(&changes);
    }
    infos.pop().unwrap()
}

fn line_at(doc: &Document, block: Handle, x: f64, y: f64) -> (Handle, f64) {
    doc.entities()
        .filter_map(|entity| {
            let EntityType::Line(line) = entity else {
                return None;
            };
            let low = line.start.y.min(line.end.y);
            let high = line.start.y.max(line.end.y);
            (line.common.owner_handle == block
                && line.common.layer.rsplit('|').next() == Some(ifc_plan::CUT_LAYER)
                && close(line.start.x, x)
                && close(line.end.x, x)
                && low < y
                && y < high)
                .then(|| {
                    (
                        line.common.handle,
                        (y - line.start.y) / (line.end.y - line.start.y),
                    )
                })
        })
        .next()
        .expect("cut line at measurement point")
}

fn add_dimension(doc: &mut Document, block: Handle, insert: Handle, y: f64) -> Handle {
    let points = [
        Vector3::new(1900.0, y + 3000.0, 0.0),
        Vector3::new(7900.0, y + 3000.0, 0.0),
    ];
    let mut dimension = DimensionLinear::horizontal(points[0], points[1]);
    dimension.definition_point = points[1] + Vector3::new(0.0, -250.0, 0.0);
    let dimension = doc
        .add_entity(EntityType::Dimension(Box::new(Dimension::Linear(
            dimension,
        ))))
        .unwrap();
    let mut references: [Vec<AssocDimensionReference>; 4] = Default::default();
    let mut reactors = vec![insert, dimension];
    for (index, x) in [-100.0, 5900.0].into_iter().enumerate() {
        let (target, param) = line_at(doc, block, x, y);
        reactors.push(target);
        references[index].push(AssocDimensionReference {
            class_name: "AcDbOsnapPointRef".to_owned(),
            osnap_type: 10, // nearest
            main_subent_type: 1,
            main_gs_marker: 1,
            osnap_distance: param,
            osnap_point: points[index],
            xrefs: vec![insert, target],
            ..Default::default()
        });
    }
    let mut association = AssociativeObject::new("DIMASSOC", "AcDbDimAssoc");
    association.handle = doc.allocate_handle();
    association.reactors.push(dimension);
    association.data = AssociativeData::DimensionAssociation(AssocDimensionAssociation {
        dimension,
        associativity: 3,
        references,
        ..Default::default()
    });
    for handle in reactors {
        doc.get_entity_mut(handle)
            .unwrap()
            .common_mut()
            .reactors
            .push(association.handle);
    }
    doc.objects
        .insert(association.handle, ObjectType::Associative(association));
    dimension
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.05
}

fn measure(doc: &Document, id: Handle) -> f64 {
    let Some(EntityType::Dimension(dim)) = doc.get_entity(id) else {
        panic!("host linear dimension {id:?} is missing");
    };
    let Dimension::Linear(dim) = dim.as_ref() else {
        panic!("host dimension {id:?} is not linear");
    };
    dim.base.actual_measurement
}

fn targets(doc: &Document, id: Handle) -> [Option<Handle>; 2] {
    let assoc = associations(doc).find(|a| a.dimension == id).unwrap();
    std::array::from_fn(|index| {
        let reference = &assoc.references[index][0];
        (reference.xrefs.len() == 2 && reference.xref_paths.is_empty())
            .then(|| reference.xrefs[1])
            .filter(|handle| doc.get_entity(*handle).is_some())
    })
}

fn probe_case(name: &str, modified: &[u8], y: f64) {
    let model = ifc_plan::load_model(ORIGINAL).unwrap();
    let view = View::new(&model, 0, 1.2, 1.5).unwrap();
    let output = tempfile::tempdir().unwrap();
    let root = output.path();
    {
        let dir = root.join(name);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(IFC_NAME);
        fs::write(&path, ORIGINAL).unwrap();
        let mut scene = Scene::new();
        let reserved_handles = if name == "move" { 65_536 } else { 66_000 };
        for _ in 0..reserved_handles {
            scene.document.allocate_handle();
        }
        let (block, insert) = attach(&mut scene, &dir, &view);
        let id = add_dimension(&mut scene.document, block, insert, y);
        let before = targets(&scene.document, id);
        assert!(before.iter().all(Option::is_some));
        assert!(close(measure(&scene.document, id), 6000.0));
        let detail = scene
            .document
            .add_entity(EntityType::Line(Line::from_points(
                Vector3::new(2010.0, 3000.0, 0.0),
                Vector3::new(4000.0, 3000.0, 0.0),
            )))
            .unwrap();
        let own_detail = scene.document.get_entity(detail).unwrap().clone();
        fs::write(&path, modified).unwrap();
        let info = reload(&mut scene, &dir, block);
        let after = targets(&scene.document, id);
        println!(
            "{name} at y={y} mm: targets {before:?} -> {after:?}; measurement {}; diagnostics {:?}",
            measure(&scene.document, id),
            info.diagnostics
        );
        assert!(matches!(info.status, XrefStatus::Loaded));
        assert!(after[0].is_some());
        assert_ne!(
            before[0], after[0],
            "the copy in the host is renewed on reload"
        );
        let expected = if name == "move" { 7000.0 } else { 6000.0 };
        assert!(close(measure(&scene.document, id), expected));
        assert_eq!(after[1].is_some(), name == "move");
        assert_eq!(
            info.diagnostics
                .iter()
                .any(|d| d.contains("1 IFC XREF dimension reference")),
            name == "opening"
        );
        assert_eq!(scene.document.get_entity(detail), Some(&own_detail));

        let mut saved = scene.document.clone();
        strip_resolved_xref_content(&mut saved);
        for format in ["dwg", "dxf"] {
            let host = dir.join(format!("host.{format}"));
            let bytes = crate::io::save_to_bytes(&saved, format, saved.version).unwrap();
            merged_copies().lock().unwrap().remove(&block);
            let mut reopened = Scene::new();
            reopened.document = crate::io::load_bytes_finalized(&host, bytes).unwrap();
            let block = reopened.document.block_records.get(BLOCK).unwrap();
            assert_eq!(block.xref_path, IFC_NAME);
            let block = block.handle;
            assert_eq!(
                ifc_slice::views(&reopened.document).get(&block),
                Some(&view)
            );
            let info = reload(&mut reopened, &dir, block);
            let links = targets(&reopened.document, id);
            assert!(matches!(info.status, XrefStatus::Loaded));
            assert!(links[0].is_some(), "{format}: {links:?}");
            assert_eq!(links[1].is_some(), name == "move", "{format}");
            assert_eq!(
                info.diagnostics
                    .iter()
                    .any(|d| d.contains("1 IFC XREF dimension reference")),
                name == "opening"
            );
            assert!(close(measure(&reopened.document, id), expected));
            let EntityType::Line(reopened_detail) = reopened.document.get_entity(detail).unwrap()
            else {
                panic!("host detail is missing")
            };
            let EntityType::Line(own_detail) = &own_detail else {
                unreachable!()
            };
            assert_eq!(
                (reopened_detail.start, reopened_detail.end),
                (own_detail.start, own_detail.end)
            );
            let EntityType::Insert(placed) = reopened.document.get_entity(insert).unwrap() else {
                panic!("XREF insertion is missing")
            };
            assert_eq!(placed.insert_point, Vector3::new(2000.0, 3000.0, 0.0));
        }
    }
}

#[test]
fn direct_ifc_xref_moves_wall() {
    probe_case("move", MOVED, -1300.0);
}

#[test]
fn direct_ifc_xref_reports_split_at_opening() {
    probe_case("opening", OPENING, 250.0);
}

#[test]
fn direct_ifc_xref_shares_one_source_snapshot_per_read_batch() {
    let base = tempfile::tempdir().unwrap();
    let path = base.path().join(IFC_NAME);
    fs::create_dir(base.path().join("alias")).unwrap();
    fs::write(&path, ORIGINAL).unwrap();
    let model = ifc_plan::load_model(ORIGINAL).unwrap();
    let base_view = View::new(&model, 0, 1.2, 1.5).unwrap();
    let second_view = View::new(&model, 0, 1.0, 1.5).unwrap();

    let batch = ifc_slice::ReadBatch::default();
    let first = batch.read(IFC_NAME, base.path(), &base_view);
    assert!(matches!(first.outcome, Some(Ok(_))));

    fs::write(&path, b"not an IFC file").unwrap();
    let via_alias = batch.read(&format!("alias/../{IFC_NAME}"), base.path(), &second_view);
    assert!(matches!(via_alias.outcome, Some(Ok(_))));

    let fresh = ifc_slice::ReadBatch::default().read(IFC_NAME, base.path(), &second_view);
    assert!(matches!(fresh.outcome, Some(Err(ref error)) if error.contains("STEP content")));
}

#[test]
fn direct_ifc_xref_keeps_last_good_slice_and_uses_the_storey_global_id() {
    let output = tempfile::tempdir().unwrap();
    let dir = output.path().to_path_buf();
    let path = dir.join(IFC_NAME);
    fs::write(&path, ORIGINAL).unwrap();
    let model = ifc_plan::load_model(ORIGINAL).unwrap();
    let view = View::new(&model, 0, 1.2, 1.5).unwrap();
    let mut scene = Scene::new();
    for _ in 0..67_000 {
        scene.document.allocate_handle();
    }
    let (block, insert) = attach(&mut scene, &dir, &view);
    let dimension = add_dimension(&mut scene.document, block, insert, -1300.0);
    let handles = |doc: &Document| -> HashSet<Handle> {
        ifc_slice::source_keys(doc, block)
            .unwrap()
            .into_values()
            .collect()
    };
    let expected = handles(&scene.document);
    assert!(!expected.is_empty());
    let same = reload(&mut scene, &dir, block);
    assert!(matches!(same.status, XrefStatus::Loaded));
    assert_eq!(handles(&scene.document), expected);
    assert!(targets(&scene.document, dimension)
        .iter()
        .all(Option::is_some));

    fs::remove_file(&path).unwrap();
    let missing = reload(&mut scene, &dir, block);
    assert!(matches!(missing.status, XrefStatus::NotFound));
    assert_eq!(handles(&scene.document), expected);
    let source = std::str::from_utf8(ORIGINAL).unwrap();
    let other = source.replace("0000000000000000000001'", "1000000000000000000001'");
    let missing_storey = source.replace("0000000000000000000003'", "1000000000000000000003'");
    for (bytes, message) in [
        (&b"not an IFC file"[..], "STEP content"),
        (other.as_bytes(), "different project"),
        (missing_storey.as_bytes(), "storey is missing"),
    ] {
        fs::write(&path, bytes).unwrap();
        let failed = reload(&mut scene, &dir, block);
        assert!(matches!(failed.status, XrefStatus::Failed));
        assert!(
            failed.diagnostics.iter().any(|line| line.contains(message)),
            "{failed:?}"
        );
        assert_eq!(handles(&scene.document), expected);
        assert!(close(measure(&scene.document, dimension), 6000.0));
        assert!(targets(&scene.document, dimension)
            .iter()
            .all(Option::is_some));
    }

    assert!(!source.contains("#120"));
    let renamed = source
        .replace("Ground floor", "Renamed ground floor")
        .replace("#12", "#120");
    fs::write(&path, renamed).unwrap();
    let restored = reload(&mut scene, &dir, block);
    assert!(matches!(restored.status, XrefStatus::Loaded));
    assert_eq!(handles(&scene.document), expected);
    assert!(targets(&scene.document, dimension)
        .iter()
        .all(Option::is_some));
    println!("unchanged, missing, invalid, wrong project, missing storey, same GlobalId after rename and STEP renumber: verified");
}
