use super::*;

#[test]
fn boot_time_declaration_is_attributed_to_the_declared_package_id() {
    // HOST-FIXES-TODO 1: the spawn bootstrap registers entity classes and
    // quad panels while GetManifest is still in flight — the registry
    // entry does not exist yet, and the manifest window used to serve
    // with an empty identity ("plugin \"\" is not a loaded, alive
    // plugin"). The declaration law now treats the IN-FLIGHT BOOT as the
    // live, authenticated transport: the package's declared id rides the
    // connection before the manifest and the manifest must confirm it,
    // while the empty identity still refuses.
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    crate::plugin::external::with_fresh_installed_manager(|| {
        // An empty identity is never attributable, boot window or not.
        let empty_err = host
            .register_plugin_class("", &traverse_class())
            .unwrap_err();
        assert!(
            empty_err.contains("not a loaded, alive plugin"),
            "got: {empty_err}"
        );

        // The declared package id in its boot window passes the law.
        crate::plugin::external::with_manager(|m| m.begin_boot("opencad.example"));
        assert!(
            host.register_plugin_class("opencad.example", &traverse_class())
                .is_ok()
        );

        // After the boot window closes (and without a registry entry at
        // that id) the law refuses again: a different class key under the
        // same owner must not register.
        crate::plugin::external::with_manager(|m| m.end_boot("opencad.example"));
        let mut unrelated = traverse_class();
        unrelated.class_id = "OTHER_KEY".to_string();
        let closed_err = host
            .register_plugin_class("opencad.example", &unrelated)
            .unwrap_err();
        assert!(
            closed_err.contains("not a loaded, alive plugin"),
            "got: {closed_err}"
        );

        // The empty identity law survives the whole exercise.
        let still_empty = host
            .register_plugin_class("", &traverse_class())
            .unwrap_err();
        assert!(still_empty.contains("not a loaded, alive plugin"));
    });
}

#[test]
fn plugin_class_registration_is_owned_idempotent_and_refusing() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let not_owner = host
        .register_plugin_class("ghost", &traverse_class())
        .unwrap_err();
    assert!(
        not_owner.contains("not a loaded, alive plugin"),
        "got: {not_owner}"
    );
    install_class_owner();
    // Register, then re-register identical: idempotent, still one entry.
    assert!(
        host.register_plugin_class("PLUGIN", &traverse_class())
            .is_ok()
    );
    assert!(
        host.register_plugin_class("PLUGIN", &traverse_class())
            .is_ok()
    );
    assert_eq!(host.app.tabs[0].plugin_classes.total(), 1);
    // A different schema refuses without touching the live class.
    let mut bumped = traverse_class();
    bumped.schema_version = 5;
    let err = host.register_plugin_class("PLUGIN", &bumped).unwrap_err();
    assert!(err.contains("different schema"), "got: {err}");
    assert_eq!(
        host.app.tabs[0]
            .plugin_classes
            .find("PLUGIN", "PLUGIN_ALIGN_DISPLAY")
            .expect("live entry")
            .schema_version,
        4
    );
    // Malformed declarations refuse: empty identity, unnamed grip
    // parameter, duplicate grip ids.
    let mut empty = traverse_class();
    empty.class_id = String::new();
    assert!(host.register_plugin_class("PLUGIN", &empty).is_err());
    let mut unnamed = traverse_class();
    unnamed.grip_schema[0].param_name = String::new();
    assert!(host.register_plugin_class("PLUGIN", &unnamed).is_err());
    let mut dup = traverse_class();
    dup.grip_schema
        .push(ocs_plugin_api::host::PluginGripSchemaEntry {
            grip_id: 1,
            param_name: "other".to_string(),
            drives: None,
        });
    assert!(host.register_plugin_class("PLUGIN", &dup).is_err());
    assert_eq!(host.app.tabs[0].plugin_classes.total(), 1);
}

#[test]
fn plugin_class_registry_survives_across_request_sessions() {
    // The registry is TAB state, not session state: HostSession lives one
    // request; the class declaration must outlive the dropping session
    // (this is the regression for the per-session-registry defect).
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            host.register_plugin_class("PLUGIN", &traverse_class())
                .is_ok()
        );
    }
    // Session B: a DIFFERENT HostSession over the same tab (the IPC
    // server's per-request reality) stages the instance.
    let mut host = HostSession::new(&mut app, 0);
    let handles = host
        .stage_plugin_class_instance("PLUGIN", &traverse_instance())
        .expect("the class outlives its declaring session");
    assert!(!handles.is_empty());
    assert!(host.app.tabs[0].scene.is_reference_only(handles[0]));
    assert!(!host.app.tabs[0].scene.native_edit_disabled(handles[0]));
    let stamp = host
        .read_record(handles[0], "PLUGIN_ALIGN_DISPLAY")
        .expect("legacy owner stamp");
    assert_eq!(stamp.values.len(), 2, "unflagged staging is unchanged");
}

