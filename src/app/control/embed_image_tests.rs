//! The automation `embed_image` reads and decodes its picture on a worker.
use super::*;
use document::EntityType;

fn fresh_drawing() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let response = app.automation_op(r#"{"op":"new"}"#);
    assert_eq!(response["ok"], true, "{response}");
    app
}

/// The settled response of request `id`.
fn settled(app: &OpenCADStudio, id: &str) -> Value {
    app.control
        .completed
        .iter()
        .find(|(request, _, _)| request == id)
        .map(|(_, _, response)| response.clone())
        .unwrap_or(Value::Null)
}

/// A 4 x 2 PNG picture in a new folder; its path.
fn picture(stem: &str) -> String {
    let dir = std::env::temp_dir().join(format!("ocs-embed-image-{stem}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("plan.png");
    image::RgbImage::new(4, 2)
        .save(&path)
        .expect("the picture is written");
    path.display().to_string()
}

fn frames(app: &OpenCADStudio, i: usize) -> usize {
    app.tabs[i]
        .scene
        .document
        .entities()
        .filter(|e| matches!(e, EntityType::Ole2Frame(_)))
        .count()
}

/// The request for `path`, 8 units wide at the origin.
fn embed(app: &mut OpenCADStudio, id: &str, path: &str) -> Task<Message> {
    let document_id = app.tabs[app.active_tab].id;
    let (response, task) = app.control_request(json!({
        "protocol":1,"op":"embed_image","request_id":id,
        "document_id":document_id,"path":path,"at":[0,0],"width":8,
    }));
    assert_eq!(response["status"], "accepted", "{response}");
    task
}

/// The operation reads on a worker: nothing is placed until the worker
/// ends, then it settles with its result.
#[test]
fn the_operation_settles_when_its_read_ends() {
    let mut app = fresh_drawing();
    let i = app.active_tab;
    let path = picture("worker");
    let task = embed(&mut app, "embed-worker", &path);
    assert_eq!(frames(&app, i), 0, "nothing is placed during the read");
    app.drive_headless_task(task).expect("the operation's task");
    let settled = settled(&app, "embed-worker");
    assert_eq!(settled["ok"], true, "{settled}");
    assert_eq!(settled["result"]["kind"], "Ole2Frame", "{settled}");
    assert_eq!(settled["result"]["height"], 4.0, "{settled}");
    assert_eq!(frames(&app, i), 1);
}

/// A file that is not a picture fails the operation with `embed_failed`,
/// as it failed at once before.
#[test]
fn a_file_that_is_not_a_picture_fails_the_operation() {
    let mut app = fresh_drawing();
    let i = app.active_tab;
    let path = picture("not-a-picture");
    std::fs::write(&path, b"not a picture").unwrap();
    let task = embed(&mut app, "embed-not-a-picture", &path);
    app.drive_headless_task(task).expect("the operation's task");
    let settled = settled(&app, "embed-not-a-picture");
    assert_eq!(settled["ok"], false, "{settled}");
    assert_eq!(settled["code"], "embed_failed", "{settled}");
    assert_eq!(frames(&app, i), 0);
}

/// A request that cannot be placed (a bad point) fails the operation after
/// the read with the same code as before.
#[test]
fn a_bad_point_fails_the_operation_after_the_read() {
    let mut app = fresh_drawing();
    let i = app.active_tab;
    let path = picture("bad-point");
    let document_id = app.tabs[i].id;
    let (_, task) = app.control_request(json!({
        "protocol":1,"op":"embed_image","request_id":"embed-bad-point",
        "document_id":document_id,"path":path,"at":[0],
    }));
    app.drive_headless_task(task).expect("the operation's task");
    let settled = settled(&app, "embed-bad-point");
    assert_eq!(settled["ok"], false, "{settled}");
    assert_eq!(settled["code"], "invalid_point", "{settled}");
    assert_eq!(frames(&app, i), 0);
}

/// The operation fails with `document_closed` when its drawing was
/// replaced during the read, and places nothing.
#[test]
fn a_replaced_drawing_fails_the_operation() {
    let mut app = fresh_drawing();
    let i = app.active_tab;
    let path = picture("replaced");
    let task = embed(&mut app, "embed-replaced", &path);
    app.tabs[i].document_generation += 1;
    app.drive_headless_task(task).expect("the operation's task");
    let settled = settled(&app, "embed-replaced");
    assert_eq!(settled["ok"], false, "{settled}");
    assert_eq!(settled["code"], "document_closed", "{settled}");
    assert_eq!(frames(&app, i), 0);
}

/// A reader that panics settles the operation as failed, and the app goes
/// on.
#[test]
fn a_reader_panic_fails_the_operation() {
    let mut app = fresh_drawing();
    let errors = app.command_line.error_revision;
    app.control.pending = Some(Operation {
        id: "embed-panics".to_string(),
        request: json!({"op":"embed_image"}),
        pending: 1,
        error_revision: errors,
        document_id: None,
        origin_document: app.tabs[app.active_tab].id,
        geometry_revision: 0,
        result: json!({}),
        cancelled: false,
        failure: None,
    });
    let task = app.control_embed_image_with(
        json!({"op":"embed_image","path":"panics.png"}),
        "panics.png".to_string(),
        |_| panic!("a decoder bug"),
    );
    app.drive_headless_task(task).expect("the operation's task");
    app.control.pending.as_mut().unwrap().pending = 0;
    app.control_settle();
    let settled = settled(&app, "embed-panics");
    assert_eq!(settled["ok"], false, "{settled}");
    assert_eq!(
        settled["error"], "embed_image failed: a decoder bug",
        "{settled}"
    );
}
