#[cfg(test)]
#[path = "trim_hatch_tests.rs"]
mod trim_hatch_tests;

use super::*;

/// All command-line history text joined by newlines. Kept at module scope so
/// the behaviour suites below do not each carry a copy.
fn all_output(app: &OpenCADStudio) -> String {
    app.command_line
        .history
        .iter()
        .map(|e| e.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}


#[cfg(test)]
mod command_replacement_tests {
    use super::super::*;
    use document::entities::{Circle, Line};
    use document::types::Vector3;

    #[test]
    fn one_to_one_edits_keep_identity_but_type_changes_do_not() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )));
        let owner = app.tabs[tab]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
            .common()
            .owner_handle;

        let kept = app.replace_command_entity(
            tab,
            handle,
            vec![document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(3.0, 0.0, 0.0),
            ))],
        );
        assert_eq!(kept, vec![handle]);
        let edited = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(edited.common().owner_handle, owner);
        assert!(matches!(edited, document::EntityType::Line(line) if line.end.x == 3.0));

        let allocated = app.replace_command_entity(
            tab,
            handle,
            vec![document::EntityType::Circle(Circle::from_coords(
                0.0, 0.0, 0.0, 2.0,
            ))],
        );
        assert_eq!(allocated.len(), 1);
        assert_ne!(allocated[0], handle);
        assert!(app.tabs[tab].scene.document.get_entity(handle).is_none());
    }

    #[test]
    fn live_geometry_updates_keep_document_identity_and_display_properties() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let tab = app.active_tab;
        let handle = app.tabs[tab]
            .scene
            .add_entity(document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(1.0, 0.0, 0.0),
            )));
        let expected = {
            let entity = app.tabs[tab].scene.document.get_entity_mut(handle).unwrap();
            entity.common_mut().layer = "LIVE".to_string();
            entity.common_mut().invisible = true;
            entity.common_mut().linetype_scale = 2.5;
            entity.common().clone()
        };

        let _ = app.apply_cmd_result(CmdResult::UpdateLiveEntity {
            handle,
            entity: document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(4.0, 0.0, 0.0),
            )),
            finish: false,
        });

        let updated = app.tabs[tab].scene.document.get_entity(handle).unwrap();
        assert_eq!(updated.common().handle, expected.handle);
        assert_eq!(updated.common().owner_handle, expected.owner_handle);
        assert_eq!(updated.common().layer, expected.layer);
        assert_eq!(updated.common().invisible, expected.invisible);
        assert_eq!(updated.common().linetype_scale, expected.linetype_scale);
        assert!(matches!(updated, document::EntityType::Line(line) if line.end.x == 4.0));
    }
}

#[cfg(test)]
mod trim_extend_history_tests;

#[cfg(test)]
mod parametric_constraint_undo_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
mod delobj_tests {
    use super::super::*;
    use document::types::Vector3;

    fn sweep_source_presence(value: i16, mode: crate::command::ExtrudeMode) -> (bool, bool) {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut circle = document::Circle::new();
        circle.radius = 2.0;
        let profile = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Circle(circle));
        let path =
            app.tabs[i]
                .scene
                .add_entity(document::EntityType::Line(document::Line::from_points(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 5.0),
                )));
        app.delete_objects = value;

        let _ = app.apply_cmd_result(CmdResult::SweepEntities {
            handles: vec![profile],
            path_handle: path,
            mode,
            options: crate::command::SweepOptions::default(),
            color: [1.0; 4],
            expressions: [None, None],
        });

        let created = app.tabs[i]
            .scene
            .document
            .entities()
            .any(|entity| match mode {
                crate::command::ExtrudeMode::Solid => {
                    matches!(entity, document::EntityType::Solid3D(_))
                }
                crate::command::ExtrudeMode::Surface => {
                    matches!(entity, document::EntityType::Surface(_))
                }
            });
        assert!(created, "SWEEP must produce the requested result");
        (
            app.tabs[i].scene.document.get_entity(profile).is_some(),
            app.tabs[i].scene.document.get_entity(path).is_some(),
        )
    }

    #[test]
    fn sweep_applies_all_four_source_deletion_modes() {
        let solid_expectations = [(true, true), (false, true), (false, false), (false, false)];
        for (value, expected) in solid_expectations.into_iter().enumerate() {
            assert_eq!(
                sweep_source_presence(value as i16, crate::command::ExtrudeMode::Solid),
                expected,
            );
        }
        // A swept surface is associative by default (SURFACEASSOCIATIVITY 1)
        // and keeps the profile and path it follows, whatever DELOBJ says;
        // DELOBJ reaches surfaces only with associativity off, a profile-wide
        // switch this parallel test leaves alone.
        assert!(crate::scene::model::sweep_model::surface_associativity());
        for value in [2, 3] {
            assert_eq!(
                sweep_source_presence(value, crate::command::ExtrudeMode::Surface),
                (true, true),
            );
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod thicken_tests;

#[cfg(test)]
mod dispatched_command_task_tests;

/// BEDIT behavior characterization tests.
#[cfg(test)]
mod bedit_characterization_tests;

#[cfg(test)]
mod block_base_point_tests;

#[cfg(test)]
mod sweep_path_end_tests;

#[cfg(test)]
mod copytolayer_tests;

#[cfg(test)]
mod selection_history_tests {
    use crate::app::OpenCADStudio;
    use crate::command::StepInput;
    use document::entities::Line;
    use document::types::Vector3;

    fn three_lines() -> (OpenCADStudio, Vec<document::Handle>) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handles = (0..3)
            .map(|n| {
                app.tabs[i]
                    .scene
                    .add_entity(document::EntityType::Line(Line::from_points(
                        Vector3::new(n as f64 * 10.0, 0.0, 0.0),
                        Vector3::new(n as f64 * 10.0 + 5.0, 0.0, 0.0),
                    )))
            })
            .collect();
        (app, handles)
    }

    fn selected(app: &OpenCADStudio) -> Vec<document::Handle> {
        app.tabs[app.active_tab]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect()
    }

    fn pick(app: &mut OpenCADStudio, handle: document::Handle) {
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(handle, false);
        let current = selected(app);
        let _ = app.feed_command(StepInput::SelectionComplete(current));
    }

    #[test]
    fn select_command_commits_the_picked_set() {
        let (mut app, h) = three_lines();
        let _ = app.run_command_line("SELECT");
        let i = app.active_tab;
        assert!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|c| c.is_selection_gathering()),
            "SELECT must gather objects"
        );
        pick(&mut app, h[1]);
        let _ = app.feed_command(StepInput::Enter);

        assert_eq!(selected(&app), vec![h[1]]);
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
    }

    #[test]
    fn select_previous_restores_the_set_before_select() {
        let (mut app, h) = three_lines();
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(h[0], false);

        // Objects already selected are taken at once.
        let _ = app.run_command_line("SELECT");
        assert!(app.tabs[i].active_cmd.is_none());
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entity(h[1], false);
        assert_eq!(selected(&app), vec![h[1]]);

        let _ = app.run_command_line("SELECTPREVIOUS");
        assert_eq!(selected(&app), vec![h[0]]);
    }

    #[test]
    fn select_previous_without_a_previous_set_keeps_the_selection() {
        let (mut app, h) = three_lines();
        let i = app.active_tab;
        app.tabs[i].scene.select_entity(h[2], false);
        let _ = app.run_command_line("SELECTPREVIOUS");
        assert_eq!(selected(&app), vec![h[2]]);
    }
}

#[cfg(test)]
mod dash_layer_properties_tests {
    use crate::app::OpenCADStudio;

    fn app_with_layer() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("-LAYER NEW WALLS");
        app
    }

    fn layer(app: &OpenCADStudio) -> &document::tables::Layer {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get("WALLS")
            .expect("WALLS layer")
    }

    /// A quoted name or description keeps its spaces, as in AutoCAD.
    #[test]
    fn dash_layer_takes_quoted_names_with_spaces() {
        let mut app = app_with_layer();
        let _ = app.run_command_line(r#"-LAYER NEW "Power Cables""#);
        let _ = app.run_command_line(r#"-LAYER D "outer walls" WALLS"#);
        let _ = app.run_command_line(r#"-LAYER R WALLS "Main Walls""#);
        let layers = &app.tabs[app.active_tab].scene.document.layers;
        assert!(layers.get("Power Cables").is_some());
        assert!(layers.get("WALLS").is_none());
        assert_eq!(layers.get("Main Walls").expect("renamed").description, "outer walls");
    }

    #[test]
    fn dash_layer_sets_transparency_and_description() {
        let mut app = app_with_layer();
        let _ = app.run_command_line("-LAYER TR 50 WALLS");
        let _ = app.run_command_line("-LAYER D walls WALLS");

        assert_eq!(
            layer(&app).transparency,
            document::types::Transparency::from_percent(0.5)
        );
        assert_eq!(layer(&app).description, "walls");
    }

    #[test]
    fn dash_layer_plot_flag_is_visible_in_the_layer_list() {
        let mut app = app_with_layer();
        let _ = app.run_command_line("-LAYER PLOT NOPLOT WALLS");
        assert!(!layer(&app).is_plottable);

        let listed = app.automation_op(r#"{"op":"layers"}"#);
        let walls = listed["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["name"] == "WALLS")
            .expect("WALLS listed");
        assert_eq!(walls["plottable"], false);
        assert_eq!(walls["transparency"], 0);
        assert_eq!(walls["description"], "");
    }
}

#[cfg(test)]
mod attedit_dash_tests;

#[cfg(test)]
mod layer_filter_tests;

#[cfg(test)]
mod dim_update_regen_tests;

mod mission_suites;

#[cfg(test)]
mod mission_m100_tests {
    use super::super::*;
    use super::all_output;

    fn add_mleader(app: &mut OpenCADStudio) -> document::Handle {
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
        app.tabs[i]
            .scene
            .add_entity(document::EntityType::MultiLeader(Box::new(leader)))
    }

    fn style(app: &OpenCADStudio, name: &str) -> document::objects::MultiLeaderStyle {
        app.tabs[app.active_tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                document::objects::ObjectType::MultiLeaderStyle(style)
                    if style.name.eq_ignore_ascii_case(name) =>
                {
                    Some(style.clone())
                }
                _ => None,
            })
            .expect("style")
    }

    fn leader(app: &OpenCADStudio, handle: document::Handle) -> document::entities::MultiLeader {
        match app.tabs[app.active_tab].scene.document.get_entity(handle) {
            Some(document::EntityType::MultiLeader(leader)) => (**leader).clone(),
            _ => panic!("multileader missing"),
        }
    }

    #[test]
    fn a_style_with_a_closed_arrow_and_its_text_height_can_be_applied() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);

        let _ = app.run_command_line("MLEADERSTYLE NEW NOTE-2.5 TEXT 2.5 ARROW CLOSED");
        let output = all_output(&app);
        assert!(output.contains("'NOTE-2.5' created with"), "{output}");
        let created = style(&app, "NOTE-2.5");
        assert_eq!(created.text_height, 2.5);
        let arrow = created.arrowhead_handle.expect("closed arrow block");
        let arrow_name = app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .iter()
            .find(|record| record.handle == arrow)
            .map(|record| record.name.clone());
        assert_eq!(arrow_name.as_deref(), Some("_Closed"));

        let handle = add_mleader(&mut app);
        let _ = app.run_command_line("MLEADERSTYLE APPLY NOTE-2.5 ALL");
        let output = all_output(&app);
        assert!(
            output.contains("applied 'NOTE-2.5' to 1 multileader(s)"),
            "{output}"
        );

        let styled = leader(&app, handle);
        assert_eq!(styled.style_handle, Some(created.handle));
        assert_eq!(styled.text_height, 2.5);
        assert_eq!(styled.context.text_height, 2.5);
        assert_eq!(styled.arrowhead_handle, Some(arrow));

        let _ = app.run_command_line("MLEADERSTYLE INFO ALL");
        let output = all_output(&app);
        assert!(output.contains("NOTE-2.5"), "{output}");
        assert!(
            output.contains(&format!("MLEADER {:x}", handle.value())),
            "{output}"
        );
    }

    #[test]
    fn changing_landing_distance_updates_existing_geometry() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let _ = app.run_command_line("MLEADERSTYLE NEW NOTE-2.5 TEXT 2.5");
        let handle = add_mleader(&mut app);
        let _ = app.run_command_line("MLEADERSTYLE APPLY NOTE-2.5 ALL");
        let before = leader(&app, handle)
            .context
            .leader_roots
            .first()
            .map(|root| root.landing_distance)
            .unwrap_or_default();

        let _ = app.run_command_line("MLEADERSTYLE SET NOTE-2.5 LANDING 5");
        let output = all_output(&app);
        assert!(
            output.contains("'NOTE-2.5'.LANDING = 5 (1 multileader(s) updated)"),
            "{output}"
        );

        assert_eq!(style(&app, "NOTE-2.5").landing_distance, 5.0);
        let after = leader(&app, handle);
        assert_eq!(after.dogleg_length, 5.0);
        let root = after.context.leader_roots.first().expect("leader root");
        assert_eq!(root.landing_distance, 5.0);
        assert_ne!(root.landing_distance, before);

        let _ = app.run_command_line("MLEADERSTYLE LIST");
        let output = all_output(&app);
        assert!(output.contains("landing:5.00 used:1"), "{output}");
    }

    #[test]
    fn an_unknown_property_or_style_is_reported() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{ "op": "new" }"#);
        let _ = app.run_command_line("MLEADERSTYLE NEW NOTE-2.5 TEXT 2.5");

        let _ = app.run_command_line("MLEADERSTYLE SET NOTE-2.5 WIDTH 4");
        assert!(
            all_output(&app).contains("unknown property 'width'"),
            "{}",
            all_output(&app)
        );

        let _ = app.run_command_line("MLEADERSTYLE APPLY GHOST ALL");
        assert!(
            all_output(&app).contains("'GHOST' not found"),
            "{}",
            all_output(&app)
        );
    }
}

/// #2588: POLYGON takes the inscribed/circumscribed option after the centre
/// and a typed radius, as the reference does.
#[cfg(test)]
mod polygon_flow_tests {
    use crate::app::OpenCADStudio;

    fn polygon(app: &OpenCADStudio) -> Vec<(f64, f64)> {
        let i = app.active_tab;
        app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::LwPolyline(polyline) => Some(
                    polyline
                        .vertices
                        .iter()
                        .map(|v| (v.location.x, v.location.y))
                        .collect(),
                ),
                _ => None,
            })
            .expect("a polygon was drawn")
    }

    fn drawn(line: &str) -> Vec<(f64, f64)> {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line(line);
        polygon(&app)
    }

    #[test]
    fn a_typed_radius_draws_an_inscribed_polygon_with_its_bottom_edge_level() {
        let vertices = drawn("POLYGON 6 0,0 I 10");
        assert_eq!(vertices.len(), 6);
        for (x, y) in &vertices {
            assert!((x.hypot(*y) - 10.0).abs() < 1e-9, "vertex on the circle");
        }
        let lowest: Vec<_> = vertices
            .iter()
            .filter(|(_, y)| {
                (*y - vertices.iter().map(|v| v.1).fold(f64::MAX, f64::min)).abs() < 1e-9
            })
            .collect();
        assert_eq!(lowest.len(), 2, "bottom edge level: {vertices:?}");
    }

    #[test]
    fn circumscribed_takes_the_radius_to_the_edge_midpoints() {
        let vertices = drawn("POLYGON 4 0,0 C 10");
        assert_eq!(vertices.len(), 4);
        for (x, y) in &vertices {
            assert!(
                (x.abs() - 10.0).abs() < 1e-9 && (y.abs() - 10.0).abs() < 1e-9,
                "{vertices:?}"
            );
        }
    }

    #[test]
    fn enter_at_the_option_keeps_the_default_and_a_picked_point_finishes() {
        let vertices = drawn("POLYGON 5 0,0  0,8");
        assert_eq!(vertices.len(), 5);
        assert!(
            vertices
                .iter()
                .any(|(x, y)| x.abs() < 1e-9 && (y - 8.0).abs() < 1e-9),
            "{vertices:?}"
        );
    }
}

