// Faz 4c end-to-end: OCS synthesizes a real per-object annotation context
// (extension-dict chain + `AcDb*ObjectContextData` leaf) for an entity, and it
// survives a DWG save/reload — i.e. an OCS-authored annotative object carries a
// genuine per-scale representation, interoperably, not just the native flag.

use codec::entities::{EntityType, MText};
use codec::objects::{Dictionary, ObjectContextKind, ObjectType, Scale};
use codec::types::Vector3;
use codec::{Document, DxfVersion, Handle};
use OpenCADStudio::io;
use OpenCADStudio::scene::annotative;

#[test]
fn empty_annotation_scales_is_not_annotative() {
    // An object whose AcDbContextDataManager -> ACDB_ANNOTATIONSCALES collection
    // is EMPTY (a single-representation marker with no per-scale reps — common in
    // files where objects were flagged annotative but never given a scale) must
    // NOT be treated as annotative. Otherwise it gets (mis)scaled by the
    // annotation factor inside annotation-scaled viewports, ballooning the text.
    let mut doc = Document::with_version(DxfVersion::AC1032);
    let mut m = MText::new();
    m.is_annotative = false; // no native flag, no annotative style
    let ent = doc.add_entity(EntityType::MText(Box::new(m))).unwrap();

    // Build xdict -> "AcDbContextDataManager" -> "ACDB_ANNOTATIONSCALES" (empty).
    let mk = |doc: &mut Document, owner: Handle| -> Handle {
        let h = doc.allocate_handle();
        let mut d = Dictionary::new();
        d.handle = h;
        d.owner = owner;
        doc.objects.insert(h, ObjectType::Dictionary(d));
        h
    };
    let xd = mk(&mut doc, ent);
    let mgr = mk(&mut doc, xd);
    let coll = mk(&mut doc, mgr); // left empty
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&xd) {
        d.add_entry("AcDbContextDataManager", mgr);
    }
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&mgr) {
        d.add_entry("ACDB_ANNOTATIONSCALES", coll);
    }
    if let Some(e) = doc.get_entity_mut(ent) {
        e.common_mut().xdictionary_handle = Some(xd);
    }

    assert!(
        !annotative::is_annotative(&doc, doc.get_entity(ent).unwrap()),
        "empty ACDB_ANNOTATIONSCALES must not read as annotative"
    );

    // Adding a real per-scale representation makes it annotative again.
    let sh = doc.allocate_handle();
    let mut s = Scale::new("1:50", 1.0, 50.0);
    s.handle = sh;
    doc.objects.insert(sh, ObjectType::Scale(s));
    assert!(annotative::create_annotation_context(&mut doc, ent, sh));
    assert!(
        annotative::is_annotative(&doc, doc.get_entity(ent).unwrap()),
        "a non-empty scale list must read as annotative"
    );
}

/// Walk an entity's xdict → "AcDbContextDataManager" → "ACDB_ANNOTATIONSCALES"
/// and return its leaf handles.
fn context_leaves(doc: &Document, entity: Handle) -> Vec<Handle> {
    let Some(xd) = doc
        .get_entity(entity)
        .and_then(|e| e.common().xdictionary_handle)
    else {
        return vec![];
    };
    let Some(mgr) = annotative::as_dict(doc, xd).and_then(|d| d.get("AcDbContextDataManager"))
    else {
        return vec![];
    };
    let Some(coll) = annotative::as_dict(doc, mgr).and_then(|d| d.get("ACDB_ANNOTATIONSCALES"))
    else {
        return vec![];
    };
    annotative::as_dict(doc, coll)
        .map(|d| d.entries.iter().map(|(_, h)| *h).collect())
        .unwrap_or_default()
}

