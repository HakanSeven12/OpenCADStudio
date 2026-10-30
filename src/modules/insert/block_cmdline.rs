//! Command-line forms of the Block Definition tools: -BLOCK, -WBLOCK,
//! -BEDIT and ATTSYNC, with the reference's prompts and messages, and the
//! choices the Block Definition / Write Block dialogs remember.

use codec::{Document, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::ui::window::block_definition::{invalid_block_name_char, BlockObjectMode};

// ── Remembered dialog choices ─────────────────────────────────────────────

/// What the Block Definition and Write Block dialogs remember between
/// uses (and the profile keeps): the on-screen switches, the object
/// conversion, Allow exploding and Open in block editor. -BLOCK and -WBLOCK
/// start from the same conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockDefaults {
    pub base_on_screen: bool,
    pub objects_on_screen: bool,
    pub mode: BlockObjectMode,
    pub allow_exploding: bool,
    pub open_in_editor: bool,
    pub wblock_mode: BlockObjectMode,
}

static DEFAULTS: std::sync::Mutex<BlockDefaults> = std::sync::Mutex::new(BlockDefaults {
    base_on_screen: false,
    objects_on_screen: false,
    mode: BlockObjectMode::Convert,
    allow_exploding: true,
    open_in_editor: false,
    wblock_mode: BlockObjectMode::Retain,
});

pub fn defaults() -> std::sync::MutexGuard<'static, BlockDefaults> {
    DEFAULTS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The settings code of a conversion (0 Retain, 1 Convert, 2 Delete).
pub fn mode_code(mode: BlockObjectMode) -> u8 {
    match mode {
        BlockObjectMode::Retain => 0,
        BlockObjectMode::Convert => 1,
        BlockObjectMode::Delete => 2,
    }
}

pub fn mode_from_code(code: u8) -> BlockObjectMode {
    match code {
        0 => BlockObjectMode::Retain,
        2 => BlockObjectMode::Delete,
        _ => BlockObjectMode::Convert,
    }
}

fn mode_name(mode: BlockObjectMode) -> &'static str {
    match mode {
        BlockObjectMode::Retain => "Retain",
        BlockObjectMode::Convert => "Convert",
        BlockObjectMode::Delete => "Delete",
    }
}

/// `Current settings: Object conversion=…`, printed as -BLOCK and -WBLOCK start.
pub fn current_settings_line(mode: BlockObjectMode) -> String {
    format!("Current settings: Object conversion={}", mode_name(mode))
}

// ── Block table listing (`?`) ─────────────────────────────────────────────

/// The block names of a drawing as the `?` listing sorts and counts them.
#[derive(Clone, Debug, Default)]
pub struct BlockTable {
    /// (name, kind) for every block but the model and paper spaces.
    pub entries: Vec<(String, BlockKind)>,
    /// Names of the blocks that hold attribute definitions.
    pub attributed: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    User,
    External,
    Dependent,
    Unnamed,
}

impl BlockTable {
    pub fn of(doc: &Document) -> Self {
        let mut entries = Vec::new();
        let mut attributed = Vec::new();
        for br in doc.block_records.iter() {
            let upper = br.name.to_ascii_uppercase();
            if upper == "*MODEL_SPACE" || upper.starts_with("*PAPER_SPACE") {
                continue;
            }
            let kind = if br.flags.is_xref || br.flags.is_xref_overlay {
                BlockKind::External
            } else if br.name.contains('|') {
                BlockKind::Dependent
            } else if br.name.starts_with('*') || br.flags.anonymous {
                BlockKind::Unnamed
            } else {
                BlockKind::User
            };
            if kind == BlockKind::User
                && br.entity_handles.iter().any(|h| {
                    matches!(
                        doc.get_entity(*h),
                        Some(codec::EntityType::AttributeDefinition(_))
                    )
                })
            {
                attributed.push(br.name.clone());
            }
            entries.push((br.name.clone(), kind));
        }
        entries.sort_by(|a, b| sort_key(&a.0).cmp(&sort_key(&b.0)));
        attributed.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        Self {
            entries,
            attributed,
        }
    }

