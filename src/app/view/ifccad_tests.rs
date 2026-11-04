use super::*;
#[cfg(not(target_arch = "wasm32"))]
use crate::app::{SaveContinuation, SavePurpose};
use iced::Task;

#[test]
fn ifccad_format_label_keeps_the_ifcx_extension_and_legacy_selection() {
    let options = crate::io::save_format_options();
    assert!(options.contains(&"IFCCAD".to_string()));
    assert!(!options.contains(&"IFCX".to_string()));
    for label in ["IFCCAD", "IFCX", "ifccad"] {
        assert_eq!(crate::io::parse_save_format(label).0, "ifcx");
        assert_eq!(crate::io::canonical_save_format(label), "IFCCAD");
    }
    assert_eq!(codec::Format::name(&codec::Ifcx), "IFCX");
}

#[cfg(not(target_arch = "wasm32"))]
fn save(app: &mut OpenCADStudio, tab: usize, ext: &str, purpose: SavePurpose) -> Task<Message> {
    let path = std::env::temp_dir().join(format!(
        "ocs-ifccad-alpha-{}-{}.{ext}",
        std::process::id(),
        app.tabs[tab].id
    ));
    app.queue_native_save(
        tab,
        path,
        codec::DxfVersion::AC1032,
        purpose,
        SaveContinuation::None,
        false,
        false,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn ask_alpha(app: &mut OpenCADStudio, tab: usize, purpose: SavePurpose) -> Task<Message> {
    let task = save(app, tab, "ifcx", purpose);
    if app.active_modal == Some(crate::app::ModalKind::ConversionLoss) {
        // OCS's initial scene can contain separately diagnosed converter losses.
        app.update(Message::ConversionLossProceed)
    } else {
        task
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn ifccad_first_interactive_save_waits_for_alpha_consent() {
    let mut app = OpenCADStudio::new_for_test();
    app.main_window = Some(iced::window::Id::unique());
    let _ = ask_alpha(&mut app, 0, SavePurpose::Manual);
    assert!(
        app.active_modal.is_some(),
        "first IFCCAD save must wait for consent"
    );
    assert!(app.modal_title().contains("IFCCAD"));
    assert!(
        app.active_save_jobs.is_empty(),
        "must not start writing before consent"
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn ifccad_alpha_cancel_does_not_acknowledge_the_warning() {
    let mut app = OpenCADStudio::new_for_test();
    app.main_window = Some(iced::window::Id::unique());
    let _ = ask_alpha(&mut app, 0, SavePurpose::SaveAs);
    let _ = app.update(Message::CloseModal);
    assert_eq!(app.active_modal, None);
    let _ = ask_alpha(&mut app, 0, SavePurpose::Manual);
    assert!(
        app.active_modal.is_some(),
        "cancel must ask again on the next save"
    );
    assert!(app.active_save_jobs.is_empty());
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn ifccad_consent_belongs_to_the_saved_tab_and_includes_conversion_losses() {
    use crate::app::ModalKind;
    let mut app = OpenCADStudio::new_for_test();
    app.main_window = Some(iced::window::Id::unique());
    let _ = ask_alpha(&mut app, 0, SavePurpose::Manual);
    assert_eq!(app.active_modal, Some(ModalKind::IfccadAlphaWarning));
    app.push_test_document();
    app.active_tab = 1;
    let task = app.update(Message::IfccadAlphaProceed);
    use iced::futures::StreamExt;
    if let Some(mut stream) = iced_runtime::task::into_stream(task) {
        while let Some(action) = iced::futures::executor::block_on(stream.next()) {
            if let iced_runtime::Action::Output(message) = action {
                let _ = app.update(message);
            }
        }
    }
    let path = std::env::temp_dir().join(format!(
        "ocs-ifccad-alpha-{}-{}.ifcx",
        std::process::id(),
        app.tabs[0].id
    ));
    crate::io::load_file(&path).expect("accepted save reopens through production reader");
    std::fs::remove_file(&path).unwrap();
    app.tabs[0]
        .scene
        .document
        .add_entity(codec::EntityType::Ray(codec::entities::Ray::new(
            codec::types::Vector3::ZERO,
            codec::types::Vector3::UNIT_X,
        )))
        .unwrap();
    let _ = save(&mut app, 0, "ifcx", SavePurpose::Manual);
    assert!(app.tabs[0].ifccad_alpha_acknowledged);
    assert!(!app.tabs[1].ifccad_alpha_acknowledged);
    assert_eq!(app.active_modal, Some(ModalKind::IfccadAlphaWarning));
    assert!(app
        .conversion_losses
        .iter()
        .any(|loss| loss.to_ascii_lowercase().contains("ray")));
    assert!(!app.conversion_loss_allowed);
    let _ = app.update(Message::CloseModal);
    let _ = ask_alpha(&mut app, 1, SavePurpose::Manual);
    assert_eq!(app.active_modal, Some(ModalKind::IfccadAlphaWarning));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn ifccad_alpha_does_not_interrupt_headless_autosave_or_dwg() {
    let mut app = OpenCADStudio::new_for_test();
    let _ = save(&mut app, 0, "ifcx", SavePurpose::Manual);
    assert_ne!(
        app.active_modal,
        Some(crate::app::ModalKind::IfccadAlphaWarning)
    );
    let mut app = OpenCADStudio::new_for_test();
    app.main_window = Some(iced::window::Id::unique());
    let _ = save(&mut app, 0, "ifcx", SavePurpose::Autosave);
    assert_ne!(
        app.active_modal,
        Some(crate::app::ModalKind::IfccadAlphaWarning)
    );
    let mut app = OpenCADStudio::new_for_test();
    app.main_window = Some(iced::window::Id::unique());
    let _ = save(&mut app, 0, "dwg", SavePurpose::Manual);
    assert_ne!(
        app.active_modal,
        Some(crate::app::ModalKind::IfccadAlphaWarning)
    );
}

#[test]
fn ifccad_alpha_buttons_are_visible_and_emit_separate_actions() {
    let _guard = crate::headless_ui_lock();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(|| {
            for with_losses in [false, true] {
                let mut app = OpenCADStudio::new_for_test();
                app.active_modal = Some(crate::app::ModalKind::IfccadAlphaWarning);
                if with_losses {
                    app.conversion_losses = vec!["One unsupported entity".into()];
                }
                let title = app.modal_title();
                let mut ui = iced_test::Simulator::with_size(
                    iced::Settings::default(),
                    (640., 480.),
                    crate::ui::modal::modal(
                        Space::new().width(Fill).height(Fill),
                        &title,
                        app.modal_content().unwrap(),
                        Message::CloseModal,
                        iced::Vector::ZERO,
                        crate::ui::modal::ModalOptions::STANDARD,
                    ),
                );
                if with_losses {
                    assert!(ui
                        .find("• One unsupported entity")
                        .unwrap()
                        .visible_bounds()
                        .is_some());
                }
                for label in [
                    t!("Save anyway").into_owned(),
                    t!("Cancel").into_owned(),
                ] {
                    assert!(ui.find(label.as_str()).unwrap().visible_bounds().is_some());
                    ui.click(label.as_str()).unwrap();
                }
                let messages: Vec<_> = ui.into_messages().collect();
                assert!(messages
                    .iter()
                    .any(|m| matches!(m, Message::IfccadAlphaProceed)));
                assert!(messages.iter().any(|m| matches!(m, Message::CloseModal)));
            }
        })
        .unwrap()
        .join()
        .unwrap();
}