#[test]
fn ocs_synthesized_mtext_context_survives_dwg_roundtrip() {
    let mut doc = Document::with_version(DxfVersion::AC1032);

    // An MTEXT plus a named scale to attach it to.
    let mut m = MText::new();
    m.insertion_point = Vector3::new(100.0, 200.0, 0.0);
    m.rectangle_width = 50.0;
    m.is_annotative = true;
    let ent = doc.add_entity(EntityType::MText(Box::new(m))).expect("add mtext");

    let sh = doc.allocate_handle();
    let mut scale = Scale::new("1:50", 1.0, 50.0);
    scale.handle = sh;
    doc.objects.insert(sh, ObjectType::Scale(scale));

    // OCS synthesizes the whole context chain.
    assert!(
        annotative::create_annotation_context(&mut doc, ent, sh),
        "create_annotation_context should succeed for MTEXT"
    );
    // Idempotent: a second call for the same scale must not add a second leaf.
    assert!(annotative::create_annotation_context(&mut doc, ent, sh));
    assert_eq!(context_leaves(&doc, ent).len(), 1, "duplicate leaf created");

    // Full DWG round-trip through the OCS IO layer.
    let bytes = io::save_to_bytes(&doc, "dwg", DxfVersion::AC1032).expect("save dwg");
    let doc2 = io::load_bytes("rt.dwg", bytes).expect("reload dwg");

    // The reloaded entity still carries exactly one leaf, a modeled MTEXT
    // context, whose 340 resolves to the "1:50" scale.
    let leaves = context_leaves(&doc2, ent);
    assert_eq!(leaves.len(), 1, "context leaf lost across round-trip");
    match doc2.objects.get(&leaves[0]) {
        Some(ObjectType::ObjectContextData(c)) => {
            assert!(
                matches!(c.kind, ObjectContextKind::MText(_)),
                "leaf is not an MTEXT context"
            );
            match doc2.objects.get(&c.scale) {
                Some(ObjectType::Scale(s)) => assert_eq!(s.name, "1:50", "wrong scale link"),
                other => panic!("scale handle did not resolve to a Scale: {other:?}"),
            }
        }
        other => panic!("leaf is not an ObjectContextData: {other:?}"),
    }
    // The reader also side-maps the leaf's annotation scale.
    assert!(
        doc2.context_scales.contains_key(&leaves[0]),
        "leaf missing from context_scales"
    );
}

#[test]
fn ocs_synthesized_block_context_survives_dwg_roundtrip() {
    use codec::entities::Insert;

    let mut doc = Document::with_version(DxfVersion::AC1032);

    let mut ins = Insert::new("BLK", Vector3::new(10.0, 20.0, 0.0));
    ins.rotation = 0.5;
    let ent = doc.add_entity(EntityType::Insert(ins)).expect("add insert");

    let sh = doc.allocate_handle();
    let mut scale = Scale::new("1:100", 1.0, 100.0);
    scale.handle = sh;
    doc.objects.insert(sh, ObjectType::Scale(scale));

    assert!(annotative::create_annotation_context(&mut doc, ent, sh));

    let bytes = io::save_to_bytes(&doc, "dwg", DxfVersion::AC1032).expect("save dwg");
    let doc2 = io::load_bytes("rt.dwg", bytes).expect("reload dwg");

    let leaves = context_leaves(&doc2, ent);
    assert_eq!(leaves.len(), 1, "block context leaf lost");
    match doc2.objects.get(&leaves[0]) {
        Some(ObjectType::ObjectContextData(c)) => {
            assert!(
                matches!(c.kind, ObjectContextKind::BlkRef { .. }),
                "not a BLKREF context"
            );
        }
        other => panic!("leaf is not an ObjectContextData: {other:?}"),
    }
}