    pub fn user_names(&self) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter(|(_, kind)| *kind == BlockKind::User)
            .map(|(name, _)| name.as_str())
    }

    /// The user block named `name`, in its stored case.
    pub fn find_user(&self, name: &str) -> Option<String> {
        self.user_names()
            .find(|n| n.eq_ignore_ascii_case(name))
            .map(str::to_string)
    }

    /// `Defined blocks.`, the matching names, and the four counts of the
    /// blocks the pattern matched.
    pub fn listing(&self, pattern: &str) -> String {
        let pattern = if pattern.trim().is_empty() {
            "*"
        } else {
            pattern.trim()
        };
        let matches = |name: &str| {
            pattern
                .split(',')
                .any(|p| crate::io::xref_model::wildcard_match(name, p.trim()))
        };
        let mut out = String::from("Defined blocks.");
        let mut counts = [0usize; 4];
        for (name, kind) in &self.entries {
            if !matches(name) {
                continue;
            }
            match kind {
                BlockKind::User => {
                    out.push_str(&format!("\n  \"{name}\""));
                    counts[0] += 1;
                }
                BlockKind::External => counts[1] += 1,
                BlockKind::Dependent => counts[2] += 1,
                BlockKind::Unnamed => counts[3] += 1,
            }
        }
        out.push_str(&format!(
            "\nUser     External     Dependent   Unnamed\nBlocks   References   Blocks      Blocks\n{:>5}{:>10}{:>12}{:>12}",
            counts[0], counts[1], counts[2], counts[3]
        ));
        out
    }
}

/// Punctuation sorts before digits and letters; letters ignore case.
fn sort_key(name: &str) -> Vec<(u8, char)> {
    name.chars()
        .map(|c| {
            let lower = c.to_lowercase().next().unwrap_or(c);
            let rank = if c.is_alphanumeric() { 1 } else { 0 };
            (rank, lower)
        })
        .collect()
}

fn yes_no(text: &str, default: bool) -> Option<bool> {
    match text.trim().to_ascii_uppercase().as_str() {
        "" => Some(default),
        "Y" | "YES" => Some(true),
        "N" | "NO" => Some(false),
        _ => None,
    }
}

/// A block name -BLOCK accepts.
fn valid_block_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('*')
        && invalid_block_name_char(name).is_none()
        && name.chars().count() <= 255
}

// ── -BLOCK ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum BlockStep {
    Name,
    ListPattern,
    Redefine,
    Base,
    Annotative,
    Orient,
    Mode,
    Objects,
}

/// -BLOCK: `Enter block name or [?]:` → (redefine?) →
/// `Specify insertion base point or [Annotative/mOde]:` → `Select objects:`.
/// A preselection is taken as the objects; an empty selection defines an
/// empty block.
pub struct DashBlockCommand {
    step: BlockStep,
    table: BlockTable,
    ucs: crate::app::helpers::UcsXform,
    name: String,
    redefine: bool,
    mode: BlockObjectMode,
    annotative: bool,
    match_orientation: bool,
    base: DVec3,
    preselected: Vec<Handle>,
    handles: Vec<Handle>,
    /// The drawing's insertion units, given to the definition.
    units: i16,
}

impl DashBlockCommand {
    pub fn new(
        table: BlockTable,
        ucs: crate::app::helpers::UcsXform,
        preselected: Vec<Handle>,
        units: i16,
    ) -> Self {
        Self {
            units,
            step: BlockStep::Name,
            table,
            ucs,
            name: String::new(),
            redefine: false,
            mode: defaults().mode,
            annotative: false,
            match_orientation: false,
            base: DVec3::ZERO,
            preselected,
            handles: Vec::new(),
        }
    }

    pub fn settings_line(&self) -> String {
        current_settings_line(self.mode)
    }

