//! The import commands: IMPORT and the files it routes, ACISIN, WMFIN and
//! SVGIN, and the 3D Studio Import window, run as typed on the command
//! line (with the file named after the command, so FILEDIA does not
//! matter) on a new drawing.

use super::*;
use crate::ui::window::import_dialogs::ImportDialogMsg;
use document::EntityType;
use std::sync::Arc;

fn fixture(path: &str) -> String {
    format!("{}/tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// Bytes under a name of their own, as a file picked this session: the
/// importers read registered sources before the disk.
fn register(name: &str, bytes: impl Into<Vec<u8>>) -> String {
    let path = format!("import-command-test-{name}");
    crate::scene::model::pdf_raster::register_source(&path, Arc::new(bytes.into()));
    path
}

fn new_drawing() -> OpenCADStudio {
    let mut app = OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    app
}

/// Runs the command and the tasks it starts (model files are read on a
/// worker thread) to the end, as the automation server does.
fn command(app: &mut OpenCADStudio, cmd: &str) {
    let task = app.update(Message::Command(cmd.to_string()));
    app.drive_headless_task(task)
        .expect("the command runs headless");
}

fn dialog(app: &mut OpenCADStudio, message: ImportDialogMsg) {
    let task = app.update(Message::ImportDialog(message));
    app.drive_headless_task(task)
        .expect("the dialog runs headless");
}

fn last_line(app: &OpenCADStudio) -> String {
    app.command_line
        .history_plain_text()
        .lines()
        .last()
        .unwrap_or_default()
        .to_string()
}

/// What an import can change: the undo steps, the blocks (beyond the two
/// spaces), and the objects in model space, by kind.
#[derive(Debug, PartialEq)]
struct State {
    undo: usize,
    blocks: Vec<String>,
    model: Vec<String>,
}

fn kind(entity: &EntityType) -> String {
    let debug = format!("{entity:?}");
    debug.split('(').next().unwrap_or_default().to_string()
}

fn state(app: &OpenCADStudio) -> State {
    let tab = &app.tabs[app.active_tab];
    let document = &tab.scene.document;
    let model = document
        .block_records
        .get("*Model_Space")
        .map(|b| b.handle)
        .expect("model space");
    State {
        undo: tab.history.undo_stack.len(),
        blocks: document
            .block_records
            .iter()
            .map(|b| b.name.clone())
            .filter(|n| !n.starts_with('*'))
            .collect(),
        model: document
            .entities()
            .filter(|e| e.common().owner_handle == model)
            .map(kind)
            .collect(),
    }
}

/// The blocks the model-space inserts name, in order.
fn inserted(app: &OpenCADStudio) -> Vec<String> {
    let document = &app.tabs[app.active_tab].scene.document;
    let model = document.block_records.get("*Model_Space").map(|b| b.handle);
    document
        .entities()
        .filter_map(|e| match e {
            EntityType::Insert(insert) if Some(insert.common.owner_handle) == model => {
                Some(insert.block_name.clone())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn import_sends_each_file_to_its_importer() {
    // STEP: a block per product, and its insert.
    let mut app = new_drawing();
    command(&mut app, &format!("IMPORT {}", fixture("step/box.step")));
    let after = state(&app);
    assert_eq!(after.undo, 1);
    assert_eq!(after.blocks, ["Box"]);
    assert_eq!(inserted(&app), ["Box"]);
    assert_eq!(last_line(&app), "Import Job Complete");

    // IGES: the solid itself.
    let mut app = new_drawing();
    command(&mut app, &format!("IMPORT {}", fixture("iges/box.igs")));
    assert_eq!(state(&app).model, ["Solid3D"]);
    assert_eq!(last_line(&app), "Import Job Complete");

    // A metafile and an SVG drawing: a block of the picture, inserted.
    for (file, block) in [
        (fixture("wmf/shapes.wmf"), "WMF0"),
        (register("drawing.svg", SQUARE_SVG), "SVG0"),
    ] {
        let mut app = new_drawing();
        command(&mut app, &format!("IMPORT {file}"));
        assert_eq!(state(&app).blocks, [block], "{file}");
        assert_eq!(inserted(&app), [block], "{file}");
        assert_eq!(state(&app).undo, 1, "{file}");
    }

    // 3D Studio and PDF files open their import windows.
    let mut app = new_drawing();
    command(&mut app, &format!("IMPORT {}", fixture("tds/box.3ds")));
    assert_eq!(app.active_modal, Some(ModalKind::TdsImport));
    let mut app = new_drawing();
    command(&mut app, &format!("IMPORT {}", fixture("pdf/inkscape.pdf")));
    assert_eq!(app.active_modal, Some(ModalKind::PdfImportFile));

    // Formats no importer reads, and a file that is not there.
    for (file, message) in [
        (
            register("part.ipt", "x"),
            "Inventor: OpenCAD Studio has no importer for this format.".to_string(),
        ),
        (
            register("data.xyz", "x"),
            "OpenCAD Studio has no importer for this format.".to_string(),
        ),
        (
            register("plan.dgn", "not a design file"),
            "Invalid or unsupported DGN file.".to_string(),
        ),
        (
            "import-command-test-missing.step".to_string(),
            "File import-command-test-missing.step does not exist.".to_string(),
        ),
    ] {
        let mut app = new_drawing();
        let before = state(&app);
        command(&mut app, &format!("IMPORT {file}"));
        assert!(
            last_line(&app).ends_with(&message),
            "{file}: {}",
            last_line(&app)
        );
        assert_eq!(state(&app), before, "{file}");
        assert_eq!(app.active_modal, None, "{file}");
    }
}

/// A square, for the SVG importer.
const SQUARE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="10" y="10" width="50" height="50" fill="none" stroke="black"/></svg>"#;

#[test]
fn a_file_no_importer_reads_says_so_and_changes_nothing() {
    let cases = [
        (
            "bad.step",
            "ISO-10303-21; nothing here",
            "has no model the importer reads.",
        ),
        (
            "bad.igs",
            "not an IGES file",
            "has no model the importer reads.",
        ),
        (
            "bad.wmf",
            "not a metafile",
            "is not a valid Windows metafile.",
        ),
        ("bad.svg", "not an SVG file", "is not a valid SVG file."),
        (
            "empty.svg",
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"></svg>"#,
            "has no shapes to import.",
        ),
        (
            "bad.3ds",
            "not a 3D Studio file",
            "is not a valid 3D Studio file.",
        ),
    ];
    for (name, bytes, message) in cases {
        let mut app = new_drawing();
        let path = register(name, bytes);
        let before = state(&app);
        command(&mut app, &format!("IMPORT {path}"));
        let line = last_line(&app);
        assert!(
            line.contains(&path) && line.ends_with(message),
            "{name}: {line}"
        );
        assert_eq!(state(&app), before, "{name}");
        assert_eq!(app.active_modal, None, "{name}");
    }
}

#[test]
fn iges_solids_import_as_solids() {
    for (file, solids) in [
        ("iges/box.igs", 1),
        ("iges/cylinder.igs", 1),
        ("iges/inch_cube.igs", 1),
        ("iges/two_solids.igs", 2),
    ] {
        let mut app = new_drawing();
        command(&mut app, &format!("IMPORT {}", fixture(file)));
        let after = state(&app);
        assert_eq!(after.model, vec!["Solid3D"; solids], "{file}");
        assert_eq!(after.undo, 1, "{file}");
    }
}

#[test]
fn a_model_import_is_one_undo_step_and_keeps_the_drawings_block_names() {
    let mut app = new_drawing();
    let box_step = fixture("step/box.step");
    command(&mut app, &format!("IMPORT {box_step}"));
    command(&mut app, &format!("IMPORT {box_step}"));
    // The second product named Box gets a name of its own, and its insert
    // follows it.
    assert_eq!(state(&app).blocks, ["Box", "Box_1"]);
    assert_eq!(inserted(&app), ["Box", "Box_1"]);
    assert_eq!(state(&app).undo, 2);
    app.undo_active_tab();
    assert_eq!(inserted(&app), ["Box"]);

    // An assembly: its parts as blocks the assembly's block inserts.
    let mut app = new_drawing();
    command(
        &mut app,
        &format!("IMPORT {}", fixture("step/assembly.step")),
    );
    let after = state(&app);
    assert_eq!(after.blocks, ["Plate", "Post", "Assembly"]);
    assert_eq!(inserted(&app), ["Assembly"]);
    assert_eq!(after.undo, 1);
}

#[test]
fn metafiles_and_svg_drawings_come_in_as_blocks() {
    // IMPORT puts the picture at the view's top-left corner, a pixel or
    // two in.
    let mut app = new_drawing();
    let (centre, size, pixel) = app.tabs[app.active_tab].scene.view_window();
    command(&mut app, &format!("IMPORT {}", fixture("wmf/shapes.wmf")));
    let document = &app.tabs[app.active_tab].scene.document;
    let at = document
        .entities()
        .find_map(|e| match e {
            EntityType::Insert(insert) => Some(insert.insert_point),
            _ => None,
        })
        .expect("an insert");
    assert!((at.x - (centre[0] - 0.5 * size[0] + pixel)).abs() < 1e-9);
    assert!((at.y - (centre[1] + 0.5 * size[1] - 2.0 * pixel)).abs() < 1e-9);

    // WMFIN and SVGIN: the name without its extension is enough; the block
    // is placed by the insertion prompts, and the next one takes the next
    // number.
    let mut app = new_drawing();
    let wmf = register(
        "picture.wmf",
        std::fs::read(fixture("wmf/shapes.wmf")).unwrap(),
    );
    let svg = register("picture.svg", SQUARE_SVG);
    for (cmd, block) in [
        (format!("WMFIN {}", wmf.trim_end_matches(".wmf")), "WMF0"),
        (format!("WMFIN {wmf}"), "WMF1"),
        (format!("SVGIN {}", svg.trim_end_matches(".svg")), "SVG0"),
    ] {
        let before = state(&app);
        command(&mut app, &cmd);
        let after = state(&app);
        assert_eq!(
            after.blocks.last().map(String::as_str),
            Some(block),
            "{cmd}"
        );
        assert_eq!(after.undo, before.undo + 1, "{cmd}");
        assert_eq!(after.model, before.model, "{cmd}: placed by the prompts");
        assert!(app.tabs[app.active_tab].active_cmd.is_some(), "{cmd}");
        app.tabs[app.active_tab].active_cmd = None;
    }

    // A metafile's faces become text styles; one the drawing has is kept.
    let mut app = new_drawing();
    let tab = app.active_tab;
    let mut mine = document::tables::TextStyle::new("WMF-Arial0");
    mine.font_file = "mine.ttf".to_string();
    mine.handle = app.tabs[tab].scene.document.allocate_handle();
    let _ = app.tabs[tab].scene.document.text_styles.add(mine);
    command(&mut app, &format!("IMPORT {}", fixture("wmf/text.wmf")));
    let document = &app.tabs[tab].scene.document;
    let styles: Vec<&str> = document
        .text_styles
        .iter()
        .map(|s| s.name.as_str())
        .filter(|n| n.starts_with("WMF-"))
        .collect();
    assert_eq!(styles, ["WMF-Arial0", "WMF-Times New Roman0"]);
    assert_eq!(
        document
            .text_styles
            .get("WMF-Arial0")
            .map(|s| s.font_file.as_str()),
        Some("mine.ttf")
    );
    let mut used: Vec<&str> = document
        .entities()
        .filter_map(|e| match e {
            EntityType::Text(text) => Some(text.style.as_str()),
            _ => None,
        })
        .collect();
    used.sort();
    assert_eq!(used, ["WMF-Arial0", "WMF-Arial0", "WMF-Times New Roman0"]);
}

#[test]
fn acisin_reads_a_sat_file() {
    // A box, written as SAT by the app's own writer.
    let body = ocs_solid::model::make_box([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]).unwrap();
    let sat = crate::scene::convert::acis_export::solid_to_sat(&body)
        .expect("a box writes")
        .to_sat_string();
    let path = register("box.sat", sat);
    let mut app = new_drawing();
    command(&mut app, &format!("ACISIN {path}"));
    let after = state(&app);
    assert_eq!(after.model, ["Solid3D"]);
    assert_eq!(after.undo, 1);

    // A missing file is looked for in the current directory.
    let mut app = new_drawing();
    let before = state(&app);
    command(&mut app, "ACISIN import-command-test-missing.sat");
    let line = last_line(&app);
    assert!(
        line.contains(
            "\"import-command-test-missing.sat\": file not found in the current directory"
        ),
        "{line}"
    );
    assert_eq!(state(&app), before);
}

/// The 3D Studio Import window's rows: name, kind and material count.
fn rows(app: &OpenCADStudio) -> Vec<(String, &'static str, usize)> {
    app.import_dialogs
        .tds
        .as_ref()
        .map(|s| s.objects.clone())
        .unwrap_or_default()
}

/// Checks the window's rows named `names`, then OK.
fn import_rows(app: &mut OpenCADStudio, names: &[&str]) {
    for (k, (name, _, _)) in rows(app).iter().enumerate() {
        if names.contains(&name.as_str()) {
            dialog(app, ImportDialogMsg::TdsCheck(k, true));
        }
    }
    dialog(app, ImportDialogMsg::TdsOk);
}

fn layers(app: &OpenCADStudio) -> Vec<String> {
    app.tabs[app.active_tab]
        .scene
        .document
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

#[test]
fn a_3d_studio_import_runs_through_its_window() {
    let mut app = new_drawing();
    command(&mut app, &format!("IMPORT {}", fixture("tds/objects.3ds")));
    assert_eq!(app.active_modal, Some(ModalKind::TdsImport));
    // The objects by name, case aside, after the global settings.
    let names: Vec<String> = rows(&app).into_iter().map(|(n, _, _)| n).collect();
    assert_eq!(
        names,
        ["Global Settings", "Alpha", "Beta", "mid/part", "zeta"]
    );
    // OK with nothing checked keeps the window open.
    dialog(&mut app, ImportDialogMsg::TdsOk);
    assert_eq!(app.active_modal, Some(ModalKind::TdsImport));
    assert_eq!(state(&app).undo, 0);
    // Two of the four meshes, each on a layer of its name: one undo step.
    import_rows(&mut app, &["Alpha", "Beta"]);
    assert_eq!(app.active_modal, None);
    assert_eq!(state(&app).undo, 1);
    let made = layers(&app);
    assert!(made.contains(&"ALPHA".to_string()) && made.contains(&"BETA".to_string()));
    assert!(!made.contains(&"ZETA".to_string()), "{made:?}");

    // A camera becomes a named view.
    let mut app = new_drawing();
    command(
        &mut app,
        &format!("IMPORT {}", fixture("tds/lights_camera.3ds")),
    );
    import_rows(&mut app, &["Camera"]);
    let views: Vec<String> = app.tabs[app.active_tab]
        .scene
        .document
        .views
        .iter()
        .map(|v| v.name.clone())
        .collect();
    assert_eq!(views, ["CAMERA"]);

    // A mesh of two materials asks how to take it; split, it comes in on
    // its layer.
    let mut app = new_drawing();
    command(
        &mut app,
        &format!("IMPORT {}", fixture("tds/two_materials.3ds")),
    );
    import_rows(&mut app, &["Crate"]);
    assert_eq!(app.active_modal, Some(ModalKind::TdsMaterial));
    assert_eq!(state(&app).undo, 0);
    dialog(&mut app, ImportDialogMsg::TdsMatChoice(0));
    dialog(&mut app, ImportDialogMsg::TdsMatApply);
    assert_eq!(app.active_modal, None);
    assert_eq!(state(&app).undo, 1);
    assert!(layers(&app).contains(&"CRATE".to_string()));

    // Cancel at the question stops the import at that mesh: the only mesh,
    // so nothing comes in.
    let mut app = new_drawing();
    command(
        &mut app,
        &format!("IMPORT {}", fixture("tds/two_materials.3ds")),
    );
    import_rows(&mut app, &["Crate"]);
    let task = app.update(Message::CloseModal);
    app.drive_headless_task(task).expect("the import task");
    assert_eq!(app.active_modal, None);
    assert!(!layers(&app).contains(&"CRATE".to_string()));
}
