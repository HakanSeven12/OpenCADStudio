//! DATA-09: an MTEXT's relations (XData, extension dictionary, group,
//! annotation scales, block ownership) survive every kind of editor commit.
use super::*;
use crate::app::{Message, OpenCADStudio};
use crate::scene::annotative as anno;
use codec::Document;

const APP: &str = "OCS_DATA09";
const BLOCK: &str = "DATA09_BLOCK";
const RAW: &str = r"\pxqc;{\fArial|b0|i0|c238|p34;Árvíztűrő} {\C1;tükör}\Pfúrógép Ø±";

struct Fixture {
    app: OpenCADStudio,
    model: Handle,
    block: Handle,
}

fn xdata(tag: &str) -> codec::xdata::ExtendedData {
    use codec::xdata::{ExtendedData, ExtendedDataRecord, XDataValue};
    let mut record = ExtendedDataRecord::new(APP);
    for value in [
        XDataValue::String(format!("megőrzés {tag}")),
        XDataValue::Real(1.234_567_890_123),
        XDataValue::Integer16(42),
        XDataValue::Point3D(Vector3::new(1.5, -2.25, 0.)),
    ] {
        record.add_value(value);
    }
    let mut data = ExtendedData::new();
    data.add_record(record);
    data
}

fn rich_mtext(tag: &str) -> MText {
    let mut t = MText::with_value(RAW, Vector3::new(12.5, -7.25, 0.));
    t.height = 2.75;
    t.rotation = 0.5;
    t.rectangle_width = 40.;
    t.rectangle_height = Some(12.);
    t.line_spacing_factor = 1.25;
    t.line_spacing_style = codec::entities::LineSpacingStyle::Exactly;
    t.attachment_point = AttachmentPoint::MiddleCenter;
    t.background_fill_flags = 1;
    t.background_color = codec::types::Color::Index(42);
    t.background_scale = 1.75;
    t.background_transparency = 37;
    t.common.color = codec::types::Color::Index(5);
    t.common.extended_data = xdata(tag);
    t
}

/// Relations that exist in the CAD model: a registered XData application, an
/// extension dictionary with an XRecord, a GROUP whose member carries the
/// group as its reactor, two annotation scale contexts, and block ownership.
fn fixture() -> Fixture {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let scene = &mut app.tabs[1].scene;
    let mut appid = codec::tables::AppId::new(APP);
    appid.handle = scene.document.allocate_handle();
    scene.document.app_ids.add(appid).unwrap();

    let mut model = rich_mtext("model");
    model.is_annotative = true;
    let model = scene
        .document
        .add_entity(EntityType::MText(Box::new(model)))
        .unwrap();
    let record = scene.document.ensure_xrecord(model, APP);
    if let Some(codec::objects::ObjectType::XRecord(x)) = scene.document.objects.get_mut(&record) {
        use codec::objects::{XRecordEntry, XRecordValue};
        x.entries = vec![
            XRecordEntry::new(1, XRecordValue::String("kapcsolat".into())),
            XRecordEntry::new(40, XRecordValue::Double(0.125)),
        ];
    }
    let one = anno::ensure_scale_object(&mut scene.document, &codec::objects::Scale::scale_1_1());
    let two = anno::ensure_scale_object(&mut scene.document, &codec::objects::Scale::scale_1_2());
    assert!(anno::create_annotation_context(
        &mut scene.document,
        model,
        one
    ));
    assert!(anno::create_annotation_context(
        &mut scene.document,
        model,
        two
    ));
    let group = scene.create_group("DATA09".into(), vec![model]);
    scene
        .document
        .get_entity_mut(model)
        .unwrap()
        .common_mut()
        .reactors
        .push(group);

    // The block copy also carries manual dynamic columns of unequal height.
    let mut inner = rich_mtext("block");
    inner.column_data.column_type = 2;
    inner.column_data.column_count = 3;
    inner.column_data.width = 12.;
    inner.column_data.gutter = 1.5;
    inner.column_data.heights = vec![9., 13.5, 17.25];
    let inner = scene
        .document
        .add_entity(EntityType::MText(Box::new(inner)))
        .unwrap();
    let identity = codec::types::Transform::identity();
    scene
        .create_block_from_entities(&[inner], BLOCK, &identity, &identity)
        .unwrap();
    let block = block_mtext(&scene.document);
    assert_ne!(model, block);
    Fixture { app, model, block }
}