/// #2593: MEASUREGEOM Quick names the edges its lines reach and measures
/// the closed area a click lands in.
#[cfg(test)]
mod measuregeom_quick_tests {
    use crate::app::OpenCADStudio;
    use crate::command::{CadCommand, CmdResult};
    use glam::DVec3;

    #[test]
    fn quick_reads_the_edges_it_reaches_and_a_click_measures_the_area() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("RECTANG 0,0 10,4");
        let scene = &app.tabs[app.active_tab].scene;
        let mut command = crate::modules::draw::inquiry::measuregeom::MeasureGeomCommand::new();
        let _ = command.on_text_input("Q");
        let cursor = DVec3::new(3.0, 1.0, 0.0);
        let wires = command.object_pick_hover_previews(scene, cursor);
        let plain = |s: String| s.replace(['\u{2068}', '\u{2069}'], "");
        let readout = plain(command.measurement_readout(cursor).expect("readout"));
        assert!(readout.contains("Length = 10"), "{readout}");
        assert!(readout.contains("Length = 4"), "{readout}");
        assert!(wires.len() >= 4);
        let CmdResult::ReportMeasurement(area) = command.on_point(cursor) else {
            panic!("expected the area");
        };
        let area = plain(area);
        assert!(
            area.contains("Area = 40") && area.contains("Perimeter = 28"),
            "{area}"
        );
    }
}

/// #1912: with Dynamic Input on, Down lists the step's options at the
/// cursor; Enter picks the highlighted one. Ctrl+9 hides the command line.
#[cfg(test)]
mod dynamic_options_tests {
    use crate::app::{ArrowKey, Message, OpenCADStudio};

    fn down() -> Message {
        Message::ArrowKeyPressed {
            direction: ArrowKey::Down,
            shortcut: "DOWN".into(),
            extend_selection: false,
        }
    }

    #[test]
    fn down_lists_the_options_and_enter_picks_one() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.dyn_input = true;
        let _ = app.run_command_line("CIRCLE");
        let _ = app.update(down());
        assert!(app.context_menu_open(), "Down opens the options");
        // CIRCLE's first option is 3P; Enter picks it.
        let _ = app.update(Message::CommandFinalize);
        assert!(!app.context_menu_open());
        let prompt = app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .unwrap()
            .prompt();
        assert!(prompt.to_uppercase().contains("FIRST POINT"), "{prompt}");
    }

    #[test]
    fn down_without_dynamic_input_or_options_does_nothing() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.dyn_input = false;
        let _ = app.run_command_line("CIRCLE");
        let _ = app.update(down());
        assert!(!app.context_menu_open());
    }

    #[test]
    fn ctrl_9_hides_and_shows_the_command_line() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let key = if cfg!(target_os = "macos") {
            "CMD+9"
        } else {
            "CTRL+9"
        }
        .to_string();
        let _ = app.update(Message::ShortcutPressed(key.clone()));
        assert!(app.command_line_hidden);
        let _ = app.update(Message::ShortcutPressed(key));
        assert!(!app.command_line_hidden);
    }
}

/// #2195: undo and redo bring back the view each step was made in.
#[cfg(test)]
mod undo_view_tests {
    use crate::app::{Message, OpenCADStudio};

    #[test]
    fn undo_returns_to_where_the_change_was_made() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let made_at = app.tabs[i].scene.camera.borrow().target;
        let _ = app.run_command_line("LINE 0,0 10,0 ");
        // Look somewhere far away.
        app.tabs[i].scene.camera.borrow_mut().target = glam::DVec3::new(1.0e6, 1.0e6, 0.0);
        let _ = app.update(Message::Undo);
        assert!(app.tabs[i].scene.camera.borrow().target.distance(made_at) < 1e-9);
        app.tabs[i].scene.camera.borrow_mut().target = glam::DVec3::new(-5.0e5, 0.0, 0.0);
        let _ = app.update(Message::Redo);
        assert!(app.tabs[i].scene.camera.borrow().target.distance(made_at) < 1e-9);
    }
}

/// #2033: Alt+letter opens a menu-bar menu as a keyboard-driven menu, and
/// every row of the idle shortcut menu has a letter.
#[cfg(test)]
mod menu_keyboard_tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::popup::context_menu::{MenuAction, MenuRow};

    #[test]
    fn alt_letter_opens_the_menu_bar_menu() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        app.show_menu_bar = true;
        let _ = app.update(Message::ShortcutPressed("ALT+F".into()));
        assert!(app.context_menu_open());
        let menu = app.current_context_menu();
        let rows = menu.selectable();
        assert!(rows.len() > 3);
        assert!(rows.iter().all(|row| row.mnemonic.is_some()));
        assert!(rows.iter().any(|row| row.action == MenuAction::MenuLine("NEW".into())
            || matches!(&row.action, MenuAction::MenuLine(line) if line.starts_with("NEW") || line.starts_with("QNEW"))));
        let _ = app.close_context_menu();
        assert!(app.menu_bar_popup.is_none());
    }

    #[test]
    fn the_idle_shortcut_menu_has_letters() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let menu = app.current_context_menu();
        let items = menu.rows.iter().filter_map(|row| match row {
            MenuRow::Item(item) => Some(item),
            _ => None,
        });
        for item in items {
            assert!(item.mnemonic.is_some(), "{}", item.label);
        }
    }
}

/// Copy, scale and viewport command coverage.
#[cfg(test)]
mod copy_and_viewport_commands;

#[cfg(test)]
mod array_zoom_extents_tests {
    use crate::app::OpenCADStudio;
    use crate::scene::assoc_array::AssocArraySpec;

    /// Zoom extents (ZOOM E, a double click of the middle button) frames
    /// every item of an associative array of a rectangle, not only the first:
    /// the items after the first render as stubs of one prototype, and those
    /// must count like the wires they stand for.
    #[test]
    fn zoom_extents_frames_every_item_of_a_rectangle_array() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let reply = app.automation_op(r#"{"op":"run","cmd":"RECTANG 0,0 10,5"}"#);
        assert_eq!(reply["status"], "completed", "{reply}");
        let i = app.active_tab;
        let rectangle = app.tabs[i]
            .scene
            .document
            .entities()
            .last()
            .map(|entity| entity.common().handle)
            .unwrap();
        app.handle_assoc_array(
            vec![rectangle],
            AssocArraySpec::Rectangular {
                columns: 4,
                rows: 4,
                column_spacing: 20.0,
                row_spacing: 30.0,
            },
            None,
        );

        app.tabs[i].scene.fit_all();
        let (min, max) = app.tabs[i]
            .scene
            .camera
            .borrow()
            .fitted_model_bounds()
            .expect("zoom extents fitted the drawing");
        assert!(min.abs_diff_eq(glam::DVec3::ZERO, 1e-9), "min {min:?}");
        assert!(
            max.abs_diff_eq(glam::DVec3::new(70.0, 95.0, 0.0), 1e-9),
            "max {max:?}, want the last item's far corner (70, 95)"
        );
    }
}

#[cfg(test)]
mod insert_and_dimstyle_commands;
/// Per-viewport layer overrides and style commands.
#[cfg(test)]
mod layer_and_style_commands;

/// XREF and sheet-set command coverage.
#[cfg(test)]
mod xref_and_sheet_set_tests;

#[cfg(test)]
mod selection_and_attribute_sync_tests;

#[cfg(test)]
mod text_mask_tests {
    use crate::app::OpenCADStudio;

    /// A mask is a wipeout around the text, so it must turn with the text.
    /// The old bounds were the axis-aligned box of the rotated glyph wires,
    /// and the rectangle built from it covered far more than the text did —
    /// on a 45-degree label it hid a large square of the drawing around it.
    #[test]
    fn a_rotated_text_gets_a_mask_that_turns_with_it() {
        use document::entities::Text;
        use document::types::Vector3;

        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut text = Text::with_value("MASK", Vector3::new(20.0, 20.0, 0.0));
        text.height = 2.0;
        text.rotation = std::f64::consts::FRAC_PI_4;
        let handle = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Text(text));
        app.tabs[i].scene.select_entities(&[handle]);

        let _ = app.run_command_line("TEXTMASK");
        let _ = app.feed_active_cmd("");

        let wipeout = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Wipeout(wipeout) => Some(wipeout.clone()),
                _ => None,
            })
            .expect("the text is masked");
        assert_ne!(
            wipeout.clip_type,
            document::entities::WipeoutClipType::Rectangular,
            "a rotated text needs a turned polygon, not an axis-aligned box"
        );
        let corners = wipeout.world_boundary_vertices();
        assert_eq!(corners.len(), 4);
        // The first edge follows the text's 45-degree rotation.
        let (dx, dy) = (corners[1].x - corners[0].x, corners[1].y - corners[0].y);
        assert!(
            dx.abs() > 0.2 && dy.abs() > 0.2,
            "the mask edge must not stay on an axis: {dx}, {dy}"
        );
        assert!(
            (dx.abs() - dy.abs()).abs() < 1e-6,
            "the mask edge turns 45 degrees: {dx}, {dy}"
        );
    }
}

#[cfg(test)]
mod change_properties_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::{Color, Vector3};

    fn color_of(app: &OpenCADStudio, handle: document::Handle) -> Color {
        app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .expect("the object stays")
            .common()
            .color
    }

    /// CHPROP's prompt offers "name / 1-255 / ByLayer", but its parser read
    /// the value as a bare index, so every named colour and ByLayer itself was
    /// rejected as invalid. The objects kept their old colour.
    #[test]
    fn chprop_color_accepts_the_names_and_bylayer_its_prompt_offers() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(document::EntityType::Line(
            Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0)),
        ));
        app.tabs[i].scene.select_entities(&[line]);

        let _ = app.run_command_line("CHPROP COLOR GREEN");
        assert_eq!(color_of(&app, line), Color::Index(3), "a named colour");

        let _ = app.run_command_line("CHPROP COLOR BYLAYER");
        assert_eq!(color_of(&app, line), Color::ByLayer, "ByLayer itself");

        let _ = app.run_command_line("CHPROP COLOR CHARTREUSE");
        assert_eq!(
            color_of(&app, line),
            Color::ByLayer,
            "an unknown colour leaves the object alone"
        );
    }
}

#[cfg(test)]
mod table_style_tests {
    use crate::app::OpenCADStudio;
    use document::objects::ObjectType;

    fn style(app: &OpenCADStudio, name: &str) -> document::objects::TableStyle {
        app.tabs[app.active_tab]
            .scene
            .document
            .objects
            .values()
            .find_map(|object| match object {
                ObjectType::TableStyle(style) if style.name.eq_ignore_ascii_case(name) => {
                    Some(style.clone())
                }
                _ => None,
            })
            .expect("the style exists")
    }

    /// TABLESTYLE could create a style and make it current but every style it
    /// created kept the standard margins and text height: SET took the name and
    /// ignored the rest, so a style could not be authored from the command
    /// line at all.
    #[test]
    fn tablestyle_set_authors_the_style_properties() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let _ = app.run_command_line("TABLESTYLE NEW LEDGER");
        let _ = app.run_command_line("TABLESTYLE SET LEDGER HMARGIN 0.25");
        assert_eq!(style(&app, "LEDGER").horizontal_margin, 0.25);

        let _ = app.run_command_line("TABLESTYLE SET LEDGER TEXTHEIGHT 3.5");
        let ledger = style(&app, "LEDGER");
        assert_eq!(ledger.data_row_style.text_height, 3.5);
        assert_eq!(ledger.header_row_style.text_height, 3.5);
        assert_eq!(ledger.title_row_style.text_height, 3.5);

        let _ = app.run_command_line("TABLESTYLE SET LEDGER ANNOTATIVE YES");
        assert!(style(&app, "LEDGER").annotative);

        // Without a property, SET still picks the style new tables take.
        let _ = app.run_command_line("TABLESTYLE SET LEDGER");
        assert_eq!(
            app.tabs[i].scene.document.header.current_table_style_name,
            "LEDGER"
        );
    }
}

#[cfg(test)]
mod layer_command_tests {
    use crate::app::OpenCADStudio;
    use document::types::Color;

    fn layer_color(app: &OpenCADStudio, name: &str) -> Color {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get(name)
            .map(|layer| layer.color)
            .expect("the layer exists")
    }

    /// The layer manager's colour prompt accepts a name as readily as an ACI
    /// index, and the layer may come before or after the colour. The plain
    /// LAYER command used to take a number only, in name-first order, so
    /// `LAYER COLOR WALLS RED` and `LAYER COLOR 3 WALLS` both failed.
    #[test]
    fn layer_color_takes_the_names_and_either_order() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("LAYER NEW WALLS");

        let _ = app.run_command_line("LAYER COLOR WALLS RED");
        assert_eq!(layer_color(&app, "WALLS"), Color::Index(1));

        let _ = app.run_command_line("LAYER COLOR 3 WALLS");
        assert_eq!(layer_color(&app, "WALLS"), Color::Index(3));

        let _ = app.run_command_line("LAYER COLOR WALLS CHARTREUSE");
        assert_eq!(
            layer_color(&app, "WALLS"),
            Color::Index(3),
            "an unknown colour leaves the layer alone"
        );
    }
}

#[cfg(test)]
mod layer_tool_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::Vector3;

    fn draw_on(app: &mut OpenCADStudio, layer: &str, x: f64) -> document::Handle {
        let mut line = Line::from_points(
            Vector3::new(x, 0.0, 0.0),
            Vector3::new(x, 10.0, 0.0),
        );
        line.common.layer = layer.to_string();
        app.tabs[app.active_tab]
            .scene
            .add_entity(document::EntityType::Line(line))
    }

    fn layer(app: &OpenCADStudio, name: &str) -> document::tables::Layer {
        app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .get(name)
            .cloned()
            .expect("the layer exists")
    }

    /// LAYOFF and LAYFRZ pick the layers of the selection; the layer new
    /// objects are drawn on has to survive, exactly as `-LAYER OFF` and
    /// `-LAYER FREEZE` refuse the current layer. The plain commands used to
    /// hide it like any other, leaving no visible layer for the next line.
    #[test]
    fn layer_tools_leave_the_current_layer_alone() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("-LAYER NEW WALLS");
        let _ = app.run_command_line("-LAYER NEW DIMS");
        let _ = app.run_command_line("-LAYER SET WALLS");
        let current = draw_on(&mut app, "WALLS", 0.0);
        let other = draw_on(&mut app, "DIMS", 5.0);
        let i = app.active_tab;

        app.tabs[i].scene.select_entities(&[current, other]);
        let _ = app.run_command_line("LAYFRZ");
        assert!(
            !layer(&app, "WALLS").flags.frozen,
            "the current layer must stay thawed"
        );
        assert!(
            layer(&app, "DIMS").flags.frozen,
            "the picked layer still freezes"
        );

        let _ = app.run_command_line("-LAYER THAW DIMS");
        app.tabs[i].scene.select_entities(&[current, other]);
        let _ = app.run_command_line("LAYOFF");
        assert!(
            !layer(&app, "WALLS").flags.off,
            "the current layer must stay on"
        );
        assert!(layer(&app, "DIMS").flags.off, "the picked layer still turns off");

        // The one-at-a-time pick path refuses the same layer.
        let _ = app.run_command_line("-LAYER ON DIMS");
        app.set_layer_on_from_command(i, "WALLS", false);
        assert!(
            !layer(&app, "WALLS").flags.off,
            "a pick on the current layer must leave it on"
        );
    }
}

