use super::*;
use crate::app::ModalKind;
use iced_test::Simulator;

fn with_ui_test(test: impl FnOnce() + Send + 'static) {
    let _guard = crate::headless_ui_lock();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(test)
        .expect("spawn UI test")
        .join()
        .unwrap_or_else(|error| std::panic::resume_unwind(error));
}

fn dirty_definition_edit() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "EDIT_ME",
        codec::types::Vector3::ZERO,
        vec![codec::EntityType::Line(codec::entities::Line::from_points(
            codec::types::Vector3::ZERO,
            codec::types::Vector3::UNIT_X,
        ))],
    );
    let reference = app.tabs[i]
        .scene
        .document
        .add_entity(codec::EntityType::Insert(codec::entities::Insert::new(
            "EDIT_ME",
            codec::types::Vector3::ZERO,
        )))
        .unwrap();
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", reference.value()));
    let _ = app.automation_op(r#"{"op":"run","cmd":"CIRCLE 1,0 2"}"#);
    app.finish_pending_history(app.active_tab);
    let _ = app.dispatch_command("BCLOSE");
    assert_eq!(app.active_modal, Some(ModalKind::BlockEditChanges));
    app
}

fn dialog(app: &OpenCADStudio) -> Element<'_, Message> {
    crate::ui::modal::modal(
        Space::new().width(Fill).height(Fill),
        "Block - Save Parameter Changes?",
        app.modal_content().expect("block close dialog"),
        Message::BlockEditChangesCancel,
        iced::Vector::ZERO,
        crate::ui::modal::ModalOptions::STANDARD,
    )
}

fn visible(ui: &mut Simulator<'_, Message>, label: &str) -> iced::Rectangle {
    let target = ui
        .find(label)
        .unwrap_or_else(|error| panic!("{label}: {error}"));
    let clipped = target.visible_bounds().expect("control must be visible");
    clipped
}

#[test]
fn block_close_dialog_explains_the_risk_and_counts_references() {
    with_ui_test(|| {
        let app = dirty_definition_edit();
        assert_eq!(app.modal_title(), "Block - Save Parameter Changes?");
        let mut ui = Simulator::with_size(
            iced::Settings::default(),
            (900.0, 700.0),
            dialog(&app),
        );
        visible(&mut ui, "Save changes to EDIT_ME?");
        visible(
            &mut ui,
            "Changes to parameters may update existing block references in ways that are not always obvious. What do you want to do?",
        );
        visible(&mut ui, "Number of block references in the drawing: 1");
        visible(&mut ui, "Save the changes");
        visible(&mut ui, "Discard the changes");
        visible(&mut ui, "Cancel");
        ui.click("Save the changes").expect("save button");
        assert!(ui
            .into_messages()
            .any(|message| matches!(message, Message::BlockEditChangesSave)));
    });
}
