//! Superscript, subscript, Stack and fractions, driven through the editor's
//! own messages and checked on the source and the rendered cells.
use super::*;
use crate::app::{Message, OpenCADStudio};

fn app(raw: &str) -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
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
fn stacks(app: &OpenCADStudio) -> Vec<StackingData> {
    doc_to_cells(&ed(app).doc)
        .into_iter()
        .filter_map(|c| match c {
            Cell::Stack {
                data, head: true, ..
            } => Some(data),
            _ => None,
        })
        .collect()
}

#[test]
fn every_selection_of_abc12_raises_and_lowers_and_restores_exactly() {
    let text = "abc12";
    for a in 0..text.len() {
        for b in a + 1..=text.len() {
            let (head, part, tail) = (&text[..a], &text[a..b], &text[b..]);
            for (script, code) in [
                (Script::Super, format!(r"\S{part}^;")),
                (Script::Sub, format!(r"\S^{part};")),
            ] {
                let mut app = app(text);
                select(&mut app, a, b);
                let _ = app.update(Message::MTextScript(script == Script::Super));
                assert_eq!(raw(&app), format!("{head}{code}{tail}"), "{a}..{b}");
                assert!(ed(&app).script_active(script));
                let other = if script == Script::Super {
                    Script::Sub
                } else {
                    Script::Super
                };
                assert!(!ed(&app).script_active(other), "the two exclude each other");
                // The same entry again restores the original text exactly.
                let _ = app.update(Message::MTextScript(script == Script::Super));
                assert_eq!(raw(&app), text);
                assert_eq!(ed(&app).sel, Some((a, b)));
            }
        }
    }
}

#[test]
fn switching_index_kind_swaps_it_and_undo_restores_each_step() {
    let mut app = app("x2");
    select(&mut app, 1, 2);
    let _ = app.update(Message::MTextScript(true));
    assert_eq!(raw(&app), r"x\S2^;");
    let _ = app.update(Message::MTextScript(false));
    assert_eq!(raw(&app), r"x\S^2;");
    assert!(ed(&app).script_active(Script::Sub));
    let selected = ed(&app).sel.unwrap();
    assert_eq!(stacks(&app).len(), 1);
    assert_eq!(selected.1 - selected.0, 2, "the stack's own visible slots");
    app.mtext_undo();
    assert_eq!(raw(&app), r"x\S2^;");
    app.mtext_undo();
    assert_eq!(raw(&app), "x2");
    app.mtext_redo();
    assert_eq!(raw(&app), r"x\S2^;");
}

#[test]
fn an_index_keeps_the_formatting_of_its_characters_and_the_text_around_it() {
    let mut app = app(r"{\C1;\Labc}12");
    select(&mut app, 0, 2);
    let _ = app.update(Message::MTextScript(true));
    assert_eq!(raw(&app), r"{\C1;\L\Sab^;c}12");
    let cells = doc_to_cells(&ed(&app).doc);
    let Cell::Stack { props, .. } = &cells[0] else {
        panic!()
    };
    assert!(props.stroke.underline());
    assert_eq!(
        props.color,
        Some(codec::entities::mtext_format::MTextColor::Index(1))
    );
}

#[test]
fn unsupported_index_selections_leave_source_and_history_untouched() {
    for (source, a, b) in [
        (r"a{\Lb}c", 0, 2),  // two formats
        (r"a\Pb", 0, 3),     // line break
        (r"a\S1/2;b", 0, 5), // contains a fraction
        ("a/b", 0, 3),       // separator character
        ("50%", 0, 3),       // percent
    ] {
        let mut app = app(source);
        select(&mut app, a, b);
        let undo = ed(&app).undo.len();
        let _ = app.update(Message::MTextScript(true));
        assert_eq!(raw(&app), source);
        assert_eq!(ed(&app).undo.len(), undo);
    }
    // A subscript cannot start with a space: readers skip one after `^`.
    let mut app = self::app("a b");
    select(&mut app, 1, 3);
    let _ = app.update(Message::MTextScript(false));
    assert_eq!(raw(&app), "a b");
    // Without a selection there is nothing to raise.
    let mut app = self::app("ab");
    let _ = app.update(Message::MTextScript(true));
    assert_eq!(raw(&app), "ab");
}

#[test]
fn fractions_are_created_in_all_three_forms_and_empty_parts_are_refused() {
    for (kind, code) in [
        (StackingType::Horizontal, r"\S1/2;"),
        (StackingType::Diagonal, r"\S1#2;"),
        (StackingType::Limit, r"\S1^2;"),
    ] {
        let mut app = app("ab");
        select(&mut app, 1, 1);
        let _ = app.update(Message::MTextFractionOpen);
        assert_eq!(ed(&app).menu, Some(EditorMenu::Fraction));
        let _ = app.update(Message::MTextFractionPart(
            FractionPart::Numerator,
            "1".into(),
        ));
        // An empty denominator is refused and the popup stays open.
        let _ = app.update(Message::MTextFractionInsert);
        assert_eq!(raw(&app), "ab");
        assert_eq!(ed(&app).menu, Some(EditorMenu::Fraction));
        let _ = app.update(Message::MTextFractionPart(
            FractionPart::Denominator,
            "2".into(),
        ));
        let _ = app.update(Message::MTextFractionKind(kind));
        let _ = app.update(Message::MTextFractionInsert);
        assert_eq!(raw(&app), format!("a{code}b"));
        assert!(ed(&app).menu.is_none());
        assert_eq!(
            stacks(&app),
            vec![StackingData {
                numerator: "1".into(),
                denominator: "2".into(),
                stacking_type: kind,
            }]
        );
        app.mtext_undo();
        assert_eq!(raw(&app), "ab");
    }
    // Whitespace alone is no numerator either.
    let mut app = app("");
    let _ = app.update(Message::MTextFractionOpen);
    let _ = app.update(Message::MTextFractionPart(
        FractionPart::Numerator,
        " ".into(),
    ));
    let _ = app.update(Message::MTextFractionPart(
        FractionPart::Denominator,
        "4".into(),
    ));
    let _ = app.update(Message::MTextFractionInsert);
    assert_eq!(raw(&app), "");
}

