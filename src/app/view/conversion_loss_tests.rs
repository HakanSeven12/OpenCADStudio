use super::*;
use crate::app::ModalKind;
use iced::{mouse, Event, Point, Rectangle, Vector};
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

fn warning(app: &OpenCADStudio) -> Element<'_, Message> {
    crate::ui::modal::modal(
        Space::new().width(Fill).height(Fill),
        "Save warning",
        app.modal_content().expect("conversion warning"),
        Message::ConversionLossBack,
        Vector::ZERO,
        crate::ui::modal::ModalOptions::STANDARD,
    )
}

fn visible(ui: &mut Simulator<'_, Message>, label: &str) -> Rectangle {
    let target = ui
        .find(label)
        .unwrap_or_else(|error| panic!("{label}: {error}"));
    let bounds = target.bounds();
    let clipped = target.visible_bounds().expect("control must be visible");
    assert!(
        clipped.width >= bounds.width - 0.1 && clipped.height >= bounds.height - 0.1,
        "{label} must be fully visible: {bounds:?}, clipped to {clipped:?}"
    );
    bounds
}

fn capture(ui: &mut Simulator<'_, Message>, name: &str) {
    if let Some(directory) = std::env::var_os("OCS_SAVE_WARNING_CAPTURES") {
        assert!(ui
            .snapshot(&Theme::Dark)
            .expect("render warning")
            .matches_image(std::path::PathBuf::from(directory).join(format!("{name}.png")))
            .expect("write warning capture"));
    }
}

#[test]
fn conversion_loss_short_list_keeps_compact_dialog_and_existing_actions() {
    with_ui_test(|| {
        let mut app = OpenCADStudio::new_for_test();
        app.active_modal = Some(ModalKind::ConversionLoss);
        app.conversion_losses = vec!["First diagnostic".into()];
        let mut ui =
            Simulator::with_size(iced::Settings::default(), (1024.0, 768.0), warning(&app));
        let first = visible(&mut ui, "• First diagnostic");
        let save = visible(&mut ui, t!("Save anyway").as_ref());
        assert!(save.y - first.y < 100.0, "short list should stay compact");
        capture(&mut ui, "short");
        ui.click(t!("Save anyway").as_ref()).expect("save button");
        ui.click(t!("Back").as_ref()).expect("back button");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages
            .iter()
            .any(|message| matches!(message, Message::ConversionLossProceed)));
        assert!(messages
            .iter()
            .any(|message| matches!(message, Message::ConversionLossBack)));
    });
}

#[test]
fn conversion_loss_long_list_scrolls_without_moving_actions() {
    with_ui_test(|| {
        let mut app = OpenCADStudio::new_for_test();
        app.active_modal = Some(ModalKind::ConversionLoss);
        app.conversion_losses = (1..=90).map(|i| format!("Diagnostic {i}")).collect();
        for (name, size, extra) in [
            ("many", (1024.0, 768.0), Vector::ZERO),
            ("small", (640.0, 480.0), Vector::ZERO),
            ("shrunk", (420.0, 300.0), Vector::new(300.0, 400.0)),
        ] {
            app.modal_resize = extra;
            let mut ui = Simulator::with_size(iced::Settings::default(), size, warning(&app));
            let save = visible(&mut ui, t!("Save anyway").as_ref());
            let back = visible(&mut ui, t!("Back").as_ref());
            let explanation = t!(
                "Saving as \"%{target}\" cannot keep the following, so it will be lost:",
                target = "",
            );
            let header = visible(&mut ui, explanation.as_ref());
            let first = visible(&mut ui, "• Diagnostic 1");
            assert!(ui
                .find("• Diagnostic 90")
                .expect("last diagnostic retained")
                .visible_bounds()
                .is_none());
            capture(&mut ui, &format!("{name}-top"));
            ui.point_at(first.center());
            ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1000.0 },
            })]);
            visible(&mut ui, "• Diagnostic 90");
            assert_eq!(visible(&mut ui, t!("Save anyway").as_ref()), save);
            assert_eq!(visible(&mut ui, t!("Back").as_ref()), back);
            assert_eq!(visible(&mut ui, explanation.as_ref()), header);
            capture(&mut ui, &format!("{name}-bottom"));
            ui.click(t!("Back").as_ref())
                .expect("back remains clickable");
            assert!(ui
                .into_messages()
                .any(|message| matches!(message, Message::ConversionLossBack)));
        }
    });
}

