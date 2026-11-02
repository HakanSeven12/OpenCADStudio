//! CENTEREXE, CENTERLAYER, CENTERLTYPE, CENTERLTSCALE, CENTERLTYPEFILE,
//! CENTERCROSSSIZE, CENTERCROSSGAP and CENTERMARKEXE: the settings of new
//! centre marks and centre lines, with the reference's prompts and checks.

use crate::app::{Message, OpenCADStudio};
use iced::Task;

const NAMES: [&str; 8] = [
    "CENTEREXE",
    "CENTERLAYER",
    "CENTERLTYPE",
    "CENTERLTSCALE",
    "CENTERLTYPEFILE",
    "CENTERCROSSSIZE",
    "CENTERCROSSGAP",
    "CENTERMARKEXE",
];

pub(crate) fn is_center_variable(name: &str) -> bool {
    NAMES.iter().any(|known| known.eq_ignore_ascii_case(name))
}

/// A cross size or gap as typed: a positive length, a positive factor of
/// the diameter (`0.1x`) or ByLinetype. Returns the factor when relative.
fn measure(value: &str) -> Option<Option<f64>> {
    let value = value.trim();
    if value.eq_ignore_ascii_case("ByLinetype") {
        return Some(None);
    }
    if let Some(factor) = value.strip_suffix(['x', 'X']) {
        let factor = factor.trim().parse::<f64>().ok()?;
        return (factor.is_finite() && factor > 0.0).then_some(Some(factor));
    }
    let length = value.parse::<f64>().ok()?;
    (length.is_finite() && length > 0.0).then_some(None)
}

impl OpenCADStudio {
    /// The value a CENTER* prompt shows between angle brackets.
    fn center_variable_display(&self, name: &str, i: usize) -> String {
        let settings = self.tabs[i].scene.centerline_settings();
        let quoted = |value: &str| format!("\"{value}\"");
        match name {
            "CENTEREXE" => format!("{:.4}", settings.extension),
            "CENTERLTSCALE" => format!("{:.4}", settings.linetype_scale),
            "CENTERCROSSSIZE" => quoted(&settings.cross_size),
            "CENTERCROSSGAP" => quoted(&settings.cross_gap),
            "CENTERLTYPEFILE" => quoted(&settings.linetype_file),
            "CENTERMARKEXE" => if settings.mark_extensions { "ON" } else { "OFF" }.to_string(),
            "CENTERLAYER" if settings.layer.trim() == "." || settings.layer.is_empty() => {
                quoted(&crate::tr!("center-cmd", "var-use-current"))
            }
            "CENTERLAYER" => quoted(&settings.layer),
            "CENTERLTYPE" if settings.linetype.trim() == "." => {
                quoted(&crate::tr!("center-cmd", "var-use-current"))
            }
            _ => quoted(&settings.linetype),
        }
    }

    fn center_variable_prompt(&mut self, name: &str, i: usize) {
        let current = self.center_variable_display(name, i);
        let prompt = if matches!(name, "CENTERLAYER" | "CENTERLTYPE") {
            crate::tr!("center-cmd", "var-prompt-current", name = name, current = current)
        } else {
            crate::tr!("center-cmd", "var-prompt", name = name, current = current)
        };
        self.command_line.push_output(&prompt);
        self.pending_setvar = Some(name.to_string());
    }

    /// `CENTEREXE [value]` (etc.) and `SETVAR CENTEREXE value`: prompt for a
    /// value, or check and store the one given; a rejected value is reported
    /// and asked for again.
    pub(super) fn center_setvar(&mut self, name: &str, value: Option<&str>, i: usize) -> Task<Message> {
        let name = name.to_ascii_uppercase();
        let Some(value) = value.map(|value| value.trim().trim_matches('"').to_string()) else {
            self.center_variable_prompt(&name, i);
            return Task::none();
        };
        let errors: Vec<String> = match name.as_str() {
            "CENTEREXE" => match value.parse::<f64>() {
                Ok(number) if number.is_finite() => Vec::new(),
                _ => vec![crate::tr!("center-cmd", "var-numeric")],
            },
            "CENTERLTSCALE" => match value.parse::<f64>() {
                Ok(number) if number.is_finite() && number > 0.0 => Vec::new(),
                _ => vec![crate::tr!("center-cmd", "var-ltscale")],
            },
            "CENTERCROSSSIZE" => match measure(&value) {
                Some(_) => Vec::new(),
                None => vec![crate::tr!("center-cmd", "var-format")],
            },
            "CENTERCROSSGAP" => match measure(&value) {
                Some(Some(factor)) if factor > 0.45 => vec![
                    crate::tr!("center-cmd", "var-value"),
                    crate::tr!("center-cmd", "var-gap"),
                ],
                Some(_) => Vec::new(),
                None => vec![crate::tr!("center-cmd", "var-format")],
            },
            "CENTERMARKEXE" => match value.to_ascii_uppercase().as_str() {
                "0" | "1" | "ON" | "OFF" => Vec::new(),
                _ => vec![crate::tr!("center-cmd", "var-markexe")],
            },
            "CENTERLTYPE" => {
                let document = &mut self.tabs[i].scene.document;
                let known = value.is_empty()
                    || value == "."
                    || value.eq_ignore_ascii_case("ByLayer")
                    || value.eq_ignore_ascii_case("ByBlock")
                    || document.line_types.contains(&value)
                    || crate::io::linetypes::ensure_catalog_linetype(document, &value);
                if known {
                    Vec::new()
                } else {
                    vec![crate::tr!("center-cmd", "var-ltype")]
                }
            }
            "CENTERLAYER" => {
                if value.is_empty() {
                    vec![crate::tr!("center-cmd", "var-layer")]
                } else {
                    Vec::new()
                }
            }
            "CENTERLTYPEFILE" => {
                #[cfg(target_arch = "wasm32")]
                if !value.is_empty() && !value.eq_ignore_ascii_case("acadiso.lin") && !value.eq_ignore_ascii_case("acad.lin") {
                    return Task::perform(
                        crate::io::pick_linetype_file_web(),
                        Message::CenterlineLinetypeFilePicked,
                    );
                }
                #[cfg(not(target_arch = "wasm32"))]
                if let Ok(source) = std::fs::read_to_string(&value) {
                    crate::io::linetypes::populate_document_from_source(
                        &mut self.tabs[i].scene.document,
                        &source,
                        std::path::Path::new(&value).parent(),
                    );
                }
                Vec::new()
            }
            _ => Vec::new(),
        };
        if !errors.is_empty() {
            for error in &errors {
                self.command_line.push_error(error);
            }
            self.center_variable_prompt(&name, i);
            return Task::none();
        }
        let stored = match name.as_str() {
            "CENTERMARKEXE" => match value.to_ascii_uppercase().as_str() {
                "1" | "ON" => "1".to_string(),
                _ => "0".to_string(),
            },
            _ => value,
        };
        self.tabs[i].scene.set_centerline_setting(&name, &stored);
        self.tabs[i].dirty = true;
        Task::none()
    }
}
