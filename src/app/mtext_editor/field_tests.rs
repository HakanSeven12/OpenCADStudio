//! Text around native FIELDs: the fields are atomic, the text around them
//! is edited and formatted, and the save matches what the codec would write.
use super::*;
use crate::app::{Message, OpenCADStudio};
use codec::fields::NewField;
use codec::objects::ObjectType;
use codec::DxfVersion;

/// An MTEXT whose value is `template` with each `%<\_FldIdx n>%` shown as
/// `values[n]`, opened in the editor.
fn host(template: &str, values: &[&str]) -> (OpenCADStudio, Handle) {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    let doc = &mut app.tabs[1].scene.document;
    let h = doc
        .add_entity(EntityType::MText(Box::new(MText::with_value(
            "",
            codec::types::Vector3::new(0., 0., 0.),
        ))))
        .unwrap();
    doc.set_text_field(h, template, children(values)).unwrap();
    let value = value(&app, h);
    app.open_mtext_editor(DVec3::ZERO, Some(h), &value, 2.5, None);
    (app, h)
}
fn children(values: &[&str]) -> Vec<NewField> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| NewField::new(format!(r"\AcVar CustomValue{i}"), *v))
        .collect()
}
fn value(app: &OpenCADStudio, h: Handle) -> String {
    match app.tabs[1].scene.document.get_entity(h) {
        Some(EntityType::MText(t)) => t.value.clone(),
        _ => panic!(),
    }
}
fn raw(app: &OpenCADStudio) -> String {
    ed(app).content.text()
}
fn ed(app: &OpenCADStudio) -> &MTextEditorState {
    app.mtext_editor.as_ref().unwrap()
}
fn caret(app: &mut OpenCADStudio, at: usize) {
    let ed = app.mtext_editor.as_mut().unwrap();
    ed.sel = Some((at, at));
    ed.sel_anchor = at;
    ed.caret = at;
}
fn select(app: &mut OpenCADStudio, a: usize, b: usize) {
    let ed = app.mtext_editor.as_mut().unwrap();
    ed.sel = Some((a, b));
    ed.sel_anchor = a;
    ed.caret = b;
}
/// Every FIELD-related object except the host's container.
fn other_field_objects(app: &OpenCADStudio, container: Handle) -> Vec<String> {
    let doc = &app.tabs[1].scene.document;
    let mut out: Vec<String> = doc
        .objects
        .iter()
        .filter(|(h, o)| {
            **h != container && matches!(o, ObjectType::Field(_) | ObjectType::FieldList(_))
        })
        .map(|(h, o)| format!("{h:?}{o:?}"))
        .collect();
    out.sort();
    out
}
fn container(app: &OpenCADStudio, h: Handle) -> (Handle, String, String) {
    let doc = &app.tabs[1].scene.document;
    let (handle, field) = doc
        .objects
        .iter()
        .find_map(|(handle, o)| match o {
            ObjectType::Field(f) if f.evaluator_id == "_text" && f.owner != Handle::NULL => {
                let _ = h;
                Some((*handle, f.clone()))
            }
            _ => None,
        })
        .unwrap();
    let checksum = field
        .child_values
        .iter()
        .find(|c| c.key == "ACFD_FIELDTEXT_CHECKSUM")
        .map(|c| format!("{:?}", c.value))
        .unwrap();
    (handle, field.code, checksum)
}

#[test]
fn text_around_a_field_is_edited_and_saved_as_the_codec_would() {
    let (mut app, h) = host(r"a %<\_FldIdx 0>% b", &["12"]);
    assert_eq!(raw(&app), "a 12 b");
    let (handle, _, _) = container(&app, h);
    let others = other_field_objects(&app, handle);
    // Before, after, and right next to the field.
    caret(&mut app, 0);
    app.mtext_type("x");
    caret(&mut app, 5);
    app.mtext_type("y");
    let end = doc_to_cells(&ed(&app).doc).len();
    caret(&mut app, end);
    app.mtext_type("!");
    assert_eq!(raw(&app), "xa 12y b!");
    assert!(app.mtext_commit());
    // A codec-only control: the same template set from scratch.
    let mut control = app.tabs[1].scene.document.clone();
    control
        .set_text_field(h, r"xa %<\_FldIdx 0>%y b!", children(&["12"]))
        .unwrap();
    let Some(EntityType::MText(t)) = control.get_entity(h) else {
        panic!()
    };
    assert_eq!(value(&app, h), t.value);
    let (_, code, checksum) = container(&app, h);
    assert_eq!(code, r"xa %<\_FldIdx 0>%y b!");
    let control_checksum = control
        .objects
        .values()
        .find_map(|o| match o {
            ObjectType::Field(f) if f.evaluator_id == "_text" => f
                .child_values
                .iter()
                .find(|c| c.key == "ACFD_FIELDTEXT_CHECKSUM")
                .map(|c| format!("{:?}", c.value)),
            _ => None,
        })
        .unwrap();
    assert_eq!(checksum, control_checksum);
    // The field objects themselves are untouched.
    assert_eq!(other_field_objects(&app, handle), others);
}

