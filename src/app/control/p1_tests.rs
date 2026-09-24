//! Integration tests for the P1/P2 automation operations — template flows,
//! close, sysvar, layouts/page setups, cross-document copy, groups,
//! selection sets, per-page plotting, where filters and hatch readout.

#![cfg(test)]

use crate::app::OpenCADStudio;
use serde_json::{json, Value};

/// Envelope mutation against the active document: fills `{doc}` with the
/// document_id read from state, like every external client must.
fn mutate(app: &mut OpenCADStudio, request: &str) -> Value {
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    app.automation_op(&request.replacen("{doc}", &doc.to_string(), 1))
}

fn count_type(app: &mut OpenCADStudio, kind: &str) -> u64 {
    let q = app.automation_op(&format!(
        r#"{{"op":"query","type":"{kind}","detail":"summary"}}"#
    ));
    q["count"].as_u64().unwrap()
}

#[test]
fn query_where_filters_on_entity_properties() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 0,0 1"}"#);
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 20,0 5"}"#);
    // radius > 2 keeps only the big circle (the filter runs over
    // RFC 6901 property paths, same operators as records).
    let q = app.automation_op(
        r#"{"op":"query","type":"Circle","detail":"geometry","where":[{"path":"/radius","op":"gt","value":2}]}"#,
    );
    assert_eq!(q["ok"], true, "{}", q["error"]);
    assert_eq!(q["count"], 1);
    assert_eq!(q["entities"][0]["radius"], 5.0);

    // An invalid pointer is refused up front, as is an unknown operator.
    let q = app.automation_op(
        r#"{"op":"query","type":"Circle","where":[{"path":"radius","op":"eq","value":1}]}"#,
    );
    assert_eq!(q["ok"], false);
    let q = app.automation_op(
        r#"{"op":"query","type":"Circle","where":[{"path":"/radius","op":"between","value":1}]}"#,
    );
    assert_eq!(q["ok"], false);
}

#[test]
fn close_op_guards_dirty_documents_and_discards() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 1,1"}"#);

    // Dirty without discard: refused.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"close","request_id":"cl0","document_id":{doc}}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "document_dirty");

    // Discard closes the tab (single tab → replaced by a fresh drawing).
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"close","request_id":"cl1","document_id":{doc},"discard":true}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(r["result"]["closed"], true);
    assert_eq!(app.tabs[app.active_tab].scene.document.entities().count(), 0);

    // Unknown document id: document_closed.
    let r = app.automation_op(
        r#"{"protocol":1,"op":"close","request_id":"cl2","document_id":9999}"#,
    );
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "document_closed");
}

#[test]
fn sysvar_round_trips_and_refuses_unknowns() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"sv1","document_id":{doc},"set":{{"ltscale":2.5,"mirrtext":0}}}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let header = &app.tabs[app.active_tab].scene.document.header;
    assert!((header.linetype_scale - 2.5).abs() < 1e-9);
    assert!(!header.mirror_text);

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"sv2","document_id":{doc},"get":["ltscale","mirrtext"]}}"#
    ));
    assert_eq!(r["result"]["values"]["ltscale"], 2.5);
    assert_eq!(r["result"]["values"]["mirrtext"], 0);

    // clayer only accepts an existing layer.
    mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"sv2b","document_id":{doc},"entities":[{"type":"Point","location":[0,0],"layer":"Walls"}]}"#,
    );
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"sv3","document_id":{doc},"set":{{"clayer":"Walls"}}}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(
        app.tabs[app.active_tab].scene.document.header.current_layer_name,
        "Walls"
    );
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"sv4","document_id":{doc},"set":{{"clayer":"NOPE"}}}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "invalid_sysvar_value");

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"sv5","document_id":{doc},"set":{{"not_a_sysvar":1}}}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "unknown_sysvar");
}

