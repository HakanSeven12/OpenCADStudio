//! App-level LAYWALK tests: the override, its lifecycle, and the actions the
//! pallet can take on the drawing.

#[cfg(test)]
mod tests {
    use crate::app::{Message, OpenCADStudio};
    use crate::ui::dock::PanelId;
    use crate::ui::window::layer_walk::{LayerWalkMenuItem, LayerWalkMsg};
    use codec::entities::Line;
    use codec::EntityType;

    fn fresh_app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    /// The walk keys its set on layer handles, so a test layer needs a real
    /// one — exactly what the LAYER command does when it creates a table row.
    fn add_layer(app: &mut OpenCADStudio, tab: usize, name: &str) {
        let mut layer = codec::tables::Layer::new(name);
        layer.handle = app.tabs[tab].scene.document.allocate_handle();
        let _ = app.tabs[tab].scene.document.layers.add(layer);
    }

    /// `WALLS` / `DOOR` with two / one objects, `GHOST` with none. The current
    /// layer stays on `0`, and no layer gets any protection from the walk.
    fn three_layer_drawing() -> OpenCADStudio {
        let mut app = fresh_app();
        let i = app.active_tab;
        for name in ["WALLS", "DOOR", "GHOST"] {
            add_layer(&mut app, i, name);
        }
        for (layer, n) in [("WALLS", 2), ("DOOR", 1)] {
            for _ in 0..n {
                let mut line = Line::new();
                line.common.layer = layer.to_string();
                let _ = app.tabs[i]
                    .scene
                    .document
                    .add_entity(EntityType::Line(line));
            }
        }
        app
    }

    fn walk(app: &mut OpenCADStudio, msg: LayerWalkMsg) {
        let _ = app.update(Message::Laywalk(msg));
    }

    /// The layers the walk currently hides, by name.
    fn hidden(app: &OpenCADStudio, names: &[&str]) -> Vec<String> {
        let tab = &app.tabs[app.active_tab];
        names
            .iter()
            .filter(|n| {
                tab.scene
                    .layer_walk_hidden(tab.scene.document.layers.get(n))
            })
            .map(|n| n.to_string())
            .collect()
    }

    fn session(app: &OpenCADStudio) -> &crate::ui::window::layer_walk::LayerWalkSession {
        app.tabs[app.active_tab]
            .layer_walk
            .as_ref()
            .expect("a walk is open")
    }

    fn rows(app: &OpenCADStudio) -> Vec<String> {
        app.tabs[app.active_tab]
            .layer_walk_rows
            .iter()
            .map(|r| r.name.clone())
            .collect()
    }

    // ── Open / walk / close ────────────────────────────────────────────────

    #[test]
    fn opening_selects_visible_layers_and_hides_nothing() {
        let mut app = three_layer_drawing();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        app.open_layer_walk();
        assert_eq!(session(&app).selected, vec!["0", "DOOR", "GHOST", "WALLS"]);
        assert!(hidden(&app, &["0", "WALLS", "DOOR", "GHOST"]).is_empty());
        // Everything visible: no override at all, not a set-the-size-of-everything.
        assert!(!app.tabs[app.active_tab]
            .scene
            .layer_walk_active_for_current_space());
    }

