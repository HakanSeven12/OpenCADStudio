#[cfg(test)]
mod block_base_point_tests {
    use super::super::*;
    use crate::command::{CadCommand, CmdResult};
    use crate::modules::insert::insert_block::InsertBlockCommand;
    use document::entities::AttributeDefinition;
    use document::types::Vector3;
    use document::EntityType;

    /// base-point fix (A), host side: `AttreqNeeded` is where the host resolves the
    /// block's ATTDEFs, so it also injects the definition's base point. Without
    /// that the committed attribute lands on the codec transform's position.
    #[test]
    fn locked_attreq_injects_the_block_base_point_for_attribute_placement() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        // These tests follow the prompt chain, so the insertion form that
        // answers it in one go has to stay out of the way.
        app.tabs[i].scene.document.header.attribute_dialog = false;
        // Block "ATTREQ_BP": base (10,5,0); LINE (10,5)-(30,5); ATTDEF TAG1 at
        // (12,8). Rendering convention (see scene::block_base_point_tests): the
        // ATTDEF lands at (100,100) + Rot(30)*Scale(2)*((12,8) - (10,5))
        //   = (100,100) + 2*(1.732051 - 1.5, 1 + 2.598076) = (100.464102, 107.196152).
        app.tabs[i].scene.define_block_raw(
            "ATTREQ_BP",
            Vector3::new(10.0, 5.0, 0.0),
            vec![
                EntityType::Line(document::entities::Line::from_points(
                    Vector3::new(10.0, 5.0, 0.0),
                    Vector3::new(30.0, 5.0, 0.0),
                )),
                EntityType::AttributeDefinition(AttributeDefinition {
                    insertion_point: Vector3::new(12.0, 8.0, 0.0),
                    height: 1.0,
                    ..AttributeDefinition::new(
                        "TAG1".to_string(),
                        "Tag one".to_string(),
                        String::new(),
                    )
                }),
            ],
        );

        let mut cmd = InsertBlockCommand::classic_for_block(
            "ATTREQ_BP".to_string(),
            Vec::new(),
            String::new(),
        );
        let _ = cmd.on_point(glam::DVec3::new(100.0, 100.0, 0.0));
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
        assert!(matches!(cmd.on_text_input("2"), Some(CmdResult::NeedPoint)));
        assert!(matches!(
            cmd.on_text_input("30"),
            Some(CmdResult::AttreqNeeded { .. })
        ));
        app.tabs[i].active_cmd = Some(Box::new(cmd));

        let _ = app.apply_cmd_result(CmdResult::AttreqNeeded {
            block_name: "ATTREQ_BP".to_string(),
        });
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("the INSERT command waits for the attribute value")
            .on_text_input("VALUE1")
            .expect("the value finishes the INSERT");
        let _ = app.apply_cmd_result(result);