#[test]
fn layout_create_and_page_setup_configure_sheets() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"layout_create","request_id":"lc1","document_id":{doc},"name":"PLAN"}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert!(r["result"]["layouts"].as_array().unwrap().iter().any(|n| n == "PLAN"));

    // Duplicate refused.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"layout_create","request_id":"lc2","document_id":{doc},"name":"PLAN"}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "layout_exists");

    // Page setup on the new layout: A4 landscape, fit, centered.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"page_setup_set","request_id":"ps1","document_id":{doc},"layout":"PLAN","paper":"ISO_A4_(210.00_x_297.00_MM)","orientation":"landscape","fit":true,"center":true}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let ps = app.tabs[app.active_tab]
        .scene
        .plot_settings_for("PLAN")
        .expect("stored page setup");
    assert!(ps.paper_size.contains("ISO_A4"));
    assert!(ps.flags.plot_centered);
    assert!(ps.flags.use_standard_scale);

    // Bad scale string refused; unknown layout refused.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"page_setup_set","request_id":"ps2","document_id":{doc},"layout":"PLAN","scale":"nonsense"}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "invalid_scale");
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"page_setup_set","request_id":"ps3","document_id":{doc},"layout":"GHOST"}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "layout_missing");
}

#[test]
fn new_from_template_and_dwt_save_round_trip() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 2"}"#);
    let dwt = std::env::temp_dir().join(format!("ocs_tpl_{}.dwt", std::process::id()));
    let _ = std::fs::remove_file(&dwt);
    let p = dwt.to_string_lossy().replace('\\', "\\\\");
    let saved = app.automation_op(&format!(r#"{{"op":"save","path":"{p}"}}"#));
    assert_eq!(saved["ok"], true, "{}", saved["error"]);
    assert!(dwt.exists(), ".dwt file written");

    // A fresh template-based drawing carries the template's entities and
    // no source path. The read may race the file-system rename on Windows.
    let mut r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"new","request_id":"nt1","template":"{p}"}}"#
    ));
    for attempt in 0..5 {
        if r["ok"] == true {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        r = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"new","request_id":"nt-retry-{attempt}","template":"{p}"}}"#
        ));
    }
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(r["result"]["total"], 1);
    assert!(app.tabs[app.active_tab].current_path.is_none());

    drop(app);
    let sidecar = dwt.with_file_name(format!(
        ".{}.ocs.lock",
        dwt.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(sidecar);
    let _ = std::fs::remove_file(&dwt);
}

#[test]
fn wblock_with_template_base_keeps_template_tables() {
    // The template drawing carries layer TPL; the exported entity set lands
    // in a copy of that template so TPL survives (Catalog §2.1).
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"wt0","document_id":{doc},"entities":[{"type":"Line","start":[0,0],"end":[5,5],"layer":"TPL"}]}"#,
    );
    let tpl = std::env::temp_dir().join(format!("ocs_wbtpl_{}.dwg", std::process::id()));
    let _ = std::fs::remove_file(&tpl);
    let tp = tpl.to_string_lossy().replace('\\', "\\\\");
    assert_eq!(app.automation_op(&format!(r#"{{"op":"save","path":"{tp}"}}"#))["ok"], true);

    // A new drawing holds the one entity to export.
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 1,1 2"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Circle","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    let out = std::env::temp_dir().join(format!("ocs_wb_out_{}.dxf", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let op = out.to_string_lossy().replace('\\', "\\\\");
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"wblock","request_id":"wt1","document_id":{doc},"path":"{op}","template":"{tp}","handles":["{handle}"]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);

    // The export contains the circle AND the template's TPL layer.
    let opened = app.automation_op(&format!(r#"{{"op":"open","path":"{op}"}}"#));
    assert_eq!(opened["ok"], true);
    let layers = app.automation_op(r#"{"op":"layers"}"#);
    assert!(layers["layers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["name"] == "TPL"));
    drop(app);
    for path in [tpl, out] {
        let sidecar = path.with_file_name(format!(
            ".{}.ocs.lock",
            path.file_name().unwrap().to_string_lossy()
        ));
        let _ = std::fs::remove_file(sidecar);
        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn entities_copy_to_another_open_document() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 2"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Circle","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    // A second document with its own layer table.
    app.tabs
        .push(crate::app::document::DocumentTab::new_drawing(99));
    let target = app.tabs[1].id;

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"entities_copy_to","request_id":"cp1","document_id":{target},"handles":["{handle}"]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(r["result"]["count"], 1);

    // Source keeps the original; the target holds the fresh copy.
    assert_eq!(count_type(&mut app, "Circle"), 1);
    app.active_tab = 1;
    assert_eq!(count_type(&mut app, "Circle"), 1);
    app.active_tab = 0;

    // A missing target is refused.
    let r = app.automation_op(
        r#"{"protocol":1,"op":"entities_copy_to","request_id":"cp2","document_id":4242,"handles":["01"]}"#,
    );
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "document_closed");
}

#[test]
fn group_create_and_selection_sets() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 1,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 2"}"#);
    let q = app.automation_op(r#"{"op":"query","detail":"summary"}"#);
    let handles: Vec<String> = q["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| format!("\"{}\"", e["handle"].as_str().unwrap()))
        .collect();
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"group_create","request_id":"g1","document_id":{doc},"name":"FRAME","handles":[{}]}}"#,
        handles.join(",")
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let objects = &app.tabs[app.active_tab].scene.document.objects;
    assert!(objects.values().any(|o| matches!(
        o,
        acadrust::objects::ObjectType::Group(group) if group.name == "FRAME"
    )));

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"selection_set_save","request_id":"s1","document_id":{doc},"name":"frame-set","handles":[{}]}}"#,
        handles.join(",")
    ));
    assert_eq!(r["result"]["count"], 2);

    // Load recalls (and selects by default).
    app.automation_op(r#"{"op":"select","clear":true}"#);
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"selection_set_load","request_id":"s2","document_id":{doc},"name":"frame-set"}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(r["result"]["selected"], 2);

    // Unknown set refused.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"selection_set_load","request_id":"s3","document_id":{doc},"name":"ghost"}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "selection_set_missing");
}