#[cfg(test)]
mod attedit_one_at_a_time_tests {
    use crate::app::OpenCADStudio;
    use document::entities::{AttributeEntity, Insert};
    use document::types::Vector3;

    fn two_typed_references() -> (OpenCADStudio, document::Handle, document::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut make = |name: &str, value: &str| {
            let mut insert = Insert::new(name, Vector3::ZERO);
            insert
                .attributes
                .push(AttributeEntity::new("NUMBER".into(), value.into()));
            app.tabs[i]
                .scene
                .add_entity(document::EntityType::Insert(insert))
        };
        let alpha = make("ALPHA", "111");
        let beta = make("BETA", "222");
        (app, alpha, beta)
    }

    /// The one-at-a-time branch still asks for the block, tag and value
    /// specifications, so it must edit the reference those specs name —
    /// not fall back to a bare `ATTEDIT` that re-asks for a pick despite
    /// the selection and the specs.
    #[test]
    fn attedit_one_at_a_time_opens_the_editor_on_the_specified_reference() {
        let (mut app, alpha, beta) = two_typed_references();
        let i = app.active_tab;
        app.tabs[i].scene.select_entities(&[alpha, beta]);

        let _ = app.run_command_line("-ATTEDIT");
        let _ = app.feed_script_line("Y");
        let _ = app.feed_script_line("BETA");
        let _ = app.feed_script_line("");
        let _ = app.feed_script_line("");

        assert_eq!(
            app.attr_editor_handle,
            Some(beta),
            "the editor must open on the reference the block name specification names"
        );
        assert_eq!(app.attr_editor_rows.len(), 1);
    }
}

#[cfg(test)]
mod viewport_annotation_sync_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Viewport;
    use document::EntityType;

    /// The status pill's sync action re-links the viewport it describes to
    /// its annotation scale, from the headless dispatch path. A clean round
    /// trip: the viewport has the scale stored, the stored link is cleared,
    /// and the sync message re-establishes it.
    #[test]
    fn the_sync_action_relinks_a_viewport_to_its_scale() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let handle = {
            let i = app.active_tab;
            let scene = &mut app.tabs[i].scene;
            scene.add_layout("P1").unwrap();
            scene.current_layout = "P1".to_string();
            let mut viewport = Viewport::new();
            viewport.width = 200.0;
            viewport.height = 100.0;
            let handle = scene
                .document
                .add_entity_to_layout(EntityType::Viewport(Box::new(viewport)), "P1")
                .unwrap();
            assert!(scene.add_scale("1:100", 1.0, 100.0));
            assert!(scene.set_viewport_scale_named_for(handle, "1:100").is_some());
            // Clear the link the render paths see, as a drawing from an
            // older file or a bad reference would carry.
            scene
                .document
                .set_viewport_annotation_scale(handle, document::Handle::NULL);
            assert_eq!(
                scene.document.viewport_annotation_scale(handle),
                Some(document::Handle::NULL)
            );
            handle
        };

        let _ = app.update(crate::app::Message::SyncViewportAnnotationScale);

        let scene = &mut app.tabs[app.active_tab].scene;
        let stored = scene.document.viewport_annotation_scale(handle);
        let named = scene
            .scale_handle_ensuring("1:100")
            .expect("the 1:100 scale exists");
        assert_eq!(
            stored,
            Some(named),
            "the sync action links the viewport the pill describes"
        );
    }
}



#[cfg(test)]
mod data_extraction_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::Vector3;

    /// `-DATAEXTRACTION <settings> <point>` runs a saved extraction end to
    /// end: the external file the wizard offers is written with the rows,
    /// and the extraction table lands at the point. This pins the whole
    /// path an automated run needs — no point of it stops at a prompt
    /// nobody can answer headlessly.
    #[test]
    fn a_saved_extraction_writes_the_file_and_places_the_table() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let _ = app.run_command_line("-LAYER NEW WALLS");
        let mut line = Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0));
        line.common.layer = "WALLS".to_string();
        app.tabs[i].scene.add_entity(document::EntityType::Line(line));

        let dir = tempfile::tempdir().expect("a temporary extraction directory");
        let folder = dir.path().to_path_buf();
        let ext = folder.join("lines.csv");
        let settings_path = folder.join("lines.dxex");
        let settings = serde_json::json!({
            "version": 1,
            "source": "currentDrawing",
            "includeCurrent": true,
            "includeSubfolders": false,
            "objectTypes": ["Line"],
            "properties": ["Layer"],
            "combineIdentical": false,
            "showCount": true,
            "showName": true,
            "outputTable": true,
            "outputFile": true,
            "outputPath": ext.display().to_string(),
            "tableTitle": "LINES",
            "propertyHeaders": true,
            "inUseOnly": false,
            "blocksOnly": false,
            "attributedBlocksOnly": false,
            "displayAllTypes": true,
            "hiddenCategories": [],
        });
        std::fs::write(&settings_path, settings.to_string()).expect("the settings file");

        let settings_text = settings_path.display().to_string().replace('\\', "/");
        let line = format!("-DATAEXTRACTION {settings_text} 0,0");
        let _ = app.run_command_line(&line);

        let written = std::fs::read_to_string(&ext).expect("the extraction file was written");
        assert!(
            written.to_uppercase().contains("WALLS"),
            "the file names the layer: {written}"
        );
        let tables: Vec<document::entities::Table> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Table(table) => Some((**table).clone()),
                _ => None,
            })
            .collect();
        assert_eq!(tables.len(), 1, "one extraction table was placed");
        assert_eq!(
            tables[0].cell_text(0, 0).map(|text| text.to_string()).as_deref(),
            Some("LINES"),
        );
    }
}

#[cfg(test)]
mod attsync_order_tests {
    use crate::app::OpenCADStudio;
    use document::entities::{AttributeDefinition, AttributeEntity, Insert};
    use document::EntityType;
    use document::types::Vector3;

    fn redefine(app: &mut OpenCADStudio, name: &str, attributes: &[(&str, &str)]) {
        let definition = |tag: &str, index: usize| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0 + index as f64 * 3.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(tag.into(), format!("{tag} prompt"), String::new())
            })
        };
        app.tabs[app.active_tab].scene.define_block_raw(
            name,
            Vector3::ZERO,
            attributes
                .iter()
                .enumerate()
                .map(|(index, (tag, _))| definition(tag, index))
                .collect(),
        );
    }

    fn placed(app: &mut OpenCADStudio, name: &str, values: &[(&str, &str)]) -> document::Handle {
        let mut insert = Insert::new(name, Vector3::new(100.0, 100.0, 0.0));
        for (tag, value) in values {
            insert
                .attributes
                .push(AttributeEntity::new(tag.to_string(), value.to_string()));
        }
        app.tabs[app.active_tab]
            .scene
            .add_entity(EntityType::Insert(insert))
    }

    #[test]
    fn attsync_reorders_the_reference_to_follow_the_definition() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        redefine(&mut app, "PANEL", &[("ALPHA", "a"), ("BRAVO", "b")]);
        let reference = placed(&mut app, "PANEL", &[("ALPHA", "first"), ("BRAVO", "second")]);
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "PANEL",
                Vector3::ZERO,
                vec![
                    EntityType::AttributeDefinition(AttributeDefinition {
                        insertion_point: Vector3::new(2.0, 5.0, 0.0),
                        height: 1.0,
                        ..AttributeDefinition::new("BRAVO".into(), "b".into(), String::new())
                    }),
                    EntityType::AttributeDefinition(AttributeDefinition {
                        insertion_point: Vector3::new(2.0, 2.0, 0.0),
                        height: 1.0,
                        ..AttributeDefinition::new("ALPHA".into(), "a".into(), String::new())
                    }),
                ],
            )
            .expect("the definition takes the reordered tags");

        let _ = app.run_command_line("ATTSYNC PANEL");

        let document = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = document.get_entity(reference).expect("reference")
        else {
            panic!("not an insert");
        };
        let pairs: Vec<(String, String)> = insert
            .attributes
            .iter()
            .map(|attribute| (attribute.tag.clone(), attribute.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("BRAVO".into(), "second".into()),
                ("ALPHA".into(), "first".into())
            ],
            "the reference follows the definition's order, by tag"
        );
    }
}

#[cfg(test)]
mod battman_tests {
    use crate::app::OpenCADStudio;
    use document::entities::{AttributeDefinition, AttributeEntity, Insert};
    use document::EntityType;
    use document::types::Vector3;

    /// BATTMAN asks the picked reference's block for its definitions and
    /// writes the edited list back to the block, tag-for-tag, values kept.
    /// The reference an editing session renames keeps the value it held.
    #[test]
    fn battman_applies_the_edited_definition_to_its_references() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "PANEL",
            Vector3::ZERO,
            vec![EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, 2.0, 0.0),
                height: 1.0,
                ..AttributeDefinition::new("TAGGED".into(), "Tag".into(), String::new())
            })],
        );
        let mut insert = Insert::new("PANEL", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TAGGED".into(), "kept".into()));
        let reference = app.tabs[i]
            .scene
            .add_entity(EntityType::Insert(insert));

        app.tabs[i].scene.select_entities(&[reference]);
        let _ = app.dispatch_command("BATTMAN");
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::AttMan),
            "BATTMAN opens on the picked reference's block"
        );

        let _ = app.update(crate::app::Message::AttMan(
            crate::ui::window::attman_dialog::AttManMsg::Tag("RENAMED".to_string()),
        ));
        let _ = app.update(crate::app::Message::AttMan(
            crate::ui::window::attman_dialog::AttManMsg::Apply,
        ));

        let document = &app.tabs[i].scene.document;
        let EntityType::Insert(insert) = document.get_entity(reference).expect("reference")
        else {
            panic!("not an insert");
        };
        let tags: Vec<String> = insert.attributes.iter().map(|a| a.tag.clone()).collect();
        assert_eq!(tags, vec!["RENAMED".to_string()]);
        let values: Vec<String> = insert
            .attributes
            .iter()
            .map(|a| a.get_value().to_string())
            .collect();
        assert_eq!(values, vec!["kept".to_string()]);
    }
}



#[cfg(test)]
mod hatch_panel_edit_tests {
    use crate::app::OpenCADStudio;
    use crate::modules::draw::draw::hatch::{HatchFill, HatchSettings};
    use document::entities::{BoundaryEdge, Hatch, PolylineEdge};
    use document::types::{Color, Vector2};

    fn app_base_settings(fill: HatchFill) -> HatchSettings {
        HatchSettings {
            fill,
            angle_deg: 0.0,
            scale: 1.0,
            origin: [0.0, 0.0],
            method: 0,
            islands: document::entities::HatchStyleType::Normal,
            associative: true,
            separate: false,
            keep_boundaries: false,
            gap_tolerance: 0.0,
            color: None,
            background: None,
            layer: None,
            transparency: None,
            gradient_colors: [Color::Index(5), Color::Index(1)],
            draw_order: 0,
            annotative: false,
        }
    }

    /// The Hatch panel is one of the paths that writes onto the entity. The
    /// prebuilt fill model has to follow, like every other editing path; a
    /// stale cache would keep showing the old pattern after OK.
    #[test]
    fn the_panel_edit_rebuilds_the_cached_fill_model() {
        use document::entities::BoundaryPath as BoundaryPathType;
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        let mut path = BoundaryPathType::new();
        path.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(
            [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]
                .into_iter()
                .map(|p| Vector2::new(p[0], p[1]))
                .collect(),
            true,
        )));
        hatch.paths.push(path);
        let handle = app.tabs[i]
            .scene
            .add_entity(document::EntityType::Hatch(Box::new(hatch)));
        app.tabs[i].scene.refresh_fill_model(handle);
        assert_eq!(
            app.tabs[i].scene.hatches.get(&handle).map(|m| m.name.clone()),
            Some("SOLID".to_string())
        );

        let before = app_base_settings(HatchFill::Solid);
        let after = app_base_settings(HatchFill::Pattern("ANSI31".to_string()));
        app.apply_hatch_panel_edit(handle, &before, &after);

        let model = app.tabs[i]
            .scene
            .hatches
            .get(&handle)
            .expect("the fill model was rebuilt");
        assert_eq!(model.name, "ANSI31");
    }
}

#[cfg(test)]
mod group_roundtrip_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::Vector3;

    fn scratch_path(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let path = dir.path().join(format!("ocs-groups-{tag}.dwg"));
        (dir, path)
    }

    /// Groups are document objects with real records: they must survive the
    /// round trip through the file, with their membership, the same as every
    /// other object. The mission this pins claimed they were missing.
    #[test]
    fn a_group_survives_a_save_and_reload() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let a = app.tabs[app.active_tab].scene.add_entity(
            document::EntityType::Line(Line::from_points(
                Vector3::new(0.0, 0.0, 0.0),
                Vector3::new(10.0, 0.0, 0.0),
            )),
        );
        let b = app.tabs[app.active_tab].scene.add_entity(
            document::EntityType::Line(Line::from_points(
                Vector3::new(0.0, 5.0, 0.0),
                Vector3::new(10.0, 5.0, 0.0),
            )),
        );
        let i = app.active_tab;
        app.tabs[i].scene.create_group("FRAME".to_string(), vec![a, b]);
        let (_scratch_dir, path) = scratch_path("roundtrip");
        let saved = app.automation_op(&format!(r#"{{"op":"save","path":"{}"}}"#, path.display().to_string().replace('\\', "/")));
        assert_eq!(saved["ok"], true, "the save reported success: {saved}");
        let opened = app.automation_op(&format!(r#"{{"op":"open","path":"{}"}}"#, path.display().to_string().replace('\\', "/")));
        assert_eq!(opened["ok"], true, "the open reported success: {opened}");

        let groups: Vec<String> = app.tabs[app.active_tab]
            .scene
            .groups()
            .map(|group| group.name.clone())
            .collect();
        assert_eq!(groups, vec!["FRAME".to_string()]);
        let _ = std::fs::remove_file(&path);
    }
}



#[cfg(test)]
mod copy_draw_order_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::Vector3;

    fn sort_entries(app: &OpenCADStudio) -> std::collections::HashMap<u64, u64> {
        app.tabs[app.active_tab]
            .scene
            .document
            .objects
            .values()
            .filter_map(|obj| match obj {
                document::objects::ObjectType::SortEntitiesTable(t) => Some(t),
                _ => None,
            })
            .flat_map(|t| t.entries().map(|e| (e.entity_handle.value(), e.sort_handle.value())))
            .collect()
    }

    fn fresh_line(app: &mut OpenCADStudio, x: f64) -> document::Handle {
        app.tabs[app.active_tab].scene.add_entity(
            document::EntityType::Line(Line::from_points(
                Vector3::new(x, 0.0, 0.0),
                Vector3::new(x, 10.0, 0.0),
            )),
        )
    }

    /// When several overlapping objects are copied, the copies must keep
    /// the original objects' drawing order: whatever was behind stays
    /// behind, whatever was in front stays in front.
    #[test]
    fn copying_multiple_overlapping_objects_preserves_their_order() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let a = fresh_line(&mut app, 0.0);
        let b = fresh_line(&mut app, 5.0);
        let c = fresh_line(&mut app, 10.0);
        let i = app.active_tab;

        // Reverse the automatic handle order: send B behind A, keep C where it is.
        app.tabs[i].scene.select_entities(&[b]);
        let _ = app.run_command_line("DRAWORDER BACK");

        eprintln!("before copy table: {:?}", sort_entries(&app));

        app.tabs[i].scene.select_entities(&[a, b, c]);
        let _ = app.run_command_line("COPY");
        let _ = app.feed_script_line("0,0");
        let _ = app.feed_script_line("100,0");
        let _ = app.feed_script_line("");

        let entries = sort_entries(&app);
        eprintln!("after copy table:  {:?}", entries);
        let eff = |h: document::Handle| {
            entries
                .get(&h.value())
                .copied()
                .unwrap_or(h.value())
        };
        let line_starts: Vec<(document::Handle, f64)> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|e| match e {
                document::EntityType::Line(l) => Some((e.common().handle, l.start.x)),
                _ => None,
            })
            .collect();
        let by_x = |x: f64| {
            line_starts
                .iter()
                .find(|(_, start)| (start - x).abs() < 1e-9)
                .map(|(h, _)| *h)
                .expect("a line at that offset")
        };
        // The right-most copies correspond to the five source positions.
        let copy_of = |source: document::Handle| {
            let x = line_starts.iter().find(|(h, _)| *h == source).map(|(_, x)| *x).unwrap();
            by_x(x + 100.0)
        };
        let cp_a = copy_of(a);
        let cp_b = copy_of(b);
        let cp_c = copy_of(c);
        eprintln!(
            "source effs: a={} b={} c={}  copies: a={} b={} c={}",
            eff(a), eff(b), eff(c), eff(cp_a), eff(cp_b), eff(cp_c)
        );
        // The source order itself: B behind A behind C.
        assert!(eff(b) < eff(a), "B stays behind A");
        assert!(eff(a) < eff(c), "A stays behind C");
        // And the copies replicate it.
        assert!(eff(cp_b) < eff(cp_a), "the copy of B must stay behind the copy of A");
        assert!(eff(cp_a) < eff(cp_c), "the copy of A must stay behind the copy of C");
    }
}

