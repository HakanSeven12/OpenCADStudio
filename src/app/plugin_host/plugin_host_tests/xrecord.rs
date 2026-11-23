use super::*;

#[test]
fn xrecord_crud_round_trips_refusals_and_list() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "{\"k\":\"road\",\"s\":1}"),
            document::objects::XRecordEntry::double(40, 0.125),
            document::objects::XRecordEntry::int16(70, 3),
            document::objects::XRecordEntry::int32(90, -45000),
            document::objects::XRecordEntry::bool(290, true),
        ]
    };

    // Create seeds the namespace: the dictionary chain is auto-created and
    // reading returns the entries verbatim.
    let alpha = host
        .create_xrecord(&["PLUGIN"], "alpha", entries())
        .unwrap();
    assert!(!alpha.is_null());
    assert_eq!(host.read_xrecord(&["PLUGIN"], "alpha").unwrap(), entries());
    // Dictionary lookup is case-insensitive, both for the path …
    assert_eq!(host.read_xrecord(&["plugin"], "alpha").unwrap(), entries());
    // â€¦ and re-creating an existing name is refused without any change.
    assert!(
        host.create_xrecord(&["plugin"], "ALPHA", entries())
            .unwrap_err()
            .contains("already exists")
    );

    // Namespace reuse is idempotent: the root still holds one PLUGIN entry.
    let beta = host.create_xrecord(&["PLUGIN"], "beta", entries()).unwrap();
    let root = host.document().header.named_objects_dict_handle;
    let cvifc_entries = match host.document().objects.get(&root) {
        Some(document::objects::ObjectType::Dictionary(dictionary)) => dictionary
            .entries
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case("PLUGIN"))
            .count(),
        _ => 0,
    };
    assert_eq!(cvifc_entries, 1);

    // Listing names only the keys that address XRecords, in dictionary
    // order; a nested dictionary is not an XRecord.
    host.create_xrecord(&["PLUGIN", "LAYOUTS"], "nested", entries())
        .unwrap();
    assert_eq!(
        host.list_xrecords(&["PLUGIN"]).unwrap(),
        vec!["alpha".to_owned(), "beta".to_owned()]
    );
    assert_eq!(
        host.list_xrecords(&["PLUGIN", "LAYOUTS"]).unwrap(),
        vec!["nested".to_owned()]
    );
    // The root dictionary holds non-XRecord entries (ACAD_LAYOUT et al.);
    // listing only reports XRecords.
    let root_names = host.list_xrecords(&[]).unwrap();
    assert!(
        !root_names
            .iter()
            .any(|n| n.eq_ignore_ascii_case("ACAD_LAYOUT"))
    );

    // Update refuses on a missing name and never creates anything.
    assert!(
        host.update_xrecord(&["PLUGIN"], "gamma", entries())
            .unwrap_err()
            .contains("does not exist")
    );
    // Case-insensitive resolution finds the stored ALPHA and replaces its
    // entries atomically; the handle is stable.
    assert_eq!(
        host.update_xrecord(
            &["PLUGIN"],
            "ALPHA",
            vec![document::objects::XRecordEntry::string(1, "v2")]
        )
        .unwrap(),
        alpha
    );
    assert_eq!(
        host.read_xrecord(&["PLUGIN"], "alpha").unwrap(),
        vec![document::objects::XRecordEntry::string(1, "v2")]
    );

    // Entries are validated before any change: reserved and out-of-range
    // group codes refuse (5 and 105 are the object handle codes).
    assert!(
        host.create_xrecord(
            &["PLUGIN"],
            "gamma",
            vec![document::objects::XRecordEntry::int16(5, 1)]
        )
        .unwrap_err()
        .contains("reserved")
    );
    assert!(
        host.create_xrecord(
            &["PLUGIN"],
            "gamma",
            vec![document::objects::XRecordEntry::int16(400, 1)]
        )
        .unwrap_err()
        .contains("out of range")
    );
    assert!(
        host.update_xrecord(
            &["PLUGIN"],
            "alpha",
            vec![document::objects::XRecordEntry::int16(105, 1)]
        )
        .is_err()
    );
    assert_eq!(
        host.read_xrecord(&["PLUGIN"], "alpha").unwrap(),
        vec![document::objects::XRecordEntry::string(1, "v2")]
    );
    // Dictionary and record names are symbol names.
    assert!(host.create_xrecord(&["PLUGIN"], "  ", entries()).is_err());
    assert!(
        host.create_xrecord(&["PLUGIN", "A|B"], "gamma", entries())
            .is_err()
    );
    assert!(host.create_xrecord(&["PLUGIN"], "a<b", entries()).is_err());

    // Reads distinguish a missing dictionary, a missing name and a name
    // that addresses another object kind.
    assert!(
        host.read_xrecord(&["NOPE"], "alpha")
            .unwrap_err()
            .contains("does not exist")
    );
    assert!(
        host.read_xrecord(&["PLUGIN", "LAYOUTS"], "alpha")
            .unwrap_err()
            .contains("does not exist")
    );
    assert!(
        host.delete_xrecord(&["PLUGIN"], "LAYOUTS")
            .unwrap_err()
            .contains("not an XRecord")
    );

    // Delete removes exactly the named record and returns its content.
    let removed = host.delete_xrecord(&["PLUGIN"], "beta").unwrap();
    assert_eq!(removed.handle, beta);
    assert_eq!(removed.entries, entries());
    assert_eq!(
        host.list_xrecords(&["PLUGIN"]).unwrap(),
        vec!["alpha".to_owned()]
    );
    assert!(
        host.read_xrecord(&["PLUGIN"], "beta")
            .unwrap_err()
            .contains("does not exist")
    );
    assert!(host.delete_xrecord(&["PLUGIN"], "beta").is_err());
}

