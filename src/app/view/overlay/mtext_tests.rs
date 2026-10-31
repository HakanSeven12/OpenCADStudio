use super::*;
use crate::app::mtext_editor::{EditorMenu, MTextEditorState, MTextFmt, ParaNumber};
use iced_test::simulator::{self, Simulator};

macro_rules! label {
    ($id:literal) => {
        crate::tr!("mtext-toolbar", $id)
    };
}

fn editor() -> MTextEditorState {
    MTextEditorState::new(glam::DVec3::ZERO, "Árvíz", 2.5, None)
}

fn overlay_sim(ed: &MTextEditorState, size: Size) -> Simulator<'_, Message> {
    Simulator::with_size(
        Default::default(),
        size,
        mtext_editor_overlay(
            ed,
            vec!["Standard".into()],
            Vector::ZERO,
            Vector::ZERO,
            None,
        ),
    )
}

fn bounds_of(ui: &mut Simulator<'_, Message>, label: impl Into<String>) -> Rectangle {
    let label = label.into();
    ui.find(label.clone())
        .unwrap_or_else(|_| panic!("{label} not found"))
        .visible_bounds()
        .unwrap_or_else(|| panic!("{label} not visible"))
}

/// The action a menu row runs: what its pick message carries.
fn picked(messages: impl Iterator<Item = Message>) -> Vec<Message> {
    messages
        .map(|m| match m {
            Message::MTextMenuPick(inner) => *inner,
            other => other,
        })
        .collect()
}

const MENUS: [(EditorMenu, &str); 8] = [
    (EditorMenu::Position, "position"),
    (EditorMenu::Background, "background"),
    (EditorMenu::More, "more"),
    (EditorMenu::Paragraph, "paragraph"),
    (EditorMenu::Columns, "columns"),
    (EditorMenu::Justify, "justification"),
    (EditorMenu::Symbols, "symbols"),
    (EditorMenu::Find, "find"),
];

fn menu_title(key: &str) -> String {
    match key {
        "more" => label!("more"),
        "paragraph" => label!("paragraph"),
        "columns" => label!("columns"),
        "justification" => label!("justification"),
        "symbols" => label!("symbols"),
        "position" => label!("position"),
        "background" => label!("background"),
        _ => label!("find"),
    }
}

#[test]
fn every_menu_opens_from_its_trigger_without_touching_the_text() {
    for (menu, key) in MENUS {
        let mut ed = editor();
        ed.sel = Some((1, 3));
        ed.caret = 3;
        let mut ui = overlay_sim(&ed, Size::new(1280., 720.));
        ui.click(menu_title(key)).unwrap();
        assert!(
            ui.into_messages()
                .any(|m| matches!(m, Message::MTextMenu(Some(found)) if found == menu)),
            "{menu:?}"
        );
        assert_eq!(ed.content.text(), "Árvíz");
        assert_eq!((ed.sel, ed.caret), (Some((1, 3)), 3));
    }
}

