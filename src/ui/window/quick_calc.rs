//! The QuickCalc pallet (QUICKCALC / QC): an expression field on the shared
//! calculator engine (`app::calc`) with its history, number pad, scientific
//! and point functions, a unit converter, variables, and picks that take a
//! point, distance or angle from the drawing.
//!
//! The pallet's state and everything it does without the drawing live here;
//! the app side (`app::quick_calc`) adds the system variables, the
//! clipboard, the command line and the picks.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::app::calc::{self, AngleMode, CalcContext, CalcError, SysVars, Value};
use crate::modules::draw::inquiry::calc_pick::PickKind;

/// The expression field, so the app can focus it.
pub const INPUT_ID: &str = "quick-calc-input";

/// How long a status message ("Copied 870") stays in the preview line.
const FLASH_SECS: f32 = 2.2;

/// The collapsible sections below the keypad, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Units,
    Variables,
}

impl Section {
    pub const ALL: [Section; 2] = [Section::Units, Section::Variables];

    fn index(self) -> usize {
        self as usize
    }
}

/// What the unit converter converts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitCat {
    Length,
    Area,
    Angle,
}

impl UnitCat {
    pub const ALL: [UnitCat; 3] = [UnitCat::Length, UnitCat::Area, UnitCat::Angle];

    /// The units, each with its size in the category's base unit (mm, m², °).
    pub fn units(self) -> &'static [(&'static str, f64)] {
        match self {
            UnitCat::Length => &[
                ("mm", 1.0),
                ("cm", 10.0),
                ("m", 1000.0),
                ("km", 1_000_000.0),
                ("in", 25.4),
                ("ft", 304.8),
            ],
            UnitCat::Area => &[
                ("mm²", 1e-6),
                ("cm²", 1e-4),
                ("m²", 1.0),
                ("ha", 10_000.0),
                ("ft²", 0.092_903_04),
            ],
            UnitCat::Angle => &[
                ("deg", 1.0),
                ("rad", 180.0 / std::f64::consts::PI),
                ("grad", 0.9),
            ],
        }
    }

    /// The pair the converter starts on.
    fn defaults(self) -> (&'static str, &'static str) {
        match self {
            UnitCat::Length => ("mm", "m"),
            UnitCat::Area => ("mm²", "m²"),
            UnitCat::Angle => ("deg", "rad"),
        }
    }

    fn size(self, unit: &str) -> Option<f64> {
        self.units()
            .iter()
            .find(|(name, _)| *name == unit)
            .map(|(_, size)| *size)
    }
}

#[derive(Debug, Clone)]
pub enum QuickCalcMsg {
    Input(String),
    /// Enter: evaluate, add to the history, set `ans`.
    Submit,
    /// Ctrl+Enter, or the toolbar button: the current value to the command line.
    PasteToCommandLine,
    Copy,
    Pick(PickKind),
    CancelPick,
    /// A pick finished (from `CmdResult::QuickCalcPick`).
    Picked(Value),
    /// The toolbar's DEG / RAD button: switch to the other angle mode.
    ToggleAngleMode,
    ClearHistory,
    /// A history row's expression: load it into the input.
    HistoryLoad(usize),
    /// A history row's result: copy it.
    HistoryCopy(usize),
    /// ↑ (`true`) / ↓ through the history while the input has focus.
    HistoryStep(bool),
    Key(Key),
    Memory(MemoryOp),
    /// A value picked from the memory flyout.
    MemoryInsert(usize),
    SetKeypadMode(KeypadMode),
    /// Open or close a flyout over the keys.
    Flyout(Flyout),
    CloseFlyout,
    Toggle(Section),
    UnitCat(UnitCat),
    UnitFrom(&'static str),
    UnitTo(&'static str),
    UnitValue(String),
    UnitSwap,
    UnitInsert,
    Var(VarMsg),
    /// The status message timer ran out.
    FlashExpired,
}

/// The settings kept with the user's profile.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuickCalcSettings {
    /// The variables, in their list order.
    pub variables: Vec<Variable>,
    /// Which sections are open, in [`Section::ALL`] order.
    pub sections: [bool; 2],
    pub radians: bool,
    pub scientific: bool,
}

/// One evaluated line. The value keeps full precision; only the view rounds.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub expr: String,
    pub value: Value,
}

/// What the input currently evaluates to.
#[derive(Debug, Clone, PartialEq)]
pub enum Preview {
    Empty,
    Ok(Value),
    Err(CalcError),
}

/// A finished evaluation the app still has to act on.
#[derive(Debug, Clone, PartialEq)]
pub struct Submitted {
    pub value: Value,
    /// `$name = …`: the variable it set.
    pub assign: Option<String>,
}

