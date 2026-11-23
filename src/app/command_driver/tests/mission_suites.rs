use super::super::*;
use super::all_output;

#[cfg(test)]
mod mission_m025_tests {
    use super::super::super::*;
    use super::all_output;

    #[test]
    fn annotation_and_hatch_variables_read_and_write_through_setvar() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("SETVAR CANNOSCALE 1:75");
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .header
                .current_annotation_scale,
            "1:75"
        );
        let _ = app.run_command_line("SETVAR CANNOSCALE");
        assert!(all_output(&app).contains("CANNOSCALE <\"1:75\">"));

        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 0");
        assert!(!app.tabs[app.active_tab].scene.annotation_all_visible());
        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 1");
        assert!(app.tabs[app.active_tab].scene.annotation_all_visible());
        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE");
        assert!(all_output(&app).contains("ANNOALLVISIBLE <1>"));

        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE 2");
        assert_eq!(app.annotation_auto_scale, 2);
        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE");
        assert!(all_output(&app).contains("ANNOAUTOSCALE <2>"));

        let _ = app.run_command_line("SETVAR HPLAYER");
        assert!(all_output(&app).contains("HPLAYER = ."));
    }

    #[test]
    fn annotation_variables_reject_out_of_range_values_with_a_typed_error() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("SETVAR ANNOAUTOSCALE 9");
        assert_eq!(app.annotation_auto_scale, -4);
        assert!(all_output(&app).contains("Requires an integer between -4 and 4."));

        let _ = app.run_command_line("SETVAR ANNOALLVISIBLE 2");
        assert!(all_output(&app).contains("Requires 0 or 1 only."));
    }
}

#[cfg(test)]
mod mission_m027_tests {
    use super::super::super::*;
    use super::all_output;
    use document::entities::Line;
    use document::types::Vector3;

    #[test]
    fn annoupdate_gathers_a_selection_instead_of_refusing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));

        let _ = app.run_command_line("ANNOUPDATE");
        assert!(
            app.tabs[tab].active_cmd.is_some(),
            "ANNOUPDATE must prompt for a selection, not error immediately"
        );
        assert!(!all_output(&app).contains("select annotation objects first"));

        let _ = app.run_command_line("ALL");
        assert!(app.tabs[tab].active_cmd.is_none());
    }

    #[test]
    fn annoupdate_all_explains_when_nothing_is_annotative() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));

        let _ = app.run_command_line("ANNOUPDATE ALL");
        assert!(all_output(&app).contains("1 found 0 were updated"));
    }

    #[test]
    fn annoupdate_on_a_preselected_non_annotative_object_is_a_no_op_with_reason() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let line = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )));
        app.tabs[tab].scene.select_entity(line, false);

        let before = format!("{:?}", app.tabs[tab].scene.document.get_entity(line));
        let _ = app.run_command_line("ANNOUPDATE");
        let output = all_output(&app);
        assert!(output.contains("Select objects:"), "{output}");
        assert_eq!(
            format!("{:?}", app.tabs[tab].scene.document.get_entity(line)),
            before
        );
    }
}

#[cfg(test)]
mod mission_m029_tests {
    use super::super::super::*;
    use super::all_output;

    #[test]
    fn scale_list_reports_ratio_and_in_use_count() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("SCALELISTEDIT ADD 1:75");
        let _ = app.run_command_line("CANNOSCALE 1:75");
        let _ = app.run_command_line("SCALELISTEDIT LIST");
        let output = all_output(&app);
        assert!(output.contains("1:75"), "{output}");
        assert!(output.contains("in use: 0"), "{output}");
        assert!(output.contains("(current)"), "{output}");
    }

    /// `SCALELISTEDIT ADD <name> <ratio>` adds a scale under its own name;
    /// the ratio alone still names the scale after itself.
    #[test]
    fn scale_list_add_takes_a_name_before_the_ratio() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("SCALELISTEDIT ADD Site_plan 1:500");
        let _ = app.run_command_line("SCALELISTEDIT ADD 1:75");
        let scales = app.tabs[app.active_tab].scene.scale_list();
        let names: Vec<&str> = scales.iter().map(|(name, ..)| name.as_str()).collect();
        assert!(names.contains(&"Site_plan"), "{names:?}");
        assert!(names.contains(&"1:75"), "{names:?}");
        let _ = app.run_command_line("CANNOSCALE Site_plan");
        let _ = app.run_command_line("SCALELISTEDIT LIST");
        let output = all_output(&app);
        assert!(
            output.contains("Site_plan  factor=0.002000  in use: 0  (current)"),
            "{output}"
        );
        let _ = app.run_command_line("SCALELISTEDIT ADD Site_plan 1:500");
        assert!(all_output(&app).contains("Scale Site_plan already exists."));
    }

    #[test]
    fn deleting_the_current_scale_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("SCALELISTEDIT ADD 1:75");
        let _ = app.run_command_line("CANNOSCALE 1:75");
        let _ = app.run_command_line("SCALELISTEDIT DELETE 1:75");
        let output = all_output(&app);
        assert!(output.contains("Cannot delete the current annotation scale"));
        assert_eq!(
            app.tabs[app.active_tab]
                .scene
                .document
                .header
                .current_annotation_scale,
            "1:75"
        );
    }

    #[test]
    fn an_unused_scale_can_be_deleted_and_disappears_from_the_list() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("SCALELISTEDIT ADD 1:75");
        let _ = app.run_command_line("SCALELISTEDIT DELETE 1:75");
        let _ = app.run_command_line("SCALELISTEDIT LIST");
        let output = all_output(&app);
        assert!(output.contains("Removed annotation scale 1:75"), "{output}");
        assert!(!output.contains("1:75  factor"), "{output}");
    }
}

#[cfg(test)]
mod mission_m051_tests {
    use super::super::super::*;
    use super::all_output;
    use document::entities::{Line, Text};
    use document::types::Vector3;