#[test]
fn plot_per_page_writes_one_pdf_per_layout() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 100,80"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    app.automation_op(&format!(
        r#"{{"protocol":1,"op":"layout_create","request_id":"pp0","document_id":{doc},"name":"PLAN"}}"#
    ));
    let out = std::env::temp_dir().join(format!("ocs_pp_{}.pdf", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let op = out.to_string_lossy().replace('\\', "\\\\");
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"plot","request_id":"pp1","document_id":{doc},"path":"{op}","layout":"all","per_page":true}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let files = r["result"]["files"].as_array().unwrap();
    let expected = app.tabs[app.active_tab].scene.layout_names().len();
    assert_eq!(files.len(), expected, "one PDF per layout");
    for file in files {
        let bytes = std::fs::read(file["path"].as_str().unwrap()).expect("per-page pdf");
        assert!(bytes.starts_with(b"%PDF"));
        let _ = std::fs::remove_file(file["path"].as_str().unwrap());
    }
}

#[test]
fn hatch_boundary_is_flattened_in_query() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"h1","document_id":{doc},"entities":[
            {"type":"Hatch","boundary":[[0,0],[10,0],[10,10],[0,10]],"solid":true}
        ]}"#,
    );
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let q = app.automation_op(r#"{"op":"query","type":"Hatch","detail":"full"}"#);
    assert_eq!(q["count"], 1);
    let boundary = q["entities"][0]["boundary"].as_array().expect("boundary");
    assert_eq!(boundary[0].as_array().unwrap().len(), 4);
}

#[test]
fn diagnostic_layer_survives_plain_io_round_trip() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"dg1","document_id":{doc},"entities":[
            {"type":"Point","location":[1,1],"layer":"Walls"}
        ]}"#,
    );
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(r["result"]["layers_created"], json!(["Walls"]));

    // Direct io round trip on a clone of the document.
    let doc = app.tabs[app.active_tab].scene.document.clone();
    let bytes = crate::io::save_to_bytes(&doc, "dxf", acadrust::DxfVersion::AC1032)
        .expect("save to bytes");
    let path = std::env::temp_dir().join(format!("ocs_diag_{}.dxf", std::process::id()));
    std::fs::write(&path, &bytes).unwrap();
    let loaded = crate::io::load_file(&path).expect("load back");
    let _ = std::fs::remove_file(&path);
    let names: Vec<String> = loaded.layers.iter().map(|l| l.name.clone()).collect();
    assert!(
        names.iter().any(|n| n == "Walls"),
        "Walls lost in plain io round trip; layers = {names:?}"
    );
}