#[cfg(test)]
mod dimension_query_tests {
    use crate::app::OpenCADStudio;

    /// The automation query answers for a dimension like it does for the
    /// other entity families: the measured value and the style it was
    /// drawn with, so a caller can read a drawing's dimensions without
    /// opening each entity record.
    #[test]
    fn a_dimension_query_reports_its_measurement_and_style() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");

        let r = app.automation_op(
            r#"{"op":"query","type":"DIMENSION","detail":"full","fields":["measurement","style"]}"#,
        );
        assert_eq!(r["ok"], true, "{r}");
        let entity = &r["entities"][0];
        let measurement = entity["measurement"]
            .as_f64()
            .expect("measurement is numeric");
        assert!((measurement - 10.0).abs() < 1e-6, "measurement: {measurement}");
        assert_eq!(entity["style"], "ISO-25", "the drawing's current style");
    }
}

#[cfg(test)]
mod dim_baseline_chain_tests {
    use crate::app::OpenCADStudio;

    /// DIMBASELINE takes the last dimension as its base and grows the
    /// series from the first extension line origin: a typed origin
    /// produces one more dimension of the same style, measured from the
    /// base's first origin, spaced by DIMDLI.
    #[test]
    fn dimbaseline_continues_the_series_from_the_last_dimension() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");
        let _ = app.run_command_line("DIMBASELINE");
        let _ = app.feed_script_line("15,3");
        let _ = app.feed_script_line("");

        let dims: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Dimension(dimension) => Some(dimension.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(dims.len(), 2, "one base dimension and one baseline dimension");
        let measured: Vec<f64> = dims.iter().map(|d| d.measurement()).collect();
        assert!(
            measured
                .iter()
                .any(|m| (*m - 10.0).abs() < 1e-6),
            "the base measures 10: {measured:?}"
        );
        assert!(
            measured
                .iter()
                .any(|m| (*m - 15.0).abs() < 1e-6),
            "the baseline dimension measures from the shared first origin to the typed one: {measured:?}"
        );
    }
}

#[cfg(test)]
mod dim_continue_chain_tests {
    use crate::app::OpenCADStudio;

    /// DIMCONTINUE chains from the last dimension: each typed origin
    /// starts where the previous dimension ended, so a series of
    /// endpoints measures as a running chain.
    #[test]
    fn dimcontinue_chains_from_the_last_dimension_end() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");
        let _ = app.run_command_line("DIMCONTINUE");
        let _ = app.feed_script_line("18,0");
        let _ = app.feed_script_line("");

        let dims: Vec<_> = app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Dimension(dimension) => Some(dimension.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(dims.len(), 2, "one base dimension and one continued dimension");
        let measured: Vec<f64> = dims.iter().map(|d| d.measurement()).collect();
        assert!(
            measured
                .iter()
                .any(|m| (*m - 10.0).abs() < 1e-6),
            "the base measures 10: {measured:?}"
        );
        assert!(
            measured
                .iter()
                .any(|m| (*m - 8.0).abs() < 1e-6),
            "the continued dimension runs from the base's end to the typed origin: {measured:?}"
        );
    }
}

#[cfg(test)]
mod dim_inspect_window_tests {
    use crate::app::OpenCADStudio;
    use crate::ui::window::dim_inspect_dialog::DimInspectMsg;
    use crate::app::Message;

    /// The inspection window stamps the dimensions it opened on: the
    /// shape, label and rate ride the dimension's extension data, in one
    /// undo step, and the same read answers the window when it opens
    /// again on an already inspected dimension.
    #[test]
    fn the_inspection_window_stamps_the_selected_dimension() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.run_command_line("DIMLINEAR 0,0 10,0 5,2");
        let i = app.active_tab;
        let dim = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Dimension(_) => Some(entity.common().handle),
                _ => None,
            })
            .expect("the dimension was placed");
        app.tabs[i].scene.select_entities(&[dim]);

        let _ = app.run_command_line("DIMINSPECT");

        assert!(
            app.dim_inspect_dialog.is_some(),
            "the window opened on the selected dimension"
        );

        let _ = app.update(Message::DimInspect(DimInspectMsg::LabelOn(true)));
        let _ = app.update(Message::DimInspect(DimInspectMsg::Label("QCS".into())));
        let _ = app.update(Message::DimInspect(DimInspectMsg::RateOn(true)));
        let _ = app.update(Message::DimInspect(DimInspectMsg::Rate("100%".into())));
        let _ = app.update(Message::DimInspect(DimInspectMsg::Ok));

        let entity = app.tabs[i]
            .scene
            .document
            .get_entity(dim)
            .expect("the dimension survived");
        let inspection = crate::entities::dim_inspect::read(&entity.common().extended_data)
            .expect("the inspection was written");
        assert_eq!(inspection.label.as_deref(), Some("QCS"));
        assert_eq!(inspection.rate.as_deref(), Some("100%"));
        assert!(app.dim_inspect_dialog.is_none(), "the window closed on Ok");

        // The window reads its current values back from the dimension.
        app.tabs[i].scene.select_entities(&[dim]);
        let _ = app.run_command_line("DIMINSPECT");
        let state = app
            .dim_inspect_dialog
            .as_ref()
            .expect("the window reopened");
        assert_eq!(state.label, "QCS", "the stored label is shown");
        assert!(state.any_inspected, "the dimension reports as inspected");
    }
}

#[cfg(test)]
mod join_command_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::Vector3;

    fn fresh_line(app: &mut OpenCADStudio, x1: f64, x2: f64) -> document::Handle {
        app.tabs[app.active_tab].scene.add_entity(
            document::EntityType::Line(Line::from_points(
                Vector3::new(x1, 0.0, 0.0),
                Vector3::new(x2, 0.0, 0.0),
            )),
        )
    }

    /// Pickfirst JOIN: two collinear lines selected together merge into
    /// the one line that spans both, and the consumed piece is erased.
    #[test]
    fn join_merges_collinear_lines_selected_together() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let first = fresh_line(&mut app, 0.0, 10.0);
        let second = fresh_line(&mut app, 10.0, 18.0);
        app.tabs[i].scene.select_entities(&[first, second]);

        let _ = app.run_command_line("JOIN");

        let lines: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => Some(line.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 1, "two collinear lines joined into one: {lines:?}");
        let line = &lines[0];
        let xs = [line.start.x, line.end.x];
        assert_eq!(xs.iter().cloned().reduce(f64::min), Some(0.0), "{line:?}");
        assert_eq!(xs.iter().cloned().reduce(f64::max), Some(18.0), "{line:?}");
    }
}

#[cfg(test)]
mod array_surface_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::Vector3;

    fn out(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    /// ARRAYRECT on the pickfirst selection runs to its associative
    /// array from the command line: typed row and column counts and
    /// spacings produce the array entity, and the source geometry is
    /// consumed by it.
    #[test]
    fn arrayrect_builds_the_associative_array_from_the_preselection() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(
            document::EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            )),
        );
        app.tabs[i].scene.select_entities(&[line]);

        let _ = app.run_command_line("ARRAYRECT");
        let _ = app.feed_script_line("3");
        let _ = app.feed_script_line("2");
        let _ = app.feed_script_line("10");
        let _ = app.feed_script_line("5");

        assert!(
            out(&app).contains("associative array created"),
            "the command reports the array: {}",
            out(&app)
        );
        assert!(
            app.tabs[i].scene.document.get_entity(line).is_none(),
            "the source line is consumed by the array"
        );
        let inserts = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|entity| matches!(entity, document::EntityType::Insert(_)))
            .count();
        assert!(inserts >= 1, "the array item entity exists");
    }
}

#[cfg(test)]
mod scale_rotate_copy_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Line;
    use document::types::Vector3;

    fn fresh_line(app: &mut OpenCADStudio, x: f64) -> document::Handle {
        app.tabs[app.active_tab].scene.add_entity(
            document::EntityType::Line(Line::from_points(
                Vector3::new(x, 0.0, 0.0),
                Vector3::new(x + 10.0, 0.0, 0.0),
            )),
        )
    }

    /// SCALE's Copy option scales a duplicate about the base point and
    /// leaves the source untouched; the factor prompt stays on offer so
    /// the value follows the keyword.
    #[test]
    fn scale_copy_keeps_the_source_and_places_the_scaled_duplicate() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let source = fresh_line(&mut app, 0.0);
        app.tabs[i].scene.select_entities(&[source]);

        let _ = app.run_command_line("SCALE");
        let _ = app.feed_script_line("0,0");
        let _ = app.feed_script_line("C");
        let _ = app.feed_script_line("2");

        let lengths: Vec<f64> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => {
                    Some((line.end - line.start).length())
                }
                _ => None,
            })
            .collect();
        assert_eq!(lengths.len(), 2, "the source and its copy: {lengths:?}");
        assert!(
            lengths.iter().any(|l| (*l - 10.0).abs() < 1e-9),
            "the source keeps its length: {lengths:?}"
        );
        assert!(
            lengths.iter().any(|l| (*l - 20.0).abs() < 1e-9),
            "the copy is scaled: {lengths:?}"
        );
    }

    /// ROTATE's Copy option rotates a duplicate about the base point and
    /// leaves the source at its original angle.
    #[test]
    fn rotate_copy_keeps_the_source_and_places_the_rotated_duplicate() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let source = fresh_line(&mut app, 0.0);
        app.tabs[i].scene.select_entities(&[source]);

        let _ = app.run_command_line("ROTATE");
        let _ = app.feed_script_line("0,0");
        let _ = app.feed_script_line("C");
        let _ = app.feed_script_line("90");

        let horizontal = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => Some(line.clone()),
                _ => None,
            })
            .filter(|line| (line.end.y - line.start.y).abs() < 1e-9)
            .count();
        let vertical = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => Some(line.clone()),
                _ => None,
            })
            .filter(|line| (line.end.x - line.start.x).abs() < 1e-9)
            .count();
        assert_eq!(horizontal, 1, "the source keeps its angle");
        assert_eq!(vertical, 1, "the duplicate is rotated 90 degrees");
    }
}

#[cfg(test)]
mod attedit_editor_tests {
    use crate::app::OpenCADStudio;
    use document::entities::{AttributeEntity, Insert};
    use document::types::Vector3;

    fn attributed_insert() -> (OpenCADStudio, document::Handle) {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut insert = Insert::new("TITLEBLOCK", Vector3::new(0.0, 0.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TITLE".into(), "old".into()));
        let handle = app.tabs[i].scene.add_entity(document::EntityType::Insert(insert));
        (app, handle)
    }

    /// Bare EATTEDIT opens the interactive attribute editor on the
    /// selected reference instead of ending silently: the editor's rows
    /// list the reference's attributes with their current values.
    #[test]
    fn eattedit_opens_the_editor_on_the_selected_reference() {
        let (mut app, handle) = attributed_insert();
        let i = app.active_tab;
        app.tabs[i].scene.select_entities(&[handle]);

        let _ = app.run_command_line("EATTEDIT");

        assert_eq!(
            app.attr_editor_rows.len(),
            1,
            "the editor opened on the reference's one attribute"
        );
        assert_eq!(
            app.attr_editor_rows[0].value,
            "old",
            "the row shows the attribute's current value"
        );
    }
}

#[cfg(test)]
mod count_mode_tests {
    use crate::app::OpenCADStudio;
    use document::entities::Insert;
    use document::types::Vector3;

    fn out(app: &OpenCADStudio) -> String {
        app.command_line.history_plain_text().trim().to_string()
    }

    /// COUNT C (Count Selection) takes the preselection as its target:
    /// the count is announced, count mode is on with the picked blocks
    /// as its target, and the number answers COUNTNUMBER afterwards.
    #[test]
    fn count_selection_counts_the_preselected_inserts() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "VALVE",
            Vector3::ZERO,
            vec![document::EntityType::Circle(document::entities::Circle::new())],
        );
        let mut handles = Vec::new();
        for n in 0..3 {
            let insert = Insert::new("VALVE", Vector3::new(n as f64 * 30.0, 0.0, 0.0));
            handles.push(app.tabs[i].scene.add_entity(document::EntityType::Insert(insert)));
        }
        app.tabs[i].scene.select_entities(&handles);

        let _ = app.run_command_line("COUNT C");

        assert!(out(&app).contains("3 found"), "the count announced: {}", out(&app));
        let mode = app.tabs[i].count.as_ref().expect("count mode is on");
        assert_eq!(mode.count(&app.tabs[i].scene.document), 3, "the mode counts the picked blocks");
    }
}

#[cfg(test)]
mod select_by_handle_tests {
    use crate::app::OpenCADStudio;
    use document::entities::{Insert, Line};
    use document::types::Vector3;

    /// Selecting by explicit handles (hex, as `query` returns them) takes
    /// what was named: a line and a block reference named together both end
    /// up selected, and the selection survives the op, so a following
    /// command acts on both.
    #[test]
    fn select_by_handles_selects_every_named_entity() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(document::EntityType::Line(
            Line::from_points(Vector3::ZERO, Vector3::new(10.0, 0.0, 0.0)),
        ));
        let insert = app.tabs[i].scene.add_entity(document::EntityType::Insert(
            Insert::new("VALVE", Vector3::new(30.0, 0.0, 0.0)),
        ));