#[test]
fn selectable_custom_instances_are_selectable_but_native_edit_disabled() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let mut host = HostSession::new(&mut app, 0);
    assert!(
        host.register_plugin_class("PLUGIN", &traverse_class())
            .is_ok()
    );
    let mut instance = traverse_instance();
    instance.selectable_custom = true;
    instance.design_record.clear();
    let handles = host
        .stage_plugin_class_instance("PLUGIN", &instance)
        .expect("the selectable custom instance stages");
    assert!(!handles.is_empty());
    for &handle in &handles {
        assert!(!host.app.tabs[0].scene.is_reference_only(handle));
        assert!(host.app.tabs[0].scene.native_edit_disabled(handle));
        let stamp = host
            .read_record(handle, "PLUGIN_ALIGN_DISPLAY")
            .expect("selectable custom owner stamp");
        assert_eq!(stamp.values.len(), 3);
        assert!(matches!(
            &stamp.values[1],
            document::xdata::XDataValue::String(value) if value.is_empty()
        ));
        assert!(matches!(
            stamp.values[2],
            document::xdata::XDataValue::Integer32(1)
        ));
        assert!(host.entity_grips(handle).expect("grip read").is_empty());

        let original = host.document().get_entity(handle).unwrap().clone();
        assert!(!host.app.tabs[0].scene.update_entity(original.clone()));
        host.app.tabs[0].scene.erase_entities(&[handle]);
        assert!(host.document().get_entity(handle).is_some());
    }

    let handle = handles[0];
    host.app.tabs[0].scene.select_entity(handle, true);
    assert!(host.app.tabs[0].scene.selected.contains(&handle));
    assert!(host.app.property_target_handles(0).is_empty());
    let original = host.document().get_entity(handle).unwrap().clone();
    assert!(
        host.update_entity(original),
        "plugin updates remain allowed"
    );
    assert!(host.remove_entity(handle), "plugin erases remain allowed");
    assert!(host.document().get_entity(handle).is_none());
}

#[test]
fn plugin_class_instances_refuse_foreign_owners() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let mut host = HostSession::new(&mut app, 0);
    assert!(
        host.register_plugin_class("PLUGIN", &traverse_class())
            .is_ok()
    );
    // Another alive plugin sees the class but cannot stage it.
    crate::plugin::grip_points::install_in_process_sink("PLUGIN_RIVAL", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    let err = host
        .stage_plugin_class_instance("PLUGIN_RIVAL", &traverse_instance())
        .unwrap_err();
    assert!(err.contains("owned by plugin"), "got: {err}");
    assert!(err.contains("PLUGIN"), "got: {err}");
    assert_eq!(host.document().entities().count(), 0);
}

#[test]
fn plugin_class_instance_stages_reference_only_with_identity_stamp() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let instance = traverse_instance();
    let handles;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            host.register_plugin_class("PLUGIN", &traverse_class())
                .is_ok()
        );
        handles = host
            .stage_plugin_class_instance("PLUGIN", &instance)
            .expect("the class instance stages");
        assert!(!handles.is_empty());
        for handle in &handles {
            // The identity stamp: class key as the XDATA application, the
            // schema version + the record linkage as its values, the
            // APPID auto-registered so the stamp survives the file.
            let stamped = host
                .read_record(*handle, "PLUGIN_ALIGN_DISPLAY")
                .expect("the class identity stamp");
            assert_eq!(stamped.values.len(), 2);
            let ocs_plugin_api::host::document::xdata::XDataValue::Integer32(version) =
                stamped.values[0]
            else {
                panic!("the stamp carries the schema version as an integer");
            };
            assert_eq!(version, 4);
            let ocs_plugin_api::host::document::xdata::XDataValue::String(record) =
                stamped.values[1].clone()
            else {
                panic!("the stamp carries the design record linkage");
            };
            assert_eq!(record, "PLUGIN_ALIGN_DISPLAY");
            assert!(host.document().app_ids.contains("PLUGIN_ALIGN_DISPLAY"));
            assert!(
                host.app.tabs[0].scene.is_reference_only(*handle),
                "staged class display must be reference-only"
            );
        }
    }
    app.finish_pending_history(0);
    // Stamp + designation + add: ONE undo step, the entity-write contract.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    app.undo_steps(1);
    for handle in &handles {
        assert!(app.tabs[0].scene.document.get_entity(*handle).is_none());
    }
}

