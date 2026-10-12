//! Where the ribbon offers a command, so a command started from the command
//! line, an alias, a shortcut or Repeat lights the same button a click on it
//! would. Seeing which button runs a typed command is how its place in the
//! ribbon is learned.

use crate::modules::{ModuleEvent, RibbonGroup, RibbonItem, ToolDef};

use super::{draw_panel, item_id, Ribbon};

/// One button of a group: the id its highlight is keyed on, and the command
/// line it dispatches (`None` for a button that raises a non-command event).
type Entry<'a> = (&'static str, Option<&'a str>);

fn tool_entry(t: &ToolDef) -> Entry<'_> {
    match &t.event {
        ModuleEvent::Command(cmd) => (t.id, Some(cmd.as_str())),
        _ => (t.id, None),
    }
}

/// The buttons one ribbon item shows: itself, the items of a dropdown, or the
/// small tools under a combo. A dropdown item is keyed on the command it
/// dispatches, which is what the dropdown's highlight looks for.
fn item_entries(item: &RibbonItem) -> Vec<Entry<'_>> {
    match item {
        RibbonItem::Tool(t) | RibbonItem::LabeledTool(t) | RibbonItem::LargeTool(t) => {
            vec![tool_entry(t)]
        }
        RibbonItem::PropertiesGroup { match_prop } => vec![tool_entry(match_prop)],
        RibbonItem::Dropdown { items, .. }
        | RibbonItem::LabeledDropdown { items, .. }
        | RibbonItem::LargeDropdown { items, .. } => {
            items.iter().map(|(cmd, _, _)| (*cmd, Some(*cmd))).collect()
        }
        RibbonItem::ToolGrid { columns } | RibbonItem::StyleComboGroup { rows: columns, .. } => {
            columns.iter().flatten().map(tool_entry).collect()
        }
        RibbonItem::LayerComboGroup { row2, row3 } => {
            row2.iter().chain(row3).map(tool_entry).collect()
        }
    }
}

/// Every button `group` offers: its items' buttons and the tools of its title
/// slide-out, keyed like dropdown items on the command they dispatch.
pub(super) fn group_entries(group: &RibbonGroup) -> Vec<Entry<'_>> {
    let mut out: Vec<_> = group.tools.iter().flat_map(item_entries).collect();
    let ids: Vec<_> = group.tools.iter().filter_map(item_id).collect();
    out.extend(draw_panel::slide_out_commands(group.title, &ids).map(|cmd| (cmd, Some(cmd))));
    out
}

/// Whether `item` shows the button keyed `id`, lit or as the item a lit
/// dropdown holds.
pub(super) fn item_holds(item: &RibbonItem, id: &str) -> bool {
    item_entries(item).iter().any(|(entry, _)| *entry == id)
}

/// Whether `group` holds the button keyed `id`.
pub(super) fn group_holds(group: &RibbonGroup, id: &str) -> bool {
    group_entries(group).iter().any(|(entry, _)| *entry == id)
}

impl Ribbon {
    /// Every button of every tab, with the index of the tab it is on. The
    /// tab on show comes first, so a command offered on two tabs lights the
    /// button the user can already see.
    fn entries(&self) -> impl Iterator<Item = (usize, Entry<'_>)> {
        let order = std::iter::once(self.active)
            .chain((0..self.modules.len()).filter(move |i| *i != self.active));
        order
            .filter_map(|i| self.modules.get(i).map(|m| (i, m)))
            .flat_map(|(i, module)| {
                module
                    .ribbon_groups()
                    .iter()
                    .flat_map(group_entries)
                    .map(move |entry| (i, entry))
            })
    }