    #[test]
    fn walking_to_one_layer_hides_the_rest_without_touching_the_drawing() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let undo_before = app.tabs[i].history.undo_stack.len();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert_eq!(hidden(&app, &["DOOR", "GHOST"]), ["DOOR", "GHOST"]);
        assert!(app.tabs[i].scene.layer_walk_active_for_current_space());
        // The layer table is the proof it is a temporary view, not an edit.
        for name in ["WALLS", "DOOR", "GHOST"] {
            let l = app.tabs[i].scene.document.layers.get(name).unwrap();
            assert!(!l.flags.off && !l.flags.frozen, "{name} unchanged");
        }
        assert!(!app.tabs[i].dirty);
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before);
    }

    #[test]
    fn close_with_restore_on_exit_discards_and_restores_the_layer_table() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let undo_before = app.tabs[i].history.undo_stack.len();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(&mut app, LayerWalkMsg::Close);
        assert!(app.tabs[i].layer_walk.is_none());
        assert!(!app.tabs[i].scene.layer_walk_active());
        assert!(!app.tabs[i].dirty);
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before);
    }

    #[test]
    fn keep_result_commits_the_walk_as_exactly_one_undo_step() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let undo_before = app.tabs[i].history.undo_stack.len();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(&mut app, LayerWalkMsg::KeepResult);
        // The walk's visible set is written to the table: what it showed is on,
        // what it hid is off.
        assert!(
            !app.tabs[i]
                .scene
                .document
                .layers
                .get("WALLS")
                .unwrap()
                .flags
                .off
        );
        assert!(
            app.tabs[i]
                .scene
                .document
                .layers
                .get("DOOR")
                .unwrap()
                .flags
                .off
        );
        assert!(
            app.tabs[i]
                .scene
                .document
                .layers
                .get("GHOST")
                .unwrap()
                .flags
                .off
        );
        assert!(
            app.tabs[i]
                .scene
                .document
                .layers
                .get("0")
                .unwrap()
                .flags
                .off,
            "unpinned layer 0 is committed off with everything else the walk hid"
        );
        assert!(app.tabs[i].dirty);
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);
        // The override is gone: what is on screen is now the table's own doing.
        assert!(!app.tabs[i].scene.layer_walk_active());
        assert!(app.tabs[i].layer_walk.is_none());
    }

    #[test]
    fn closing_with_restore_off_commits_like_keep_result() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let undo_before = app.tabs[i].history.undo_stack.len();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(&mut app, LayerWalkMsg::ToggleRestoreOnExit(false));
        walk(&mut app, LayerWalkMsg::Close);
        assert!(
            app.tabs[i]
                .scene
                .document
                .layers
                .get("DOOR")
                .unwrap()
                .flags
                .off
        );
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);
    }

    #[test]
    fn an_unchanged_walk_commits_nothing() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let undo_before = app.tabs[i].history.undo_stack.len();
        app.open_layer_walk();
        walk(&mut app, LayerWalkMsg::ToggleRestoreOnExit(false));
        walk(&mut app, LayerWalkMsg::Close);
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before);
        assert!(!app.tabs[i].dirty);
    }

    // ── Every closing path cleans up ───────────────────────────────────────

    #[test]
    fn closing_the_dock_clears_the_override() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert!(app.tabs[app.active_tab].scene.layer_walk_active());
        let _ = app.dock_set_open(PanelId::Laywalk, false);
        assert!(!app.tabs[app.active_tab].scene.layer_walk_active());
        assert!(app.tabs[app.active_tab].layer_walk.is_none());
    }

    #[test]
    fn running_a_layer_command_ends_the_walk() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        let _ = app.run_command_line("LAYOFF");
        assert!(app.tabs[app.active_tab].layer_walk.is_none());
        assert!(!app.tabs[app.active_tab].scene.layer_walk_active());
    }

    #[test]
    fn erasing_ends_the_walk_so_the_hiding_never_eats_the_result() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        let _ = app.dispatch_command("ERASE");
        assert!(!app.tabs[app.active_tab].scene.layer_walk_active());
    }

    #[test]
    fn a_second_laywalk_closes_the_first() {
        let mut app = three_layer_drawing();
        let _ = app.run_command_line("LAYWALK");
        assert!(app.tabs[app.active_tab].layer_walk.is_some());
        let _ = app.run_command_line("LAYWALK");
        assert!(app.tabs[app.active_tab].layer_walk.is_none());
        assert!(!app.tabs[app.active_tab].scene.layer_walk_active());
    }

    #[test]
    fn quoted_transparent_commands_do_not_end_a_walk() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        let _ = app.run_command_line("'ZOOM");
        assert!(app.tabs[app.active_tab].layer_walk.is_some());
        assert!(app.tabs[app.active_tab]
            .scene
            .layer_walk_active_for_current_space());
    }

    #[test]
    fn loading_a_document_into_a_walking_tab_drops_the_previous_hide() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert!(app.tabs[i].scene.layer_walk_active());
        let other = codec::Document::new();
        app.tabs[i].scene.document = other;
        app.reset_layer_walk_for(i);
        assert!(!app.tabs[i].scene.layer_walk_active());
        assert!(app.tabs[i].layer_walk_rows.is_empty());
    }

    // ── Rows keep honest while the pallet is open ──────────────────────────

    #[test]
    fn churn_while_open_reaches_the_rows_and_prunes_the_selection() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(
            &mut app,
            LayerWalkMsg::ToggleHold {
                name: "DOOR".into(),
            },
        );
        // Another command renames one walked layer and deletes the other.
        app.tabs[i]
            .scene
            .document
            .layers
            .rename("WALLS", "WALL-NEW")
            .ok();
        let _ = app.tabs[i].scene.document.layers.remove("DOOR");
        app.refresh_layer_panel();
        assert_eq!(
            rows(&app),
            vec!["0".to_string(), "GHOST".to_string(), "WALL-NEW".to_string()]
        );
        // Selection is by name, so a rename drops the row that lost its name
        // and a deletion drops the held layer that no longer exists.
        assert_eq!(session(&app).selected, Vec::<String>::new());
        assert_eq!(session(&app).held, Vec::<String>::new());
        // Nothing is selected any more, so the walk hides every row it lost —
        // the current layer included.
        assert_eq!(
            hidden(&app, &["0", "GHOST", "WALL-NEW"]),
            ["0", "GHOST", "WALL-NEW"]
        );
    }

    #[test]
    fn layer_created_while_open_joins_the_list() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        add_layer(&mut app, i, "ROOF");
        app.refresh_layer_panel();
        assert!(rows(&app).contains(&"ROOF".to_string()));
        // Unselected, so the walk hides it: the list is the walk's world.
        assert_eq!(hidden(&app, &["ROOF"]), ["ROOF"]);
    }

    #[test]
    fn held_layers_stay_visible_and_everything_else_hides() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(
            &mut app,
            LayerWalkMsg::ToggleHold {
                name: "GHOST".into(),
            },
        );
        assert!(hidden(&app, &["DOOR"]).contains(&"DOOR".to_string()));
        assert!(hidden(&app, &["GHOST"]).is_empty());
        assert!(session(&app).held.contains(&"GHOST".to_string()));
        // Clear everything: only what is held is left on screen.
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::ClearAll),
        );
        assert_eq!(
            hidden(&app, &["0", "WALLS", "DOOR", "GHOST"]),
            ["0", "WALLS", "DOOR"]
        );
    }

    #[test]
    fn walking_off_the_current_layer_hides_it_like_any_other() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        // `WALLS` is current; walking to `DOOR` alone hides it.
        app.tabs[i].scene.document.header.current_layer_name = "WALLS".into();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "DOOR".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert_eq!(hidden(&app, &["WALLS"]), ["WALLS"]);
        assert!(hidden(&app, &["DOOR"]).is_empty());
    }

    #[test]
    fn keep_result_turns_the_current_layer_off() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.tabs[i].scene.document.header.current_layer_name = "WALLS".into();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "DOOR".into(),
                ctrl: false,
                shift: false,
            },
        );
        let undo_before = app.tabs[i].history.undo_stack.len();
        walk(&mut app, LayerWalkMsg::KeepResult);
        let doc = &app.tabs[i].scene.document;
        let off = |n: &str| doc.layers.get(n).map(|l| l.flags.off).unwrap_or(false);
        assert!(
            off("WALLS"),
            "the walked-off current layer is committed off"
        );
        assert!(off("0") && off("GHOST"));
        assert!(!off("DOOR"));
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);
    }

    #[test]
    fn the_zero_pin_is_the_only_layer_keep_result_refuses_to_turn_off() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "DOOR".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(&mut app, LayerWalkMsg::ToggleLayer0AlwaysOn(true));
        walk(&mut app, LayerWalkMsg::KeepResult);
        let doc = &app.tabs[i].scene.document;
        let off = |n: &str| doc.layers.get(n).map(|l| l.flags.off).unwrap_or(false);
        assert!(!off("0"), "the pin keeps layer 0 on through the commit");
        assert!(off("WALLS") && off("GHOST"));
    }

    #[test]
    fn layer0_always_on_pins_the_zero_layer() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        app.tabs[i].scene.document.header.current_layer_name = "WALLS".into();
        app.walk_rebuild();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "DOOR".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert_eq!(hidden(&app, &["0"]), ["0"]);
        assert_eq!(hidden(&app, &["WALLS"]), ["WALLS"]);
        walk(&mut app, LayerWalkMsg::ToggleLayer0AlwaysOn(true));
        assert!(hidden(&app, &["0"]).is_empty());
        assert_eq!(hidden(&app, &["WALLS"]), ["WALLS"]);
    }

    #[test]
    fn select_all_clear_all_and_invert_use_the_drawn_rows() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::SelectAll),
        );
        assert_eq!(session(&app).selected.len(), 4);
        walk(&mut app, LayerWalkMsg::MenuItem(LayerWalkMenuItem::Invert));
        assert!(session(&app).selected.is_empty());
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::SelectAll),
        );
        assert_eq!(session(&app).selected.len(), 4);
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::ClearAll),
        );
        assert!(session(&app).selected.is_empty());
    }

    #[test]
    fn filter_limits_what_select_all_reaches() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::ClearAll),
        );
        walk(&mut app, LayerWalkMsg::FilterEdit("D*".into()));
        walk(&mut app, LayerWalkMsg::FilterApply);
        walk(&mut app, LayerWalkMsg::FilterOnlyMatch(true));
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::SelectAll),
        );
        assert_eq!(session(&app).selected, vec!["DOOR"]);
        // Typing alone is inert until Enter.
        walk(&mut app, LayerWalkMsg::FilterEdit("G*".into()));
        assert_eq!(session(&app).pattern.as_deref(), Some("D*"));
    }

    #[test]
    fn keyboard_navigation_steps_through_the_list() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "0".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert!(app.layer_walk_owns_arrows());
        walk(&mut app, LayerWalkMsg::KeyNav { up: false });
        assert_eq!(session(&app).selected, vec!["DOOR"]);
        walk(&mut app, LayerWalkMsg::KeyNav { up: true });
        assert_eq!(session(&app).selected, vec!["0"]);
        // A click in the drawing gives the arrows back.
        app.layer_walk_lost_focus();
        assert!(!app.layer_walk_owns_arrows());
    }

    #[test]
    fn the_hover_lives_on_the_row_under_the_pointer() {
        let mut app = three_layer_drawing();
        app.open_layer_walk();
        let before = hidden(&app, &["0", "WALLS", "DOOR", "GHOST"]);
        walk(
            &mut app,
            LayerWalkMsg::RowHover {
                name: "WALLS".into(),
                on: true,
            },
        );
        assert_eq!(session(&app).hovered.as_deref(), Some("WALLS"));
        // View-only: the hover must not re-run the override.
        assert_eq!(
            hidden(&app, &["0", "WALLS", "DOOR", "GHOST"]),
            before,
            "a hover changes nothing on screen"
        );
        // Rows above the hovered one leave *after* the new row enters, so a
        // leave may only clear its own row.
        walk(
            &mut app,
            LayerWalkMsg::RowHover {
                name: "DOOR".into(),
                on: true,
            },
        );
        walk(
            &mut app,
            LayerWalkMsg::RowHover {
                name: "WALLS".into(),
                on: false,
            },
        );
        assert_eq!(session(&app).hovered.as_deref(), Some("DOOR"));
        walk(
            &mut app,
            LayerWalkMsg::RowHover {
                name: "DOOR".into(),
                on: false,
            },
        );
        assert_eq!(session(&app).hovered.as_deref(), None);
    }

    // ── Edge-case drawings ─────────────────────────────────────────────────

    #[test]
    fn an_empty_drawing_opens_a_one_row_walk_and_closes_cleanly() {
        let mut app = fresh_app();
        app.open_layer_walk();
        assert_eq!(rows(&app), vec!["0"]);
        assert!(session(&app).selected == vec!["0".to_string()]);
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "0".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert!(!app.tabs[app.active_tab].scene.layer_walk_active());
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::ZoomExtents),
        );
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::SelectObjectsOnLayer),
        );
        walk(&mut app, LayerWalkMsg::Close);
        assert!(app.tabs[app.active_tab].layer_walk.is_none());
    }

    #[test]
    fn a_single_layer_drawing_walks_and_restores() {
        let mut app = fresh_app();
        let i = app.active_tab;
        add_layer(&mut app, i, "ONLY");
        let mut line = Line::new();
        line.common.layer = "ONLY".to_string();
        let _ = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Line(line));
        // `ONLY` is current, so the walk can hide the one other layer it has.
        app.tabs[i].scene.document.header.current_layer_name = "ONLY".into();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "ONLY".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert_eq!(hidden(&app, &["0"]), ["0"]);
        walk(&mut app, LayerWalkMsg::Close);
        assert!(hidden(&app, &["0", "ONLY"]).is_empty());
    }

    #[test]
    fn thousands_of_layers_still_walk() {
        let mut app = fresh_app();
        let i = app.active_tab;
        for n in 0..2000 {
            add_layer(&mut app, i, &format!("LAYER{n:04}"));
        }
        app.open_layer_walk();
        assert_eq!(app.tabs[i].layer_walk_rows.len(), 2001);
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "LAYER1234".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert_eq!(hidden(&app, &["LAYER0001", "LAYER1234"]), ["LAYER0001"]);
        // The title counts every layer, and the override is one small set.
        assert_eq!(session(&app).selected, vec!["LAYER1234"]);
        walk(&mut app, LayerWalkMsg::Close);
        assert!(!app.tabs[i].scene.layer_walk_active());
    }

    #[test]
    fn xref_and_special_names_are_carried_by_the_walk() {
        let mut app = fresh_app();
        let i = app.active_tab;
        for name in ["PLAN|WALLS", "ROOF-A1", "TÉLÉ", "0"] {
            if name != "0" {
                add_layer(&mut app, i, name);
            }
        }
        let mut line = Line::new();
        line.common.layer = "PLAN|WALLS".to_string();
        let _ = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Line(line));
        app.open_layer_walk();
        assert!(rows(&app).contains(&"PLAN|WALLS".to_string()));
        // A block-reference name is never offered for purge, whatever its count.
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "PLAN|WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        assert!(app.tabs[i]
            .layer_walk_rows
            .iter()
            .all(|r| !r.purged || r.name != "PLAN|WALLS"));
        assert_eq!(hidden(&app, &["ROOF-A1", "TÉLÉ"]), ["ROOF-A1", "TÉLÉ"]);
    }

    // ── Pallet actions ─────────────────────────────────────────────────────

    #[test]
    fn select_objects_park_the_command_and_a_pick_replaces_the_selection() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(&mut app, LayerWalkMsg::SelectObjects);
        // The prompt is the app's standard object selection; it relaunches
        // LAYWALK_SELECT once the pick is in hand.
        assert_eq!(
            app.tabs[i].active_cmd.as_ref().map(|c| c.name()),
            Some("SELECT")
        );
        assert!(app.tabs[i].layer_walk_picking);
        // The pallet is hidden only while the prompt is live.
        assert!(!app.dock_panel_visible(PanelId::Laywalk));
        let handles: Vec<codec::Handle> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| e.common().layer == "DOOR")
            .map(|e| e.common().handle)
            .collect();
        app.tabs[i].scene.selected = handles.into_iter().collect();
        // `handle_relaunch` clears the live prompt before dispatching its name.
        app.tabs[i].active_cmd = None;
        let _ = app.run_command_line("LAYWALK_SELECT");
        assert_eq!(session(&app).selected, vec!["DOOR"]);
        assert!(!app.tabs[i].layer_walk_picking);
        assert!(app.dock_panel_visible(PanelId::Laywalk));
    }

    #[test]
    fn purge_removes_only_the_selected_unreferenced_layers() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let undo_before = app.tabs[i].history.undo_stack.len();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "GHOST".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(&mut app, LayerWalkMsg::Purge);
        assert!(session(&app).purge_confirm);
        walk(&mut app, LayerWalkMsg::PurgeConfirm(true));
        assert!(app.tabs[i].scene.document.layers.get("GHOST").is_none());
        assert!(app.tabs[i].scene.document.layers.get("WALLS").is_some());
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);
        // The list re-reads the table after the removal.
        assert!(!rows(&app).contains(&"GHOST".to_string()));
        assert!(app.tabs[i].layer_walk.is_some());
    }

    #[test]
    fn purge_is_a_no_op_when_the_selection_holds_referenced_layers() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let undo_before = app.tabs[i].history.undo_stack.len();
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(&mut app, LayerWalkMsg::Purge);
        assert!(!session(&app).purge_confirm);
        walk(&mut app, LayerWalkMsg::PurgeConfirm(true));
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before);
        assert!(app.tabs[i].scene.document.layers.get("WALLS").is_some());
    }

    #[test]
    fn saving_a_layer_state_while_walking_records_the_walk() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::SaveState),
        );
        assert!(matches!(
            app.active_modal,
            Some(crate::app::ModalKind::LayerStateEditor)
        ));
        let draft = app.layer_state_edit_draft.as_ref().expect("a draft");
        let off = |name: &str| {
            draft
                .layers
                .iter()
                .find(|l| l.layer_name == name)
                .map(|l| l.off)
                .unwrap_or_else(|| panic!("{name} missing"))
        };
        assert!(!off("WALLS"));
        assert!(off("DOOR") && off("GHOST"), "the walk's hide is the state");
        assert!(off("0"), "the walk hides the current layer like any other");
        // Nothing was written to the document by opening the editor.
        assert!(app.tabs[i].scene.document.layer_states().is_empty());
    }

    #[test]
    fn select_objects_on_layer_only_picks_the_walked_space() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::SelectObjectsOnLayer),
        );
        assert_eq!(app.tabs[i].scene.selected.len(), 2);
        let selected = app.tabs[i].scene.selected.clone();
        let all_on_walls = app.tabs[i]
            .scene
            .document
            .entities()
            .filter(|e| selected.contains(&e.common().handle))
            .all(|e| e.common().layer == "WALLS");
        assert!(all_on_walls);
    }

    #[test]
    fn filters_save_list_and_delete() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(&mut app, LayerWalkMsg::FilterEdit("WALL*".into()));
        walk(&mut app, LayerWalkMsg::FilterApply);
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::SaveFilter),
        );
        assert_eq!(app.tabs[i].layer_walk_filters, vec!["WALL*".to_string()]);
        assert_eq!(session(&app).saved_active.as_deref(), Some("WALL*"));
        walk(
            &mut app,
            LayerWalkMsg::MenuItem(LayerWalkMenuItem::DeleteFilter),
        );
        assert!(app.tabs[i].layer_walk_filters.is_empty());
        assert!(session(&app).pattern.is_none());
    }

    // ── Space identity ─────────────────────────────────────────────────────

    #[test]
    fn a_walk_only_ever_covers_one_space() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        let model = app.tabs[i].scene.interaction_space_key_pub();
        assert_eq!(session(&app).space, Some(model));
        // Walking into a layout re-targets the override and drops model's.
        let _ = app.update(Message::LayoutSwitch("Layout1".into()));
        assert_eq!(
            session(&app).space,
            Some(app.tabs[i].scene.interaction_space_key_pub())
        );
        assert_ne!(session(&app).space, Some(model));
        assert!(app.tabs[i].scene.space_layer_walk(model).is_none());
        assert!(app.tabs[i].scene.layer_walk_active_for_current_space());
        walk(&mut app, LayerWalkMsg::Close);
        let layout = app.tabs[i].scene.interaction_space_key_pub();
        assert!(app.tabs[i].scene.space_layer_walk(layout).is_none());
        assert!(app.tabs[i].scene.space_layer_walk(model).is_none());
        assert!(!app.tabs[i].scene.layer_walk_active());
    }

    #[test]
    fn entering_a_viewport_moves_the_walk_with_the_user() {
        let mut app = three_layer_drawing();
        let i = app.active_tab;
        let model = app.tabs[i].scene.interaction_space_key_pub();
        let _ = app.update(Message::LayoutSwitch("Layout1".into()));
        let mut vp = codec::entities::Viewport::default();
        vp.width = 200.;
        vp.height = 100.;
        vp.view_height = 50.;
        let handle = app.tabs[i]
            .scene
            .add_entity(EntityType::Viewport(Box::new(vp)));
        app.open_layer_walk();
        walk(
            &mut app,
            LayerWalkMsg::RowClick {
                name: "WALLS".into(),
                ctrl: false,
                shift: false,
            },
        );
        let paper = app.tabs[i].scene.interaction_space_key_pub();
        let _ = app.update(Message::EnterViewport(handle));
        assert_ne!(app.tabs[i].scene.interaction_space_key_pub(), paper);
        assert_eq!(
            session(&app).space,
            Some(app.tabs[i].scene.interaction_space_key_pub())
        );
        assert!(app.tabs[i].scene.layer_walk_active_for_current_space());
        let _ = app.update(Message::ExitViewport);
        assert_eq!(session(&app).space, Some(paper));
        assert!(app.tabs[i].scene.space_layer_walk(paper).is_some());
        assert!(app.tabs[i].scene.space_layer_walk(model).is_none());
    }
}
