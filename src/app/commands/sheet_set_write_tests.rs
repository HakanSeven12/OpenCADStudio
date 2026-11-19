//! Sheet set forms that write their drawing on a worker: Rename & Renumber
//! of a closed sheet, and New Sheet.
use super::*;
use serde_json::json;

/// A sheet set action through the automation protocol, its tasks driven.
fn sheet(app: &mut OpenCADStudio, value: &str) -> serde_json::Value {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let state = app.control_state();
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let req = json!({
        "protocol": 1,
        "document_id": state["document_id"].clone(),
        "revision": state["revision"].clone(),
        "client_id": "test",
        "request_id": format!("write-{id}"),
        "op": "action",
        "name": "sheet_set",
        "value": value,
    });
    app.automation_op(&req.to_string())
}

/// An app with a new set in a new folder and its sheets `numbers` (title
/// "Plan"), not open; the folder.
fn set_with_sheets(stem: &str, numbers: &[&str]) -> (OpenCADStudio, PathBuf) {
    let dir = std::env::temp_dir().join(format!("ocs-ss-write-{stem}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    let folder = dir.display().to_string().replace('\\', "/");
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let _ = sheet(&mut app, "new");
    let _ = sheet(&mut app, "existing=0");
    let _ = sheet(&mut app, "name=WRITESET");
    let _ = sheet(&mut app, &format!("folder={folder}"));
    let _ = sheet(&mut app, "ok");
    for number in numbers {
        let _ = sheet(&mut app, "menu=new_sheet:set");
        let _ = sheet(&mut app, &format!("number={number}"));
        let _ = sheet(&mut app, "title=Plan");
        let _ = sheet(&mut app, &format!("folder={folder}"));
        let _ = sheet(&mut app, "ok");
    }
    assert_eq!(sheets(&app), numbers.len(), "the sheets are in the set");
    (app, dir)
}

fn sheets(app: &OpenCADStudio) -> usize {
    app.sheet_set.db().map_or(0, |db| db.sheets().len())
}

/// The set's layout name for sheet `number`.
fn layout_of(app: &OpenCADStudio, number: &str) -> String {
    let id = app.sheet_set_target(number).expect("the sheet");
    let db = app.sheet_set.db().unwrap();
    db.layout_reference(&id, "Layout").unwrap_or_default().name
}

/// The paper-space layouts of the drawing at `path`.
fn layouts(path: &Path) -> Vec<String> {
    let doc = crate::io::load_file(path).expect("the drawing reads");
    paper_layouts(&doc).into_iter().map(|(n, _)| n).collect()
}

/// The Rename & Renumber form of sheet `number`, its layout name changed.
fn rename_form_of(app: &mut OpenCADStudio, number: &str) -> Form {
    let _ = sheet(app, &format!("menu=rename:{number}"));
    let _ = sheet(app, "layout_name=Ground Floor");
    let Some(SsDialog::Form(f)) = app.sheet_set.dialog.take() else {
        panic!("the Rename & Renumber form is open");
    };
    f
}

/// The New Sheet form for sheet `number` in folder `dir`.
fn new_sheet_form(app: &mut OpenCADStudio, number: &str, dir: &Path) -> Form {
    let _ = sheet(app, "menu=new_sheet:set");
    let _ = sheet(app, &format!("number={number}"));
    let _ = sheet(app, "title=Plan");
    let folder = dir.display().to_string().replace('\\', "/");
    let _ = sheet(app, &format!("folder={folder}"));
    let Some(SsDialog::Form(f)) = app.sheet_set.dialog.take() else {
        panic!("the New Sheet form is open");
    };
    f
}

/// A closed sheet's layout is renamed in its file on a worker; the set
/// changes, and the dialog closes, when the write ends.
#[test]
fn a_closed_sheet_layout_is_renamed_on_a_worker() {
    let _sets = crate::entities::field::sheet_sets_one_at_a_time();
    let (mut app, dir) = set_with_sheets("rename", &["A-1"]);
    let f = rename_form_of(&mut app, "A-1");
    let task = app.apply_rename(f, None, false, write_renamed_layout);
    assert_eq!(
        layout_of(&app, "A-1"),
        "A-1 Plan",
        "the set waits for the write"
    );
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(
            &message,
            Message::SheetSet(SheetSetMsg::RenameWritten(_, None, Ok(())))
        ),
        "{message:?}"
    );
    let _ = app.update(message);
    assert_eq!(layout_of(&app, "A-1"), "Ground Floor");
    assert_eq!(layouts(&dir.join("A-1 Plan.dwg")), ["Ground Floor"]);
    assert!(app.sheet_set.dialog.is_none(), "the dialog is closed");
}

/// A writer panic is caught on the worker: the form comes back with it,
/// and the set is not changed.
#[test]
fn a_rename_writer_panic_brings_the_form_back() {
    let _sets = crate::entities::field::sheet_sets_one_at_a_time();
    let (mut app, _dir) = set_with_sheets("rename-panic", &["A-1"]);
    let f = rename_form_of(&mut app, "A-1");
    let task = app.apply_rename(f, None, false, |_, _, _, _| {
        panic!("writer panic in the rename")
    });
    let _ = app.update(OpenCADStudio::first_task_message(task));
    let Some(SsDialog::Form(f)) = &app.sheet_set.dialog else {
        panic!("the form is back");
    };
    let error = f.error.clone().unwrap_or_default();
    assert!(error.contains("writer panic in the rename"), "{error}");
    assert_eq!(layout_of(&app, "A-1"), "A-1 Plan", "the set is not changed");
}

/// An error after another dialog opened goes to the command line; that
/// dialog stays.
#[test]
fn a_rename_error_after_another_dialog_opened_keeps_that_dialog() {
    let _sets = crate::entities::field::sheet_sets_one_at_a_time();
    let (mut app, _dir) = set_with_sheets("rename-other", &["A-1"]);
    let f = rename_form_of(&mut app, "A-1");
    let task = app.apply_rename(f, None, false, |_, _, _, _| {
        panic!("writer panic in the rename")
    });
    let _ = sheet(&mut app, "menu=new_subset:set");
    let _ = app.update(OpenCADStudio::first_task_message(task));
    let Some(SsDialog::Form(f)) = &app.sheet_set.dialog else {
        panic!("a form is open");
    };
    assert_eq!(f.kind, FormKind::NewSubset, "the other dialog stays");
    assert!(f.error.is_none());
    let history = app.command_line.history_plain_text();
    assert!(history.contains("writer panic in the rename"), "{history}");
}

/// Next after the write goes to the next sheet's form.
#[test]
fn rename_next_goes_to_the_next_sheet_after_the_write() {
    let _sets = crate::entities::field::sheet_sets_one_at_a_time();
    let (mut app, _dir) = set_with_sheets("rename-next", &["A-1", "A-2"]);
    let f = rename_form_of(&mut app, "A-1");
    app.sheet_set.dialog = Some(SsDialog::Form(f));
    let task = app.on_sheet_set(SheetSetMsg::Next);
    assert!(app.sheet_set.dialog.is_none(), "no form during the write");
    let _ = app.update(OpenCADStudio::first_task_message(task));
    let Some(SsDialog::Form(f)) = &app.sheet_set.dialog else {
        panic!("the next sheet's form is open");
    };
    assert_eq!(Some(f.component.clone()), app.sheet_set_target("A-2"));
    assert_eq!(layout_of(&app, "A-1"), "Ground Floor");
}

/// New Sheet writes its drawing on a worker; the sheet joins the set
/// when the write ends.
#[test]
fn a_new_sheet_is_written_on_a_worker() {
    let _sets = crate::entities::field::sheet_sets_one_at_a_time();
    let (mut app, dir) = set_with_sheets("new", &[]);
    let f = new_sheet_form(&mut app, "B-1", &dir);
    let task = app.create_sheet_with(f, write_new_sheet);
    assert_eq!(sheets(&app), 0, "the set waits for the write");
    let message = OpenCADStudio::first_task_message(task);
    assert!(
        matches!(
            &message,
            Message::SheetSet(SheetSetMsg::SheetWritten(_, _, Ok(_)))
        ),
        "{message:?}"
    );
    let _ = app.update(message);
    assert_eq!(sheets(&app), 1);
    assert_eq!(layout_of(&app, "B-1"), "B-1 Plan");
    assert_eq!(layouts(&dir.join("B-1 Plan.dwg")), ["B-1 Plan"]);
    assert!(app.sheet_set.dialog.is_none(), "the dialog is closed");
    // The set records the layout's handle; the drawing's link records the
    // revision of the set's write.
    let doc = crate::io::load_file(&dir.join("B-1 Plan.dwg")).unwrap();
    let (_, handle) = paper_layouts(&doc).remove(0);
    assert!(!handle.is_empty());
    let id = app.sheet_set_target("B-1").unwrap();
    let db = app.sheet_set.db().unwrap();
    assert_eq!(db.layout_reference(&id, "Layout").unwrap().handle, handle);
    let link = doc.sheet_set_data().expect("the sheet link");
    assert_eq!(link.sheet_set_version, db.file_revision());
}

/// A writer panic is caught on the worker: the form comes back with it,
/// and no sheet joins the set.
#[test]
fn a_new_sheet_writer_panic_brings_the_form_back() {
    let _sets = crate::entities::field::sheet_sets_one_at_a_time();
    let (mut app, dir) = set_with_sheets("new-panic", &[]);
    let f = new_sheet_form(&mut app, "B-1", &dir);
    let task = app.create_sheet_with(f, |_| panic!("writer panic in the new sheet"));
    let _ = app.update(OpenCADStudio::first_task_message(task));
    let Some(SsDialog::Form(f)) = &app.sheet_set.dialog else {
        panic!("the form is back");
    };
    let error = f.error.clone().unwrap_or_default();
    assert!(error.contains("writer panic in the new sheet"), "{error}");
    assert_eq!(sheets(&app), 0, "no sheet joins the set");
}