#[test]
fn a_fields_own_text_cannot_be_typed_into_or_deleted() {
    let (mut app, _) = host(r"a %<\_FldIdx 0>% b", &["12"]);
    let before = raw(&app);
    caret(&mut app, 3);
    app.mtext_type("x");
    assert_eq!(raw(&app), before);
    caret(&mut app, 4);
    app.mtext_backspace();
    assert_eq!(raw(&app), before, "backspace into the field");
    select(&mut app, 1, 5);
    app.mtext_backspace();
    assert_eq!(raw(&app), before, "a selection over the field");
    // Right next to it both sides work.
    caret(&mut app, 2);
    app.mtext_backspace();
    assert_eq!(raw(&app), "a12 b");
}

#[test]
fn equal_characters_beside_a_field_follow_the_caret() {
    // "1" + field "1" + "1": typing "1" in front of the field must stay a
    // literal in front of it, not move the field.
    let (mut app, h) = host(r"1%<\_FldIdx 0>%1", &["1"]);
    caret(&mut app, 1);
    app.mtext_type("1");
    assert_eq!(raw(&app), "1111");
    let ed_spans = ed(&app).field_host.as_ref().unwrap().spans.clone();
    assert_eq!(ed_spans, vec![2..3]);
    assert!(app.mtext_commit());
    assert_eq!(container(&app, h).1, r"11%<\_FldIdx 0>%1");
}

#[test]
fn two_fields_and_undo_keep_the_spans_in_step() {
    let (mut app, h) = host(r"%<\_FldIdx 0>%-%<\_FldIdx 1>%", &["A", "B"]);
    assert_eq!(raw(&app), "A-B");
    caret(&mut app, 2);
    app.mtext_type("mid");
    assert_eq!(raw(&app), "A-midB");
    app.mtext_undo();
    assert_eq!(raw(&app), "A-B");
    caret(&mut app, 1);
    app.mtext_type(" ");
    assert!(app.mtext_commit());
    assert_eq!(container(&app, h).1, r"%<\_FldIdx 0>% -%<\_FldIdx 1>%");
}

#[test]
fn formatting_beside_a_field_is_saved_and_over_it_is_refused() {
    let (mut app, h) = host(r"a %<\_FldIdx 0>% b", &["12"]);
    // Over the field's own text: refused, nothing changes.
    select(&mut app, 1, 5);
    app.mtext_apply_fmt(MTextFmt::Underline);
    assert_eq!(raw(&app), "a 12 b");
    // Beside it, and the object font in front of everything: kept, and
    // the template follows with the same codes.
    select(&mut app, 0, 1);
    app.mtext_apply_fmt(MTextFmt::Underline);
    caret(&mut app, 0);
    let _ = app.update(Message::MTextFont("Simplex".into()));
    assert_eq!(raw(&app), r"\fSimplex;{\La} 12 b");
    assert!(app.mtext_commit());
    assert_eq!(container(&app, h).1, r"\fSimplex;{\La} %<\_FldIdx 0>% b");
    assert_eq!(
        codec::fields::field_spans(&app.tabs[1].scene.document, h, &value(&app, h)),
        Some(vec![16..18]),
        "the field is found again in the formatted text"
    );
}

#[test]
fn an_edited_field_host_reopens_editable_after_dwg_and_dxf() {
    let (mut app, h) = host(r"a %<\_FldIdx 0>% b", &["12"]);
    caret(&mut app, 0);
    app.mtext_type("x");
    assert!(app.mtext_commit());
    for (ext, version) in [("dwg", DxfVersion::AC1032), ("dxf", DxfVersion::AC1032)] {
        let bytes = crate::io::save_to_bytes(&app.tabs[1].scene.document, ext, version).unwrap();
        let doc = crate::io::load_bytes(&format!("field.{ext}"), bytes).unwrap();
        let Some(EntityType::MText(t)) = doc.get_entity(h) else {
            panic!()
        };
        assert_eq!(t.value, "xa 12 b", "{ext}");
        assert_eq!(
            codec::fields::field_spans(&doc, h, &t.value),
            Some(vec![3..5]),
            "{ext}: the field is found again"
        );
        let mut reopened = OpenCADStudio::new_for_test();
        reopened
            .tabs
            .push(crate::app::document::DocumentTab::new_drawing(1));
        reopened.active_tab = 1;
        reopened.tabs[1].scene.document = doc.clone();
        reopened.open_mtext_editor(DVec3::ZERO, Some(h), &t.value, 2.5, None);
        caret(&mut reopened, 0);
        reopened.mtext_type("y");
        assert_eq!(raw(&reopened), "yxa 12 b", "{ext}");
    }
}