/// Every command of the old third and fourth rows and of the second row's
/// buttons is still one click away, now in a menu.
#[test]
fn every_former_toolbar_command_is_in_a_menu() {
    let rows: Vec<(EditorMenu, String, fn(&Message) -> bool)> = vec![
        (EditorMenu::More, label!("superscript"), |m| {
            matches!(m, Message::MTextScript(true))
        }),
        (EditorMenu::More, label!("subscript"), |m| {
            matches!(m, Message::MTextScript(false))
        }),
        (EditorMenu::More, label!("stack"), |m| {
            matches!(m, Message::MTextStack)
        }),
        (EditorMenu::More, label!("uppercase"), |m| {
            matches!(m, Message::MTextFmt(MTextFmt::Uppercase))
        }),
        (EditorMenu::More, label!("lowercase"), |m| {
            matches!(m, Message::MTextFmt(MTextFmt::Lowercase))
        }),
        (EditorMenu::More, label!("clear"), |m| {
            matches!(m, Message::MTextClearFormatting)
        }),
        (
            EditorMenu::Paragraph,
            format!("{} 1.5", label!("line-spacing")),
            |m| matches!(m, Message::MTextLineSpacing(f) if *f == 1.5),
        ),
        (
            EditorMenu::Columns,
            label!("static"),
            |m| matches!(m, Message::MTextColumnMode(s) if s == "Static"),
        ),
        (
            EditorMenu::Columns,
            label!("dynamic-manual"),
            |m| matches!(m, Message::MTextColumnMode(s) if s == "Dynamic manual"),
        ),
        (EditorMenu::Columns, label!("reverse"), |m| {
            matches!(m, Message::MTextColumnFlowReversed(true))
        }),
        (EditorMenu::Justify, t!("Middle Center").into_owned(), |m| {
            matches!(
                m,
                Message::MTextJustify(codec::entities::mtext::AttachmentPoint::MiddleCenter)
            )
        }),
        (
            EditorMenu::Symbols,
            label!("degree"),
            |m| matches!(m, Message::MTextInsert(s) if s == "°"),
        ),
        (
            EditorMenu::Symbols,
            label!("diameter"),
            |m| matches!(m, Message::MTextInsert(s) if s == "⌀"),
        ),
        (EditorMenu::Find, label!("find-next"), |m| {
            matches!(m, Message::MTextFindNext)
        }),
        (EditorMenu::Find, label!("replace"), |m| {
            matches!(m, Message::MTextReplaceNext)
        }),
        (EditorMenu::Find, label!("replace-all"), |m| {
            matches!(m, Message::MTextReplaceAll)
        }),
    ];
    for (menu, row, expected) in rows {
        let mut ed = editor();
        ed.menu = Some(menu);
        let mut ui = overlay_sim(&ed, Size::new(1280., 720.));
        ui.click(row.clone())
            .unwrap_or_else(|_| panic!("{row} not in {menu:?}"));
        let messages = picked(ui.into_messages());
        assert!(messages.iter().any(expected), "{row}: {messages:?}");
    }
    // Undo and redo stay buttons of the second row.
    for (glyph, expected) in [("↶", Message::MTextUndo), ("↷", Message::MTextRedo)] {
        let ed = editor();
        let mut ui = overlay_sim(&ed, Size::new(1280., 720.));
        ui.click(glyph).unwrap();
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| std::mem::discriminant(m) == std::mem::discriminant(&expected)),
            "{glyph}: {messages:?}"
        );
    }
}

/// The fields of the old rows take typing inside their menu.
#[test]
fn menu_fields_take_typing_and_keep_the_menu_open() {
    let cases: Vec<(EditorMenu, String, fn(&Message) -> bool)> = vec![
        (
            EditorMenu::More,
            label!("oblique"),
            |m| matches!(m, Message::MTextOblique(s) if s.ends_with('7')),
        ),
        (
            EditorMenu::More,
            label!("tracking"),
            |m| matches!(m, Message::MTextCharSpace(s) if s.ends_with('7')),
        ),
        (
            EditorMenu::Paragraph,
            label!("first-indent"),
            |m| matches!(m, Message::MTextParagraphNumber(ParaNumber::FirstIndent, s) if s.ends_with('7')),
        ),
        (
            EditorMenu::Paragraph,
            label!("space-after"),
            |m| matches!(m, Message::MTextParagraphNumber(ParaNumber::SpaceAfter, s) if s.ends_with('7')),
        ),
        (
            EditorMenu::Columns,
            label!("count"),
            |m| matches!(m, Message::MTextColumnCount(s) if s.ends_with('7')),
        ),
        (
            EditorMenu::Columns,
            label!("gutter"),
            |m| matches!(m, Message::MTextColumnGutter(s) if s.ends_with('7')),
        ),
        (
            EditorMenu::Find,
            label!("find-what"),
            |m| matches!(m, Message::MTextFindText(s) if s.ends_with('7')),
        ),
    ];
    for (menu, name, expected) in cases {
        let mut ed = editor();
        ed.menu = Some(menu);
        let mut ui = overlay_sim(&ed, Size::new(1280., 720.));
        // The field sits to the right of its label, on the same row.
        let row = bounds_of(&mut ui, name.clone());
        ui.point_at(iced::Point::new(row.x + row.width + 40., row.center_y()));
        ui.simulate(simulator::click());
        ui.typewrite("7");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages.iter().any(expected), "{name}: {messages:?}");
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, Message::MTextMenu(None) | Message::MTextMenuPick(_))),
            "{name}: {messages:?}"
        );
    }
}