#[test]
fn object_scale_membership_add_remove_roundtrips() {
    let mut doc = Document::with_version(DxfVersion::AC1032);
    let mut m = MText::new();
    m.insertion_point = Vector3::new(1.0, 2.0, 0.0);
    let ent = doc.add_entity(EntityType::MText(Box::new(m))).unwrap();

    let mk_scale = |doc: &mut Document, name: &str, d: f64| {
        let h = doc.allocate_handle();
        let mut s = Scale::new(name, 1.0, d);
        s.handle = h;
        doc.objects.insert(h, ObjectType::Scale(s));
        h
    };
    let s50 = mk_scale(&mut doc, "1:50", 50.0);
    let s100 = mk_scale(&mut doc, "1:100", 100.0);

    // Add two memberships.
    assert!(annotative::create_annotation_context(&mut doc, ent, s50));
    assert!(annotative::create_annotation_context(&mut doc, ent, s100));
    let mut names: Vec<String> = annotative::object_scale_memberships(&doc, ent)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    names.sort();
    assert_eq!(names, vec!["1:100", "1:50"], "both memberships expected");

    // Remove one, keep the other; survives a round-trip.
    assert!(annotative::remove_annotation_context_for_scale(
        &mut doc, ent, s50
    ));
    let bytes = io::save_to_bytes(&doc, "dwg", DxfVersion::AC1032).unwrap();
    let doc2 = io::load_bytes("rt.dwg", bytes).unwrap();
    let remaining: Vec<String> = annotative::object_scale_memberships(&doc2, ent)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(
        remaining,
        vec!["1:100"],
        "exactly the un-removed scale remains"
    );

    // Removing the last representation tears the chain down → non-annotative.
    let mut doc3 = doc2.clone();
    // resolve the surviving scale handle in the reloaded doc
    let s100b = doc3
        .objects
        .iter()
        .find_map(|(h, o)| matches!(o, ObjectType::Scale(s) if s.name == "1:100").then_some(*h))
        .unwrap();
    assert!(annotative::remove_annotation_context_for_scale(
        &mut doc3, ent, s100b
    ));
    assert!(
        annotative::object_scale_memberships(&doc3, ent).is_empty(),
        "no memberships after removing the last"
    );
    assert!(
        !annotative::is_annotative(&doc3, doc3.get_entity(ent).unwrap()),
        "object should be non-annotative once its last representation is gone"
    );
}

/// A context's fit overrides reach the dimension drawn at its scale. AutoCAD
/// 2025 keeps, for a 1:50 context, DIMTMOVE 1 (leader) with override code 16
/// under a style whose DIMTMOVE is 0, and draws that scale with the leader.
#[test]
fn dimension_context_fit_overrides_reach_the_drawn_dimension() {
    use codec::entities::{Dimension, DimensionLinear};
    use codec::xdata::XDataValue;
    use OpenCADStudio::entities::dim_override as ov;

    let mut doc = Document::with_version(DxfVersion::AC1032);
    let dim = DimensionLinear::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(150.0, 0.0, 0.0));
    let ent = doc
        .add_entity(EntityType::Dimension(Box::new(Dimension::Linear(dim))))
        .unwrap();
    let sh = doc.allocate_handle();
    let mut s = Scale::new("1:50", 1.0, 50.0);
    s.handle = sh;
    doc.objects.insert(sh, ObjectType::Scale(s));
    assert!(annotative::create_annotation_context(&mut doc, ent, sh));
    let leaf = context_leaves(&doc, ent)[0];

    let drawn_dimtmove = |doc: &Document| {
        let drawn =
            annotative::entity_for_annotation_context(doc, doc.get_entity(ent).unwrap(), Some(sh));
        ov::int(&drawn.common().extended_data, ov::DIMTMOVE)
    };
    let set_context = |doc: &mut Document, dimtmove: bool, override_code: u8| {
        let Some(ObjectType::ObjectContextData(c)) = doc.objects.get_mut(&leaf) else {
            panic!("dimension context leaf");
        };
        let ObjectContextKind::Dim(d) = &mut c.kind else {
            panic!("dimension context kind");
        };
        d.dimtmove = dimtmove;
        d.override_code = override_code;
    };

    // Not flagged as overridden: the style decides.
    set_context(&mut doc, true, 0);
    assert_eq!(drawn_dimtmove(&doc), None);

    set_context(&mut doc, true, 16);
    assert_eq!(drawn_dimtmove(&doc), Some(1));
    // Only the drawn copy carries it; the stored dimension is unchanged.
    assert_eq!(
        ov::int(
            &doc.get_entity(ent).unwrap().common().extended_data,
            ov::DIMTMOVE
        ),
        None
    );

    // The dimension's own override still wins.
    ov::set(&mut doc, ent, ov::DIMTMOVE, Some(XDataValue::Integer16(2)));
    assert_eq!(drawn_dimtmove(&doc), Some(2));
}
