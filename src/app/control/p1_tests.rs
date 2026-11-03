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
    assert_eq!(
        app.tabs[app.active_tab].scene.document.entities().count(),
        0
    );

    // Unknown document id: document_closed.
    let r =
        app.automation_op(r#"{"protocol":1,"op":"close","request_id":"cl2","document_id":9999}"#);
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
        app.tabs[app.active_tab]
            .scene
            .document
            .header
            .current_layer_name,
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
fn sysvar_reads_the_active_spaces_constraint_status() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"cs0","document_id":{doc},"get":["constraintstatus"]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let status = &r["result"]["values"]["constraintstatus"];
    assert_eq!(status["scope"], "*Model_Space");
    assert_eq!(status["constraint_count"], 0);
    assert_eq!(status["enabled_count"], 0);
    // The solver has not run yet, so the derived figure is not known.
    assert_eq!(status["dof"], Value::Null);

    let i = app.active_tab;
    let line = app.tabs[i]
        .scene
        .document
        .entities()
        .next()
        .expect("one line")
        .common()
        .handle;
    {
        let set = app.tabs[i]
            .scene
            .parametric_constraint_set_mut(ParametricScope::ModelSpace);
        set.add(
            ConstraintKind::Vertical,
            vec![ParametricRef::whole(line)],
            None,
        );
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );
        set.constraints.last_mut().expect("just added").enabled = false;
        set.dof = Some(7);
    }

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"cs1","document_id":{doc},"get":["CONSTRAINTSTATUS"]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let status = &r["result"]["values"]["constraintstatus"];
    assert_eq!(status["constraint_count"], 2);
    // A suppressed constraint still counts as present, but not as live.
    assert_eq!(status["enabled_count"], 1);
    assert_eq!(status["dof"], 7);

    // Computed, so there is nothing to write.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"cs2","document_id":{doc},"set":{{"CONSTRAINTSTATUS":1}}}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "unknown_sysvar");
}

#[test]
fn sysvar_reads_a_block_definitions_constraint_status_while_editing_it() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    let i = app.active_tab;
    let line = app.tabs[i]
        .scene
        .document
        .entities()
        .next()
        .expect("one line")
        .common()
        .handle;
    let defined = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_define","request_id":"csb-def","document_id":{doc},"name":"STATUS_DEF","base":[0,0,0],"handles":["{:X}"]}}"#,
        line.value()
    ));
    assert_eq!(defined["ok"], true, "{defined}");
    let definition = app.tabs[i]
        .scene
        .document
        .block_records
        .get("STATUS_DEF")
        .expect("new definition")
        .handle;
    app.tabs[i].scene.block_edit_block = Some(definition);
    {
        let set = app.tabs[i]
            .scene
            .parametric_constraint_set_mut(ParametricScope::Block(definition));
        set.add(
            ConstraintKind::Fixed,
            vec![ParametricRef::whole(line)],
            None,
        );
        set.dof = Some(2);
    }

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"csb0","document_id":{doc},"get":["constraintstatus"]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let status = &r["result"]["values"]["constraintstatus"];
    assert_eq!(status["scope"], "STATUS_DEF");
    assert_eq!(status["constraint_count"], 1);
    assert_eq!(status["enabled_count"], 1);
    assert_eq!(status["dof"], 2);

    // Leaving the editor reads model space, whose own set is untouched.
    app.tabs[i].scene.block_edit_block = None;
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"csb1","document_id":{doc},"get":["constraintstatus"]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    let status = &r["result"]["values"]["constraintstatus"];
    assert_eq!(status["scope"], "*Model_Space");
    assert_eq!(status["constraint_count"], 0);
}