#[test]
fn conversion_loss_wrapped_and_unbroken_messages_remain_readable() {
    with_ui_test(|| {
        let mut app = OpenCADStudio::new_for_test();
        app.active_modal = Some(ModalKind::ConversionLoss);
        app.conversion_losses = vec![
            "A long diagnostic with multiple words that must wrap inside the available dialog width. ".repeat(6),
            "UnbrokenDiagnostic".repeat(50),
            "Final diagnostic".into(),
        ];
        let mut ui = Simulator::with_size(iced::Settings::default(), (420.0, 300.0), warning(&app));
        visible(&mut ui, t!("Save anyway").as_ref());
        visible(&mut ui, t!("Back").as_ref());
        let first = ui
            .find(format!("• {}", app.conversion_losses[0]).as_str())
            .expect("wrapped diagnostic");
        assert!(first.bounds().height > 30.0);
        assert!(first.bounds().x >= 0.0 && first.bounds().x + first.bounds().width <= 420.0);
        let unbroken = ui
            .find(format!("• {}", app.conversion_losses[1]).as_str())
            .expect("unbroken diagnostic");
        assert!(unbroken.bounds().height > 30.0);
        assert!(
            unbroken.bounds().x >= 0.0 && unbroken.bounds().x + unbroken.bounds().width <= 420.0
        );
        ui.point_at(Point::new(
            first.bounds().center_x(),
            first.visible_bounds().expect("first visible").center_y(),
        ));
        ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1000.0 },
        })]);
        visible(&mut ui, "• Final diagnostic");
        capture(&mut ui, "wrapped-bottom");
    });
}

mod drawing_properties_loss {
    use super::*;
    use crate::app::{SaveContinuation, SavePurpose};
    use iced::futures::StreamExt;
    use iced::Task;

