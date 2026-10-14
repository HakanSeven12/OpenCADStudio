//! Block delete / rename / purge characterisation. Tests only;
//! no production code changed.
//!
//! Paths covered:
//!   * command line — `RENAME BLOCK old new` (and the absence of a
//!     block-definition DELETE on the command line)
//!   * Blocks palette — `MenuAction::Remove` (there is no rename action)
//!   * plugin API — `HostSession::table_operation(BlockDelete / BlockRename)`
//!   * control/"MCP" — `{"protocol":1,"op":"block_delete"}`
//!   * PURGE — `PURGE BLOCKS`
//!
//! Every scenario is built in code; expected values are hand-written constants.
//! Naming follows the BEDIT convention: `locked_*` for behaviour we want to
//! keep, `documenting_*` for behaviour we expect to change.
//!
//! Every path goes through the shared block service
//! (`crate::scene::block_service`). The `documenting_*` cells whose behaviour
//! changed on purpose were converted to `locked_*` tests pinning the new,
//! shared outcome.

use crate::app::OpenCADStudio;
use codec::entities::{
    Block as BlockEntity, BlockEnd, Dimension, DimensionAligned, Insert, LeaderContentType, Line,
    MultiLeader,
};
use codec::objects::Dictionary;
use codec::tables::{BlockRecord, DimStyle};
use codec::types::{Handle, Vector3};
use codec::EntityType;

fn app() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    app.automation_op(r#"{"op":"new"}"#);
    app
}

fn tab(app: &OpenCADStudio) -> usize {
    app.active_tab
}

fn line(start: [f64; 3], end: [f64; 3]) -> EntityType {
    EntityType::Line(Line::from_points(
        Vector3::new(start[0], start[1], start[2]),
        Vector3::new(end[0], end[1], end[2]),
    ))
}

/// One block definition holding a single LINE (0,0)-(1,0).
fn define_block(app: &mut OpenCADStudio, name: &str) -> Handle {
    let i = tab(app);
    app.tabs[i].scene.define_block_raw(
        name,
        Vector3::ZERO,
        vec![line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0])],
    );
    record_handle(app, name)
}

/// A block definition built by hand, so `*` / xref / layout records can be
/// named exactly (the codec's table does not police the `*` prefix).
fn define_raw_block(app: &mut OpenCADStudio, name: &str, mut record: BlockRecord) -> Handle {
    let i = tab(app);
    let doc = &mut app.tabs[i].scene.document;
    let next = doc.next_handle();
    let br = Handle::new(next);
    let block = Handle::new(next + 1);
    let end = Handle::new(next + 2);
    record.handle = br;
    record.block_entity_handle = block;
    record.block_end_handle = end;
    doc.block_records.add(record).unwrap();
    let mut marker = BlockEntity::new(name, Vector3::ZERO);
    marker.common.handle = block;
    marker.common.owner_handle = br;
    doc.add_entity(EntityType::Block(marker)).unwrap();
    let mut block_end = BlockEnd::new();
    block_end.common.handle = end;
    block_end.common.owner_handle = br;
    doc.add_entity(EntityType::BlockEnd(block_end)).unwrap();
    let mut entity = Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0));
    entity.common.owner_handle = br;
    doc.add_entity(EntityType::Line(entity)).unwrap();
    br
}

fn record_handle(app: &OpenCADStudio, name: &str) -> Handle {
    app.tabs[tab(app)]
        .scene
        .document
        .block_records
        .get(name)
        .map(|r| r.handle)
        .unwrap_or_else(|| panic!("block {name} is not defined"))
}

fn record_exists(app: &OpenCADStudio, name: &str) -> bool {
    app.tabs[tab(app)]
        .scene
        .document
        .block_records
        .get(name)
        .is_some()
}

fn add_insert(app: &mut OpenCADStudio, block: &str) -> Handle {
    let i = tab(app);
    let insert = Insert::new(block.to_string(), Vector3::new(5.0, 5.0, 0.0));
    app.tabs[i].scene.add_entity(EntityType::Insert(insert))
}

/// INSERTs of any kind referencing `name`, in model space and inside block
/// definitions alike (the document's flat entity list).
fn insert_count(app: &OpenCADStudio, name: &str) -> usize {
    app.tabs[tab(app)]
        .scene
        .document
        .entities()
        .filter(
            |e| matches!(e, EntityType::Insert(ins) if ins.block_name.eq_ignore_ascii_case(name)),
        )
        .count()
}

fn entity_count(app: &OpenCADStudio) -> usize {
    app.tabs[tab(app)].scene.document.entities().count()
}

fn errors(app: &OpenCADStudio) -> Vec<String> {
    app.command_line
        .history
        .iter()
        .filter(|entry| entry.kind == crate::ui::command_line::EntryKind::Error)
        .map(|entry| entry.text.clone())
        .collect()
}

fn outputs(app: &OpenCADStudio) -> Vec<String> {
    app.command_line
        .history
        .iter()
        .filter(|entry| entry.kind == crate::ui::command_line::EntryKind::Output)
        .map(|entry| entry.text.clone())
        .collect()
}

/// The label of the step a path pushed. `push_undo_snapshot` parks the entry
/// in `history.pending` until something finishes it, so a fresh step may not
/// be on the stack yet.
fn last_undo_label(app: &OpenCADStudio) -> Option<String> {
    let history = &app.tabs[tab(app)].history;
    history
        .undo_stack
        .last()
        .map(|snapshot| snapshot.label().to_string())
        .or_else(|| {
            history
                .pending
                .as_ref()
                .map(|pending| pending.label.clone())
        })
}

fn undo_entry_count(app: &OpenCADStudio) -> usize {
    let history = &app.tabs[tab(app)].history;
    history.undo_stack.len() + usize::from(history.pending.is_some())
}

fn undo(app: &mut OpenCADStudio) {
    let _ = app.update(crate::app::Message::Undo);
}

// ── scenario fixtures ────────────────────────────────────────────────────

/// 1 unreferenced block.
fn fixture_unreferenced(app: &mut OpenCADStudio) -> String {
    let name = "BO_ONE";
    define_block(app, name);
    name.to_string()
}

/// 1b a second unreferenced block under a different name, so a rename
/// collision needs two distinct records.
fn fixture_other_unreferenced(app: &mut OpenCADStudio) -> String {
    let name = "BO_OTHER";
    define_block(app, name);
    name.to_string()
}

/// 2 referenced by a model-space INSERT.
fn fixture_model_space_reference(app: &mut OpenCADStudio) -> String {
    let name = "BO_TWO";
    define_block(app, name);
    add_insert(app, name);
    name.to_string()
}

