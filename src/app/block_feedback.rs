use super::OpenCADStudio;
use crate::scene::block_actions::{self as actions, feedback};
use codec::{EntityType, Handle};
use glam::DVec3;
use iced::{Point, Rectangle};
use std::sync::Arc;
mod grip_tips;

pub(super) struct DependencyCache {
    key: (u64, u64, Handle),
    segments: Arc<Vec<[DVec3; 2]>>,
}

impl OpenCADStudio {
    fn dynamic_grip_summary(&self, handle: Handle, id: usize) -> Option<String> {
        if !self.block_editor_preferences.grip_tips {
            return None;
        }
        if id == super::visibility::VIS_GRIP_ID {
            let state = self.tabs[self.active_tab]
                .visibility_grip
                .as_ref()
                .filter(|grip| grip.insert_handle == handle)
                .and_then(|grip| {
                    grip.current
                        .and_then(|current| grip.state_names.get(current))
                });
            return Some(match state {
                Some(state) => crate::tr!("block-grip", "visibility-value", state = state.clone()),
                None => crate::tr!("block-grip", "visibility-label"),
            });
        }
        if id == super::visibility::TABLE_GRIP_ID {
            let row = self.tabs[self.active_tab]
                .table_grip
                .as_ref()
                .filter(|grip| grip.insert_handle == handle)
                .and_then(|grip| grip.current);
            return Some(match row {
                Some(row) => crate::tr!("block-grip", "table-row", row = (row + 1).to_string()),
                None => crate::tr!("block-grip", "table-label"),
            });
        }
        if let Some(alignment) = self.tabs[self.active_tab]
            .scene
            .alignment_grips(handle)
            .into_iter()
            .find(|grip| grip.id == id)
        {
            let mode = crate::t!(if alignment.perpendicular {
                "Perpendicular"
            } else {
                "Tangent"
            })
            .into_owned();
            return Some(crate::tr!("block-grip", "alignment", mode = mode));
        }
        let (parameter, value) = self.dynamic_grip_value(handle, id)?;
        let base = match value {
            actions::Value::Frame { base, .. } => base,
            _ => parameter.base,
        };
        let value = match value {
            actions::Value::Flip(state) => {
                let source = self.tabs[self.active_tab]
                    .block_test
                    .as_ref()
                    .filter(|test| test.insert == handle)
                    .map_or(&self.tabs[self.active_tab].scene.document, |test| {
                        test.baseline.as_ref()
                    });
                crate::scene::block_parameter_properties::flip_state_label(
                    source,
                    parameter.handle,
                    state,
                )?
            }
            actions::Value::Point(at) | actions::Value::Frame { end: at, .. }
                if parameter.point || parameter.xy =>
            {
                format!("({:.4}, {:.4})", at.x - base.x, at.y - base.y)
            }
            actions::Value::Point(at) | actions::Value::Frame { end: at, .. }
                if parameter.rotation =>
            {
                format!("{:.4}°", (at.y - base.y).atan2(at.x - base.x).to_degrees())
            }
            actions::Value::Point(at) | actions::Value::Frame { end: at, .. } => {
                format!("{:.4}", at.distance(base))
            }
        };
        Some(format!("{}: {value}", parameter.name))
    }

    fn dynamic_grip_value(
        &self,
        handle: Handle,
        id: usize,
    ) -> Option<(actions::Parameter, actions::Value)> {
        let (node, _) = crate::scene::block_grips::runtime::decode(id)?;
        let tab = &self.tabs[self.active_tab];
        if let Some(test) = tab.block_test.as_ref().filter(|test| test.insert == handle) {
            let p = actions::all_parameters(&test.baseline, test.definition)
                .into_iter()
                .find(|p| p.id == node)?;
            let value = actions::instance::test_value(&tab.scene.document, handle, &p).ok()?;
            if !crate::scene::block_grips::runtime::action_grips(&test.baseline, test.definition, &p, value).iter().any(|grip| grip.id == id) { return None; }
            Some((p, value))
        } else {
            let definition = actions::instance::definition(&tab.scene.document, handle)?;
            let p = actions::all_parameters(&tab.scene.document, definition)
                .into_iter()
                .find(|p| p.id == node)?;
            let value = actions::instance::value(&tab.scene.document, handle, &p).ok()?;
            if !crate::scene::block_grips::runtime::action_grips(&tab.scene.document, definition, &p, value).iter().any(|grip| grip.id == id) { return None; }
            Some((p, value))
        }
    }