#[test]
fn every_popup_opens_under_its_own_trigger_inside_the_window() {
    for size in [Size::new(1280., 720.), Size::new(1100., 600.)] {
        for (menu, key) in MENUS {
            let mut ed = editor();
            ed.menu = Some(menu);
            let mut ui = overlay_sim(&ed, size);
            let trigger = bounds_of(&mut ui, menu_title(key));
            let first = match menu {
                EditorMenu::More => label!("superscript"),
                EditorMenu::Paragraph => label!("bullets"),
                EditorMenu::Columns => label!("no-columns"),
                EditorMenu::Justify => t!("Top Left").into_owned(),
                EditorMenu::Symbols => label!("diameter"),
                EditorMenu::Find => label!("find-what"),
                EditorMenu::Fraction => unreachable!("opened from More"),
                EditorMenu::Background => label!("use-mask"),
                EditorMenu::Position => label!("direction-style"),
            };
            let item = bounds_of(&mut ui, first);
            assert!(item.y >= trigger.y + trigger.height, "{menu:?} {size:?}");
            assert!(
                item.y < trigger.y + trigger.height + 80.,
                "{menu:?} {size:?}"
            );
            assert!(item.x >= 0. && item.x + item.width <= size.width);
            assert!(item.y + item.height <= size.height, "{menu:?} {size:?}");
        }
    }
}

#[test]
fn a_press_outside_closes_the_popup_once_and_is_consumed() {
    let mut ed = editor();
    ed.menu = Some(EditorMenu::Symbols);
    let mut ui = overlay_sim(&ed, Size::new(1280., 720.));
    let title = bounds_of(&mut ui, t!("Text Editor").into_owned());
    ui.point_at(iced::Point::new(title.x + 5., 700.));
    ui.simulate(simulator::click());
    let messages: Vec<_> = ui.into_messages().collect();
    assert_eq!(
        messages
            .iter()
            .filter(|m| matches!(m, Message::MTextMenu(None)))
            .count(),
        1,
        "{messages:?}"
    );
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::MTextSelStart(_))),
        "the caret must not move under a closing click: {messages:?}"
    );
}

#[test]
fn the_editor_keeps_its_minimum_size_inside_the_window() {
    let content = || {
        container(Space::new().width(10).height(10))
            .id(iced::widget::Id::new("probe"))
            .width(Fill)
            .height(Fill)
    };
    for (viewport, extra, expected) in [
        // Shrunk by the resize handle: held at the minimum.
        (
            Size::new(1280., 720.),
            Vector::new(-500., -500.),
            MTEXT_EDITOR_MIN_SIZE,
        ),
        // A window smaller than the minimum still caps it.
        (Size::new(600., 300.), Vector::ZERO, Size::new(600., 300.)),
    ] {
        let mut ui = Simulator::with_size(
            Default::default(),
            viewport,
            crate::ui::modal::intrinsic_with_min(
                Space::new().width(10).height(10),
                content(),
                Size::INFINITE,
                extra,
                MTEXT_EDITOR_MIN_SIZE,
            ),
        );
        let probe = ui
            .find(iced::widget::Id::new("probe"))
            .unwrap()
            .visible_bounds()
            .unwrap();
        assert_eq!(probe.size(), expected, "{viewport:?} {extra:?}");
    }
}

#[test]
fn escape_closes_an_open_menu_before_the_editor() {
    let mut app = crate::app::OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    app.open_mtext_editor(glam::DVec3::ZERO, None, "ab", 2.5, None);
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::Columns)));
    assert_eq!(
        app.mtext_editor.as_ref().unwrap().menu,
        Some(EditorMenu::Columns)
    );
    let _ = app.on_command_escape();
    let ed = app.mtext_editor.as_ref().expect("the editor stays open");
    assert_eq!(ed.menu, None);
    assert_eq!(ed.content.text(), "ab");
    let _ = app.on_command_escape();
    assert!(app.mtext_editor.is_none());
}