    fn add_note(app: &mut OpenCADStudio) -> (document::Handle, document::Handle) {
        let i = app.active_tab;
        let mut text = Text::with_value("NOTE", Vector3::ZERO);
        text.height = 2.0;
        let text = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Text(text));
        let line = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::new(-5.0, 0.5, 0.0),
                Vector3::new(15.0, 0.5, 0.0),
            )));
        (text, line)
    }

    fn masks(app: &OpenCADStudio) -> Vec<document::Handle> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, document::EntityType::Wipeout(_)))
            .map(|entity| entity.common().handle)
            .collect()
    }

    fn bounds(app: &OpenCADStudio, handle: document::Handle) -> ([f32; 3], [f32; 3]) {
        let (mut min, mut max) = ([f32::MAX; 3], [f32::MIN; 3]);
        for wire in app.tabs[app.active_tab].scene.wire_models_for(&[handle]) {
            for point in &wire.points {
                for k in 0..3 {
                    min[k] = min[k].min(point[k]);
                    max[k] = max[k].max(point[k]);
                }
            }
        }
        (min, max)
    }

    fn mask_selection(app: &mut OpenCADStudio) {
        let _ = app.run_command_line("TEXTMASK");
        let _ = app.feed_command(crate::command::StepInput::Enter);
    }

    #[test]
    fn masking_text_adds_a_wipeout_and_off_removes_it_again() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (text, _line) = add_note(&mut app);
        let i = app.active_tab;

        assert!(masks(&app).is_empty());
        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        assert_eq!(masks(&app).len(), 1, "{}", all_output(&app));

        app.tabs[i].scene.select_entity(text, false);
        let _ = app.run_command_line("TEXTMASK OFF");
        assert!(masks(&app).is_empty(), "{}", all_output(&app));
        app.tabs[i].scene.select_entity(text, false);
        let record = app.tabs[i]
            .scene
            .document
            .get_entity(text)
            .unwrap()
            .common()
            .extended_data
            .get_record("TEXTMASK");
        assert!(record.is_none(), "the association must be dropped on OFF");
    }

    #[test]
    fn a_larger_offset_grows_the_masked_area() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (text, _line) = add_note(&mut app);
        let i = app.active_tab;

        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        let before = masks(&app);
        assert_eq!(before.len(), 1);
        let (min_before, max_before) = bounds(&app, before[0]);

        let _ = app.run_command_line("TEXTMASK OFFSET 0.5");
        assert!(
            all_output(&app).contains("offset = 0.5"),
            "{}",
            all_output(&app)
        );
        assert_eq!(
            crate::io::drawing_variable(&app.tabs[i].scene.document, "TEXTMASKOFFSET").as_deref(),
            Some("0.5")
        );
        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        let after = masks(&app);
        assert_eq!(after.len(), 1, "the old mask must be replaced, not stacked");
        let (min_after, max_after) = bounds(&app, after[0]);

        assert!(
            (max_after[0] - min_after[0]) > (max_before[0] - min_before[0]) + 0.2,
            "offset 0.5 must grow the mask: before {:?}, after {:?}",
            (min_before, max_before),
            (min_after, max_after)
        );
        assert!((min_after[0] - (min_before[0] - 0.2)).abs() < 1e-3);
    }

    #[test]
    fn off_without_a_selection_clears_every_mask_in_the_drawing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (text, _line) = add_note(&mut app);
        let i = app.active_tab;

        app.tabs[i].scene.select_entity(text, false);
        mask_selection(&mut app);
        app.tabs[i].scene.deselect_all();
        assert_eq!(masks(&app).len(), 1);

        let _ = app.run_command_line("TEXTMASK OFF");
        assert!(masks(&app).is_empty());
        assert!(all_output(&app).contains("removed 1 mask"));
    }
}

#[cfg(test)]
mod mission_m062_tests {
    use super::super::super::*;
    use super::all_output;
    use document::entities::Line;
    use document::types::Vector3;