    /// The button id that runs the command line `cmd`: an exact match first,
    /// then a button that runs its bare verb (`OFFSET 5` → Offset).
    pub(super) fn tool_for_command(&self, cmd: &str) -> Option<&'static str> {
        let cmd = cmd.trim();
        let verb = cmd.split_whitespace().next().unwrap_or(cmd);
        [cmd, verb].into_iter().find_map(|want| {
            self.entries()
                .find(|(_, (_, runs))| runs.is_some_and(|runs| runs.eq_ignore_ascii_case(want)))
                .map(|(_, (id, _))| id)
        })
    }

    /// Light the ribbon button of the command line `cmd` that is starting,
    /// however it was started. A button the user just clicked to run `cmd`
    /// stays lit, so a command offered on two tabs keeps the clicked one; a
    /// command with no button clears the previous command's highlight.
    pub fn show_command(&mut self, cmd: &str) {
        let clicked = self.active_tool.as_deref().is_some_and(|current| {
            self.entries().any(|(_, (id, runs))| {
                id == current && runs.is_some_and(|runs| runs.eq_ignore_ascii_case(cmd.trim()))
            })
        });
        if !clicked {
            self.active_tool = self.tool_for_command(cmd).map(str::to_string);
        }
    }

    /// The tab holding the lit button, when it is not the tab on show: its
    /// header lights instead, pointing at where the button lives. The Layout
    /// module has no tab (its tools are on the side toolbar).
    pub(super) fn tab_holding_active_tool(&self) -> Option<usize> {
        let id = self.active_tool.as_deref()?;
        let holds = |i: usize| {
            self.modules[i]
                .ribbon_groups()
                .iter()
                .any(|group| group_holds(group, id))
        };
        if self
            .modules
            .get(self.active)
            .is_some_and(|_| holds(self.active))
        {
            return None;
        }
        (0..self.modules.len()).find(|i| self.modules[*i].id() != "layout" && holds(*i))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::{CadModule, IconKind};

    struct TestTab {
        id: &'static str,
        groups: Vec<RibbonGroup>,
    }

    impl CadModule for TestTab {
        fn id(&self) -> &'static str {
            self.id
        }
        fn title(&self) -> &'static str {
            self.id
        }
        fn ribbon_groups(&self) -> &[RibbonGroup] {
            &self.groups
        }
    }

    fn tool(id: &'static str, cmd: &str) -> ToolDef {
        ToolDef {
            id,
            label: id,
            icon: IconKind::Glyph("x"),
            event: ModuleEvent::Command(cmd.to_string()),
        }
    }

    /// Home: OFFSET (button id differs from its command), a CIRCLE dropdown,
    /// LAYERS (a non-command event). View: ZOOM EXTENTS and a second OFFSET.
    fn ribbon() -> Ribbon {
        let mut layers = tool("LAYERS", "");
        layers.event = ModuleEvent::ToggleLayers;
        let home = TestTab {
            id: "home",
            groups: vec![RibbonGroup {
                title: "Modify",
                tools: vec![
                    RibbonItem::LargeTool(tool("OFFSET_TOOL", "OFFSET")),
                    RibbonItem::Dropdown {
                        id: "circle_dd",
                        icon: IconKind::Glyph("o"),
                        items: vec![
                            ("CIRCLE", "Center, Radius", IconKind::Glyph("o")),
                            ("CIRCLE_2P", "2-Point", IconKind::Glyph("o")),
                        ],
                        default: "CIRCLE",
                    },
                    RibbonItem::Tool(layers),
                ],
            }],
        };
        let view = TestTab {
            id: "view",
            groups: vec![RibbonGroup {
                title: "Navigate",
                tools: vec![
                    RibbonItem::Tool(tool("ZOOM_EXTENTS", "ZOOM EXTENTS")),
                    RibbonItem::Tool(tool("OFFSET_VIEW", "OFFSET")),
                ],
            }],
        };
        let mut ribbon = Ribbon::default();
        ribbon.set_modules(vec![Box::new(home), Box::new(view)]);
        ribbon
    }

    #[test]
    fn a_command_lights_the_button_that_runs_it_not_one_named_like_it() {
        let mut r = ribbon();
        r.show_command("OFFSET");
        assert_eq!(r.active_tool.as_deref(), Some("OFFSET_TOOL"));
        assert_eq!(r.tab_holding_active_tool(), None);
    }

    #[test]
    fn a_dropdown_item_is_keyed_on_its_command_so_the_dropdown_lights() {
        let mut r = ribbon();
        r.show_command("circle_2p");
        assert_eq!(r.active_tool.as_deref(), Some("CIRCLE_2P"));
    }

    #[test]
    fn a_button_on_another_tab_lights_that_tab() {
        let mut r = ribbon();
        r.show_command("ZOOM EXTENTS");
        assert_eq!(r.active_tool.as_deref(), Some("ZOOM_EXTENTS"));
        assert_eq!(r.tab_holding_active_tool(), Some(1));
        r.select(1);
        assert_eq!(r.tab_holding_active_tool(), None);
    }

    #[test]
    fn a_command_on_two_tabs_prefers_the_tab_on_show_then_the_clicked_one() {
        let mut r = ribbon();
        r.select(1);
        r.show_command("OFFSET");
        assert_eq!(r.active_tool.as_deref(), Some("OFFSET_VIEW"));
        // A clicked button keeps its highlight when its command dispatches,
        // even though another button for the same command comes first.
        r.select(0);
        r.activate_tool("OFFSET_VIEW");
        r.show_command("OFFSET");
        assert_eq!(r.active_tool.as_deref(), Some("OFFSET_VIEW"));
    }

    #[test]
    fn arguments_fall_back_to_the_bare_verb() {
        let mut r = ribbon();
        r.show_command("OFFSET 5");
        assert_eq!(r.active_tool.as_deref(), Some("OFFSET_TOOL"));
    }

    #[test]
    fn a_command_without_a_button_clears_the_previous_highlight() {
        let mut r = ribbon();
        r.show_command("OFFSET");
        r.show_command("DIST");
        assert_eq!(r.active_tool, None);
        assert_eq!(r.tab_holding_active_tool(), None);
        // A non-command button has no command line to match.
        r.show_command("LAYERS");
        assert_eq!(r.active_tool, None);
    }

    #[test]
    fn slide_out_tools_and_their_options_belong_to_their_group() {
        // The Draw group's title slide-out holds SPLINE; the Modify one holds
        // the draw-order submenu.
        let draw = RibbonGroup {
            title: "Draw",
            tools: vec![],
        };
        assert!(group_holds(&draw, "SPLINE"));
        let modify = RibbonGroup {
            title: "Modify",
            tools: vec![],
        };
        assert!(group_holds(&modify, "DRAWORDER_BACK"));
    }

    #[test]
    fn the_shipped_ribbon_resolves_its_own_commands() {
        let mut r = Ribbon::default();
        for cmd in ["LINE", "OFFSET", "SPLINE", "ZOOM EXTENTS"] {
            r.show_command(cmd);
            assert!(r.active_tool.is_some(), "{cmd} has no ribbon button");
        }
    }
}
