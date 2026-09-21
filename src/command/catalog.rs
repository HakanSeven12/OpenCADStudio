//! User-facing command metadata.
//!
//! Execution remains owned by [`super::CommandRegistration`]. This catalog
//! supplies presentation metadata to command-driven UI surfaces without
//! changing dispatch. Every registered command receives a derived fallback;
//! authored entries progressively replace those fallbacks.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::OnceLock;

use crate::ui::icon_catalog::{self, IconId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CommandCategory {
    Draw,
    Modify,
    Clipboard,
    Selection,
    Layers,
    Annotate,
    Insert,
    View,
    Parametric,
    Model,
    Manage,
    File,
    Utility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataQuality {
    Authored,
    Derived,
}

/// Static metadata which command modules can submit beside an implementation.
#[derive(Clone, Copy)]
pub struct CommandMetadata {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub label: &'static str,
    pub description: Option<&'static str>,
    pub icon: Option<IconId>,
    pub category: CommandCategory,
}

pub struct CommandMetadataRegistration {
    pub metadata: CommandMetadata,
}

inventory::collect!(CommandMetadataRegistration);

/// Register authored command metadata without coupling it to a UI surface.
#[macro_export]
macro_rules! register_command_metadata {
    (
        id: $id:literal,
        aliases: [$($alias:literal),* $(,)?],
        label: $label:literal,
        description: $description:expr,
        icon: $icon:expr,
        category: $category:ident $(,)?
    ) => {
        inventory::submit! {
            $crate::command::catalog::CommandMetadataRegistration {
                metadata: $crate::command::catalog::CommandMetadata {
                    id: $id,
                    aliases: &[$($alias),*],
                    label: $label,
                    description: $description,
                    icon: $icon,
                    category: $crate::command::catalog::CommandCategory::$category,
                },
            }
        }
    };
}

#[derive(Clone, Debug)]
pub struct CommandDescriptor {
    pub id: &'static str,
    pub aliases: Vec<&'static str>,
    pub label_source: String,
    pub description_source: Option<&'static str>,
    pub icon: Option<IconId>,
    pub category: CommandCategory,
    pub metadata_quality: MetadataQuality,
}

impl CommandDescriptor {
    pub fn translated_label(&self) -> Cow<'static, str> {
        crate::i18n::translate(&self.label_source)
    }

    pub fn translated_description(&self) -> Option<Cow<'static, str>> {
        self.description_source.map(crate::i18n::translate)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    pub canonical_commands: usize,
    pub authored_descriptors: usize,
    pub derived_descriptors: usize,
    pub with_descriptions: usize,
    pub with_icons: usize,
    pub aliases: usize,
    pub validation_errors: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoverageReport {
    pub summary: Coverage,
    pub derived_commands: Vec<&'static str>,
    pub missing_descriptions: Vec<&'static str>,
    pub missing_icons: Vec<&'static str>,
}

impl CoverageReport {
    pub fn render_text(&self) -> String {
        fn line(label: &str, commands: &[&str]) -> String {
            if commands.is_empty() {
                format!("{label}: none\n")
            } else {
                format!("{label} ({}): {}\n", commands.len(), commands.join(", "))
            }
        }

        let mut output = format!(
            "Command metadata coverage\n\
             canonical: {}\n\
             authored: {}\n\
             derived: {}\n\
             descriptions: {}\n\
             icons: {}\n\
             aliases: {}\n\
             validation errors: {}\n",
            self.summary.canonical_commands,
            self.summary.authored_descriptors,
            self.summary.derived_descriptors,
            self.summary.with_descriptions,
            self.summary.with_icons,
            self.summary.aliases,
            self.summary.validation_errors,
        );
        output.push_str(&line("Derived commands", &self.derived_commands));
        output.push_str(&line(
            "Commands without descriptions",
            &self.missing_descriptions,
        ));
        output.push_str(&line("Commands without icons", &self.missing_icons));
        output
    }
}

pub struct CommandCatalog {
    descriptors: BTreeMap<&'static str, CommandDescriptor>,
    canonical_ids: HashMap<String, &'static str>,
    aliases: HashMap<String, &'static str>,
    validation_errors: Vec<String>,
}

impl CommandCatalog {
    fn build() -> Self {
        let mut descriptors = BTreeMap::new();
        for id in crate::command::all_registered_command_names()
            .into_iter()
            .chain(APP_COMMANDS.iter().copied())
        {
            descriptors
                .entry(id)
                .or_insert_with(|| derived_descriptor(id));
        }

        apply_builtin_ribbon_metadata(&mut descriptors);

        let mut authored = Vec::from(CORE_METADATA);
        authored.extend(
            inventory::iter::<CommandMetadataRegistration>
                .into_iter()
                .map(|r| r.metadata),
        );

        let mut validation_errors = Vec::new();
        let mut authored_ids = HashSet::new();
        for metadata in authored {
            if !authored_ids.insert(metadata.id.to_ascii_uppercase()) {
                validation_errors.push(format!("duplicate command metadata: {}", metadata.id));
                continue;
            }
            descriptors.insert(
                metadata.id,
                CommandDescriptor {
                    id: metadata.id,
                    aliases: metadata.aliases.to_vec(),
                    label_source: metadata.label.to_string(),
                    description_source: metadata.description,
                    icon: metadata.icon,
                    category: metadata.category,
                    metadata_quality: MetadataQuality::Authored,
                },
            );
        }

        let mut aliases = HashMap::new();
        let canonical_upper: HashMap<String, &'static str> = descriptors
            .keys()
            .map(|id| (id.to_ascii_uppercase(), *id))
            .collect();
        let authored_aliases: Vec<(&'static str, &'static str)> = descriptors
            .values()
            .filter(|d| d.metadata_quality == MetadataQuality::Authored)
            .flat_map(|d| d.aliases.iter().copied().map(move |alias| (alias, d.id)))
            .collect();
        for (alias, target) in authored_aliases {
            let upper = alias.to_ascii_uppercase();
            if let Some(existing) = canonical_upper.get(&upper) {
                if *existing != target {
                    if let Some(existing_descriptor) = descriptors.get(existing) {
                        if existing_descriptor.metadata_quality == MetadataQuality::Derived {
                            descriptors.remove(existing);
                        } else {
                            validation_errors.push(format!(
                                "alias {alias} for {target} shadows canonical command {existing}"
                            ));
                            continue;
                        }
                    }
                }
            }
            if let Some(previous) = aliases.insert(upper, target) {
                if previous != target {
                    validation_errors.push(format!(
                        "alias {alias} maps to both {previous} and {target}"
                    ));
                }
            }
        }

        let canonical_ids = descriptors
            .keys()
            .map(|id| (id.to_ascii_uppercase(), *id))
            .collect();

        Self {
            descriptors,
            canonical_ids,
            aliases,
            validation_errors,
        }
    }

    pub fn descriptor(&self, command: &str) -> Option<&CommandDescriptor> {
        let normalized = normalize_command(command).to_ascii_uppercase();
        if let Some(id) = self.canonical_ids.get(&normalized) {
            return self.descriptors.get(id);
        }
        if let Some(target) = self.aliases.get(&normalized) {
            return self.descriptors.get(target);
        }
        let base = normalized.split_whitespace().next()?;
        self.canonical_ids
            .get(base)
            .and_then(|id| self.descriptors.get(id))
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.descriptors.values()
    }

    pub fn validation_errors(&self) -> &[String] {
        &self.validation_errors
    }

    pub fn coverage(&self) -> Coverage {
        let authored_descriptors = self
            .descriptors
            .values()
            .filter(|d| d.metadata_quality == MetadataQuality::Authored)
            .count();
        Coverage {
            canonical_commands: self.descriptors.len(),
            authored_descriptors,
            derived_descriptors: self.descriptors.len() - authored_descriptors,
            with_descriptions: self
                .descriptors
                .values()
                .filter(|d| d.description_source.is_some())
                .count(),
            with_icons: self
                .descriptors
                .values()
                .filter(|d| d.icon.is_some())
                .count(),
            aliases: self.aliases.len(),
            validation_errors: self.validation_errors.len(),
        }
    }

    pub fn coverage_report(&self) -> CoverageReport {
        let mut derived_commands = Vec::new();
        let mut missing_descriptions = Vec::new();
        let mut missing_icons = Vec::new();
        for descriptor in self.descriptors.values() {
            if descriptor.metadata_quality == MetadataQuality::Derived {
                derived_commands.push(descriptor.id);
            }
            if descriptor.description_source.is_none() {
                missing_descriptions.push(descriptor.id);
            }
            if descriptor.icon.is_none() {
                missing_icons.push(descriptor.id);
            }
        }
        CoverageReport {
            summary: self.coverage(),
            derived_commands,
            missing_descriptions,
            missing_icons,
        }
    }
}

static CATALOG: OnceLock<CommandCatalog> = OnceLock::new();

pub fn all() -> &'static CommandCatalog {
    CATALOG.get_or_init(CommandCatalog::build)
}

pub fn descriptor(command: &str) -> Option<&'static CommandDescriptor> {
    all().descriptor(command)
}