    fn temp_pdf(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("ocs_m062_{tag}.pdf"));
        (dir, path)
    }

    #[test]
    fn exporting_the_current_layout_writes_a_real_pdf_and_reports_it() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(100.0, 100.0, 0.0),
            )));
        let (_path_guard, path) = temp_pdf("current");

        let _ = app.run_command_line(&format!("EXPORTPDF {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Exported:"), "{output}");
        assert!(output.contains("1 page(s)"), "{output}");
        assert!(output.contains("mm"), "{output}");
        let bytes = std::fs::read(&path).expect("the command must create the file");
        assert!(bytes.len() > 100, "a real PDF, not a stub: {output}");
        assert!(
            bytes.starts_with(b"%PDF"),
            "the file must carry a PDF header"
        );
    }

    #[test]
    fn exporting_all_layouts_reports_the_page_count() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(50.0, 25.0, 0.0),
            )));
        app.tabs[i].scene.add_layout("Sheet1").unwrap();
        app.tabs[i].scene.initialize_layout_page_setup("Sheet1");
        app.tabs[i].scene.add_layout_default_viewport("Sheet1");
        let expected_pages = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .filter(|name| name != "Model")
            .count();
        let (_path_guard, path) = temp_pdf("all");

        let _ = app.run_command_line(&format!("EXPORTPDF {} ALL", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Exported:"), "{output}");
        assert!(
            output.contains(&format!("{expected_pages} page(s)")),
            "expected {expected_pages} paper layouts: {output}"
        );
        let bytes = std::fs::read(&path).expect("the command must create the file");
        assert!(bytes.starts_with(b"%PDF"));
    }

    #[test]
    fn a_missing_output_path_is_refused_with_usage_text() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("EXPORTPDF ");
        assert!(all_output(&app).contains("Usage: EXPORTPDF"));
    }

    fn temp_dwf(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let (dir, path) = temp_pdf(tag);
        (dir, path.with_extension("dwf"))
    }

    #[test]
    fn exporting_the_current_layout_to_dwf_writes_it_as_a_sheet() {
        use crate::scene::model::underlay_vector::dwf as reader;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(100.0, 100.0, 0.0),
            )));
        let layout = app.tabs[i].scene.current_layout.clone();
        let (_path_guard, path) = temp_dwf("current");

        let _ = app.run_command_line(&format!("EXPORTDWF {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Exported:"), "{output}");
        assert!(output.contains("1 sheet(s)"), "{output}");
        assert!(output.contains("mm"), "{output}");
        let bytes = std::fs::read(&path).expect("the command must create the file");
        assert!(bytes.starts_with(b"(DWF V06.00)"), "the package tag");
        assert_eq!(reader::sheet_names(&bytes), Some(vec![layout.clone()]));
        let sheet = reader::sheet(&bytes, &layout).expect("the sheet opens");
        assert!(
            sheet.paths.iter().any(|path| path.stroke.is_some()),
            "the line is on the sheet: {output}"
        );
    }

    #[test]
    fn exporting_all_layouts_to_dwf_names_a_sheet_after_each() {
        use crate::scene::model::underlay_vector::dwf as reader;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(50.0, 25.0, 0.0),
            )));
        app.tabs[i].scene.add_layout("Sheet1").unwrap();
        app.tabs[i].scene.initialize_layout_page_setup("Sheet1");
        app.tabs[i].scene.add_layout_default_viewport("Sheet1");
        let layouts: Vec<String> = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .filter(|name| name != "Model")
            .collect();
        let (_path_guard, path) = temp_dwf("all");

        let _ = app.run_command_line(&format!("EXPORTDWF {} ALL", path.display()));

        let output = all_output(&app);
        assert!(
            output.contains(&format!("{} sheet(s)", layouts.len())),
            "one sheet for each paper layout: {output}"
        );
        let bytes = std::fs::read(&path).expect("the command must create the file");
        let _ = std::fs::remove_file(&path);
        assert_eq!(reader::sheet_names(&bytes), Some(layouts));
    }

    #[test]
    fn a_missing_dwf_path_is_refused_with_usage_text() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("EXPORTDWF ");
        assert!(all_output(&app).contains("Usage: EXPORTDWF"));
    }
}

#[cfg(test)]
mod mission_m060_tests {
    use super::super::super::*;
    use super::all_output;

    fn temp_pdf(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("ocs_m060_{tag}.pdf"));
        (dir, path)
    }

    fn paper_layouts(app: &mut OpenCADStudio) -> Vec<String> {
        let i = app.active_tab;
        let names: Vec<String> = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .filter(|name| name != "Model")
            .collect();
        for name in &names {
            app.tabs[i].scene.initialize_layout_page_setup(name);
        }
        names
    }

    #[test]
    fn publishing_three_layouts_writes_one_pdf_with_three_pages() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.add_layout("Sheet1").unwrap();
        app.tabs[i].scene.add_layout("Sheet2").unwrap();
        let names = paper_layouts(&mut app);
        assert_eq!(names.len(), 3, "expected three paper layouts: {names:?}");
        let (_path_guard, path) = temp_pdf("three");

        let _ = app.run_command_line(&format!("PUBLISH {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Publishing 3 sheet(s)"), "{output}");
        assert!(
            output.contains("Published:") && output.contains("3 page(s)"),
            "{output}"
        );
        for name in &names {
            assert!(output.contains(&format!("  {name}:")), "{output}");
        }
        let bytes = std::fs::read(&path).expect("publish must create the PDF");
        assert!(bytes.starts_with(b"%PDF"), "a real PDF was written");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn choosing_two_sheets_publishes_two_pages() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.add_layout("Sheet2").unwrap();
        app.tabs[i].scene.add_layout("Sheet3").unwrap();
        let names = paper_layouts(&mut app);
        let chosen = [names[0].clone(), names[1].clone()];
        let (_path_guard, path) = temp_pdf("two");

        let _ = app.run_command_line(&format!(
            "PUBLISH {} {},{}",
            path.display(),
            chosen[0],
            chosen[1]
        ));

        let output = all_output(&app);
        assert!(output.contains("Publishing 2 sheet(s)"), "{output}");
        assert!(output.contains("2 page(s)"), "{output}");
        assert!(!output.contains(&format!("  {}:", names[2])), "{output}");
        let bytes = std::fs::read(&path).expect("publish must create the PDF");
        assert!(bytes.starts_with(b"%PDF"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unknown_sheet_name_is_refused_before_writing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        paper_layouts(&mut app);
        let (_path_guard, path) = temp_pdf("unknown");

        let _ = app.run_command_line(&format!("PUBLISH {} MISSING-SHEET", path.display()));

        let output = all_output(&app);
        assert!(
            output.contains("no layout named 'MISSING-SHEET'"),
            "{output}"
        );
        assert!(
            !path.exists(),
            "nothing should be written on an unknown sheet"
        );
    }
}

#[cfg(test)]
mod mission_m042_tests {
    use super::super::super::*;
    use super::all_output;
    use document::entities::Text;
    use document::types::Vector3;