#[test]
fn a_menu_action_closes_the_menu_and_acts() {
    let mut app = crate::app::OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    app.open_mtext_editor(glam::DVec3::ZERO, None, "ab", 2.5, None);
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::Symbols)));
    let _ = app.update(Message::MTextMenuPick(Box::new(Message::MTextInsert(
        "°".into(),
    ))));
    let ed = app.mtext_editor.as_ref().unwrap();
    assert_eq!(ed.menu, None);
    assert_eq!(ed.content.text(), "ab°");
    // The same trigger again closes its menu; another one switches.
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::More)));
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::More)));
    assert_eq!(app.mtext_editor.as_ref().unwrap().menu, None);
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::More)));
    let _ = app.update(Message::MTextMenu(Some(EditorMenu::Find)));
    assert_eq!(
        app.mtext_editor.as_ref().unwrap().menu,
        Some(EditorMenu::Find)
    );
}

fn app_with(raw: &str) -> crate::app::OpenCADStudio {
    let mut app = crate::app::OpenCADStudio::new_for_test();
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(1));
    app.active_tab = 1;
    app.open_mtext_editor(glam::DVec3::ZERO, None, raw, 2.5, None);
    app
}

fn mouse(event: iced::mouse::Event) -> Vec<Event> {
    vec![Event::Mouse(event)]
}

#[test]
fn the_ruler_handle_drags_the_box_width_and_the_band_is_not_text() {
    let mut app = app_with("");
    let ed = app.mtext_editor.as_mut().unwrap();
    ed.rect_width = 20.0;
    app.rebuild_mtext_preview();
    let ed = app.mtext_editor.as_ref().unwrap();
    let scale = ed.preview_scale();
    let mut ui = overlay_sim(ed, Size::new(1280., 720.));
    let view = ui
        .find(iced::widget::Id::new("mtext-editor-viewport"))
        .unwrap()
        .visible_bounds()
        .unwrap();
    // Inside the frame's padding: the empty text starts at the ruler's left end.
    let origin = view.x + 2. + MTEXT_PREVIEW_PAD;
    let handle = iced::Point::new(origin + 20.0 * scale, view.y + 2. + MTEXT_RULER_H * 0.5);
    ui.point_at(handle);
    ui.simulate(mouse(iced::mouse::Event::ButtonPressed(
        iced::mouse::Button::Left,
    )));
    let moved = iced::Point::new(handle.x + 5.0 * scale, handle.y);
    ui.point_at(moved);
    ui.simulate(mouse(iced::mouse::Event::CursorMoved { position: moved }));
    ui.simulate(mouse(iced::mouse::Event::ButtonReleased(
        iced::mouse::Button::Left,
    )));
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::MTextBoxDrag(Some(w)) if (w - 25.0).abs() < 0.1)),
        "{messages:?}"
    );
    assert!(messages
        .iter()
        .any(|m| matches!(m, Message::MTextBoxDrag(None))));
    assert!(!messages
        .iter()
        .any(|m| matches!(m, Message::MTextSelStart(_))));
    // A press on the band away from the handle does not move the caret.
    let mut ui = overlay_sim(ed, Size::new(1280., 720.));
    ui.point_at(iced::Point::new(origin + 5.0 * scale, handle.y));
    ui.simulate(simulator::click());
    assert!(!ui
        .into_messages()
        .any(|m| matches!(m, Message::MTextSelStart(_) | Message::MTextBoxDrag(_))));
}

#[test]
fn a_ruler_drag_is_one_undo_step_and_the_field_takes_typing() {
    let mut app = app_with("ab");
    let before = app.mtext_editor.as_ref().unwrap().rect_width;
    for width in [Some(30.0), Some(31.5), Some(33.0), None] {
        let _ = app.update(Message::MTextBoxDrag(width));
    }
    let ed = app.mtext_editor.as_ref().unwrap();
    assert_eq!(ed.rect_width, 33.0);
    assert!(ed.keep_width);
    assert_eq!(ed.rect_width_field(), "33");
    app.mtext_undo();
    assert_eq!(app.mtext_editor.as_ref().unwrap().rect_width, before);
    // Typing: a valid width applies at once; a half-typed one is shown as typed.
    let _ = app.update(Message::MTextRectWidthField("12.".into()));
    let ed = app.mtext_editor.as_ref().unwrap();
    assert_eq!(
        (ed.rect_width, ed.rect_width_field().as_str()),
        (12.0, "12.")
    );
    let _ = app.update(Message::MTextRectWidthField("x".into()));
    let ed = app.mtext_editor.as_ref().unwrap();
    assert_eq!((ed.rect_width, ed.rect_width_field().as_str()), (12.0, "x"));
    assert_eq!(
        app.mtext_editor
            .as_ref()
            .unwrap()
            .build_mtext()
            .rectangle_width,
        12.0
    );
}

