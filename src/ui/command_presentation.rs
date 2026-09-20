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
    // Exact variants (for example `ZOOM EXTENTS`) must win over the base
    // descriptor (`ZOOM`).
    icon_catalog::command_icon(command)
        .or_else(|| catalog::descriptor(command).and_then(|descriptor| descriptor.icon))
}

/// Ribbon icon with catalog artwork preferred over the surface fallback.
pub fn ribbon_icon(command: &str, surface_fallback: IconKind) -> IconKind {
    icon(command)
        .map(|id| IconKind::Svg(icon_catalog::bytes(id)))
        .unwrap_or(surface_fallback)
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
    fn tooltip_contains_description_and_canonical_command() {
        let tip = tooltip("POLYLINE", "Polyline");
        assert!(tip.contains("connected line"));
        assert!(tip.contains("PLINE"));
    }
}
