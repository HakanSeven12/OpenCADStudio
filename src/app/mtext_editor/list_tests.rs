//! Bullets, numbering, tab and line spacing mode, through the editor's own
//! messages.
use super::*;
use crate::app::{Message, OpenCADStudio};

/// The bullet the menu writes: the Symbol font's `·` and the tab.
const BULLET: &str = "{\\fSymbol|b0|i0|c2|p18;\u{b7}\t}";

fn app(raw: &str) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::tab_document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    app.open_mtext_editor(DVec3::ZERO, None, raw, 2.5, None);
    app
}
fn ed(app: &OpenCADStudio) -> &MTextEditorState {
    app.mtext_editor.as_ref().unwrap()
}
fn select(app: &mut OpenCADStudio, a: usize, b: usize) {
    let ed = app.mtext_editor.as_mut().unwrap();
    ed.sel = Some((a, b));
    ed.sel_anchor = a;
    ed.caret = b;
}
fn raw(app: &OpenCADStudio) -> String {
    ed(app).content.text()
}
fn texts(app: &OpenCADStudio) -> Vec<String> {
    ed(app)
        .doc
        .to_plain_text()
        .split('\n')
        .map(str::to_owned)
        .collect()
}

#[test]
fn bullets_on_selected_paragraphs_and_none_restores_the_source_exactly() {
    let source = r"one\Ptwo\Pthree";
    let mut app = app(source);
    select(&mut app, 1, 6); // "one" .. "two"
    let _ = app.update(Message::MTextList(Some(ListKind::Bullet)));
    // The hanging indent is written once where the list starts; the
    // paragraph after the list sets it back.
    assert_eq!(
        raw(&app),
        format!("\\pxi-3,l3,t3;{BULLET}one\\P{BULLET}two\\P\\pi0,l0,tz;three")
    );
    assert_eq!(ed(&app).list_kind(), Some(ListKind::Bullet));
    assert_eq!(texts(&app), ["·\tone", "·\ttwo", "three"]);
    // None removes marker and settings: the original source, byte for byte.
    select(&mut app, 0, 8);
    let _ = app.update(Message::MTextList(None));
    assert_eq!(raw(&app), source);
    assert_eq!(ed(&app).list_kind(), None);
    app.mtext_undo();
    assert_eq!(ed(&app).list_kind(), Some(ListKind::Bullet));
    app.mtext_undo();
    assert_eq!(raw(&app), source);
}

#[test]
fn numbering_a_selected_range_starts_at_one() {
    let mut app = app(r"intro\Pa\Pb\Pend");
    select(&mut app, 6, 9); // "a" .. "b"
    let _ = app.update(Message::MTextList(Some(ListKind::Number)));
    assert_eq!(texts(&app), ["intro", "1.\ta", "2.\tb", "end"]);
    assert_eq!(ed(&app).list_kind(), Some(ListKind::Number));
    // Bullets over numbers changes the markers only.
    let _ = app.update(Message::MTextList(Some(ListKind::Bullet)));
    assert_eq!(texts(&app), ["intro", "·\ta", "·\tb", "end"]);
}

#[test]
fn enter_continues_the_list_and_renumbers_the_items_after_it() {
    let mut app = app(r"a\Pb\Pc");
    select(&mut app, 0, 5);
    let _ = app.update(Message::MTextList(Some(ListKind::Number)));
    assert_eq!(texts(&app), ["1.\ta", "2.\tb", "3.\tc"]);
    // Caret after "a": Enter opens item 2, the rest follow on.
    select(&mut app, 4, 4);
    app.mtext_type("\n");
    assert_eq!(texts(&app), ["1.\ta", "2.\t", "3.\tb", "4.\tc"]);
    assert_eq!(ed(&app).caret, 8, "after the new item's marker");
    app.mtext_type("x");
    assert_eq!(texts(&app), ["1.\ta", "2.\tx", "3.\tb", "4.\tc"]);
    // Splitting an item moves the rest of its text into the next one.
    let caret = ed(&app).caret - 1;
    select(&mut app, caret, caret);
    app.mtext_type("\n");
    assert_eq!(texts(&app), ["1.\ta", "2.\t", "3.\tx", "4.\tb", "5.\tc"]);
    // One undo step per Enter.
    app.mtext_undo();
    assert_eq!(texts(&app), ["1.\ta", "2.\tx", "3.\tb", "4.\tc"]);
}

