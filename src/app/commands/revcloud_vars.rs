//! REVCLOUDCREATEMODE, REVCLOUDARCVARIANCE and REVCLOUDGRIPS (profile
//! settings) and REVCLOUDAPPROXARCLEN (kept in the drawing), with the
//! reference's prompts and checks.

use crate::app::OpenCADStudio;
use crate::entities::revcloud;

impl OpenCADStudio {
    /// `REVCLOUD… [value]`: report, check and store one of the revision
    /// cloud variables; a rejected value is reported and asked for again.
    /// `false` when `name` is not one of them.
    pub(super) fn revcloud_setvar(&mut self, name: &str, value: Option<&str>, i: usize) -> bool {
        let on_off = |on: bool| if on { "ON" } else { "OFF" };
        let current = match name {
            "REVCLOUDCREATEMODE" => revcloud::create_mode().to_string(),
            "REVCLOUDARCVARIANCE" => on_off(revcloud::arc_variance()).to_string(),
            "REVCLOUDGRIPS" => on_off(revcloud::grips_on()).to_string(),
            "REVCLOUDAPPROXARCLEN" => format!(
                "{:.4}",
                crate::io::drawing_variable(
                    &self.tabs[i].scene.document,
                    revcloud::ARC_LENGTH_VARIABLE
                )
                .and_then(|value| value.trim().parse::<f64>().ok())
                .unwrap_or(0.0)
            ),
            _ => return false,
        };
        let prompt = format!("Enter new value for {name} <{current}>:");
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            self.command_line.push_output(&prompt);
            self.pending_setvar = Some(name.to_string());
            return true;
        };
        let flag = |value: &str| match value.to_ascii_uppercase().as_str() {
            "1" | "ON" => Some(true),
            "0" | "OFF" => Some(false),
            _ => None,
        };
        let refused = match name {
            "REVCLOUDCREATEMODE" => match value.parse::<u8>() {
                Ok(mode @ 0..=2) => {
                    revcloud::set_create_mode(mode);
                    None
                }
                _ => Some(crate::tr!("revcloud-cmd", "var-mode")),
            },
            "REVCLOUDARCVARIANCE" | "REVCLOUDGRIPS" => match flag(value) {
                Some(on) => {
                    if name == "REVCLOUDGRIPS" {
                        revcloud::set_grips_on(on);
                        // The grips of selected clouds follow at once.
                        self.tabs[i].scene.bump_geometry();
                    } else {
                        revcloud::set_arc_variance(on);
                    }
                    None
                }
                None => Some(crate::tr!("revcloud-cmd", "var-onoff")),
            },
            _ => match value.parse::<f64>() {
                Ok(length) if length.is_finite() => {
                    crate::io::set_drawing_variable(
                        &mut self.tabs[i].scene.document,
                        revcloud::ARC_LENGTH_VARIABLE,
                        &format!("{length:.6}"),
                    );
                    self.tabs[i].dirty = true;
                    None
                }
                _ => Some(crate::tr!("revcloud-cmd", "numeric")),
            },
        };
        match refused {
            Some(message) => {
                self.command_line.push_error(&message);
                self.command_line.push_output(&prompt);
                self.pending_setvar = Some(name.to_string());
            }
            None => self.save_config(),
        }
        true
    }
}
