//! Interactive native Rotation, Alignment and Flip placement. Rotation's base
//! angle sets the line the default angle turns from; the radius pick supplies
//! the length only. Rotation then takes a label location and a grip count.
use crate::command::{CadCommand, CmdOption, CmdResult};
use crate::scene::{
    block_orientation_parameter::{self, Kind},
    WireModel,
};
use codec::{types::Vector3, Handle};
use glam::DVec3;

pub struct OrientationParameterCommand {
    document_id: u64,
    definition: Handle,
    kind: Kind,
    base: Option<DVec3>,
    second: Option<DVec3>,
    perpendicular: bool,
    choosing_type: bool,
    base_angle: f64,
    choosing_base_angle: bool,
    rotation: Option<f64>,
    label: Option<DVec3>,
    grips: Option<usize>,
}

impl OrientationParameterCommand {
    pub(crate) fn new(document_id: u64, definition: Handle, kind: Kind) -> Self {
        Self {
            document_id,
            definition,
            kind,
            base: None,
            second: None,
            perpendicular: kind == Kind::Alignment,
            choosing_type: false,
            base_angle: 0.0,
            choosing_base_angle: false,
            rotation: None,
            label: None,
            grips: None,
        }
    }
    fn asking_base_angle(&self) -> bool {
        self.kind == Kind::Rotation && self.second.is_some() && self.choosing_base_angle
    }
    fn asking_label(&self) -> bool {
        self.kind == Kind::Rotation && self.rotation.is_some() && self.label.is_none()
    }
    fn asking_grips(&self) -> bool {
        self.kind == Kind::Rotation && self.label.is_some() && self.grips.is_none()
    }
    fn finish(&self, end: DVec3, reference: DVec3) -> CmdResult {
        self.place(end, reference, None)
    }
    fn place(&self, end: DVec3, reference: DVec3, label_offset: Option<f64>) -> CmdResult {
        let base = self.base.unwrap();
        let vector = |p: DVec3| Vector3::new(p.x, p.y, p.z);
        if let Some(error) = block_orientation_parameter::points_error(
            self.kind,
            vector(base),
            vector(end),
            vector(reference),
        ) {
            return CmdResult::ReportError(crate::t!(error).into_owned());
        }
        CmdResult::PlaceBlockOrientationParameter {
            document_id: self.document_id,
            definition: self.definition,
            kind: self.kind,
            perpendicular: self.perpendicular,
            base,
            end,
            reference,
            label_offset,
            grips: self.grips,
        }
    }
    fn rotation_geometry(&self, angle: f64) -> (DVec3, DVec3) {
        let base = self.base.unwrap();
        let radius = (self.second.unwrap() - base).length();
        let direction = |angle: f64| DVec3::new(angle.cos(), angle.sin(), 0.0) * radius;
        (
            base + direction(self.base_angle + angle),
            base + direction(self.base_angle),
        )
    }
    fn rotation_at(&mut self, angle: f64) -> CmdResult {
        self.rotation = Some(angle);
        CmdResult::NeedPoint
    }
    fn finish_rotation(&self) -> CmdResult {
        let angle = self.rotation.unwrap_or(0.0);
        let (end, reference) = self.rotation_geometry(angle);
        let label = self.label.unwrap();
        let vector = |p: DVec3| Vector3::new(p.x, p.y, p.z);
        let offset = block_orientation_parameter::rotation_label_offset(
            vector(self.base.unwrap()),
            vector(end),
            vector(reference),
            vector(label),
        );
        self.place(end, reference, offset)
    }
    fn commit_grips(&mut self, grips: usize) -> CmdResult {
        self.grips = Some(grips);
        self.finish_rotation()
    }
    fn set_base_angle(&mut self, angle: f64) {
        self.base_angle = angle.rem_euclid(std::f64::consts::TAU);
        self.choosing_base_angle = false;
    }
}