    fn create(&self, handles: Vec<Handle>) -> CmdResult {
        CmdResult::CreateBlockWithOptions {
            options: Box::new(crate::scene::CreateBlockOptions {
                name: self.name.clone(),
                handles,
                base_point: self.base,
                world_to_block: self.ucs.to_ucs_transform_at(self.base),
                block_to_world: self.ucs.to_wcs_transform_at(self.base),
                mode: self.mode,
                annotative: self.annotative,
                match_orientation: self.match_orientation,
                scale_uniformly: self.annotative,
                allow_exploding: true,
                unit: self.units,
                description: String::new(),
                hyperlink_url: String::new(),
                hyperlink_desc: String::new(),
                redefine: self.redefine,
            }),
        }
    }

    fn after_base(&mut self) -> CmdResult {
        if self.preselected.is_empty() {
            self.step = BlockStep::Objects;
            CmdResult::NeedPoint
        } else {
            let handles = self.preselected.clone();
            self.create(handles)
        }
    }
}

impl CadCommand for DashBlockCommand {
    fn name(&self) -> &'static str {
        "-BLOCK"
    }

    fn prompt(&self) -> String {
        match self.step {
            BlockStep::Name => "Enter block name or [?]:".to_string(),
            BlockStep::ListPattern => "Enter block(s) to list <*>:".to_string(),
            BlockStep::Redefine => format!(
                "Block \"{}\" already exists. Redefine it? [Yes/No] <N>:",
                self.name
            ),
            BlockStep::Base => "Specify insertion base point or [Annotative/mOde]:".to_string(),
            BlockStep::Annotative => "Create annotative block [Yes/No] <Y>:".to_string(),
            BlockStep::Orient => {
                "Orient relative to sheet in paper space viewports [Yes/No] <N>:".to_string()
            }
            BlockStep::Mode => format!(
                "Choose mode for selected object conversion [Convert to block/Retain/Delete] <{}>:",
                mode_name(self.mode)
            ),
            BlockStep::Objects => "Select objects:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        let opts: &[(&str, &str)] = match self.step {
            BlockStep::Name => &[("?", "?")],
            BlockStep::Redefine | BlockStep::Annotative | BlockStep::Orient => {
                &[("Yes", "Y"), ("No", "N")]
            }
            BlockStep::Base => &[("Annotative", "A"), ("mOde", "O")],
            BlockStep::Mode => &[("Convert to block", "C"), ("Retain", "R"), ("Delete", "D")],
            _ => &[],
        };
        opts.iter().map(|(l, k)| CmdOption::new(l, k)).collect()
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, BlockStep::Objects)
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            BlockStep::Name | BlockStep::ListPattern => InputKind::FreeText,
            BlockStep::Base => InputKind::Point,
            BlockStep::Objects => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, BlockStep::Base)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        let up = t.to_ascii_uppercase();
        Some(match self.step {
            BlockStep::Name => {
                if t == "?" {
                    self.step = BlockStep::ListPattern;
                    return Some(CmdResult::NeedPoint);
                }
                if !valid_block_name(t) {
                    return Some(CmdResult::ReportError("Invalid block name.".to_string()));
                }
                match self.table.find_user(t) {
                    Some(existing) => {
                        self.name = existing;
                        self.step = BlockStep::Redefine;
                    }
                    None => {
                        self.name = t.to_string();
                        self.step = BlockStep::Base;
                    }
                }
                CmdResult::NeedPoint
            }
            BlockStep::ListPattern => CmdResult::Measurement(self.table.listing(t)),
            BlockStep::Redefine => match yes_no(t, false) {
                Some(true) => {
                    self.redefine = true;
                    self.step = BlockStep::Base;
                    CmdResult::NeedPoint
                }
                Some(false) => CmdResult::Measurement(String::new()),
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            BlockStep::Base => match up.as_str() {
                "A" | "ANNOTATIVE" => {
                    self.step = BlockStep::Annotative;
                    CmdResult::NeedPoint
                }
                "O" | "MODE" => {
                    self.step = BlockStep::Mode;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError("Point or option keyword required.".to_string()),
            },
            BlockStep::Annotative => match yes_no(t, true) {
                Some(true) => {
                    self.annotative = true;
                    self.step = BlockStep::Orient;
                    CmdResult::NeedPoint
                }
                Some(false) => {
                    self.annotative = false;
                    self.match_orientation = false;
                    self.step = BlockStep::Base;
                    CmdResult::NeedPoint
                }
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            BlockStep::Orient => match yes_no(t, false) {
                Some(orient) => {
                    self.match_orientation = orient;
                    self.step = BlockStep::Base;
                    CmdResult::NeedPoint
                }
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            BlockStep::Mode => {
                let mode = match up.as_str() {
                    "" => Some(self.mode),
                    "C" | "CONVERT" => Some(BlockObjectMode::Convert),
                    "R" | "RETAIN" => Some(BlockObjectMode::Retain),
                    "D" | "DELETE" => Some(BlockObjectMode::Delete),
                    _ => None,
                };
                match mode {
                    Some(mode) => {
                        self.mode = mode;
                        self.step = BlockStep::Base;
                        CmdResult::NeedPoint
                    }
                    None => CmdResult::ReportError("Invalid option keyword.".to_string()),
                }
            }
            BlockStep::Objects => return None,
        })
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            BlockStep::Base => {
                self.base = pt;
                self.after_base()
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, BlockStep::Objects)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            BlockStep::Name => CmdResult::ReportError("Invalid block name.".to_string()),
            BlockStep::ListPattern => CmdResult::Measurement(self.table.listing("*")),
            BlockStep::Redefine => CmdResult::Measurement(String::new()),
            BlockStep::Base => {
                CmdResult::ReportError("Point or option keyword required.".to_string())
            }
            BlockStep::Annotative | BlockStep::Orient | BlockStep::Mode => {
                self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
            }
            BlockStep::Objects => {
                let handles = std::mem::take(&mut self.handles);
                self.create(handles)
            }
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── -WBLOCK ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum WblockStep {
    File,
    Replace,
    Source,
    Base,
    Mode,
    Objects,
}

/// What -WBLOCK writes.
#[derive(Clone, Debug, PartialEq)]
pub enum WblockSource {
    /// An existing block definition.
    Block(String),
    /// The whole drawing.
    Drawing,
    /// Objects with a base point (the conversion applied after writing).
    Objects {
        handles: Vec<Handle>,
        base: DVec3,
        mode: BlockObjectMode,
    },
}

/// -WBLOCK: `Enter name of output file:` → (replace?) →
/// `Enter name of existing block or [= (block=output file)/* (whole drawing)]
/// <define new drawing>:`
/// → `Specify insertion base point or [mOde]:` → `Select objects:`.
pub struct DashWblockCommand {
    step: WblockStep,
    table: BlockTable,
    path: String,
    mode: BlockObjectMode,
    base: DVec3,
    preselected: Vec<Handle>,
    handles: Vec<Handle>,
}

impl DashWblockCommand {
    pub fn new(table: BlockTable, preselected: Vec<Handle>) -> Self {
        Self {
            step: WblockStep::File,
            table,
            path: String::new(),
            mode: defaults().wblock_mode,
            base: DVec3::ZERO,
            preselected,
            handles: Vec::new(),
        }
    }

    pub fn settings_line(&self) -> String {
        current_settings_line(self.mode)
    }

    fn finish(&self, source: WblockSource) -> CmdResult {
        CmdResult::Dispatch(format!(
            "_WBLOCKWRITE {}",
            serde_json::json!({
                "path": self.path,
                "source": match &source {
                    WblockSource::Block(name) => serde_json::json!({"block": name}),
                    WblockSource::Drawing => serde_json::json!({"drawing": true}),
                    WblockSource::Objects { handles, base, mode } => serde_json::json!({
                        "handles": handles.iter().map(|h| h.value()).collect::<Vec<_>>(),
                        "base": [base.x, base.y, base.z],
                        "mode": mode_code(*mode),
                    }),
                }
            })
        ))
    }

    fn after_base(&mut self) -> CmdResult {
        if self.preselected.is_empty() {
            self.step = WblockStep::Objects;
            CmdResult::NeedPoint
        } else {
            self.finish(WblockSource::Objects {
                handles: self.preselected.clone(),
                base: self.base,
                mode: self.mode,
            })
        }
    }

    fn file_name_block(&self) -> String {
        std::path::Path::new(&self.path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// The output path -WBLOCK writes: `.dwg` is added when no extension is given.
pub fn wblock_output_path(text: &str) -> String {
    let path = text.trim().trim_matches('"').to_string();
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".dwg") || lower.ends_with(".dxf") {
        path
    } else {
        format!("{path}.dwg")
    }
}

impl CadCommand for DashWblockCommand {
    fn name(&self) -> &'static str {
        "-WBLOCK"
    }

    fn prompt(&self) -> String {
        match self.step {
            WblockStep::File => "Enter name of output file:".to_string(),
            WblockStep::Replace => format!(
                "{} already exists, do you want to replace it? [Yes/No] <N>:",
                self.path.replace('/', "\\")
            ),
            WblockStep::Source => "Enter name of existing block or\n[= (block=output file)/* (whole drawing)] <define new drawing>:".to_string(),
            WblockStep::Base => "Specify insertion base point or [mOde]:".to_string(),
            WblockStep::Mode => format!(
                "Choose mode for selected object conversion [Convert to block/Retain/Delete] <{}>:",
                mode_name(self.mode)
            ),
            WblockStep::Objects => "Select objects:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        let opts: &[(&str, &str)] = match self.step {
            WblockStep::Replace => &[("Yes", "Y"), ("No", "N")],
            WblockStep::Source => &[("=", "="), ("*", "*")],
            WblockStep::Base => &[("mOde", "O")],
            WblockStep::Mode => &[("Convert to block", "C"), ("Retain", "R"), ("Delete", "D")],
            _ => &[],
        };
        opts.iter().map(|(l, k)| CmdOption::new(l, k)).collect()
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, WblockStep::Objects)
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            WblockStep::File | WblockStep::Source => InputKind::FreeText,
            WblockStep::Base | WblockStep::Objects => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, WblockStep::Base)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        let up = t.to_ascii_uppercase();
        Some(match self.step {
            WblockStep::File => {
                if t.is_empty() {
                    return Some(CmdResult::Cancel);
                }
                self.path = wblock_output_path(t);
                self.step = if std::path::Path::new(&self.path).exists() {
                    WblockStep::Replace
                } else {
                    WblockStep::Source
                };
                CmdResult::NeedPoint
            }
            WblockStep::Replace => match yes_no(t, false) {
                Some(true) => {
                    self.step = WblockStep::Source;
                    CmdResult::NeedPoint
                }
                Some(false) => CmdResult::Measurement(String::new()),
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            WblockStep::Source => match t {
                "" => {
                    self.step = WblockStep::Base;
                    CmdResult::NeedPoint
                }
                "*" => self.finish(WblockSource::Drawing),
                "=" => {
                    let name = self.file_name_block();
                    match self.table.find_user(&name) {
                        Some(block) => self.finish(WblockSource::Block(block)),
                        None => CmdResult::ReportError(format!("Block \"{name}\" not found.")),
                    }
                }
                name => match self.table.find_user(name) {
                    Some(block) => self.finish(WblockSource::Block(block)),
                    None => CmdResult::ReportError(format!("Block \"{name}\" not found.")),
                },
            },
            WblockStep::Base => match up.as_str() {
                "O" | "MODE" => {
                    self.step = WblockStep::Mode;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError("Point or option keyword required.".to_string()),
            },
            WblockStep::Mode => {
                let mode = match up.as_str() {
                    "" => Some(self.mode),
                    "C" | "CONVERT" => Some(BlockObjectMode::Convert),
                    "R" | "RETAIN" => Some(BlockObjectMode::Retain),
                    "D" | "DELETE" => Some(BlockObjectMode::Delete),
                    _ => None,
                };
                match mode {
                    Some(mode) => {
                        self.mode = mode;
                        self.step = WblockStep::Base;
                        CmdResult::NeedPoint
                    }
                    None => CmdResult::ReportError("Invalid option keyword.".to_string()),
                }
            }
            WblockStep::Objects => return None,
        })
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if self.step == WblockStep::Base {
            self.base = pt;
            return self.after_base();
        }
        CmdResult::NeedPoint
    }

    fn is_selection_gathering(&self) -> bool {
        matches!(self.step, WblockStep::Objects)
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            WblockStep::File => CmdResult::Cancel,
            WblockStep::Replace | WblockStep::Source | WblockStep::Mode => {
                self.on_text_input("").unwrap_or(CmdResult::NeedPoint)
            }
            WblockStep::Base => {
                CmdResult::ReportError("Point or option keyword required.".to_string())
            }
            WblockStep::Objects => {
                let handles = std::mem::take(&mut self.handles);
                if handles.is_empty() {
                    return CmdResult::Cancel;
                }
                self.finish(WblockSource::Objects {
                    handles,
                    base: self.base,
                    mode: self.mode,
                })
            }
        }
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

// ── -BEDIT ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum BeditStep {
    Name,
    ListPattern,
}

/// -BEDIT: `Enter block name or [?]:` opens the block editor on that
/// definition, or on a new empty one for a new name.
pub struct DashBeditCommand {
    step: BeditStep,
    table: BlockTable,
}

impl DashBeditCommand {
    pub fn new(table: BlockTable) -> Self {
        Self {
            step: BeditStep::Name,
            table,
        }
    }
}

impl DashBeditCommand {
    /// The user blocks matching `pattern`, one per line, or
    /// `No blocks found.`
    fn names(&self, pattern: &str) -> String {
        let pattern = if pattern.trim().is_empty() {
            "*"
        } else {
            pattern.trim()
        };
        let lines: Vec<String> = self
            .table
            .user_names()
            .filter(|name| {
                pattern
                    .split(',')
                    .any(|p| crate::io::xref_model::wildcard_match(name, p.trim()))
            })
            .map(|name| format!(" {name}"))
            .collect();
        if lines.is_empty() {
            "No blocks found.".to_string()
        } else {
            lines.join(
                "
",
            )
        }
    }
}

impl CadCommand for DashBeditCommand {
    fn name(&self) -> &'static str {
        "-BEDIT"
    }

    fn prompt(&self) -> String {
        match self.step {
            BeditStep::Name => "Enter block name or [?]:".to_string(),
            BeditStep::ListPattern => "Enter block(s) to list <*>:".to_string(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            BeditStep::Name => vec![CmdOption::new("?", "?")],
            BeditStep::ListPattern => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        Some(match self.step {
            BeditStep::Name => {
                if t == "?" {
                    self.step = BeditStep::ListPattern;
                    return Some(CmdResult::NeedPoint);
                }
                if !valid_block_name(t) {
                    return Some(CmdResult::ReportError("Invalid block name.".to_string()));
                }
                let name = self.table.find_user(t).unwrap_or_else(|| t.to_string());
                CmdResult::Dispatch(format!("BEDIT_NEW:{name}"))
            }
            BeditStep::ListPattern => {
                self.step = BeditStep::Name;
                CmdResult::ReportMeasurement(self.names(t))
            }
        })
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            BeditStep::Name => CmdResult::Cancel,
            BeditStep::ListPattern => {
                self.step = BeditStep::Name;
                CmdResult::ReportMeasurement(self.names("*"))
            }
        }
    }
}

// ── ATTSYNC ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum AttsyncStep {
    Option,
    Name,
    Pick,
    Confirm,
}

/// ATTSYNC: `Enter an option [?/Name/Select] <Select>:`; Name asks
/// `Enter name of block to sync or [?]:`, Select picks a reference and asks
/// `ATTSYNC block NAME? [Yes/No] <Yes>:`.
pub struct AttsyncCommand {
    step: AttsyncStep,
    table: BlockTable,
    /// Block name of each block reference, for picks.
    reference_blocks: rustc_hash::FxHashMap<Handle, String>,
    picked: String,
}

impl AttsyncCommand {
    pub fn new(table: BlockTable, reference_blocks: rustc_hash::FxHashMap<Handle, String>) -> Self {
        Self {
            step: AttsyncStep::Option,
            table,
            reference_blocks,
            picked: String::new(),
        }
    }

    fn attributed_line(&self) -> String {
        format!("Attributed blocks: {}", self.table.attributed.join(", "))
    }

    fn sync(&self, name: &str) -> CmdResult {
        match self
            .table
            .attributed
            .iter()
            .find(|n| n.eq_ignore_ascii_case(name))
        {
            Some(block) => CmdResult::Dispatch(format!("_ATTSYNCBLOCK {block}")),
            None => CmdResult::Measurement(format!(
                "Block table contains no attributed block named {name}."
            )),
        }
    }
}

impl CadCommand for AttsyncCommand {
    fn name(&self) -> &'static str {
        "ATTSYNC"
    }

    fn prompt(&self) -> String {
        match self.step {
            AttsyncStep::Option => "Enter an option [?/Name/Select] <Select>:".to_string(),
            AttsyncStep::Name => "Enter name of block to sync or [?]:".to_string(),
            AttsyncStep::Pick => "Select a block:".to_string(),
            AttsyncStep::Confirm => format!("ATTSYNC block {}? [Yes/No] <Yes>:", self.picked),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        let opts: &[(&str, &str)] = match self.step {
            AttsyncStep::Option => &[("?", "?"), ("Name", "N"), ("Select", "S")],
            AttsyncStep::Name => &[("?", "?")],
            AttsyncStep::Confirm => &[("Yes", "Y"), ("No", "N")],
            AttsyncStep::Pick => &[],
        };
        opts.iter().map(|(l, k)| CmdOption::new(l, k)).collect()
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, AttsyncStep::Pick)
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            AttsyncStep::Name => InputKind::FreeText,
            AttsyncStep::Pick => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, AttsyncStep::Pick)
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        match self.reference_blocks.get(&handle) {
            Some(name) => {
                if self
                    .table
                    .attributed
                    .iter()
                    .any(|n| n.eq_ignore_ascii_case(name))
                {
                    self.picked = name.clone();
                    self.step = AttsyncStep::Confirm;
                    CmdResult::NeedPoint
                } else {
                    CmdResult::Measurement(format!(
                        "Block table contains no attributed block named {name}."
                    ))
                }
            }
            None => CmdResult::ReportError("Error selecting entity.".to_string()),
        }
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        let up = t.to_ascii_uppercase();
        Some(match self.step {
            AttsyncStep::Option => match up.as_str() {
                "?" => CmdResult::ReportMeasurement(self.attributed_line()),
                "N" | "NAME" => {
                    self.step = AttsyncStep::Name;
                    CmdResult::NeedPoint
                }
                "" | "S" | "SELECT" => {
                    self.step = AttsyncStep::Pick;
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            AttsyncStep::Name => {
                if t == "?" {
                    return Some(CmdResult::ReportMeasurement(self.attributed_line()));
                }
                if t.is_empty() {
                    return Some(CmdResult::Cancel);
                }
                self.sync(t)
            }
            AttsyncStep::Confirm => match yes_no(t, true) {
                Some(true) => {
                    let name = self.picked.clone();
                    self.sync(&name)
                }
                Some(false) => CmdResult::Cancel,
                None => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            AttsyncStep::Pick => return None,
        })
    }

    fn on_point(&mut self, _pt: DVec3) -> CmdResult {
        if self.step == AttsyncStep::Pick {
            return CmdResult::ReportError("Error selecting entity.".to_string());
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            AttsyncStep::Pick => CmdResult::Cancel,
            _ => self.on_text_input("").unwrap_or(CmdResult::Cancel),
        }
    }
}
