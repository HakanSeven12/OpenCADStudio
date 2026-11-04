//! Quad cursor menu — the *model*.
//!
//! Pure sibling of `context_menu.rs` (docs/quad-menu.md §1–§3.3, §3.9):
//! `quad_panels_for_target` maps the cursor target, the most-recently-used
//! memory and the registered plugin panels to the panels the quad shows,
//! without knowing anything about iced. The view renders the panels and
//! routes picks back through `QuadMsg`; the app layer classifies the target
//! and records launches into [`QuadMru`].

use codec::Handle;
use iced::Point;
use rustc_hash::FxHashMap;

/// Columns of the expanded quad (`QUADWIDTH`), default 6, as in commercial solutions.
pub const QUAD_WIDTH_DEFAULT: u8 = 6;

/// Maximum most-recently-used commands remembered per entity type key.
pub const QUAD_MRU_CAP: usize = 16;

/// Which cursor situation the quad was opened for (docs/quad-menu.md §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuadMode {
    /// Dwell on a hovered entity (quad-on-hover flag).
    Hover,
    /// A selection set exists (quad-on-select flag).
    Selection,
    /// Nothing under the cursor: creation / recent quad.
    NoSelection,
}

/// Icon of a quad item: the command's ribbon SVG when the registry has one,
/// otherwise the glyph tile filled from a short text abbreviation of the
/// label (never a hand-invented icon; docs/quad-menu.md §3.9).
#[derive(Debug, Clone)]
pub enum QuadIcon {
    /// A command's ribbon icon (static SVG bytes), drawn in its own colours.
    Svg(&'static [u8]),
    /// A plugin's registered icon (API v15): the bytes are plugin-owned heap
    /// data converted from the wire (`OwnedIconKind::Svg`), kept alive by an
    /// `Arc` so the UI borrows without copying or leaking per registration.
    /// Rendered identically to `Svg`, plus the §4 budget fallback applied at
    /// conversion time (oversized/malformed icons never get here).
    SvgOwned(std::sync::Arc<[u8]>),
    /// Monochrome interface glyph with the fallback abbreviation text.
    Glyph(String),
}

/// One clickable cell of a quad panel. Items are always enabled (the
/// model only offers commands the target can run) and carry no extra
/// tip text: the label is the tooltip.
#[derive(Debug, Clone)]
pub struct QuadItem {
    pub label: String,
    /// The full command line the pick dispatches (`Message::Command`).
    pub command: String,
    pub icon: QuadIcon,
}

impl QuadItem {
    /// Build a quad item for `command`, resolving the label and icon through
    /// the command registry (single source of truth with ribbon + search).
    fn from_command(command: &str) -> Self {
        Self {
            label: quad_command_label(command),
            command: command.to_string(),
            icon: quad_command_icon(command),
        }
    }
}

/// A grid of quad items. Index 0 of a quad is the "primary" panel; the rest
/// open as tabs along the bottom edge. A panel's `applies_to` list controls
/// when its tab is shown; plugin panels are recognized by their namespaced
/// id prefix (`plugin:<plugin_id>:<panel_id>`) — the prefix is the
/// mechanism.
#[derive(Debug, Clone)]
pub struct QuadPanel {
    /// Stable panel id; plugin panels are namespaced `plugin:<plugin_id>:<id>`.
    pub id: String,
    /// Tab label. The primary panel is tab-less and labels itself the empty
    /// string (hover quad) or "Recent" (No Selection quad).
    pub label: String,
    pub items: Vec<QuadItem>,
    /// Entity type keys this panel applies to (`entity_type_name()` strings
    /// or plugin `class_id`s). Empty or containing `"*"` = universal.
    pub applies_to: Vec<String>,
}

/// What the cursor is over when the quad opens (docs/quad-menu.md §3.2).
#[derive(Debug, Clone, PartialEq)]
pub enum QuadTarget {
    /// Right-click in the void: the No Selection quad.
    None,
    /// A single hovered entity; `type_key` is the canonical
    /// `entity_type_name()` string ("Line", "Insert", …).
    Hovered {
        handle: Handle,
        type_key: String,
    },
    /// The current selection set, possibly of mixed types.
    Selection {
        handles: Vec<Handle>,
        type_keys: Vec<String>,
    },
}

impl QuadTarget {
    /// True for [`QuadTarget::None`] — nothing under the cursor and no selection.
    pub fn is_none(&self) -> bool {
        matches!(self, QuadTarget::None)
    }