#[test]
fn block_delete_and_define_replace_manage_definitions() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 1,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"CIRCLE 5,5 2"}"#);
    let q = app.automation_op(r#"{"op":"query","detail":"summary"}"#);
    let joined = q["entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| format!("\"{}\"", e["handle"].as_str().unwrap()))
        .collect::<Vec<_>>()
        .join(",");
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_define","request_id":"bl0","document_id":{doc},"name":"MARK","base":[0,0,0],"handles":[{joined}]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(count_type(&mut app, "Insert"), 1);

    // Re-import parity (CF-01.3): replace drops the previous definition,
    // its children and its insert inside one undoable step.
    app.automation_op(r#"{"op":"run","cmd":"POINT 9,9"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Point","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_define","request_id":"bl1","document_id":{doc},"name":"MARK","base":[0,0,0],"handles":["{handle}"],"replace":true}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(count_type(&mut app, "Insert"), 1, "one insert after replace");
    assert_eq!(count_type(&mut app, "Line"), 0, "old children erased");
    assert_eq!(count_type(&mut app, "Circle"), 0, "old children erased");

    // Delete removes the insert, the markers, the children and the record.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_delete","request_id":"bl2","document_id":{doc},"name":"MARK"}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert!(r["result"]["erased"].as_u64().unwrap() >= 3, "insert + markers + child");
    assert_eq!(count_type(&mut app, "Insert"), 0);

    // Unknown names and protected layout records are refused.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_delete","request_id":"bl3","document_id":{doc},"name":"GHOST"}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "block_missing");
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_delete","request_id":"bl4","document_id":{doc},"name":"*Model_Space"}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "block_protected");
}

#[test]
fn wblock_normalize_shifts_the_export_to_the_origin() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 100,200 110,210"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = std::env::temp_dir().join(format!("ocs_wbnorm_{}.dxf", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let op = out.to_string_lossy().replace('\\', "\\\\");
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"wblock","request_id":"wn1","document_id":{doc},"path":"{op}","handles":["{handle}"],"normalize":true}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(r["result"]["normalized"], true);
    assert_eq!(app.automation_op(&format!(r#"{{"op":"open","path":"{op}"}}"#))["ok"], true);
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
    assert_eq!(q["entities"][0]["start"], json!([0.0, 0.0, 0.0]));
    assert_eq!(q["entities"][0]["end"], json!([10.0, 10.0, 0.0]));
    let _ = std::fs::remove_file(&out);
}

#[test]
fn file_identity_is_stable_and_survives_a_round_trip() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r1 = mutate(
        &mut app,
        r#"{"protocol":1,"op":"file_identity","request_id":"fi1","document_id":{doc}}"#,
    );
    assert_eq!(r1["ok"], true, "{}", r1["error"]);
    assert_eq!(r1["result"]["created"], true);
    let identity = r1["result"]["identity"].as_str().unwrap().to_owned();
    assert_eq!(identity.len(), 36, "RFC 4122 shape: {identity}");
    assert_eq!(&identity[8..9], "-");

    // Idempotent: the second call returns the same identity.
    let r2 = mutate(
        &mut app,
        r#"{"protocol":1,"op":"file_identity","request_id":"fi2","document_id":{doc}}"#,
    );
    assert_eq!(r2["result"]["created"], false);
    assert_eq!(r2["result"]["identity"], identity.as_str());

    // renew mints a fresh one.
    let r3 = mutate(
        &mut app,
        r#"{"protocol":1,"op":"file_identity","request_id":"fi3","document_id":{doc},"renew":true}"#,
    );
    let renewed = r3["result"]["identity"].as_str().unwrap().to_owned();
    assert_ne!(renewed, identity);

    // The marker survives a save → open round trip.
    let path = std::env::temp_dir().join(format!("ocs_fid_{}.dwg", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let p = path.to_string_lossy().replace('\\', "\\\\");
    assert_eq!(app.automation_op(&format!(r#"{{"op":"save","path":"{p}"}}"#))["ok"], true);
    assert_eq!(app.automation_op(&format!(r#"{{"op":"open","path":"{p}"}}"#))["ok"], true);
    let r4 = mutate(
        &mut app,
        r#"{"protocol":1,"op":"file_identity","request_id":"fi4","document_id":{doc}}"#,
    );
    assert_eq!(r4["result"]["identity"], renewed.as_str());
    drop(app);
    let sidecar = path.with_file_name(format!(
        ".{}.ocs.lock",
        path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(sidecar);
    let _ = std::fs::remove_file(&path);
}

#[test]
fn state_reports_the_hand_seed() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let before = u64::from_str_radix(state["hand_seed"].as_str().unwrap(), 16).unwrap();
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 1,1"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let after = u64::from_str_radix(state["hand_seed"].as_str().unwrap(), 16).unwrap();
    assert!(after > before, "issuing a handle advances the seed");
}
