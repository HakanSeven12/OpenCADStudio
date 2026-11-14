//! Interactive front-end for the UCS command.
//!
//! Bare `UCS` enters this command so the option and its value are entered
//! step-by-step — `UCS` ⏎ → option ⏎ → value ⏎ — and so a single line
//! `UCS Z 90` (typed in the command line with Space between tokens, or sent
//! headless as `{"op":"run","cmd":"UCS Z 90"}`) feeds the option then the value
//! as text steps. Execution is delegated to the existing inline `UCS …` handler
//! via [`CmdResult::Dispatch`], so the coordinate-system math and persistence
//! stay in one place. (#169)

use crate::command::{CadCommand, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;
use crate::t;
use glam::DVec3;

/// The option prompt's keywords, in the order of its bracket list.
const OPTIONS: [&str; 11] = [
    "FACE", "OBJECT", "WORLD", "VIEW", "3POINT", "Z", "X", "Y", "ORIGIN", "SAVE", "DELETE",
];

/// The option a label of the localized prompt names (`M`, `Monde` for World
/// in French): its bracket list lines up with [`OPTIONS`]. A `_` prefix
/// keeps the typed text as an international keyword.
fn localized_option(typed: &str) -> Option<&'static str> {
    if typed.starts_with('_') {
        return None;
    }
    let prompt = t!(
        "UCS  specify new origin or [Face/OBject/World/View/3Point/Z/X/Y/Origin/Save/Delete] or name:"
    );
    let labels = prompt.split_once('[')?.1.split_once(']')?.0;
    let labels: Vec<&str> = labels.split('/').collect();
    if labels.len() != OPTIONS.len() {
        return None;
    }
    if typed == "3" {
        return Some("3POINT");
    }
    labels
        .iter()
        .zip(OPTIONS)
        .find(|(label, _)| crate::command::keyword_matches(label, label, typed))
        .map(|(_, option)| option)
}

#[derive(Default)]
pub struct UcsCommand {
    /// The chosen option keyword (uppercased), once entered; `None` until then.
    option: Option<String>,
    points: Vec<DVec3>,
    plane: WorkingPlane,
}

impl UcsCommand {
    pub fn new() -> Self {
        Self::default()
    }

    /// Options that take no further typed argument: they execute immediately
    /// or, for Face / OBject, start their own entity pick.
    fn is_zero_arg(opt: &str) -> bool {
        matches!(
            opt,
            "W" | "WORLD" | "VIEW" | "V" | "LIST" | "?" | "FACE" | "F" | "OBJECT" | "OB"
        )
    }

    /// Options whose argument is a coordinate (so a click is accepted too).
    fn takes_point(opt: &str) -> bool {
        matches!(opt, "ORIGIN" | "O" | "3POINT" | "3P")
    }

    /// Options that expect one more typed argument (angle or name).
    fn takes_value(opt: &str) -> bool {
        matches!(
            opt,
            "Z" | "X" | "Y" | "ORIGIN" | "O" | "SAVE" | "S" | "DELETE" | "DEL" | "D"
        )
    }
}