    /// Whether a panel whose `applies_to` list is `applies_to` shows for this
    /// target. An empty list or a `"*"` entry makes the panel universal.
    ///
    /// - `None` matches only universal panels.
    /// - `Hovered` matches when its `type_key` is contained.
    /// - `Selection` matches when *any* of its `type_keys` is contained.
    pub fn matches_applies(&self, applies_to: &[String]) -> bool {
        if applies_to.is_empty() || applies_to.iter().any(|k| k == "*") {
            return true;
        }
        match self {
            QuadTarget::None => false,
            QuadTarget::Hovered { type_key, .. } => applies_to
                .iter()
                .any(|k| k.eq_ignore_ascii_case(type_key)),
            QuadTarget::Selection { type_keys, .. } => type_keys
                .iter()
                .any(|type_key| applies_to.iter().any(|k| k.eq_ignore_ascii_case(type_key))),
        }
    }
}

/// Transient quad window state next to the context-menu state in
/// `SelectionMenu` (the "selection boundary owns transient popup state"
/// convention).
#[derive(Debug, Clone)]
pub struct QuadUi {
    /// Screen position the quad is anchored at (cursor position).
    pub anchor: Point,
    pub mode: QuadMode,
    /// Collapsed = single default icon; expanded = the panel grid with tabs.
    pub expanded: bool,
    /// Index into the open quad's panel list — over ALL panels, primary =
    /// `0` (tabs carry their real panel index); a collapsed quad keeps `0`.
    pub active_tab: usize,
}

impl QuadUi {
    /// Open the quad collapsed at `anchor` for `mode`.
    pub fn open(anchor: Point, mode: QuadMode) -> Self {
        Self {
            anchor,
            mode,
            expanded: false,
            active_tab: 0,
        }
    }
}

/// What a quad interaction produces; translated into app messages by the
/// quad's pick router (mirrors `on_context_menu_pick`).
#[derive(Debug, Clone, PartialEq)]
pub enum QuadMsg {
    /// Toggle the quad on/off (F12, status-bar field).
    Toggle,
    /// Enter or leave the quad (cursor entered / left the window).
    Expand(bool),
    /// Activate the tab at `index`.
    Tab(usize),
    /// Pick an item: the panel's id and the index into its items.
    Pick {
        panel_id: String,
        item_index: usize,
    },
    /// Close the quad without picking (outside press, Esc).
    Close,
}

/// Per-entity-type-key most-recently-used command memory, plus a global list
/// for the No Selection quad. In-memory only; persistence is a later phase.
pub struct QuadMru {
    per_type: FxHashMap<String, Vec<String>>,
    global: Vec<String>,
}

impl Default for QuadMru {
    fn default() -> Self {
        Self::new()
    }
}

impl QuadMru {
    /// An empty memory.
    pub fn new() -> Self {
        Self {
            per_type: FxHashMap::default(),
            global: Vec::new(),
        }
    }

    /// Move `command` to the front of `type_key`'s list, de-duplicating and
    /// capping at [`QUAD_MRU_CAP`]. Recorded when a command is launched from
    /// a quad (the `Message::QuadRun` wrapper).
    pub fn record_for(&mut self, type_key: &str, command: &str) {
        record(&mut self.per_type.entry(type_key.to_string()).or_default(), command);
    }

    /// Move `command` to the front of the global list (No Selection quad).
    pub fn record_global(&mut self, command: &str) {
        record(&mut self.global, command);
    }

    /// The last `n` commands launched from `type_key`'s quad, newest first.
    pub fn recent_for(&self, type_key: &str, n: usize) -> Vec<String> {
        self.per_type
            .get(type_key)
            .map_or_else(Vec::new, |list| list.iter().take(n).cloned().collect())
    }