        let r = app.automation_op(&format!(
            r#"{{"op":"select","handles":["{:X}","{:X}"]}}"#,
            line.value(),
            insert.value()
        ));

        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(r["selected"], 2, "both named entities are selected: {r}");
        let selected: Vec<document::Handle> = app.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        assert!(selected.contains(&line) && selected.contains(&insert), "{selected:?}");
    }
}


// The Block Definition dialog's whole flow is message-driven: BLOCK opens the
// band over the current selection, the typed fields update its state, and
// applying commits through the same CreateBlockWithOptions result as -BLOCK.
mod block_definition_dialog_tests {
    use super::super::*;
    use crate::app::Message;
    use document::entities::Line;
    use document::types::Vector3;

    #[test]
    fn block_def_apply_defines_the_block_and_consumes_the_preselected_objects() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(document::EntityType::Line(
            Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0)),
        ));
        app.tabs[i].scene.select_entity(line, true);

        let _ = app.run_command_line("BLOCK");

        let state = app
            .block_definition
            .as_ref()
            .expect("BLOCK opens the definition dialog");
        assert_eq!(
            state.selected_handles,
            vec![line],
            "the current selection carries into the dialog"
        );
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::BlockDefinition)
        );

        let _ = app.update(Message::BlockDefName("PANEL".into()));
        let _ = app.update(Message::BlockDefBaseX("2".into()));
        let _ = app.update(Message::BlockDefBaseY("3".into()));

        let _ = app.update(Message::BlockDefApply);

        assert!(
            app.block_definition.is_none(),
            "applying closes the dialog"
        );
        assert_eq!(app.active_modal, None);

        let record = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PANEL")
            .expect("applying defines the named block");
        assert!(
            !record.entity_handles.is_empty(),
            "the definition holds the converted objects"
        );
        assert!(
            app.tabs[i].scene.document.get_entity(line).is_none(),
            "convert mode consumes the source objects"
        );

        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(ins) if ins.block_name == "PANEL" => Some(ins.clone()),
                _ => None,
            })
            .expect("convert mode leaves a reference where the objects were");
        assert_eq!(
            insert.insert_point,
            Vector3::new(2.0, 3.0, 0.0),
            "the reference sits at the picked base point"
        );
        assert!(
            app.tabs[i].scene.selected.contains(&insert.common.handle),
            "the new reference is selected"
        );
    }
}


// -BLOCK from the command line: the name and base point steps run through
// the active command, and a preselection stands in for the select-objects
// step, so the block is defined in one pass.
mod dash_block_command_line_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::entities::Line;
    use document::types::Vector3;
    use glam::DVec3;

    #[test]
    fn dash_block_defines_the_block_from_a_preselection_and_a_typed_base() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(document::EntityType::Line(
            Line::from_points(Vector3::new(0.0, 0.0, 0.0), Vector3::new(10.0, 0.0, 0.0)),
        ));
        app.tabs[i].scene.select_entity(line, true);

        let _ = app.run_command_line("-BLOCK");
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("-BLOCK stays active to ask its prompts");
        assert_eq!(command.name(), "-BLOCK");
        assert_eq!(command.prompt(), "Enter block name or [?]:");

        let _ = app.feed_command(StepInput::Text("GEAR".into()));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("the base point step follows the name")
                .prompt(),
            "Specify insertion base point or [Annotative/mOde]:"
        );
        let _ = app.feed_command(StepInput::Point(DVec3::new(2.0, 3.0, 0.0)));

        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the preselection completes the command"
        );
        assert!(app.command_line.last_error.is_none());

        let record = app.tabs[i]
            .scene
            .document
            .block_records
            .get("GEAR")
            .expect("the command line defines the named block");
        assert!(
            !record.entity_handles.is_empty(),
            "the definition holds the preselected objects"
        );
        assert!(
            app.tabs[i].scene.document.get_entity(line).is_none(),
            "convert mode consumes the preselected objects"
        );
        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(ins) if ins.block_name == "GEAR" => Some(ins.clone()),
                _ => None,
            })
            .expect("convert mode leaves a reference where the objects were");
        assert_eq!(
            insert.insert_point,
            Vector3::new(2.0, 3.0, 0.0),
            "the reference sits at the typed base point"
        );
    }
}


// -INSERT takes its name and insertion point on one command line and places
// a reference of the named definition at that point.
mod dash_insert_placement_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::entities::{EntityType, Line};
    use document::types::Vector3;

    #[test]
    fn dash_insert_places_the_named_block_at_the_typed_point() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "GEAR",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );

        let _ = app.run_command_line("-INSERT GEAR 5,5");
        // Classic -INSERT keeps asking after the point: the line's trailing
        // Enter accepted the X scale <1> default, Y scale follows.
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("classic -INSERT asks the scale factors next")
                .prompt(),
            "Enter Y scale factor <use X scale factor>:"
        );
        let _ = app.feed_command(StepInput::Enter);
        let _ = app.feed_command(StepInput::Enter);

        assert!(
            app.tabs[i].active_cmd.is_none(),
            "name and point complete the placement: prompt={:?} out={:?}",
            app.tabs[i]
                .active_cmd
                .as_ref()
                .map(|c| (c.name(), c.prompt())),
            app.command_line.history_plain_text()
        );
        assert!(app.command_line.last_error.is_none());
        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(ins) if ins.block_name == "GEAR" => Some(ins.clone()),
                _ => None,
            })
            .expect("-INSERT places a reference of the named definition");
        assert_eq!(
            insert.insert_point,
            Vector3::new(5.0, 5.0, 0.0),
            "the reference lands on the typed point"
        );
        assert_eq!(
            app.tabs[i].insname, "GEAR",
            "the placed name is remembered for the next INSERT"
        );
    }
}


// The Scale option presets the factor before the insertion point, so the
// placement completes with that scale instead of asking the classic scale
// prompts.
mod dash_insert_scale_option_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::entities::{EntityType, Line};
    use document::types::Vector3;
    use glam::DVec3;

    #[test]
    fn dash_insert_scale_option_places_the_block_at_the_preset_scale() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "GEAR",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );

        let _ = app.run_command_line("-INSERT");
        let _ = app.feed_command(StepInput::Text("GEAR".into()));
        let _ = app.feed_command(StepInput::Text("S".into()));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("the Scale option asks for the factor")
                .prompt(),
            "Specify scale factor for XYZ axes <1>:"
        );
        let _ = app.feed_command(StepInput::Text("2".into()));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("the preset scale returns to the insertion point")
                .prompt(),
            "Specify insertion point or [Basepoint/Scale/X/Y/Z/Rotate/Explode/REpeat]:"
        );
        let _ = app.feed_command(StepInput::Point(DVec3::new(5.0, 5.0, 0.0)));
        let _ = app.feed_command(StepInput::Enter);

        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the preset scale places the block without the scale prompts: prompt={:?} err={:?}",
            app.tabs[i].active_cmd.as_ref().map(|c| (c.name(), c.prompt())),
            app.command_line.last_error
        );
        assert!(app.command_line.last_error.is_none());
        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                document::EntityType::Insert(ins) if ins.block_name == "GEAR" => Some(ins.clone()),
                _ => None,
            })
            .expect("the option sequence places a reference");
        let base = crate::scene::render_graph::block_base_point(
            &app.tabs[i].scene.document,
            "GEAR",
        );
        let xform =
            crate::scene::render_graph::insert_transform_with_base(&insert, base);
        let end = xform.apply(Vector3::new(10.0, 0.0, 0.0));
        assert_eq!(
            (end.x, end.y),
            (25.0, 5.0),
            "the 10-unit member is drawn twice as long from the typed point: {end:?}"
        );
    }
}


// BEDIT entered from the command line with a single reference preselected
// jumps straight into that block's editor; BSAVE saves and stays, and BCLOSE
// leaves.
mod bedit_command_line_entry_tests {
    use super::super::*;
    use document::entities::{EntityType, Insert, Line};
    use document::types::Vector3;

    #[test]
    fn a_preselected_reference_enters_the_editor_and_bsave_bclose_drive_it() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "VALVE",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let reference = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new(
                "VALVE",
                Vector3::new(20.0, 0.0, 0.0),
            )))
            .unwrap();
        app.tabs[i].scene.select_entity(reference, true);
        app.tabs[i].dirty = false;

        let _ = app.run_command_line("BEDIT");

        let session = app.tabs[i]
            .active_block_edit_session()
            .expect("a preselected reference enters the editor without the pick prompt");
        assert_eq!(
            session.br_handle,
            app.tabs[i]
                .scene
                .document
                .block_records
                .get("VALVE")
                .unwrap()
                .handle,
            "the editor opens on the referenced definition"
        );
        assert!(app.tabs[i].active_cmd.is_none(), "no pick step is installed");

        let result = app.automation_op(r#"{"op":"run","cmd":"CIRCLE 3,0 2"}"#);
        assert_eq!(result["ok"], true, "{result}");
        app.finish_pending_history(i);
        assert!(app.block_edit_dirty(i), "the drawn circle marks the editor dirty");

        let _ = app.dispatch_command("BSAVE");
        assert!(
            app.tabs[i].active_block_edit_session().is_some(),
            "BSAVE saves and stays in the editor"
        );
        assert!(!app.block_edit_dirty(i), "BSAVE clears the dirty flag");
        let members: Vec<_> = app.tabs[i]
            .scene
            .document
            .block_records
            .get("VALVE")
            .unwrap()
            .entity_handles
            .clone();
        assert_eq!(members.len(), 2, "the saved circle joins the definition");

        let _ = app.dispatch_command("BCLOSE");
        assert!(
            app.tabs[i].active_block_edit_session().is_none(),
            "BCLOSE leaves the editor"
        );
        assert_eq!(app.active_modal, None);
        let members: Vec<_> = app.tabs[i]
            .scene
            .document
            .block_records
            .get("VALVE")
            .unwrap()
            .entity_handles
            .clone();
        assert_eq!(
            members.len(),
            2,
            "the saved definition keeps its members after the editor closes"
        );
    }
}


// The whole dynamic-block chain - author a Linear parameter, attach a Move
// action, save, close, then drag the placed reference's parameter grip - is
// driven here through the public command surface rather than scene calls.
mod dynamic_block_command_surface_tests {
    use super::super::*;
    use crate::app::Message;
    use crate::command::StepInput;
    use crate::scene::block_actions as actions;
    use crate::scene::block_grips::runtime;
    use document::entities::{EntityType, Insert, Line};
    use document::types::Vector3;
    use glam::DVec3;

    fn authored_lines(app: &OpenCADStudio, insert: document::Handle) -> Vec<(DVec3, DVec3)> {
        let document::EntityType::Insert(reference) = app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(insert)
            .unwrap()
        else {
            panic!("the reference is an INSERT")
        };
        app.tabs[app.active_tab]
            .scene
            .document
            .entities_in_block(&reference.block_name)
            .filter_map(|entity| match entity {
                document::EntityType::Line(line) => {
                    Some((actions::v(line.start), actions::v(line.end)))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_command_authored_move_action_drives_the_placed_reference_grip() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "SLIDER",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let reference = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new("SLIDER", Vector3::ZERO)))
            .unwrap();
        app.tabs[i].dirty = false;
        app.tabs[i].scene.select_entity(reference, true);

        let _ = app.run_command_line("BEDIT");
        let i = app.active_tab;
        let definition = app.tabs[i]
            .active_block_edit_session()
            .unwrap()
            .br_handle;
        let member = app.tabs[i]
            .scene
            .document
            .block_records
            .get("SLIDER")
            .unwrap()
            .entity_handles[0];

        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,-2");
        let parameter = actions::parameters(
            &app.tabs[i].scene.document,
            definition,
            actions::Kind::Move,
        )[0]
        .clone();

        let _ = app.dispatch_command("BACTION MOVE");
        let _ = app.update(Message::CommandInput(parameter.name.clone()));
        let _ = app.update(Message::CommandSubmit);
        let _ = app.feed_command(StepInput::SelectionComplete(vec![member]));
        let _ = app.feed_command(StepInput::Enter);
        let _ = app.update(Message::CommandInput("3,4".into()));
        let _ = app.update(Message::CommandSubmit);
        assert!(app.tabs[i].active_cmd.is_none(), "the action was authored");
        assert!(app.command_line.last_error.is_none());
        assert_eq!(
            actions::actions(&app.tabs[i].scene.document, definition).len(),
            1,
            "one Move action follows the Linear parameter"
        );

        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
        let i = app.active_tab;
        let before = authored_lines(&app, reference);

        app.tabs[i].scene.select_entity(reference, true);
        app.tabs[i].selected_handle = Some(reference);
        app.refresh_properties();
        let grip_id = runtime::id(parameter.id, 1).expect("linear end grip");
        let start = app.tabs[i]
            .selected_grips
            .iter()
            .find(|grip| grip.id == grip_id)
            .expect("the placed reference offers the linear end grip")
            .clone();
        let target = start.world + start.dir.expect("linear grip has a direction") * 5.0;
        assert!(app.start_test_action_grip(reference, start.id));

        let _ = app.feed_command(StepInput::Point(target));

        assert!(app.tabs[i].active_cmd.is_none(), "the grip drag completes");
        assert_eq!(app.command_line.last_error, None);
        let after = authored_lines(&app, reference);
        assert_eq!(
            before,
            vec![(DVec3::ZERO, DVec3::new(10.0, 0.0, 0.0))],
            "the saved reference starts at the definition geometry"
        );
        assert_eq!(
            after,
            vec![(DVec3::new(5.0, 0.0, 0.0), target)],
            "the grip delta moves the action target by the same amount"
        );
        let moved = after[0];
        assert!(
            (moved.1.x - target.x).abs() < 1e-6,
            "the moved member follows the grip: {moved:?} target {target:?}"
        );
    }
}


mod placed_reference_property_tests;


// BPARAMETERSET composes an authored parameter with its action editor. An
// unrecognised label must be refused with the advertised message and must
// leave the editor document and its history untouched.
mod unsupported_parameter_set_tests {
    use super::super::*;
    use document::entities::{EntityType, Insert, Line};
    use document::types::Vector3;

    #[test]
    fn an_unsupported_parameter_set_label_is_refused_without_changing_the_editor() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "PANEL",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let reference = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new("PANEL", Vector3::ZERO)))
            .unwrap();
        app.tabs[i].dirty = false;
        app.tabs[i].scene.select_entity(reference, true);
        let _ = app.run_command_line("BEDIT");
        let i = app.active_tab;
        assert!(app.tabs[i].active_block_edit_session().is_some());

        let before = app.tabs[i].scene.document.clone();
        let history = app.tabs[i].history.undo_stack.len();

        let _ = app.run_command_line("BPARAMETERSET Radar Set");

        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("This Parameter Set is not supported yet."),
            "the unsupported label is refused with the advertised message"
        );
        assert_eq!(app.tabs[i].scene.document, before, "nothing was authored");
        assert_eq!(
            app.tabs[i].history.undo_stack.len(),
            history,
            "the refusal is not an undo step"
        );
        assert!(
            app.tabs[i].pending_parameter_set_follow_up.is_none(),
            "no follow-up action is armed"
        );
        assert!(app.tabs[i].active_cmd.is_none(), "no prompt is installed");

        // A label the palette actually offers still starts its workflow.
        let _ = app.run_command_line("BPARAMETERSET Visibility Set");
        assert!(app.tabs[i].active_cmd.is_some());
    }
}