#[test]
fn enter_on_an_empty_item_leaves_the_list() {
    let mut app = app(r"a\Pb");
    select(&mut app, 0, 3);
    let _ = app.update(Message::MTextList(Some(ListKind::Bullet)));
    let end = doc_to_cells(&ed(&app).doc).len();
    select(&mut app, end, end);
    app.mtext_type("\n");
    assert_eq!(texts(&app), ["·\ta", "·\tb", "·\t"]);
    app.mtext_type("\n");
    // The emptied item stays as a plain empty paragraph and the caret goes
    // on in a new one below it.
    assert_eq!(texts(&app), ["·\ta", "·\tb", "", ""]);
    assert_eq!(ed(&app).list_kind(), None);
    let source = raw(&app);
    assert!(source.ends_with(r"\P\pi0,l0,tz;\P"), "{source}");
    app.mtext_type("z");
    assert_eq!(texts(&app), ["·\ta", "·\tb", "", "z"]);
}

#[test]
fn marker_text_without_a_hanging_indent_is_not_a_list() {
    let mut app = app("•\tx");
    assert_eq!(ed(&app).list_kind(), None);
    select(&mut app, 3, 3);
    app.mtext_type("\n");
    assert_eq!(texts(&app), ["•\tx", ""], "an ordinary line break");
}

#[test]
fn an_imported_list_is_continued_and_turned_off_whole() {
    let source = "\\pxi-2,l2,t2;1.\ta";
    let mut app = app(source);
    assert_eq!(ed(&app).list_kind(), Some(ListKind::Number));
    let end = ed(&app).visible_character_count();
    select(&mut app, end, end);
    app.mtext_type("\n");
    assert_eq!(texts(&app), ["1.\ta", "2.\t"]);
    // Off takes the marker and the hanging indent away.
    let mut app = self::app(source);
    let _ = app.update(Message::MTextList(None));
    assert_eq!(raw(&app), "a");
    // A typed list's first lines stay one text height in, as the
    // reference's menu Off leaves them.
    let mut app = self::app("\\pxi-3,l4,t4;1.\tAlpha\\P2.\tBeta");
    let end = ed(&app).visible_character_count();
    select(&mut app, 0, end);
    let _ = app.update(Message::MTextList(None));
    assert_eq!(raw(&app), "\\pxi1;Alpha\\PBeta");
}

#[test]
fn a_list_item_body_starts_at_the_hanging_indent() {
    let mut app = app("ab");
    let _ = app.update(Message::MTextList(Some(ListKind::Bullet)));
    // Marker at the margin, text at 3 text heights (2.5 × 3).
    let boxes = &ed(&app).glyph_boxes;
    let body = boxes.iter().find(|b| b.vis == 2).unwrap();
    assert!((body.xmin - 7.5).abs() < 0.05, "{}", body.xmin);
    let marker = boxes.iter().find(|b| b.vis == 0).unwrap();
    assert!(marker.xmin < 0.5, "{}", marker.xmin);
}

#[test]
fn a_tab_goes_in_at_the_caret_and_keeps_the_paragraph_tab_stops() {
    let source = r"\pxt20;ab";
    let mut app = app(source);
    select(&mut app, 1, 1);
    let _ = app.update(Message::MTextTab);
    assert_eq!(raw(&app), "\\pxt20;a\tb");
    assert_eq!(ed(&app).caret, 2);
    // One caret slot per character, the tab included.
    assert_eq!(doc_to_cells(&ed(&app).doc).len(), 3);
    app.mtext_undo();
    assert_eq!(raw(&app), source);
}

#[test]
fn line_spacing_mode_and_a_typed_factor_are_saved_with_the_text() {
    use document::entities::LineSpacingStyle as S;
    let mut app = app(r"a\Pb");
    let _ = app.update(Message::MTextLineSpacingMode(true));
    let _ = app.update(Message::MTextLineSpacingInput("1,75".into()));
    let mt = ed(&app).build_mtext();
    assert_eq!(mt.line_spacing_style, S::Exactly);
    assert_eq!(mt.line_spacing_factor, 1.75);
    // Outside 0.25..=4 the factor is kept as typed and changes nothing.
    let _ = app.update(Message::MTextLineSpacingInput("9".into()));
    assert_eq!(ed(&app).build_mtext().line_spacing_factor, 1.75);
    assert_eq!(ed(&app).line_spacing_draft.as_deref(), Some("9"));
    // One step each, the mode included.
    app.mtext_undo();
    app.mtext_undo();
    assert_eq!(ed(&app).build_mtext().line_spacing_style, S::AtLeast);
}

#[test]
fn a_typed_marker_and_tab_make_an_item_that_two_enters_leave() {
    let mut app = app("");
    for s in ["1", ".", "\t", "a", "\n", "\n", "b"] {
        app.mtext_type(s);
    }
    assert_eq!(raw(&app), r"\pxi-3,l4,t4;1.	a\P\pi0,l0,tz;\Pb");
}