        let attribute = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(ins) if ins.block_name == "ATTREQ_BP" => {
                    ins.attributes.first().cloned()
                }
                _ => None,
            })
            .expect("the committed INSERT's attribute");
        assert_eq!(attribute.tag, "TAG1");
        let point = attribute.insertion_point;
        assert!(
            (point.x - 100.464102).abs() < 1e-6
                && (point.y - 107.196152).abs() < 1e-6
                && point.z.abs() < 1e-9,
            "attribute at ({:.6},{:.6},{:.6}), expected (100.464102,107.196152,0)",
            point.x,
            point.y,
            point.z
        );
    }

    /// An attribute placed through the command line is an object in its own
    /// right: a handle of its own, the reference as its owner and a terminator,
    /// so that the saved file carries it and the reference stays attributed.
    #[test]
    fn locked_attreq_attributes_are_written_as_objects_of_their_own() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.header.attribute_dialog = false;
        app.tabs[i].scene.define_block_raw(
            "ATTREQ_ID",
            Vector3::ZERO,
            vec![
                EntityType::Line(document::entities::Line::from_points(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(20.0, 0.0, 0.0),
                )),
                EntityType::AttributeDefinition(AttributeDefinition {
                    insertion_point: Vector3::new(1.0, 2.0, 0.0),
                    height: 1.0,
                    ..AttributeDefinition::new(
                        "TAG1".to_string(),
                        "Tag one".to_string(),
                        "100".to_string(),
                    )
                }),
            ],
        );

        let mut cmd = InsertBlockCommand::classic_for_block(
            "ATTREQ_ID".to_string(),
            Vec::new(),
            String::new(),
        );
        let _ = cmd.on_point(glam::DVec3::new(0.0, 0.0, 0.0));
        let _ = cmd.on_text_input("2");
        let _ = cmd.on_text_input("2");
        assert!(matches!(
            cmd.on_text_input("0"),
            Some(CmdResult::AttreqNeeded { .. })
        ));
        app.tabs[i].active_cmd = Some(Box::new(cmd));
        let _ = app.apply_cmd_result(CmdResult::AttreqNeeded {
            block_name: "ATTREQ_ID".to_string(),
        });
        let result = app.tabs[i]
            .active_cmd
            .as_mut()
            .expect("the INSERT command waits for the attribute value")
            .on_text_input("VALUE1")
            .expect("the value finishes the INSERT");
        let _ = app.apply_cmd_result(result);

        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(candidate) if candidate.block_name == "ATTREQ_ID" => {
                    Some(candidate.clone())
                }
                _ => None,
            })
            .expect("the placed reference");
        assert_eq!(insert.attributes.len(), 1);
        let attribute = &insert.attributes[0];
        assert!(
            !attribute.common.handle.is_null(),
            "the attribute is an object of its own, so it needs a handle"
        );
        assert_eq!(
            attribute.common.owner_handle, insert.common.handle,
            "the attribute belongs to the reference it was placed on"
        );
        assert!(
            insert.seqend_handle.is_some(),
            "the attributes need a terminator"
        );

        for ext in ["dxf", "dwg"] {
            let bytes = crate::io::save_to_bytes(
                &app.tabs[i].scene.document,
                ext,
                app.tabs[i].scene.document.version,
            )
            .unwrap_or_else(|error| panic!("save {ext}: {error}"));
            let read = crate::io::load_bytes(&format!("attreq.{ext}"), bytes)
                .unwrap_or_else(|error| panic!("load {ext}: {error}"));
            let reloaded = read
                .entities()
                .find_map(|entity| match entity {
                    EntityType::Insert(candidate) if candidate.block_name == "ATTREQ_ID" => {
                        Some(candidate.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{ext}: the reference was not written"));
            assert_eq!(
                reloaded.attributes.len(),
                1,
                "{ext}: the reference was written without its attribute"
            );
            assert_eq!(reloaded.attributes[0].tag, "TAG1", "{ext}: the tag");
            assert_eq!(
                reloaded.attributes[0].value, "VALUE1",
                "{ext}: the value entered at placement"
            );
        }
    }

    /// A constant attribute is never asked for, but it still belongs to the
    /// reference: the value the definition carries is the value the reference
    /// shows.
    #[test]
    fn locked_attreq_places_a_constant_attribute_without_asking() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        let mut attdef =
            AttributeDefinition::new("TAG1".to_string(), "Tag one".to_string(), "42".to_string());
        attdef.flags.constant = true;
        app.tabs[i].scene.define_block_raw(
            "ATTREQ_CONST",
            Vector3::ZERO,
            vec![
                EntityType::Line(document::entities::Line::from_points(
                    Vector3::new(0.0, 0.0, 0.0),
                    Vector3::new(20.0, 0.0, 0.0),
                )),
                EntityType::AttributeDefinition(attdef),
            ],
        );

        let mut cmd = InsertBlockCommand::classic_for_block(
            "ATTREQ_CONST".to_string(),
            Vec::new(),
            String::new(),
        );
        let _ = cmd.on_point(glam::DVec3::new(0.0, 0.0, 0.0));
        let _ = cmd.on_text_input("2");
        let _ = cmd.on_text_input("2");
        assert!(matches!(
            cmd.on_text_input("0"),
            Some(CmdResult::AttreqNeeded { .. })
        ));
        app.tabs[i].active_cmd = Some(Box::new(cmd));
        let _ = app.apply_cmd_result(CmdResult::AttreqNeeded {
            block_name: "ATTREQ_CONST".to_string(),
        });
        assert!(
            app.tabs[i].active_cmd.is_none(),
            "a constant attribute asks nothing, so the placement is finished"
        );

        let insert = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| match entity {
                EntityType::Insert(candidate) if candidate.block_name == "ATTREQ_CONST" => {
                    Some(candidate.clone())
                }
                _ => None,
            })
            .expect("the placed reference");
        assert_eq!(
            insert.attributes.len(),
            1,
            "the constant attribute belongs to the reference even though nothing was asked"
        );
        assert_eq!(insert.attributes[0].value, "42");
    }

    /// base-point fix (D), host side: picking an existing block in the Block
    /// Definition dialog prefills the base point from that definition.
    #[test]
    fn locked_block_def_name_select_prefills_the_definition_base_point() {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "DEF_BASE",
            Vector3::new(10.0, 5.0, 0.0),
            vec![EntityType::Line(document::entities::Line::from_points(
                Vector3::new(10.0, 5.0, 0.0),
                Vector3::new(30.0, 5.0, 0.0),
            ))],
        );
        app.block_definition = Some(
            crate::ui::window::block_definition::BlockDefinitionState::new(
                vec!["DEF_BASE".to_string()],
                Vec::new(),
                0,
                4,
            ),
        );
        assert_eq!(
            app.block_definition
                .as_ref()
                .map(|s| s.base_point_y.as_str()),
            Some("0.0000"),
            "a new block starts at 0"
        );

        let _ = app.update(Message::BlockDefNameSelect("DEF_BASE".to_string()));

        let state = app.block_definition.as_ref().expect("the dialog is open");
        assert_eq!(state.name, "DEF_BASE");
        assert_eq!(state.base_point_x, "10.0000");
        assert_eq!(state.base_point_y, "5.0000");
        assert_eq!(state.base_point_z, "0.0000");
        assert_eq!(state.parse_base_point(), glam::DVec3::new(10.0, 5.0, 0.0));
    }

    /// A block with base (10,5,0) and LINE (10,5)-(30,5), referenced by an
    /// INSERT at (100,100) with 30° rotation and uniform scale 2. Under the
    /// convention the LINE renders at (100,100)-(134.641016,120).
    fn block_and_instance_with_base(app: &mut OpenCADStudio, name: &str) -> document::Handle {
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            name,
            Vector3::new(10.0, 5.0, 0.0),
            vec![EntityType::Line(document::entities::Line::from_points(
                Vector3::new(10.0, 5.0, 0.0),
                Vector3::new(30.0, 5.0, 0.0),
            ))],
        );
        let mut insert =
            document::entities::Insert::new(name.to_string(), Vector3::new(100.0, 100.0, 0.0));
        insert.set_x_scale(2.0);
        insert.set_y_scale(2.0);
        insert.set_z_scale(2.0);
        insert.rotation = std::f64::consts::PI / 6.0;
        app.tabs[i].scene.add_entity(EntityType::Insert(insert))
    }

    /// base-point follow-up fix: the REFEDIT working copy is placed through the
    /// base-point-aware INSERT transform, so it sits exactly on the rendered
    /// reference, and saving folds it back to the same block-local geometry.
    #[test]
    fn locked_refedit_working_copy_sits_on_the_rendered_reference() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "REFEDIT_BP");
        let i = app.active_tab;

        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", handle.value()));
        assert!(
            app.tabs[i].refedit_session.is_some(),
            "setup: REFEDIT_BEGIN opens a session"
        );

        // The working copy is the LINE the session moved into model space.
        let temp: Vec<document::Handle> = app.tabs[i]
            .refedit_session
            .as_ref()
            .expect("the session")
            .temp_handles
            .clone();
        let line = temp
            .iter()
            .find_map(|h| match app.tabs[i].scene.document.get_entity(*h) {
                Some(EntityType::Line(line)) => Some(line.clone()),
                _ => None,
            })
            .expect("the working-copy LINE");
        // (100,100) + Rot(30)*2*((10,5)-(10,5)) and ...*((30,5)-(10,5)).
        assert!(
            (line.start.x - 100.0).abs() < 1e-6
                && (line.start.y - 100.0).abs() < 1e-9
                && (line.end.x - 134.641016).abs() < 1e-6
                && (line.end.y - 120.0).abs() < 1e-6,
            "working copy at ({:.6},{:.6})-({:.6},{:.6}), expected (100,100)-(134.641016,120)",
            line.start.x,
            line.start.y,
            line.end.x,
            line.end.y
        );

        // Saving without editing restores the definition verbatim: the
        // inverse is the exact inverse of the forward transform.
        let _ = app.dispatch_command("REFCLOSE_SAVE");
        assert!(app.tabs[i].refedit_session.is_none());
        let stored = app.tabs[i]
            .scene
            .document
            .block_records
            .get("REFEDIT_BP")
            .expect("the block record")
            .entity_handles
            .iter()
            .find_map(|h| match app.tabs[i].scene.document.get_entity(*h) {
                Some(EntityType::Line(line)) => Some(line.clone()),
                _ => None,
            })
            .expect("the stored LINE");
        assert!(
            (stored.start.x - 10.0).abs() < 1e-9
                && (stored.start.y - 5.0).abs() < 1e-9
                && (stored.end.x - 30.0).abs() < 1e-9
                && (stored.end.y - 5.0).abs() < 1e-9,
            "stored LINE at ({:.9},{:.9})-({:.9},{:.9}), expected (10,5)-(30,5)",
            stored.start.x,
            stored.start.y,
            stored.end.x,
            stored.end.y
        );
        assert_eq!(
            crate::scene::render_graph::block_base_point(&app.tabs[i].scene.document, "REFEDIT_BP"),
            Vector3::new(10.0, 5.0, 0.0),
            "the base point survives the round trip"
        );
    }

    /// The definition body is replaced on save, so the terminator has to move
    /// after the new body.
    #[test]
    fn a_refedit_save_ends_the_definition_after_its_body() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "REFEDIT_ORDER");
        let i = app.active_tab;
        let _ = app.dispatch_command(&format!("REFEDIT_BEGIN:{}", handle.value()));
        assert!(
            app.tabs[i].refedit_session.is_some(),
            "setup: REFEDIT_BEGIN opens a session"
        );
        let _ = app.dispatch_command("REFCLOSE_SAVE");
        assert!(app.tabs[i].refedit_session.is_none());
        let record = app.tabs[i]
            .scene
            .document
            .block_records
            .get("REFEDIT_ORDER")
            .expect("the block record");
        let marker = record.block_entity_handle.value();
        let end = record.block_end_handle.value();
        let body: Vec<_> = record.entity_handles.iter().map(|h| h.value()).collect();
        assert!(
            body.iter().all(|h| *h > marker),
            "body handles follow the BLOCK marker: {body:?}"
        );
        assert!(
            end > *body.iter().max().expect("the body is not empty"),
            "the ENDBLK handle follows the body: marker {marker}, body {body:?}, end {end}"
        );
        assert!(
            matches!(
                app.tabs[i]
                    .scene
                    .document
                    .get_entity(document::Handle::new(end)),
                Some(EntityType::BlockEnd(_))
            ),
            "the record's end handle names an ENDBLK entity"
        );
    }

    /// base-point follow-up fix: the dynamic-block visibility grip is a
    /// block-definition coordinate, so it follows the base-point convention
    /// too: (100,100) + Rot(30)*2*((12,3) - (10,5)) = (105.464102, 98.535898).
    #[test]
    fn locked_visibility_grip_follows_the_base_point_convention() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "VIS_BP");
        let i = app.active_tab;

        let def_block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("VIS_BP")
            .expect("the block record")
            .handle;
        let mut param = document::objects::BlockVisibilityParameter::default();
        param.owner = def_block;
        param.def_point = Vector3::new(12.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .document
            .block_visibility_params
            .insert(document::Handle::new(0x70), param);
        app.tabs[i].selected_handle = Some(handle);

        app.refresh_visibility_grip([0.0, 0.0, 0.0]);

        let grip = app.tabs[i]
            .selected_grips
            .last()
            .expect("the visibility lookup grip");
        assert!(
            (grip.world.x - 105.464102).abs() < 1e-6 && (grip.world.y - 98.535898).abs() < 1e-6,
            "grip at ({:.6},{:.6}), expected (105.464102,98.535898)",
            grip.world.x,
            grip.world.y
        );
        assert!(
            app.tabs[i].visibility_grip.is_some(),
            "the selection is a dynamic-block reference"
        );
    }

    /// The visibility grip opens a list, so its triangle points down like a
    /// list arrow, even on a rotated reference (the fixture turns it 30 deg).
    #[test]
    fn visibility_grip_points_down() {
        let mut app = OpenCADStudio::new_for_test();
        let handle = block_and_instance_with_base(&mut app, "VIS_DOWN");
        let i = app.active_tab;
        let def_block = app.tabs[i]
            .scene
            .document
            .block_records
            .get("VIS_DOWN")
            .expect("the block record")
            .handle;
        let mut param = document::objects::BlockVisibilityParameter::default();
        param.owner = def_block;
        param.def_point = Vector3::new(12.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .document
            .block_visibility_params
            .insert(document::Handle::new(0x70), param);
        app.tabs[i].selected_handle = Some(handle);

        app.refresh_visibility_grip([0.0, 0.0, 0.0]);

        let grip = app.tabs[i]
            .selected_grips
            .last()
            .expect("the visibility lookup grip");
        assert_eq!(grip.dir, Some(glam::DVec3::NEG_Y));
    }
}