#[test]
fn the_status_bar_counts_and_carries_the_three_actions() {
    let app = app_with(r"ab\Pc\S1/2;");
    let ed = app.mtext_editor.as_ref().unwrap();
    let mut ui = overlay_sim(ed, Size::new(1280., 720.));
    let counts = crate::tr!("mtext-toolbar", "counts", characters = 6, paragraphs = 2);
    assert!(ui.find(counts).is_ok());
    for (label, expected) in [
        (t!("Cancel").into_owned(), "cancel"),
        (t!("Apply").into_owned(), "apply"),
        (t!("Close Text Editor").into_owned(), "ok"),
    ] {
        let mut ui = overlay_sim(ed, Size::new(1280., 720.));
        ui.click(label.clone()).unwrap();
        let messages: Vec<_> = ui.into_messages().collect();
        let found = messages.iter().any(|m| match expected {
            "cancel" => matches!(m, Message::MTextCancel),
            "apply" => matches!(m, Message::MTextApply),
            _ => matches!(m, Message::MTextOk),
        });
        assert!(found, "{label}: {messages:?}");
    }
    // The actions sit under the text.
    let view = ui
        .find(iced::widget::Id::new("mtext-editor-viewport"))
        .unwrap()
        .visible_bounds()
        .unwrap();
    let apply = bounds_of(&mut ui, t!("Apply").into_owned());
    assert!(apply.y > view.y + view.height);
}

#[test]
fn a_selection_band_covers_descenders_without_reaching_the_next_line() {
    let glyph = |vis: usize, y: f32| crate::entities::text_support::GlyphBox {
        vis,
        xmin: vis as f32,
        xmax: vis as f32 + 1.0,
        ymin: y,
        ymax: y + 2.0,
        is_rtl: false,
        local: [0.0; 4],
    };
    let boxes = [glyph(0, 10.0), glyph(1, 6.0)];
    let (low, high) = line_band(&boxes, &boxes[0]);
    // Below the box for descenders, above it for accents.
    assert!(low < 10.0 && high > 12.0);
    // Never into the next line's half of the gap.
    assert!(low >= (8.0 + 10.0) * 0.5);
    let (low, high) = line_band(&boxes, &boxes[1]);
    assert!(high <= (8.0 + 10.0) * 0.5 && low < 6.0);
}

#[test]
fn a_narrow_editor_scrolls_the_second_row_to_its_last_menu() {
    let ed = editor();
    let mut ui = overlay_sim(&ed, Size::new(960., 600.));
    let strip = ui
        .find(iced::widget::Id::new("mtext-toolbar-2"))
        .unwrap()
        .visible_bounds()
        .unwrap();
    assert!(ui
        .find(label!("find"))
        .unwrap()
        .visible_bounds()
        .is_none_or(|b| b.x + b.width > strip.x + strip.width));
    ui.point_at(strip.center());
    ui.simulate([Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Pixels { x: -1000., y: 0. },
    })]);
    let find = bounds_of(&mut ui, label!("find"));
    assert!(find.x + find.width <= strip.x + strip.width + 1.);
}

#[test]
fn the_counts_read_one_character_and_one_paragraph_in_the_singular() {
    let one = crate::tr!("mtext-toolbar", "counts", characters = 1, paragraphs = 1);
    let more = crate::tr!("mtext-toolbar", "counts", characters = 2, paragraphs = 3);
    // Fluent isolates each placeable with FSI / PDI marks.
    let plain = |s: String| s.replace(['\u{2068}', '\u{2069}'], "");
    assert_eq!(plain(one), "1 character · 1 paragraph");
    assert_eq!(plain(more), "2 characters · 3 paragraphs");
}
