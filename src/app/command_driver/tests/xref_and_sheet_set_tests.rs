#[cfg(test)]
mod xref_and_sheet_set_tests {
    use crate::app::OpenCADStudio;

    fn out_text(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    fn new_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn dimoverride_list_reports_the_override_and_clear_returns_the_style() {
        let mut app = new_app();
        let i = app.active_tab;
        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");
        let dimension = app
            .tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Dimension(dimension) => Some(dimension.base().common.handle),
                _ => None,
            })
            .expect("DIMLINEAR placed a dimension");
        app.tabs[i].scene.select_entities(&[dimension]);

        let _ = app.run_command_line("DIMOVERRIDE_APPLY DIMTXT=4");
        let out = out_text(&app);
        assert!(out.contains("DIMOVERRIDE: updated"), "{out}");

        let _ = app.run_command_line("DIMOVERRIDELIST");
        let out = out_text(&app);
        assert!(out.contains("DIMTXT = 4"), "{out}");
        assert!(out.contains("Dimension"), "{out}");

        let _ = app.run_command_line("DIMOVERRIDE_APPLY CLEAR");
        let _ = app.run_command_line("DIMOVERRIDELIST");
        let out = out_text(&app);
        assert!(out.contains("No overrides"), "{out}");
        // The style itself never changed.
        let style = app.tabs[i]
            .scene
            .document
            .dim_styles
            .get("ISO-25")
            .unwrap();
        assert_eq!(style.dimtxt, 2.5);
    }

    #[test]
    fn mleader_edit_adds_then_removes_a_leader_and_reports_both_vertex_lists() {
        let mut app = new_app();
        let i = app.active_tab;
        let mut leader = document::entities::MultiLeader::with_text(
            "office note",
            document::types::Vector3::new(20.0, 0.0, 0.0),
            vec![
                document::types::Vector3::ZERO,
                document::types::Vector3::new(5.0, 5.0, 0.0),
            ],
        );
        leader.text_height = 0.18;
        leader.context.text_height = 0.18;
        let handle = app.tabs[i]
            .scene
            .add_entity(document::EntityType::MultiLeader(Box::new(leader)));

        // The initial note: one leader, two vertices.
        let q = app.automation_op(&format!(
            r#"{{"op":"query","handle":"{handle:X}","detail":"full"}}"#
        ));
        assert_eq!(q["entities"][0]["leader_count"], 1, "{q}");
        let leaders = q["entities"][0]["leaders"].as_array().unwrap();
        assert_eq!(leaders[0]["points"].as_array().unwrap().len(), 2, "{q}");

        // Add a second leader through the edit command.
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{handle:X}"]}}"#
        ));
        let _ = app.run_command_line("MLEADEREDIT");
        assert!(app.tabs[i].active_cmd.is_some());
        let _ = app.feed_active_cmd("40,10");
        let _ = app.feed_active_cmd("");
        let q = app.automation_op(&format!(
            r#"{{"op":"query","handle":"{handle:X}","detail":"full"}}"#
        ));
        assert_eq!(q["entities"][0]["leader_count"], 2, "{q}");
        let leaders = q["entities"][0]["leaders"].as_array().unwrap();
        assert_eq!(leaders.len(), 2, "{q}");
        let lists: Vec<&Vec<serde_json::Value>> = leaders
            .iter()
            .map(|leader| leader["points"].as_array().unwrap())
            .collect();
        assert_eq!(lists[0].len(), 2, "{q}");
        assert_eq!(lists[1].len(), 1, "{q}");
        assert_ne!(lists[0], lists[1], "{q}");

        // Remove the first leader — one leader remains.
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{handle:X}"]}}"#
        ));
        let _ = app.run_command_line("MLEADEREDIT");
        let _ = app.feed_active_cmd("R");
        let _ = app.feed_active_cmd("0,0");
        let _ = app.feed_active_cmd("");
        let q = app.automation_op(&format!(
            r#"{{"op":"query","handle":"{handle:X}","detail":"full"}}"#
        ));
        assert_eq!(q["entities"][0]["leader_count"], 1, "{q}");
    }

    #[test]
    fn sheet_set_with_a_custom_property_and_a_field() {
        let _sets = crate::entities::field::sheet_sets_one_at_a_time();
        use document::objects::{Dictionary, ObjectType};
        use serde_json::json;

        fn request(app: &mut OpenCADStudio, mut req: serde_json::Value) -> serde_json::Value {
            let state = app.control_state();
            req["protocol"] = json!(1);
            req["document_id"] = state["document_id"].clone();
            req["revision"] = state["revision"].clone();
            req["client_id"] = json!("test");
            if req.get("request_id").is_none() {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                req["request_id"] = json!(format!(
                    "req-{}",
                    NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                ));
            }
            let (r, task) = app.control_request(req.clone());
            app.drive_headless_task(task).unwrap();
            if matches!(r["status"].as_str(), Some("accepted" | "running")) {
                app.control_request(json!({"op":"operation","request_id":req["request_id"]}))
                    .0
            } else {
                r
            }
        }
        let sheet = |app: &mut OpenCADStudio, value: &str| {
            request(app, json!({"op":"action","name":"sheet_set","value":value}))
        };

        let dir = tempfile::tempdir().expect("a temporary directory for the sheet set");

        let mut app = new_app();

        // Create the set through the wizard.
        let _ = sheet(&mut app, "new");
        let _ = sheet(&mut app, "existing=0");
        let _ = sheet(&mut app, "name=PROJECTSET");
        let _ = sheet(&mut app, &format!("folder={}", dir.path().display()));
        let _ = sheet(&mut app, "ok");

        // Two subsets.
        let _ = sheet(&mut app, "menu=new_subset:set");
        let _ = sheet(&mut app, "title=Architectural");
        let _ = sheet(&mut app, "ok");
        let _ = sheet(&mut app, "menu=new_subset:set");
        let _ = sheet(&mut app, "title=Structural");
        let _ = sheet(&mut app, "ok");

        // Three sheets.
        for (subset, number, title) in [
            ("Architectural", "A-101", "Plans"),
            ("Architectural", "A-102", "Sections"),
            ("Structural", "S-201", "Framing"),
        ] {
            let _ = sheet(&mut app, &format!("menu=new_sheet:{subset}"));
            let _ = sheet(&mut app, &format!("number={number}"));
            let _ = sheet(&mut app, &format!("title={title}"));
            let _ = sheet(&mut app, &format!("folder={}", dir.path().display()));
            let _ = sheet(&mut app, "ok");
        }

        // A custom property on the set.
        let _ = sheet(&mut app, "menu=properties:set");
        let _ = sheet(&mut app, "add_custom");
        let _ = sheet(&mut app, "custom_name=PROJECT");
        let _ = sheet(&mut app, "custom_owner=0");
        let _ = sheet(&mut app, "add_custom_ok");
        let _ = sheet(&mut app, "custom=0:The Aragorn Project");
        let _ = sheet(&mut app, "ok");

        // The set round-trips from disk with both subsets, the three sheets
        // and the custom property.
        let dst = dir.path().join("PROJECTSET.dst");
        assert!(dst.is_file(), "the wizard wrote {}", dst.display());
        let db = crate::app::commands::sheet_set::read_set(&dst.to_string_lossy())
            .expect("the set round-trips from disk");
        let subset_count = db
            .sheet_set()
            .children
            .iter()
            .filter(|child| child.name == "AcSmSubset")
            .count();
        let sheet_count = db
            .sheet_set()
            .children
            .iter()
            .filter(|child| child.name == "AcSmSubset")
            .map(|subset| {
                subset
                    .children
                    .iter()
                    .filter(|child| child.name == "AcSmSheet")
                    .count()
            })
            .sum::<usize>();
        assert_eq!(subset_count, 2, "two subsets");
        assert_eq!(sheet_count, 3, "three sheets across the subsets");
        let custom = document::sheet_set::custom_properties(db.sheet_set());
        assert!(
            custom
                .iter()
                .any(|(name, value, _)| name == "PROJECT" && value == "The Aragorn Project"),
            "{custom:?}"
        );

        // A field on the first sheet's layout reads the custom property.
        let sheet_path = dir.path().join("A-101 Plans.dwg");
        assert!(sheet_path.is_file());
        let mut doc = crate::io::load_file(&sheet_path).expect("the sheet drawing loads");
        let host = doc
            .add_entity_to_layout(
                document::EntityType::Point(document::entities::Point::new()),
                "A-101 Plans",
            )
            .expect("the layout accepts a point");
        let root_h = doc.allocate_handle();
        let fields_h = doc.allocate_handle();
        let text_h = doc.allocate_handle();
        let child_h = doc.allocate_handle();
        let mut root = Dictionary::new();
        root.handle = root_h;
        root.owner = host;
        root.add_entry("ACAD_FIELD", fields_h);
        let mut fields = Dictionary::new();
        fields.handle = fields_h;
        fields.owner = root_h;
        fields.add_entry("TEXT", text_h);
        doc.objects.insert(root_h, ObjectType::Dictionary(root));
        doc.objects.insert(fields_h, ObjectType::Dictionary(fields));
        doc.fields.insert(
            text_h,
            document::document::FieldDef {
                handle: text_h,
                owner: fields_h,
                evaluator: "_text".to_string(),
                code: r"%<\_FldIdx 0>%".to_string(),
                objects: vec![],
            },
        );
        doc.fields.insert(
            child_h,
            document::document::FieldDef {
                handle: child_h,
                owner: text_h,
                evaluator: "AcSm".to_string(),
                code: r"\AcSm SheetSet.PROJECT".to_string(),
                objects: vec![],
            },
        );

        // The custom property field reads the set through the open sheet.
        let resolved = crate::entities::field::resolve(&doc, host);
        assert_eq!(
            resolved.as_deref(),
            Some("The Aragorn Project"),
            "the field reads the set's custom property on the sheet's layout"
        );

        // And the other sheet's own field shows its sheet number instead.
        doc.fields.insert(
            child_h,
            document::document::FieldDef {
                handle: child_h,
                owner: text_h,
                evaluator: "AcSm".to_string(),
                code: r"\AcSm Sheet.Number".to_string(),
                objects: vec![],
            },
        );
        let resolved = crate::entities::field::resolve(&doc, host);
        assert_eq!(
            resolved.as_deref(),
            Some("A-101"),
            "the sheet field reads the sheet number"
        );

    }

    fn add_line(app: &mut OpenCADStudio, x1: f64, y1: f64, x2: f64, y2: f64) -> document::Handle {
        use document::entities::Line;
        use document::types::Vector3;
        let i = app.active_tab;
        let doc = &mut app.tabs[i].scene.document;
        let line = Line::from_points(Vector3::new(x1, y1, 0.0), Vector3::new(x2, y2, 0.0));
        doc.add_entity(document::EntityType::Line(line)).unwrap()
    }

    fn reported_lines(app: &OpenCADStudio, before: usize) -> Vec<String> {
        app.command_line.history[before..]
            .iter()
            .map(|entry| entry.text.trim().to_string())
            .collect()
    }

    fn breakdown_sum(said: &[String]) -> usize {
        said.iter()
            .filter_map(|text| {
                let mut parts = text.splitn(2, ' ');
                let (n, rest) = (parts.next()?, parts.next()?);
                n.parse::<usize>()
                    .ok()
                    .filter(|_| !rest.contains("constraint(s) applied"))
            })
            .sum()
    }

    #[test]
    fn autoconstrain_reports_the_breakdown_for_a_rectangle() {
        let mut app = new_app();
        let a = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let b = add_line(&mut app, 100.0, 0.0, 100.0, 50.0);
        let c = add_line(&mut app, 100.0, 50.0, 0.0, 50.0);
        let d = add_line(&mut app, 0.0, 50.0, 0.0, 0.0);
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{a:X}","{b:X}","{c:X}","{d:X}"]}}"#
        ));
        let before = app.command_line.history.len();
        let inferred = app.automation_op(r#"{"op":"run","cmd":"AUTOCONSTRAIN"}"#);
        assert_eq!(inferred["ok"], true, "{inferred}");
        let said = reported_lines(&app, before);
        let out = said.join("\n");
        assert!(out.contains("constraint(s) applied"), "{out}");
        let reported = said
            .iter()
            .find_map(|text| text.split_whitespace().next().and_then(|n| n.parse::<usize>().ok()))
            .expect("the total line prints");
        assert_eq!(breakdown_sum(&said), reported, "{out}");
        assert!(out.contains("Coincident"), "{out}");
        assert!(out.contains("Horizontal"), "{out}");
        assert!(out.contains("Vertical"), "{out}");
    }

    #[test]
    fn autoconstrain_honours_the_enabled_kinds() {
        let mut app = new_app();
        app.auto_constrain_settings
            .enabled
            .retain(|kind| *kind != crate::app::settings::AutoConstraintKind::Coincident);
        let a = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let b = add_line(&mut app, 100.0, 0.0, 100.0, 50.0);
        let c = add_line(&mut app, 100.0, 50.0, 0.0, 50.0);
        let d = add_line(&mut app, 0.0, 50.0, 0.0, 0.0);
        let _ = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{a:X}","{b:X}","{c:X}","{d:X}"]}}"#
        ));
        let before = app.command_line.history.len();
        let inferred = app.automation_op(r#"{"op":"run","cmd":"AUTOCONSTRAIN"}"#);
        assert_eq!(inferred["ok"], true, "{inferred}");
        let said = reported_lines(&app, before);
        let out = said.join("\n");
        assert!(out.contains("constraint(s) applied"), "{out}");
        assert!(!out.contains("Coincident"), "{out}");
        assert!(out.contains("Horizontal"), "{out}");
        assert!(out.contains("Vertical"), "{out}");
    }

    #[test]
    fn autoconstrain_honours_the_distance_tolerance() {
        // Two lines whose endpoints are 0.02 apart: within the default
        // tolerance they become coincident; with a 1e-6 tolerance they do not.
        for (tolerance, expect_coincident) in [(0.05, true), (1e-6, false)] {
            let mut app = new_app();
            app.auto_constrain_settings.distance_tolerance = tolerance;
            let a = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
            let b = add_line(&mut app, 100.0, 0.02, 200.0, 0.0);
            let _ = app.automation_op(&format!(
                r#"{{"op":"select","handles":["{a:X}","{b:X}"]}}"#
            ));
            let before = app.command_line.history.len();
            let inferred = app.automation_op(r#"{"op":"run","cmd":"AUTOCONSTRAIN"}"#);
            assert_eq!(inferred["ok"], true, "{inferred}");
            let said = reported_lines(&app, before);
            let out = said.join("\n");
            assert_eq!(out.contains("Coincident"), expect_coincident, "{out}");
        }
    }

    /// A path inside a fresh temporary directory. The returned `TempDir`
    /// keeps the directory alive for the whole test and removes it on drop.
    fn temp_named(stem: &str, extension: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("{stem}.{extension}"));
        (dir, path)
    }

    /// A fresh tab as a host drawing referencing `source`; returns the tab
    /// index and the reference's block-record handle.
    fn host_with_xref(
        app: &mut OpenCADStudio,
        source: &std::path::Path,
    ) -> (usize, document::Handle) {
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(
                app.tabs.len(),
            ));
        let idx = app.tabs.len() - 1;
        app.active_tab = idx;
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("-XREF");
        // The attach reads the reference off the UI thread: drive each task.
        for input in ["ATTACH", &source.display().to_string(), "0,0", "1", "", ""] {
            let task = app.feed_active_cmd(input);
            app.drive_headless_task(task).expect("the attach's task");
        }
        let record = app.tabs[idx]
            .scene
            .document
            .block_records
            .iter()
            .find(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .map(|br| br.handle)
            .expect("the host carries the reference");
        (idx, record)
    }

    fn referenced_entity_count(
        app: &OpenCADStudio,
        tab: usize,
        record: document::Handle,
    ) -> usize {
        app.tabs[tab]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == record)
            .count()
    }

    #[test]
    fn saving_the_source_reloads_the_host_reference() {
        let (_source_dir, source) = temp_named("m071-src", "dwg");
        let mut app = new_app();

        // Backslashes would escape out of the JSON string; the resolver
        // normalises forward slashes.
        let source_fwd = source.display().to_string().replace('\\', "/");
        // Tab 0: the source drawing with one line.
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));

        let (host, record) = host_with_xref(&mut app, &source);
        assert_eq!(referenced_entity_count(&app, host, record), 1);

        // A second tab opens the source, gains a line and saves — the
        // host must reload the reference from disk.
        app.tabs
            .push(crate::app::tab_document::DocumentTab::new_drawing(9));
        let source_tab = app.tabs.len() - 1;
        app.active_tab = source_tab;
        let _ = app.automation_op(&format!(r#"{{"op":"open","path":"{source_fwd}"}}"#));
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 20,0"}"#);
        let _ = app.automation_op(r#"{"op":"save"}"#);

        assert_eq!(
            referenced_entity_count(&app, host, record),
            2,
            "the host reloaded the saved source"
        );
    }

    #[test]
    fn xopen_asks_to_pick_a_reference_and_resolves_it_under_the_point() {
        let (_source_dir, source) = temp_named("m071-pick", "dwg");
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let source_fwd = source.display().to_string().replace('\\', "/");
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));

        let (host, _record) = host_with_xref(&mut app, &source);
        // No selection: XOPEN asks for a pick.
        let _ = app.run_command_line("XOPEN");
        assert!(
            app.tabs[host].active_cmd.is_some(),
            "the pick command starts"
        );
        let out = out_text(&app);
        assert!(out.contains("Select an external reference"), "{out}");
        // A pick on the reference under the point resolves to its source.
        let _ = app.feed_active_cmd("2.5,0");
        let out = out_text(&app);
        assert!(out.contains("XOPEN: opening"), "{out}");
        assert!(out.contains(&source.display().to_string()), "{out}");
    }

    #[test]
    fn xopen_reports_a_missing_source_and_leaves_the_host_alone() {
        let (_source_dir, source) = temp_named("m071-missing", "dwg");
        let mut app = new_app();

        let _ = app.automation_op(r#"{"op":"new"}"#);
        let source_fwd = source.display().to_string().replace('\\', "/");
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));

        let (host, record) = host_with_xref(&mut app, &source);
        std::fs::remove_file(&source).expect("the source is removed for the test");

        let insert = app.tabs[host]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(insert) => Some(insert.common.handle),
                _ => None,
            })
            .expect("the host carries the reference insert");
        let _ = app.automation_op(&format!(r#"{{"op":"select","handles":["{insert:X}"]}}"#));
        let before = referenced_entity_count(&app, host, record);
        let _ = app.run_command_line("XOPEN");
        let out = out_text(&app);
        assert!(out.contains("cannot find"), "{out}");
        assert_eq!(
            referenced_entity_count(&app, host, record),
            before,
            "the missing-file refusal leaves the reference alone"
        );
    }

    use crate::command::CmdResult;
    use crate::scene::parametric_constraints::{ConstraintKind, ParametricRef, ParametricScope};

    /// M098: a constrained pair draws its constraint bar. The horizontal and
    /// coincident glyphs of the pair are placed and listed, so showing the bar
    /// surfaces what constrains the geometry.
    #[test]
    fn constraint_bar_lists_the_glyphs_of_a_constrained_pair() {
        let mut app = new_app();
        let i = app.active_tab;
        let first = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let second = add_line(&mut app, 40.0, 0.0, 60.0, 0.0);
        let scope = ParametricScope::ModelSpace;
        let set = app.tabs[i].scene.parametric_constraint_set_mut(scope);
        let horizontal = set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(first), ParametricRef::whole(second)],
            None,
        );
        let coincident = set.add(
            ConstraintKind::Coincident,
            vec![ParametricRef::whole(first), ParametricRef::whole(second)],
            None,
        );
        app.tabs[i].scene.note_parametric_constraint_applied(scope, horizontal, 3);
        app.tabs[i].scene.note_parametric_constraint_applied(scope, coincident, 3);
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);

        let placements = app.tabs[i].scene.constraint_glyph_placements_screen(
            scope,
            (800.0, 600.0),
            true,
            3,
            4095,
        );
        let labels: Vec<&str> = placements.iter().map(|p| p.3.as_str()).collect();
        assert!(!placements.is_empty(), "a constrained pair draws a bar");
        assert!(labels.iter().any(|label| *label == "—"), "{labels:?}");
        assert!(labels.iter().any(|label| *label == "≡"), "{labels:?}");
    }

    /// M098: hiding the bar removes its glyphs, and deleting the constraint
    /// the bar named frees it again — the bar's own remove path.
    #[test]
    fn constraint_bar_hide_removes_the_glyphs_and_delete_frees_the_constraint() {
        let mut app = new_app();
        let i = app.active_tab;
        let first = add_line(&mut app, 0.0, 0.0, 100.0, 0.0);
        let _ = app.apply_cmd_result(CmdResult::AddParametricConstraint {
            kind: ConstraintKind::Horizontal,
            refs: vec![ParametricRef::whole(first)],
            driving_param: None,
            label: "Horizontal constraint",
        });
        let scope = ParametricScope::ModelSpace;
        let id = app.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .unwrap()
            .constraints[0]
            .id;

        let hidden = app.tabs[i].scene.set_parametric_constraint_visibility(
            scope,
            None,
            false,
            false,
        );
        assert_eq!(hidden, 1);
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        assert!(app.tabs[i]
            .scene
            .constraint_glyph_placements_screen(scope, (800.0, 600.0), true, 3, 4095)
            .is_empty());

        app.delete_parametric_constraint(id);
        assert!(app.tabs[i]
            .scene
            .parametric_constraint_set(scope)
            .unwrap()
            .constraints
            .is_empty());
        app.undo_steps(1);
        assert_eq!(
            app.tabs[i]
                .scene
                .parametric_constraint_set(scope)
                .unwrap()
                .constraints
                .len(),
            1
        );
    }

    /// The live status of one reference as the automation surface reports it.
    fn xref_status(app: &OpenCADStudio, name: &str) -> String {
        app.control_state()["xrefs"]
            .as_array()
            .and_then(|xs| xs.iter().find(|x| x["name"].as_str() == Some(name)))
            .and_then(|x| x["status"].as_str())
            .unwrap_or_default()
            .to_string()
    }

    /// The name of the host's external reference block record.
    fn xref_name(app: &OpenCADStudio, tab: usize) -> String {
        app.tabs[tab]
            .scene
            .document
            .block_records
            .iter()
            .find(|br| br.flags.is_xref || br.flags.is_xref_overlay)
            .map(|br| br.name.clone())
            .expect("the host carries a reference")
    }

    /// A saved one-line source drawing and a host that references it, so the
    /// reference loads from disk. Returns the app, the source path, the host
    /// tab, the reference's block record and its name.
    fn m074_host(
        stem: &str,
    ) -> (
        OpenCADStudio,
        tempfile::TempDir,
        std::path::PathBuf,
        usize,
        document::Handle,
        String,
    ) {
        let (source_dir, source) = temp_named(stem, "dwg");
        let mut app = new_app();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let source_fwd = source.display().to_string().replace('\\', "/");
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 10,0"}"#);
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{source_fwd}"}}"#));
        let (host, record) = host_with_xref(&mut app, &source);
        let name = xref_name(&app, host);
        // Save the host so it has a folder: a relative reference path only
        // resolves (and only reloads) against a saved drawing.
        let host_path = source.with_file_name(format!("{stem}-host.dwg"));
        let host_fwd = host_path.display().to_string().replace('\\', "/");
        app.active_tab = host;
        let _ = app.automation_op(&format!(r#"{{"op":"save","path":"{host_fwd}"}}"#));
        (app, source_dir, source, host, record, name)
    }

    /// M074: unloading a reference reports the unloaded status and hides its
    /// merged geometry.
    #[test]
    fn xref_unload_reports_status_and_hides_geometry() {
        let (mut app, _source_dir, _source, host, record, name) = m074_host("m074-unload");
        assert_eq!(referenced_entity_count(&app, host, record), 1);
        assert_eq!(xref_status(&app, &name), "loaded");

        let _ = app.run_command_line(&format!("XREF Unload {name}"));
        let out = out_text(&app);
        assert!(out.contains("unloaded"), "{out}");
        assert_eq!(xref_status(&app, &name), "unloaded");
        assert_eq!(
            referenced_entity_count(&app, host, record),
            0,
            "the unloaded reference's merged geometry is hidden"
        );
    }

    /// M074: reloading clears the unloaded status and restores the geometry.
    #[test]
    fn xref_reload_reports_status_and_restores_geometry() {
        let (mut app, _source_dir, _source, host, record, name) = m074_host("m074-reload");
        let _ = app.run_command_line(&format!("XREF Unload {name}"));
        assert_eq!(xref_status(&app, &name), "unloaded");

        let _ = app.run_command_line(&format!("XREF Reload {name}"));
        let out = out_text(&app);
        assert!(out.contains("Reloaded"), "{out}");
        assert_eq!(xref_status(&app, &name), "loaded");
        assert_eq!(
            referenced_entity_count(&app, host, record),
            1,
            "the reloaded reference's geometry is back"
        );
    }

    /// M074: a source that disappears is reported not-found, on demand.
    #[test]
    fn xref_status_reports_a_missing_source() {
        let (mut app, _source_dir, source, _host, _record, name) = m074_host("m074-missing");
        assert_eq!(xref_status(&app, &name), "loaded");

        std::fs::remove_file(&source).expect("the source file is removed");
        let _ = app.run_command_line("XREF ?");
        let out = out_text(&app);
        assert!(out.contains("Not found"), "{out}");
        assert!(out.contains("not found"), "{out}");
        assert_eq!(xref_status(&app, &name), "not_found");
    }

    /// A Text hosting a whole-text FILENAME field, so its value is the
    /// drawing's file name. The drawing is named first so the field resolves.
    fn file_name_field_text(app: &mut OpenCADStudio, name: &str) -> document::Handle {
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path = Some(format!(r"C:\work\{name}.dwg"));
        let text = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Text(
                document::entities::Text::with_value("OLD", document::types::Vector3::ZERO),
            ));
        let _ = app.run_command_line(&format!("-FIELD {:X} FILENAME", text.value()));
        text
    }

    fn field_text_value(app: &OpenCADStudio, handle: document::Handle) -> String {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Text(text)) => text.value.clone(),
            _ => panic!("the field host disappeared"),
        }
    }

    /// M043: FIELDEVAL masks the automatic events, while UPDATEFIELD always
    /// runs. With every bit cleared the save event evaluates nothing and the
    /// cached text survives; the explicit command still catches it up.
    #[test]
    fn fieldeval_zero_masks_automatic_updates_but_not_updatefield() {
        let mut app = new_app();
        let i = app.active_tab;
        let text = file_name_field_text(&mut app, "ONE");
        assert_eq!(field_text_value(&app, text), "ONE.dwg");

        let _ = app.run_command_line("FIELDEVAL 0");
        let out = out_text(&app);
        assert_eq!(
            crate::entities::field::fieldeval(&app.tabs[i].scene.document),
            0,
            "{out}"
        );

        app.tabs[i].scene.document.source_path = Some(r"C:\work\TWO.dwg".into());
        assert_eq!(
            app.tabs[i].scene.update_fields(2, None),
            (0, 0),
            "the save event is masked off"
        );
        assert_eq!(field_text_value(&app, text), "ONE.dwg");

        // UPDATEFIELD is explicit: the mask never applies to it.
        app.tabs[i].scene.select_entities(&[text]);
        let _ = app.run_command_line("UPDATEFIELD");
        let out = out_text(&app);
        assert!(out.contains("field(s) found"), "{out}");
        assert_eq!(field_text_value(&app, text), "TWO.dwg", "{out}");
    }

    /// M043: the mask is per event. A save-only drawing keeps its cached text
    /// at open time and catches up through the real save path.
    #[test]
    fn fieldeval_save_bit_skips_the_open_event() {
        let mut app = new_app();
        let i = app.active_tab;
        let text = file_name_field_text(&mut app, "ONE");
        // The SETVAR spelling takes the same path as the bare variable.
        let _ = app.run_command_line("SETVAR FIELDEVAL 2");
        assert_eq!(
            crate::entities::field::fieldeval(&app.tabs[i].scene.document),
            2
        );

        app.tabs[i].scene.document.source_path = Some(r"C:\work\THREE.dwg".into());
        assert_eq!(
            app.tabs[i].scene.update_fields(1, None),
            (0, 0),
            "open is masked off"
        );
        assert_eq!(field_text_value(&app, text), "ONE.dwg");

        // The save path is allowed and the value follows the written file.
        let (_path_dir, path) = temp_named("m043-save", "dwg");
        let forward = path.display().to_string().replace('\\', "/");
        let saved = app.automation_op(&format!(r#"{{"op":"save","path":"{forward}"}}"#));
        assert_eq!(saved["ok"], true, "{saved}");
        assert_eq!(field_text_value(&app, text), "m043-save.dwg");

        // The file on disk carries its own name too: reopening must not be
        // what corrects a title block that shipped with the previous one.
        let bytes = std::fs::read(&path).expect("the saved drawing is readable");
        let reopened =
            crate::io::load_bytes_finalized(&path, bytes).expect("the saved drawing reparses");
        let stored = reopened
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Text(text) => Some(text.value.clone()),
                _ => None,
            })
            .expect("the saved drawing keeps its text");
        assert_eq!(stored, "m043-save.dwg");
    }

    /// M072: IMAGEATTACH takes a path on the command line and places the
    /// picture without the file picker; the insertion-point prompt follows as
    /// after a dropped file.
    #[test]
    fn imageattach_takes_a_path_from_the_command_line() {
        use base64::Engine as _;
        let mut app = new_app();
        let (_path_dir, path) = temp_named("m072-pic", "png");
        let png = base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==")
            .expect("the fixture is valid base64");
        std::fs::write(&path, png).expect("the picture is written");

        let _ = app.run_command_line(&format!("IMAGEATTACH \"{}\"", path.display()));
        let out = out_text(&app);
        assert!(out.contains("1×1 px"), "{out}");
        assert!(out.contains("Specify insertion point"), "{out}");

        let _ = app.feed_active_cmd("0,0");
        let _ = app.feed_active_cmd("5,0");
        let after = out_text(&app);
        let image = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::RasterImage(image) => Some(image.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the command placed a raster image; said: {after}"));
        assert_eq!(image.file_path, path.to_string_lossy());
        // One pixel wide placed 5 units across: the u-vector is one pixel.
        let world_width = image.u_vector.length() * image.size.x;
        assert!((world_width - 5.0).abs() < 1.0e-6, "{world_width}");
    }

    /// A table with a title row merged across both columns. Columns are 20
    /// wide and rows 10 high; the insertion point is the world origin.
    fn merged_title_table(app: &mut OpenCADStudio) -> document::Handle {
        use document::entities::table::CellRange;
        let mut table = document::entities::Table::new(document::types::Vector3::ZERO, 3, 2);
        for column in &mut table.columns {
            column.width = 20.0;
        }
        for row in &mut table.rows {
            row.height = 10.0;
        }
        table.merge_cells(CellRange::new(0, 0, 0, 1));
        table.cell_mut(0, 0).unwrap().set_text("before");
        app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Table(Box::new(table)))
    }

    /// M047: a merged cell is one cell. A TABLEDIT that lands in the covered
    /// half must write the merged cell the drawing shows, not the hidden one
    /// underneath it; the in-place editor and the command-line form have to
    /// agree on which cell is being edited.
    #[test]
    fn tabledit_in_a_merged_cell_edits_the_merged_cell() {
        let mut app = new_app();
        let i = app.active_tab;
        let handle = merged_title_table(&mut app);

        // (30, -5) is the right half of the merged title row: column 1 is
        // covered by the merge that starts in column 0.
        let _ = app.run_command_line("TABLEDIT 30,-5 after");
        let out = out_text(&app);
        assert!(out.contains("TABLE CELL  Enter text for [0,0]"), "{out}");

        let Some(document::EntityType::Table(table)) =
            app.tabs[i].scene.document.get_entity(handle)
        else {
            panic!("the table disappeared");
        };
        assert_eq!(
            table.cell_text(0, 0),
            Some("after"),
            "the merged cell takes the text"
        );
        assert_eq!(
            table.cell_text(0, 1),
            Some(""),
            "the covered cell must not hold the text"
        );
    }
}
