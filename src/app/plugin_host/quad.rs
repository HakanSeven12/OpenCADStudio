// Quad menu panel conversion helpers.

use super::*;

/// Map the visible plugins' registered quad panels into the quad model's
/// panels (docs/quad-menu.md §4): builtin panels first, then plugins in
/// registration order (the store's Vec keeps that order stable). The exposed
/// panel id is namespaced `plugin:<plugin_id>:<panel_id>` (the namespacing
/// guard against the builtin `quad.*` ids) and every item dispatches
/// `quadplugin:<plugin_id>:<panel_id>:<control_id>`.
///
/// Liveness echoes the grips: a disabled or dead plugin contributes nothing
/// (the same `grip_points::visibility` predicate), and the icon budget
/// applies here — an oversized or unparseable `OwnedIconKind::Svg` degrades
/// to the glyph tile instead of refusing the item (§4 "Custom plugin icons":
/// defensive fallback).
pub fn plugin_quad_panels(
    disabled: &rustc_hash::FxHashSet<String>,
) -> Vec<crate::ui::popup::quad_menu::QuadPanel> {
    let visible = crate::plugin::grip_points::visibility(disabled);
    let panels = crate::plugin::quad::with_store(|store| store.panels_for_visible(visible));
    // User decision: only explicitly-registered quad panels contribute.
    // Plain plugin commands stay command-line / auto-complete; no auto
    // "Plugins" tab is synthesized (the v14 compat feed is gone).
    panels
        .into_iter()
        .map(|(plugin_id, def)| plugin_quad_panel(&plugin_id, def))
        .collect()
}

/// One declared quad panel (API v15) as the quad model's
/// [`QuadPanel`](crate::ui::popup::quad_menu::QuadPanel): the namespaced id,
/// the title as tab label and every item mapped to its `quadplugin:` command
/// with the §4 icon conversion. Both action kinds ride the same
/// `quadplugin:` string; the pick path resolves the declared kind
/// (`QuadStore::resolve`) and either dispatches the `Command` verb host-side
/// or publishes the `Callback` pick to the owning plugin.
fn plugin_quad_panel(
    plugin_id: &str,
    def: ocs_plugin_api::dialog::QuadPanelDef,
) -> crate::ui::popup::quad_menu::QuadPanel {
    use crate::ui::popup::quad_menu::{QuadItem, QuadPanel};

    let ocs_plugin_api::dialog::QuadPanelDef {
        panel_id,
        title,
        applies_to,
        items,
    } = def;
    let items = items
        .into_iter()
        .map(|item| {
            // The command is the scheme triple for both action kinds; the
            // pick path looks the kind up at dispatch time.
            QuadItem {
                label: item.label.clone(),
                command: format!(
                    "{}{plugin_id}:{panel_id}:{control_id}",
                    crate::plugin::QUAD_PLUGIN_PREFIX,
                    control_id = item.control_id,
                ),
                icon: quad_item_icon(item.label, item.icon),
            }
        })
        .collect();
    QuadPanel {
        id: crate::plugin::quad::quad_panel_ui_id(plugin_id, &panel_id),
        label: title,
        items,
        applies_to,
    }
}

/// The §4 icon budget conversion (docs/quad-menu.md "Custom plugin icons"):
/// svg bytes within the budget that at least look like SVG become the UI's
/// [`QuadIcon::SvgOwned`](crate::ui::popup::quad_menu::QuadIcon::SvgOwned);
/// a declared glyph passes through; an oversized or malformed-intent icon
/// degrades to the glyph tile (label's first letter) instead of refusing the
/// item — the panel survives, the plugin sees the reason in its response.
pub(super) fn quad_item_icon(
    label: String,
    icon: ocs_plugin_api::ribbon::owned::OwnedIconKind,
) -> crate::ui::popup::quad_menu::QuadIcon {
    use ocs_plugin_api::ribbon::owned::OwnedIconKind;

    match icon {
        OwnedIconKind::Svg(bytes)
            if bytes.len() <= crate::plugin::quad::MAX_QUAD_ICON_BYTES
                && looks_like_svg(&bytes) =>
        {
            crate::ui::popup::quad_menu::QuadIcon::SvgOwned(bytes.into())
        }
        OwnedIconKind::Glyph(glyph) => crate::ui::popup::quad_menu::QuadIcon::Glyph(glyph),
        _ => crate::ui::popup::quad_menu::QuadIcon::Glyph(
            label
                .chars()
                .next()
                .map(|ch| ch.to_ascii_uppercase().to_string())
                .unwrap_or_else(|| "?".to_string()),
        ),
    }
}

/// Cheap "this is meant to be an SVG document" check — a parse happens in
/// the renderer; this only routes obvious non-SVG payloads to the glyph
/// fallback (§4 "malformed-intent").
fn looks_like_svg(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .map(|text| {
            let head = text.trim_start();
            head.starts_with("<svg") || head.starts_with("<?xml") || head.starts_with("<!--")
        })
        .unwrap_or(false)
}
