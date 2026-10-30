// DIMOVERRIDE — set or clear per-object dimension-variable overrides.
//
// Variables and their values are gathered here; the assembled list is handed
// to `DIMOVERRIDE_APPLY <NAME=value;…>` (or `DIMOVERRIDE_APPLY CLEAR`), which
// asks for the dimensions and writes the overrides.

use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::entities::dim_override::{named_var, parse_var_value, VarKind};
use crate::t;

/// The variables whose values are names: arrowhead blocks and the text style.
pub const NAMED_VALUE_VARS: [&str; 5] = ["DIMBLK", "DIMBLK1", "DIMBLK2", "DIMLDRBLK", "DIMTXSTY"];

/// What a named-value variable can take in the drawing, and its current value.
#[derive(Default)]
pub struct NamedValues {
    /// The current value of each of `NAMED_VALUE_VARS`, as shown in `<…>`.
    pub defaults: Vec<(String, String)>,
    pub blocks: Vec<String>,
    pub text_styles: Vec<String>,
}

pub struct DimOverrideCommand {
    vars: Vec<(String, String)>,
    /// The variable whose value is asked, and the value Enter keeps.
    pending: Option<(String, VarKind, String)>,
    /// The current dimension style, whose values are the defaults.
    style: Option<codec::tables::DimStyle>,
    named: NamedValues,
}

impl DimOverrideCommand {
    pub fn new(style: Option<codec::tables::DimStyle>, named: NamedValues) -> Self {
        Self {
            vars: Vec::new(),
            pending: None,
            style,
            named,
        }
    }

    /// Why a named value is refused: an arrowhead block that is neither
    /// standard nor in the drawing, or a text style the drawing lacks.
    fn named_refusal(&self, name: &str, value: &str) -> Option<String> {
        if name == "DIMTXSTY" {
            let known = self
                .named
                .text_styles
                .iter()
                .any(|style| style.eq_ignore_ascii_case(value.trim()));
            return (!known).then(|| t!("No style found.").into_owned());
        }
        let known = crate::entities::arrowheads::typed_standard_block(value).is_some()
            || self
                .named
                .blocks
                .iter()
                .any(|block| block.eq_ignore_ascii_case(value.trim()));
        (!known).then(|| crate::tf!("Block \"{value}\" not defined").into_owned())
    }

    /// Record `value` for the variable being asked, or say why it is refused.
    fn accept(&mut self, value: &str) -> CmdResult {
        let Some((name, kind, current)) = self.pending.take() else {
            return CmdResult::NeedPoint;
        };
        let refused = if NAMED_VALUE_VARS.contains(&name.as_str()) {
            self.named_refusal(&name, value)
        } else {
            self.value_refusal(&name, kind, value)
        };
        if let Some(message) = refused {
            self.pending = Some((name, kind, current));
            return CmdResult::ReportError(message);
        }
        self.vars.retain(|(known, _)| *known != name);
        self.vars.push((name, value.to_string()));
        CmdResult::NeedPoint
    }

    fn value_refusal(&self, name: &str, kind: VarKind, value: &str) -> Option<String> {
        match (kind, parse_var_value(kind, value)) {
            (_, None) if kind == VarKind::Real => {
                Some(t!("Requires numeric distance or two points.").into_owned())
            }
            (_, None) => Some(t!("DIMOVERRIDE: invalid value for this variable.").into_owned()),
            (_, Some(codec::xdata::XDataValue::Real(number)))
                if number <= 0.0 && matches!(name, "DIMASZ" | "DIMTXT" | "DIMTSZ") =>
            {
                (number < 0.0).then(|| t!("Value must be positive.").into_owned())
            }
            _ => None,
        }
    }
}

impl CadCommand for DimOverrideCommand {
    fn name(&self) -> &'static str {
        "DIMOVERRIDE"
    }

    fn prompt(&self) -> String {
        match &self.pending {
            None if self.vars.is_empty() => {
                t!("DIMOVERRIDE  Enter dimension variable name to override or [Clear overrides]:")
                    .into_owned()
            }
            None => t!("DIMOVERRIDE  Enter dimension variable name to override:").into_owned(),
            Some((_, _, current)) => crate::tf!(
                "DIMOVERRIDE  Enter new value for dimension variable <{current}>:"
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        if self.pending.is_some() || !self.vars.is_empty() {
            return Vec::new();
        }
        vec![CmdOption::new("Clear overrides", "CLEAR")]
    }

    fn input_kind(&self) -> InputKind {
        if matches!(self.pending, Some((_, VarKind::Text, _))) {
            InputKind::FreeText
        } else {
            InputKind::SingleToken
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let value = text.trim();
        if self.pending.is_some() {
            if value.is_empty() {
                return None;
            }
            return Some(self.accept(value));
        }
        if value.is_empty() {
            return None;
        }
        let upper = value.to_ascii_uppercase();
        if self.vars.is_empty() && matches!(upper.trim_start_matches('_'), "C" | "CLEAR") {
            return Some(CmdResult::Dispatch("DIMOVERRIDE_APPLY CLEAR".to_string()));
        }
        if let Some(var) = NAMED_VALUE_VARS.iter().find(|var| **var == upper) {
            let current = self
                .named
                .defaults
                .iter()
                .find(|(name, _)| name == var)
                .map(|(_, value)| value.clone())
                .unwrap_or_default();
            self.pending = Some((upper, VarKind::Text, current));
            return Some(CmdResult::NeedPoint);
        }
        Some(match named_var(&upper) {
            Some((code, kind)) => {
                let current = self
                    .style
                    .as_ref()
                    .and_then(|style| crate::entities::dim_override::style_value_text(style, code))
                    .unwrap_or_default();
                self.pending = Some((upper, kind, current));
                CmdResult::NeedPoint
            }
            None => CmdResult::ReportError(
                crate::tf!("Variable \"{upper}\" not found.").into_owned(),
            ),
        })
    }

    fn on_point(&mut self, _point: DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        // Enter at the value keeps the current one.
        if let Some((_, _, current)) = &self.pending {
            let current = current.clone();
            return self.accept(&current);
        }
        if self.vars.is_empty() {
            return CmdResult::Cancel;
        }
        let spec: Vec<String> = self
            .vars
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect();
        CmdResult::Dispatch(format!("DIMOVERRIDE_APPLY {}", spec.join(";")))
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

/// Parse a `NAME=value;…` list into record pairs; the error names the first bad entry.
pub fn parse_spec(spec: &str) -> Result<Vec<(i16, codec::xdata::XDataValue)>, String> {
    spec.split(';')
        .filter(|entry| !entry.trim().is_empty())
        .map(|entry| {
            let (name, value) = entry.split_once('=').unwrap_or((entry, ""));
            named_var(name)
                .and_then(|(code, kind)| Some((code, parse_var_value(kind, value)?)))
                .ok_or_else(|| entry.trim().to_string())
        })
        .collect()
}

inventory::submit!(crate::command::CommandRegistration { names: &["DIMOVERRIDE"] });