fn block_mtext(doc: &Document) -> Handle {
    let mut found = doc
        .entities_in_block(BLOCK)
        .filter(|e| matches!(e, EntityType::MText(_)))
        .map(|e| e.common().handle);
    let handle = found.next().expect("block keeps its MTEXT");
    assert!(found.next().is_none());
    handle
}

fn text(doc: &Document, handle: Handle) -> &MText {
    match doc.get_entity(handle) {
        Some(EntityType::MText(t)) => t,
        other => panic!("MTEXT {handle:?} must survive, got {other:?}"),
    }
}

/// Every MTEXT field and relation compared, by name. XData compares records,
/// not the raw DWG EED blobs a reader keeps for verbatim re-writing.
fn differences(a: &Document, b: &Document, handle: Handle) -> Vec<&'static str> {
    let (x, y) = (text(a, handle), text(b, handle));
    let mut out = Vec::new();
    let mut check = |name, same: bool| {
        if !same {
            out.push(name);
        }
    };
    check("value", x.value == y.value);
    check("insertion_point", x.insertion_point == y.insertion_point);
    check("height", x.height == y.height);
    check("rectangle_width", x.rectangle_width == y.rectangle_width);
    check("rectangle_height", x.rectangle_height == y.rectangle_height);
    check("rotation", x.rotation == y.rotation);
    check("style", x.style == y.style);
    check("attachment_point", x.attachment_point == y.attachment_point);
    check(
        "drawing_direction",
        x.drawing_direction == y.drawing_direction,
    );
    check(
        "line_spacing_factor",
        x.line_spacing_factor == y.line_spacing_factor,
    );
    check(
        "line_spacing_style",
        x.line_spacing_style == y.line_spacing_style,
    );
    check("normal", x.normal == y.normal);
    check(
        "background_fill_flags",
        x.background_fill_flags == y.background_fill_flags,
    );
    check("background_scale", x.background_scale == y.background_scale);
    check("background_color", x.background_color == y.background_color);
    check(
        "background_transparency",
        x.background_transparency == y.background_transparency,
    );
    check("is_annotative", x.is_annotative == y.is_annotative);
    check(
        "effective_annotative",
        anno::is_annotative(a, a.get_entity(handle).unwrap())
            == anno::is_annotative(b, b.get_entity(handle).unwrap()),
    );
    check("column_data", x.column_data == y.column_data);
    check("color", x.common.color == y.common.color);
    check("layer", x.common.layer == y.common.layer);
    check("handle", x.common.handle == y.common.handle);
    check(
        "xdata",
        x.common.extended_data.records() == y.common.extended_data.records(),
    );
    check(
        "owner",
        owner_name(a, x.common.owner_handle) == owner_name(b, y.common.owner_handle),
    );
    check(
        "reactors",
        reactor_groups(a, &x.common.reactors) == reactor_groups(b, &y.common.reactors),
    );
    check("xrecord", xrecord(a, handle) == xrecord(b, handle));
    check(
        "annotation_scales",
        scale_names(a, handle) == scale_names(b, handle),
    );
    check(
        "group",
        group_members(a, handle) == group_members(b, handle),
    );
    out
}

fn owner_name(doc: &Document, owner: Handle) -> Option<String> {
    doc.block_records
        .iter()
        .find(|br| br.handle == owner)
        .map(|br| br.name.to_ascii_uppercase())
}

/// A group's name is its ACAD_GROUP dictionary key; the DXF reader leaves the
/// object's own name field empty.
fn group_name(doc: &Document, group: Handle) -> Option<String> {
    let Some(codec::objects::ObjectType::Dictionary(d)) =
        doc.objects.get(&doc.header.acad_group_dict_handle)
    else {
        return None;
    };
    d.entries
        .iter()
        .find(|(_, h)| *h == group)
        .map(|(k, _)| k.clone())
}

fn reactor_groups(doc: &Document, reactors: &[Handle]) -> Vec<String> {
    reactors
        .iter()
        .filter(|h| {
            matches!(
                doc.objects.get(h),
                Some(codec::objects::ObjectType::Group(_))
            )
        })
        .filter_map(|h| group_name(doc, *h))
        .collect()
}

