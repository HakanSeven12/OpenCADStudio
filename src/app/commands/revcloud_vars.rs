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
        // `REVCLOUDAPPROXARCLEN@x,y,z`: the length is being given as two
        // points and this is the second one.
        let (name, first_point) = match name.split_once('@') {
            Some((base, point)) => (base, typed_point(point, None)),
            None => (name, None),
        };
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
        let second_prompt = || crate::tr!("revcloud-cmd", "second-point");
        let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
            if first_point.is_some() {
                // Enter at the second point keeps the current value.
                return true;
            }
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
            _ => {
                let length = match first_point {
                    Some(first) => typed_point(value, Some(first)).map(|second| first.distance(second)),
                    None => value.parse::<f64>().ok().filter(|length| length.is_finite()),
                };
                if let (None, None, Some(first)) = (first_point, length, typed_point(value, None)) {
                    // A point starts a length measured between two points.
                    self.command_line.push_output(&second_prompt());
                    self.pending_setvar = Some(format!("{name}@{},{},{}", first.x, first.y, first.z));
                    return true;
                }
                match length {
                    Some(length) => {
                        crate::io::set_drawing_variable(
                            &mut self.tabs[i].scene.document,
                            revcloud::ARC_LENGTH_VARIABLE,
                            &format!("{length:.6}"),
                        );
                        self.tabs[i].dirty = true;
                        None
                    }
                    None => Some(crate::tr!("revcloud-cmd", "numeric")),
                }
            }
        };
        match refused {
            Some(message) => {
                self.command_line.push_error(&message);
                match first_point {
                    Some(first) => {
                        self.command_line.push_output(&second_prompt());
                        self.pending_setvar = Some(format!("{name}@{},{},{}", first.x, first.y, first.z));
                    }
                    None => {
                        self.command_line.push_output(&prompt);
                        self.pending_setvar = Some(name.to_string());
                    }
                }
            }
            None => self.save_config(),
        }
        true
    }
}

/// A typed `x,y[,z]` point; `@dx,dy[,dz]` is taken from `from`.
fn typed_point(text: &str, from: Option<glam::DVec3>) -> Option<glam::DVec3> {
    let (relative, text) = match text.trim().strip_prefix('@') {
        Some(rest) => (true, rest),
        None => (false, text.trim()),
    };
    let parts: Vec<f64> = text
        .split(',')
        .map(|part| part.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
        .collect::<Option<_>>()?;
    let point = match parts[..] {
        [x, y] => glam::DVec3::new(x, y, 0.0),
        [x, y, z] => glam::DVec3::new(x, y, z),
        _ => return None,
    };
    match (relative, from) {
        (true, Some(base)) => Some(base + point),
        (true, None) => None,
        (false, _) => Some(point),
    }
}