    fn add_text(app: &mut OpenCADStudio) -> document::Handle {
        let i = app.active_tab;
        let text = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Text(Text::with_value(
                "OLD",
                Vector3::ZERO,
            )));
        text
    }

    fn text_value(app: &OpenCADStudio, handle: document::Handle) -> String {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Text(text)) => text.value.clone(),
            Some(document::EntityType::MText(text)) => text.value.clone(),
            _ => panic!("text host disappeared"),
        }
    }

    #[test]
    fn a_filename_field_is_inserted_and_follows_the_drawing_name() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.source_path = Some(r"C:\work\TITLEBLOCK.dwg".to_string());
        let text = add_text(&mut app);

        let _ = app.run_command_line(&format!("-FIELD {:X} FILENAME", text.value()));

        let output = all_output(&app);
        assert!(output.contains(r"\AcVar Filename"), "{output}");
        assert_eq!(text_value(&app, text), "TITLEBLOCK.dwg", "{output}");
        let (code, _) = crate::entities::field::whole_text_field(&app.tabs[i].scene.document, text)
            .expect("the text must carry a field expression");
        assert!(code.contains("Filename"), "{code}");

        app.tabs[i].scene.document.source_path = Some(r"C:\work\PLAN.dwg".to_string());
        let (changed, _) = crate::entities::field::update_fields(
            &mut app.tabs[i].scene.document,
            32,
            Some(&[text]),
        );
        assert_eq!(changed, vec![text]);
        assert_eq!(text_value(&app, text), "PLAN.dwg");
    }

    #[test]
    fn a_date_field_uses_the_requested_format() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let text = add_text(&mut app);

        let _ = app.run_command_line(&format!(r#"-FIELD {:X} DATE "yyyy/MM/dd""#, text.value()));

        let output = all_output(&app);
        assert!(output.contains("yyyy/MM/dd"), "{output}");
        let value = text_value(&app, text);
        let bytes = value.as_bytes();
        assert_eq!(bytes.len(), 10, "date value: {value}");
        assert_eq!(bytes[4], b'/');
        assert_eq!(bytes[7], b'/');
        assert!(
            value.chars().filter(char::is_ascii_digit).count() == 8,
            "{value}"
        );
    }

    #[test]
    fn the_expression_can_be_read_back_without_changing_it() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let text = add_text(&mut app);
        let _ = app.run_command_line(&format!("-FIELD {:X} FILENAME", text.value()));
        let before = text_value(&app, text);

        let _ = app.run_command_line(&format!("-FIELD {:X}", text.value()));

        let output = all_output(&app);
        assert!(output.contains("Field: %<"), "{output}");
        assert!(output.contains(r"\AcVar Filename"), "{output}");
        assert_eq!(text_value(&app, text), before);
    }
}

#[cfg(test)]
mod mission_m055_tests {
    use super::super::super::*;
    use super::all_output;

    fn enter_layout(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let name = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        app.tabs[i].scene.set_current_layout(name);
        app.tabs[i].scene.load_current_layout_state();
    }

    fn viewport_rects(app: &OpenCADStudio) -> Vec<(i64, i64, i64, i64)> {
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        let mut rects: Vec<(i64, i64, i64, i64)> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some((
                        (vp.center.x * 1000.0).round() as i64,
                        (vp.center.y * 1000.0).round() as i64,
                        (vp.width * 1000.0).round() as i64,
                        (vp.height * 1000.0).round() as i64,
                    ))
                }
                _ => None,
            })
            .collect();
        rects.sort();
        rects
    }

    #[test]
    fn four_viewports_can_be_saved_and_restored_after_deleting_two() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);

        let _ = app.run_command_line("VPORTS 4");
        let _ = app.feed_active_cmd("F");
        assert_eq!(viewport_rects(&app).len(), 4, "{}", all_output(&app));
        let saved = viewport_rects(&app);

        let _ = app.run_command_line("VPORTS SAVE FOUR");
        assert!(all_output(&app).contains("saved configuration 'FOUR'"));
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        let two: Vec<document::Handle> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .take(2)
            .collect();
        app.tabs[i].scene.erase_entities(&two);
        assert_eq!(viewport_rects(&app).len(), 2);

        let _ = app.run_command_line("VPORTS RESTORE FOUR");
        assert!(all_output(&app).contains("restored 'FOUR' with 4 viewport(s)"));
        assert_eq!(
            viewport_rects(&app),
            saved,
            "restore must recreate the same rectangles"
        );
    }

    #[test]
    fn saved_configurations_are_listed_and_can_be_deleted() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS 2V");
        let _ = app.feed_active_cmd("F");
        let _ = app.run_command_line("VPORTS SAVE TWO");

        let _ = app.run_command_line("VPORTS LIST");
        let output = all_output(&app);
        assert!(output.contains("TWO  (2 viewport(s))"), "{output}");

        let _ = app.run_command_line("VPORTS DELETE TWO");
        let output = all_output(&app);
        assert!(output.contains("deleted 'TWO'"), "{output}");
        let before = app.command_line.history.len();
        let _ = app.run_command_line("VPORTS LIST");
        let listed = app.command_line.history[before..]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!listed.contains("TWO  (2 viewport(s))"), "{listed}");
        assert!(
            listed.contains("No saved viewport configurations."),
            "{listed}"
        );

        let _ = app.run_command_line("VPORTS RESTORE TWO");
        assert!(all_output(&app).contains("no saved configuration named 'TWO'"));
    }

    #[test]
    fn restoring_an_unknown_configuration_names_the_missing_one() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);

        let _ = app.run_command_line("VPORTS RESTORE NOPE");

        assert!(all_output(&app).contains("no saved configuration named 'NOPE'"));
    }
}

#[cfg(test)]
mod mission_m058_tests {
    use super::super::super::*;
    use super::all_output;

    fn enter_layout(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        let name = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        app.tabs[i].scene.set_current_layout(name);
        app.tabs[i].scene.load_current_layout_state();
    }

