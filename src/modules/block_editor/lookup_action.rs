//! Select a Lookup parameter, then open its property table.
use crate::command::{CadCommand, CmdResult, InputKind, PointPickContext};
use document::Handle;
use glam::DVec3;

#[derive(Clone, Debug)]
pub(crate) struct Candidate {
    pub name: String,
    pub point: DVec3,
}

pub(crate) struct LookupActionCommand {
    document: u64,
    definition: Handle,
    candidates: Vec<Candidate>,
    context: Option<PointPickContext>,
}
impl LookupActionCommand {
    pub(crate) fn new(document: u64, definition: Handle, candidates: Vec<Candidate>) -> Self {
        Self {
            document,
            definition,
            candidates,
            context: None,
        }
    }
    fn choose(&self, candidate: &Candidate) -> CmdResult {
        CmdResult::OpenBlockLookupTable {
            document: self.document,
            definition: self.definition,
            name: candidate.name.clone(),
        }
    }
}
impl CadCommand for LookupActionCommand {
    fn name(&self) -> &'static str {
        "BACTION"
    }
    fn input_kind(&self) -> InputKind {
        InputKind::FreeText
    }
    fn prompt(&self) -> String {
        crate::t!("BACTION Lookup  Select a parameter endpoint or enter its name:").into_owned()
    }
    fn wants_point_pick_context(&self) -> bool {
        true
    }
    fn set_point_pick_context(&mut self, context: Option<PointPickContext>) {
        self.context = context;
    }
    fn on_text_input(&mut self, s: &str) -> Option<CmdResult> {
        let s = s.trim();
        let matches = self
            .candidates
            .iter()
            .filter(|candidate| candidate.name.eq_ignore_ascii_case(s))
            .collect::<Vec<_>>();
        Some(if matches.len() == 1 {
            self.choose(matches[0])
        } else {
            CmdResult::ReportError(
                crate::t!("Enter a unique Lookup parameter name or pick its location.")
                    .into_owned(),
            )
        })
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        let Some(c) = self.context else {
            return CmdResult::ReportError(
                crate::t!("Enter the Lookup parameter name.").into_owned(),
            );
        };
        let screen =
            |p: DVec3| crate::scene::view::projection::project_screen(c.view, c.eye, c.bounds, p);
        let Some(click) = screen(point) else {
            return CmdResult::ReportError(crate::t!("Pick a Lookup parameter grip.").into_owned());
        };
        let candidate = self
            .candidates
            .iter()
            .filter_map(|p| screen(p.point).map(|at| (at.distance(click), p)))
            .filter(|(d, _)| d.is_finite() && *d <= c.aperture_px.max(10.))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, p)| p);
        match candidate {
            Some(candidate) => self.choose(candidate),
            None => CmdResult::ReportError(crate::t!("Pick a Lookup parameter grip.").into_owned()),
        }
    }
    fn on_enter(&mut self) -> CmdResult {
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PointPickContext {
        PointPickContext {
            view: glam::Mat4::orthographic_rh_gl(-10., 10., -10., 10., -1., 1.),
            eye: DVec3::ZERO,
            bounds: iced::Rectangle {
                x: 0.,
                y: 0.,
                width: 400.,
                height: 400.,
            },
            aperture_px: 100.,
        }
    }

    fn command() -> LookupActionCommand {
        LookupActionCommand::new(
            0,
            Handle::new(1),
            vec![
                Candidate {
                    name: "Lookup1".into(),
                    point: DVec3::new(-1., -1., 0.),
                },
                Candidate {
                    name: "Lookup2".into(),
                    point: DVec3::new(1., 1., 0.),
                },
            ],
        )
    }

    fn perspective_context() -> PointPickContext {
        PointPickContext {
            view: glam::camera::rh::proj::directx::perspective(
                45.0_f32.to_radians(),
                1.0,
                0.1,
                2000.0,
            ),
            eye: DVec3::ZERO,
            bounds: iced::Rectangle {
                x: 0.,
                y: 0.,
                width: 400.,
                height: 400.,
            },
            aperture_px: 100.,
        }
    }

    #[test]
    fn a_grip_behind_the_eye_is_not_picked() {
        // A point behind the eye divides by a negative clip W and lands under
        // the cursor; a pick must not take it for a grip.
        let mut command = LookupActionCommand::new(
            0,
            Handle::new(1),
            vec![Candidate {
                name: "Behind".into(),
                point: DVec3::new(0., 0., 100.),
            }],
        );
        command.set_point_pick_context(Some(perspective_context()));
        let CmdResult::ReportError(error) = command.on_point(DVec3::new(0., 0., -100.)) else {
            panic!("a grip behind the eye is not a pick target");
        };
        assert_eq!(error, "Pick a Lookup parameter grip.");

        // Both project under the cursor; only the one in front is a target.
        let mut command = LookupActionCommand::new(
            0,
            Handle::new(1),
            vec![
                Candidate {
                    name: "Behind".into(),
                    point: DVec3::new(0., 0., 100.),
                },
                Candidate {
                    name: "Front".into(),
                    point: DVec3::new(0., 0., -100.),
                },
            ],
        );
        command.set_point_pick_context(Some(perspective_context()));
        let pick = command.on_point(DVec3::new(0., 0., -100.));
        assert!(
            matches!(
                pick,
                CmdResult::OpenBlockLookupTable { ref name, .. } if name == "Front"
            ),
            "the grip behind the eye does not shadow the one in front"
        );
    }

    #[test]
    fn a_grip_pick_chooses_the_lookup_nearest_the_click() {
        let mut command = command();
        command.set_point_pick_context(Some(context()));
        // Both grips sit inside the aperture; the click is closer to the second.
        let near_second = command.on_point(DVec3::new(0.8, 0.8, 0.));
        assert!(matches!(
            near_second,
            CmdResult::OpenBlockLookupTable { ref name, .. } if name == "Lookup2"
        ));
    }

    #[test]
    fn a_pick_away_from_every_grip_keeps_the_prompt() {
        let mut command = command();
        command.set_point_pick_context(Some(context()));
        let CmdResult::ReportError(error) = command.on_point(DVec3::new(0., 9., 0.)) else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Pick a Lookup parameter grip.");
        assert_eq!(
            command.prompt(),
            "BACTION Lookup  Select a parameter endpoint or enter its name:"
        );
        // The refusal leaves the command ready: the next pick still resolves.
        let after_refusal = command.on_point(DVec3::new(0.8, 0.8, 0.));
        assert!(
            matches!(
                after_refusal,
                CmdResult::OpenBlockLookupTable { ref name, .. } if name == "Lookup2"
            ),
            "a refused pick does not discard the candidates"
        );
    }

    #[test]
    fn a_pick_without_a_screen_asks_for_the_name() {
        let CmdResult::ReportError(error) = command().on_point(DVec3::ZERO) else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Enter the Lookup parameter name.");
    }
}
