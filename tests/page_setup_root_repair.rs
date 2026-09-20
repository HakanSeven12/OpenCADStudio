// Some DWGs written by other programs leave the header's root named-objects
// dictionary pointer unresolvable — it names a handle that never loaded (or a
// non-dictionary), while the real named-object sub-dictionaries are owned by an
// unrelated handle. Navigating that root then silently no-ops, so registering a
// new named-object entry (a page setup, the variable dictionary, an
// annotation scale) would vanish instead of persisting.
//
// `annotative::root_named_dict_handle` resolves the root robustly and, when it
// truly can't be found, synthesises a fresh one so registration sticks. These
// guards exercise that repair through the page-setup and CTAB write paths.

use acadrust::objects::{ObjectType, PlotSettings};
use acadrust::Handle;
use OpenCADStudio::scene::Scene;

/// Break a document the way a foreign DWG does: dangle the header root pointer
/// and re-home the real root under a non-dictionary owner, so neither the
/// pointer nor the `owner == NULL` heuristic can rediscover it.
fn break_root_pointer(scene: &mut Scene) {
    let doc = &mut scene.document;
    let real_root = doc.header.named_objects_dict_handle;
    if let Some(ObjectType::Dictionary(d)) = doc.objects.get_mut(&real_root) {
        // Owner points at the dimstyle-control handle (0x0A) — present in the
        // file but not a dictionary, exactly like the repro drawing.
        d.owner = Handle::new(0x0A);
    }
    doc.header.named_objects_dict_handle = Handle::new(0x00DE_AD00);
    doc.header.acad_plotsettings_dict_handle = Handle::new(0x00BA_D000);
}

#[test]
fn page_setup_persists_when_root_pointer_is_unresolvable() {
    let mut scene = Scene::new();
    break_root_pointer(&mut scene);

    scene.page_setup_save("Metric-A4", PlotSettings::new("Metric-A4"));

    // The setup is retrievable again…
    assert!(
        scene.page_setup_names().contains(&"Metric-A4".to_string()),
        "saved page setup must be listed back"
    );
    assert!(
        scene.page_setup_get("Metric-A4").is_some(),
        "saved page setup must be resolvable"
    );

    // …and it is linked into a *resolvable* root named-objects dictionary, so it
    // will round-trip through a save rather than dangling.
    let root_h = scene.document.header.named_objects_dict_handle;
    let ObjectType::Dictionary(root) = scene
        .document
        .objects
        .get(&root_h)
        .expect("root handle must resolve to a loaded object")
    else {
        panic!("root handle must resolve to a dictionary");
    };
    assert!(
        root.entries.iter().any(|(k, _)| k == "ACAD_PLOTSETTINGS"),
        "ACAD_PLOTSETTINGS must be registered in the (repaired) root NOD"
    );
}

#[test]
fn page_setup_survives_a_dwg_roundtrip_after_root_repair() {
    let mut scene = Scene::new();
    break_root_pointer(&mut scene);
    scene.page_setup_save("Wide", PlotSettings::new("Wide"));

    // Full DWG write/read cycle: the repaired root must carry the setup across.
    let bytes = OpenCADStudio::io::save_to_bytes(&scene.document, "dwg", scene.document.version)
        .expect("save to DWG bytes");
    let doc = OpenCADStudio::io::load_bytes("roundtrip.dwg", bytes).expect("reload DWG bytes");

    let mut reloaded = Scene::new();
    reloaded.document = doc;
    assert!(
        reloaded.page_setup_names().contains(&"Wide".to_string()),
        "page setup must persist across a DWG save/reload once the root is repaired"
    );
}

#[test]
fn existing_but_unlinked_plot_settings_are_still_listed() {
    // A drawing whose ACAD_PLOTSETTINGS dictionary exists but whose header
    // pointer to it is stale: the setups must still be discoverable via the
    // PlotSettings objects' owner, mirroring the scale-list fallback.
    let mut scene = Scene::new();
    scene.page_setup_save("Draft", PlotSettings::new("Draft"));
    // Now stale the header pointer (leave the dictionary + object intact).
    scene.document.header.acad_plotsettings_dict_handle = Handle::new(0x00BA_D000);

    assert!(
        scene.page_setup_names().contains(&"Draft".to_string()),
        "page setup must be found via the PlotSettings owner when the header \
         plot-settings pointer is stale"
    );
}

#[test]
fn ctab_is_created_against_a_repaired_root() {
    let mut scene = Scene::new();
    break_root_pointer(&mut scene);

    // Switching the active layout records CTAB; with a broken root pointer this
    // used to silently drop the record.
    scene.set_current_layout("Layout1".to_string());
    assert_eq!(
        OpenCADStudio::io::saved_active_layout(&scene.document).as_deref(),
        Some("Layout1"),
        "CTAB must persist even when the root pointer needed repair"
    );

    let root_h = scene.document.header.named_objects_dict_handle;
    let ObjectType::Dictionary(root) = scene
        .document
        .objects
        .get(&root_h)
        .expect("root must resolve")
    else {
        panic!("root must be a dictionary");
    };
    let variable_dictionary = root
        .entries
        .iter()
        .find(|(key, _)| key == "AcDbVariableDictionary")
        .map(|(_, handle)| *handle)
        .expect("variable dictionary must be registered in the repaired root NOD");
    let Some(ObjectType::Dictionary(variable_dictionary)) =
        scene.document.objects.get(&variable_dictionary)
    else {
        panic!("variable dictionary must resolve");
    };
    assert!(
        variable_dictionary
            .entries
            .iter()
            .any(|(key, _)| key == "CTAB"),
        "CTAB must be registered in the variable dictionary"
    );
}