#[test]
fn xrecord_mutations_are_single_undo_steps_that_restore_objects() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let original_entries = || vec![document::objects::XRecordEntry::string(1, "one")];
    let changed_entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "changed"),
            document::objects::XRecordEntry::int16(70, 9),
        ]
    };
    let (alpha, beta);
    {
        let mut host = HostSession::new(&mut app, 0);
        alpha = host
            .create_xrecord(&["PLUGIN"], "alpha", original_entries())
            .unwrap();
        beta = host
            .create_xrecord(&["PLUGIN"], "beta", original_entries())
            .unwrap();
        // Refusals record no undo step.
        assert!(
            host.create_xrecord(&["PLUGIN"], "alpha", original_entries())
                .is_err()
        );
        assert!(
            host.update_xrecord(&["PLUGIN"], "gamma", original_entries())
                .is_err()
        );
        host.update_xrecord(&["PLUGIN"], "alpha", changed_entries())
            .unwrap();
        let removed = host.delete_xrecord(&["PLUGIN"], "beta").unwrap();
        assert_eq!(removed.handle, beta);
    }
    app.finish_pending_history(0);
    // Four mutating calls = four undo steps; the refusals recorded none.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 4);

    let xrecord = |doc: &Document, handle: Handle| -> Option<document::objects::XRecord> {
        match doc.objects.get(&handle) {
            Some(document::objects::ObjectType::XRecord(record)) => Some(record.clone()),
            _ => None,
        }
    };
    let has_dictionary_entry = |doc: &Document, name: &str| -> bool {
        let root = doc.header.named_objects_dict_handle;
        match doc.objects.get(&root) {
            Some(document::objects::ObjectType::Dictionary(dictionary)) => dictionary
                .entries
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name)),
            _ => false,
        }
    };

    // Undo the delete: beta is back with its exact prior entries.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert_eq!(xrecord(document, beta).unwrap().entries, original_entries());
    // Undo the update: alpha's entries are restored byte-exactly.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert_eq!(
        xrecord(document, alpha).unwrap().entries,
        original_entries()
    );
    // Undo beta's creation.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(xrecord(document, beta).is_none());
    assert!(xrecord(document, alpha).is_some());
    // Undo alpha's creation: the auto-created dictionary went with it,
    // because the pending history snapshot restores the whole objects map.
    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(xrecord(document, alpha).is_none());
    assert!(
        !has_dictionary_entry(document, "PLUGIN"),
        "undoing the first create removed the auto-created dictionary"
    );
    // Redo re-applies the whole ladder.
    app.redo_steps(4);
    let document = &app.tabs[0].scene.document;
    assert_eq!(xrecord(document, alpha).unwrap().entries, changed_entries());
    assert!(xrecord(document, beta).is_none());
}