#[derive(Debug)]
pub struct QuickCalcPalette {
    pub show: bool,
    pub input: String,
    pub history: Vec<Entry>,
    /// The history row ↑/↓ is on.
    pub hist_idx: Option<usize>,
    pub ans: Option<Value>,
    pub mode: AngleMode,
    /// Every variable, global to all drawings.
    pub variables: Vec<Variable>,
    pub selected_var: Option<String>,
    /// Categories folded shut in the list.
    pub collapsed: HashSet<String>,
    /// The Variable Definition form, while it is open.
    pub form: Option<VarForm>,
    /// Memory, newest first.
    pub memory: Vec<f64>,
    pub open: [bool; 2],
    pub keypad_mode: KeypadMode,
    /// 2nd and hyp on the scientific keypad.
    pub second: bool,
    pub hyp: bool,
    pub flyout: Option<Flyout>,
    /// The pick the pallet started (its command may since have ended).
    pub pick: Option<PickKind>,
    pub flash: Option<(String, iced::time::Instant)>,
    pub unit_cat: UnitCat,
    pub unit_from: &'static str,
    pub unit_to: &'static str,
    pub unit_value: String,
    /// The user is typing in the input (set by input, cleared by any press
    /// elsewhere). Escape clears the input only then, since a focused
    /// field drops its focus before the Escape shortcut arrives.
    pub typing: bool,
    /// The input as it was when Enter failed on it. Its error shows only
    /// while the input is unchanged, so a half-typed `1.2m / ` is not
    /// flagged before the user asks for the result.
    pub failed_input: Option<String>,
}

impl Default for QuickCalcPalette {
    fn default() -> Self {
        let (unit_from, unit_to) = UnitCat::Length.defaults();
        Self {
            show: false,
            input: String::new(),
            history: Vec::new(),
            hist_idx: None,
            ans: None,
            mode: AngleMode::Degrees,
            variables: Vec::new(),
            selected_var: None,
            collapsed: HashSet::new(),
            form: None,
            memory: Vec::new(),
            open: QuickCalcSettings::default().sections,
            keypad_mode: KeypadMode::Standard,
            second: false,
            hyp: false,
            flyout: None,
            pick: None,
            flash: None,
            unit_cat: UnitCat::Length,
            unit_from,
            unit_to,
            unit_value: "1".into(),
            typing: false,
            failed_input: None,
        }
    }
}

/// Whether `input` starts with `* / ^`, so it continues from `ans`.
fn continues_ans(input: &str) -> bool {
    input.trim_start().starts_with(['*', '/', '^'])
}

/// Whether `input` has a length suffix, so its value was converted.
fn has_length_suffix(input: &str) -> bool {
    let bytes = input.as_bytes();
    if input.contains(['\'', '"']) {
        return true;
    }
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() || bytes[i] == b'.' {
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1;
            }
            let suffix = input[start..i].to_ascii_lowercase();
            if matches!(suffix.as_str(), "mm" | "cm" | "m" | "km" | "in" | "ft") {
                return true;
            }
        } else {
            i += 1;
        }
    }
    false
}

/// A value as the clipboard and the command line get it: full precision,
/// and a point as the `x,y,z` a coordinate prompt reads.
pub fn full_text(value: &Value) -> String {
    match value {
        Value::Number(n) => n.to_string(),
        Value::Point(p) if p.z == 0.0 => format!("{},{}", p.x, p.y),
        Value::Point(p) => format!("{},{},{}", p.x, p.y, p.z),
    }
}

impl QuickCalcPalette {
    pub fn settings(&self) -> QuickCalcSettings {
        QuickCalcSettings {
            variables: self.variables.clone(),
            sections: self.open,
            radians: self.mode == AngleMode::Radians,
            scientific: self.keypad_mode == KeypadMode::Scientific,
        }
    }

    pub fn apply_settings(&mut self, s: &QuickCalcSettings) {
        self.variables = variables::clean(&s.variables);
        self.open = s.sections;
        self.keypad_mode = if s.scientific {
            KeypadMode::Scientific
        } else {
            KeypadMode::Standard
        };
        self.mode = if s.radians {
            AngleMode::Radians
        } else {
            AngleMode::Degrees
        };
    }

    pub fn is_open(&self, section: Section) -> bool {
        self.open[section.index()]
    }

    /// The evaluation context, every variable defined.
    pub fn context(&self, base_unit: f64, sysvars: Option<SysVars>) -> CalcContext {
        self.context_and_errors(base_unit, sysvars).0
    }

    /// Whether the error of the current input should show: only after Enter
    /// failed on exactly this text.
    pub fn error_shown(&self) -> bool {
        self.failed_input.as_deref() == Some(self.input.as_str())
    }