    fn viewports(app: &OpenCADStudio) -> Vec<document::Handle> {
        let i = app.active_tab;
        let layout_block = app.tabs[i].scene.current_layout_block_handle_pub();
        app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Viewport(vp)
                    if vp.id > 1 && vp.common.owner_handle == layout_block =>
                {
                    Some(vp.common.handle)
                }
                _ => None,
            })
            .collect()
    }

    fn vp(app: &OpenCADStudio, handle: document::Handle) -> &document::entities::Viewport {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::Viewport(vp)) => vp,
            _ => panic!("viewport missing"),
        }
    }

    #[test]
    fn maximizing_keeps_the_scale_and_restoring_puts_the_viewport_back() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS 2H");
        let _ = app.feed_active_cmd("F");
        let handles = viewports(&app);
        assert_eq!(handles.len(), 2, "{}", all_output(&app));
        let (target, other) = (handles[0], handles[1]);
        let i = app.active_tab;

        let ((x0, y0), (x1, y1)) = app.tabs[i].scene.printable_area_limits().unwrap();
        if let Some(document::EntityType::Viewport(vp)) =
            app.tabs[i].scene.document.get_entity_mut(target)
        {
            vp.custom_scale = 0.02; // 1:50
            vp.view_height = 0.0;
            vp.status.locked = true;
        }
        app.tabs[i].scene.active_viewport = Some(target);
        let before = vp(&app, target).clone();
        let other_before = vp(&app, other).clone();

        let _ = app.run_command_line("VPMAX");

        let output = all_output(&app);
        assert!(output.contains("scale 1:50"), "{output}");
        let maximized = vp(&app, target);
        assert!((maximized.width - (x1 - x0)).abs() < 1e-6);
        assert!((maximized.height - (y1 - y0)).abs() < 1e-6);
        assert!((maximized.center.x - (x0 + x1) / 2.0).abs() < 1e-6);
        assert!(maximized.status.locked, "the lock state must survive");
        assert!(
            (crate::scene::vp_effective_scale(
                maximized.custom_scale,
                maximized.view_height,
                maximized.height
            ) - 0.02)
                .abs()
                < 1e-9
        );

        let _ = app.run_command_line("VPMIN");

        let restored = vp(&app, target);
        assert!((restored.width - before.width).abs() < 1e-9);
        assert!((restored.height - before.height).abs() < 1e-9);
        assert!((restored.center.x - before.center.x).abs() < 1e-9);
        assert!((restored.center.z - before.center.z).abs() < 1e-9);
        assert!((restored.view_target.x - before.view_target.x).abs() < 1e-9);
        assert!(restored.status.locked);
        assert!(
            (crate::scene::vp_effective_scale(
                restored.custom_scale,
                restored.view_height,
                restored.height
            ) - 0.02)
                .abs()
                < 1e-9
        );
        assert_eq!(app.tabs[i].scene.active_viewport, Some(target));
        let other_after = vp(&app, other);
        assert!((other_after.width - other_before.width).abs() < 1e-9);
        assert!((other_after.center.x - other_before.center.x).abs() < 1e-9);
    }

    #[test]
    fn restoring_without_maximizing_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        enter_layout(&mut app);
        let _ = app.run_command_line("VPORTS SINGLE");
        let _ = app.feed_active_cmd("F");

        let _ = app.run_command_line("VPMIN");

        assert!(all_output(&app).contains("no viewport is maximized"));
    }

    #[test]
    fn maximizing_in_model_space_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);

        let _ = app.run_command_line("VPMAX");

        assert!(all_output(&app).contains("switch to a paper layout first"));
    }
}

#[cfg(test)]
mod mission_m059_tests {
    use super::super::super::*;
    use super::all_output;

    fn open_layout(app: &mut OpenCADStudio, name: &str) {
        let i = app.active_tab;
        app.tabs[i].scene.set_current_layout(name.to_string());
        app.tabs[i].scene.load_current_layout_state();
    }

    fn set_sheet(app: &mut OpenCADStudio, width: f64, height: f64, style: &str) {
        let i = app.active_tab;
        let layout = app.tabs[i].scene.current_layout.clone();
        let mut ps = app.tabs[i]
            .scene
            .effective_plot_settings()
            .expect("the layout has plot settings");
        ps.paper_width = width;
        ps.paper_height = height;
        ps.current_style_sheet = style.to_string();
        ps.paper_size = format!("TEST_{width}x{height}");
        app.tabs[i].scene.set_layout_plot_settings(&layout, &ps);
    }

    #[test]
    fn two_named_setups_can_be_saved_listed_and_applied() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        app.tabs[i].scene.add_layout("Sheet2").unwrap();
        app.tabs[i].scene.initialize_layout_page_setup("Sheet2");
        open_layout(&mut app, &first);
        set_sheet(&mut app, 420.0, 297.0, "mono.ctb");
        let _ = app.run_command_line("-PAGESETUP SAVE A3-MONO");

        open_layout(&mut app, "Sheet2");
        set_sheet(&mut app, 841.0, 594.0, "colour.ctb");
        let _ = app.run_command_line("-PAGESETUP SAVE A1-COLOUR");

        let _ = app.run_command_line("-PAGESETUP LIST");
        let output = all_output(&app);
        assert!(
            output.contains("A3-MONO: 420.0×297.0 mm  style=mono.ctb"),
            "{output}"
        );
        assert!(
            output.contains("A1-COLOUR: 841.0×594.0 mm  style=colour.ctb"),
            "{output}"
        );

        let _ = app.run_command_line("-PAGESETUP APPLY A3-MONO");
        let applied = app.tabs[i].scene.effective_plot_settings().unwrap();
        assert!((applied.paper_width - 420.0).abs() < 1e-9, "{applied:?}");
        assert!((applied.paper_height - 297.0).abs() < 1e-9, "{applied:?}");
        assert_eq!(applied.current_style_sheet, "mono.ctb");
        let output = all_output(&app);
        assert!(output.contains("applied 'A3-MONO' to Sheet2"), "{output}");
    }

    #[test]
    fn a_setup_can_be_renamed_and_deleted() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        open_layout(&mut app, &first);
        set_sheet(&mut app, 420.0, 297.0, "mono.ctb");
        let _ = app.run_command_line("-PAGESETUP SAVE OLD");

        let _ = app.run_command_line("-PAGESETUP RENAME OLD NEW");
        assert_eq!(
            app.tabs[i].scene.page_setup_names(),
            vec!["NEW".to_string()]
        );
        assert!(app.tabs[i].scene.page_setup_get("NEW").is_some());

        let _ = app.run_command_line("-PAGESETUP DELETE NEW");
        assert!(app.tabs[i].scene.page_setup_get("NEW").is_none());
        assert!(all_output(&app).contains("deleted 'NEW'"));
    }

    #[test]
    fn applying_an_unknown_setup_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = app.tabs[i]
            .scene
            .layout_names()
            .into_iter()
            .find(|name| name != "Model")
            .expect("a paper layout");
        open_layout(&mut app, &first);

        let _ = app.run_command_line("-PAGESETUP APPLY NOPE");

        assert!(all_output(&app).contains("no setup named 'NOPE'"));
    }
}