// Saving or opening another editor while the Test Block window is in front
// is refused with a specific message; closing the Test Block releases the
// guard.
mod test_block_persistence_refusal_tests {
    use super::super::*;
    use document::entities::{EntityType, Insert, Line};
    use document::types::Vector3;

    #[test]
    fn saving_while_a_test_block_is_open_is_refused_until_it_is_closed() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let source = app.active_tab;
        app.tabs[source].scene.define_block_raw(
            "PANEL",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let reference = app.tabs[source]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new("PANEL", Vector3::ZERO)))
            .unwrap();
        app.tabs[source].dirty = false;
        app.tabs[source].scene.select_entity(reference, true);
        let _ = app.run_command_line("BEDIT");
        assert!(app.tabs[source].active_block_edit_session().is_some());

        let result = app.automation_op(r#"{"op":"run","cmd":"CIRCLE 3,0 2"}"#);
        assert_eq!(result["ok"], true, "{result}");
        app.finish_pending_history(source);
        assert!(app.block_edit_dirty(source));

        let _ = app.dispatch_command("BTESTBLOCK");
        let test = app.active_tab;
        assert_ne!(test, source, "the Test Block window is a separate tab");
        assert!(app.tabs[test].block_test.is_some());
        let before = app.tabs[test].scene.document.clone();

        let _ = app.run_command_line("BSAVE");

        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Close Test Block before saving or opening another block editor."),
            "the save is refused while the Test Block window is open"
        );
        assert_eq!(
            app.tabs[test].scene.document, before,
            "the refused save changes nothing"
        );
        assert_eq!(app.active_tab, test, "the Test Block window stays in front");

        let _ = app.run_command_line("BEDIT_OPEN:PANEL");
        assert_eq!(
            app.command_line.last_error.as_deref(),
            Some("Close Test Block before saving or opening another block editor."),
            "opening another editor is refused too"
        );

        let _ = app.dispatch_command("BCLOSETESTBLOCK");
        assert!(app.tabs[source].block_test.is_none());
        assert_eq!(app.active_tab, source, "closing returns to the source drawing");
        assert!(app.block_edit_dirty(source), "the edit is still unsaved");

        let _ = app.dispatch_command("BSAVE");
        assert!(!app.block_edit_dirty(source), "saving works again");
        assert!(app.tabs[source].active_block_edit_session().is_some());
    }
}


// The wblock bridge op can shift the exported selection so its bounds minimum
// lands on the origin; that opt-in path had no test.
mod wblock_normalized_export_tests {
    use super::super::*;

    #[test]
    fn a_normalized_wblock_export_moves_the_selection_to_the_origin() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 10,10 20,20"}"#);
        let handle = app.automation_op(r#"{"op":"query","type":"Line","detail":"summary"}"#)
            ["entities"][0]["handle"]
            .as_str()
            .unwrap()
            .to_owned();
        let doc = app.automation_op(r#"{"protocol":1,"op":"state"}"#)["document_id"]
            .as_u64()
            .unwrap();

        let path = dir.path().join("ocs_wblock_norm.dxf");
        let _ = std::fs::remove_file(&path);
        let escaped = path.to_string_lossy().replace('\\', "\\\\");
        let result = app.automation_op(&format!(
            r#"{{"protocol":1,"op":"wblock","request_id":"wb-norm","document_id":{doc},"path":"{escaped}","handles":["{handle}"],"normalize":true}}"#
        ));
        assert_eq!(result["ok"], true, "{}", result["error"]);

        let opened = app.automation_op(&format!(r#"{{"op":"open","path":"{escaped}"}}"#));
        assert_eq!(opened["ok"], true, "{}", opened["error"]);
        let line = &app.automation_op(r#"{"op":"query","type":"Line"}"#)["entities"][0];
        let start = line["start"].as_array().unwrap();
        let end = line["end"].as_array().unwrap();
        assert_eq!(
            (
                start[0].as_f64().unwrap(),
                start[1].as_f64().unwrap(),
                end[0].as_f64().unwrap(),
                end[1].as_f64().unwrap()
            ),
            (0.0, 0.0, 10.0, 10.0),
            "the exported line's bounds minimum is now the origin: {line}"
        );
        drop(app);
    }
}


// Defining a block under a name that already exists asks for confirmation;
// only confirming replaces the definition's body.
mod block_redefinition_dialog_tests {
    use super::super::*;
    use crate::app::Message;
    use document::entities::{EntityType, Line};
    use document::types::Vector3;

    #[test]
    fn confirming_a_block_redefinition_replaces_the_definition_body() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "PANEL",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let old = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PANEL")
            .unwrap()
            .entity_handles[0];
        let replacement = app.tabs[i].scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::new(30.0, 0.0, 0.0),
            Vector3::new(40.0, 0.0, 0.0),
        )));
        app.tabs[i].scene.select_entity(replacement, true);

        let _ = app.run_command_line("BLOCK");
        let _ = app.update(Message::BlockDefName("PANEL".into()));
        let _ = app.update(Message::BlockDefApply);

        assert_eq!(
            app.block_definition.as_ref().and_then(|s| s.confirm_redefine.as_deref()),
            Some("PANEL"),
            "the existing name asks for confirmation"
        );
        assert!(app.block_definition.is_some(), "the dialog stays open");
        assert!(
            app.tabs[i].scene.document.get_entity(old).is_some(),
            "the old definition is intact while the dialog waits"
        );

        let _ = app.update(Message::BlockDefConfirmRedefine(false));
        assert_eq!(
            app.block_definition.as_ref().and_then(|s| s.confirm_redefine.as_deref()),
            None,
            "declining clears the prompt"
        );
        assert!(app.block_definition.is_some(), "declining keeps the dialog open");
        assert!(app.tabs[i].scene.document.get_entity(old).is_some());

        let _ = app.update(Message::BlockDefApply);
        let _ = app.update(Message::BlockDefConfirmRedefine(true));

        assert!(app.block_definition.is_none(), "confirming closes the dialog");
        assert_eq!(app.active_modal, None);
        let doc = &app.tabs[i].scene.document;
        assert!(
            doc.get_entity(old).is_none(),
            "the old body entity was removed: old={old:?}"
        );
        let body: Vec<_> = doc.entities_in_block("PANEL").collect();
        assert_eq!(body.len(), 1, "the new body replaces the old one");
        assert!(
            matches!(body[0], EntityType::Line(ref line) if line.start.x == 30.0),
            "the replacement line is in the definition"
        );
        assert!(doc.get_entity(replacement).is_none(), "convert mode consumes the source");
        assert!(
            app.command_line.history_plain_text().contains("Block \"PANEL\" redefined"),
            "the redefinition is reported"
        );
    }
}


// The Attribute Definition dialog is exposed through the `attdef_dialog`
// action, but no test drives its fields and Ok end to end.
mod attdef_dialog_automation_tests {
    use super::super::*;
    use document::EntityType;

    fn field(app: &mut OpenCADStudio, value: &str) -> serde_json::Value {
        let mut req = serde_json::json!({"op":"action","name":"attdef_dialog","value":value});
        apply_control_request(app, &mut req)
    }

    fn apply_control_request(app: &mut OpenCADStudio, req: &mut serde_json::Value) -> serde_json::Value {
        let state = app.control_state();
        req["protocol"] = serde_json::json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        static SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let serial = SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        req["request_id"] = serde_json::json!(format!("attdef-dialog-{serial}"));
        let (reply, task) = app.control_request(req.clone());
        app.drive_headless_task(task).unwrap();
        reply
    }

    #[test]
    fn the_attdef_dialog_action_creates_a_positioned_definition() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let opened = app.automation_op(r#"{"op":"run","cmd":"ATTDEF"}"#);
        assert_eq!(opened["ok"], true, "{opened}");
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::AttDef),
            "ATTDEF opens the dialog"
        );

        for value in [
            "tag=DRAWING_NO",
            "prompt=Drawing number",
            "default=A-101",
            "height=2.5",
            "rotation=0",
            "x=5",
            "y=7",
            "on_screen=0",
        ] {
            let reply = field(&mut app, value);
            assert_eq!(reply["ok"], true, "{value}: {reply}");
        }
        let state = app.attdef_dialog.as_ref().expect("the dialog state is live");
        assert_eq!(state.tag, "DRAWING_NO");
        assert_eq!(state.prompt, "Drawing number");
        assert_eq!(state.default, "A-101");
        assert_eq!(state.coords[0], "5");
        assert_eq!(state.coords[1], "7");

        let mut ok_req = serde_json::json!({"op":"action","name":"dialog_ok"});
        let applied = apply_control_request(&mut app, &mut ok_req);
        assert_ne!(applied["code"], "no_dialog", "{applied}");
        assert!(app.attdef_dialog.is_none(), "Ok closes the dialog state");
        assert_ne!(app.active_modal, Some(crate::app::ModalKind::AttDef));

        let definition = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::AttributeDefinition(def) => Some(def.clone()),
                _ => None,
            })
            .expect("Ok writes an attribute definition");
        assert_eq!(definition.tag, "DRAWING_NO");
        assert_eq!(definition.prompt, "Drawing number");
        assert_eq!(definition.default_value, "A-101");
        assert_eq!(definition.height, 2.5);
        assert_eq!(
            (definition.insertion_point.x, definition.insertion_point.y),
            (5.0, 7.0),
            "the typed coordinates are the placement point"
        );
    }
}


// Bare ATTSYNC offers [?/Name/Select]; the Name path was never driven from
// the prompts - all existing coverage uses the inline `ATTSYNC <block>` form.
mod attsync_prompt_flow_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::entities::{AttributeDefinition, AttributeEntity, Insert};
    use document::types::Vector3;
    use document::EntityType;

    #[test]
    fn the_attsync_name_prompt_syncs_the_named_block() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let attdef = |name: &str, default: &str, y: f64| {
            EntityType::AttributeDefinition(AttributeDefinition {
                insertion_point: Vector3::new(2.0, y, 0.0),
                height: 1.0,
                ..AttributeDefinition::new(name.into(), name.into(), default.into())
            })
        };
        app.tabs[i].scene.define_block_raw(
            "SYNCED",
            Vector3::ZERO,
            vec![attdef("TAG1", "one", 2.0)],
        );
        let mut insert = Insert::new("SYNCED", Vector3::new(100.0, 100.0, 0.0));
        insert
            .attributes
            .push(AttributeEntity::new("TAG1".into(), "kept".into()));
        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(insert));
        app.tabs[i]
            .scene
            .redefine_block_raw(
                "SYNCED",
                Vector3::ZERO,
                vec![attdef("TAG1", "one", 2.0), attdef("TAG2", "two", 5.0)],
            )
            .expect("the definition takes the new tag");

        let _ = app.run_command_line("ATTSYNC");
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("the option prompt follows bare ATTSYNC")
                .prompt(),
            "Enter an option [?/Name/Select] <Select>:"
        );
        let _ = app.feed_command(StepInput::Text("N".into()));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("the Name option asks which block")
                .prompt(),
            "Enter name of block to sync or [?]:"
        );
        let _ = app.feed_command(StepInput::Text("SYNCED".into()));

        assert!(
            app.tabs[i].active_cmd.is_none(),
            "naming the block completes the sync"
        );
        assert!(app.command_line.last_error.is_none());
        let document::EntityType::Insert(updated) = app.tabs[i]
            .scene
            .document
            .get_entity(reference)
            .unwrap()
        else {
            panic!("the reference remains an INSERT")
        };
        let pairs: Vec<_> = updated
            .attributes
            .iter()
            .map(|attribute| (attribute.tag.clone(), attribute.get_value().to_string()))
            .collect();
        assert_eq!(
            pairs,
            vec![("TAG1".into(), "kept".into()), ("TAG2".into(), "two".into())],
            "the prompts sync the definition's tags onto the reference"
        );
    }
}


// The dash form -OBJECTSCALE runs its own [Add/Delete/?] prompt session; its
// option and scale-listing prompts were not covered.
mod dash_objectscale_prompt_tests {
    use super::super::*;
    use crate::command::StepInput;

    #[test]
    fn dash_objectscale_lists_scales_from_the_option_prompt() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let _ = app.run_command_line("TEXT 0,0 2 0 hello");
        let _ = app.run_command_line("SELECTALL");
        let handle = app.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .next()
            .map(|(handle, _)| handle)
            .unwrap();

        let _ = app.run_command_line("-OBJECTSCALE");
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("the dash command gathers annotated objects first");
        assert_eq!(command.name(), "-OBJECTSCALE");
        assert!(command.is_selection_gathering(), "{}", command.prompt());
        let _ = app.feed_command(StepInput::SelectionComplete(vec![handle]));
        let _ = app.feed_command(StepInput::Enter);
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("the option prompt follows the selection");
        assert!(
            command.prompt().contains("Add") || command.prompt().contains("?"),
            "the option prompt offers the scale list: {:?}",
            command.prompt()
        );
        let _ = app.feed_command(StepInput::Text("?".into()));
        assert!(
            app.command_line.history_plain_text().contains("1:1")
                || app.command_line.history_plain_text().contains("scale"),
            "the listing shows the drawing's scales: {}",
            app.command_line.history_plain_text()
        );
        assert!(
            app.tabs[i].active_cmd.is_some(),
            "the ? listing keeps the option prompt open"
        );
        assert!(app.command_line.last_error.is_none());
    }
}


// A selected annotation object exposes an Annotative Yes/No row in
// Properties; picking Yes drives the per-object annotative toggle.
mod annotative_properties_row_tests {
    use super::super::*;
    use document::entities::MText;
    use document::types::Vector3;
    use document::EntityType;

    fn row(app: &OpenCADStudio) -> crate::scene::model::object::Property {
        app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find(|property| property.field == "annotative")
            .expect("the Annotative row is in Properties")
            .clone()
    }

    #[test]
    fn the_annotative_properties_choice_toggles_the_object() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handle = app.tabs[i].scene.add_entity(EntityType::MText(Box::new(
            MText::with_value("hello".to_string(), Vector3::ZERO),
        )));
        app.tabs[i].scene.select_entity(handle, true);
        app.tabs[i].selected_handle = Some(handle);
        app.refresh_properties();

        let before = row(&app);
        assert_eq!(before.label, "Annotative");
        assert!(
            matches!(&before.value, crate::scene::model::object::PropValue::Choice { selected, options } if selected == "No" && options.contains(&"Yes".to_string())),
            "the row offers Yes/No and starts at No: {:?}",
            before.value
        );

        let _ = app
            .set_property_value(before, &serde_json::json!("Yes"))
            .unwrap();
        app.finish_pending_history(i);

        let document::EntityType::MText(text) = app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .unwrap()
        else {
            panic!("the entity stays an MTEXT")
        };
        assert!(text.is_annotative, "the Properties choice toggled the object");

        app.refresh_properties();
        assert!(
            matches!(&row(&app).value, crate::scene::model::object::PropValue::Choice { selected, .. } if selected == "Yes"),
            "the row reads back the new state"
        );
    }
}


// The ANNOALLVISIBLE command toggles the scene's per-layout annotation
// visibility; its observable effect on an offscale annotative object had no
// command-line test.
mod annotation_visibility_command_tests {
    use super::super::*;
    use document::entities::Text;
    use document::objects::Scale;
    use document::EntityType;

    #[test]
    fn annoallvisible_hides_and_shows_an_offscale_annotative_object() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let at_50 = crate::scene::annotative::ensure_scale_object(
            &mut app.tabs[i].scene.document,
            &Scale::new("1:50".into(), 1.0, 50.0),
        );
        let current = crate::scene::annotative::ensure_scale_object(
            &mut app.tabs[i].scene.document,
            &Scale::new("1:1".into(), 1.0, 1.0),
        );
        let handle = app.tabs[i].scene.add_entity(EntityType::Text(Text::new()));
        assert!(crate::scene::annotative::create_annotation_context(
            &mut app.tabs[i].scene.document,
            handle,
            at_50
        ));