pub fn canonical_id(command: &str) -> Option<&'static str> {
    descriptor(command).map(|d| d.id)
}

pub fn coverage_report() -> CoverageReport {
    all().coverage_report()
}

fn normalize_command(command: &str) -> &str {
    command.trim().trim_start_matches('\'')
}

fn humanize_command_id(id: &str) -> String {
    id.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let lower = part.to_ascii_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn derived_descriptor(id: &'static str) -> CommandDescriptor {
    CommandDescriptor {
        id,
        aliases: Vec::new(),
        label_source: humanize_command_id(id),
        description_source: None,
        icon: icon_catalog::command_icon(id),
        category: CommandCategory::Utility,
        metadata_quality: MetadataQuality::Derived,
    }
}

/// Promote the labels, categories and SVGs already authored for built-in
/// ribbon tools into catalog-owned descriptors. Core metadata below can still
/// override these values with a more specific description or shared IconId.
fn apply_builtin_ribbon_metadata(descriptors: &mut BTreeMap<&'static str, CommandDescriptor>) {
    use crate::modules::{IconKind, ModuleEvent, RibbonItem, ToolDef};

    fn category(module: &str) -> CommandCategory {
        match module {
            "draw" => CommandCategory::Draw,
            "modify" => CommandCategory::Modify,
            "parametric" => CommandCategory::Parametric,
            "model" => CommandCategory::Model,
            "insert" => CommandCategory::Insert,
            "annotate" => CommandCategory::Annotate,
            "view" | "layout" => CommandCategory::View,
            "manage" => CommandCategory::Manage,
            _ => CommandCategory::Utility,
        }
    }

    fn ribbon_category(module: &str, group: &str) -> CommandCategory {
        match group {
            "Modify" => CommandCategory::Modify,
            "Annotation" | "Text" | "Dimensions" | "Centerlines" | "Leaders" | "Tables"
            | "Markup" | "Annotation Scaling" => CommandCategory::Annotate,
            "Layers" => CommandCategory::Layers,
            "Block" | "Reference" | "Point Cloud" | "Attributes" | "Import" | "Content" => {
                CommandCategory::Insert
            }
            "Clipboard" => CommandCategory::Clipboard,
            "Viewport" | "Viewport Tools" | "Navigate" | "Model Viewports" | "Visual Style"
            | "Projection" | "Preset" | "Palettes" | "Interface" => CommandCategory::View,
            "Plot" => CommandCategory::File,
            "Properties" | "Groups" | "Measure" | "Customization" | "Cleanup" | "Application"
            | "Manage" => CommandCategory::Manage,
            _ => category(module),
        }
    }

    fn canonical_surface_id(
        descriptors: &BTreeMap<&'static str, CommandDescriptor>,
        command: &str,
        fallback: &'static str,
    ) -> &'static str {
        let base = normalize_command(command)
            .split_whitespace()
            .next()
            .unwrap_or(fallback);
        if let Some(metadata) = CORE_METADATA.iter().find(|metadata| {
            metadata.id.eq_ignore_ascii_case(base)
                || metadata
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(base))
        }) {
            return metadata.id;
        }
        descriptors
            .keys()
            .find(|id| id.eq_ignore_ascii_case(base))
            .copied()
            .unwrap_or(fallback)
    }

    fn promote(
        descriptors: &mut BTreeMap<&'static str, CommandDescriptor>,
        command: &str,
        fallback_id: &'static str,
        label: &'static str,
        icon: IconKind,
        category: CommandCategory,
    ) {
        let id = canonical_surface_id(descriptors, command, fallback_id);
        let icon = match icon {
            IconKind::Svg(svg) => {
                icon_catalog::command_icon(command).or(Some(IconId::Ribbon { command: id, svg }))
            }
            IconKind::Glyph(_) => icon_catalog::command_icon(command),
        };
        let descriptor = descriptors
            .entry(id)
            .or_insert_with(|| derived_descriptor(id));
        descriptor.label_source = label.to_string();
        descriptor.icon = icon;
        descriptor.category = category;
        descriptor.metadata_quality = MetadataQuality::Authored;
    }

    fn promote_tool(
        descriptors: &mut BTreeMap<&'static str, CommandDescriptor>,
        tool: &ToolDef,
        category: CommandCategory,
    ) {
        if let ModuleEvent::Command(command) = &tool.event {
            promote(
                descriptors,
                command,
                tool.id,
                tool.label,
                tool.icon,
                category,
            );
        }
    }

    for module in crate::modules::registry::all_modules() {
        for group in module.ribbon_groups() {
            let category = ribbon_category(module.id(), group.title);
            for item in &group.tools {
                match item {
                    RibbonItem::Tool(tool)
                    | RibbonItem::LabeledTool(tool)
                    | RibbonItem::LargeTool(tool) => {
                        promote_tool(descriptors, tool, category);
                    }
                    RibbonItem::Dropdown { items, .. }
                    | RibbonItem::LabeledDropdown { items, .. }
                    | RibbonItem::LargeDropdown { items, .. } => {
                        for (command, label, icon) in items {
                            promote(descriptors, command, command, label, *icon, category);
                        }
                    }
                    RibbonItem::ToolGrid { columns }
                    | RibbonItem::StyleComboGroup { rows: columns, .. } => {
                        for tool in columns.iter().flatten() {
                            promote_tool(descriptors, tool, category);
                        }
                    }
                    RibbonItem::LayerComboGroup { row2, row3 } => {
                        for tool in row2.iter().chain(row3) {
                            promote_tool(descriptors, tool, category);
                        }
                    }
                    RibbonItem::PropertiesGroup { match_prop } => {
                        promote_tool(descriptors, match_prop, category);
                    }
                }
            }
        }
    }

    for (command, label, svg, owner) in crate::ui::ribbon::builtin_extension_command_presentations()
    {
        promote(
            descriptors,
            command,
            command,
            label,
            IconKind::Svg(svg),
            category(owner),
        );
    }
}

