// Core ribbon module registry — one boxed instance of every built-in CAD
// module, in ribbon-tab display order. External add-ons are appended on top
// of this list at runtime by `plugin::registry`.
//
// To add a core module:
//   1. Create src/modules/my_name/mod.rs  (implement CadModule as MyNameModule)
//   2. Add `pub mod my_name;` to src/modules/mod.rs
//   3. Add one `Box::new(super::my_name::MyNameModule)` line below, in the
//      position you want its tab to appear.
use crate::modules::{CadModule, IconKind, ModuleEvent, RibbonItem, ToolDef};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Returns one boxed instance of every registered core CAD module.
/// Called once at startup by `Ribbon::new()`.
pub fn all_modules() -> Vec<Box<dyn CadModule>> {
    vec![
        Box::new(super::draw::DrawModule),
        Box::new(super::parametric::ParametricModule),
        Box::new(super::model::ModelModule),
        Box::new(super::insert::InsertModule),
        Box::new(super::annotate::AnnotateModule),
        Box::new(super::view::ViewModule),
        Box::new(super::manage::ManageModule),
        Box::new(super::layout::LayoutModule),
    ]
}

/// Label and icon of every core ribbon command, keyed by its upper-case
/// command line (`LINE`, `ZOOM EXTENTS`). The ribbon definition beside each
/// command stays the one source; the shortcut list, the context menus and the
/// command-line suggestions read it from here.
pub fn ribbon_commands() -> &'static HashMap<String, (&'static str, IconKind)> {
    static COMMANDS: OnceLock<HashMap<String, (&'static str, IconKind)>> = OnceLock::new();
    COMMANDS.get_or_init(|| {
        let mut out = HashMap::new();
        let tool = |out: &mut HashMap<_, _>, t: &ToolDef| {
            if let ModuleEvent::Command(command) = &t.event {
                out.insert(command.to_ascii_uppercase(), (t.label, t.icon));
            }
        };
        for module in all_modules() {
            for group in module.ribbon_groups() {
                for item in &group.tools {
                    match item {
                        RibbonItem::Tool(t)
                        | RibbonItem::LabeledTool(t)
                        | RibbonItem::LargeTool(t) => tool(&mut out, t),
                        RibbonItem::Dropdown { items, .. }
                        | RibbonItem::LabeledDropdown { items, .. }
                        | RibbonItem::LargeDropdown { items, .. } => {
                            for (command, label, icon) in items {
                                out.insert(command.to_ascii_uppercase(), (*label, *icon));
                            }
                        }
                        RibbonItem::ToolGrid { columns }
                        | RibbonItem::StyleComboGroup { rows: columns, .. } => {
                            for t in columns.iter().flatten() {
                                tool(&mut out, t);
                            }
                        }
                        RibbonItem::LayerComboGroup { row2, row3 } => {
                            for t in row2.iter().chain(row3) {
                                tool(&mut out, t);
                            }
                        }
                        RibbonItem::PropertiesGroup { match_prop } => tool(&mut out, match_prop),
                    }
                }
            }
        }
        for (command, label, icon) in crate::ui::ribbon::panel_tools() {
            out.entry(command.to_string())
                .or_insert((label, IconKind::Svg(icon)));
        }
        out
    })
}

/// The ribbon icon of a command line, for surfaces that show the command
/// outside the ribbon. A leading `'` (transparent use) is ignored.
pub fn command_icon(command: &str) -> Option<&'static [u8]> {
    let key = command.trim().trim_start_matches('\'').to_ascii_uppercase();
    match ribbon_commands().get(&key)?.1 {
        IconKind::Svg(bytes) => Some(bytes),
        IconKind::Glyph(_) => None,
    }
}