#[cfg(test)]
mod mission_m061_tests {
    use super::super::super::*;
    use super::all_output;
    use document::entities::Line;
    use document::types::Vector3;

    fn temp_svg(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("ocs_m061_{tag}.svg"));
        (dir, path)
    }

    fn add_diagonal(app: &mut OpenCADStudio) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(100.0, 60.0, 0.0),
            )));
    }

    fn viewbox(svg: &str) -> Option<(f64, f64)> {
        let start = svg.find("viewBox=\"")? + "viewBox=\"".len();
        let rest = &svg[start..];
        let end = rest.find('"')?;
        let mut numbers = rest[..end].split_whitespace();
        let _x = numbers.next()?.parse::<f64>().ok()?;
        let _y = numbers.next()?.parse::<f64>().ok()?;
        let width = numbers.next()?.parse::<f64>().ok()?;
        let height = numbers.next()?.parse::<f64>().ok()?;
        Some((width, height))
    }

    #[test]
    fn preview_writes_a_real_svg_with_the_layout_paper_aspect() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        add_diagonal(&mut app);
        let (_path_guard, path) = temp_svg("model");

        let _ = app.run_command_line(&format!("PREVIEW {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("Preview:"), "{output}");
        assert!(output.contains("297.0×210.0 mm"), "{output}");
        let svg = std::fs::read_to_string(&path).expect("the preview file must exist");
        assert!(svg.contains("<svg"), "a real SVG, not a stub");
        let (width, height) = viewbox(&svg).expect("the SVG has a viewBox");
        assert!((width - 297.0).abs() < 0.5, "viewBox width: {width}");
        assert!((height - 210.0).abs() < 0.5, "viewBox height: {height}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn preview_follows_the_layout_page_setup() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        add_diagonal(&mut app);
        app.tabs[i].scene.add_layout("Sheet1").unwrap();
        app.tabs[i].scene.initialize_layout_page_setup("Sheet1");
        app.tabs[i].scene.set_current_layout("Sheet1".into());
        app.tabs[i].scene.load_current_layout_state();
        let layout = app.tabs[i].scene.current_layout.clone();
        let mut ps = app.tabs[i].scene.effective_plot_settings().unwrap();
        ps.paper_size = "ISO_A3_(297.00_x_420.00_MM)".to_string();
        ps.paper_width = 297.0;
        ps.paper_height = 420.0;
        ps.rotation = document::objects::PlotRotation::Degrees90;
        app.tabs[i].scene.set_layout_plot_settings(&layout, &ps);
        let (_path_guard, path) = temp_svg("a3");

        let _ = app.run_command_line(&format!("PREVIEW {}", path.display()));

        let output = all_output(&app);
        assert!(output.contains("420.0×297.0 mm"), "{output}");
        let svg = std::fs::read_to_string(&path).expect("the preview file must exist");
        let (width, height) = viewbox(&svg).expect("the SVG has a viewBox");
        assert!(
            (width / height - 420.0 / 297.0).abs() < 0.01,
            "{width}x{height}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_preview_of_empty_space_is_refused() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (_path_guard, path) = temp_svg("empty");

        let _ = app.run_command_line(&format!("PREVIEW {}", path.display()));

        assert!(all_output(&app).contains("Nothing to preview"));
        assert!(!path.exists());
    }
}

#[cfg(test)]
mod mission_m070_tests {
    use super::super::super::*;
    use super::all_output;

    fn fixture_dir(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("ocs_m070_{tag}"));
        std::fs::create_dir_all(&path).expect("the temporary directory");
        (dir, path)
    }

    fn write_source(dir: &std::path::Path) {
        let mut source = document::Document::new();
        source
            .layers
            .add(document::tables::Layer::new("WALLS"))
            .unwrap();
        source
            .layers
            .add(document::tables::Layer::new("HIDDEN"))
            .unwrap();
        source
            .text_styles
            .add(document::tables::TextStyle::new("NOTE"))
            .unwrap();
        let mut title = document::tables::BlockRecord::new("TITLE");
        title.handle = source.allocate_handle();
        let title_handle = title.handle;
        source.block_records.add(title).unwrap();
        let mut line = document::entities::Line::new();
        line.common.layer = "WALLS".to_string();
        line.common.owner_handle = title_handle;
        source.add_entity(document::EntityType::Line(line)).unwrap();
        let mut text = document::entities::Text::new();
        text.style = "NOTE".to_string();
        text.common.owner_handle = title_handle;
        source.add_entity(document::EntityType::Text(text)).unwrap();
        let bytes = crate::io::save_to_bytes(&source, "dwg", source.version).unwrap();
        std::fs::write(dir.join("plan.dwg"), bytes).unwrap();
    }

    fn attach(app: &mut OpenCADStudio, dir: &std::path::Path) -> u64 {
        let i = app.active_tab;
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        let mut record = document::tables::BlockRecord::new("PLAN");
        record.flags.is_xref = true;
        record.xref_path = "plan.dwg".to_string();
        record.handle = app.tabs[i].scene.document.allocate_handle();
        let key = record.handle.value();
        app.tabs[i]
            .scene
            .document
            .block_records
            .add(record)
            .unwrap();
        key
    }

    #[test]
    fn a_layer_and_a_block_can_be_bound_from_the_command_line() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (_dir_guard, dir) = fixture_dir("cmd");
        write_source(&dir);
        let _ = attach(&mut app, &dir);
        let i = app.active_tab;

        let _ = app.run_command_line("XBIND PLAN LAYER WALLS");
        let output = all_output(&app);
        assert!(output.contains("layer WALLS -> PLAN$0$WALLS"), "{output}");
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("PLAN$0$WALLS")
            .is_some());
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("PLAN$0$HIDDEN")
            .is_none());
        assert!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get("PLAN")
                .unwrap()
                .flags
                .is_xref
        );

        let _ = app.run_command_line("XBIND PLAN BLOCK TITLE");
        let output = all_output(&app);
        assert!(output.contains("block TITLE -> PLAN$0$TITLE"), "{output}");
        let block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN$0$TITLE")
            .expect("bound block")
            .clone();
        let owned = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| entity.common().owner_handle == block.handle)
            .count();
        assert_eq!(owned, 2, "{output}");
        assert!(app.tabs[i]
            .scene
            .document
            .text_styles
            .get("PLAN$0$NOTE")
            .is_some());

        let _ = app.run_command_line("XBIND PLAN");
        let output = all_output(&app);
        assert!(output.contains("Bindable symbols in \"PLAN\""), "{output}");
        assert!(output.contains("layer WALLS"), "{output}");
        assert!(output.contains("block TITLE"), "{output}");

        let _ = app.run_command_line("XBIND PLAN LAYER NOPE");
        assert!(all_output(&app).contains("no layer matching 'NOPE'"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unknown_reference_is_refused_and_nothing_is_copied() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let (_dir_guard, dir) = fixture_dir("missingref");
        write_source(&dir);
        let _ = attach(&mut app, &dir);
        let i = app.active_tab;

        let _ = app.run_command_line("XBIND GHOST LAYER WALLS");

        assert!(all_output(&app).contains("no reference matches 'GHOST'"));
        assert!(!app.tabs[i]
            .scene
            .document
            .layers
            .names()
            .any(|name| name.contains("$0$")));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod mission_m076_tests {
    use super::super::super::*;
    use super::all_output;

    /// A layer state keys each row by layer handle, so a test layer needs a
    /// real one — exactly what the LAYER command does when it creates a row.
    fn add_layer(app: &mut OpenCADStudio, name: &str) {
        let i = app.active_tab;
        let mut layer = document::tables::Layer::new(name);
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.layers.add(layer).unwrap();
    }

    fn layer_off(app: &OpenCADStudio, name: &str) -> bool {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get(name)
            .expect("layer")
            .flags
            .off
    }

    fn set_off(app: &mut OpenCADStudio, name: &str, off: bool) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut(name)
            .expect("layer")
            .flags
            .off = off;
    }

    fn set_color(app: &mut OpenCADStudio, name: &str, index: u8) {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut(name)
            .expect("layer")
            .color = document::types::Color::Index(index);
    }

    fn layer_color(app: &OpenCADStudio, name: &str) -> document::types::Color {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get(name)
            .expect("layer")
            .color
    }

    fn temp_las(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("ocs_m076_{tag}.las"));
        (dir, path)
    }

    #[test]
    fn three_layers_turned_off_come_back_on() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        for name in ["WALLS", "DOORS", "WINDOWS"] {
            add_layer(&mut app, name);
        }

        let _ = app.run_command_line("-LAYERSTATE SAVE ALL-ON");
        assert!(all_output(&app).contains("saved \"ALL-ON\""));

        for name in ["WALLS", "DOORS", "WINDOWS"] {
            set_off(&mut app, name, true);
        }
        let _ = app.run_command_line("-LAYERSTATE RESTORE ALL-ON");

        for name in ["WALLS", "DOORS", "WINDOWS"] {
            assert!(!layer_off(&app, name), "{name} must come back on");
        }
        let output = all_output(&app);
        assert!(output.contains("restored \"ALL-ON\""), "{output}");
        assert!(
            app.active_modal.is_none(),
            "-LAYERSTATE must not open the dialog"
        );
    }

    #[test]
    fn a_restore_filter_touches_only_the_named_properties() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        add_layer(&mut app, "WALLS");
        set_color(&mut app, "WALLS", 1);
        let _ = app.run_command_line("-LAYERSTATE SAVE PALETTE");

        set_color(&mut app, "WALLS", 5);
        set_off(&mut app, "WALLS", true);

        let _ = app.run_command_line("-LAYERSTATE RESTORE PALETTE ON");
        assert!(!layer_off(&app, "WALLS"));
        assert_eq!(layer_color(&app, "WALLS"), document::types::Color::Index(5));

        let _ = app.run_command_line("-LAYERSTATE RESTORE PALETTE COLOR");
        assert_eq!(layer_color(&app, "WALLS"), document::types::Color::Index(1));
        assert!(!layer_off(&app, "WALLS"));

        let _ = app.run_command_line("-LAYERSTATE RESTORE PALETTE NONSENSE");
        assert!(all_output(&app).contains("unknown restore property 'NONSENSE'"));
    }

    #[test]
    fn a_state_can_be_renamed_and_round_tripped_through_a_las_file() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        add_layer(&mut app, "WALLS");
        set_off(&mut app, "WALLS", true);
        let _ = app.run_command_line("-LAYERSTATE SAVE OLD-NAME");
        let (_path_guard, path) = temp_las("roundtrip");

        let _ = app.run_command_line(&format!("-LAYERSTATE EXPORT OLD-NAME {}", path.display()));
        assert!(path.exists(), "{}", all_output(&app));
        let _ = app.run_command_line("-LAYERSTATE RENAME OLD-NAME NEW-NAME");
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("NEW-NAME")
            .is_some());
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("OLD-NAME")
            .is_none());

        let _ = app.run_command_line("-LAYERSTATE DELETE NEW-NAME");
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("NEW-NAME")
            .is_none());
        let task = app.run_command_line(&format!("-LAYERSTATE IMPORT {}", path.display()));
        app.drive_headless_task(task).expect("the read task");
        let imported = app.tabs[app.active_tab]
            .scene
            .document
            .layer_state("OLD-NAME")
            .expect("the exported state returns from the file");
        assert_eq!(imported.layers.len(), 2, "layer 0 and WALLS");
        assert!(imported
            .layers
            .iter()
            .any(|layer| layer.layer_name == "WALLS" && layer.off));

        set_off(&mut app, "WALLS", false);
        let _ = app.run_command_line("-LAYERSTATE RESTORE OLD-NAME");
        assert!(layer_off(&app, "WALLS"), "{}", all_output(&app));
        std::fs::remove_file(&path).ok();
    }
}