    pub(super) fn block_feedback(
        &self,
        clip: Rectangle,
    ) -> crate::ui::overlay::authoring::AuthoringFeedback {
        let tab = &self.tabs[self.active_tab];
        let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
        let edit_frame = tab.scene.viewport_edit_frame((vw, vh));
        let camera = edit_frame
            .as_ref()
            .map(|(camera, _)| camera.clone())
            .unwrap_or_else(|| tab.scene.camera.borrow().clone());
        let paper = tab.scene.current_layout != "Model" && edit_frame.is_none();
        let clip = edit_frame
            .as_ref()
            .map(|(_, rect)| *rect)
            .unwrap_or(if paper {
                Rectangle {
                    x: 0.0,
                    y: 0.0,
                    width: vw,
                    height: vh,
                }
            } else {
                clip
            });
        let mut result = crate::ui::overlay::authoring::AuthoringFeedback {
            clip,
            ..Default::default()
        };
        let project = |point: DVec3| {
            let p = if paper {
                let half_h = camera.ortho_size();
                let half_w = half_h * clip.width / clip.height.max(1.0);
                Point::new(
                    (point.x as f32 - camera.target.x as f32 + half_w) / (2.0 * half_w)
                        * clip.width,
                    (camera.target.y as f32 + half_h - point.y as f32) / (2.0 * half_h)
                        * clip.height,
                )
            } else {
                let p = camera.project(point, clip)?;
                Point::new(p.x, p.y)
            };
            (p.x.is_finite() && p.y.is_finite()).then_some(Point::new(clip.x + p.x, clip.y + p.y))
        };
        if self.block_editor_preferences.dependency_highlight {
            if let Some((session, selected)) =
                tab.active_block_edit_session().zip(tab.selected_authoring)
            {
                let key = (tab.id, tab.scene.geometry_epoch, selected);
                let mut cache = self.block_dependency_cache.borrow_mut();
                if cache.as_ref().is_none_or(|entry| entry.key != key) {
                    let handles =
                        feedback::dependencies(&tab.scene.document, session.br_handle, selected);
                    let mut segments = vec![];
                    for wire in tab.scene.wire_models_for(&handles) {
                        let mut previous = None;
                        for (index, high) in wire.points.iter().enumerate() {
                            let low = wire.points_low.get(index).copied().unwrap_or([0.0; 3]);
                            let point = DVec3::new(
                                high[0] as f64 + low[0] as f64,
                                high[1] as f64 + low[1] as f64,
                                high[2] as f64 + low[2] as f64,
                            );
                            if !point.is_finite() {
                                previous = None;
                                continue;
                            }
                            if let Some(previous) = previous {
                                segments.push([previous, point]);
                            }
                            previous = Some(point);
                        }
                    }
                    *cache = Some(DependencyCache {
                        key,
                        segments: Arc::new(segments),
                    });
                }
                if let Some(cache) = cache.as_ref() {
                    result.dependencies = cache
                        .segments
                        .iter()
                        .filter_map(|[a, b]| Some([project(*a)?, project(*b)?]))
                        .collect();
                }
            }
        }
        if self.block_editor_preferences.value_markers {
            // Markers describe the hovered/active parameter; they never enter
            // the grip collection or the drawing's snapping candidates.
            let hovered = self
                .grip_hover
                .as_ref()
                .map(|hover| (hover.handle, hover.grip_id));
            let hot = tab
                .hot_grips
                .iter()
                .copied()
                .find(|(_, id)| super::block_actions::is_test_grip(*id));
            let active = tab
                .active_cmd
                .as_ref()
                .and_then(|command| command.block_parameter_target())
                .and_then(|(insert, parameter)| {
                    let handle =
                        insert.or_else(|| tab.block_test.as_ref().map(|test| test.insert))?;
                    let (source, definition): (&codec::Document, _) =
                        if let Some(test) = tab.block_test.as_ref() {
                            (&test.baseline, test.definition)
                        } else {
                            (
                                &tab.scene.document,
                                actions::instance::definition(&tab.scene.document, handle)?,
                            )
                        };
                    let p = actions::all_parameters(source, definition).into_iter().find(|p| p.handle == parameter)?;
                    let value = if tab.block_test.is_some() { actions::instance::test_value(&tab.scene.document, handle, &p).ok()? }
                        else { actions::instance::value(&tab.scene.document, handle, &p).ok()? };
                    let active_id = tab.active_cmd.as_ref().and_then(|command| command.block_parameter_grip_id());
                    let grip = crate::scene::block_grips::runtime::action_grips(source, definition, &p, value).into_iter().find(|grip| active_id.is_none_or(|id| grip.id == id))?;
                    Some((handle, grip.id))
                });
            if let Some((handle, id)) = active.or(hot).or(hovered) {
                if let Some((parameter, value)) = self.dynamic_grip_value(handle, id) {
                    if let Some(EntityType::Insert(reference)) =
                        tab.scene.document.get_entity(handle)
                    {
                        let source = tab
                            .block_test
                            .as_ref()
                            .map_or(&tab.scene.document, |test| &test.baseline);
                        let transform = crate::scene::render_graph::insert_transform(
                            &tab.scene.document,
                            reference,
                        );
                        let definition = tab.block_test.as_ref().map(|test| test.definition)
                            .or_else(|| actions::instance::definition(&tab.scene.document, handle));
                        result.markers = definition.map(|definition| crate::scene::block_grips::runtime::markers(source, definition, &parameter, value, id)).unwrap_or_default()
                            .into_iter()
                            .filter_map(|point| {
                                project(actions::v(transform.apply(actions::native(point))))
                            })
                            .collect();
                    }
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::{
        entities::{Insert, Line},
        objects::{BlockParameterValueSet, DynamicBlockData, ObjectType},
        types::Vector3,
    };

    #[test]
    fn block_feedback_uses_dependencies_and_transformed_value_markers_without_document_edits() {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.define_block_raw(
            "FEEDBACK",
            Vector3::ZERO,
            vec![EntityType::Line(Line::from_points(
                Vector3::ZERO,
                Vector3::new(10.0, 0.0, 0.0),
            ))],
        );
        let insert = app.tabs[i]
            .scene
            .document
            .add_entity(EntityType::Insert(Insert::new("FEEDBACK", Vector3::ZERO)))
            .unwrap();
        let _ = app.dispatch_command(&format!("BEDIT_BEGIN:{}", insert.value()));
        let _ = app.run_command_line("BPARAMETER LINEAR 0,0 10,0 5,2");
        let definition = app.tabs[i].active_block_edit_session().unwrap().br_handle;
        let target = app.tabs[i]
            .scene
            .document
            .block_records
            .get("FEEDBACK")
            .unwrap()
            .entity_handles[0];
        let parameter = actions::all_parameters(&app.tabs[i].scene.document, definition)[0].handle;
        let action = actions::create(
            &mut app.tabs[i].scene.document,
            definition,
            actions::Kind::Move,
            parameter,
            &[target],
            DVec3::ZERO,
        )
        .unwrap();
        let Some(ObjectType::DynamicBlock(object)) =
            app.tabs[i].scene.document.objects.get_mut(&parameter)
        else {
            panic!()
        };
        let DynamicBlockData::LinearParameter(native) = &mut object.data else {
            panic!()
        };
        native.value_set = BlockParameterValueSet {
            flags: 8,
            values: vec![5.0, 10.0, 15.0],
            ..Default::default()
        };
        app.tabs[i].scene.bump_geometry();
        app.tabs[i].selected_authoring = Some(parameter);
        let clip = Rectangle {
            x: 40.0,
            y: 30.0,
            width: 800.0,
            height: 600.0,
        };
        app.tabs[i].scene.selection.borrow_mut().view.vp_size = (800.0, 600.0);
        let before = app.tabs[i].scene.document.clone();
        let depth = app.tabs[i].history.undo_stack.len();
        assert!(!app.block_feedback(clip).dependencies.is_empty());
        let cached = app
            .block_dependency_cache
            .borrow()
            .as_ref()
            .unwrap()
            .segments
            .clone();
        assert!(!app.block_feedback(clip).dependencies.is_empty());
        assert!(Arc::ptr_eq(
            &cached,
            &app.block_dependency_cache
                .borrow()
                .as_ref()
                .unwrap()
                .segments
        ));
        app.tabs[i].selected_authoring = Some(action);
        assert!(!app.block_feedback(clip).dependencies.is_empty());
        let _ = app.dispatch_command("BDEPENDENCYHIGHLIGHT 0");
        assert!(app.block_feedback(clip).dependencies.is_empty());
        assert_eq!(app.tabs[i].scene.document, before);
        assert_eq!(app.tabs[i].history.undo_stack.len(), depth);
        let _ = app.dispatch_command("BSAVE");
        let _ = app.dispatch_command("BCLOSE");
        let Some(EntityType::Insert(reference)) = app.tabs[i].scene.document.get_entity_mut(insert)
        else {
            panic!()
        };
        reference.insert_point = Vector3::new(20.0, 30.0, 0.0);
        reference.rotation = std::f64::consts::FRAC_PI_2;
        reference.set_x_scale(2.0);
        reference.set_y_scale(2.0);
        reference.set_z_scale(2.0);
        app.tabs[i].scene.select_entity(insert, true);
        app.tabs[i].selected_handle = Some(insert);
        let grip_id = crate::scene::block_grips::runtime::id(actions::all_parameters(&app.tabs[i].scene.document, definition).into_iter().find(|p| p.handle == parameter).unwrap().id, 1).unwrap();
        app.grip_hover = Some(super::super::GripHover {
            document: app.tabs[i].id,
            handle: insert,
            grip_id: grip_id,
            screen: Point::ORIGIN,
            started: iced::time::Instant::now(),
        });
        let before = app.tabs[i].scene.document.clone();
        let markers = app.block_feedback(clip).markers;
        assert_eq!(markers.len(), 3);
        let camera = app.tabs[i].scene.camera.borrow();
        for (marker, distance) in markers.iter().zip([5.0, 10.0, 15.0]) {
            let expected = camera
                .project(DVec3::new(20.0, 30.0 + 2.0 * distance, 0.0), clip)
                .unwrap();
            assert!((marker.x - (clip.x + expected.x)).abs() < 0.001);
            assert!((marker.y - (clip.y + expected.y)).abs() < 0.001);
        }
        drop(camera);
        assert!(app
            .dynamic_grip_tip(insert, grip_id)
            .unwrap()
            .contains("10.0000"));
        let _ = app.dispatch_command("GRIPTIPS 0");
        assert!(app.dynamic_grip_tip(insert, grip_id).is_none());
        let _ = app.dispatch_command("BTMARKDISPLAY 0");
        assert!(app.block_feedback(clip).markers.is_empty());
        assert_eq!(app.tabs[i].scene.document, before);
        let _ = app.dispatch_command("BTMARKDISPLAY 1");
        app.grip_hover = None;
        app.tabs[i].active_cmd = Some(Box::new(
            crate::modules::block_editor::action::TestParameterCommand {
                grip_input: None,
                display_offset: DVec3::ZERO,
                insert: Some(insert),
                document: app.tabs[i].id,
                definition,
                parameter,
            },
        ));
        assert_eq!(app.block_feedback(clip).markers.len(), 3);
    }
}