    /// The last `n` commands launched from any quad, newest first.
    pub fn recent_global(&self, n: usize) -> Vec<String> {
        self.global.iter().take(n).cloned().collect()
    }
}

/// Move-to-front, de-duplicating insert capped at [`QUAD_MRU_CAP`].
fn record(list: &mut Vec<String>, command: &str) {
    if let Some(idx) = list.iter().position(|c| c == command) {
        list.remove(idx);
    } else if list.len() >= QUAD_MRU_CAP {
        list.pop();
    }
    list.insert(0, command.to_string());
}

/// The ribbon label of `command`, the command line itself when the registry
/// has no entry for it.
fn quad_command_label(command: &str) -> String {
    let key = command.trim().trim_start_matches('\'').to_ascii_uppercase();
    crate::modules::registry::ribbon_commands()
        .get(&key)
        .map_or_else(|| command.to_string(), |&(label, _)| label.to_string())
}

/// Resolve a command's icon: registry SVGs to [`QuadIcon::Svg`], else the
/// glyph fallback filled with a short text abbreviation of the label.
pub fn quad_command_icon(command: &str) -> QuadIcon {
    match crate::modules::registry::command_icon(command) {
        Some(bytes) => QuadIcon::Svg(bytes),
        None => QuadIcon::Glyph(glyph_abbrev(&quad_command_label(command))),
    }
}

/// Initials of up to four words ("Hatch Edit" → "HE"), else the label's first
/// two letters, upper-cased.
fn glyph_abbrev(label: &str) -> String {
    let words: Vec<&str> = label.split_whitespace().collect();
    if words.len() > 1 && words.len() <= 4 {
        words
            .iter()
            .filter_map(|word| word.chars().next())
            .map(|ch| ch.to_ascii_uppercase())
            .collect()
    } else {
        label.chars().take(2).collect::<String>().to_ascii_uppercase()
    }
}

/// Push `command` when not already present.
fn push_unique(commands: &mut Vec<String>, command: String) {
    if !commands.iter().any(|c| *c == command) {
        commands.push(command);
    }
}

/// Static tab group of a quad command, used to file the commands that did not
/// fit the primary panel into short-labelled tabs. `command`'s leading verb
/// decides the group; unknown commands file under "Modify".
fn command_group(command: &str) -> &'static str {
    let verb = command
        .trim()
        .trim_start_matches('\'')
        .trim_start_matches('-')
        .split([' ', ';'])
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if verb.starts_with("DIM") {
        return "Dimension";
    }
    match verb.as_str() {
        "LINE" | "CIRCLE" | "CIRCLE_CD" | "CIRCLE_2P" | "CIRCLE_3P" | "CIRCLE_TTR"
        | "CIRCLE_TTT" | "ARC" | "ARC_3P" | "ELLIPSE" | "RECT" | "RECTANG" | "POLY"
        | "PLINE" | "PLINEFIT" | "SPLINE" | "POINT" | "HATCH" | "DONUT" | "HELIX"
        | "REVCLOUD" | "REVCLOUD_RECTANGULAR" | "REVCLOUD_POLYGONAL" => "Draw",
        "TEXT" | "MTEXT" | "TEXTEDIT" | "DDEDIT" | "MTEDIT" | "TEDIT" | "TXT2MTXT"
        | "ATTDEF" | "ATTEDIT" | "LEADER" | "MLEADER" | "TABLE" | "TABLEDIT" => "Annotate",
        "INSERT" | "BLOCK" | "BEDIT" | "REFEDIT" | "CLIP" | "XCLIP" | "XATTACH"
        | "ATTACH" => "Block",
        _ => "Modify",
    }
}

/// Universal panel (`applies_to` empty or a literal `"*"`).
fn is_universal(applies_to: &[String]) -> bool {
    applies_to.is_empty() || applies_to.iter().any(|k| k == "*")
}