// User-facing one-shot actions which are not backed by an interactive
// CadCommand registration yet. Keeping them here gives every command-driven
// surface the same presentation metadata without changing dispatch.
const APP_COMMANDS: &[&str] = &[
    // File and application actions.
    "NEW",
    "OPEN",
    "SAVE",
    "SAVEAS",
    "QSAVE",
    "PRINT",
    "PLOT",
    "QUIT",
    "HELP",
    "FIND",
    "CANCEL",
    // View, input and selection actions handled directly by the application.
    "UNDO",
    "REDO",
    "PROPERTIES",
    "OPTIONS",
    "PLAN",
    "ZOOM",
    "SPACEMOUSEFIT",
    "SPACEMOUSETOP",
    "COMMANDHISTORY",
    "TOGGLEOSNAP",
    "TOGGLE3DOSNAP",
    "ISOPLANE",
    "GRID",
    "ORTHO",
    "SNAP",
    "POLAR",
    "OTRACK",
    "DYNINPUT",
    "CLEANSCREEN",
    "SELECTALL",
    // Ribbon actions which currently have no interactive CadCommand registration.
    "PARALLEL",
    "POINTCLOUDATTACH",
    "UNDERLAYLAYERS",
    "UOSNAP",
    "XCLIP",
];

const CORE_METADATA: &[CommandMetadata] = &[
    meta(
        "NEW",
        &[],
        "New",
        Some("Creates a new drawing."),
        Some(IconId::New),
        CommandCategory::File,
    ),
    meta(
        "OPEN",
        &[],
        "Open",
        Some("Opens an existing drawing."),
        Some(IconId::Open),
        CommandCategory::File,
    ),
    meta(
        "SAVE",
        &[],
        "Save",
        Some("Saves the current drawing."),
        Some(IconId::Save),
        CommandCategory::File,
    ),
    meta(
        "SAVEAS",
        &[],
        "Save As",
        Some("Saves the current drawing under a new name or format."),
        Some(IconId::SaveAs),
        CommandCategory::File,
    ),
    meta(
        "QSAVE",
        &[],
        "Quick Save",
        Some("Saves the current drawing using its existing name."),
        Some(IconId::Save),
        CommandCategory::File,
    ),
    meta(
        "PRINT",
        &[],
        "Print",
        Some("Prints or plots the current drawing."),
        Some(IconId::Print),
        CommandCategory::File,
    ),
    meta(
        "PLOT",
        &[],
        "Plot",
        Some("Opens the drawing plot workflow."),
        Some(IconId::Print),
        CommandCategory::File,
    ),
    meta(
        "LINE",
        &[],
        "Line",
        Some("Creates straight line segments."),
        Some(IconId::Line),
        CommandCategory::Draw,
    ),
    meta(
        "PLINE",
        &["POLYLINE"],
        "Polyline",
        Some("Creates connected line and arc segments as one polyline."),
        Some(IconId::Polyline),
        CommandCategory::Draw,
    ),
    meta(
        "CIRCLE",
        &[],
        "Circle",
        Some("Creates a circle using a center point and radius."),
        Some(IconId::Circle),
        CommandCategory::Draw,
    ),
    meta(
        "ARC",
        &[],
        "Arc",
        Some("Creates an arc using three points."),
        Some(IconId::Arc3Point),
        CommandCategory::Draw,
    ),
    meta(
        "RECT",
        &["RECTANG"],
        "Rectangle - Two Corners",
        None,
        Some(IconId::Rectangle),
        CommandCategory::Draw,
    ),
    meta(
        "MOVE",
        &[],
        "Move",
        Some("Moves selected objects by a displacement."),
        Some(IconId::Move),
        CommandCategory::Modify,
    ),
    meta(
        "COPY",
        &[],
        "Copy",
        Some("Copies selected objects."),
        Some(IconId::Copy),
        CommandCategory::Modify,
    ),
    meta(
        "ROTATE",
        &[],
        "Rotate",
        Some("Rotates selected objects around a base point."),
        Some(IconId::Rotate),
        CommandCategory::Modify,
    ),
    meta(
        "SCALE",
        &[],
        "Scale",
        Some("Scales selected objects around a base point."),
        Some(IconId::Scale),
        CommandCategory::Modify,
    ),
    meta(
        "MIRROR",
        &[],
        "Mirror",
        Some("Creates a mirrored copy of selected objects."),
        Some(IconId::Mirror),
        CommandCategory::Modify,
    ),
    meta(
        "STRETCH",
        &[],
        "Stretch",
        Some("Stretches vertices inside a crossing selection."),
        Some(IconId::Stretch),
        CommandCategory::Modify,
    ),
    meta(
        "ERASE",
        &["DELETE"],
        "Erase",
        Some("Removes selected objects from the drawing."),
        Some(IconId::Erase),
        CommandCategory::Modify,
    ),
    meta(
        "CUTCLIP",
        &[],
        "Cut",
        Some("Cuts selected objects to the clipboard."),
        Some(IconId::Cut),
        CommandCategory::Clipboard,
    ),
    meta(
        "COPYCLIP",
        &[],
        "Copy",
        Some("Copies selected objects to the clipboard."),
        Some(IconId::CopyClipboard),
        CommandCategory::Clipboard,
    ),
    meta(
        "COPYBASE",
        &[],
        "Copy with Base Point",
        Some("Copies selected objects using a specified base point."),
        Some(IconId::CopyClipboard),
        CommandCategory::Clipboard,
    ),
    meta(
        "PASTECLIP",
        &["PASTE"],
        "Paste",
        Some("Pastes objects from the clipboard."),
        Some(IconId::Paste),
        CommandCategory::Clipboard,
    ),
    meta(
        "PASTEBLOCK",
        &[],
        "Paste as Block",
        Some("Pastes clipboard objects as a block."),
        Some(IconId::Paste),
        CommandCategory::Clipboard,
    ),
    meta(
        "PASTEORIG",
        &[],
        "Paste to Original Coordinates",
        Some("Pastes clipboard objects at their original coordinates."),
        Some(IconId::Paste),
        CommandCategory::Clipboard,
    ),
    meta(
        "DRAWORDER",
        &[],
        "Draw Order",
        Some("Changes the display order of selected objects."),
        Some(IconId::DrawOrder),
        CommandCategory::Modify,
    ),
    meta(
        "UNDO",
        &[],
        "Undo",
        Some("Reverses the most recent operation."),
        Some(IconId::Undo),
        CommandCategory::Utility,
    ),
    meta(
        "REDO",
        &[],
        "Redo",
        Some("Restores the most recently undone operation."),
        Some(IconId::Redo),
        CommandCategory::Utility,
    ),
    meta(
        "PAN",
        &[],
        "Pan",
        Some("Moves the view without changing its scale."),
        Some(IconId::Pan),
        CommandCategory::View,
    ),
    meta(
        "ZOOM",
        &[],
        "Zoom",
        Some("Changes the magnification of the current view."),
        Some(IconId::Zoom),
        CommandCategory::View,
    ),
    meta(
        "PROPERTIES",
        &[],
        "Properties",
        Some("Shows or hides the Properties panel."),
        Some(IconId::Properties),
        CommandCategory::View,
    ),
    meta(
        "OPTIONS",
        &[],
        "Options",
        Some("Opens application settings."),
        Some(IconId::Options),
        CommandCategory::Manage,
    ),
    meta(
        "ISOLATEOBJECTS",
        &[],
        "Isolate Objects",
        Some("Temporarily hides all objects except the selection."),
        None,
        CommandCategory::Selection,
    ),
    meta(
        "HIDEOBJECTS",
        &[],
        "Hide Objects",
        Some("Temporarily hides selected objects."),
        None,
        CommandCategory::Selection,
    ),
    meta(
        "UNISOLATEOBJECTS",
        &[],
        "End Object Isolation",
        Some("Restores temporarily hidden objects."),
        None,
        CommandCategory::Selection,
    ),
    meta(
        "SELECTALL",
        &[],
        "Select All",
        Some("Selects all selectable objects in the current drawing."),
        None,
        CommandCategory::Selection,
    ),
];