#[cfg(test)]
mod mission_m078_tests {
    use super::super::super::*;
    use super::all_output;

    fn temp_dir(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("ocs_m078_{tag}"));
        std::fs::create_dir_all(&path).expect("the temporary directory");
        (dir, path)
    }

    /// A layer a translation can act on needs a real handle.
    fn add_layer(app: &mut OpenCADStudio, name: &str) {
        let i = app.active_tab;
        let mut layer = document::tables::Layer::new(name);
        layer.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.layers.add(layer).unwrap();
    }

    fn add_line(app: &mut OpenCADStudio, layer: &str) {
        let i = app.active_tab;
        let mut line = document::entities::Line::new();
        line.common.layer = layer.to_string();
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::Line(line));
    }

    fn line_layers(app: &OpenCADStudio) -> Vec<String> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(_) => Some(entity.common().layer.clone()),
                _ => None,
            })
            .collect()
    }

    /// A standards drawing holding the office layer names.
    fn write_standards(dir: &std::path::Path, layers: &[&str]) -> std::path::PathBuf {
        let mut document = document::Document::new();
        for name in layers {
            document
                .layers
                .add(document::tables::Layer::new(*name))
                .unwrap();
        }
        let bytes = crate::io::save_to_bytes(&document, "dwg", document.version).unwrap();
        let path = dir.join("office.dwg");
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_mapping_moves_objects_and_purges_the_source() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let (_dir_guard, dir) = temp_dir("apply");
        let standards = write_standards(&dir, &["WALLS"]);
        let i = app.active_tab;
        add_layer(&mut app, "A-WALL");
        add_line(&mut app, "A-WALL");
        add_line(&mut app, "0");

        let task = app.run_command_line(&format!("-LAYTRANS {} A-WALL=WALLS", standards.display()));
        app.drive_headless_task(task).expect("the read task");

        let output = all_output(&app);
        assert!(
            output.contains("LAYTRANS: \"A-WALL\" -> \"WALLS\" (1 object(s))."),
            "{output}"
        );
        assert!(
            output.contains("translated 1 layer(s), 1 object(s)"),
            "{output}"
        );
        assert_eq!(
            line_layers(&app),
            vec!["WALLS".to_string(), "0".to_string()]
        );
        assert!(app.tabs[i].scene.document.layers.get("WALLS").is_some());
        assert!(
            app.tabs[i].scene.document.layers.get("A-WALL").is_none(),
            "the empty source layer is purged"
        );
        assert!(
            app.active_modal.is_none(),
            "-LAYTRANS must not open the dialog"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_saved_mapping_list_can_be_reused_on_another_drawing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let (_dir_guard, dir) = temp_dir("reuse");
        let standards = write_standards(&dir, &["WALLS", "DOORS"]);
        let map_file = dir.join("office.map");

        let _ = app.run_command_line(&format!(
            "-LAYTRANS SAVE {} A-WALL=WALLS A-DOOR=DOORS",
            map_file.display()
        ));
        assert!(map_file.exists(), "{}", all_output(&app));
        let text = std::fs::read_to_string(&map_file).unwrap();
        assert!(text.contains("A-WALL -> WALLS"), "{text}");

        let task = app.run_command_line(&format!("-LAYTRANS LIST {}", standards.display()));
        app.drive_headless_task(task).expect("the read task");
        let output = all_output(&app);
        assert!(output.contains("target layer(s)"), "{output}");
        assert!(output.contains("  WALLS"), "{output}");
        assert!(output.contains("  DOORS"), "{output}");

        // Another drawing, same mapping list, no dialog in the way.
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        add_layer(&mut app, "A-WALL");
        add_layer(&mut app, "A-DOOR");
        add_line(&mut app, "A-WALL");
        add_line(&mut app, "A-DOOR");

        let task = app.run_command_line(&format!(
            "-LAYTRANS LOAD {} {}",
            map_file.display(),
            standards.display()
        ));
        app.drive_headless_task(task).expect("the read task");
        let output = all_output(&app);
        assert!(
            output.contains("translated 2 layer(s), 2 object(s)"),
            "{output}"
        );
        let mut layers = line_layers(&app);
        layers.sort();
        assert_eq!(layers, vec!["DOORS".to_string(), "WALLS".to_string()]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_target_the_standards_do_not_define_is_reported_and_nothing_moves() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let (_dir_guard, dir) = temp_dir("missing");
        let standards = write_standards(&dir, &["WALLS"]);
        add_layer(&mut app, "A-WALL");
        add_line(&mut app, "A-WALL");

        let task = app.run_command_line(&format!("-LAYTRANS {} A-WALL=GHOST", standards.display()));
        app.drive_headless_task(task).expect("the read task");

        let output = all_output(&app);
        assert!(
            output.contains("target layer is not in the loaded set"),
            "{output}"
        );
        assert_eq!(line_layers(&app), vec!["A-WALL".to_string()]);
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get("A-WALL")
            .is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}