/// Assemble the full quad: index 0 is the "primary" grid panel, `[1..]` are
/// the bottom tabs.
///
/// For a hovered entity or a selection the primary panel leads with the
/// target type's most recently used commands (its first item is the quad's
/// default command: first MRU entry if any, else the first curated command
/// for the type), filled up from the curated `registry::quad_entity_commands`
/// table to `quad_width.max(1)` items. Commands that do not fit are filed
/// into short-labelled groups, and plugin panels matching the target follow.
///
/// With no selection the primary panel lists the globally recent commands
/// (falling back to the creation commands while empty) and the tabs are the
/// static creation groups plus the universal plugin panels.
pub fn quad_panels_for_target(
    target: &QuadTarget,
    mru: &QuadMru,
    quad_width: usize,
    plugin_panels: &[QuadPanel],
) -> Vec<QuadPanel> {
    let width = quad_width.max(1);
    let mut panels = Vec::new();
    match target {
        QuadTarget::None => {
            let mut commands = mru.recent_global(width);
            if commands.len() < width {
                for (_, cmds) in crate::modules::registry::quad_no_selection_groups() {
                    for cmd in *cmds {
                        push_unique(&mut commands, (*cmd).to_string());
                        if commands.len() >= width {
                            break;
                        }
                    }
                    if commands.len() >= width {
                        break;
                    }
                }
            }
            panels.push(QuadPanel {
                id: "quad.recent".to_string(),
                label: "Recent".to_string(),
                items: commands.iter().map(|c| QuadItem::from_command(c)).collect(),
                applies_to: Vec::new(),
            });
            for (idx, (label, cmds)) in
                crate::modules::registry::quad_no_selection_groups().iter().enumerate()
            {
                panels.push(QuadPanel {
                    id: format!("quad.grp.{idx}"),
                    label: (*label).to_string(),
                    items: cmds.iter().map(|c| QuadItem::from_command(c)).collect(),
                    applies_to: Vec::new(),
                });
            }
            for panel in plugin_panels {
                if is_universal(&panel.applies_to) {
                    panels.push(plugin_panel(panel));
                }
            }
        }
        QuadTarget::Hovered { type_key, .. } => {
            let mut union: Vec<String> = Vec::new();
            for cmd in mru.recent_for(type_key, QUAD_MRU_CAP) {
                push_unique(&mut union, cmd);
            }
            for cmd in crate::modules::registry::quad_entity_commands(type_key) {
                push_unique(&mut union, (*cmd).to_string());
            }
            panels.push(entity_primary(&union, width));
            panels.extend(command_group_panels(&union[union.len().min(width)..]));
            panels.extend(
                plugin_panels
                    .iter()
                    .filter(|p| target.matches_applies(&p.applies_to))
                    .map(|p| plugin_panel(p)),
            );
        }
        QuadTarget::Selection { type_keys, .. } => {
            let mut union: Vec<String> = Vec::new();
            for key in type_keys {
                for cmd in mru.recent_for(key, QUAD_MRU_CAP) {
                    push_unique(&mut union, cmd);
                }
            }
            for key in type_keys {
                for cmd in crate::modules::registry::quad_entity_commands(key) {
                    push_unique(&mut union, (*cmd).to_string());
                }
            }
            panels.push(entity_primary(&union, width));
            panels.extend(command_group_panels(&union[union.len().min(width)..]));
            panels.extend(
                plugin_panels
                    .iter()
                    .filter(|p| target.matches_applies(&p.applies_to))
                    .map(|p| plugin_panel(p)),
            );
        }
    }
    panels
}

/// The primary panel of a hover/selection quad: `width` of the deduplicated
/// union, tab-less under the cursor.
fn entity_primary(union: &[String], width: usize) -> QuadPanel {
    QuadPanel {
        id: "quad.primary".to_string(),
        label: String::new(),
        items: union
            .iter()
            .take(width)
            .map(|c| QuadItem::from_command(c))
            .collect(),
        applies_to: Vec::new(),
    }
}

/// File the commands that did not fit the primary panel into static groups by
/// [`command_group`], keeping encounter order of the groups.
fn command_group_panels(commands: &[String]) -> Vec<QuadPanel> {
    let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
    for command in commands {
        let group = command_group(command);
        match groups.iter_mut().find(|(name, _)| *name == group) {
            Some((_, items)) => items.push(command.clone()),
            None => groups.push((group, vec![command.clone()])),
        }
    }
    let mut panels = Vec::new();
    for (idx, (label, items)) in groups.into_iter().enumerate() {
        panels.push(QuadPanel {
            id: format!("quad.grp.{idx}"),
            label: label.to_string(),
            items: items.iter().map(|c| QuadItem::from_command(c)).collect(),
            applies_to: Vec::new(),
        });
    }
    panels
}

/// Clone a plugin-registered panel for the build (the `applies_to` filtering
/// happened at the call site; plugin panels are recognized by their
/// namespaced id prefix).
fn plugin_panel(panel: &QuadPanel) -> QuadPanel {
    panel.clone()
}

#[cfg(test)]
mod quad_tests {
    use super::*;

    fn plugin_panel(id: &str, applies_to: &[&str]) -> QuadPanel {
        QuadPanel {
            id: id.to_string(),
            label: "Plugin".to_string(),
            items: Vec::new(),
            applies_to: applies_to.iter().map(|k| (*k).to_string()).collect(),
        }
    }