impl CadCommand for OrientationParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }
    fn prompt(&self) -> String {
        let step = if self.choosing_type {
            if self.perpendicular {
                "Choose alignment type [Perpendicular/Tangent] <Perpendicular>:"
            } else {
                "Choose alignment type [Perpendicular/Tangent] <Tangent>:"
            }
        } else if self.base.is_none() {
            "Specify parameter base point:"
        } else {
            match (self.kind, self.second.is_some()) {
                (Kind::Rotation, false) => "Specify radius of parameter:",
                (Kind::Rotation, true) if self.choosing_base_angle => "Specify base angle <0>:",
                (Kind::Rotation, true) if self.asking_label() => "Specify label location:",
                (Kind::Rotation, true) if self.asking_grips() => {
                    "Enter number of grips [0/1] <1>:"
                }
                (Kind::Rotation, true) => "Specify default rotation angle or [Base angle] <0>:",
                (Kind::Alignment, _) => {
                    if self.perpendicular {
                        "Specify alignment direction or [Type] <Perpendicular>:"
                    } else {
                        "Specify alignment direction or [Type] <Tangent>:"
                    }
                }
                (Kind::Flip, false) => "Specify reflection line endpoint:",
                (Kind::Flip, true) => "Specify flip parameter label location:",
            }
        };
        crate::tf!(
            "BPARAMETER {}  {}",
            crate::t!(self.kind.name()),
            crate::t!(step)
        )
        .into_owned()
    }
    fn options(&self) -> Vec<CmdOption> {
        if self.kind == Kind::Rotation
            && self.second.is_some()
            && !self.choosing_base_angle
            && self.rotation.is_none()
        {
            return vec![CmdOption::new(crate::t!("Base angle").as_ref(), "B")];
        }
        if self.kind != Kind::Alignment {
            return vec![];
        }
        if self.choosing_type {
            vec![
                CmdOption::new(crate::t!("Perpendicular").as_ref(), "Perpendicular"),
                CmdOption::new(crate::t!("Tangent").as_ref(), "Tangent"),
            ]
        } else {
            vec![CmdOption::new(crate::t!("Type").as_ref(), "Type")]
        }
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.choosing_type {
            return CmdResult::ReportError(
                crate::t!("Choose Perpendicular or Tangent.").into_owned(),
            );
        }
        if !point.is_finite() {
            return CmdResult::ReportError(
                crate::t!("Specify a finite parameter location.").into_owned(),
            );
        }
        let Some(base) = self.base else {
            self.base = Some(point);
            return CmdResult::NeedPoint;
        };
        if (point.z - base.z).abs() > 1e-8 {
            return CmdResult::ReportError(
                crate::t!("Specify points in the same block XY plane.").into_owned(),
            );
        }
        if self.kind == Kind::Flip && self.second.is_some() {
            return self.finish(self.second.unwrap(), point);
        }
        if self.asking_grips() {
            return CmdResult::ReportError(
                crate::t!("Enter the number of grips: 0 or 1.").into_owned(),
            );
        }
        if self.asking_label() {
            self.label = Some(point);
            return CmdResult::NeedPoint;
        }
        let d = point - base;
        let radius = d.x.hypot(d.y);
        let degenerate = radius <= 1e-10 || !radius.is_finite() || !d.length().is_finite();
        if self.asking_base_angle() {
            if degenerate {
                return CmdResult::ReportError(
                    crate::t!("Specify a finite nonzero base angle direction.").into_owned(),
                );
            }
            self.set_base_angle(d.y.atan2(d.x));
            return CmdResult::NeedPoint;
        }
        if degenerate {
            return CmdResult::ReportError(
                crate::t!("Specify a finite nonzero parameter direction or radius.").into_owned(),
            );
        }
        match self.kind {
            Kind::Alignment => self.finish(point, point),
            Kind::Flip => {
                self.second = Some(point);
                CmdResult::NeedPoint
            }
            Kind::Rotation => {
                if self.second.is_some() {
                    self.rotation_at(d.y.atan2(d.x))
                } else {
                    self.second = Some(base + DVec3::X * radius);
                    CmdResult::NeedPoint
                }
            }
        }
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.kind == Kind::Alignment {
            let text = text.trim().to_ascii_uppercase();
            if self.choosing_type {
                match text.as_str() {
                    "PERPENDICULAR" | "P" => self.perpendicular = true,
                    "TANGENT" | "T" => self.perpendicular = false,
                    _ => {
                        return Some(CmdResult::ReportError(
                            crate::t!("Choose Perpendicular or Tangent.").into_owned(),
                        ))
                    }
                }
                self.choosing_type = false;
                return Some(CmdResult::NeedPoint);
            }
            if matches!(text.as_str(), "TYPE" | "T") {
                self.choosing_type = true;
                return Some(CmdResult::NeedPoint);
            }
        }
        if self.kind != Kind::Rotation || self.base.is_none() {
            return None;
        }
        let text = text.trim();
        if self.asking_base_angle() {
            let value = if text.is_empty() {
                0.0
            } else {
                text.parse::<f64>().ok()?
            };
            if !value.is_finite() {
                return Some(CmdResult::ReportError(
                    crate::t!("Specify a finite base angle.").into_owned(),
                ));
            }
            self.set_base_angle(value.to_radians());
            return Some(CmdResult::NeedPoint);
        }
        if self.asking_grips() {
            let grips = match text {
                "" => 1,
                "0" => 0,
                "1" => 1,
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Enter the number of grips: 0 or 1.").into_owned(),
                    ))
                }
            };
            return Some(self.commit_grips(grips));
        }
        if self.asking_label() {
            return Some(CmdResult::ReportError(
                crate::t!("Specify a label location.").into_owned(),
            ));
        }
        if self.second.is_some()
            && matches!(
                text.to_ascii_uppercase().as_str(),
                "B" | "BASE" | "BASE ANGLE"
            )
        {
            self.choosing_base_angle = true;
            return Some(CmdResult::NeedPoint);
        }
        let value = text.parse::<f64>().ok()?;
        if !value.is_finite() {
            return Some(CmdResult::ReportError(
                crate::t!("Specify a finite radius or angle.").into_owned(),
            ));
        }
        Some(if self.second.is_some() {
            self.rotation_at(value.rem_euclid(360.0).to_radians())
        } else if value > 1e-10 {
            self.on_point(self.base.unwrap() + DVec3::X * value)
        } else {
            CmdResult::ReportError(crate::t!("Specify a positive parameter radius.").into_owned())
        })
    }
    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        let Some(base) = self.base.filter(|_| point.is_finite()) else {
            return vec![];
        };
        let mut lines = vec![];
        if self.kind == Kind::Rotation {
            let radius = self
                .second
                .map(|p| (p - base).length())
                .unwrap_or((point - base).length());
            if !radius.is_finite() {
                return vec![];
            }
            let angle = if let Some(angle) = self.rotation {
                self.base_angle + angle
            } else {
                let turn = if self.asking_base_angle() {
                    0.0
                } else {
                    self.base_angle
                };
                turn + (point.y - base.y).atan2(point.x - base.x)
            };
            let end = base + DVec3::new(angle.cos(), angle.sin(), 0.0) * radius;
            lines.push(vec![base.to_array(), end.to_array()]);
            lines.push(
                (0..=64)
                    .map(|i| {
                        let a = std::f64::consts::TAU * i as f64 / 64.0;
                        (base + DVec3::new(a.cos(), a.sin(), 0.0) * radius).to_array()
                    })
                    .collect(),
            );
        } else {
            lines.push(vec![
                base.to_array(),
                self.second.unwrap_or(point).to_array(),
            ]);
            if let Some(end) = self.second {
                lines.push(vec![end.to_array(), point.to_array()]);
            }
        }
        lines
            .into_iter()
            .map(|points| {
                WireModel::solid_f64(
                    "preview_orientation_parameter".into(),
                    points,
                    WireModel::CYAN,
                    false,
                )
            })
            .collect()
    }
    fn on_enter(&mut self) -> CmdResult {
        if self.choosing_type {
            self.choosing_type = false;
            return CmdResult::NeedPoint;
        }
        if self.asking_base_angle() {
            self.set_base_angle(0.0);
            return CmdResult::NeedPoint;
        }
        if self.asking_grips() {
            return self.commit_grips(1);
        }
        if self.asking_label() {
            return CmdResult::Cancel;
        }
        if self.kind == Kind::Rotation && self.second.is_some() {
            self.rotation_at(0.0)
        } else {
            CmdResult::Cancel
        }
    }
}