const fn meta(
    id: &'static str,
    aliases: &'static [&'static str],
    label: &'static str,
    description: Option<&'static str>,
    icon: Option<IconId>,
    category: CommandCategory,
) -> CommandMetadata {
    CommandMetadata {
        id,
        aliases,
        label,
        description,
        icon,
        category,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_command_has_a_descriptor() {
        let catalog = all();
        for name in crate::command::all_registered_command_names() {
            assert!(
                catalog.descriptor(name).is_some(),
                "missing descriptor for {name}"
            );
        }
    }

    #[test]
    fn catalog_has_no_identity_or_alias_collisions() {
        assert_eq!(all().validation_errors(), &[] as &[String]);
    }

    #[test]
    fn authored_aliases_resolve_to_the_canonical_command() {
        assert_eq!(canonical_id("POLYLINE"), Some("PLINE"));
        assert_eq!(canonical_id("RECTANG"), Some("RECT"));
        assert_eq!(canonical_id("delete"), Some("ERASE"));
        assert_eq!(canonical_id("'PAN"), Some("PAN"));
        assert_eq!(canonical_id("ZOOM EXTENTS"), Some("ZOOM"));
    }

    #[test]
    fn canonical_lookup_is_case_insensitive() {
        assert_eq!(canonical_id("line"), Some("LINE"));
        assert_eq!(canonical_id("  zoom extents  "), Some("ZOOM"));
    }

    #[test]
    fn descriptors_translate_at_read_time_and_icons_resolve() {
        let move_command = descriptor("MOVE").expect("MOVE descriptor");
        assert!(!move_command.translated_label().is_empty());
        assert!(!move_command.translated_description().unwrap().is_empty());
        assert!(!icon_catalog::bytes(move_command.icon.unwrap()).is_empty());
    }

    #[test]
    fn coverage_accounts_for_every_descriptor() {
        let coverage = all().coverage();
        assert_eq!(
            coverage.canonical_commands,
            coverage.authored_descriptors + coverage.derived_descriptors
        );
        assert_eq!(coverage.validation_errors, 0);
        assert!(coverage.with_icons > 0);
        assert!(coverage.with_descriptions > 0);
    }

    #[test]
    fn every_descriptor_has_a_non_empty_label() {
        for command in all().descriptors() {
            assert!(
                !command.label_source.trim().is_empty(),
                "empty label for {}",
                command.id
            );
        }
    }

    #[test]
    fn descriptor_icons_follow_the_shared_icon_catalog() {
        for command in all().descriptors() {
            if let Some(icon) = icon_catalog::command_icon(command.id) {
                assert_eq!(command.icon, Some(icon), "icon mismatch for {}", command.id);
            }
        }
    }

    #[test]
    fn every_ribbon_command_resolves_to_a_descriptor() {
        use crate::modules::{ModuleEvent, RibbonItem};
        let mut missing = Vec::new();
        let mut derived = Vec::new();
        let mut missing_icons = Vec::new();
        let mut check = |command: &str| match descriptor(command) {
            None => missing.push(command.to_string()),
            Some(descriptor) => {
                if descriptor.metadata_quality == MetadataQuality::Derived {
                    derived.push(command.to_string());
                }
                if crate::ui::command_presentation::icon(command)
                    .is_none_or(|icon| crate::ui::icon_catalog::bytes(icon).is_empty())
                {
                    missing_icons.push(command.to_string());
                }
            }
        };
        for module in crate::modules::registry::all_modules() {
            for group in module.ribbon_groups() {
                for item in &group.tools {
                    match item {
                        RibbonItem::Tool(tool)
                        | RibbonItem::LabeledTool(tool)
                        | RibbonItem::LargeTool(tool) => {
                            if let ModuleEvent::Command(command) = &tool.event {
                                check(command);
                            }
                        }
                        RibbonItem::Dropdown { items, .. }
                        | RibbonItem::LabeledDropdown { items, .. }
                        | RibbonItem::LargeDropdown { items, .. } => {
                            for (command, _, _) in items {
                                check(command);
                            }
                        }
                        RibbonItem::ToolGrid { columns }
                        | RibbonItem::StyleComboGroup { rows: columns, .. } => {
                            for tool in columns.iter().flatten() {
                                if let ModuleEvent::Command(command) = &tool.event {
                                    check(command);
                                }
                            }
                        }
                        RibbonItem::LayerComboGroup { row2, row3 } => {
                            for tool in row2.iter().chain(row3) {
                                if let ModuleEvent::Command(command) = &tool.event {
                                    check(command);
                                }
                            }
                        }
                        RibbonItem::PropertiesGroup { match_prop } => {
                            if let ModuleEvent::Command(command) = &match_prop.event {
                                check(command);
                            }
                        }
                    }
                }
            }
        }
        missing.sort();
        missing.dedup();
        derived.sort();
        derived.dedup();
        missing_icons.sort();
        missing_icons.dedup();
        assert!(
            missing.is_empty(),
            "ribbon commands missing descriptors: {missing:?}"
        );
        assert!(
            derived.is_empty(),
            "built-in ribbon commands still using derived metadata: {derived:?}"
        );
        assert!(
            missing_icons.is_empty(),
            "built-in ribbon commands missing icons: {missing_icons:?}"
        );
    }

    #[test]
    fn coverage_report_lists_every_remaining_fallback() {
        let report = coverage_report();
        assert_eq!(
            report.derived_commands.len(),
            report.summary.derived_descriptors
        );
        assert!(report
            .derived_commands
            .windows(2)
            .all(|ids| ids[0] <= ids[1]));
        assert!(report.render_text().contains("Command metadata coverage"));
    }

    #[test]
    fn every_builtin_extension_command_has_authored_metadata_and_category() {
        for (command, _, _, _) in crate::ui::ribbon::builtin_extension_command_presentations() {
            let descriptor = descriptor(command)
                .unwrap_or_else(|| panic!("extension command {command} has no descriptor"));
            assert_eq!(
                descriptor.metadata_quality,
                MetadataQuality::Authored,
                "{command} still uses derived metadata"
            );
            assert_ne!(
                descriptor.category,
                CommandCategory::Utility,
                "{command} has no extension-panel category"
            );
        }
    }
}