/// 3 referenced only by an INSERT nested inside another definition.
fn fixture_nested_reference(app: &mut OpenCADStudio) -> String {
    let name = "BO_INNER";
    let outer = "BO_OUTER";
    define_block(app, name);
    let inner_insert = Insert::new(name.to_string(), Vector3::ZERO);
    let i = tab(app);
    app.tabs[i].scene.define_block_raw(
        outer,
        Vector3::ZERO,
        vec![EntityType::Insert(inner_insert)],
    );
    name.to_string()
}

/// 4 referenced by a dimension's arrow block (the dimension-style reference
/// family, the same relationship a table's anonymous block has with its
/// contents).
fn fixture_dimension_reference(app: &mut OpenCADStudio) -> String {
    let name = "BO_ARROW";
    let br = define_block(app, name);
    let i = tab(app);
    let mut style = DimStyle::new("BO_ARROWSTYLE");
    style.dimblk = br;
    app.tabs[i].scene.document.dim_styles.add(style).unwrap();
    let mut dimension =
        DimensionAligned::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    dimension.base.style_name = "BO_ARROWSTYLE".to_string();
    app.tabs[i]
        .scene
        .add_entity(EntityType::Dimension(Dimension::Aligned(dimension)));
    name.to_string()
}

/// 4b a dimension-style arrow block held by handle *and* by name, so a rename
/// has to follow the name-based reference too (a handle-based one survives on
/// its own).
fn fixture_dimension_name_reference(app: &mut OpenCADStudio) -> String {
    let name = "BO_ARROWNAME";
    let br = define_block(app, name);
    let i = tab(app);
    let mut style = DimStyle::new("BO_ARROWNAMESTYLE");
    style.dimblk = br;
    style.dimblk_name = name.to_string();
    app.tabs[i].scene.document.dim_styles.add(style).unwrap();
    let mut dimension =
        DimensionAligned::new(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
    dimension.base.style_name = "BO_ARROWNAMESTYLE".to_string();
    app.tabs[i]
        .scene
        .add_entity(EntityType::Dimension(Dimension::Aligned(dimension)));
    name.to_string()
}

/// 6 a multi-leader's block content.
fn fixture_multileader_reference(app: &mut OpenCADStudio) -> String {
    let name = "BO_ML";
    let br = define_block(app, name);
    let i = tab(app);
    let mut leader = MultiLeader::new();
    leader.content_type = LeaderContentType::Block;
    leader.context.has_block_contents = true;
    leader.block_content_handle = Some(br);
    app.tabs[i]
        .scene
        .add_entity(EntityType::MultiLeader(Box::new(leader)));
    name.to_string()
}

/// 5 anonymous block, unreferenced.
fn fixture_anonymous(app: &mut OpenCADStudio) -> String {
    let name = "*BO_ANON";
    define_raw_block(app, name, BlockRecord::new(name.to_string()));
    name.to_string()
}

/// 8 a layout (*) block record, under the name the table stores it as (the
/// service reports the stored name in its messages).
fn fixture_layout(app: &mut OpenCADStudio) -> String {
    app.tabs[tab(app)]
        .scene
        .document
        .block_records
        .get("*Model_Space")
        .map(|record| record.name.clone())
        .expect("a model space record")
}

/// 9 xref block.
fn fixture_xref(app: &mut OpenCADStudio) -> String {
    let name = "BO_XREF";
    define_block(app, name);
    let i = tab(app);
    app.tabs[i]
        .scene
        .document
        .block_records
        .get_mut(name)
        .unwrap()
        .flags
        .is_xref = true;
    name.to_string()
}

/// 10 a dynamic block: the record's extension dictionary carries an
/// `ACAD_ENHANCEDBLOCK` entry whose variants dictionary sources a `*U`
/// anonymous record. Both records must survive every delete and purge path.
fn attach_dynamic_block(app: &mut OpenCADStudio, record: &str) -> (Handle, Handle) {
    let record_handle = record_handle(app, record);
    // The *U anonymous record the variants dictionary sources.
    let variant_handle = define_raw_block(
        app,
        "*U_BO_VARIANT",
        BlockRecord::new("*U_BO_VARIANT".to_string()),
    );
    let i = tab(app);
    let doc = &mut app.tabs[i].scene.document;
    let xdict = doc.allocate_handle();
    let variants = doc.allocate_handle();
    let mut dictionary = Dictionary::new();
    dictionary.handle = xdict;
    dictionary.owner = record_handle;
    dictionary.add_entry("ACAD_ENHANCEDBLOCK", variants);
    doc.objects
        .insert(xdict, codec::objects::ObjectType::Dictionary(dictionary));
    let mut variants_dictionary = Dictionary::new();
    variants_dictionary.handle = variants;
    variants_dictionary.owner = xdict;
    variants_dictionary.add_entry("VisibilityState1", variant_handle);
    doc.objects.insert(
        variants,
        codec::objects::ObjectType::Dictionary(variants_dictionary),
    );
    (record_handle, variant_handle)
}

/// A fixture dynamic block: `BO_DYN` plus its `*U` variant record.
fn fixture_dynamic_block(app: &mut OpenCADStudio) -> String {
    let name = "BO_DYN";
    define_block(app, name);
    attach_dynamic_block(app, name);
    name.to_string()
}

// ── path drivers ─────────────────────────────────────────────────────────

fn palette_remove(app: &mut OpenCADStudio, name: &str) {
    let _ = app.on_block_palette(crate::ui::window::block_palette::BlockPaletteMsg::Menu(
        crate::ui::window::block_palette::Item::Current(name.to_string()),
        crate::ui::window::block_palette::MenuAction::Remove,
    ));
}

#[cfg(not(target_arch = "wasm32"))]
fn plugin_block_delete(app: &mut OpenCADStudio, name: &str) -> Result<Handle, String> {
    use ocs_plugin_api::host::TableOperation;
    let i = tab(app);
    let mut host = crate::app::plugin_host::HostSession::new(app, i);
    host.table_operation(TableOperation::BlockDelete {
        name: name.to_string(),
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn plugin_block_rename(app: &mut OpenCADStudio, from: &str, to: &str) -> Result<Handle, String> {
    use ocs_plugin_api::host::TableOperation;
    let i = tab(app);
    let mut host = crate::app::plugin_host::HostSession::new(app, i);
    host.table_operation(TableOperation::BlockRename {
        from: from.to_string(),
        to: to.to_string(),
    })
}

fn control_request(app: &mut OpenCADStudio, mut req: serde_json::Value) -> serde_json::Value {
    use serde_json::json;
    static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let state = app.control_state();
    req["protocol"] = json!(1);
    req["document_id"] = state["document_id"].clone();
    req["revision"] = state["revision"].clone();
    req["client_id"] = json!("test");
    if req.get("request_id").is_none() {
        let id = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        req["request_id"] = json!(format!("req-{id}"));
    }
    let (response, task) = app.control_request(req.clone());
    app.drive_headless_task(task).unwrap();
    if matches!(response["status"].as_str(), Some("accepted" | "running")) {
        app.control_request(json!({"op":"operation","request_id":req["request_id"]}))
            .0
    } else {
        response
    }
}

fn mcp_block_delete(app: &mut OpenCADStudio, name: &str) -> serde_json::Value {
    use serde_json::json;
    let req = json!({"op":"block_delete","name":name});
    control_request(app, req)
}

/// `"force": true` — the protocol change: it erases the
/// referencing INSERTs in the same undo step instead of refusing.
fn mcp_block_delete_forced(app: &mut OpenCADStudio, name: &str) -> serde_json::Value {
    use serde_json::json;
    let req = json!({"op":"block_delete","name":name,"force":true});
    control_request(app, req)
}

fn begin_bedit(app: &mut OpenCADStudio, handle: Handle) {
    let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", handle.value()));
}

/// The exact text the shared service formatter produces, asserted by every
/// path so the matrix cannot drift apart again.
fn referenced_text(name: &str, count: usize) -> String {
    format!("block \"{name}\" is referenced {count} time(s) in the drawing and cannot be removed")
}

/// What the command line shows for one of the service's messages: the text
/// itself plus the `INVALID:` decoration `CommandLine::push_error` adds. The
/// palette and `RENAME` print through that line, so their cells carry the
/// decoration; the plugin and control paths hand the raw text back.
fn command_line_error(text: &str) -> String {
    format!("✕ INVALID: {text}")
}

fn bedit_open(app: &OpenCADStudio) -> bool {
    app.tabs[tab(app)].scene.block_edit_block.is_some()
}

// ── command line: RENAME ─────────────────────────────────────────────────

#[test]
fn locked_rename_block_repoints_inserts_and_undo_restores_the_name() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let insert = add_insert(&mut app, &name);
    let _ = app.dispatch_command(&format!("RENAME BLOCK {name} BO_ONE_RENAMED"));
    assert!(
        !errors(&app).iter().any(|e| e.contains("RENAME")),
        "unexpected RENAME error: {:?}",
        errors(&app)
    );
    assert!(record_exists(&app, "BO_ONE_RENAMED"));
    assert!(!record_exists(&app, &name));
    let repointed = app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(insert)
        .and_then(|e| match e {
            EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        });
    assert_eq!(repointed.as_deref(), Some("BO_ONE_RENAMED"));
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("Rename block BO_ONE to BO_ONE_RENAMED")
    );
    assert!(
        outputs(&app)
            .iter()
            .any(|o| o.contains("RENAME") && o.contains("BO_ONE_RENAMED")),
        "no RENAME success line: {:?}",
        outputs(&app)
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
    assert!(!record_exists(&app, "BO_ONE_RENAMED"));
    let restored = app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(insert)
        .and_then(|e| match e {
            EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        });
    assert_eq!(restored.as_deref(), Some(name.as_str()));
}

#[test]
fn locked_rename_to_an_existing_name_is_refused_with_the_shared_text_leaving_no_undo_entry() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let other = fixture_other_unreferenced(&mut app);
    let before = undo_entry_count(&app);
    let insert_count_before = insert_count(&app, &name);
    let _ = app.dispatch_command(&format!("RENAME BLOCK {name} {other}"));
    assert!(record_exists(&app, &name));
    assert!(record_exists(&app, &other));
    assert_eq!(insert_count(&app, &name), insert_count_before);
    assert_eq!(
        undo_entry_count(&app),
        before,
        "a refused RENAME must not leave an undo entry"
    );
    assert_eq!(
        errors(&app),
        vec![command_line_error(&format!(
            "block \"{other}\" already exists"
        ))]
    );
    assert!(
        !outputs(&app).iter().any(|o| o.contains("RENAME")),
        "a success line was printed for a refused rename: {:?}",
        outputs(&app)
    );
}

#[test]
fn locked_rename_to_an_invalid_name_is_refused_with_the_shared_text() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let before = undo_entry_count(&app);
    let _ = app.dispatch_command(&format!("RENAME BLOCK {name} bad/name"));
    assert!(record_exists(&app, &name));
    assert!(!record_exists(&app, "bad/name"));
    assert_eq!(undo_entry_count(&app), before);
    assert_eq!(
        errors(&app),
        vec![command_line_error("a block name cannot contain '/'")]
    );
}

/// A rename follows a name-based reference wherever it sits: an INSERT nested
/// inside another definition repoints like a model-space one.
#[test]
fn locked_rename_follows_an_insert_nested_inside_another_definition() {
    let mut app = app();
    let name = fixture_nested_reference(&mut app);
    let nested = app.tabs[tab(&app)]
        .scene
        .document
        .entities()
        .find(|e| matches!(e, EntityType::Insert(ins) if ins.block_name == name))
        .map(|e| e.common().handle)
        .expect("the nested INSERT");
    let _ = app.dispatch_command(&format!("RENAME BLOCK {name} BO_INNER_RENAMED"));
    assert!(record_exists(&app, "BO_INNER_RENAMED"));
    assert!(!record_exists(&app, &name));
    let repointed = app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(nested)
        .and_then(|e| match e {
            EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        });
    assert_eq!(repointed.as_deref(), Some("BO_INNER_RENAMED"));
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("Rename block BO_INNER to BO_INNER_RENAMED")
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
    let restored = app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(nested)
        .and_then(|e| match e {
            EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        });
    assert_eq!(restored.as_deref(), Some(name.as_str()));
}

/// A dimension-style arrow block held by name follows the rename too; the
/// handle-based reference keeps working because the record keeps its handle.
#[test]
fn locked_rename_repoints_a_dimension_arrow_block_name() {
    let mut app = app();
    let name = fixture_dimension_name_reference(&mut app);
    let _ = app.dispatch_command(&format!("RENAME BLOCK {name} BO_ARROWNAME_NEW"));
    assert!(record_exists(&app, "BO_ARROWNAME_NEW"));
    assert!(!record_exists(&app, &name));
    let i = tab(&app);
    let style = app.tabs[i]
        .scene
        .document
        .dim_styles
        .get("BO_ARROWNAMESTYLE")
        .unwrap();
    assert_eq!(style.dimblk_name, "BO_ARROWNAME_NEW");
    assert!(
        style.dimblk.is_valid(),
        "the handle-based arrow was dropped"
    );
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("Rename block BO_ARROWNAME to BO_ARROWNAME_NEW")
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
    assert_eq!(
        app.tabs[tab(&app)]
            .scene
            .document
            .dim_styles
            .get("BO_ARROWNAMESTYLE")
            .unwrap()
            .dimblk_name,
        name
    );
}

/// A rename refuses the protected records with the shared text, one refusal
/// per record and no undo entry for either.
#[test]
fn locked_rename_refuses_anonymous_and_layout_records_with_the_shared_text() {
    let mut app = app();
    let anonymous = fixture_anonymous(&mut app);
    let layout = fixture_layout(&mut app);
    let before = undo_entry_count(&app);
    let _ = app.dispatch_command(&format!("RENAME BLOCK {anonymous} BO_NEW"));
    let _ = app.dispatch_command(&format!("RENAME BLOCK {layout} BO_NEW"));
    assert!(record_exists(&app, &anonymous));
    assert!(record_exists(&app, &layout));
    assert!(!record_exists(&app, "BO_NEW"));
    assert_eq!(undo_entry_count(&app), before);
    assert_eq!(
        errors(&app),
        vec![
            command_line_error(&format!("block \"{anonymous}\" is an anonymous block")),
            command_line_error(&format!(
                "block \"{layout}\" is a model or paper space layout record"
            )),
        ]
    );
}

/// A rename refuses a block with an open BEDIT session, whatever the new name.
#[test]
fn locked_rename_refuses_a_block_with_an_open_bedit_session() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let insert = add_insert(&mut app, &name);
    begin_bedit(&mut app, insert);
    let before = undo_entry_count(&app);
    let _ = app.dispatch_command(&format!("RENAME BLOCK {name} BO_ONE_RENAMED"));
    assert!(record_exists(&app, &name));
    assert!(!record_exists(&app, "BO_ONE_RENAMED"));
    assert_eq!(undo_entry_count(&app), before);
    assert_eq!(
        errors(&app),
        vec![command_line_error(&format!(
            "block \"{name}\" is open in the block editor; close the editor first"
        ))]
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

/// A handle-based reference (a multi-leader's block content, a
/// dimension-style arrow handle) does not block a rename: the record keeps its
/// handle, so the reference keeps resolving.
#[test]
fn locked_rename_succeeds_when_only_a_handle_based_reference_exists() {
    let mut app = app();
    let name = fixture_multileader_reference(&mut app);
    let _ = app.dispatch_command(&format!("RENAME BLOCK {name} BO_ML_RENAMED"));
    assert!(record_exists(&app, "BO_ML_RENAMED"));
    assert!(!record_exists(&app, &name));
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("Rename block BO_ML to BO_ML_RENAMED")
    );
    let content = app.tabs[tab(&app)]
        .scene
        .document
        .entities()
        .find_map(|e| match e {
            EntityType::MultiLeader(ml) => ml.block_content_handle,
            _ => None,
        })
        .expect("the multi-leader's block content");
    let renamed = app.tabs[tab(&app)]
        .scene
        .document
        .block_records
        .get("BO_ML_RENAMED")
        .unwrap();
    assert_eq!(
        renamed.handle, content,
        "the handle-based reference no longer resolves"
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
}

#[test]
fn documenting_command_line_has_no_block_definition_delete() {
    let mut app = app();
    let name = fixture_model_space_reference(&mut app);
    let before = undo_entry_count(&app);
    for command in ["DELETE", "BDELETE", "BLOCKDELETE", "PURGEBLOCK"] {
        let _ = app.dispatch_command(command);
    }
    assert!(
        record_exists(&app, &name),
        "a command-line block delete would have removed {name}"
    );
    assert_eq!(
        undo_entry_count(&app),
        before,
        "no command created an undo entry for a block delete"
    );
    assert!(insert_count(&app, &name) > 0);
    // read from code: DELETE is a shortcut for DELETESELECTED
    // (src/app/shortcuts.rs:50) and ERASE erases selected entities
    // (src/app/commands/draw.rs:933); no command name deletes a definition.
}

// ── Blocks palette ───────────────────────────────────────────────────────

#[test]
fn locked_palette_remove_deletes_an_unreferenced_block_with_a_delete_undo_label() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    palette_remove(&mut app, &name);
    assert!(!record_exists(&app, &name), "the definition survived");
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("Delete block BO_ONE")
    );
    assert!(
        outputs(&app).iter().any(|o| o.contains("removed")),
        "no palette success line: {:?}",
        outputs(&app)
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
}

#[test]
fn locked_palette_remove_refuses_a_model_space_referenced_block() {
    let mut app = app();
    let name = fixture_model_space_reference(&mut app);
    let entities_before = entity_count(&app);
    let undo_before = undo_entry_count(&app);
    palette_remove(&mut app, &name);
    assert!(record_exists(&app, &name), "the definition was removed");
    assert_eq!(insert_count(&app, &name), 1, "the INSERT was erased");
    assert_eq!(entity_count(&app), entities_before);
    assert_eq!(
        undo_entry_count(&app),
        undo_before,
        "an undo entry was pushed"
    );
    assert_eq!(
        errors(&app),
        vec![command_line_error(&referenced_text(&name, 1))],
        "the palette must show the shared refusal text"
    );
}

#[test]
fn locked_palette_remove_refuses_a_block_referenced_from_inside_a_definition() {
    let mut app = app();
    let name = fixture_nested_reference(&mut app);
    let undo_before = undo_entry_count(&app);
    palette_remove(&mut app, &name);
    assert!(record_exists(&app, &name));
    assert_eq!(insert_count(&app, &name), 1);
    assert_eq!(undo_entry_count(&app), undo_before);
    assert_eq!(
        errors(&app),
        vec![command_line_error(&referenced_text(&name, 1))],
        "a nested INSERT is a reference like any other"
    );
}

#[test]
fn locked_palette_remove_refuses_a_multileader_block_content_reference() {
    let mut app = app();
    let name = fixture_multileader_reference(&mut app);
    let undo_before = undo_entry_count(&app);
    palette_remove(&mut app, &name);
    assert!(record_exists(&app, &name));
    assert_eq!(
        errors(&app),
        vec![command_line_error(&referenced_text(&name, 1))],
        "a multi-leader's block content is a reference like any other"
    );
    assert_eq!(undo_entry_count(&app), undo_before);
}

#[test]
fn locked_palette_remove_refuses_a_dimension_arrow_block_reference() {
    let mut app = app();
    let name = fixture_dimension_reference(&mut app);
    let undo_before = undo_entry_count(&app);
    palette_remove(&mut app, &name);
    assert!(
        record_exists(&app, &name),
        "the palette deleted a block a dimension style still points at"
    );
    assert_eq!(
        errors(&app),
        vec![command_line_error(&referenced_text(&name, 2))],
        "the dimension's arrow-block handle and the picture are references"
    );
    assert_eq!(undo_entry_count(&app), undo_before);
}

#[test]
fn locked_palette_remove_refuses_an_anonymous_block() {
    let mut app = app();
    let name = fixture_anonymous(&mut app);
    let undo_before = undo_entry_count(&app);
    palette_remove(&mut app, &name);
    assert!(
        record_exists(&app, &name),
        "the palette deleted an anonymous block"
    );
    assert_eq!(
        errors(&app),
        vec![command_line_error(&format!(
            "block \"{name}\" is an anonymous block"
        ))]
    );
    assert_eq!(undo_entry_count(&app), undo_before);
}

#[test]
fn locked_palette_remove_refuses_the_model_space_record() {
    let mut app = app();
    let name = fixture_layout(&mut app);
    palette_remove(&mut app, &name);
    assert!(
        record_exists(&app, &name),
        "the palette removed the model space record"
    );
    assert_eq!(
        errors(&app),
        vec![command_line_error(&format!(
            "block \"{name}\" is a model or paper space layout record"
        ))]
    );
}

#[test]
fn locked_palette_remove_refuses_an_xref_block() {
    let mut app = app();
    let name = fixture_xref(&mut app);
    palette_remove(&mut app, &name);
    assert!(record_exists(&app, &name), "the palette deleted the xref");
    assert_eq!(
        errors(&app),
        vec![command_line_error(&format!(
            "block \"{name}\" is an externally referenced block"
        ))]
    );
}

#[test]
fn locked_palette_remove_refuses_a_dynamic_block() {
    let mut app = app();
    let name = fixture_dynamic_block(&mut app);
    let undo_before = undo_entry_count(&app);
    palette_remove(&mut app, &name);
    assert!(
        record_exists(&app, &name),
        "the palette deleted a dynamic block"
    );
    assert_eq!(
        errors(&app),
        vec![command_line_error(&format!(
            "block \"{name}\" holds dynamic-block data"
        ))]
    );
    assert_eq!(undo_entry_count(&app), undo_before);
}

#[test]
fn locked_palette_remove_refuses_a_block_with_an_open_bedit_session() {
    let mut app = app();
    let name = fixture_model_space_reference(&mut app);
    let insert = add_insert(&mut app, &name);
    begin_bedit(&mut app, insert);
    assert!(bedit_open(&app), "BEDIT did not start");
    let undo_before = undo_entry_count(&app);
    // The session is the reason for the refusal, not the INSERT it edits.
    palette_remove(&mut app, &name);
    assert!(record_exists(&app, &name));
    assert_eq!(undo_entry_count(&app), undo_before);
    assert_eq!(
        errors(&app),
        vec![command_line_error(&format!(
            "block \"{name}\" is open in the block editor; close the editor first"
        ))]
    );
}

/// Switching replaces the open session; the active definition stays protected.
#[test]
fn locked_palette_remove_refuses_the_open_block_after_switching() {
    let mut app = app();
    let inner = "BO_NEST_INNER";
    define_block(&mut app, inner);
    let outer = "BO_NEST_OUTER";
    let nested_insert = Insert::new(inner.to_string(), Vector3::ZERO);
    let i = tab(&app);
    app.tabs[i].scene.define_block_raw(
        outer,
        Vector3::ZERO,
        vec![EntityType::Insert(nested_insert)],
    );
    let outer_reference = add_insert(&mut app, outer);
    let inner_reference = app.tabs[i]
        .scene
        .document
        .entities()
        .find(|e| matches!(e, EntityType::Insert(ins) if ins.block_name == inner))
        .map(|e| e.common().handle)
        .expect("the nested INSERT");
    begin_bedit(&mut app, outer_reference);
    begin_bedit(&mut app, inner_reference);
    assert_eq!(
        app.tabs[tab(&app)].block_edits.len(),
        1,
        "switching must keep one session"
    );
    for name in [inner] {
        palette_remove(&mut app, name);
        assert!(record_exists(&app, name), "{name} went while open");
        let expected = command_line_error(&format!(
            "block \"{name}\" is open in the block editor; close the editor first"
        ));
        assert_eq!(
            errors(&app).last(),
            Some(&expected),
            "{name} was refused with the wrong text"
        );
    }
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

// ── plugin API ───────────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_delete_removes_an_unreferenced_block_and_undo_restores_it() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let before = entity_count(&app);
    assert!(plugin_block_delete(&mut app, &name).is_ok());
    assert!(!record_exists(&app, &name));
    assert!(entity_count(&app) < before, "the block's entities survived");
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("Delete block BO_ONE")
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_delete_refuses_a_model_space_reference() {
    let mut app = app();
    let name = fixture_model_space_reference(&mut app);
    let undo_before = undo_entry_count(&app);
    let err = plugin_block_delete(&mut app, &name).expect_err("should refuse");
    assert_eq!(err, referenced_text(&name, 1));
    assert!(record_exists(&app, &name));
    assert_eq!(insert_count(&app, &name), 1);
    assert_eq!(undo_entry_count(&app), undo_before);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_delete_refuses_a_block_referenced_only_inside_a_definition() {
    let mut app = app();
    let name = fixture_nested_reference(&mut app);
    let err = plugin_block_delete(&mut app, &name).expect_err("should refuse");
    assert_eq!(err, referenced_text(&name, 1));
    assert!(record_exists(&app, &name));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_delete_refuses_a_dimension_arrow_block() {
    let mut app = app();
    let name = fixture_dimension_reference(&mut app);
    let err = plugin_block_delete(&mut app, &name).expect_err("should refuse");
    assert_eq!(err, referenced_text(&name, 2));
    assert!(record_exists(&app, &name));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_delete_refuses_anonymous_layout_and_xref_blocks() {
    let mut app = app();
    let anonymous = fixture_anonymous(&mut app);
    let err = plugin_block_delete(&mut app, &anonymous).expect_err("should refuse");
    assert_eq!(err, format!("block \"{anonymous}\" is an anonymous block"));
    assert!(record_exists(&app, &anonymous));

    let xref = fixture_xref(&mut app);
    let err = plugin_block_delete(&mut app, &xref).expect_err("should refuse");
    assert_eq!(
        err,
        format!("block \"{xref}\" is an externally referenced block")
    );
    assert!(record_exists(&app, &xref));

    let layout = fixture_layout(&mut app);
    let err = plugin_block_delete(&mut app, &layout).expect_err("should refuse");
    assert_eq!(
        err,
        format!("block \"{layout}\" is a model or paper space layout record")
    );
    assert!(record_exists(&app, &layout));
}

/// The plugin API renames through the same service, so its refusals carry the
/// same text as the command line's and the palette's.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_rename_refuses_anonymous_layout_and_xref_records() {
    let mut app = app();
    let anonymous = fixture_anonymous(&mut app);
    let err =
        plugin_block_rename(&mut app, &anonymous, "BO_PLUGIN_RENAMED").expect_err("should refuse");
    assert_eq!(err, format!("block \"{anonymous}\" is an anonymous block"));

    let xref = fixture_xref(&mut app);
    let err = plugin_block_rename(&mut app, &xref, "BO_PLUGIN_RENAMED").expect_err("should refuse");
    assert_eq!(
        err,
        format!("block \"{xref}\" is an externally referenced block")
    );

    let layout = fixture_layout(&mut app);
    let err =
        plugin_block_rename(&mut app, &layout, "BO_PLUGIN_RENAMED").expect_err("should refuse");
    assert_eq!(
        err,
        format!("block \"{layout}\" is a model or paper space layout record")
    );
    for name in [anonymous, xref, layout] {
        assert!(record_exists(&app, &name), "{name} was renamed");
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_rename_follows_inserts_and_refuses_taken_or_invalid_names() {
    let mut app = app();
    let name = fixture_model_space_reference(&mut app);
    let insert = add_insert(&mut app, &name);
    let other = fixture_unreferenced(&mut app);
    assert!(plugin_block_rename(&mut app, &name, "BO_PLUGIN_RENAMED").is_ok());
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some("Rename block BO_TWO to BO_PLUGIN_RENAMED")
    );
    assert!(!record_exists(&app, &name));
    let repointed = app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(insert)
        .and_then(|e| match e {
            EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        });
    assert_eq!(repointed.as_deref(), Some("BO_PLUGIN_RENAMED"));

    let before = undo_entry_count(&app);
    let err = plugin_block_rename(&mut app, "BO_PLUGIN_RENAMED", &other)
        .expect_err("should refuse a taken name");
    assert_eq!(err, format!("block \"{other}\" already exists"));
    let err = plugin_block_rename(&mut app, "BO_PLUGIN_RENAMED", "bad/name")
        .expect_err("should refuse an invalid name");
    assert_eq!(err, "a block name cannot contain '/'");
    let err = plugin_block_rename(&mut app, "BO_PLUGIN_RENAMED", "")
        .expect_err("should refuse an empty name");
    assert_eq!(err, "a block name cannot be empty");
    assert_eq!(undo_entry_count(&app), before);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn locked_plugin_block_delete_refuses_a_block_with_an_open_bedit_session() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    // A BEDIT session can only be started on a reference, so insert one.
    let insert = add_insert(&mut app, &name);
    begin_bedit(&mut app, insert);
    let err = plugin_block_delete(&mut app, &name).expect_err("should refuse");
    assert_eq!(
        err,
        format!("block \"{name}\" is open in the block editor; close the editor first")
    );
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

// ── control ("MCP") block_delete ─────────────────────────────────────────

#[test]
fn locked_mcp_block_delete_refuses_a_referenced_block_without_force() {
    let mut app = app();
    let name = fixture_model_space_reference(&mut app);
    add_insert(&mut app, &name);
    let undo_before = undo_entry_count(&app);
    let response = mcp_block_delete(&mut app, &name);
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["code"], "block_referenced", "{response}");
    assert_eq!(response["error"], referenced_text(&name, 2));
    assert!(record_exists(&app, &name));
    assert_eq!(insert_count(&app, &name), 2);
    assert_eq!(undo_entry_count(&app), undo_before);
}

#[test]
fn locked_mcp_block_delete_with_force_erases_referencing_inserts_and_undo_restores_them() {
    let mut app = app();
    let name = fixture_model_space_reference(&mut app);
    let insert = add_insert(&mut app, &name);
    let response = mcp_block_delete_forced(&mut app, &name);
    assert_eq!(response["ok"], true, "{response}");
    assert!(!record_exists(&app, &name), "the definition survived");
    assert_eq!(insert_count(&app, &name), 0, "the INSERTs survived");
    assert!(app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(insert)
        .is_none());
    assert_eq!(
        last_undo_label(&app).as_deref(),
        Some(format!("Delete block {name}").as_str())
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
    assert_eq!(insert_count(&app, &name), 2);
    assert!(app.tabs[tab(&app)]
        .scene
        .document
        .get_entity(insert)
        .is_some());
}

#[test]
fn locked_mcp_block_delete_with_force_erases_inserts_nested_in_other_definitions() {
    let mut app = app();
    let name = fixture_nested_reference(&mut app);
    let response = mcp_block_delete_forced(&mut app, &name);
    assert_eq!(response["ok"], true, "{response}");
    assert!(!record_exists(&app, &name));
    assert_eq!(insert_count(&app, &name), 0, "the nested INSERT survived");
    undo(&mut app);
    assert!(record_exists(&app, &name));
    assert_eq!(insert_count(&app, &name), 1);
}

#[test]
fn locked_mcp_block_delete_refuses_layout_xref_and_anonymous_records() {
    let mut app = app();
    let layout = fixture_layout(&mut app);
    let response = mcp_block_delete(&mut app, &layout);
    assert_eq!(response["code"], "block_protected", "{response}");
    assert!(record_exists(&app, &layout));

    let xref = fixture_xref(&mut app);
    let response = mcp_block_delete_forced(&mut app, &xref);
    assert_eq!(response["code"], "block_protected", "{response}");
    assert!(record_exists(&app, &xref), "the xref definition survived");

    let anonymous = fixture_anonymous(&mut app);
    let response = mcp_block_delete_forced(&mut app, &anonymous);
    assert_eq!(response["code"], "block_protected", "{response}");
    assert!(record_exists(&app, &anonymous));
}

#[test]
fn locked_mcp_block_delete_refuses_a_dimension_arrow_block() {
    let mut app = app();
    let name = fixture_dimension_reference(&mut app);
    let response = mcp_block_delete(&mut app, &name);
    assert_eq!(response["code"], "block_referenced", "{response}");
    assert_eq!(response["error"], referenced_text(&name, 2), "{response}");
    assert!(record_exists(&app, &name));
    assert!(
        app.tabs[tab(&app)]
            .scene
            .document
            .dim_styles
            .iter()
            .any(|style| style.dimblk.is_valid()),
        "the dimension style kept its arrow block handle"
    );
}

#[test]
fn locked_mcp_block_delete_reports_an_unknown_block() {
    let mut app = app();
    let response = mcp_block_delete(&mut app, "BO_NOPE");
    assert_eq!(response["code"], "block_missing", "{response}");
}

#[test]
fn locked_mcp_block_delete_refuses_a_block_with_an_open_bedit_session() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let insert = add_insert(&mut app, &name);
    begin_bedit(&mut app, insert);
    assert!(bedit_open(&app));
    let response = mcp_block_delete(&mut app, &name);
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["code"], "block_in_edit", "{response}");
    assert_eq!(
        response["error"],
        format!("block \"{name}\" is open in the block editor; close the editor first")
    );
    assert!(record_exists(&app, &name));
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

/// `force` erases referencing INSERTs, not the other protections: an open
/// BEDIT session still refuses.
#[test]
fn locked_mcp_block_delete_with_force_still_refuses_an_open_bedit_session() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let insert = add_insert(&mut app, &name);
    begin_bedit(&mut app, insert);
    let response = mcp_block_delete_forced(&mut app, &name);
    assert_eq!(response["ok"], false, "{response}");
    assert_eq!(response["code"], "block_in_edit", "{response}");
    assert!(record_exists(&app, &name));
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

// ── PURGE ────────────────────────────────────────────────────────────────

#[test]
fn locked_purge_removes_an_unreferenced_block_and_undo_restores_it() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let before = entity_count(&app);
    let _ = app.dispatch_command("PURGE BLOCKS");
    assert!(!record_exists(&app, &name), "the definition survived");
    assert!(entity_count(&app) < before);
    assert_eq!(last_undo_label(&app).as_deref(), Some("Purge"));
    assert!(
        outputs(&app).iter().any(|o| o.contains("PURGE:")),
        "no PURGE success line: {:?}",
        outputs(&app)
    );
    undo(&mut app);
    assert!(record_exists(&app, &name));
}

#[test]
fn locked_purge_keeps_a_model_space_referenced_block() {
    let mut app = app();
    let model_space = fixture_model_space_reference(&mut app);
    let dimension = fixture_dimension_reference(&mut app);
    let _ = app.dispatch_command("PURGE BLOCKS");
    assert!(
        record_exists(&app, &model_space),
        "purged a model-space referenced block"
    );
    assert_eq!(insert_count(&app, &model_space), 1);
    assert!(
        record_exists(&app, &dimension),
        "purged a dimension arrow block"
    );
}

#[test]
fn locked_purge_removes_a_block_referenced_only_inside_an_unreferenced_definition() {
    // An INSERT inside an unreferenced definition is a reference like any
    // other, so the inner block is skipped on the first pass; the outer
    // definition goes (nothing references it), and only once it is gone does
    // the inner block qualify on the next pass. Both records end up removed,
    // exactly as before the shared service, but now for one shared reason.
    let mut app = app();
    let nested = fixture_nested_reference(&mut app);
    let _ = app.dispatch_command("PURGE BLOCKS");
    assert!(
        !record_exists(&app, &nested),
        "the nested-only block was kept"
    );
    assert!(
        !record_exists(&app, "BO_OUTER"),
        "the outer definition was kept"
    );
    assert_eq!(insert_count(&app, &nested), 0);
}

#[test]
fn locked_purge_removes_an_unreferenced_anonymous_block() {
    let mut app = app();
    let name = fixture_anonymous(&mut app);
    let _ = app.dispatch_command("PURGE BLOCKS");
    assert!(!record_exists(&app, &name), "the anonymous block survived");
}

/// The unsafe anonymous case: an INSERT references the anonymous record, so
/// PURGE keeps it exactly like any other referenced block.
#[test]
fn locked_purge_keeps_a_referenced_anonymous_block() {
    let mut app = app();
    let name = fixture_anonymous(&mut app);
    add_insert(&mut app, &name);
    let _ = app.dispatch_command("PURGE BLOCKS");
    assert!(
        record_exists(&app, &name),
        "purge removed a referenced anonymous block"
    );
    assert_eq!(insert_count(&app, &name), 1);
}

/// A dynamic block and the `*U` record its variants dictionary sources are
/// both unreferenced, yet both stay: without the dictionary walk the
/// anonymous `*U` record would qualify on its own.
#[test]
fn locked_purge_keeps_a_dynamic_block_and_its_u_variant() {
    let mut app = app();
    let name = fixture_dynamic_block(&mut app);
    let _ = app.dispatch_command("PURGE BLOCKS");
    assert!(record_exists(&app, &name), "purged a dynamic block");
    assert!(
        record_exists(&app, "*U_BO_VARIANT"),
        "purged the *U record the dynamic block sources"
    );
}

#[test]
fn locked_purge_keeps_xref_and_layout_records() {
    let mut app = app();
    let xref = fixture_xref(&mut app);
    let layout = fixture_layout(&mut app);
    let _ = app.dispatch_command("PURGE BLOCKS");
    assert!(record_exists(&app, &xref), "purged an xref definition");
    assert!(record_exists(&app, &layout), "purged a layout record");
}

#[test]
fn locked_purge_skips_a_block_with_an_open_bedit_session() {
    let mut app = app();
    let name = fixture_unreferenced(&mut app);
    let insert = add_insert(&mut app, &name);
    begin_bedit(&mut app, insert);
    let _ = app.dispatch_command("PURGE BLOCKS");
    // The BEDIT session's own reference keeps the block live through the
    // edited block record, so the purge sees it as reachable.
    assert!(record_exists(&app, &name));
    let _ = app.dispatch_command("BEDIT_DISCARD");
}

#[test]
fn locked_attedit_attribute_flags_cases() {
    use crate::ui::window::attribute_editor::AttrRow;
    use codec::entities::{AttributeDefinition, AttributeEntity};
    use codec::tables::BlockRecord;

    // Verify flag behavior in ATTEDIT conversion and apply:
    // 1. Normal attribute: editable, modifies value
    let mut normal_entity =
        AttributeEntity::new("NORM_TAG".to_string(), "original_norm".to_string());
    let normal_row = AttrRow {
        tag: "NORM_TAG".to_string(),
        prompt: "Enter norm:".to_string(),
        value: "edited_norm".to_string(),
        text_style: "STANDARD".to_string(),
        height: "2.5".to_string(),
        rotation: "0".to_string(),
        width_factor: "1".to_string(),
        oblique: "0".to_string(),
        h_align: codec::entities::HorizontalAlignment::Left,
        v_align: codec::entities::VerticalAlignment::Baseline,
        backwards: false,
        upside_down: false,
        layer: "0".to_string(),
        color: codec::types::Color::ByLayer,
        linetype: "ByLayer".to_string(),
        line_weight: codec::types::LineWeight::ByLayer,
        is_constant: false,
    };
    assert!(!normal_row.is_constant);

    // 2. Constant attribute: is_constant is true, apply ignores value edit
    let mut const_entity =
        AttributeEntity::new("CONST_TAG".to_string(), "original_const".to_string());
    const_entity.flags.constant = true;
    let mut const_row = normal_row.clone();
    const_row.tag = "CONST_TAG".to_string();
    const_row.value = "attempted_edit".to_string();
    const_row.is_constant = true;

    // 3. Invisible attribute: editable in ATTEDIT (flags.invisible is true, but is_constant is false)
    let mut invis_entity =
        AttributeEntity::new("INVIS_TAG".to_string(), "original_invis".to_string());
    invis_entity.flags.invisible = true;
    let mut invis_row = normal_row.clone();
    invis_row.tag = "INVIS_TAG".to_string();
    invis_row.value = "edited_invis".to_string();
    invis_row.is_constant = false;

    // 4. Preset attribute: editable in ATTEDIT (preset only affects initial insertion prompt)
    let mut preset_entity =
        AttributeEntity::new("PRESET_TAG".to_string(), "original_preset".to_string());
    preset_entity.flags.preset = true;
    let mut preset_row = normal_row.clone();
    preset_row.tag = "PRESET_TAG".to_string();
    preset_row.value = "edited_preset".to_string();
    preset_row.is_constant = false;

    // Verify metadata mapping from CadDocument
    let mut doc = codec::CadDocument::new();
    let next = doc.next_handle();
    let br_h = Handle::new(next);
    let mut br = BlockRecord::new("FLAG_BLOCK");
    br.handle = br_h;
    br.block_entity_handle = Handle::new(next + 1);
    br.block_end_handle = Handle::new(next + 2);

    let mut ad_norm = AttributeDefinition::new(
        "NORM_TAG".to_string(),
        "Enter norm:".to_string(),
        "def".to_string(),
    );
    let mut ad_const = AttributeDefinition::new(
        "CONST_TAG".to_string(),
        "Enter const:".to_string(),
        "def".to_string(),
    );
    ad_const.flags.constant = true;
    let mut ad_invis = AttributeDefinition::new(
        "INVIS_TAG".to_string(),
        "Enter invis:".to_string(),
        "def".to_string(),
    );
    ad_invis.flags.invisible = true;
    let mut ad_preset = AttributeDefinition::new(
        "PRESET_TAG".to_string(),
        "Enter preset:".to_string(),
        "def".to_string(),
    );
    ad_preset.flags.preset = true;

    let h1 = Handle::new(next + 3);
    let h2 = Handle::new(next + 4);
    let h3 = Handle::new(next + 5);
    let h4 = Handle::new(next + 6);
    ad_norm.common.handle = h1;
    ad_const.common.handle = h2;
    ad_invis.common.handle = h3;
    ad_preset.common.handle = h4;
    br.entity_handles = vec![h1, h2, h3, h4];

    doc.block_records.add(br).unwrap();
    doc.add_entity(EntityType::AttributeDefinition(ad_norm))
        .unwrap();
    doc.add_entity(EntityType::AttributeDefinition(ad_const))
        .unwrap();
    doc.add_entity(EntityType::AttributeDefinition(ad_invis))
        .unwrap();
    doc.add_entity(EntityType::AttributeDefinition(ad_preset))
        .unwrap();

    let meta = crate::app::update::command::block_attr_meta(&doc, "FLAG_BLOCK");
    assert!(!meta.get("NORM_TAG").unwrap().constant);
    assert!(meta.get("CONST_TAG").unwrap().constant);
    assert!(!meta.get("INVIS_TAG").unwrap().constant);
    assert!(!meta.get("PRESET_TAG").unwrap().constant);

    // Test conversion from AttributeEntity to AttrRow
    let r_norm = crate::app::update::command::attr_row_from_entity(&normal_entity, &meta);
    assert!(!r_norm.is_constant);
    assert_eq!(r_norm.prompt, "Enter norm:");

    let r_const = crate::app::update::command::attr_row_from_entity(&const_entity, &meta);
    assert!(r_const.is_constant);
    assert_eq!(r_const.prompt, "Enter const:");

    let r_invis = crate::app::update::command::attr_row_from_entity(&invis_entity, &meta);
    assert!(!r_invis.is_constant);
    assert_eq!(r_invis.prompt, "Enter invis:");

    let r_preset = crate::app::update::command::attr_row_from_entity(&preset_entity, &meta);
    assert!(!r_preset.is_constant);
    assert_eq!(r_preset.prompt, "Enter preset:");

    // Test apply_attr_row behavior on constant vs non-constant
    // Normal: applies
    let mut edited_norm_row = r_norm.clone();
    edited_norm_row.value = "edited_norm".to_string();
    assert!(crate::app::update::command::apply_attr_row(
        &mut normal_entity,
        &edited_norm_row
    ));
    assert_eq!(normal_entity.get_value(), "edited_norm");

    // Constant: ignores value change!
    let mut edited_const_row = r_const.clone();
    edited_const_row.value = "attempted_edit".to_string();
    assert!(!crate::app::update::command::apply_attr_row(
        &mut const_entity,
        &edited_const_row
    ));
    assert_eq!(const_entity.get_value(), "original_const");

    // Invisible: applies
    let mut edited_invis_row = r_invis.clone();
    edited_invis_row.value = "edited_invis".to_string();
    assert!(crate::app::update::command::apply_attr_row(
        &mut invis_entity,
        &edited_invis_row
    ));
    assert_eq!(invis_entity.get_value(), "edited_invis");

    // Preset: applies
    let mut edited_preset_row = r_preset.clone();
    edited_preset_row.value = "edited_preset".to_string();
    assert!(crate::app::update::command::apply_attr_row(
        &mut preset_entity,
        &edited_preset_row
    ));
    assert_eq!(preset_entity.get_value(), "edited_preset");
}

#[test]
fn locked_bedit_insert_refuses_cycle() {
    let mut app = app();
    let name_a = "BEDIT_CYC_A";
    let name_b = "BEDIT_CYC_B";

    define_block(&mut app, name_a);

    // B contains an insert of A
    let ins_a = Insert::new(name_a.to_string(), Vector3::ZERO);
    let i = tab(&app);
    app.tabs[i]
        .scene
        .define_block_raw(name_b, Vector3::ZERO, vec![EntityType::Insert(ins_a)]);

    // Now open BEDIT for block A
    let insert_a = add_insert(&mut app, name_a);
    begin_bedit(&mut app, insert_a);
    assert!(bedit_open(&app));

    // Try committing an INSERT of B into A inside BEDIT space:
    // Inserting B into A would create A -> B -> A cycle
    let _ = app.dispatch_command("-INSERT");
    let mut feed = |text: &str| {
        let _ = app.update(crate::app::Message::CommandInput(text.to_string()));
        let _ = app.update(crate::app::Message::CommandSubmit);
    };
    feed(name_b);
    feed("0,0");
    feed("1");
    feed("1");
    feed("0");

    // Verify refusal error on command line
    let errs = errors(&app);
    assert!(
        errs.iter()
            .any(|e| e.contains("circular dependency detected")),
        "expected cycle error in errors: {errs:?}"
    );

    let _ = app.dispatch_command("BEDIT_DISCARD");
}