    #[test]
    fn mru_dedupes_moves_to_front_and_caps() {
        let mut mru = QuadMru::new();
        mru.record_for("Line", "MOVE");
        mru.record_for("Line", "COPY");
        mru.record_for("Line", "MOVE");
        assert_eq!(mru.recent_for("Line", 10), vec!["MOVE", "COPY"]);
        assert!(mru.recent_for("Circle", 10).is_empty());

        for i in 0..QUAD_MRU_CAP + 4 {
            mru.record_for("Line", &format!("CMD{i}"));
        }
        let all = mru.recent_for("Line", QUAD_MRU_CAP);
        assert_eq!(all.len(), QUAD_MRU_CAP);
        assert_eq!(all[0], format!("CMD{}", QUAD_MRU_CAP + 3));
        // Oldest entries fell off the tail of the capped list.
        assert_eq!(mru.recent_for("Line", 100).len(), QUAD_MRU_CAP);
        assert_eq!(mru.recent_for("Line", 2), vec![all[0].clone(), all[1].clone()]);

        let mut mru = QuadMru::new();
        mru.record_global("OFFSET");
        mru.record_global("MOVE");
        mru.record_global("OFFSET");
        assert_eq!(mru.recent_global(10), vec!["OFFSET", "MOVE"]);
        assert_eq!(mru.recent_global(1), vec!["OFFSET".to_string()]);
        assert_eq!(mru.recent_for("Line", 5), Vec::<String>::new());
    }

    #[test]
    fn hover_line_primary_respects_quad_width_mru_wins_curated_fills() {
        let mut mru = QuadMru::new();
        mru.record_for("Line", "PEDIT");
        mru.record_for("Line", "BREAK");
        let target = QuadTarget::Hovered {
            handle: Handle::new(1),
            type_key: "Line".to_string(),
        };

        // MRU leads (moves the type-specific edit commands to the front);
        // the primary panel is capped at the given width exactly.
        let panels = quad_panels_for_target(&target, &mru, 3, &[]);
        assert_eq!(panels.len(), 2);
        assert_eq!(panels[0].id, "quad.primary");
        assert_eq!(panels[0].label, "");
        let cmds: Vec<&str> = panels[0].items.iter().map(|i| i.command.as_str()).collect();
        assert_eq!(cmds, vec!["BREAK", "PEDIT", "MOVE"]);
        // The default command is the first MRU entry.
        assert_eq!(panels[0].items[0].command, "BREAK");

        // Overflow files into a short-labelled static group panel.
        let panels = quad_panels_for_target(&target, &mru, QUAD_WIDTH_DEFAULT as usize, &[]);
        let primary = quad_panels_for_target(&target, &mru, 6, &[]).remove(0);
        assert_eq!(primary.items.len(), 6);
        let primaries: Vec<&str> = primary.items.iter().map(|i| i.command.as_str()).collect();
        assert!(primaries.contains(&"PEDIT"));
        assert!(panels
            .iter()
            .any(|p| p.id.starts_with("quad.grp.") && !p.label.is_empty()));
        // The tabs never duplicate the primary's commands (docs/quad-menu.md
        // §3.3); the primary itself is the source of `primaries`.
        for panel in &panels[1..] {
            assert!(!panel.items.is_empty());
            assert!(panel
                .items
                .iter()
                .all(|i| !primaries.contains(&i.command.as_str())));
        }

        // No MRU yet: the first curated command of the type is the default.
        let panels = quad_panels_for_target(&target, &QuadMru::new(), 2, &[]);
        let curated = crate::modules::registry::quad_entity_commands("Line");
        assert!(!curated.is_empty());
        assert_eq!(panels[0].items[0].command, curated[0]);
    }

    #[test]
    fn selection_primary_leads_with_any_types_mru() {
        let mut mru = QuadMru::new();
        mru.record_for("Circle", "OFFSET");
        let target = QuadTarget::Selection {
            handles: vec![Handle::new(1), Handle::new(2)],
            type_keys: vec!["Line".to_string(), "Circle".to_string()],
        };
        let panels = quad_panels_for_target(&target, &mru, 4, &[]);
        assert_eq!(panels[0].items[0].command, "OFFSET");
        let curated = crate::modules::registry::quad_entity_commands("Line");
        let cmds: Vec<&str> = panels[0].items.iter().map(|i| i.command.as_str()).collect();
        assert!(cmds.contains(&curated[0]), "curated commands fill {cmds:?}");
    }