#[test]
fn a_selected_fraction_is_loaded_and_replaced_whole() {
    let mut app = app(r"x\S1/2;y");
    select(&mut app, 1, 4);
    let _ = app.update(Message::MTextFractionOpen);
    let draft = ed(&app).fraction.clone().unwrap();
    assert_eq!(
        (
            draft.numerator.as_str(),
            draft.denominator.as_str(),
            draft.kind
        ),
        ("1", "2", StackingType::Horizontal)
    );
    let _ = app.update(Message::MTextFractionPart(
        FractionPart::Numerator,
        "3".into(),
    ));
    let _ = app.update(Message::MTextFractionPart(
        FractionPart::Denominator,
        "16".into(),
    ));
    let _ = app.update(Message::MTextFractionKind(StackingType::Diagonal));
    let _ = app.update(Message::MTextFractionInsert);
    assert_eq!(raw(&app), r"x\S3#16;y");
    assert_eq!(ed(&app).sel, Some((1, 5)));
    app.mtext_undo();
    assert_eq!(raw(&app), r"x\S1/2;y");
}

#[test]
fn a_plain_a_slash_b_selection_prefills_and_nothing_else_is_guessed() {
    let mut app = app("3/4 in");
    select(&mut app, 0, 3);
    let draft = ed(&app).fraction_draft();
    assert_eq!(
        (draft.numerator.as_str(), draft.denominator.as_str()),
        ("3", "4")
    );
    let _ = app.update(Message::MTextFractionOpen);
    let _ = app.update(Message::MTextFractionInsert);
    assert_eq!(raw(&app), r"\S3/4; in");
    for (raw, b) in [("1/2/3", 5), ("abc", 3)] {
        let mut other = self::app(raw);
        select(&mut other, 0, b);
        let draft = ed(&other).fraction_draft();
        assert!(
            draft.numerator.is_empty() && draft.denominator.is_empty(),
            "{raw}"
        );
    }
}

#[test]
fn the_fraction_row_toggles_its_popup_and_escape_steps_back() {
    let mut app = app("ab");
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::More)));
    let _ = app.update(Message::MTextFractionOpen);
    assert_eq!(ed(&app).menu, Some(EditorMenu::Fraction));
    let _ = app.update(Message::MTextFractionOpen);
    assert_eq!(ed(&app).menu, Some(EditorMenu::More));
    assert!(ed(&app).fraction.is_none());
}

#[test]
fn an_index_preview_has_one_box_per_caret_slot_and_the_drawn_forms() {
    // The renderer draws exactly these forms raised / lowered and smaller:
    // text_support::upstream_compat_one_sided_mtext_stacks_use_the_correct_baseline.
    for (script, source, slots) in [(Script::Super, r"x\S2^;", 2), (Script::Sub, r"x\S^2;", 3)] {
        let mut app = app("x2");
        select(&mut app, 1, 2);
        let _ = app.update(Message::MTextScript(script == Script::Super));
        assert_eq!(raw(&app), source);
        let cells = doc_to_cells(&ed(&app).doc).len();
        assert_eq!(cells, slots);
        assert_eq!(ed(&app).glyph_boxes.len(), slots, "{script:?}");
        // The caret steps over the index as one atom.
        app.mtext_caret_move(-1, false);
        assert_eq!(ed(&app).caret, 1);
    }
}

#[test]
fn stack_splits_the_selection_at_its_first_separator_as_before() {
    for (text, raw_after) in [
        ("1/2", r"\S1/2;"),
        ("3#4", r"\S3#4;"),
        ("2^", r"\S2^;"),
        ("^2", r"\S^2;"),
    ] {
        let mut app = app(&format!("x{text}"));
        select(&mut app, 1, 1 + text.chars().count());
        let _ = app.update(Message::MTextStack);
        assert_eq!(raw(&app), format!("x{raw_after}"), "{text}");
        app.mtext_undo();
        assert_eq!(raw(&app), format!("x{text}"));
    }
    // Nothing to split at, or a fraction with an empty half: refused.
    for text in ["ab", "1/", "^"] {
        let mut app = app(text);
        select(&mut app, 0, text.chars().count());
        let _ = app.update(Message::MTextStack);
        assert_eq!(raw(&app), text);
        assert!(ed(&app).undo.is_empty());
    }
}