#[test]
fn class_stamp_and_display_set_survive_dxf_and_dwg_round_trips() {
    // The stamp's file-level law: the identity stamp (the XDATA record
    // under the auto-registered APPID) and the staged display set
    // survive BOTH file formats with empty loss rows; the stamp values
    // (schema version + record linkage + selectable-custom flags) stay
    // byte-identical.
    use document::{Dwg, Dxf, Format};

    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let handles;
    {
        let mut host = HostSession::new(&mut app, 0);
        assert!(
            host.register_plugin_class("PLUGIN", &traverse_class())
                .is_ok()
        );
        let mut instance = traverse_instance();
        instance.selectable_custom = true;
        handles = host
            .stage_plugin_class_instance("PLUGIN", &instance)
            .expect("the selectable custom class instance stages");
    }
    let read_stamp = |document: &document::Document, handle: document::Handle| {
        let entity = document
            .get_entity(handle)
            .unwrap_or_else(|| panic!("staged entity {handle:?} lost"));
        entity
            .common()
            .extended_data
            .get_record("PLUGIN_ALIGN_DISPLAY")
            .unwrap_or_else(|| panic!("the class identity stamp was lost"))
            .values
            .clone()
    };
    fn round_trip<F: Format>(
        format: &F,
        document: &document::Document,
        handles: &[document::Handle],
        stamp: &dyn Fn(&document::Document, document::Handle) -> Vec<document::xdata::XDataValue>,
        name: &str,
    ) {
        let (bytes, losses) = format.write(document).unwrap();
        assert!(
            losses.is_empty(),
            "{name} write must not lose anything: {losses:?}"
        );
        let (back, read_losses) = format.read(&bytes).unwrap();
        assert!(
            read_losses.is_empty(),
            "{name} read must not lose anything: {read_losses:?}"
        );
        for handle in handles {
            let expected = stamp(document, *handle);
            assert_eq!(
                stamp(&back, *handle),
                expected,
                "{name}: the class stamp must be byte-identical"
            );
            assert!(
                crate::plugin::entity_labels::native_edit_disabled(
                    back.get_entity(*handle).expect("flagged entity survives"),
                ),
                "{name}: the selectable-custom flag survives"
            );
        }
    }
    round_trip(
        &Dxf,
        &app.tabs[0].scene.document,
        &handles,
        &read_stamp,
        "DXF",
    );
    round_trip(
        &Dwg,
        &app.tabs[0].scene.document,
        &handles,
        &read_stamp,
        "DWG",
    );
}

#[test]
fn plugin_class_instance_refusals_change_nothing_and_record_no_steps() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    install_class_owner();
    let mut host = HostSession::new(&mut app, 0);
    assert!(
        host.register_plugin_class("PLUGIN", &traverse_class())
            .is_ok()
    );
    let epoch_before = host.app.tabs[0].scene.geometry_epoch;
    // Unregistered class.
    let mut ghost = traverse_instance();
    ghost.class_id = "GHOST".to_owned();
    assert!(host.stage_plugin_class_instance("PLUGIN", &ghost).is_err());
    // Schema version mismatch.
    let mut mismatch = traverse_instance();
    mismatch.schema_version = 5;
    assert!(
        host.stage_plugin_class_instance("PLUGIN", &mismatch)
            .is_err()
    );
    // Broken design parameter (the chain audit refuses at the contract).
    let mut broken = traverse_instance();
    let ocs_plugin_api::host::PluginDisplayDescriptor::ParametricChain(chain) = &mut broken.design;
    chain.segments[0].start_direction = f64::NAN;
    broken.tolerance = 1e-3;
    assert!(host.stage_plugin_class_instance("PLUGIN", &broken).is_err());
    // Broken tolerance.
    assert!(
        host.stage_plugin_class_instance("PLUGIN", &traverse_instance_for(0.0))
            .is_err()
    );
    assert_eq!(host.document().entities().count(), 0);
    assert_eq!(host.app.tabs[0].scene.geometry_epoch, epoch_before);
}