    pub fn preview(&self, ctx: &CalcContext) -> Preview {
        if self.input.trim().is_empty() {
            return Preview::Empty;
        }
        if let Some(error) = self.assignment_error(&self.input) {
            return Preview::Err(error);
        }
        match calc::result_of(&self.input, ctx) {
            Ok(value) => Preview::Ok(value),
            Err(error) => Preview::Err(error),
        }
    }

    /// The value Copy, Paste and the memory keys use: the input's, else the
    /// last result.
    pub fn current_value(&self, ctx: &CalcContext) -> Option<Value> {
        match self.preview(ctx) {
            Preview::Ok(value) => Some(value),
            _ => self.history.last().map(|e| e.value),
        }
    }

    /// Enter: evaluate the input; on success it joins the history and
    /// becomes `ans`, and the input clears.
    pub fn submit(&mut self, ctx: &CalcContext) -> Option<Submitted> {
        let trimmed = self.input.trim().to_string();
        if trimmed.is_empty() {
            return None;
        }
        let Preview::Ok(value) = self.preview(ctx) else {
            self.failed_input = Some(self.input.clone());
            return None;
        };
        let expr = if continues_ans(&trimmed) {
            format!("ans {trimmed}")
        } else {
            trimmed.clone()
        };
        let assign = calc::assigned_variable(&trimmed).map(|(name, _)| name);
        if let Some(name) = &assign {
            self.assign(name, value, &trimmed);
        }
        self.history.push(Entry { expr, value });
        self.ans = Some(value);
        self.input.clear();
        self.hist_idx = None;
        Some(Submitted { value, assign })
    }

    pub fn append(&mut self, text: &str) {
        self.input.push_str(text);
        self.hist_idx = None;
    }

    /// The literal a pick or a variable adds to the input.
    pub fn insert_value(&mut self, value: &Value) {
        let text = value.display();
        self.append(&text);
    }

    pub fn history_step(&mut self, up: bool) {
        if self.history.is_empty() {
            return;
        }
        match (up, self.hist_idx) {
            (true, None) => self.hist_idx = Some(self.history.len() - 1),
            (true, Some(i)) => self.hist_idx = Some(i.saturating_sub(1)),
            (false, None) => return,
            (false, Some(i)) if i + 1 >= self.history.len() => {
                self.hist_idx = None;
                self.input.clear();
                return;
            }
            (false, Some(i)) => self.hist_idx = Some(i + 1),
        }
        if let Some(i) = self.hist_idx {
            self.input = self.history[i].expr.clone();
        }
    }

    pub fn toggle(&mut self, section: Section) {
        self.open[section.index()] ^= true;
        self.flyout = None;
    }

    pub fn set_keypad_mode(&mut self, mode: KeypadMode) {
        self.keypad_mode = mode;
        self.flyout = None;
    }

    pub fn toggle_angle_mode(&mut self) {
        self.mode = match self.mode {
            AngleMode::Degrees => AngleMode::Radians,
            AngleMode::Radians => AngleMode::Degrees,
        };
    }

    pub fn set_unit_cat(&mut self, cat: UnitCat) {
        self.unit_cat = cat;
        (self.unit_from, self.unit_to) = cat.defaults();
    }

    /// The converter's result, when its value reads as a number.
    pub fn converted(&self) -> Option<f64> {
        let value: f64 = self.unit_value.trim().replace(',', ".").parse().ok()?;
        let from = self.unit_cat.size(self.unit_from)?;
        let to = self.unit_cat.size(self.unit_to)?;
        let result = value * from / to;
        result.is_finite().then_some(result)
    }

    /// What "Insert" adds: the converted value with its unit suffix when the
    /// calculator reads that unit (lengths and angles), else the bare number.
    pub fn converted_literal(&self) -> Option<String> {
        let shown = Value::Number(self.converted()?).display();
        Some(match self.unit_cat {
            UnitCat::Area => shown,
            _ => format!("{shown}{}", self.unit_to),
        })
    }

    pub fn set_flash(&mut self, message: String) {
        self.flash = Some((message, iced::time::Instant::now()));
    }

    /// Drop the status message once it has shown long enough.
    pub fn expire_flash(&mut self) {
        if self
            .flash
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed().as_secs_f32() >= FLASH_SECS - 0.05)
        {
            self.flash = None;
        }
    }

    pub fn flash_interval() -> std::time::Duration {
        std::time::Duration::from_secs_f32(FLASH_SECS)
    }
}

// ── View ────────────────────────────────────────────────────────────────

mod keypad;
mod keypad_view;
mod variables;
mod variables_view;
mod view;
pub use keypad::{Flyout, Key, KeypadMode, MemoryOp};
pub use variables::{FormMsg, StoredValue, VarForm, VarKind, VarMsg, Variable};
pub use view::{ICON, Inputs, float_height, view};

#[cfg(test)]
mod tests;