/// The entity-write composition contract, for XRecords: a plugin opens one
/// `push_undo` scope per operation and performs every write — XRecord and
/// entity, plus refusals — inside it, and the finished scope is a single
/// undo step under the plugin's own label that restores (and re-applies)
/// the whole operation at once.
#[test]
fn xrecord_writes_join_a_plugin_initiated_undo_scope() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let original_entries = || vec![document::objects::XRecordEntry::string(1, "one")];
    let changed_entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "changed"),
            document::objects::XRecordEntry::int16(70, 9),
        ]
    };
    let (alpha, nested, entity);
    {
        let mut host = HostSession::new(&mut app, 0);
        host.push_undo("PLUGIN: import");
        alpha = host
            .create_xrecord(&["PLUGIN"], "alpha", original_entries())
            .unwrap();
        nested = host
            .create_xrecord(&["PLUGIN", "LAYOUTS"], "nested", original_entries())
            .unwrap();
        // A refusal inside the scope records nothing at all.
        assert!(
            host.create_xrecord(&["PLUGIN"], "ALPHA", original_entries())
                .is_err()
        );
        assert_eq!(
            host.update_xrecord(&["PLUGIN"], "alpha", changed_entries())
                .unwrap(),
            alpha
        );
        let beta = host
            .create_xrecord(&["PLUGIN"], "beta", original_entries())
            .unwrap();
        let removed = host.delete_xrecord(&["PLUGIN"], "beta").unwrap();
        assert_eq!(removed.handle, beta);
        // Entity writes join the same scope and land in the same step.
        entity = host.add_entity(EntityType::Point(Point::at(document::types::Vector3::new(
            3.0, 0.0, 0.0,
        ))));
    }
    app.finish_pending_history(0);
    // The whole plugin operation is ONE undo step under the plugin's label.
    assert_eq!(app.tabs[0].history.undo_stack.len(), 1);
    assert_eq!(app.tabs[0].history.undo_stack[0].label(), "PLUGIN: import");

    let xrecord = |doc: &Document, handle: Handle| -> Option<document::objects::XRecord> {
        match doc.objects.get(&handle) {
            Some(document::objects::ObjectType::XRecord(record)) => Some(record.clone()),
            _ => None,
        }
    };
    let has_dictionary_entry = |doc: &Document, name: &str| -> bool {
        let root = doc.header.named_objects_dict_handle;
        match doc.objects.get(&root) {
            Some(document::objects::ObjectType::Dictionary(dictionary)) => dictionary
                .entries
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case(name)),
            _ => false,
        }
    };

    app.undo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(document.get_entity(entity).is_none());
    assert!(xrecord(document, alpha).is_none());
    assert!(xrecord(document, nested).is_none());
    assert!(
        !has_dictionary_entry(document, "PLUGIN"),
        "undoing the operation removed its auto-created dictionary namespace"
    );
    app.redo_steps(1);
    let document = &app.tabs[0].scene.document;
    assert!(matches!(
        document.get_entity(entity),
        Some(EntityType::Point(_))
    ));
    let alpha_record = xrecord(document, alpha).unwrap();
    assert_eq!(alpha_record.entries, changed_entries());
    assert!(xrecord(document, nested).is_some());
    assert!(
        has_dictionary_entry(document, "PLUGIN"),
        "redoing the operation re-seeded its namespace"
    );
}

#[test]
fn xrecords_survive_dxf_and_dwg_round_trips() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    let mut host = HostSession::new(&mut app, 0);
    let entries = || {
        vec![
            document::objects::XRecordEntry::string(1, "{\"k\":\"road\",\"s\":1}"),
            document::objects::XRecordEntry::double(40, 0.125),
            document::objects::XRecordEntry::int16(70, 3),
            document::objects::XRecordEntry::int32(90, -45000),
            document::objects::XRecordEntry::bool(290, true),
        ]
    };
    // Create carries a first payload, then the update replaces it: if the
    // writers preferred stale raw bytes over the typed entries, the loaded
    // record would still read "stale".
    host.create_xrecord(
        &["PLUGIN", "RECORDS"],
        "alpha",
        vec![document::objects::XRecordEntry::string(1, "stale")],
    )
    .unwrap();
    host.update_xrecord(&["PLUGIN", "RECORDS"], "alpha", entries())
        .unwrap();

    let find_xrecord =
        |doc: &Document, path: &[&str], name: &str| -> Option<document::objects::XRecord> {
            let mut current = doc.header.named_objects_dict_handle;
            for fragment in path {
                match doc.objects.get(&current) {
                    Some(document::objects::ObjectType::Dictionary(dictionary)) => {
                        current = dictionary.get(fragment)?
                    }
                    _ => return None,
                }
            }
            let Some(document::objects::ObjectType::Dictionary(dictionary)) =
                doc.objects.get(&current)
            else {
                return None;
            };
            let handle = dictionary.get(name)?;
            match doc.objects.get(&handle) {
                Some(document::objects::ObjectType::XRecord(record)) => Some(record.clone()),
                _ => None,
            }
        };

    let dwg = crate::io::load_bytes(
        "xrec.dwg",
        document::DwgWriter::write_to_vec(host.document()).unwrap(),
    )
    .unwrap();
    let reloaded =
        find_xrecord(&dwg, &["PLUGIN", "RECORDS"], "alpha").expect("XRecord in the reloaded DWG");
    assert_eq!(reloaded.entries, entries());

    let dxf = crate::io::load_bytes(
        "xrec.dxf",
        document::DxfWriter::new(host.document())
            .write_to_vec()
            .unwrap(),
    )
    .unwrap();
    let reloaded =
        find_xrecord(&dxf, &["PLUGIN", "RECORDS"], "alpha").expect("XRecord in the reloaded DXF");
    assert_eq!(reloaded.entries, entries());
}
