//! Values the calculator works with, and the context they live in.

use std::collections::HashMap;

use glam::DVec3;

/// A calculator result: a plain number, or a point (`[x,y]` / `[x,y,z]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Number(f64),
    Point(DVec3),
}

impl Value {
    /// The value as the calculator writes it back: numbers without float
    /// noise (`0.1+0.2` shows `0.3`), points in the literal syntax they are
    /// typed in, so a result can be pasted straight into another expression.
    pub fn display(&self) -> String {
        match self {
            Value::Number(n) => format_number(*n),
            Value::Point(p) if p.z == 0.0 => {
                format!("[{},{}]", format_number(p.x), format_number(p.y))
            }
            Value::Point(p) => format!(
                "[{},{},{}]",
                format_number(p.x),
                format_number(p.y),
                format_number(p.z)
            ),
        }
    }
}

/// Up to ten decimals with trailing zeros dropped, never `-0`. Magnitudes
/// that would round away to nothing keep their digits in scientific form.
fn format_number(n: f64) -> String {
    if n != 0.0 && n.abs() < 1e-6 {
        return format!("{n:e}");
    }
    let mut s = format!("{n:.10}");
    if s.contains('.') {
        s.truncate(s.trim_end_matches('0').trim_end_matches('.').len());
    }
    if s == "-0" {
        s = "0".into();
    }
    s
}

/// How trig functions read and return angles, and what an unsuffixed angle
/// literal means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AngleMode {
    #[default]
    Degrees,
    Radians,
}

impl AngleMode {
    pub(super) fn to_radians(self, a: f64) -> f64 {
        match self {
            AngleMode::Degrees => a.to_radians(),
            AngleMode::Radians => a,
        }
    }

    pub fn radians_to_mode(self, a: f64) -> f64 {
        match self {
            AngleMode::Degrees => a.to_degrees(),
            AngleMode::Radians => a,
        }
    }
}

/// A system variable as `getvar` sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SysVar {
    Number(f64),
    Point(DVec3),
    /// Exists but holds text (a layer name, say): not a calculator value.
    Text,
}

/// Reads system variables for `getvar`, by lowercase name. The host passes
/// it in, so the engine itself never touches the drawing.
#[derive(Clone)]
pub struct SysVars(pub std::sync::Arc<SysVarFn>);

type SysVarFn = dyn Fn(&str) -> Option<SysVar> + Send + Sync;

impl std::fmt::Debug for SysVars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SysVars")
    }
}

/// Everything an evaluation reads and may change: variables (by lowercase
/// name, without the `$`), the previous result, the angle mode, the
/// drawing's length unit and the system variables.
#[derive(Debug, Clone)]
pub struct CalcContext {
    pub vars: HashMap<String, Value>,
    pub ans: Option<Value>,
    pub angle_mode: AngleMode,
    /// Millimetres per drawing unit (INSUNITS); length suffixes convert to it.
    pub base_unit: f64,
    pub sysvars: Option<SysVars>,
}

impl CalcContext {
    pub fn new(angle_mode: AngleMode, base_unit: f64) -> Self {
        Self {
            vars: HashMap::new(),
            ans: None,
            angle_mode,
            base_unit,
            sysvars: None,
        }
    }
}

impl Default for CalcContext {
    /// Degrees, millimetre drawing.
    fn default() -> Self {
        Self::new(AngleMode::Degrees, 1.0)
    }
}
