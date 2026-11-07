use super::*;
use crate::scene::block_actions as actions;
use crate::scene::block_parameter_properties as properties;
use crate::scene::model::object::PropValue;
use document::objects::ObjectType;
use glam::DVec3;

#[test]
#[ignore = "requires OCS_DYNAMIC_BLOCK_DRAWING"]
fn audit_imported_dynamic_references() {
    let path = std::env::var("OCS_DYNAMIC_BLOCK_DRAWING").unwrap();
    let doc = crate::io::load_bytes(&path, std::fs::read(&path).unwrap()).unwrap();
    let mut app = OpenCADStudio::new_for_test();
    let i = app.active_tab;
    let mut failures = Vec::new();
    let mut total = 0;
    let mut static_references = 0;
    let mut controls = 0;
    for entity in doc.entities() {
        let EntityType::Insert(reference) = entity else {
            continue;
        };
        let handle = reference.common.handle;
        assert!(
            doc.block_records.get(&reference.block_name).is_some(),
            "reference {handle:?} has no block record"
        );
        let Some(definition) = doc.dynamic_definition_for_insert(handle) else {
            static_references += 1;
            continue;
        };
        let Some(ObjectType::Dictionary(dictionary)) = doc
            .extension_dictionary_handle(definition)
            .and_then(|handle| doc.objects.get(&handle))
        else {
            static_references += 1;
            continue;
        };
        if dictionary.get("ACAD_ENHANCEDBLOCK").is_none() {
            static_references += 1;
            continue;
        }
        total += 1;
        let previous_controls = controls;
        let block = doc
            .block_records
            .iter()
            .find(|b| b.handle == definition)
            .map(|b| b.name.as_str())
            .unwrap_or("?");
        let context = format!(
            "{block} {handle:?} ({:?}, scale {:?})",
            reference.block_name,
            reference.x_scale()
        );
        let mut fail = |control: &str, error: &str| {
            failures.push(format!("{context} {control}: {error}"));
        };
        if actions::instance::definition(&doc, handle).is_none() {
            fail("definition", "reference is unavailable");
            continue;
        }
        app.tabs[i].scene.document = doc.clone();
        app.tabs[i].scene.selected.clear();
        app.tabs[i].scene.select_entity(handle, true);
        app.tabs[i].selected_handle = Some(handle);
        app.refresh_properties();
        let params = actions::all_parameters(&doc, definition);
        let grips = app.tabs[i].selected_grips.clone();
        let markers = grips
            .iter()
            .filter_map(|g| crate::scene::block_grips::runtime::decode(g.id))
            .collect::<Vec<_>>();
        eprintln!(
            "{context} params={:?}, markers={markers:?}",
            params.iter().map(|p| (&p.name, p.id)).collect::<Vec<_>>()
        );
        let visibility = OpenCADStudio::indexed_visibility_property(&doc, 0, handle);
        for prop in properties::properties(&doc, None, 0, handle)
            .into_iter()
            .chain(visibility)
        {
            match prop.value {
                PropValue::ReadOnlyWithTooltip { tooltip, .. } => fail(&prop.label, &tooltip),
                PropValue::BlockNumber { key, value, .. } => {
                    controls += 1;
                    if let Some(number) =
                        crate::app::block_parameter_properties::parse_value(&value)
                    {
                        for next in [number + 1., number + 10.] {
                            if let Err(err) = properties::evaluate(
                                &doc,
                                None,
                                key,
                                properties::Input::Number(next),
                            ) {
                                fail(&prop.label, err);
                                break;
                            }
                        }
                    }
                }
                PropValue::BlockChoice {
                    key,
                    options,
                    selected,
                    ..
                } => {
                    for next in (0..options.len()).filter(|&next| next != selected) {
                        controls += 1;
                        let result = if prop.field == "dyn_visibility" {
                            OpenCADStudio::stage_visibility_property(&doc, key, next)
                        } else {
                            properties::evaluate(&doc, None, key, properties::Input::Choice(next))
                        };
                        if let Err(err) = result {
                            fail(&format!("{} -> {next}", prop.label), err);
                        }
                    }
                }
                _ => {}
            }
        }
        for grip in &grips {
            let Some((node, role)) = crate::scene::block_grips::runtime::decode(grip.id) else {
                continue;
            };
            controls += 1;
            app.tabs[i].scene.document = doc.clone();
            app.tabs[i].scene.selected.clear();
            app.tabs[i].scene.select_entity(handle, true);
            app.tabs[i].selected_handle = Some(handle);
            app.refresh_properties();
            app.command_line.last_error = None;
            if !app.start_test_action_grip(handle, grip.id) {
                fail(
                    &format!("grip {node}/{role}"),
                    app.command_line
                        .last_error
                        .as_deref()
                        .unwrap_or("did not start"),
                );
                continue;
            }
            if app.tabs[i].active_cmd.is_some() {
                let direction = grip.dir.unwrap_or(DVec3::new(1., 1., 0.));
                let _ = app.feed_command(crate::command::StepInput::Point(
                    grip.world + direction * 3.,
                ));
                app.tabs[i].active_cmd = None;
            }
            if let Some(error) = app.command_line.last_error.as_deref() {
                fail(&format!("grip {node}/{role}"), error);
            }
        }
        if controls == previous_controls {
            fail("controls", "no grip or editable property was exercised");
        }
    }
    eprintln!(
        "Audited {total} dynamic references, {controls} control inputs, {static_references} static references"
    );
    failures.sort();
    failures.dedup();
    assert!(total > 0);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