fn xrecord(doc: &Document, handle: Handle) -> Option<Vec<codec::objects::XRecordEntry>> {
    let dictionary = doc.extension_dictionary_handle(handle)?;
    let Some(codec::objects::ObjectType::Dictionary(d)) = doc.objects.get(&dictionary) else {
        return None;
    };
    let (_, record) = d.entries.iter().find(|(k, _)| k == APP)?;
    match doc.objects.get(record) {
        Some(codec::objects::ObjectType::XRecord(x)) => Some(x.entries.clone()),
        _ => None,
    }
}

fn scale_names(doc: &Document, handle: Handle) -> Vec<String> {
    let mut names: Vec<_> = anno::object_scale_memberships(doc, handle)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    names.sort();
    names
}

fn group_members(doc: &Document, handle: Handle) -> Vec<String> {
    doc.objects
        .values()
        .filter_map(|o| match o {
            codec::objects::ObjectType::Group(g) if g.entities.contains(&handle) => {
                group_name(doc, g.handle)
            }
            _ => None,
        })
        .collect()
}

fn open(app: &mut OpenCADStudio, handle: Handle) {
    let t = text(&app.tabs[app.active_tab].scene.document, handle).clone();
    let at = t.insertion_point;
    app.open_mtext_editor(
        DVec3::new(at.x, at.y, at.z),
        Some(handle),
        &t.value,
        t.height,
        None,
    );
}

fn sorted(mut fields: Vec<&'static str>) -> Vec<&'static str> {
    fields.sort_unstable();
    fields
}

fn commit_unchanged(handle: Handle) -> OpenCADStudio {
    let mut f = fixture();
    open(&mut f.app, handle);
    assert!(f.app.mtext_commit());
    assert!(!f.app.tabs[1].dirty, "an unchanged commit is a no-op");
    f.app
}

fn commit_property(handle: Handle) -> OpenCADStudio {
    let mut f = fixture();
    open(&mut f.app, handle);
    let _ = f.app.update(Message::MTextHeight("4.125".into()));
    let _ = f
        .app
        .update(Message::MTextColorChanged(codec::types::Color::Index(3)));
    assert!(f.app.mtext_commit());
    f.app
}

fn commit_text(handle: Handle) -> OpenCADStudio {
    let mut f = fixture();
    open(&mut f.app, handle);
    let ed = f.app.mtext_editor.as_mut().unwrap();
    ed.sel = Some((0, 1));
    ed.sel_anchor = 0;
    ed.caret = 1;
    f.app.raw_type("Ő", false);
    assert!(f.app.mtext_commit());
    f.app
}

#[test]
fn data09_relations_survive_noop_property_and_text_commits() {
    let original = fixture();
    let before = &original.app.tabs[1].scene.document;
    for (handle, other) in [
        (original.model, original.block),
        (original.block, original.model),
    ] {
        for (app, expected) in [
            (commit_unchanged(handle), vec![]),
            (commit_property(handle), vec!["color", "height"]),
            (commit_text(handle), vec!["value"]),
        ] {
            let after = &app.tabs[1].scene.document;
            assert_eq!(sorted(differences(before, after, handle)), expected);
            // The same entity is patched in place: no new handle or object,
            // and the sibling carrying the same kind of relations is untouched.
            assert_eq!(after.objects.len(), before.objects.len());
            assert_eq!(text(after, other), text(before, other));
            let (x, y) = (text(before, handle), text(after, handle));
            assert_eq!(y.common.owner_handle, x.common.owner_handle);
            assert_eq!(y.common.reactors, x.common.reactors);
            assert_eq!(y.common.xdictionary_handle, x.common.xdictionary_handle);
            assert_eq!(y.common.extended_data, x.common.extended_data);
        }
    }
    let block_owner = owner_name(before, text(before, original.block).common.owner_handle);
    assert_eq!(block_owner.as_deref(), Some(BLOCK));
    assert_eq!(
        group_members(before, original.model),
        vec!["DATA09".to_string()]
    );
    assert_eq!(scale_names(before, original.model).len(), 2);
    assert!(xrecord(before, original.model).is_some());
}