    #[test]
    fn no_selection_quad_fills_recent_then_creation_and_filters_plugins() {
        let plugins = [
            plugin_panel("plugin:a:line", &["Line"]),
            plugin_panel("plugin:b:all", &["*"]),
            plugin_panel("plugin:c:open", &[]),
        ];
        let panels =
            quad_panels_for_target(&QuadTarget::None, &QuadMru::new(), 4, &plugins);
        // Only universal plugins survive the No Selection quad.
        let ids: Vec<&str> = panels.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"plugin:b:all"));
        assert!(ids.contains(&"plugin:c:open"));
        assert!(!ids.contains(&"plugin:a:line"));
        // Plugin panels keep their namespaced ids (the prefix is the
        // plugin-ness marker).
        for panel in panels.iter().filter(|p| p.id.starts_with("plugin:")) {
            assert_eq!(panel.label, "Plugin");
        }

        // Empty MRU falls back to the creation commands for the grid.
        let recent = &panels[0];
        assert_eq!(recent.id, "quad.recent");
        assert_eq!(recent.label, "Recent");
        assert!(!recent.items.is_empty());
        assert!(recent.items.iter().all(|i| !i.command.is_empty()));

        // The tabs are the static creation groups plus the universal plugins.
        let groups = crate::modules::registry::quad_no_selection_groups();
        let static_tab_ids: Vec<&str> = panels
            .iter()
            .filter(|p| p.id.starts_with("quad.grp."))
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(static_tab_ids.len(), groups.len());
        assert_eq!(panels.len(), 1 + groups.len() + 2);

        // The global MRU fills the Recent grid first.
        let mut mru = QuadMru::new();
        mru.record_global("OFFSET");
        mru.record_global("LIST");
        let panels = quad_panels_for_target(&QuadTarget::None, &mru, 2, &[]);
        let cmds: Vec<&str> = panels[0].items.iter().map(|i| i.command.as_str()).collect();
        assert_eq!(cmds, vec!["LIST", "OFFSET"]);
        assert!(panels[1..]
            .iter()
            .all(|p| p.id.starts_with("quad.grp.") || p.id.starts_with("plugin:")));
    }

    #[test]
    fn matches_applies_cases() {
        let line = "Line".to_string();
        let circle = "Circle".to_string();
        let any = "*".to_string();

        // None matches only universal panels.
        assert!(QuadTarget::None.matches_applies(&[]));
        assert!(QuadTarget::None.matches_applies(&[any.clone()]));
        assert!(!QuadTarget::None.matches_applies(&[line.clone()]));

        let hovered = QuadTarget::Hovered {
            handle: Handle::new(7),
            type_key: line.clone(),
        };
        assert!(hovered.matches_applies(&[line.clone()]));
        assert!(hovered.matches_applies(&[any.clone()]));
        assert!(hovered.matches_applies(&[circle.clone(), line.clone()]));
        assert!(!hovered.matches_applies(&[circle.clone()]));
        // Type keys are matched case-insensitively (plugin class ids may drift).
        assert!(hovered.matches_applies(&["line".to_string()]));

        let selection = QuadTarget::Selection {
            handles: vec![Handle::new(1), Handle::new(2)],
            type_keys: vec![line.clone(), circle.clone()],
        };
        assert!(selection.matches_applies(&[circle.clone()]));
        assert!(selection.matches_applies(&[any.clone()]));
        assert!(!selection.matches_applies(&["Table".to_string()]));

        assert!(!hovered.is_none());
        assert!(QuadTarget::None.is_none());
    }

    #[test]
    fn icon_falls_back_to_glyph_for_unknown_command() {
        let icon = quad_command_icon("DEFINITELYNOTACOMMAND");
        match icon {
            QuadIcon::Glyph(abbrev) => assert!(!abbrev.is_empty()),
            QuadIcon::Svg(_) | QuadIcon::SvgOwned(_) => {
                panic!("unknown command must use the glyph fallback")
            }
        }
        // A known ribbon command resolves the same SVG everywhere it appears.
        let known = quad_command_icon("MOVE");
        match known {
            QuadIcon::Svg(bytes) => assert!(!bytes.is_empty()),
            QuadIcon::SvgOwned(_) | QuadIcon::Glyph(_) => panic!("MOVE resolved wrong icon"),
        }
        let label = quad_command_label("MOVE");
        assert!(!label.is_empty());
    }
}
