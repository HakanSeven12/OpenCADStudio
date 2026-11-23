use super::*;

// ── API v15: quad cursor menu panels ─────────────────────────────────────

fn quad_panel(id: &str, controls: &[&str]) -> ocs_plugin_api::dialog::QuadPanelDef {
    use ocs_plugin_api::dialog::{QuadActionDef, QuadItemDef};
    use ocs_plugin_api::ribbon::owned::OwnedIconKind;

    let mut def = ocs_plugin_api::dialog::QuadPanelDef::new(id, id.to_ascii_uppercase());
    for control in controls {
        def = def.with_item(QuadItemDef {
            control_id: (*control).to_string(),
            label: control.to_string(),
            icon: OwnedIconKind::Glyph(control.to_string()),
            action: QuadActionDef::Callback,
        });
    }
    def
}

#[test]
fn quad_panels_register_replace_per_owner_and_reject_foreign() {
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    // The sink marks the plugin loaded+alive for the visibility predicate
    // (the class-owner tests' idiom; thread-local).
    crate::plugin::grip_points::install_in_process_sink("PLUGIN", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    crate::plugin::grip_points::install_in_process_sink("PLUGIN_RIVAL", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });

    // Register (owner PLUGIN) and map to the quad model feed.
    let results = {
        let mut host = HostSession::new(&mut app, 0);
        host.register_quad_panels("PLUGIN", &[quad_panel("qtest.align", &["flip"])])
    };
    assert_eq!(results, vec![true]);
    {
        // The feed probe is a read-only snapshot: read it before the
        // session borrows `app` mutably (the store is process-global).
        let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
        let mut host = HostSession::new(&mut app, 0);
        let mine = feed
            .iter()
            .find(|panel| panel.id == "plugin:PLUGIN:qtest.align")
            .expect("the registered panel joins the model feed");
        assert_eq!(mine.label, "QTEST.ALIGN");
        assert_eq!(mine.items[0].command, "quadplugin:PLUGIN:qtest.align:flip");

        // Same owner: per-panel replace keeps the owner's slot (the new
        // declaration's item count wins).
        let results =
            host.register_quad_panels("PLUGIN", &[quad_panel("qtest.align", &["flip", "other"])]);
        assert_eq!(results, vec![true]);
        drop(host);
        let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
        assert_eq!(
            feed.iter()
                .find(|panel| panel.id == "plugin:PLUGIN:qtest.align")
                .expect("still registered")
                .items
                .len(),
            2
        );

        // A foreign owner cannot replace another plugin's panel id.
        let mut host = HostSession::new(&mut app, 0);
        let results =
            host.register_quad_panels("PLUGIN_RIVAL", &[quad_panel("qtest.align", &["x"])]);
        assert_eq!(results, vec![false]);

        // Unregister: only the owner's ids are removed (the ownership
        // law runs both ways — the rival's own id refuses the owner).
        assert_eq!(
            host.unregister_quad_panels("PLUGIN", &["qtest.align"]),
            vec![true]
        );
        assert_eq!(
            host.unregister_quad_panels("PLUGIN", &["qtest.align"]),
            vec![false]
        );
    }
    let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
    assert!(
        feed.iter()
            .all(|panel| panel.id != "plugin:PLUGIN:qtest.align"),
        "the unregistered panel drops out of the feed"
    );
    crate::plugin::grip_points::remove_in_process_sink("PLUGIN");
    crate::plugin::grip_points::remove_in_process_sink("PLUGIN_RIVAL");
}

#[test]
fn quad_panels_icon_budget_degrades_to_glyph() {
    use crate::ui::popup::quad_menu::QuadIcon;
    use ocs_plugin_api::dialog::QuadItemDef;
    use ocs_plugin_api::ribbon::owned::OwnedIconKind;

    // The §4 conversion: a good svg keeps its bytes in the UI model; an
    // oversized one degrades to the glyph tile without refusing the
    // panel; a declared glyph passes through.
    let good = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>"
        .as_bytes()
        .to_vec();
    let oversized = vec![b'x'; crate::plugin::quad::MAX_QUAD_ICON_BYTES + 1];
    let def = ocs_plugin_api::dialog::QuadPanelDef::new("qtest.icons", "Icons")
        .with_item(QuadItemDef {
            control_id: "good".to_string(),
            label: "Good".to_string(),
            icon: OwnedIconKind::Svg(good),
            action: ocs_plugin_api::dialog::QuadActionDef::Callback,
        })
        .with_item(QuadItemDef {
            control_id: "big".to_string(),
            label: "Big".to_string(),
            icon: OwnedIconKind::Svg(oversized),
            action: ocs_plugin_api::dialog::QuadActionDef::Callback,
        })
        .with_item(QuadItemDef {
            control_id: "glyph".to_string(),
            label: "Glyph".to_string(),
            icon: OwnedIconKind::Glyph("G".to_string()),
            action: ocs_plugin_api::dialog::QuadActionDef::Callback,
        });
    // The conversion is a pure mapping — exercise it directly.
    let icons: Vec<QuadIcon> = def
        .items
        .into_iter()
        .map(|item| quad_item_icon(item.label, item.icon))
        .collect();
    assert!(matches!(icons[0], QuadIcon::SvgOwned(_)));
    assert!(matches!(&icons[1], QuadIcon::Glyph(text) if text == "B"));
    assert!(matches!(&icons[2], QuadIcon::Glyph(text) if text == "G"));
}

#[test]
fn quad_feed_has_no_v14_auto_tab_per_plugin() {
    // User decision: the v14 auto "Plugins" tab is gone. A visible plugin
    // with plain commands contributes no quad panel at all — only
    // explicitly registered quad panels do (its commands stay reachable
    // through the command line / auto-complete).
    let mut app = OpenCADStudio::new_for_test();
    app.tabs[0].is_start = false;
    // The sink marks the plugin loaded+alive for the visibility predicate
    // (the register tests' idiom): with the removed synthesis this is
    // exactly the case that used to grow an auto tab.
    // An id of its own: the quad store is process-wide, and other tests
    // register panels for "PLUGIN" while this one runs.
    crate::plugin::grip_points::install_in_process_sink("PLUGIN_NO_QUAD", |_host, _event| {
        ocs_plugin_api::host::CustomGripResponse::default()
    });
    let feed = crate::app::plugin_host::plugin_quad_panels(&app.disabled_plugins);
    assert_eq!(
        feed.iter()
            .filter(|panel| panel.id.ends_with(":auto"))
            .count(),
        0,
        "no auto tab is synthesized for a plain-command plugin"
    );
    assert_eq!(
        feed.len(),
        0,
        "no registration, no contribution: the feed stays empty"
    );
    crate::plugin::grip_points::remove_in_process_sink("PLUGIN_NO_QUAD");
}