impl CadCommand for UcsCommand {
    fn name(&self) -> &'static str {
        "UCS"
    }

    fn prompt(&self) -> String {
        match self.option.as_deref() {
            None => {
                t!("UCS  specify new origin or [Face/OBject/World/View/3Point/Z/X/Y/Origin/Save/Delete] or name:")
                    .into_owned()
            }
            Some("Z") => t!("UCS  rotation angle about Z (degrees):").into_owned(),
            Some("X") => t!("UCS  rotation angle about X (degrees):").into_owned(),
            Some("Y") => t!("UCS  rotation angle about Y (degrees):").into_owned(),
            Some("ORIGIN") | Some("O") => t!("UCS  new origin point:").into_owned(),
            Some("3POINT") | Some("3P") => match self.points.len() {
                0 => t!("UCS  specify new origin:").into_owned(),
                1 => t!("UCS  specify point on positive X axis:").into_owned(),
                _ => t!("UCS  specify point in positive XY plane:").into_owned(),
            },
            Some("SAVE") | Some("S") => t!("UCS  name to save current UCS as:").into_owned(),
            Some("DELETE") | Some("DEL") | Some("D") => {
                t!("UCS  name of UCS to delete:").into_owned()
            }
            Some(_) => t!("UCS  value:").into_owned(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    /// The new X and Y axes, drawn from the origin while the 3-point UCS is
    /// being placed.
    fn on_mouse_move(&mut self, pt: DVec3) -> Option<WireModel> {
        if !matches!(self.option.as_deref(), Some("3POINT") | Some("3P")) {
            return None;
        }
        let (origin, x, y) = match self.points.as_slice() {
            [origin] => (*origin, pt - *origin, self.plane.z.cross(pt - *origin)),
            [origin, on_x] => {
                let x = *on_x - *origin;
                let d = pt - *origin;
                (
                    *origin,
                    x,
                    d - x * (d.dot(x) / x.length_squared().max(1.0e-300)),
                )
            }
            _ => return None,
        };
        let y = y.normalize_or_zero() * x.length();
        if y == DVec3::ZERO {
            return None;
        }
        let corner = [origin + x, origin, origin + y].map(|p| p.to_array());
        let (points, points_low) = crate::scene::convert::tessellate::points_to_ds(corner);
        let mut wire = WireModel::solid("ucs_preview".into(), points, WireModel::CYAN, false);
        wire.points_low = points_low;
        Some(wire)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        if matches!(self.option.as_deref(), None | Some("3POINT") | Some("3P"))
            && crate::entities::coord_parse::parse_coord(t).is_some()
        {
            return None;
        }
        match self.option.take() {
            // First token: the option keyword (or a named UCS to restore).
            None => {
                if t.is_empty() {
                    // Bare Enter → list (delegate to inline `UCS`).
                    return Some(CmdResult::Dispatch("UCS LIST".into()));
                }
                let up = localized_option(t)
                    .map(str::to_owned)
                    .unwrap_or_else(|| t.trim_start_matches('_').to_uppercase());
                if Self::is_zero_arg(&up) {
                    Some(CmdResult::Dispatch(format!("UCS {up}")))
                } else if Self::takes_value(&up) || Self::takes_point(&up) {
                    // Needs a value next; keep the command active and re-prompt.
                    self.option = Some(up);
                    Some(CmdResult::NeedPoint)
                } else {
                    // Not a keyword → a named UCS to activate.
                    Some(CmdResult::Dispatch(format!("UCS {t}")))
                }
            }
            // Second token: the option's value → run the assembled command.
            Some(opt) => Some(CmdResult::Dispatch(format!("UCS {opt} {t}"))),
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        // A click at the option prompt is the new origin; the X axis and XY
        // plane points follow, each optional.
        if self.option.is_none() {
            self.option = Some("3P".into());
        }
        if matches!(self.option.as_deref(), Some("3POINT") | Some("3P")) {
            self.points.push(pt);
            if self.points.len() < 3 {
                return CmdResult::NeedPoint;
            }
            let points = std::mem::take(&mut self.points);
            self.option = None;
            return CmdResult::Dispatch(format!(
                "UCS 3POINTW {},{},{}|{},{},{}|{},{},{}",
                points[0].x,
                points[0].y,
                points[0].z,
                points[1].x,
                points[1].y,
                points[1].z,
                points[2].x,
                points[2].y,
                points[2].z,
            ));
        }
        // Other point-taking options retain their explicit behavior.
        if matches!(self.option.as_deref(), Some(o) if Self::takes_point(o)) {
            self.option.take();
            return CmdResult::Dispatch(format!("UCS ORIGINW {},{},{}", pt.x, pt.y, pt.z));
        }
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        let points = std::mem::take(&mut self.points);
        match self.option.take() {
            // No option chosen yet → behave like bare `UCS` (list).
            None => CmdResult::Dispatch("UCS LIST".into()),
            // Enter accepts the axes placed so far: the origin alone moves the
            // UCS, an X point turns it about its current Z.
            Some(opt) if matches!(opt.as_str(), "3POINT" | "3P") && !points.is_empty() => {
                let origin = points[0];
                let Some(&on_x) = points.get(1) else {
                    return CmdResult::Dispatch(format!(
                        "UCS ORIGINW {},{},{}",
                        origin.x, origin.y, origin.z
                    ));
                };
                let y = self.plane.z.cross(on_x - origin);
                let on_xy = origin + if y == DVec3::ZERO { self.plane.y } else { y };
                CmdResult::Dispatch(format!(
                    "UCS 3POINTW {},{},{}|{},{},{}|{},{},{}",
                    origin.x, origin.y, origin.z, on_x.x, on_x.y, on_x.z, on_xy.x, on_xy.y, on_xy.z,
                ))
            }
            // Option chosen but no value supplied → cancel cleanly.
            Some(_) => CmdResult::Cancel,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_ucs_accepts_origin_then_x_then_xy_without_option() {
        let mut cmd = UcsCommand::new();
        assert!(matches!(
            cmd.on_point(DVec3::new(10.0, 20.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert_eq!(cmd.points.len(), 1);
        assert!(matches!(
            cmd.on_point(DVec3::new(11.0, 20.0, 0.0)),
            CmdResult::NeedPoint
        ));
        match cmd.on_point(DVec3::new(10.0, 21.0, 0.0)) {
            CmdResult::Dispatch(command) => {
                assert_eq!(command, "UCS 3POINTW 10,20,0|11,20,0|10,21,0")
            }
            _ => panic!("three clicks must dispatch the new UCS"),
        }
    }

    #[test]
    fn initial_coordinate_is_delegated_to_point_pipeline() {
        let mut cmd = UcsCommand::new();
        assert!(cmd.on_text_input("10,20,0").is_none());
        assert!(matches!(
            cmd.on_point(DVec3::new(10.0, 20.0, 0.0)),
            CmdResult::NeedPoint
        ));
        assert_eq!(cmd.points.len(), 1);
    }

    #[test]
    fn explicit_origin_still_changes_only_origin() {
        let mut cmd = UcsCommand::new();
        assert!(matches!(cmd.on_text_input("O"), Some(CmdResult::NeedPoint)));
        match cmd.on_point(DVec3::new(10.0, 20.0, 0.0)) {
            CmdResult::Dispatch(command) => assert_eq!(command, "UCS ORIGINW 10,20,0"),
            _ => panic!("explicit Origin must dispatch after one point"),
        }
    }

    #[test]
    fn localized_and_international_options_work() {
        // Tests run in en-US: the prompt's labels are the English ones.
        for option in ["World", "W", "_World"] {
            let mut cmd = UcsCommand::new();
            let result = cmd.on_text_input(option);
            assert!(matches!(result, Some(CmdResult::Dispatch(s)) if s == "UCS WORLD"));
        }
        let mut cmd = UcsCommand::new();
        assert!(matches!(
            cmd.on_text_input("Save"),
            Some(CmdResult::NeedPoint)
        ));
        let result = cmd.on_text_input("MiPlano");
        assert!(matches!(result, Some(CmdResult::Dispatch(s)) if s == "UCS SAVE MiPlano"));
    }
}