        let offscale = |app: &OpenCADStudio| {
            let doc = &app.tabs[i].scene.document;
            let common = doc.get_entity(handle).unwrap().common();
            crate::scene::annotative::annotative_offscale_for(
                doc,
                common,
                Some(current),
                app.tabs[i].scene.annotation_all_visible(),
            )
        };

        assert!(!offscale(&app), "all objects show by default in Model");
        let _ = app.run_command_line("ANNOALLVISIBLE 0");
        assert!(!app.tabs[i].scene.annotation_all_visible());
        assert!(offscale(&app), "the toggle hides the offscale object");
        assert!(app.tabs[i].dirty, "the toggle marks the drawing dirty");

        let _ = app.run_command_line("ANNOALLVISIBLE 1");
        assert!(app.tabs[i].scene.annotation_all_visible());
        assert!(!offscale(&app), "the object shows again");
    }
}


// An annotative block definition carries its flag through the BLOCK dialog
// into the file, and a placed reference reports the same state in Properties.
mod annotative_block_end_to_end_tests {
    use super::super::*;
    use crate::app::Message;
    use document::entities::{EntityType, Insert, Line};
    use document::types::Vector3;

    #[test]
    fn an_annotative_block_definition_marks_its_reference_annotative() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(EntityType::Line(Line::from_points(
            Vector3::ZERO,
            Vector3::new(10.0, 0.0, 0.0),
        )));
        app.tabs[i].scene.select_entity(line, true);

        let _ = app.run_command_line("BLOCK");
        let _ = app.update(Message::BlockDefName("ANNOBLOCK".into()));
        let _ = app.update(Message::BlockDefAnnotative(true));
        let _ = app.update(Message::BlockDefApply);

        assert!(app.block_definition.is_none(), "the dialog applied");
        assert!(
            crate::scene::annotative::block_is_annotative(
                &app.tabs[i].scene.document,
                "ANNOBLOCK"
            ),
            "the Annotative checkbox marked the definition"
        );

        let reference = app.tabs[i].scene.add_entity(EntityType::Insert(Insert::new(
            "ANNOBLOCK",
            Vector3::new(5.0, 5.0, 0.0),
        )));
        app.tabs[i].scene.select_entity(reference, true);
        app.tabs[i].selected_handle = Some(reference);
        app.refresh_properties();

        let row = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find(|property| property.field == "annotative")
            .expect("the reference offers an Annotative row");
        let shown = match &row.value {
            crate::scene::model::object::PropValue::Choice { selected, .. } => selected.clone(),
            crate::scene::model::object::PropValue::ReadOnly(value) => value.clone(),
            other => panic!("unexpected Annotative row: {other:?}"),
        };
        assert_eq!(shown, "Yes", "the reference is annotative through its definition");
    }
}


// The -TABLE command-line form asks for columns, rows and an insertion point
// before placing a native table entity; no test drove that flow.
mod table_command_line_creation_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::EntityType;
    use glam::DVec3;

    #[test]
    fn dash_table_places_a_native_table_from_its_prompts() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let _ = app.run_command_line("-TABLE");
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("-TABLE stays active to ask for columns");
        assert_eq!(command.name(), "-TABLE");
        assert!(command.prompt().contains("number of columns"), "{}", command.prompt());

        let _ = app.feed_command(StepInput::Text("3".into()));
        assert!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("rows follow the columns")
                .prompt()
                .contains("number of rows")
        );
        let _ = app.feed_command(StepInput::Text("2".into()));
        let _ = app.feed_command(StepInput::Point(DVec3::new(5.0, 7.0, 0.0)));

        assert!(app.tabs[i].active_cmd.is_none(), "the point places the table");
        assert!(app.command_line.last_error.is_none());
        let table = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Table(table) => Some(table.clone()),
                _ => None,
            })
            .expect("a native table entity was created");
        assert_eq!(table.columns.len(), 3, "the typed column count was used");
        assert_eq!(
            table.rows.len(),
            4,
            "two typed data rows plus the title and header rows"
        );
        assert_eq!(
            (table.insertion_point.x, table.insertion_point.y),
            (5.0, 7.0),
            "the table is placed at the typed point"
        );
    }
}


// The DATALINK command's scriptable form reads a CSV from a typed path and
// places a linked table; that path had no test.
mod datalink_csv_command_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::EntityType;
    use glam::DVec3;

    #[test]
    fn datalink_reads_a_csv_and_registers_a_linked_table() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;

        let path = dir.path().join("ocs_datalink.csv");
        std::fs::write(&path, "NAME,QTY\nBolt,12\nNut,30\n").unwrap();
        let escaped = path.to_string_lossy().replace('\\', "/");
        let task = app.run_command_line(&format!("DATALINK {escaped}"));
        app.drive_headless_task(task).expect("the read task");
        let _ = app.feed_command(StepInput::Point(DVec3::new(10.0, 10.0, 0.0)));

        assert!(app.command_line.last_error.is_none());
        let doc = &app.tabs[i].scene.document;
        let links = crate::app::annotation_data::data_link_entries(doc);
        assert_eq!(
            links.len(),
            1,
            "one persistent link was registered: {}",
            app.command_line.history_plain_text()
        );
        let mut table = None;
        for entity in doc.entities() {
            if let EntityType::Table(candidate) = entity {
                if (0..candidate.rows.len()).any(|row| {
                    (0..candidate.columns.len()).any(|column| {
                        candidate
                            .cell(row, column)
                            .is_some_and(|cell| cell.has_linked_data)
                    })
                })
                {
                    table = Some(candidate.clone());
                }
            }
        }
        let table = table.expect("a table linked to the CSV was placed");
        let texts: Vec<_> = (0..table.rows.len())
            .flat_map(|row| (0..table.columns.len()).map(move |column| (row, column)))
            .filter_map(|(row, column)| table.cell_text(row, column))
            .map(str::to_string)
            .collect();
        assert!(
            texts.iter().any(|text| text.contains("Bolt")),
            "the CSV rows reached the table: {texts:?}"
        );
        drop(app);
        let _ = std::fs::remove_file(&path);
    }
}


// A table cell can hold a block; the Table Cell Block window's automation
// fields drive that path, but no test covered the action + Ok round.
mod table_cell_block_dialog_tests {
    use super::super::*;
    use crate::scene::TableCellSelection;
    use document::entities::{Line, Table};
    use document::types::Vector3;
    use document::EntityType;

    fn action(app: &mut OpenCADStudio, value: &str) -> serde_json::Value {
        static SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let mut req = serde_json::json!({"op":"action","name":"table_cell_dialog","value":value});
        let state = app.control_state();
        req["protocol"] = serde_json::json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["request_id"] = serde_json::json!(format!(
            "tcell-{}",
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let (reply, task) = app.control_request(req);
        app.drive_headless_task(task).unwrap();
        reply
    }

    #[test]
    fn the_table_cell_block_dialog_installs_a_block_in_the_cell() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "CELLBLK",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(2.0, 0.0, 0.0),
            ))],
        );
        let block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("CELLBLK")
            .unwrap()
            .handle;
        let table = app.tabs[i].scene.add_entity(EntityType::Table(Box::new(
            Table::new(Vector3::ZERO, 2, 2),
        )));
        app.tabs[i]
            .scene
            .select_table_cells(TableCellSelection {
                table,
                anchor: (0, 0),
                end: (0, 0),
            });

        let _ = app.run_command_line("_TCELLBLOCK");
        assert_eq!(
            app.active_modal,
            Some(crate::app::ModalKind::TableCellBlock),
            "the Block tab of the cell dialog opened"
        );
        assert_eq!(action(&mut app, "block=CELLBLK")["ok"], true);
        assert_eq!(action(&mut app, "ok")["ok"], true);
        assert_ne!(app.active_modal, Some(crate::app::ModalKind::TableCellBlock));

        let document::EntityType::Table(table) = app.tabs[i]
            .scene
            .document
            .get_entity(table)
            .unwrap()
        else {
            panic!("the table remains a Table entity")
        };
        assert_eq!(
            crate::app::command_driver::clipboard::table_cell_blocks(table).collect::<Vec<_>>(),
            vec![block],
            "the cell now points at the chosen block definition"
        );
        assert_eq!(
            table.rows[0].cells[0].cell_type,
            document::entities::table::CellType::Block,
            "the cell's type follows its content"
        );
    }
}


mod viewport_properties_tests;


// VPCLIP's polygonal boundary flow had no test: pick the viewport, choose
// Polygonal, draw a closed shape and check the viewport is clipped to it.
mod vpclip_polygonal_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::entities::Viewport;
    use document::types::Vector3;
    use document::EntityType;
    use glam::DVec3;

    #[test]
    fn vpclip_clips_a_viewport_to_a_drawn_polygon() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handle = {
            let scene = &mut app.tabs[i].scene;
            scene.document.add_layout("Sheet").unwrap();
            scene.set_current_layout("Sheet".into());
            let mut viewport = Viewport::new();
            viewport.id = 2;
            viewport.center = Vector3::new(50.0, 50.0, 0.0);
            viewport.width = 100.0;
            viewport.height = 100.0;
            viewport.view_height = 1000.0;
            viewport.turn_on();
            scene.add_entity(EntityType::Viewport(Box::new(viewport)))
        };

        let _ = app.run_command_line("VPCLIP");
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("VPCLIP asks for a viewport")
                .prompt(),
            "Select viewport to clip:"
        );
        let _ = app.feed_command(StepInput::EntityPick(handle, DVec3::ZERO));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("then asks for the clipping object")
                .prompt(),
            "Select clipping object or [Polygonal] <Polygonal>:"
        );
        let _ = app.feed_command(StepInput::Enter);
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("Polygonal asks for the start point")
                .prompt(),
            "Specify start point:"
        );
        for point in [(10.0, 10.0), (30.0, 10.0), (30.0, 25.0), (10.0, 25.0)] {
            let _ = app.feed_command(StepInput::Point(DVec3::new(point.0, point.1, 0.0)));
        }
        let _ = app.feed_command(StepInput::Text("C".into()));

        assert!(
            app.tabs[i].active_cmd.is_none(),
            "closing the polygon completes VPCLIP"
        );
        assert!(app.command_line.last_error.is_none());
        let document::EntityType::Viewport(clipped) =
            app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("the viewport remains a Viewport")
        };
        assert!(
            !clipped.clip_boundary_handle.is_null(),
            "a clip boundary entity was created"
        );
        assert!(
            clipped.status.non_rectangular_clipping,
            "the viewport is marked as non-rectangularly clipped"
        );
        assert!(
            (clipped.width - 20.0).abs() < 1e-6 && (clipped.height - 15.0).abs() < 1e-6,
            "the viewport shrank to the drawn 20x15 boundary: {}x{}",
            clipped.width,
            clipped.height
        );
    }
}


// PLOTSTYLE's STATUS/CLEAR forms expose the loaded plot style table; no
// command-surface test covered them.
mod plotstyle_command_tests {
    use super::super::*;
    use crate::app::Message;
    use crate::io::plot_style::PickedPlotStyle;
    use document::plot_style::{PlotStyleEntry, PlotStyleTable};

    #[test]
    fn plotstyle_status_reports_the_loaded_table_and_clear_removes_it() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let layout = app.tabs[i].scene.current_layout.clone();
        let mut table = PlotStyleTable::identity("review.ctb".to_string());
        table.named_entries.insert(
            "Normal".into(),
            PlotStyleEntry {
                name: "Normal".into(),
                color: Some([255, 0, 0]),
                ..Default::default()
            },
        );
        let picked = PickedPlotStyle::from_bytes(
            "review.ctb".into(),
            table.to_bytes().unwrap(),
        )
        .unwrap();
        let _ = app.update(Message::PlotStyleLoaded(
            app.tabs[i].id,
            layout.clone(),
            Ok(Some(picked)),
        ));

        let _ = app.run_command_line("PLOTSTYLE STATUS");
        let text = app.command_line.history_plain_text();
        assert!(
            text.contains("Plot style: review.ctb"),
            "STATUS reports the active table: {text}"
        );
        assert!(app.active_plot_style.is_some());

        let clear = app.run_command_line("PLOTSTYLE CLEAR");
        app.drive_headless_task(clear).unwrap();
        assert!(app.active_plot_style.is_none(), "CLEAR removes the table");
        let _ = app.run_command_line("PLOTSTYLE STATUS");
        assert!(
            app.command_line.history_plain_text().contains("No plot style table loaded."),
            "STATUS now reports no table"
        );
    }
}


// The External References palette's selection + operation path has no test:
// opening the panel, selecting a row and detaching it must change the drawing.
mod xref_palette_op_tests {
    use super::super::*;
    use crate::app::Message;
    use crate::ui::window::xref_manager::{XrefPaletteOp, XrefRowId};
    use document::tables::BlockRecord;

    #[test]
    fn the_xref_palette_detaches_the_selected_reference() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut block = BlockRecord::new("PLAN");
        block.flags.is_xref = true;
        block.xref_path = "plans/plan.dwg".to_string();
        block.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(block).unwrap();

        let _ = app.run_command_line("EXTERNALREFERENCES");
        assert!(app.show_external_references, "the panel opened");
        let row = app
            .xref_manager
            .entries
            .iter()
            .find(|entry| entry.name == "PLAN")
            .expect("the palette lists the xref")
            .clone();
        let id = XrefRowId::of(row.key, &row.saved_path);
        let _ = app.update(Message::XrefManagerSelect(id));

        let _ = app.update(Message::XrefManagerOp(XrefPaletteOp::Detach));

        assert!(
            app.tabs[i].scene.document.block_records.get("PLAN").is_none(),
            "the palette op detached the reference"
        );
        assert!(
            app.command_line.history_plain_text().contains("DETACH")
                || app.command_line.history_plain_text().contains("detach"),
            "the palette op reported what it did: {}",
            app.command_line.history_plain_text()
        );
    }
}


// The xref path-type conversion is exposed through the control protocol's
// xref_set_path op; that surface had no test.
mod xref_pathtype_control_tests {
    use super::super::*;
    use document::tables::BlockRecord;

