//! Shared presentation lookup for command-driven UI surfaces.
//!
//! Authored catalog metadata is authoritative. Derived catalog entries keep
//! existing surface labels as an explicit compatibility fallback, which is
//! important for descriptive ribbon labels and third-party modules.

use crate::command::catalog::{self, MetadataQuality};
use crate::modules::IconKind;
use crate::ui::icon_catalog::{self, IconId};

/// Canonical command ID, including alias and transparent-command resolution.
pub fn canonical_id(command: &str) -> Option<&'static str> {
    catalog::canonical_id(command)
}

/// Localized command label. Authored metadata wins; a surface label remains
/// the fallback for derived or external commands.
pub fn label(command: &str, surface_fallback: &str) -> String {
    if let Some(variant) = catalog::variant(command) {
        return crate::i18n::translate(variant.label).into_owned();
    }
    match catalog::descriptor(command) {
        Some(descriptor) if descriptor.metadata_quality == MetadataQuality::Authored => {
            descriptor.translated_label().into_owned()
        }
        Some(descriptor) if surface_fallback.trim().is_empty() => {
            descriptor.translated_label().into_owned()
        }
        _ if !surface_fallback.trim().is_empty() => {
            crate::i18n::translate(surface_fallback).into_owned()
        }
        _ => command.trim().trim_start_matches('\'').to_string(),
    }
}

/// Localized authored description, when one exists.
pub fn description(command: &str) -> Option<String> {
    catalog::descriptor(command)
        .and_then(|descriptor| descriptor.translated_description())
        .map(Into::into)
}

/// Stable catalog icon identity for a command.
pub fn icon(command: &str) -> Option<IconId> {
    let normalized = command.trim().trim_start_matches('\'');
    // Exact variants (for example `ZOOM EXTENTS`) must win over the base
    // descriptor (`ZOOM`).
    if let Some(icon) = catalog::variant_icon(command) {
        return Some(icon);
    }
    // A command line with arguments must not inherit the base command's icon.
    // For example, `DRAWORDER F` is a submenu action, not the Draw Order menu.
    if normalized.split_whitespace().count() > 1 {
        return None;
    }
    catalog::descriptor(normalized).and_then(|descriptor| descriptor.icon)
}

/// Command icon used by ribbon buttons. Built-ins obey the catalog's explicit
/// icon decision; only external commands retain their supplied fallback.
pub fn ribbon_command_icon(command: &str, external_fallback: IconKind) -> IconKind {
    match catalog::descriptor(command) {
        Some(descriptor) if descriptor.metadata_quality == MetadataQuality::Authored => {
            icon(command)
                .map(|id| IconKind::Svg(icon_catalog::bytes(id)))
                .unwrap_or(IconKind::Glyph(""))
        }
        _ => external_fallback,
    }
}

/// Icon for a ribbon dropdown/container. The container may own artwork even
/// when its currently selected subcommand intentionally has no command icon.
pub fn ribbon_menu_icon(command: &str, container_icon: IconKind) -> IconKind {
    icon(command)
        .map(|id| IconKind::Svg(icon_catalog::bytes(id)))
        .unwrap_or(container_icon)
}

/// Tooltip text shared by ribbon and other compact command controls.
pub fn tooltip(command: &str, surface_label: &str) -> String {
    let label = label(command, surface_label);
    let id = canonical_id(command).unwrap_or_else(|| {
        command
            .trim()
            .trim_start_matches('\'')
            .split_whitespace()
            .next()
            .unwrap_or(command)
    });
    match description(command) {
        Some(description) => format!("{label}\n{description}\n{} {id}", crate::t!("Command:")),
        None => format!("{label}\n{} {id}", crate::t!("Command:")),
    }
}

/// Text used by command search in addition to the invariant command ID.
pub fn searchable_text(command: &str) -> String {
    let mut text = label(command, "").to_uppercase();
    if let Some(description) = description(command) {
        text.push(' ');
        text.push_str(&description.to_uppercase());
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_metadata_wins_over_surface_fallbacks() {
        assert_eq!(
            label("MOVE", "Legacy move label"),
            catalog::descriptor("MOVE")
                .expect("MOVE descriptor")
                .translated_label()
        );
        assert_eq!(canonical_id("delete"), Some("ERASE"));
        assert_eq!(icon("ARC_SEA"), Some(IconId::ArcStartEndAngle));
        assert_eq!(icon("ZOOM EXTENTS"), Some(IconId::ZoomExtents));
    }

    #[test]
    fn derived_commands_preserve_descriptive_surface_labels() {
        assert_eq!(label("SPLINE", "Spline Fit"), "Spline Fit");
    }

    #[test]
    fn catalog_does_not_invent_descriptions() {
        assert_eq!(description("DATAEXTRACTION"), None);
    }

    #[test]
    fn submenu_commands_do_not_reuse_parent_icons() {
        for command in [
            "ISOLATEOBJECTS",
            "HIDEOBJECTS",
            "UNISOLATEOBJECTS",
            "DRAWORDER F",
            "DRAWORDER B",
            "DRAWORDER_FRONT",
            "DRAWORDER_BACK",
            "DRAWORDER_ABOVE",
            "DRAWORDER_UNDER",
        ] {
            assert_eq!(icon(command), None, "{command}");
        }
        assert_eq!(icon("DRAWORDER"), Some(IconId::DrawOrder));
    }

    #[test]
    fn builtin_ribbon_commands_ignore_surface_fallback_icons() {
        assert!(matches!(
            ribbon_command_icon("MOVE", IconKind::Glyph("wrong")),
            IconKind::Svg(bytes) if bytes == icon_catalog::bytes(IconId::Move)
        ));
        assert!(matches!(
            ribbon_command_icon("DRAWORDER_FRONT", IconKind::Glyph("wrong")),
            IconKind::Glyph("")
        ));
        assert!(matches!(
            ribbon_menu_icon("DRAWORDER_FRONT", IconKind::Glyph("menu")),
            IconKind::Glyph("menu")
        ));
        assert!(matches!(
            ribbon_command_icon("EXTERNAL_PLUGIN_COMMAND", IconKind::Glyph("plugin")),
            IconKind::Glyph("plugin")
        ));
    }

    #[test]
    fn invocation_variants_have_catalog_labels_and_icons() {
        assert_eq!(
            label("VIEW FRONT", "wrong"),
            crate::i18n::translate("Front")
        );
        assert!(icon("VIEW FRONT").is_some());
        assert_eq!(
            label("ZOOM EXTENTS", "wrong"),
            crate::i18n::translate("Zoom Extents")
        );
        assert_eq!(icon("ZOOM EXTENTS"), Some(IconId::ZoomExtents));
    }

    #[test]
    fn tooltip_contains_description_and_canonical_command() {
        let tip = tooltip("POLYLINE", "Polyline");
        assert!(tip.contains("connected line"));
        assert!(tip.contains("PLINE"));
    }
}