    /// Run a task to completion on the current thread and collect its messages.
    fn run(task: Task<Message>) -> Vec<Message> {
        let mut messages = Vec::new();
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = iced::futures::executor::block_on(stream.next()) {
                if let iced_runtime::Action::Output(message) = action {
                    messages.push(message);
                }
            }
        }
        messages
    }

    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "ocs-dwgprops-loss-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("scratch dir");
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn app_with_properties() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let info = &mut app.tabs[0].scene.document.summary_info;
        info.custom_properties = vec![
            ("Project".into(), "Bridge".into()),
            ("Client".into(), "ACME, Inc.".into()),
            ("Area".into(), "12,5".into()),
        ];
        app
    }

    fn save_as(
        app: &mut OpenCADStudio,
        path: &std::path::Path,
        version: codec::DxfVersion,
    ) -> Task<Message> {
        app.queue_native_save(
            0,
            path.to_path_buf(),
            version,
            SavePurpose::SaveAs,
            SaveContinuation::None,
            false,
            false,
        )
    }

    // S5: "Last saved by" is stamped, not authored; alone it is nothing to warn about.
    #[test]
    fn last_saved_by_alone_saves_to_dwg_r2000_without_asking() {
        let scratch = Scratch::new("stamped");
        let path = scratch.0.join("old.dwg");
        let mut app = OpenCADStudio::new_for_test();
        app.tabs[0].scene.document.summary_info.last_saved_by = "someone".into();
        let _ = run(save_as(&mut app, &path, codec::DxfVersion::AC1015));
        assert_ne!(app.active_modal, Some(ModalKind::ConversionLoss));
        assert!(path.exists(), "the save went ahead");
    }

    // S5
    #[test]
    fn saving_custom_properties_to_dwg_r2000_asks_before_writing() {
        let scratch = Scratch::new("ask");
        let path = scratch.0.join("old.dwg");
        let mut app = app_with_properties();
        let task = save_as(&mut app, &path, codec::DxfVersion::AC1015);
        assert!(run(task).is_empty(), "nothing is saved before the answer");
        assert_eq!(app.active_modal, Some(ModalKind::ConversionLoss));
        assert!(!path.exists(), "nothing is written before the answer");
        assert_eq!(
            app.conversion_losses,
            vec![crate::tr!(
                "dwgprops-dialog",
                "save-loss",
                format = "DWG",
                count = "3"
            )]
        );
    }

    // S5
    #[test]
    fn saving_custom_properties_to_dxf_r2000_asks_before_writing() {
        let scratch = Scratch::new("ask-dxf");
        let path = scratch.0.join("old.dxf");
        let mut app = app_with_properties();
        let _ = save_as(&mut app, &path, codec::DxfVersion::AC1015);
        assert_eq!(app.active_modal, Some(ModalKind::ConversionLoss));
        assert!(!path.exists());
        assert_eq!(
            app.conversion_losses,
            vec![crate::tr!(
                "dwgprops-dialog",
                "save-loss",
                format = "DXF",
                count = "3"
            )]
        );
    }

    // S5
    #[test]
    fn save_anyway_writes_the_file_and_leaves_the_document_unchanged() {
        let scratch = Scratch::new("proceed");
        let path = scratch.0.join("old.dwg");
        let mut app = app_with_properties();
        let before = app.tabs[0].scene.document.summary_info.clone();
        let _ = save_as(&mut app, &path, codec::DxfVersion::AC1015);
        let _ = run(app.update(Message::ConversionLossProceed));
        assert_eq!(app.active_modal, None);
        assert!(path.exists(), "the held-back save is replayed");
        assert_eq!(app.tabs[0].scene.document.summary_info, before);
    }

    // S5
    #[test]
    fn back_discards_the_held_save() {
        let scratch = Scratch::new("back");
        let path = scratch.0.join("old.dwg");
        let mut app = app_with_properties();
        let _ = save_as(&mut app, &path, codec::DxfVersion::AC1015);
        let _ = app.update(Message::ConversionLossBack);
        assert_eq!(app.active_modal, None);
        assert!(!path.exists());
        assert!(app.tabs[0].scene.document.summary_info.custom_properties.len() == 3);
    }

    // S5
    #[test]
    fn nothing_to_lose_saves_to_dwg_r2000_without_asking() {
        let scratch = Scratch::new("empty");
        let path = scratch.0.join("old.dwg");
        let mut app = OpenCADStudio::new_for_test();
        let _ = run(save_as(&mut app, &path, codec::DxfVersion::AC1015));
        assert_eq!(app.active_modal, None);
        assert!(path.exists());
    }

    // S3, S5
    #[test]
    fn dwg_2018_keeps_custom_properties_in_order_without_asking() {
        let scratch = Scratch::new("round-trip");
        let path = scratch.0.join("new.dwg");
        let mut app = app_with_properties();
        let expected = app.tabs[0].scene.document.summary_info.custom_properties.clone();
        let _ = run(save_as(&mut app, &path, codec::DxfVersion::AC1032));
        assert_eq!(app.active_modal, None);
        assert!(path.exists());
        let reopened = crate::io::load_file(&path).expect("reopen saved drawing");
        assert_eq!(reopened.summary_info.custom_properties, expected);
    }

    // S3, S5
    #[test]
    fn dxf_2018_keeps_custom_properties_in_order_without_asking() {
        let scratch = Scratch::new("round-trip-dxf");
        let path = scratch.0.join("new.dxf");
        let mut app = app_with_properties();
        let expected = app.tabs[0].scene.document.summary_info.custom_properties.clone();
        let _ = run(save_as(&mut app, &path, codec::DxfVersion::AC1032));
        assert_eq!(app.active_modal, None);
        let reopened = crate::io::load_file(&path).expect("reopen saved drawing");
        assert_eq!(reopened.summary_info.custom_properties, expected);
    }

    // S5: QSAVE keeps the stored version, so an old file is guarded too.
    #[test]
    fn quick_save_of_an_old_version_drawing_asks() {
        let scratch = Scratch::new("qsave");
        let path = scratch.0.join("old.dwg");
        let mut app = app_with_properties();
        let _ = app.queue_native_save(
            0,
            path.clone(),
            codec::DxfVersion::AC1015,
            SavePurpose::Manual,
            SaveContinuation::None,
            false,
            false,
        );
        assert_eq!(app.active_modal, Some(ModalKind::ConversionLoss));
        assert!(!path.exists());
    }

    // S5: autosave cannot answer a modal and only writes a recovery copy.
    #[test]
    fn autosave_never_asks() {
        let scratch = Scratch::new("autosave");
        let path = scratch.0.join("old.dwg.ocs-autosave.sv$");
        let mut app = app_with_properties();
        let task = app.queue_native_save(
            0,
            path.clone(),
            codec::DxfVersion::AC1015,
            SavePurpose::Autosave,
            SaveContinuation::None,
            false,
            false,
        );
        let _ = run(task);
        assert_eq!(app.active_modal, None);
    }
}