#[test]
fn sysvar_batches_validate_atomically_and_undo_as_one_step() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"sv-atomic-layer","document_id":{doc},"entities":[{"type":"Point","location":[0,0],"layer":"Walls"}]}"#,
    );
    let i = app.active_tab;
    // Isolate this operation's dirty/history assertions from the layer setup.
    app.tabs[i].dirty = false;
    let history_before = app.tabs[i].history.undo_stack.len();

    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"sv-atomic-ok","document_id":{doc},"set":{{"ltscale":2.5,"clayer":"Walls"}}}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        "Walls"
    );
    assert_eq!(app.tabs[i].scene.document.header.linetype_scale, 2.5);
    assert_eq!(app.tabs[i].active_layer, "Walls");
    assert_eq!(app.tabs[i].layers.current_layer, "Walls");
    assert_eq!(app.ribbon.active_layer, "Walls");
    assert!(app.tabs[i].dirty);
    assert_eq!(app.tabs[i].history.undo_stack.len(), history_before + 1);

    // A bad later value must not leak the valid earlier value or an undo entry.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"sysvar","request_id":"sv-atomic-bad","document_id":{doc},"set":{{"ltscale":9.0,"clayer":"NOPE"}}}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "invalid_sysvar_value");
    assert_eq!(app.tabs[i].scene.document.header.linetype_scale, 2.5);
    assert_eq!(
        app.tabs[i].scene.document.header.current_layer_name,
        "Walls"
    );
    assert_eq!(app.tabs[i].history.undo_stack.len(), history_before + 1);

    // One undo restores both header variables and all current-layer mirrors.
    app.automation_op(r#"{"op":"undo"}"#);
    assert_eq!(app.tabs[i].scene.document.header.linetype_scale, 1.0);
    assert_eq!(app.tabs[i].scene.document.header.current_layer_name, "0");
    assert_eq!(app.tabs[i].active_layer, "0");
    assert_eq!(app.tabs[i].layers.current_layer, "0");
    assert_eq!(app.ribbon.active_layer, "0");
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
    assert!(r["result"]["layouts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n == "PLAN"));

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
    assert_eq!(
        app.automation_op(&format!(r#"{{"op":"save","path":"{tp}"}}"#))["ok"],
        true
    );

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
        codec::objects::ObjectType::Group(group) if group.name == "FRAME"
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
fn entities_create_supports_control_and_fit_point_splines() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let control = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"spline-control","document_id":{doc},"entities":[
            {"type":"Spline","degree":2,"knots":[0,0,0,1,1,1],"control_points":[[0,0,0],[5,5,0],[10,0,0]],"weights":[1,2,1],"layer":"CURVES"}
        ]}"#,
    );
    assert_eq!(control["ok"], true, "{control}");
    let fit = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"spline-fit","document_id":{doc},"entities":[
            {"type":"Spline","degree":3,"fit_points":[[20,0],[25,5],[30,0]]}
        ]}"#,
    );
    assert_eq!(fit["ok"], true, "{fit}");
    let query = app.automation_op(r#"{"op":"query","type":"Spline","detail":"geometry"}"#);
    assert_eq!(query["count"], 2, "{query}");
    let control_entity = query["entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entity| entity["layer"] == "CURVES")
        .unwrap();
    assert_eq!(control_entity["degree"], 2, "{control_entity}");
    assert_eq!(
        control_entity["control_points"].as_array().unwrap().len(),
        3
    );
    assert_eq!(control_entity["weights"], json!([1.0, 2.0, 1.0]));
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
    let bytes =
        crate::io::save_to_bytes(&doc, "dxf", codec::DxfVersion::AC1032).expect("save to bytes");
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
    assert_eq!(
        count_type(&mut app, "Insert"),
        1,
        "one insert after replace"
    );
    assert_eq!(count_type(&mut app, "Line"), 0, "old children erased");
    assert_eq!(count_type(&mut app, "Circle"), 0, "old children erased");

    // A definition the insert still points at is refused; "force":true erases
    // the referencing inserts in the same undo step.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_delete","request_id":"bl1b","document_id":{doc},"name":"MARK"}}"#
    ));
    assert_eq!(r["ok"], false);
    assert_eq!(r["code"], "block_referenced");
    assert_eq!(
        count_type(&mut app, "Insert"),
        1,
        "the insert survived the refusal"
    );

    // Delete removes the insert, the markers, the children and the record.
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"block_delete","request_id":"bl2","document_id":{doc},"name":"MARK","force":true}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);
    assert_eq!(
        r["result"]["references"].as_u64().unwrap(),
        1,
        "the forced delete reports what it overrode"
    );
    assert!(
        r["result"]["erased"].as_u64().unwrap() >= 3,
        "insert + markers + child"
    );
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
    assert_eq!(
        app.automation_op(&format!(r#"{{"op":"open","path":"{op}"}}"#))["ok"],
        true
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"full"}"#);
    assert_eq!(q["entities"][0]["start"], json!([0.0, 0.0, 0.0]));
    assert_eq!(q["entities"][0]["end"], json!([10.0, 10.0, 0.0]));
    let header = &app.tabs[app.active_tab].scene.document.header;
    assert_eq!(
        header.model_space_extents_min,
        codec::types::Vector3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        header.model_space_extents_max,
        codec::types::Vector3::new(10.0, 10.0, 0.0)
    );
    let _ = std::fs::remove_file(&out);
}

#[test]
fn wblock_export_carries_the_saved_header_values() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,20"}"#);
    let state = app.automation_op(r#"{"protocol":1,"op":"state"}"#);
    let doc = state["document_id"].as_u64().unwrap();
    let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = std::env::temp_dir().join(format!("ocs_wbhdr_{}.dxf", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let op = out.to_string_lossy().replace('\\', "\\\\");
    let r = app.automation_op(&format!(
        r#"{{"protocol":1,"op":"wblock","request_id":"wh1","document_id":{doc},"path":"{op}","handles":["{handle}"]}}"#
    ));
    assert_eq!(r["ok"], true, "{}", r["error"]);

    assert_eq!(
        app.automation_op(&format!(r#"{{"op":"open","path":"{op}"}}"#))["ok"],
        true
    );
    let header = &app.tabs[app.active_tab].scene.document.header;
    assert_eq!(header.measurement, 1);
    assert_eq!(header.insertion_units, 6);
    assert!(header.create_date_julian > 0.0, "creation stamp");
    assert!(header.update_date_julian > 0.0, "update stamp");
    assert_eq!(
        header.model_space_extents_min,
        codec::types::Vector3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        header.model_space_extents_max,
        codec::types::Vector3::new(10.0, 20.0, 0.0)
    );
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
    assert_eq!(
        app.automation_op(&format!(r#"{{"op":"save","path":"{p}"}}"#))["ok"],
        true
    );
    assert_eq!(
        app.automation_op(&format!(r#"{{"op":"open","path":"{p}"}}"#))["ok"],
        true
    );
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

#[test]
fn text_search_op_queries_text_and_attributes() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"ts_c1","document_id":{doc},"entities":[
            {"type":"Text","position":[0,0,0],"value":"CIRTUITS 28-09-2026","layer":"0"},
            {"type":"MText","position":[10,0,0],"value":"{\\fCentury Gothic;POMPE A EAU}","layer":"0"},
            {"type":"Text","position":[20,0,0],"value":"BAROMETRE","layer":"0"},
            {"type":"Text","position":[30,0,0],"value":"10 BAR","layer":"0"}
        ]}"#,
    );
    assert_eq!(r["ok"], true, "{}", r["error"]);

    // Case-insensitive search finds CIRTUITS
    let res = app.automation_op(r#"{"op":"text_search","find":"cirtuits"}"#);
    assert_eq!(res["ok"], true);
    assert_eq!(res["count"], 1);
    assert_eq!(res["matches"][0]["match_text"], "cirtuits");
    assert_eq!(res["matches"][0]["plain_text"], "CIRTUITS 28-09-2026");

    // Case-sensitive search with wrong case finds nothing
    let res_cs = app.automation_op(r#"{"op":"text_search","find":"cirtuits","match_case":true}"#);
    assert_eq!(res_cs["ok"], true);
    assert_eq!(res_cs["count"], 0);

    // Whole-word search: "bar" matches "10 BAR" but not "BAROMETRE"
    let res_ww = app.automation_op(r#"{"op":"text_search","find":"bar","whole_word":true}"#);
    assert_eq!(res_ww["ok"], true);
    assert_eq!(res_ww["count"], 1);
    assert_eq!(res_ww["matches"][0]["plain_text"], "10 BAR");

    // Non-whole-word search finds both "BAROMETRE" and "10 BAR"
    let res_all = app.automation_op(r#"{"op":"text_search","find":"bar","whole_word":false}"#);
    assert_eq!(res_all["ok"], true);
    assert_eq!(res_all["count"], 2);
}

#[test]
fn text_replace_op_batch_replaces_preserves_formatting_and_undoes() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"tr_c1","document_id":{doc},"entities":[
            {"type":"Text","position":[0,0,0],"value":"VANNE A ARRET","layer":"0"},
            {"type":"MText","position":[10,0,0],"value":"{\\fCentury Gothic|b0|i0;V15-CUVE EAU NON TRAITE}","layer":"0"}
        ]}"#,
    );
    assert_eq!(r["ok"], true, "{}", r["error"]);

    // Execute batch replace with 2 pairs in one atomic step
    let rep = mutate(
        &mut app,
        r#"{"protocol":1,"op":"text_replace","request_id":"tr_r1","document_id":{doc},"pairs":[
            {"find":"NON TRAITE","replace":"NON TRAITÉE"},
            {"find":"VANNE A ARRET","replace":"VANNE D'ARRÊT"}
        ]}"#,
    );
    assert_eq!(rep["ok"], true, "{}", rep["error"]);
    assert_eq!(rep["result"]["replaced"], 2);
    assert_eq!(rep["result"]["entities_changed"], 2);

    // Verify MText formatting codes were preserved
    let q = app.automation_op(r#"{"op":"text_search","find":"NON TRAITÉE"}"#);
    assert_eq!(q["ok"], true);
    assert_eq!(q["count"], 1);
    let raw = q["matches"][0]["raw_value"].as_str().unwrap();
    assert!(
        raw.contains("Century Gothic"),
        "raw MText should keep font format code: {raw}"
    );
    assert!(
        raw.contains("NON TRAITÉE"),
        "raw MText should contain replaced text: {raw}"
    );

    // Verify Text was replaced
    let q2 = app.automation_op(r#"{"op":"text_search","find":"VANNE D'ARRÊT"}"#);
    assert_eq!(q2["ok"], true);
    assert_eq!(q2["count"], 1);

    // Undo restores the original text in one step
    app.automation_op(r#"{"op":"undo"}"#);
    let q_orig = app.automation_op(r#"{"op":"text_search","find":"NON TRAITE"}"#);
    assert_eq!(q_orig["ok"], true);
    assert_eq!(q_orig["count"], 1);

    let q_orig2 = app.automation_op(r#"{"op":"text_search","find":"VANNE A ARRET"}"#);
    assert_eq!(q_orig2["ok"], true);
    assert_eq!(q_orig2["count"], 1);
}

#[test]
fn text_audit_op_simulates_dry_run_checks_terms_and_dictionary() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"ta_c1","document_id":{doc},"entities":[
            {"type":"Text","position":[0,0,0],"value":"CIRTUITS 28-09-2026","layer":"0"},
            {"type":"MText","position":[10,0,0],"value":"{\\fCentury Gothic;CUVE EAU NON TRAITE}","layer":"0"},
            {"type":"Text","position":[20,0,0],"value":"POMPE A EAU","layer":"0"}
        ]}"#,
    );
    assert_eq!(r["ok"], true, "{}", r["error"]);

    // Test text_audit with pairs, check_terms, and dictionary
    let audit_res = app.automation_op(
        r#"{
            "op":"text_audit",
            "pairs":[
                {"find":"CIRTUITS","replace":"CIRCUITS"},
                {"find":"NON TRAITE","replace":"NON TRAITÉE"},
                {"find":"INEXISTANT_PAIR","replace":"REPLACEMENT"}
            ],
            "check_terms":["CIRTUITS", "NON TRAITE", "INEXISTANT_TERM"],
            "dictionary":["EAU", "POMPE", "28-09-2026"]
        }"#,
    );
    assert_eq!(audit_res["ok"], true, "{}", audit_res["error"]);

    // Check summary
    assert_eq!(audit_res["summary"]["pairs_total"], 3);
    assert_eq!(audit_res["summary"]["pairs_unmatched"], 1);
    assert_eq!(audit_res["summary"]["check_terms_total"], 3);
    assert_eq!(audit_res["summary"]["check_terms_found"], 2);

    // Unmatched reporting
    assert_eq!(audit_res["unmatched_pairs"], json!(["INEXISTANT_PAIR"]));
    assert_eq!(
        audit_res["unmatched_check_terms"],
        json!(["INEXISTANT_TERM"])
    );

    // Simulated changes has 2 entries (Text and MText)
    let changes = audit_res["simulated_changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["find"], "CIRTUITS");
    assert_eq!(changes[0]["replace"], "CIRCUITS");
    assert_eq!(changes[0]["replaced"], 1);
    assert!(changes[1]["after"]
        .as_str()
        .unwrap()
        .contains("NON TRAITÉE"));

    // Suspect matches
    let suspects = audit_res["suspect_matches"].as_array().unwrap();
    assert_eq!(suspects.len(), 2);

    // Verify document was NOT mutated (pure read op)
    let q = app.automation_op(r#"{"op":"text_search","find":"CIRTUITS"}"#);
    assert_eq!(q["ok"], true);
    assert_eq!(q["count"], 1);
}

#[test]
fn text_replace_op_supports_dry_run_without_mutating_or_undo() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"tr_dry1","document_id":{doc},"entities":[
            {"type":"Text","position":[0,0,0],"value":"TEST BEFORE REPLACE","layer":"0"}
        ]}"#,
    );
    assert_eq!(r["ok"], true, "{}", r["error"]);

    // Run text_replace with dry_run = true
    let dry_res = mutate(
        &mut app,
        r#"{"protocol":1,"op":"text_replace","request_id":"tr_sim1","document_id":{doc},"find":"BEFORE","replace":"AFTER","dry_run":true}"#,
    );
    assert_eq!(dry_res["ok"], true, "{}", dry_res["error"]);
    assert_eq!(dry_res["result"]["dry_run"], true);
    assert_eq!(dry_res["result"]["replaced"], 1);
    assert_eq!(dry_res["result"]["entities_changed"], 1);

    // Verify the document was NOT modified
    let q_before = app.automation_op(r#"{"op":"text_search","find":"TEST BEFORE REPLACE"}"#);
    assert_eq!(q_before["ok"], true);
    assert_eq!(q_before["count"], 1);

    let q_after = app.automation_op(r#"{"op":"text_search","find":"TEST AFTER REPLACE"}"#);
    assert_eq!(q_after["ok"], true);
    assert_eq!(q_after["count"], 0);

    // Now run with dry_run = false
    let real_res = mutate(
        &mut app,
        r#"{"protocol":1,"op":"text_replace","request_id":"tr_real1","document_id":{doc},"find":"BEFORE","replace":"AFTER","dry_run":false}"#,
    );
    assert_eq!(real_res["ok"], true, "{}", real_res["error"]);
    assert_eq!(real_res["result"]["replaced"], 1);

    // Now it was modified
    let q_after2 = app.automation_op(r#"{"op":"text_search","find":"TEST AFTER REPLACE"}"#);
    assert_eq!(q_after2["ok"], true);
    assert_eq!(q_after2["count"], 1);

    // Undo restores it
    app.automation_op(r#"{"op":"undo"}"#);
    let q_undone = app.automation_op(r#"{"op":"text_search","find":"TEST BEFORE REPLACE"}"#);
    assert_eq!(q_undone["ok"], true);
    assert_eq!(q_undone["count"], 1);
}

#[test]
fn text_audit_system_spellcheck_flags_misspelled_words() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let r = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"sp_c1","document_id":{doc},"entities":[
            {"type":"Text","position":[0,0,0],"value":"The cirtuit breaker is conected","layer":"0"},
            {"type":"Text","position":[10,0,0],"value":"correct english text here","layer":"0"}
        ]}"#,
    );
    assert_eq!(r["ok"], true, "{}", r["error"]);

    // Run text_audit with system_spellcheck enabled, English language,
    // and an agent dictionary that whitelists "breaker"
    let audit = app.automation_op(
        r#"{
            "op":"text_audit",
            "system_spellcheck": true,
            "language": "en-US",
            "suggest": true,
            "dictionary": ["breaker", "the"]
        }"#,
    );
    assert_eq!(audit["ok"], true, "{}", audit["error"]);

    // system_speller metadata should be present regardless of availability
    let sp = &audit["system_speller"];
    assert_eq!(sp["enabled"], true);

    if sp["available"].as_bool() == Some(true) {
        // Backend should be reported (e.g. "windows" on Windows)
        assert!(sp["backend"].as_str().unwrap().len() > 0);
        assert!(sp["language"].as_str().is_some());

        // "cirtuit" and "conected" should be flagged as unrecognized
        let words = audit["unrecognized_words"].as_array().unwrap();
        let flagged: Vec<&str> = words.iter().filter_map(|w| w["word"].as_str()).collect();
        assert!(
            flagged.iter().any(|w| w.eq_ignore_ascii_case("cirtuit")),
            "expected 'cirtuit' to be flagged, got: {:?}",
            flagged
        );
        assert!(
            flagged.iter().any(|w| w.eq_ignore_ascii_case("conected")),
            "expected 'conected' to be flagged, got: {:?}",
            flagged
        );

        // "breaker" should NOT be flagged (it's in the agent dictionary)
        assert!(
            !flagged.iter().any(|w| w.eq_ignore_ascii_case("breaker")),
            "expected 'breaker' to be whitelisted by agent dict, got: {:?}",
            flagged
        );

        // At least one flagged word should have suggestions — except from a
        // bare Linux wordlist, which can tell a word is unknown but has no
        // way to suggest one without the hunspell CLI.
        let has_suggestions = words.iter().any(|w| {
            w["suggestions"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false)
        });
        assert!(
            has_suggestions || sp["backend"] == "linux_system",
            "expected at least one word to have suggestions"
        );

        // Summary should reflect unrecognized words
        assert!(
            audit["summary"]["unrecognized_words_count"]
                .as_u64()
                .unwrap()
                >= 2,
            "expected at least 2 unrecognized words in summary"
        );
    } else {
        // System speller not available (e.g. CI without spell-check service)
        // Just verify the response structure is well-formed
        assert!(audit["unrecognized_words"].is_array());
        eprintln!(
            "System spellcheck not available (backend: {}), skipping content assertions",
            sp["backend"]
        );
    }
}

/// The protocol envelope a bridge supplies, for the operations the
/// automation surface does not carry (a grip drag lives in the control
/// surface only).
fn control(app: &mut OpenCADStudio, mut op: Value) -> Value {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT_ID: AtomicUsize = AtomicUsize::new(0);
    let state = app.control_state();
    op["protocol"] = json!(1);
    op["document_id"] = state["document_id"].clone();
    op["revision"] = state["revision"].clone();
    op["client_id"] = json!("test");
    op["request_id"] = json!(format!("p1-{}", NEXT_ID.fetch_add(1, Ordering::Relaxed)));
    let (response, task) = app.control_request(op);
    let _ = app.drive_headless_task(task);
    response
}

/// A line end from the `geometry` query, as (x, y).
fn line_point(entity: &Value, key: &str) -> (f64, f64) {
    let p = entity[key].as_array().expect("the line reports both ends");
    (p[0].as_f64().expect("x"), p[1].as_f64().expect("y"))
}

/// Whether a point is within solver tolerance of where it was drawn.
fn stays(p: (f64, f64), want: (f64, f64)) -> bool {
    (p.0 - want.0).abs() < 1e-9 && (p.1 - want.1).abs() < 1e-9
}

/// A fixed entity must not move under a grip drag: the constraint that
/// reports itself applied has to hold the geometry where it was drawn.
#[test]
fn a_fixed_entity_resists_a_grip_drag() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 800.0);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    let fixed = app.automation_op(r#"{"op":"run","cmd":"GCFIX"}"#);
    assert_eq!(fixed["ok"], json!(true), "{fixed}");

    let dragged = control(
        &mut app,
        json!({"op":"grip_drag","handle":handle,"grip_id":1,"point":[10,5,0]}),
    );
    assert_eq!(dragged["ok"], json!(true), "{dragged}");
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let entity = &q["entities"][0];
    assert!(
        stays(line_point(entity, "start"), (0.0, 0.0))
            && stays(line_point(entity, "end"), (10.0, 0.0)),
        "a fixed line stays where it was drawn: {q}"
    );
}

/// A command transform must leave a fixed entity where it was, and say why.
#[test]
fn a_fixed_entity_resists_a_move() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    let fixed = app.automation_op(r#"{"op":"run","cmd":"GCFIX"}"#);
    assert_eq!(fixed["ok"], json!(true), "{fixed}");

    let moved = app.automation_op(r#"{"op":"run","cmd":"MOVE 0,0,0 5,5,0"}"#);
    assert_eq!(moved["ok"], json!(false), "{moved}");
    assert!(
        moved["error"]
            .as_str()
            .unwrap_or_default()
            .contains("fixed constraint"),
        "the refusal names the constraint: {moved}"
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let entity = &q["entities"][0];
    assert!(
        stays(line_point(entity, "start"), (0.0, 0.0))
            && stays(line_point(entity, "end"), (10.0, 0.0)),
        "a fixed line stays put: {q}"
    );

    // The panel edits the current selection, which the command's completion
    // cleared; pick the line again to reach the same refusal through the
    // panel's own commit.
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    app.refresh_properties();
    let before = app.command_line.history.len();
    let _ = app.update(crate::app::Message::PropGeomInput {
        field: "start_x",
        value: "5".to_owned(),
    });
    let _ = app.update(crate::app::Message::PropGeomCommit("start_x"));
    let refusal: Vec<_> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert!(
        refusal.iter().any(|text| text.contains("fixed constraint")),
        "the panel says why it ignored the edit: {refusal:?}"
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let entity = &q["entities"][0];
    assert!(
        stays(line_point(entity, "start"), (0.0, 0.0))
            && stays(line_point(entity, "end"), (10.0, 0.0)),
        "the refused panel edit left the line put: {q}"
    );
}

/// A fixed object holds the whole selection: the free object is not moved
/// alone and the command is reported as refused.
#[test]
fn a_move_holds_the_whole_selection_when_one_object_is_fixed() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 20,0 30,0"}"#);
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#);
    let handles: Vec<String> = q["entities"]
        .as_array()
        .expect("two lines")
        .iter()
        .map(|entity| entity["handle"].as_str().expect("handle").to_owned())
        .collect();
    assert_eq!(handles.len(), 2, "{q}");
    app.automation_op(&format!(
        r#"{{"op":"select","handles":["{}"]}}"#,
        handles[0]
    ));
    let fixed = app.automation_op(r#"{"op":"run","cmd":"GCFIX"}"#);
    assert_eq!(fixed["ok"], json!(true), "{fixed}");

    app.automation_op(&format!(
        r#"{{"op":"select","handles":["{}","{}"]}}"#,
        handles[0], handles[1]
    ));
    let moved = app.automation_op(r#"{"op":"run","cmd":"MOVE 0,0,0 5,5,0"}"#);
    assert_eq!(moved["ok"], json!(false), "{moved}");
    assert!(
        moved["error"]
            .as_str()
            .unwrap_or_default()
            .contains("fixed constraint"),
        "the refusal names the constraint: {moved}"
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let placed = [((0.0, 0.0), (10.0, 0.0)), ((20.0, 0.0), (30.0, 0.0))];
    for (entity, (start, end)) in q["entities"]
        .as_array()
        .expect("two lines")
        .iter()
        .zip(placed)
    {
        assert!(
            stays(line_point(entity, "start"), start) && stays(line_point(entity, "end"), end),
            "neither line left its place: {q}"
        );
    }
}

/// Bounds alignment moves whole objects, so it answers the same refusal.
#[test]
fn a_fixed_entity_resists_bounds_alignment() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 20,0 30,0"}"#);
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#);
    let handles: Vec<String> = q["entities"]
        .as_array()
        .expect("two lines")
        .iter()
        .map(|entity| entity["handle"].as_str().expect("handle").to_owned())
        .collect();
    app.automation_op(&format!(
        r#"{{"op":"select","handles":["{}"]}}"#,
        handles[0]
    ));
    let fixed = app.automation_op(r#"{"op":"run","cmd":"GCFIX"}"#);
    assert_eq!(fixed["ok"], json!(true), "{fixed}");
    app.automation_op(&format!(
        r#"{{"op":"select","handles":["{}","{}"]}}"#,
        handles[0], handles[1]
    ));

    let aligned = app.automation_op(r#"{"op":"run","cmd":"ALIGNLEFT"}"#);
    assert_eq!(aligned["ok"], json!(false), "{aligned}");
    assert!(
        aligned["error"]
            .as_str()
            .unwrap_or_default()
            .contains("fixed constraint"),
        "the refusal names the constraint: {aligned}"
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let placed = [((0.0, 0.0), (10.0, 0.0)), ((20.0, 0.0), (30.0, 0.0))];
    for (entity, (start, end)) in q["entities"]
        .as_array()
        .expect("two lines")
        .iter()
        .zip(placed)
    {
        assert!(
            stays(line_point(entity, "start"), start) && stays(line_point(entity, "end"), end),
            "neither line left its place: {q}"
        );
    }
}

/// The panel's boolean and choice rows that reshape a curve answer the same
/// refusal as its numeric rows.
#[test]
fn a_fixed_curve_resists_a_boolean_and_a_choice() {
    use codec::entities::{Polyline2D, Spline, Vertex2D};
    use codec::types::Vector3;

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let mut polyline = Polyline2D::new();
    polyline
        .vertices
        .push(Vertex2D::new(Vector3::new(0.0, 0.0, 0.0)));
    polyline
        .vertices
        .push(Vertex2D::new(Vector3::new(10.0, 0.0, 0.0)));
    let polyline_handle = app
        .commit_entity_handle(codec::EntityType::Polyline2D(polyline))
        .expect("the polyline commits");
    let mut spline = Spline::new();
    spline.control_points = vec![
        Vector3::new(0.0, 5.0, 0.0),
        Vector3::new(3.0, 8.0, 0.0),
        Vector3::new(6.0, 2.0, 0.0),
        Vector3::new(9.0, 5.0, 0.0),
    ];
    spline.knots = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    spline.weights = vec![1.0; 4];
    let spline_handle = app
        .commit_entity_handle(codec::EntityType::Spline(spline.clone()))
        .expect("the spline commits");
    for handle in [polyline_handle, spline_handle] {
        app.automation_op(&format!(
            r#"{{"op":"select","handles":["{:X}"]}}"#,
            handle.value()
        ));
        let fixed = app.automation_op(r#"{"op":"run","cmd":"GCFIX"}"#);
        assert_eq!(fixed["ok"], json!(true), "{fixed}");
    }

    app.automation_op(&format!(
        r#"{{"op":"select","handles":["{:X}"]}}"#,
        polyline_handle.value()
    ));
    app.refresh_properties();
    let before = app.command_line.history.len();
    let _ = app.update(crate::app::Message::PropBoolToggle("pl2_closed"));
    let said: Vec<_> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert!(
        said.iter().any(|text| text.contains("fixed constraint")),
        "the Closed toggle says why it was ignored: {said:?}"
    );
    match app.tabs[app.active_tab]
        .scene
        .document
        .get_entity(polyline_handle)
    {
        Some(codec::EntityType::Polyline2D(polyline)) => {
            assert!(!polyline.is_closed(), "the fixed polyline stayed open");
        }
        other => panic!("the polyline is still there: {other:?}"),
    }

    app.automation_op(&format!(
        r#"{{"op":"select","handles":["{:X}"]}}"#,
        spline_handle.value()
    ));
    app.refresh_properties();
    let before = app.command_line.history.len();
    let _ = app.update(crate::app::Message::PropGeomChoiceChanged {
        field: "spline_method",
        value: "Control Vertices".to_owned(),
    });
    let said: Vec<_> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert!(
        said.iter().any(|text| text.contains("fixed constraint")),
        "the Method choice says why it was ignored: {said:?}"
    );
    match app.tabs[app.active_tab]
        .scene
        .document
        .get_entity(spline_handle)
    {
        Some(codec::EntityType::Spline(after)) => {
            assert_eq!(
                after.control_points, spline.control_points,
                "the fixed spline kept its points"
            );
        }
        other => panic!("the spline is still there: {other:?}"),
    }
}

/// A property write must not move a fixed entity, and must say why.
#[test]
fn a_fixed_entity_resists_a_property_write() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    let fixed = app.automation_op(r#"{"op":"run","cmd":"GCFIX"}"#);
    assert_eq!(fixed["ok"], json!(true), "{fixed}");

    let written = control(
        &mut app,
        json!({"op":"property","field":"start_x","value":5.0}),
    );
    assert_eq!(written["ok"], json!(false), "{written}");
    assert!(
        written["error"]
            .as_str()
            .unwrap_or_default()
            .contains("fixed constraint"),
        "the refusal names the constraint: {written}"
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let entity = &q["entities"][0];
    assert!(
        stays(line_point(entity, "start"), (0.0, 0.0))
            && stays(line_point(entity, "end"), (10.0, 0.0)),
        "a fixed line stays put: {q}"
    );
}

/// A stretch that would move a fixed object is refused with the reason, and
/// the object keeps the coordinates it had.
#[test]
fn a_fixed_entity_resists_a_stretch() {
    use crate::command::CmdResult;

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
    let printed = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#);
    let handle = printed["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    app.automation_op(&format!(r#"{{"op":"select","handles":["{handle}"]}}"#));
    let fixed = app.automation_op(r#"{"op":"run","cmd":"GCFIX"}"#);
    assert_eq!(fixed["ok"], json!(true), "{fixed}");

    let before = app.command_line.history.len();
    let task = app.apply_cmd_result(CmdResult::StretchEntities {
        handles: vec![codec::Handle::new(
            u64::from_str_radix(&handle, 16).unwrap(),
        )],
        windows: vec![(
            glam::DVec3::new(-1.0, -1.0, 0.0),
            glam::DVec3::new(11.0, 1.0, 0.0),
        )],
        excluded_points: Vec::new(),
        delta: glam::DVec3::new(0.0, 5.0, 0.0),
    });
    app.drive_headless_task(task).unwrap();
    let said: Vec<_> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.as_str())
        .collect();
    assert!(
        said.iter().any(|text| text.contains("fixed constraint")),
        "the stretch says why it moved nothing: {said:?}"
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let entity = &q["entities"][0];
    assert!(
        stays(line_point(entity, "start"), (0.0, 0.0))
            && stays(line_point(entity, "end"), (10.0, 0.0)),
        "a fixed line stays put through a stretch: {q}"
    );
}

/// A constraint command solves the whole network, so a command can move
/// geometry it never named. The completion says so and names the entity,
/// instead of leaving the operator to find the movement on the drawing.
#[test]
fn a_constraint_command_names_geometry_the_network_moves() {
    use crate::command::CmdResult;
    use crate::scene::named_parameters::DrivingValue;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 200,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,50 10,50"}"#);
    let drawn = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let mut long = None;
    let mut short = None;
    for entity in drawn["entities"].as_array().expect("the two lines") {
        let handle = codec::Handle::new(
            u64::from_str_radix(entity["handle"].as_str().unwrap(), 16).unwrap(),
        );
        if line_point(entity, "end").0 > 100.0 {
            long = Some(handle);
        } else {
            short = Some(handle);
        }
    }
    let long = long.expect("the 200-long line");
    let short = short.expect("the other line");
    // A span that no longer matches its line: the next solve has work to do,
    // and the line is not a participant of the constraint being added.
    app.tabs[app.active_tab]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::DistanceX,
            vec![ParametricRef::point(long, 0), ParametricRef::point(long, 1)],
            Some(DrivingValue::Literal(100.0)),
        );

    let before = app.command_line.history.len();
    let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
        kind: ConstraintKind::Horizontal,
        refs: vec![ParametricRef::whole(short)],
        driving_param: None,
        label: "Horizontal constraint",
    });
    let said: Vec<String> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect();
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    assert!(
        q["entities"].as_array().unwrap().iter().any(|entity| {
            (line_point(entity, "end").0 - line_point(entity, "start").0 - 100.0).abs() < 1e-6
        }),
        "the network corrected the line nobody named: {q}"
    );
    assert!(
        said.contains(&format!(
            "The constraint network moved 1 entity the command did not name: {long}."
        )),
        "the completion names the line the network moved, and only it: {said:?}"
    );
}

/// The dedicated Horizontal command solves the same network, so it owes the
/// operator the same warning when the solve lands on geometry it never named.
#[test]
fn a_horizontal_command_names_geometry_the_network_moves() {
    use crate::command::{CmdResult, HorizontalConstraintSelection};
    use crate::scene::named_parameters::DrivingValue;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 200,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,50 30,60"}"#);
    let drawn = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let mut long = None;
    let mut slanted = None;
    for entity in drawn["entities"].as_array().expect("the two lines") {
        let handle = codec::Handle::new(
            u64::from_str_radix(entity["handle"].as_str().unwrap(), 16).unwrap(),
        );
        if line_point(entity, "end").0 > 100.0 {
            long = Some(handle);
        } else {
            slanted = Some(handle);
        }
    }
    let long = long.expect("the 200-long line");
    let slanted = slanted.expect("the slanted line");
    app.tabs[app.active_tab]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::DistanceX,
            vec![ParametricRef::point(long, 0), ParametricRef::point(long, 1)],
            Some(DrivingValue::Literal(100.0)),
        );

    let before = app.command_line.history.len();
    let _ = app.apply_cmd_result(CmdResult::AddHorizontalConstraint {
        kind: ConstraintKind::Horizontal,
        selection: HorizontalConstraintSelection::Reference(ParametricRef::whole(slanted)),
        direction: codec::types::Vector3::new(1.0, 0.0, 0.0),
        label: "Horizontal",
    });
    let said: Vec<String> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect();
    assert!(
        said.contains(&"Horizontal constraint applied.".to_string()),
        "the command completes: {said:?}"
    );
    assert!(
        said.contains(&format!(
            "The constraint network moved 1 entity the command did not name: {long}."
        )),
        "the dedicated command reports the line the network moved: {said:?}"
    );
}

/// A command that runs the solve for its own edit stays quiet about the
/// geometry that solve moves; the report belongs to constraint additions.
#[test]
fn a_move_command_does_not_report_geometry_the_solve_moves() {
    use crate::scene::named_parameters::DrivingValue;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 200,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,50 30,60"}"#);
    let drawn = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let mut long = None;
    let mut slanted = None;
    for entity in drawn["entities"].as_array().expect("the two lines") {
        let handle = codec::Handle::new(
            u64::from_str_radix(entity["handle"].as_str().unwrap(), 16).unwrap(),
        );
        if line_point(entity, "end").0 > 100.0 {
            long = Some(handle);
        } else {
            slanted = Some(handle);
        }
    }
    let long = long.expect("the 200-long line");
    let slanted = slanted.expect("the slanted line");
    let set = app.tabs[app.active_tab]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::DistanceX,
        vec![ParametricRef::point(long, 0), ParametricRef::point(long, 1)],
        Some(DrivingValue::Literal(100.0)),
    );
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(slanted)],
        None,
    );

    app.automation_op(&format!(r#"{{"op":"select","handles":["{long:X}"]}}"#));
    let before = app.command_line.history.len();
    let moved = app.automation_op(r#"{"op":"run","cmd":"MOVE 0,0,0 0,10,0"}"#);
    assert_eq!(moved["ok"], json!(true), "{moved}");
    let said: Vec<String> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect();
    assert!(
        !said
            .iter()
            .any(|text| text.contains("constraint network moved")),
        "a move command does not report the network's work: {said:?}"
    );
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let levelled = q["entities"].as_array().unwrap().iter().any(|entity| {
        let start = line_point(entity, "start");
        let end = line_point(entity, "end");
        (start.1 - end.1).abs() < 1e-6 && (start.0 - end.0).abs() < 50.0
    });
    assert!(
        levelled,
        "the solve ran and levelled the slanted line the command never named: {q}"
    );
}

/// A converted dimensional constraint is enforced by the drag that moves its
/// participant, not by some later constraint command that happens to walk the
/// network. The span is back at its constrained value before the drag ends;
/// which end the solver holds is the separate datum question.
#[test]
fn a_converted_constraint_clamps_the_grip_that_moves_it() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 800.0);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 40,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    let line = codec::Handle::new(u64::from_str_radix(&handle, 16).unwrap());
    let dimension = app.tabs[i].scene.add_entity(
        crate::modules::annotate::linear_dim::linear_dimension_entity(
            glam::DVec3::ZERO,
            glam::DVec3::new(40.0, 0.0, 0.0),
            glam::DVec3::new(20.0, 10.0, 0.0),
            glam::DVec3::X,
            None,
        ),
    );
    app.tabs[i]
        .scene
        .attach_dimension_association(dimension, vec![Some(line), Some(line)]);
    app.tabs[i].scene.select_entity(dimension, false);
    let _ = app.dispatch_command("DCCONVERTAPPLY");
    assert!(
        app.tabs[i]
            .scene
            .parametric_constraint_set(ParametricScope::ModelSpace)
            .is_some_and(|set| set
                .constraints
                .iter()
                .any(|constraint| constraint.kind == ConstraintKind::DistanceX)),
        "the dimension converts to a driving horizontal span"
    );

    let dragged = control(
        &mut app,
        json!({"op":"grip_drag","handle":handle,"grip_id":1,"point":[80,0,0]}),
    );
    assert_eq!(dragged["ok"], json!(true), "{dragged}");
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let entity = &q["entities"][0];
    let (start, end) = (line_point(entity, "start"), line_point(entity, "end"));
    assert!(
        (end.0 - start.0 - 40.0).abs() < 1e-6,
        "the converted constraint holds the measured span through the drag itself: {q}"
    );
}

/// The same constraint holds when an axis relation owns the line's direction:
/// the drag cannot stretch the span the dimension owns either.
#[test]
fn a_converted_constraint_clamps_the_grip_that_moves_a_level_line() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    app.tabs[i].scene.selection.borrow_mut().view.vp_size = (1600.0, 800.0);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 40,0"}"#);
    let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
        ["entities"][0]["handle"]
        .as_str()
        .expect("the drawn line has a handle")
        .to_owned();
    let line = codec::Handle::new(u64::from_str_radix(&handle, 16).unwrap());
    app.tabs[i]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(line)],
            None,
        );
    let dimension = app.tabs[i].scene.add_entity(
        crate::modules::annotate::linear_dim::linear_dimension_entity(
            glam::DVec3::ZERO,
            glam::DVec3::new(40.0, 0.0, 0.0),
            glam::DVec3::new(20.0, 10.0, 0.0),
            glam::DVec3::X,
            None,
        ),
    );
    app.tabs[i]
        .scene
        .attach_dimension_association(dimension, vec![Some(line), Some(line)]);
    app.tabs[i].scene.select_entity(dimension, false);
    let _ = app.dispatch_command("DCCONVERTAPPLY");

    let dragged = control(
        &mut app,
        json!({"op":"grip_drag","handle":handle,"grip_id":1,"point":[80,10,0]}),
    );
    assert_eq!(dragged["ok"], json!(true), "{dragged}");
    let q = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let entity = &q["entities"][0];
    let (start, end) = (line_point(entity, "start"), line_point(entity, "end"));
    assert!(
        (end.0 - start.0 - 40.0).abs() < 1e-6,
        "the converted constraint holds the span the drag cannot stretch: {q}"
    );
    assert!(
        (end.1 - start.1).abs() < 1e-6,
        "the level relation still holds after the drag: {q}"
    );
}

/// Auto-constraining a selection adds constraints the same way, so its
/// completion reports the geometry its solve moves outside the selection.
#[test]
fn auto_coincident_names_geometry_the_network_moves() {
    use crate::command::CmdResult;
    use crate::scene::named_parameters::DrivingValue;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 200,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 0,80"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,200 30,210"}"#);
    let drawn = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let mut long = None;
    let mut upright = None;
    let mut slanted = None;
    for entity in drawn["entities"].as_array().expect("the three lines") {
        let handle = codec::Handle::new(
            u64::from_str_radix(entity["handle"].as_str().unwrap(), 16).unwrap(),
        );
        let end = line_point(entity, "end");
        if end.0 > 100.0 {
            long = Some(handle);
        } else if end.0.abs() < 1e-6 {
            upright = Some(handle);
        } else {
            slanted = Some(handle);
        }
    }
    let long = long.expect("the 200-long line");
    let upright = upright.expect("the upright line");
    let slanted = slanted.expect("the slanted line");
    let set = app.tabs[app.active_tab]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::DistanceX,
        vec![ParametricRef::point(long, 0), ParametricRef::point(long, 1)],
        Some(DrivingValue::Literal(100.0)),
    );
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(slanted)],
        None,
    );

    let before = app.command_line.history.len();
    let _ = app.apply_cmd_result(CmdResult::AddAutoCoincidentConstraints {
        handles: vec![long, upright],
    });
    let said: Vec<String> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect();
    assert!(
        said.iter()
            .any(|text| text.contains("coincident constraint(s) applied.")),
        "the command completes: {said:?}"
    );
    assert!(
        said.contains(&format!(
            "The constraint network moved 1 entity the command did not name: {slanted}."
        )),
        "the completion names the line the network moved, and only it: {said:?}"
    );
}

/// The parallel command applies a follow-up result of its own once its second
/// line is placed. That result must not swallow the report the constraint
/// addition just queued.
#[test]
fn a_parallel_command_names_geometry_the_network_moves() {
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 100,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,20 100,40"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,200 30,210"}"#);
    let drawn = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let mut slanted = None;
    for entity in drawn["entities"].as_array().expect("the three lines") {
        let handle = codec::Handle::new(
            u64::from_str_radix(entity["handle"].as_str().unwrap(), 16).unwrap(),
        );
        if line_point(entity, "end").1 > 100.0 {
            slanted = Some(handle);
        }
    }
    let slanted = slanted.expect("the far slanted line");
    app.tabs[app.active_tab]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace)
        .add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(slanted)],
            None,
        );

    let before = app.command_line.history.len();
    let _ = app.automation_op(r#"{"op":"run","cmd":"DCALIGNED"}"#);
    let _ = app.feed_active_cmd("2L");
    let _ = app.feed_active_cmd("50,0");
    let _ = app.feed_active_cmd("50,30");
    let said: Vec<String> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect();
    assert!(
        said.iter()
            .any(|text| text.contains("Specify dimension line location")),
        "the command reaches its own placement prompt: {said:?}"
    );
    assert!(
        said.contains(&format!(
            "The constraint network moved 1 entity the command did not name: {slanted}."
        )),
        "the completion names the line the network moved, and only it: {said:?}"
    );
}

/// The inference command adds constraints without a result, so it drains its
/// own report instead of leaving it for a later command to print.
#[test]
fn auto_constrain_names_geometry_the_network_moves() {
    use crate::scene::named_parameters::DrivingValue;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 200,0"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 0,80"}"#);
    app.automation_op(r#"{"op":"run","cmd":"LINE 0,200 30,210"}"#);
    let drawn = app.automation_op(r#"{"op":"query","type":"Line","detail":"geometry"}"#);
    let mut long = None;
    let mut upright = None;
    let mut slanted = None;
    for entity in drawn["entities"].as_array().expect("the three lines") {
        let handle = codec::Handle::new(
            u64::from_str_radix(entity["handle"].as_str().unwrap(), 16).unwrap(),
        );
        let end = line_point(entity, "end");
        if end.0 > 100.0 {
            long = Some(handle);
        } else if end.0.abs() < 1e-6 {
            upright = Some(handle);
        } else {
            slanted = Some(handle);
        }
    }
    let long = long.expect("the 200-long line");
    let upright = upright.expect("the upright line");
    let slanted = slanted.expect("the slanted line");
    let set = app.tabs[app.active_tab]
        .scene
        .parametric_constraint_set_mut(ParametricScope::ModelSpace);
    set.add(
        ConstraintKind::DistanceX,
        vec![ParametricRef::point(long, 0), ParametricRef::point(long, 1)],
        Some(DrivingValue::Literal(100.0)),
    );
    set.add(
        ConstraintKind::Horizontal,
        vec![ParametricRef::whole(slanted)],
        None,
    );

    app.automation_op(&format!(
        r#"{{"op":"select","handles":["{long:X}","{upright:X}"]}}"#
    ));
    let before = app.command_line.history.len();
    let inferred = app.automation_op(r#"{"op":"run","cmd":"AUTOCONSTRAIN"}"#);
    assert_eq!(inferred["ok"], json!(true), "{inferred}");
    let said: Vec<String> = app.command_line.history[before..]
        .iter()
        .map(|entry| entry.text.clone())
        .collect();
    assert!(
        said.iter()
            .any(|text| text.contains("constraint(s) applied")),
        "the command completes: {said:?}"
    );
    assert!(
        said.contains(&format!(
            "The constraint network moved 1 entity the command did not name: {slanted}."
        )),
        "the completion names the line the network moved, and only it: {said:?}"
    );
}

/// A block whose attributes an automation client fills by tag: WIDTH asks
/// for a value, CONST carries a fixed one.
fn define_attributed_block(app: &mut OpenCADStudio) {
    use codec::entities::AttributeDefinition;
    use codec::types::Vector3;
    let definition = |tag: &str, value: &str, at: Vector3| {
        let mut definition =
            AttributeDefinition::new(tag.to_string(), format!("{tag}:"), value.to_string());
        definition.insertion_point = at;
        definition.height = 1.0;
        definition
    };
    let mut constant = definition("CONST", "fixed", Vector3::new(0.0, 5.0, 0.0));
    constant.flags.constant = true;
    let i = app.active_tab;
    app.tabs[i].scene.define_block_raw(
        "TAGGED",
        Vector3::ZERO,
        vec![
            codec::EntityType::AttributeDefinition(definition(
                "WIDTH",
                "100",
                Vector3::new(2.0, 3.0, 0.0),
            )),
            codec::EntityType::AttributeDefinition(constant),
        ],
    );
}

/// `entities_create` placed an Insert without a way to give its attribute
/// values: the client had to ATTSYNC or patch `/attributes/N/value` by index
/// afterwards. The values now come in by tag, like `-INSERT` asks for them.
#[test]
fn entities_create_insert_takes_attribute_values_by_tag() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    define_attributed_block(&mut app);
    let created = mutate(
        &mut app,
        r#"{"protocol":1,"op":"entities_create","request_id":"attr-values","document_id":{doc},"entities":[
            {"type":"Insert","block":"TAGGED","position":[10,0],"attributes":{"width":"250"}}
        ]}"#,
    );
    assert_eq!(created["ok"], true, "{created}");
    let i = app.active_tab;
    let insert = app.tabs[i]
        .scene
        .document
        .entities()
        .find_map(|entity| match entity {
            codec::EntityType::Insert(insert) if insert.block_name == "TAGGED" => {
                Some(insert.clone())
            }
            _ => None,
        })
        .expect("the reference was created");
    let values: Vec<(&str, &str)> = insert
        .attributes
        .iter()
        .map(|attribute| (attribute.tag.as_str(), attribute.value.as_str()))
        .collect();
    assert_eq!(values, vec![("WIDTH", "250"), ("CONST", "fixed")]);
    assert_eq!(
        insert.attributes[0].insertion_point,
        codec::types::Vector3::new(12.0, 3.0, 0.0),
        "the attribute is placed by the reference"
    );
    assert!(insert
        .attributes
        .iter()
        .all(|attribute| !attribute.common.handle.is_null()));
}

#[test]
fn entities_create_insert_refuses_unknown_constant_and_non_text_attributes() {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    define_attributed_block(&mut app);
    for (id, attributes, code) in [
        ("attr-unknown", r#"{"HEIGHT":"1"}"#, "unknown_attribute_tag"),
        (
            "attr-constant",
            r#"{"CONST":"other"}"#,
            "constant_attribute",
        ),
        (
            "attr-twice",
            r#"{"WIDTH":"1","width":"2"}"#,
            "invalid_attributes",
        ),
    ] {
        let refused = mutate(
            &mut app,
            &format!(
                r#"{{"protocol":1,"op":"entities_create","request_id":"{id}","document_id":{{doc}},"entities":[
                    {{"type":"Insert","block":"TAGGED","position":[0,0],"attributes":{attributes}}}
                ]}}"#
            ),
        );
        assert_eq!(refused["ok"], false, "{refused}");
        assert_eq!(refused["code"], code, "{refused}");
    }
    let spec =
        json!({"type":"Insert","block":"TAGGED","position":[0,0],"attributes":{"WIDTH":250}});
    assert!(crate::entity_creation::validate_entity(&spec, "").is_err());
    assert_eq!(count_type(&mut app, "Insert"), 0, "nothing was created");
}
