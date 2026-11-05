//! Interactive native Rotation, Alignment and Flip placement. Rotation's base
//! angle sets the line the default angle turns from; the radius pick supplies
//! the length only. Rotation then takes a label location and a grip count.
use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::{
    block_distance_parameter::ValueSet,
    block_orientation_parameter::{self, Kind},
    WireModel,
};
use codec::{types::Vector3, Handle};
use glam::DVec3;

/// Values answered at the base-point prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub name: Option<String>,
    pub label: Option<String>,
    pub chain: Option<bool>,
    pub description: Option<String>,
    pub palette: Option<bool>,
    pub value_set: Option<ValueSet>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Name,
    Label,
    Chain,
    Description,
    Palette,
    ValueSetType,
    ValueSetIncrement,
    ValueSetMinimum,
    ValueSetMaximum,
    ValueSetList,
}

impl Asking {
    fn question(self) -> &'static str {
        match self {
            Self::Chain => {
                "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
            }
            Self::Description => "Enter property description:",
            Self::Palette => "Display property in Properties palette? [Yes/No] <Yes>:",
            Self::ValueSetType => "Enter angle value set type [None/List/Increment] <None>:",
            Self::ValueSetIncrement => "Enter angle increment:",
            Self::ValueSetMinimum => "Enter minimum angle:",
            Self::ValueSetMaximum => "Enter maximum angle:",
            Self::ValueSetList => "Enter list of angle values (separated by commas):",
            Self::Name | Self::Label => unreachable!("this question is printed with its default"),
        }
    }
    /// The creation default the question's Enter answer stands for.
    fn default_answer(self) -> bool {
        self == Self::Palette
    }
    fn is_default(self, value: bool) -> bool {
        value == self.default_answer()
    }
}

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
    asking: Option<Asking>,
    options: Options,
    name_default: String,
    label_default: String,
    increment: Option<f64>,
    minimum: Option<f64>,
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
            asking: None,
            options: Options::default(),
            name_default: String::new(),
            label_default: String::new(),
            increment: None,
            minimum: None,
        }
    }
    pub(crate) fn with_name(mut self, name: String) -> Self {
        self.name_default = name;
        self
    }
    pub(crate) fn with_label(mut self, label: String) -> Self {
        self.label_default = label;
        self
    }
    fn accept(&mut self, text: Option<&str>) -> CmdResult {
        let asking = self.asking.expect("an option question is open");
        if matches!(asking, Asking::Name | Asking::Label) {
            let value = text.unwrap_or("").trim();
            match asking {
                Asking::Name => {
                    if !value.is_empty() && !value.eq_ignore_ascii_case(&self.name_default) {
                        self.options.name = Some(value.to_owned());
                    }
                }
                Asking::Label => {
                    if !value.is_empty() && !value.eq_ignore_ascii_case(&self.label_default) {
                        self.options.label = Some(value.to_owned());
                    }
                }
                _ => unreachable!(),
            }
            self.asking = None;
            return CmdResult::NeedPoint;
        }
        if asking == Asking::Description {
            let value = text.unwrap_or("");
            if !value.is_empty() {
                self.options.description = Some(value.to_owned());
            }
            self.asking = None;
            return CmdResult::NeedPoint;
        }
        if asking == Asking::ValueSetType {
            match text
                .map(str::trim)
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str()
            {
                "" | "N" | "NONE" => {
                    self.options.value_set = Some(ValueSet::None);
                    self.asking = None;
                }
                "I" | "INCREMENT" => {
                    self.options.value_set = None;
                    self.increment = None;
                    self.minimum = None;
                    self.asking = Some(Asking::ValueSetIncrement);
                }
                "L" | "LIST" => {
                    self.options.value_set = None;
                    self.asking = Some(Asking::ValueSetList);
                }
                _ => {
                    return CmdResult::ReportError(
                        crate::t!("Invalid option keyword.").into_owned(),
                    )
                }
            }
            return CmdResult::NeedPoint;
        }
        if matches!(
            asking,
            Asking::ValueSetIncrement
                | Asking::ValueSetMinimum
                | Asking::ValueSetMaximum
                | Asking::ValueSetList
        ) {
            // The questions take degrees; the parameter stores radians.
            let answer = text.unwrap_or("").trim();
            if asking == Asking::ValueSetList {
                let values = answer
                    .split(',')
                    .map(|part| part.trim().parse::<f64>())
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
                    .filter(|values| !values.is_empty() && values.iter().all(|v| v.is_finite()));
                let Some(values) = values else {
                    return CmdResult::ReportError(
                        crate::t!("Requires numeric value.").into_owned(),
                    );
                };
                self.options.value_set = Some(ValueSet::List(
                    values.into_iter().map(f64::to_radians).collect(),
                ));
                self.asking = None;
                return CmdResult::NeedPoint;
            }
            let number = answer
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(f64::to_radians);
            let Some(number) = number else {
                return CmdResult::ReportError(crate::t!("Requires numeric value.").into_owned());
            };
            match asking {
                Asking::ValueSetIncrement => {
                    self.increment = Some(number);
                    self.asking = Some(Asking::ValueSetMinimum);
                }
                Asking::ValueSetMinimum => {
                    self.minimum = Some(number);
                    self.asking = Some(Asking::ValueSetMaximum);
                }
                Asking::ValueSetMaximum => {
                    self.options.value_set = Some(ValueSet::Increment {
                        increment: self.increment.expect("the increment was answered"),
                        minimum: self.minimum.expect("the minimum was answered"),
                        maximum: number,
                    });
                    self.asking = None;
                }
                _ => unreachable!(),
            }
            return CmdResult::NeedPoint;
        }
        let value = match text.map(str::trim) {
            None | Some("") => asking.default_answer(),
            Some(text) => match text.to_ascii_uppercase().as_str() {
                "Y" | "YES" => true,
                "N" | "NO" => false,
                _ => return CmdResult::ReportError(crate::t!("Enter Yes or No.").into_owned()),
            },
        };
        if !asking.is_default(value) {
            match asking {
                Asking::Chain => self.options.chain = Some(value),
                Asking::Palette => self.options.palette = Some(value),
                Asking::Description
                | Asking::Name
                | Asking::Label
                | Asking::ValueSetType
                | Asking::ValueSetIncrement
                | Asking::ValueSetMinimum
                | Asking::ValueSetMaximum
                | Asking::ValueSetList => unreachable!(),
            }
        }
        self.asking = None;
        CmdResult::NeedPoint
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
            options: self.options.clone(),
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
        if let Some(asking) = self.asking {
            return match asking {
                Asking::Name => {
                    crate::tf!("Enter parameter name <{}>:", self.name_default).into_owned()
                }
                Asking::Label => match self.kind {
                    Kind::Flip => crate::tf!("Enter Flip property label <{}>:", self.label_default)
                        .into_owned(),
                    _ => crate::tf!("Enter rotation property label <{}>:", self.label_default)
                        .into_owned(),
                },
                other => crate::t!(other.question()).into_owned(),
            };
        }
        let step = if self.choosing_type {
            if self.perpendicular {
                "Choose alignment type [Perpendicular/Tangent] <Perpendicular>:"
            } else {
                "Choose alignment type [Perpendicular/Tangent] <Tangent>:"
            }
        } else if self.base.is_none() {
            match self.kind {
                Kind::Rotation => {
                    "Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
                }
                Kind::Flip => {
                    "Specify base point of reflection line or [Name/Label/Description/Palette]:"
                }
                _ => "Specify parameter base point:",
            }
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
    fn input_kind(&self) -> InputKind {
        match self.asking {
            Some(
                Asking::Description
                | Asking::Name
                | Asking::Label
                | Asking::ValueSetIncrement
                | Asking::ValueSetMinimum
                | Asking::ValueSetMaximum
                | Asking::ValueSetList,
            ) => InputKind::FreeText,
            Some(_) => InputKind::SingleToken,
            None => InputKind::Point,
        }
    }
    fn point_step_accepts_keywords(&self) -> bool {
        self.asking.is_none()
            && matches!(self.kind, Kind::Rotation | Kind::Flip)
            && self.base.is_none()
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
        if self.asking.is_some() {
            return CmdResult::NeedPoint;
        }
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
        if self.asking.is_some() {
            return Some(self.accept(Some(text)));
        }
        if matches!(self.kind, Kind::Rotation | Kind::Flip) && self.base.is_none() {
            let asking = match text.trim().to_ascii_uppercase().as_str() {
                "N" | "NAME" => Asking::Name,
                "L" | "LABEL" => Asking::Label,
                "C" | "CHAIN" if self.kind == Kind::Rotation => Asking::Chain,
                "D" | "DESCRIPTION" => Asking::Description,
                "P" | "PALETTE" => Asking::Palette,
                "V" if self.kind == Kind::Rotation => Asking::ValueSetType,
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Point or option keyword required.").into_owned(),
                    ))
                }
            };
            match asking {
                Asking::Name => self.options.name = None,
                Asking::Label => self.options.label = None,
                Asking::Chain => self.options.chain = None,
                Asking::Description => self.options.description = None,
                Asking::Palette => self.options.palette = None,
                Asking::ValueSetType => {
                    self.options.value_set = None;
                    self.increment = None;
                    self.minimum = None;
                }
                Asking::ValueSetIncrement
                | Asking::ValueSetMinimum
                | Asking::ValueSetMaximum
                | Asking::ValueSetList => unreachable!(),
            }
            self.asking = Some(asking);
            return Some(CmdResult::NeedPoint);
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
            let direction = |angle: f64| DVec3::new(angle.cos(), angle.sin(), 0.0) * radius;
            if self.second.is_none() || self.asking_base_angle() {
                let angle = (point.y - base.y).atan2(point.x - base.x);
                lines.push(vec![base.to_array(), (base + direction(angle)).to_array()]);
                lines.push(
                    (0..=64)
                        .map(|i| {
                            let a = std::f64::consts::TAU * i as f64 / 64.0;
                            (base + direction(a)).to_array()
                        })
                        .collect(),
                );
            } else {
                let rotation = self
                    .rotation
                    .unwrap_or_else(|| (point.y - base.y).atan2(point.x - base.x));
                let reference = base + direction(self.base_angle);
                let end = base + direction(self.base_angle + rotation);
                lines.push(vec![base.to_array(), reference.to_array()]);
                lines.push(vec![base.to_array(), end.to_array()]);
                let native = |p: DVec3| Vector3::new(p.x, p.y, p.z);
                if let Some((u, v, arc_radius, sweep)) = block_orientation_parameter::rotation_arc(
                    native(base),
                    native(end),
                    native(reference),
                ) {
                    let u = DVec3::new(u.x, u.y, u.z);
                    let v = DVec3::new(v.x, v.y, v.z);
                    lines.push(
                        (0..=48)
                            .map(|step| {
                                let a = sweep * step as f64 / 48.0;
                                (base + (u * a.cos() + v * a.sin()) * arc_radius).to_array()
                            })
                            .collect(),
                    );
                }
            }
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
        if self.asking.is_some() {
            return self.accept(None);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rotation() -> OrientationParameterCommand {
        OrientationParameterCommand::new(0, Handle::new(1), Kind::Rotation)
    }

    #[test]
    fn a_rotation_base_point_prompt_lists_the_options_it_accepts() {
        assert_eq!(
            rotation().prompt(),
            "BPARAMETER Rotation  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
        );
    }

    #[test]
    fn a_rotation_value_set_asks_its_angle_questions() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("V"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter angle value set type [None/List/Increment] <None>:"
        );
        assert_eq!(command.input_kind(), InputKind::SingleToken);
        assert!(matches!(
            command.on_text_input("I"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter angle increment:");
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input("5"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter minimum angle:");
        assert!(matches!(
            command.on_text_input("0"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter maximum angle:");
        assert!(matches!(
            command.on_text_input("360"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.value_set,
            Some(ValueSet::Increment {
                increment: 5f64.to_radians(),
                minimum: 0.0,
                maximum: 360f64.to_radians()
            }),
            "the answered degrees are stored as the angles the parameter uses"
        );
        assert!(command.asking.is_none());
        assert_eq!(
            command.prompt(),
            "BPARAMETER Rotation  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
        );
    }

    #[test]
    fn a_rotation_value_set_list_takes_comma_separated_degrees() {
        let mut command = rotation();
        for answer in ["V", "L"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert_eq!(
            command.prompt(),
            "Enter list of angle values (separated by commas):"
        );
        assert!(matches!(
            command.on_text_input("90, 180 ,270"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.value_set,
            Some(ValueSet::List(vec![
                90f64.to_radians(),
                180f64.to_radians(),
                270f64.to_radians()
            ]))
        );
    }

    #[test]
    fn an_empty_rotation_value_set_type_answer_clears_the_set() {
        let mut command = rotation();
        for answer in ["V", "I", "5", "0", "360"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert!(matches!(
            command.on_text_input("V"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.value_set, Some(ValueSet::None));
        assert!(command.asking.is_none());
    }

    #[test]
    fn a_rotation_value_set_type_outside_the_three_is_refused() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("V"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Both"),
            Some(CmdResult::ReportError(reason)) if reason == "Invalid option keyword."
        ));
        assert!(command.asking.is_some());
    }

    #[test]
    fn a_rotation_value_set_number_refuses_text() {
        let mut command = rotation();
        for answer in ["V", "I"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert!(matches!(
            command.on_text_input("wide"),
            Some(CmdResult::ReportError(reason)) if reason == "Requires numeric value."
        ));
        assert_eq!(command.prompt(), "Enter angle increment:");
        assert_eq!(command.options.value_set, None);
    }

    #[test]
    fn a_value_set_answer_on_a_flip_is_not_an_option() {
        let Some(CmdResult::ReportError(error)) =
            OrientationParameterCommand::new(0, Handle::new(1), Kind::Flip).on_text_input("V")
        else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Point or option keyword required.");
    }

    #[test]
    fn a_rotation_takes_a_name_and_a_property_label() {
        let mut command = rotation()
            .with_name("Rotation1".into())
            .with_label("Angle".into());
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter parameter name <Rotation1>:");
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.name.as_deref(),
            Some("L"),
            "a keyword typed at a text question is that text"
        );
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Body"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.name.as_deref(), Some("Body"));
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter rotation property label <Angle>:");
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.label, None, "Enter keeps the default label");
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Turn"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.label.as_deref(), Some("Turn"));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Rotation  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:",
            "both questions return to the placement prompt"
        );
    }

    #[test]
    fn a_flip_base_point_prompt_lists_the_options_it_accepts() {
        assert_eq!(
            OrientationParameterCommand::new(0, Handle::new(1), Kind::Flip).prompt(),
            "BPARAMETER Flip  Specify base point of reflection line or [Name/Label/Description/Palette]:"
        );
    }

    #[test]
    fn a_flip_takes_a_name_and_a_property_label() {
        let mut command = OrientationParameterCommand::new(0, Handle::new(1), Kind::Flip)
            .with_name("Flip".into())
            .with_label("Flip state1".into());
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter parameter name <Flip>:");
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input("Body"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.name.as_deref(), Some("Body"));
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter Flip property label <Flip state1>:");
        assert!(matches!(
            command.on_text_input("Mirror"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.label.as_deref(), Some("Mirror"));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Flip  Specify base point of reflection line or [Name/Label/Description/Palette]:",
            "both questions return to the placement prompt"
        );
    }

    #[test]
    fn a_flip_refuses_a_chain_answer_it_does_not_offer() {
        let Some(CmdResult::ReportError(error)) =
            OrientationParameterCommand::new(0, Handle::new(1), Kind::Flip).on_text_input("C")
        else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Point or option keyword required.");
    }

    #[test]
    fn other_orientation_kinds_keep_their_single_instruction() {
        assert_eq!(
            OrientationParameterCommand::new(0, Handle::new(1), Kind::Alignment).prompt(),
            "BPARAMETER Alignment  Specify parameter base point:"
        );
    }

    #[test]
    fn a_chain_answer_asks_whether_associated_actions_run() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
        assert!(matches!(
            command.on_text_input("Yes"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.chain, Some(true));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Rotation  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
        );
    }

    #[test]
    fn an_empty_chain_answer_keeps_actions_unchained() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.chain, None);
    }

    #[test]
    fn a_description_answer_takes_its_text_as_written() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter property description:");
        assert!(matches!(
            command.on_text_input("  turns the frame  "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.description.as_deref(),
            Some("  turns the frame  ")
        );
    }

    #[test]
    fn a_palette_answer_lists_in_the_palette_by_default() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Display property in Properties palette? [Yes/No] <Yes>:"
        );
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.palette, None);
    }

    #[test]
    fn a_palette_answer_can_hide_the_property() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("PALETTE"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.palette, Some(false));
    }

    #[test]
    fn a_yes_or_no_answer_outside_yes_or_no_is_refused() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        let Some(CmdResult::ReportError(error)) = command.on_text_input("maybe") else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Enter Yes or No.");
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
    }

    #[test]
    fn text_that_is_not_an_option_is_refused_with_the_prompt() {
        let Some(CmdResult::ReportError(error)) = rotation().on_text_input("Z") else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Point or option keyword required.");
    }

    #[test]
    fn options_can_be_answered_in_any_order() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("audit note"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("CHAIN"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Yes"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.chain, Some(true));
        assert_eq!(command.options.description.as_deref(), Some("audit note"));
        assert_eq!(command.options.palette, Some(false));
    }

    #[test]
    fn a_click_while_an_option_is_open_keeps_the_question() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_point(DVec3::new(1., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
    }

    #[test]
    fn a_description_question_takes_typed_text() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.input_kind(), InputKind::FreeText);
    }

    #[test]
    fn a_keyword_after_the_base_point_is_not_an_option() {
        let mut command = rotation();
        assert!(matches!(
            command.on_point(DVec3::new(1., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert!(command.on_text_input("C").is_none());
    }

    #[test]
    fn reopening_an_option_keeps_the_default_it_shows() {
        let mut command = rotation();
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.palette, None);
    }
}