    fn set_path(app: &mut OpenCADStudio, id: &str, path_type: &str) -> serde_json::Value {
        static SERIAL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let mut req = serde_json::json!({
            "op": "xref_set_path",
            "id": id,
            "path_type": path_type
        });
        let state = app.control_state();
        req["protocol"] = serde_json::json!(1);
        req["document_id"] = state["document_id"].clone();
        req["revision"] = state["revision"].clone();
        req["request_id"] = serde_json::json!(format!(
            "xref-path-{}",
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let (reply, task) = app.control_request(req);
        app.drive_headless_task(task).unwrap();
        reply
    }

    #[test]
    fn xref_set_path_converts_between_none_and_full_paths() {
        let dir = tempfile::tempdir().expect("a temporary directory");
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let host = dir.path().join("ocs_xref_host.dwg");
        assert_eq!(
            app.automation_op(&format!(
                r#"{{"op":"save","path":"{}"}}"#,
                host.to_string_lossy().replace('\\', "\\\\")
            ))["ok"],
            true
        );
        let mut block = BlockRecord::new("PLAN");
        block.flags.is_xref = true;
        block.xref_path = "plans/plan.dwg".to_string();
        block.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(block).unwrap();

        let refs = app.automation_op(r#"{"op":"xrefs"}"#);
        let id = refs["xrefs"][0]["id"].as_str().unwrap().to_string();

        let none = set_path(&mut app, &id, "none");
        assert_eq!(none["ok"], true, "{none}");
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get("PLAN")
                .unwrap()
                .xref_path,
            "plan.dwg",
            "no-path mode strips the directory"
        );

        let full = set_path(&mut app, &id, "full");
        assert_eq!(full["ok"], true, "{full}");
        let stored = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap()
            .xref_path
            .clone();
        assert!(
            stored.ends_with("plan.dwg")
                && (stored.starts_with("C:") || stored.starts_with('/')),
            "full mode stores an absolute path: {stored}"
        );
    }
}


// XREF's Overlay option changes an existing reference's semantics; that
// success path had no test (only the image-refusal path was covered).
mod xref_overlay_option_tests {
    use super::super::*;
    use document::tables::BlockRecord;

    #[test]
    fn xref_overlay_option_flips_an_attached_reference_to_overlay() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut block = BlockRecord::new("PLAN");
        block.flags.is_xref = true;
        block.xref_path = "plan.dwg".to_string();
        block.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(block).unwrap();

        let _ = app.run_command_line("XREF Overlay PLAN");

        let record = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap();
        assert!(
            record.flags.is_xref_overlay && !record.flags.is_xref,
            "the reference is now an overlay: {:?}",
            record.flags
        );
        assert!(
            app.command_line.history_plain_text().contains("set to Overlay"),
            "the conversion is reported: {}",
            app.command_line.history_plain_text()
        );

        // An overlay stays an overlay when the command is repeated.
        let _ = app.run_command_line("XREF Attach PLAN");
        let record = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap();
        assert!(
            !record.flags.is_xref && record.flags.is_xref_overlay,
            "the simple Attach-on-name path does not silently re-attach an overlay"
        );
    }
}


// The CLIP command's boundary workflow on a drawing-space reference had no
// test (XCLIP was only covered inside the block editor).
mod clip_boundary_workflow_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::entities::{Insert, Line};
    use document::types::Vector3;
    use document::EntityType;
    use glam::DVec3;

    #[test]
    fn clip_adds_a_rectangular_boundary_to_a_placed_reference() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "VALVE",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let insert = app.tabs[i]
            .scene
            .add_entity(EntityType::Insert(Insert::new("VALVE", Vector3::ZERO)));

        let _ = app.run_command_line("CLIP");
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("CLIP asks which object to clip")
                .prompt(),
            "Select Object to clip:"
        );
        let _ = app.feed_command(StepInput::EntityPick(insert, DVec3::ZERO));
        let _ = app.feed_command(StepInput::Text("N".into()));
        let _ = app.feed_command(StepInput::Text("R".into()));
        let _ = app.feed_command(StepInput::Point(DVec3::new(1.0, 1.0, 0.0)));
        let _ = app.feed_command(StepInput::Point(DVec3::new(8.0, 7.0, 0.0)));

        assert!(
            app.tabs[i].active_cmd.is_none(),
            "the boundary completes CLIP"
        );
        assert!(app.command_line.last_error.is_none());
        assert!(
            crate::scene::pick::xclip::filter_handle(
                &app.tabs[i].scene.document,
                insert
            )
            .is_some(),
            "the reference now carries a clip filter"
        );
        assert!(
            app.tabs[i].scene.document.get_entity(insert).is_some(),
            "the reference itself stays in the drawing"
        );
    }
}


// Bare REFCLOSE (with its Save/Discard option prompt) had no test; the
// internal REFCLOSE_SAVE/REFCLOSE_DISCARD forms were covered.
mod refclose_command_tests {
    use super::super::*;
    use crate::command::StepInput;
    use document::entities::{Insert, Line};
    use document::types::Vector3;
    use document::EntityType;

    #[test]
    fn refclose_prompt_saves_an_in_place_reference_edit() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "PANEL",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let reference = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new("PANEL", Vector3::ZERO)))
            .unwrap();
        app.tabs[i].dirty = false;
        app.tabs[i].scene.select_entity(reference, true);

        let _ = app.run_command_line("-REFEDIT");
        let command = app.tabs[i]
            .active_cmd
            .as_ref()
            .expect("-REFEDIT asks about the nesting level");
        assert!(command.prompt().contains("Select nesting level"), "{}", command.prompt());
        let _ = app.feed_command(StepInput::Text("O".into()));
        let _ = app.feed_command(StepInput::Enter);
        let _ = app.feed_command(StepInput::Enter);
        assert!(
            app.tabs[i].refedit_session.is_some(),
            "REFEDIT checked out the preselected reference"
        );
        let before = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PANEL")
            .unwrap()
            .entity_handles
            .len();

        let result = app.automation_op(r#"{"op":"run","cmd":"CIRCLE 3,0 2"}"#);
        assert_eq!(result["ok"], true, "{result}");
        app.finish_pending_history(i);

        let _ = app.run_command_line("REFCLOSE");
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .expect("REFCLOSE asks what to do with the changes")
                .prompt(),
            "Enter option [Save/Discard reference changes] <Save>:"
        );
        let _ = app.feed_command(StepInput::Enter);

        assert!(app.tabs[i].refedit_session.is_none(), "the session closed");
        let after = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PANEL")
            .unwrap()
            .entity_handles
            .len();
        assert_eq!(after, before + 1, "the drawn circle was saved to the definition");
    }
}


// The UNDERLAY command edits selected PDF/DGN underlays; no test drove
// its property options from the command line.
mod underlay_command_tests {
    use super::super::*;
    use document::entities::{Underlay, UnderlayDisplayFlags, UnderlayType};
    use document::EntityType;

    #[test]
    fn underlay_command_edits_the_selected_underlay() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let handle = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Underlay(Underlay::new(UnderlayType::Pdf)))
            .unwrap();
        app.tabs[i].scene.select_entity(handle, true);

        for command in [
            "UNDERLAY FADE 50",
            "UNDERLAY CONTRAST 70",
            "UNDERLAY MONO ON",
            "UNDERLAY CLIP ON",
            "UNDERLAY OFF",
        ] {
            let _ = app.run_command_line(command);
            assert!(app.command_line.last_error.is_none(), "{command}");
        }

        let document::EntityType::Underlay(underlay) =
            app.tabs[i].scene.document.get_entity(handle).unwrap()
        else {
            panic!("the underlay remains an Underlay")
        };
        assert_eq!(underlay.fade, 50, "the fade was set");
        assert_eq!(underlay.contrast, 70, "the contrast was set");
        assert!(
            underlay.flags.contains(UnderlayDisplayFlags::MONOCHROME),
            "monochrome was switched on"
        );
        assert!(
            underlay.flags.contains(UnderlayDisplayFlags::CLIPPING),
            "clipping was switched on"
        );
        assert!(!underlay.is_on(), "the underlay was switched off");
    }
}


// The Properties panel switches its rows to the selected entity's type: a
// Table gets table-only rows that a Line does not.
mod entity_specific_properties_tests {
    use super::super::*;
    use document::entities::Table;
    use document::types::Vector3;
    use document::EntityType;

    fn has_field(app: &OpenCADStudio, field: &str) -> bool {
        app.tabs[app.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .any(|property| property.field == field)
    }

    #[test]
    fn a_table_shows_table_rows_that_a_line_does_not() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let table = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Table(Box::new(Table::new(
                Vector3::ZERO,
                2,
                2,
            ))))
            .unwrap();
        let line = app.tabs[i].scene.add_entity(EntityType::Line(
            document::entities::Line::from_points(Vector3::ZERO, Vector3::new(1.0, 0.0, 0.0)),
        ));

        app.tabs[i].scene.select_entity(table, true);
        app.tabs[i].selected_handle = Some(table);
        app.refresh_properties();
        assert!(has_field(&app, "tbl_style_handle"), "a table style row is shown");
        assert!(has_field(&app, "tbl_rows"), "the row count is shown");
        assert!(has_field(&app, "tbl_cols"), "the column count is shown");
        assert!(has_field(&app, "tbl_flow_direction"), "the direction row is shown");

        let direction = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find(|property| property.field == "tbl_flow_direction")
            .unwrap()
            .clone();
        let crate::scene::model::object::PropValue::Choice { ref selected, .. } = direction.value
        else {
            panic!("the direction row is a choice")
        };
        let wanted = if selected == "Up" { "Down" } else { "Up" };
        let _ = app
            .set_property_value(direction, &serde_json::json!(wanted))
            .unwrap();
        app.finish_pending_history(i);
        app.refresh_properties();
        let after = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .find(|property| property.field == "tbl_flow_direction")
            .unwrap();
        assert!(
            matches!(&after.value, crate::scene::model::object::PropValue::Choice { selected, .. } if selected == wanted),
            "the table direction changed the row"
        );

        app.tabs[i].scene.select_entity(line, true);
        app.tabs[i].selected_handle = Some(line);
        app.refresh_properties();
        assert!(
            !has_field(&app, "tbl_style_handle"),
            "a line does not get table rows"
        );
    }
}


// A quoted 'QSELECT can run transparently inside a command that is asking for
// objects; that hand-back path had no test.
mod transparent_qselect_tests {
    use super::super::*;
    use crate::app::Message;
    use document::EntityType;

    #[test]
    fn transparent_qselect_hands_its_matches_back_to_the_running_command() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let _ = app.run_command_line("LINE 0,0 10,0");
        let _ = app.run_command_line("CIRCLE 20,20 5");
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| matches!(entity, EntityType::Line(_)).then(|| entity.common().handle))
            .unwrap();
        let circle = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| matches!(entity, EntityType::Circle(_)).then(|| entity.common().handle))
            .unwrap();

        let _ = app.run_command_line("MOVE");
        assert!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.is_selection_gathering()),
            "MOVE is asking for objects"
        );
        let _ = app.run_command_line("'QSELECT");
        assert!(app.qselect.is_some(), "Quick Select opened transparently");
        assert!(app.transparent_select.is_some(), "MOVE was parked");

        let _ = app.update(Message::QSelectSetType(Some("Circle".into())));
        let task = app.update(Message::QSelectApply);
        app.drive_headless_task(task).unwrap();

        assert!(app.qselect.is_none(), "Quick Select closed");
        assert!(app.transparent_select.is_none(), "the parked command resumed");
        assert!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .is_some_and(|command| command.is_selection_gathering()
                    && command.prompt().starts_with("MOVE")),
            "the parent MOVE command is back at its object prompt: {:?}",
            app.tabs[i]
                .active_cmd
                .as_ref()
                .map(|command| (command.name(), command.prompt()))
        );
        let selected: Vec<_> = app.tabs[i].scene.selected.iter().copied().collect();
        assert!(selected.contains(&circle) && !selected.contains(&line), "only the circle was handed back: {selected:?}");
    }
}


// A converted dimensional constraint exposes Constraint Form and Reference
// rows in Properties; driving them had no test.
mod dimensional_constraint_properties_tests {
    use super::super::*;
    use crate::command::StepInput;
    use crate::scene::parametric_constraints::DYNAMIC_DIMENSION_LAYER;
    use document::EntityType;

    #[test]
    fn a_dimensional_constraints_properties_drive_its_form_and_reference() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let _ = app.automation_op(r#"{"op":"run","cmd":"LINE 0,0 40,0"}"#);
        let i = app.active_tab;
        let line = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| matches!(entity, EntityType::Line(_)).then(|| entity.common().handle))
            .unwrap();
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

        let _ = app.run_command_line("DCCONVERT");
        let _ = app.feed_command(StepInput::SelectionComplete(vec![dimension]));
        let _ = app.feed_command(StepInput::Enter);

        app.tabs[i].scene.select_entity(dimension, true);
        app.tabs[i].selected_handle = Some(dimension);
        app.refresh_properties();
        let rows: Vec<_> = app.tabs[i]
            .properties
            .sections
            .iter()
            .flat_map(|section| &section.props)
            .cloned()
            .collect();
        let form = rows
            .iter()
            .find(|property| property.field == "dyn_constraint_form")
            .expect("the Constraint Form row is shown")
            .clone();
        assert!(
            matches!(&form.value, crate::scene::model::object::PropValue::Choice { selected, .. } if selected == "Dynamic"),
            "a converted dimension starts dynamic: {:?}",
            form.value
        );
        assert!(
            rows.iter().any(|property| property.field == "dyn_constraint_reference"),
            "the Reference row is shown too"
        );
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(dimension)
                .unwrap()
                .common()
                .layer,
            DYNAMIC_DIMENSION_LAYER
        );

        let _ = app
            .set_property_value(form, &serde_json::json!("Annotational"))
            .unwrap();
        app.finish_pending_history(i);
        assert_ne!(
            app.tabs[i]
                .scene
                .document
                .get_entity(dimension)
                .unwrap()
                .common()
                .layer,
            DYNAMIC_DIMENSION_LAYER,
            "the Annotational form moves it off the constraints layer"
        );
    }
}

#[test]
fn plugin_command_text_input_routes_single_token_free_text_and_enter() {
    use ocs_plugin_api::host::{CommandStep, HostApi, InteractiveCommand, InteractiveInputKind};
    use std::sync::{Arc, Mutex};

    struct MockTextCmd {
        step: u8,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl InteractiveCommand for MockTextCmd {
        fn prompt(&self) -> String {
            match self.step {
                0 => "Enter count:".into(),
                _ => "Enter label:".into(),
            }
        }
        fn on_point(&mut self, _pt: [f64; 3]) -> CommandStep {
            CommandStep::NeedPoint
        }
        fn input_kind(&self) -> InteractiveInputKind {
            match self.step {
                0 => InteractiveInputKind::SingleToken,
                _ => InteractiveInputKind::FreeText,
            }
        }
        fn on_text_input(&mut self, text: &str) -> CommandStep {
            self.seen.lock().unwrap().push(format!("text:{text}"));
            self.step += 1;
            CommandStep::NeedPoint
        }
        fn on_enter(&mut self) -> CommandStep {
            self.seen.lock().unwrap().push("enter".into());
            CommandStep::Done
        }
    }

    let seen = Arc::new(Mutex::new(Vec::new()));
    let mut app = crate::app::OpenCADStudio::new_for_test();
    let _ = app.automation_op(r#"{"op":"new"}"#);
    let i = app.active_tab;
    {
        let mut host = crate::app::plugin_host::HostSession::new(&mut app, i);
        host.start_interactive(Box::new(MockTextCmd {
            step: 0,
            seen: Arc::clone(&seen),
        }));
    }
    app.sync_dyn_fields();
    assert!(app.plugin_dialog_state.is_none());
    assert!(app.tabs[i].dyn_fields.is_empty());

    let _ = app.update(crate::app::Message::CommandInput("42".into()));
    let _ = app.update(crate::app::Message::CommandSubmit);
    assert!(app.tabs[i].active_cmd.is_some());
    assert!(app.plugin_dialog_state.is_none());

    let _ = app.update(crate::app::Message::CommandInput("Hello World".into()));
    assert_eq!(app.command_line.input, "Hello World");
    let _ = app.update(crate::app::Message::CommandSubmit);
    assert!(app.tabs[i].active_cmd.is_some());
    assert!(app.plugin_dialog_state.is_none());

    let _ = app.update(crate::app::Message::CommandSubmit);
    assert!(app.tabs[i].active_cmd.is_none());
    assert_eq!(
        *seen.lock().unwrap(),
        vec!["text:42", "text:Hello World", "enter"]
    );
}
