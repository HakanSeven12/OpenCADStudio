//! Authoring shared by Linear, Polar and XY parameters: Linear and Polar take a
//! label pick, XY asks for its grip count and labels itself at the endpoint.
use crate::command::{CadCommand, CmdResult, InputKind};
use crate::scene::{
    block_distance_parameter::{self, BaseLocation, Kind, ValueSet},
    WireModel,
};
use document::{types::Vector3, Handle};
use glam::DVec3;

/// Values answered at the base-point prompt before the parameter exists.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub name: Option<String>,
    pub chain: Option<bool>,
    pub description: Option<String>,
    pub palette: Option<bool>,
    pub distance_label: Option<String>,
    pub angle_label: Option<String>,
    pub horizontal_label: Option<String>,
    pub vertical_label: Option<String>,
    pub base_location: Option<BaseLocation>,
    pub value_set: Option<ValueSet>,
    pub angle_value_set: Option<ValueSet>,
    pub vertical_value_set: Option<ValueSet>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asking {
    Name,
    Chain,
    Description,
    Palette,
    DistanceLabel,
    AngleLabel,
    VerticalLabel,
    Base,
    ValueSetType,
    ValueSetIncrement,
    ValueSetMinimum,
    ValueSetMaximum,
    ValueSetList,
    AngleValueSetType,
    AngleValueSetIncrement,
    AngleValueSetMinimum,
    AngleValueSetMaximum,
    AngleValueSetList,
    VerticalValueSetType,
    VerticalValueSetIncrement,
    VerticalValueSetMinimum,
    VerticalValueSetMaximum,
    VerticalValueSetList,
}

impl Asking {
    /// The creation default the question's Enter answer stands for.
    fn default_answer(self) -> bool {
        self == Self::Palette
    }
    fn is_default(self, value: bool) -> bool {
        value == self.default_answer()
    }
}

pub struct DistanceParameterCommand {
    document_id: u64,
    definition: Handle,
    kind: Kind,
    base: Option<DVec3>,
    end: Option<DVec3>,
    grips: Option<usize>,
    asking: Option<Asking>,
    options: Options,
    name_default: String,
    distance_label_default: String,
    angle_label_default: String,
    horizontal_label_default: String,
    vertical_label_default: String,
    increment: Option<f64>,
    minimum: Option<f64>,
    angle_increment: Option<f64>,
    angle_minimum: Option<f64>,
}

impl DistanceParameterCommand {
    pub(crate) fn new(document_id: u64, definition: Handle, kind: Kind) -> Self {
        Self {
            document_id,
            definition,
            kind,
            base: None,
            end: None,
            grips: None,
            asking: None,
            options: Options::default(),
            name_default: String::new(),
            distance_label_default: String::new(),
            angle_label_default: String::new(),
            horizontal_label_default: String::new(),
            vertical_label_default: String::new(),
            increment: None,
            minimum: None,
            angle_increment: None,
            angle_minimum: None,
        }
    }
    /// A Polar parameter asks for the angle set once the distance set is
    /// answered, an XY parameter for the vertical set.
    fn after_distance_set(&self) -> Option<Asking> {
        match self.kind {
            Kind::Polar => Some(Asking::AngleValueSetType),
            Kind::XY => Some(Asking::VerticalValueSetType),
            _ => None,
        }
    }
    pub(crate) fn with_name(mut self, name: String) -> Self {
        self.name_default = name;
        self
    }
    pub(crate) fn with_labels(mut self, distance: String, angle: String) -> Self {
        self.distance_label_default = distance;
        self.angle_label_default = angle;
        self
    }
    pub(crate) fn with_xy_labels(mut self, horizontal: String, vertical: String) -> Self {
        self.horizontal_label_default = horizontal;
        self.vertical_label_default = vertical;
        self
    }
    fn accept(&mut self, text: Option<&str>) -> CmdResult {
        let asking = self.asking.expect("an option question is open");
        if matches!(
            asking,
            Asking::DistanceLabel | Asking::AngleLabel | Asking::VerticalLabel
        ) {
            let value = text.unwrap_or("").trim();
            match asking {
                Asking::DistanceLabel if self.kind == Kind::XY => {
                    if !value.is_empty()
                        && !value.eq_ignore_ascii_case(&self.horizontal_label_default)
                    {
                        self.options.horizontal_label = Some(value.to_owned());
                    }
                    self.asking = Some(Asking::VerticalLabel);
                }
                Asking::DistanceLabel => {
                    if !value.is_empty() && value != self.distance_label_default {
                        self.options.distance_label = Some(value.to_owned());
                    }
                    // Only a Polar parameter asks for an angle label as well.
                    self.asking = (self.kind == Kind::Polar).then_some(Asking::AngleLabel);
                }
                Asking::VerticalLabel => {
                    let horizontal = self
                        .options
                        .horizontal_label
                        .as_deref()
                        .unwrap_or(&self.horizontal_label_default);
                    let resolved = if value.is_empty() {
                        self.vertical_label_default.as_str()
                    } else {
                        value
                    };
                    if resolved.eq_ignore_ascii_case(horizontal) {
                        return CmdResult::ReportError(
                            crate::t!("The horizontal and vertical labels must differ.")
                                .into_owned(),
                        );
                    }
                    if !value.is_empty()
                        && !value.eq_ignore_ascii_case(&self.vertical_label_default)
                    {
                        self.options.vertical_label = Some(value.to_owned());
                    }
                    self.asking = None;
                }
                Asking::AngleLabel => {
                    let distance = self
                        .options
                        .distance_label
                        .as_deref()
                        .unwrap_or(&self.distance_label_default);
                    if value.eq_ignore_ascii_case(distance) {
                        return CmdResult::ReportError(
                            crate::t!("The distance and angle labels must differ.").into_owned(),
                        );
                    }
                    if !value.is_empty() && value != self.angle_label_default {
                        self.options.angle_label = Some(value.to_owned());
                    }
                    self.asking = None;
                }
                _ => unreachable!(),
            }
            return CmdResult::NeedPoint;
        }
        if asking == Asking::Name {
            let value = text.unwrap_or("").trim();
            if !value.is_empty() && !value.eq_ignore_ascii_case(&self.name_default) {
                self.options.name = Some(value.to_owned());
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
        if asking == Asking::Base {
            match text
                .map(str::trim)
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str()
            {
                "" | "S" | "STARTPOINT" => {
                    self.options.base_location = Some(BaseLocation::Startpoint)
                }
                "M" | "MIDPOINT" => {
                    self.options.base_location = Some(BaseLocation::Midpoint);
                }
                _ => {
                    return CmdResult::ReportError(
                        crate::t!("Invalid option keyword.").into_owned(),
                    )
                }
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
                    self.asking = self.after_distance_set();
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
                self.options.value_set = Some(ValueSet::List(values));
                self.asking = self.after_distance_set();
                return CmdResult::NeedPoint;
            }
            let number = answer.parse::<f64>().ok().filter(|v| v.is_finite());
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
                    self.asking = self.after_distance_set();
                }
                _ => unreachable!(),
            }
            return CmdResult::NeedPoint;
        }
        if asking == Asking::AngleValueSetType {
            match text
                .map(str::trim)
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str()
            {
                "" | "N" | "NONE" => {
                    self.options.angle_value_set = Some(ValueSet::None);
                    self.asking = None;
                }
                "I" | "INCREMENT" => {
                    self.options.angle_value_set = None;
                    self.angle_increment = None;
                    self.angle_minimum = None;
                    self.asking = Some(Asking::AngleValueSetIncrement);
                }
                "L" | "LIST" => {
                    self.options.angle_value_set = None;
                    self.asking = Some(Asking::AngleValueSetList);
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
            Asking::AngleValueSetIncrement
                | Asking::AngleValueSetMinimum
                | Asking::AngleValueSetMaximum
                | Asking::AngleValueSetList
        ) {
            let answer = text.unwrap_or("").trim();
            let radians = |value: f64| value.to_radians();
            if asking == Asking::AngleValueSetList {
                let values = answer
                    .split(',')
                    .map(|part| part.trim().parse::<f64>().map(radians))
                    .collect::<Result<Vec<_>, _>>()
                    .ok()
                    .filter(|values| !values.is_empty() && values.iter().all(|v| v.is_finite()));
                let Some(values) = values else {
                    return CmdResult::ReportError(
                        crate::t!("Requires numeric value.").into_owned(),
                    );
                };
                self.options.angle_value_set = Some(ValueSet::List(values));
                self.asking = None;
                return CmdResult::NeedPoint;
            }
            let number = answer
                .parse::<f64>()
                .ok()
                .filter(|v| v.is_finite())
                .map(radians);
            let Some(number) = number else {
                return CmdResult::ReportError(crate::t!("Requires numeric value.").into_owned());
            };
            match asking {
                Asking::AngleValueSetIncrement => {
                    self.angle_increment = Some(number);
                    self.asking = Some(Asking::AngleValueSetMinimum);
                }
                Asking::AngleValueSetMinimum => {
                    self.angle_minimum = Some(number);
                    self.asking = Some(Asking::AngleValueSetMaximum);
                }
                Asking::AngleValueSetMaximum => {
                    self.options.angle_value_set = Some(ValueSet::Increment {
                        increment: self.angle_increment.expect("the increment was answered"),
                        minimum: self.angle_minimum.expect("the minimum was answered"),
                        maximum: number,
                    });
                    self.asking = None;
                }
                _ => unreachable!(),
            }
            return CmdResult::NeedPoint;
        }
        if asking == Asking::VerticalValueSetType {
            match text
                .map(str::trim)
                .unwrap_or("")
                .to_ascii_uppercase()
                .as_str()
            {
                "" | "N" | "NONE" => {
                    self.options.vertical_value_set = Some(ValueSet::None);
                    self.asking = None;
                }
                "I" | "INCREMENT" => {
                    self.options.vertical_value_set = None;
                    self.increment = None;
                    self.minimum = None;
                    self.asking = Some(Asking::VerticalValueSetIncrement);
                }
                "L" | "LIST" => {
                    self.options.vertical_value_set = None;
                    self.asking = Some(Asking::VerticalValueSetList);
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
            Asking::VerticalValueSetIncrement
                | Asking::VerticalValueSetMinimum
                | Asking::VerticalValueSetMaximum
                | Asking::VerticalValueSetList
        ) {
            let answer = text.unwrap_or("").trim();
            if asking == Asking::VerticalValueSetList {
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
                self.options.vertical_value_set = Some(ValueSet::List(values));
                self.asking = None;
                return CmdResult::NeedPoint;
            }
            let number = answer.parse::<f64>().ok().filter(|v| v.is_finite());
            let Some(number) = number else {
                return CmdResult::ReportError(crate::t!("Requires numeric value.").into_owned());
            };
            match asking {
                Asking::VerticalValueSetIncrement => {
                    self.increment = Some(number);
                    self.asking = Some(Asking::VerticalValueSetMinimum);
                }
                Asking::VerticalValueSetMinimum => {
                    self.minimum = Some(number);
                    self.asking = Some(Asking::VerticalValueSetMaximum);
                }
                Asking::VerticalValueSetMaximum => {
                    self.options.vertical_value_set = Some(ValueSet::Increment {
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
                Asking::Name
                | Asking::Description
                | Asking::DistanceLabel
                | Asking::AngleLabel
                | Asking::VerticalLabel
                | Asking::Base
                | Asking::ValueSetType
                | Asking::ValueSetIncrement
                | Asking::ValueSetMinimum
                | Asking::ValueSetMaximum
                | Asking::ValueSetList
                | Asking::AngleValueSetType
                | Asking::AngleValueSetIncrement
                | Asking::AngleValueSetMinimum
                | Asking::AngleValueSetMaximum
                | Asking::AngleValueSetList
                | Asking::VerticalValueSetType
                | Asking::VerticalValueSetIncrement
                | Asking::VerticalValueSetMinimum
                | Asking::VerticalValueSetMaximum
                | Asking::VerticalValueSetList => unreachable!(),
            }
        }
        self.asking = None;
        CmdResult::NeedPoint
    }
    fn asking_grips(&self) -> bool {
        self.kind == Kind::XY && self.end.is_some() && self.grips.is_none()
    }
    /// The question the open option asks, carrying the defaults it offers.
    fn question(&self, asking: Asking) -> String {
        match asking {
            Asking::Name => crate::tf!("Enter parameter name <{}>:", self.name_default).into_owned(),
            Asking::Chain => crate::t!(
                "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
            )
            .into_owned(),
            Asking::Description => crate::t!("Enter property description:").into_owned(),
            Asking::Palette => {
                crate::t!("Display property in Properties palette? [Yes/No] <Yes>:").into_owned()
            }
            Asking::DistanceLabel if self.kind == Kind::XY => crate::tf!(
                "Enter horizontal distance property label <{}>:",
                self.horizontal_label_default
            )
            .into_owned(),
            Asking::DistanceLabel => crate::tf!(
                "Enter distance property label <{}>:",
                self.distance_label_default
            )
            .into_owned(),
            Asking::AngleLabel => crate::tf!(
                "Enter angle property label <{}>:",
                self.angle_label_default
            )
            .into_owned(),
            Asking::VerticalLabel => crate::tf!(
                "Enter vertical distance property label <{}>:",
                self.vertical_label_default
            )
            .into_owned(),
            Asking::Base => {
                crate::t!("Enter base location [Startpoint/Midpoint] <Startpoint>:").into_owned()
            }
            Asking::ValueSetType if self.kind == Kind::XY => crate::t!(
                "Enter horizontal value set type [None/List/Increment] <None>:"
            )
            .into_owned(),
            Asking::ValueSetType => crate::t!(
                "Enter distance value set type [None/List/Increment] <None>:"
            )
            .into_owned(),
            Asking::ValueSetIncrement => crate::t!("Enter distance increment:").into_owned(),
            Asking::ValueSetMinimum => crate::t!("Enter minimum distance:").into_owned(),
            Asking::ValueSetMaximum => crate::t!("Enter maximum distance:").into_owned(),
            Asking::ValueSetList => {
                crate::t!("Enter list of distance values (separated by commas):").into_owned()
            }
            Asking::AngleValueSetType => crate::t!(
                "Enter angle value set type [None/List/Increment] <None>:"
            )
            .into_owned(),
            Asking::AngleValueSetIncrement => crate::t!("Enter angle increment:").into_owned(),
            Asking::AngleValueSetMinimum => crate::t!("Enter minimum angle:").into_owned(),
            Asking::AngleValueSetMaximum => crate::t!("Enter maximum angle:").into_owned(),
            Asking::AngleValueSetList => {
                crate::t!("Enter list of angle values (separated by commas):").into_owned()
            }
            Asking::VerticalValueSetType => crate::t!(
                "Enter vertical value set type [None/List/Increment] <None>:"
            )
            .into_owned(),
            Asking::VerticalValueSetIncrement => {
                crate::t!("Enter distance increment:").into_owned()
            }
            Asking::VerticalValueSetMinimum => crate::t!("Enter minimum distance:").into_owned(),
            Asking::VerticalValueSetMaximum => crate::t!("Enter maximum distance:").into_owned(),
            Asking::VerticalValueSetList => {
                crate::t!("Enter list of distance values (separated by commas):").into_owned()
            }
        }
    }
    /// The kinds that offer a name and a label: Linear, Polar and XY. Linear
    /// asks for a distance label only, Polar for a distance label then an angle
    /// label, XY for a horizontal then a vertical distance label.
    fn accepts_label(&self) -> bool {
        matches!(self.kind, Kind::Linear | Kind::Polar | Kind::XY)
    }
    fn finish(&self, label: DVec3, grips: Option<usize>) -> CmdResult {
        CmdResult::PlaceBlockDistanceParameter {
            document_id: self.document_id,
            definition: self.definition,
            kind: self.kind,
            base: self.base.unwrap(),
            end: self.end.unwrap(),
            label,
            grips,
            options: self.options.clone(),
        }
    }
    fn commit_grips(&self, grips: usize) -> CmdResult {
        let end = self.end.unwrap();
        self.finish(end, Some(grips))
    }
}

impl CadCommand for DistanceParameterCommand {
    fn name(&self) -> &'static str {
        "BPARAMETER"
    }
    fn prompt(&self) -> String {
        if let Some(asking) = self.asking {
            return self.question(asking);
        }
        let step = if self.base.is_none() {
            if self.kind == Kind::Linear {
                "Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:"
            } else if self.accepts_label() {
                "Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
            } else {
                "Specify base point or [Chain/Description/Palette]:"
            }
        } else if self.end.is_none() {
            "Specify parameter endpoint:"
        } else if self.asking_grips() {
            "Specify number of grips [0/1/2/4] <1>:"
        } else {
            "Specify parameter label location:"
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
                Asking::Name
                | Asking::Description
                | Asking::DistanceLabel
                | Asking::AngleLabel
                | Asking::VerticalLabel
                | Asking::ValueSetIncrement
                | Asking::ValueSetMinimum
                | Asking::ValueSetMaximum
                | Asking::ValueSetList
                | Asking::AngleValueSetIncrement
                | Asking::AngleValueSetMinimum
                | Asking::AngleValueSetMaximum
                | Asking::AngleValueSetList
                | Asking::VerticalValueSetIncrement
                | Asking::VerticalValueSetMinimum
                | Asking::VerticalValueSetMaximum
                | Asking::VerticalValueSetList,
            ) => InputKind::FreeText,
            Some(_) => InputKind::SingleToken,
            None => InputKind::Point,
        }
    }
    fn point_step_accepts_keywords(&self) -> bool {
        self.asking.is_none() && self.base.is_none()
    }
    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if self.asking.is_some() {
            return CmdResult::NeedPoint;
        }
        if self.asking_grips() {
            return CmdResult::ReportError(crate::t!("Requires an integer value.").into_owned());
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
        let vector = |p: DVec3| Vector3::new(p.x, p.y, p.z);
        if let Some(error) = block_distance_parameter::points_error(
            self.kind,
            vector(base),
            vector(self.end.unwrap_or(point)),
            vector(point),
        ) {
            return CmdResult::ReportError(crate::t!(error).into_owned());
        }
        if self.end.is_none() {
            self.end = Some(point);
            return CmdResult::NeedPoint;
        }
        self.finish(point, None)
    }
    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        if self.asking.is_some() {
            return Some(self.accept(Some(text)));
        }
        if self.base.is_none() {
            let asking = match text.trim().to_ascii_uppercase().as_str() {
                "N" | "NAME" if self.accepts_label() => Asking::Name,
                "C" | "CHAIN" => Asking::Chain,
                "D" | "DESCRIPTION" => Asking::Description,
                "P" | "PALETTE" => Asking::Palette,
                "L" | "LABEL" if self.accepts_label() => Asking::DistanceLabel,
                "B" | "BASE" if self.kind == Kind::Linear => Asking::Base,
                "V" if matches!(self.kind, Kind::Linear | Kind::Polar | Kind::XY) => {
                    Asking::ValueSetType
                }
                _ => {
                    return Some(CmdResult::ReportError(
                        crate::t!("Point or option keyword required.").into_owned(),
                    ))
                }
            };
            match asking {
                Asking::Name => self.options.name = None,
                Asking::Chain => self.options.chain = None,
                Asking::Description => self.options.description = None,
                Asking::Palette => self.options.palette = None,
                Asking::DistanceLabel => {
                    self.options.distance_label = None;
                    self.options.angle_label = None;
                    self.options.horizontal_label = None;
                    self.options.vertical_label = None;
                }
                Asking::Base => self.options.base_location = None,
                Asking::ValueSetType => {
                    self.options.value_set = None;
                    self.increment = None;
                    self.minimum = None;
                    self.options.angle_value_set = None;
                    self.angle_increment = None;
                    self.angle_minimum = None;
                    self.options.vertical_value_set = None;
                    self.increment = None;
                    self.minimum = None;
                }
                Asking::AngleLabel | Asking::VerticalLabel => unreachable!(),
                Asking::ValueSetIncrement
                | Asking::ValueSetMinimum
                | Asking::ValueSetMaximum
                | Asking::ValueSetList
                | Asking::AngleValueSetType
                | Asking::AngleValueSetIncrement
                | Asking::AngleValueSetMinimum
                | Asking::AngleValueSetMaximum
                | Asking::AngleValueSetList
                | Asking::VerticalValueSetType
                | Asking::VerticalValueSetIncrement
                | Asking::VerticalValueSetMinimum
                | Asking::VerticalValueSetMaximum
                | Asking::VerticalValueSetList => unreachable!(),
            }
            self.asking = Some(asking);
            return Some(CmdResult::NeedPoint);
        }
        if !self.asking_grips() {
            return None;
        }
        let answer = text.trim();
        let grips = match answer {
            "" => 1,
            "0" => 0,
            "1" => 1,
            "2" => 2,
            "4" => 4,
            _ => {
                // Which refusal an answer gets depends on its form, not its
                // value: digits are a count this list happens not to offer,
                // anything else is no count at all. The widest integer type
                // keeps a long typed answer a count rather than a non-number.
                let refusal = if answer.parse::<i128>().is_ok() {
                    "Enter the number of grips: 0, 1, 2 or 4."
                } else {
                    "Requires an integer value."
                };
                return Some(CmdResult::ReportError(crate::t!(refusal).into_owned()));
            }
        };
        self.grips = Some(grips);
        Some(self.commit_grips(grips))
    }
    fn on_preview_wires(&mut self, point: DVec3) -> Vec<WireModel> {
        let Some(base) = self.base.filter(|_| point.is_finite()) else {
            return vec![];
        };
        let end = self.end.unwrap_or(point);
        let mut lines = vec![[base, end]];
        if self.kind == Kind::XY {
            lines = vec![
                [base, DVec3::new(end.x, base.y, base.z)],
                [base, DVec3::new(base.x, end.y, base.z)],
            ];
        }
        if self.end.is_some() && !self.asking_grips() {
            if self.kind == Kind::XY {
                let x = DVec3::new(base.x, point.y, base.z);
                let y = DVec3::new(point.x, base.y, base.z);
                lines.extend([
                    [base, x],
                    [x, DVec3::new(end.x, point.y, base.z)],
                    [base, y],
                    [y, DVec3::new(point.x, end.y, base.z)],
                ]);
            } else {
                let d = end - base;
                let n = DVec3::new(-d.y, d.x, 0.0).normalize_or_zero();
                let offset = n * (point - base).dot(n);
                lines = vec![
                    [base, base + offset],
                    [base + offset, end + offset],
                    [end + offset, end],
                ];
            }
        }
        lines
            .into_iter()
            .map(|line| {
                WireModel::solid_f64(
                    "preview_distance_parameter".into(),
                    line.into_iter().map(|point| point.to_array()).collect(),
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
        if self.asking_grips() {
            self.grips = Some(1);
            return self.commit_grips(1);
        }
        CmdResult::Cancel
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_grip_count() -> DistanceParameterCommand {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY);
        for point in [DVec3::ZERO, DVec3::new(6., 2., 0.)] {
            assert!(
                matches!(command.on_point(point), CmdResult::NeedPoint),
                "the base point and the endpoint are picks"
            );
        }
        assert!(command.asking_grips(), "the XY command now wants a count");
        command
    }

    #[test]
    fn a_linear_label_asks_only_for_the_distance_label() {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::Linear)
            .with_name("Distance3".into())
            .with_labels("Distance3".into(), "Angle3".into());
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter distance property label <Distance3>:"
        );
        assert!(matches!(
            command.on_text_input("Width"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:",
            "a Linear parameter asks for one label, not a distance and an angle"
        );
        assert_eq!(command.options.distance_label.as_deref(), Some("Width"));
        assert!(command.options.angle_label.is_none());
        assert!(command.asking.is_none(), "the label question is closed");
    }

    fn linear() -> DistanceParameterCommand {
        DistanceParameterCommand::new(0, Handle::new(1), Kind::Linear)
    }

    #[test]
    fn a_linear_base_location_asks_startpoint_or_midpoint() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("B"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter base location [Startpoint/Midpoint] <Startpoint>:"
        );
        assert_eq!(command.input_kind(), InputKind::SingleToken);
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(
            command.options.base_location,
            Some(BaseLocation::Startpoint),
            "Enter keeps startpoint"
        );
        assert!(command.asking.is_none());
        assert!(matches!(
            command.on_text_input("B"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("S"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.base_location,
            Some(BaseLocation::Startpoint)
        );
        assert!(matches!(
            command.on_text_input("B"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("X"),
            Some(CmdResult::ReportError(reason)) if reason == "Invalid option keyword."
        ));
        assert_eq!(
            command.prompt(),
            "Enter base location [Startpoint/Midpoint] <Startpoint>:",
            "a refused answer leaves the question open"
        );
        assert!(matches!(
            command.on_text_input("M"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.base_location, Some(BaseLocation::Midpoint));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:",
            "the base question returns to placement"
        );
    }

    #[test]
    fn a_value_set_increment_asks_three_numbers() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("V"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter distance value set type [None/List/Increment] <None>:"
        );
        assert_eq!(command.input_kind(), InputKind::SingleToken);
        assert!(matches!(
            command.on_text_input("I"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter distance increment:");
        assert!(matches!(
            command.on_text_input("5"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter minimum distance:");
        assert!(matches!(
            command.on_text_input("0"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter maximum distance:");
        assert!(matches!(
            command.on_text_input("100"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.value_set,
            Some(ValueSet::Increment {
                increment: 5.0,
                minimum: 0.0,
                maximum: 100.0
            })
        );
        assert!(command.asking.is_none());
        assert_eq!(
            command.prompt(),
            "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:"
        );
    }

    #[test]
    fn a_value_set_increment_keeps_negative_and_inverted_numbers_as_answered() {
        let mut command = linear();
        for answer in ["V", "I", "-5", "10", "0"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert_eq!(
            command.options.value_set,
            Some(ValueSet::Increment {
                increment: -5.0,
                minimum: 10.0,
                maximum: 0.0
            }),
            "the three numbers are stored as answered, even when they do not form a range"
        );
        assert!(command.asking.is_none());
        assert_eq!(
            command.prompt(),
            "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:"
        );
    }

    #[test]
    fn a_value_set_list_takes_comma_separated_numbers() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("V"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter list of distance values (separated by commas):"
        );
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input(" 1, 2.5 ,3 "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.value_set,
            Some(ValueSet::List(vec![1.0, 2.5, 3.0]))
        );
        assert!(command.asking.is_none());
    }

    #[test]
    fn an_empty_value_set_type_answer_clears_the_set() {
        let mut command = linear();
        for answer in ["V", "I", "5", "0", "100"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert!(command.options.value_set.is_some());
        assert!(matches!(
            command.on_text_input("V"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.value_set, Some(ValueSet::None));
        assert!(command.asking.is_none());
    }

    #[test]
    fn a_value_set_number_question_refuses_text() {
        let mut command = linear();
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
        assert_eq!(
            command.prompt(),
            "Enter distance increment:",
            "the question stays open"
        );
        assert_eq!(command.options.value_set, None);
    }

    #[test]
    fn a_value_set_list_refuses_text() {
        let mut command = linear();
        for answer in ["V", "L"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert!(matches!(
            command.on_text_input("1,,2"),
            Some(CmdResult::ReportError(reason)) if reason == "Requires numeric value."
        ));
        assert_eq!(command.options.value_set, None);
        assert!(command.asking.is_some(), "the list question stays open");
    }

    #[test]
    fn a_value_set_type_outside_the_three_is_refused() {
        let mut command = linear();
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
    fn a_distance_base_prompt_lists_the_options_it_accepts() {
        for (kind, prompt) in [
            (
                Kind::Linear,
                "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:",
            ),
            (
                Kind::Polar,
                "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:",
            ),
            (
                Kind::XY,
                "BPARAMETER XY  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:",
            ),
        ] {
            assert_eq!(
                DistanceParameterCommand::new(0, Handle::new(1), kind).prompt(),
                prompt
            );
        }
    }

    fn polar() -> DistanceParameterCommand {
        DistanceParameterCommand::new(0, Handle::new(1), Kind::Polar)
            .with_name("Polar1".into())
            .with_labels("Distance2".into(), "Angle2".into())
    }

    #[test]
    fn a_name_answer_asks_for_the_parameter_name() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter parameter name <Polar1>:");
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input("  P9  "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
        );
        assert_eq!(command.options.name.as_deref(), Some("P9"));
    }

    #[test]
    fn an_empty_name_answer_keeps_the_default() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("Name"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.name, None);
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Polar1"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.name, None);
    }

    #[test]
    fn every_distance_type_offers_a_name() {
        for kind in [Kind::Linear, Kind::Polar, Kind::XY] {
            let mut command = DistanceParameterCommand::new(0, Handle::new(1), kind);
            assert!(
                matches!(command.on_text_input("N"), Some(CmdResult::NeedPoint)),
                "{} offers a name",
                kind.name()
            );
            assert_eq!(command.input_kind(), InputKind::FreeText);
        }
    }

    #[test]
    fn the_name_travels_with_the_placement() {
        let mut command = polar();
        for answer in ["N", "P9"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        for point in [DVec3::ZERO, DVec3::new(4., 0., 0.), DVec3::new(1., 1., 0.)] {
            assert!(matches!(
                command.on_point(point),
                CmdResult::NeedPoint | CmdResult::PlaceBlockDistanceParameter { .. }
            ));
        }
        assert!(matches!(
            command.finish(DVec3::new(1., 1., 0.), None),
            CmdResult::PlaceBlockDistanceParameter { options, .. }
                if options.name.as_deref() == Some("P9")
        ));
    }

    #[test]
    fn a_label_answer_asks_both_property_labels_in_turn() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter distance property label <Distance2>:"
        );
        assert!(matches!(
            command.on_text_input("Dist9"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER Polar  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
        );
        assert_eq!(command.options.distance_label.as_deref(), Some("Dist9"));
        assert_eq!(command.options.angle_label.as_deref(), Some("C"));
    }

    #[test]
    fn an_empty_label_answer_keeps_the_default() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.distance_label, None);
        assert_eq!(command.options.angle_label, None);
    }

    #[test]
    fn a_label_question_takes_typed_text() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.input_kind(), InputKind::FreeText);
    }

    #[test]
    fn a_pick_at_a_label_question_keeps_the_question() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_point(DVec3::ONE), CmdResult::NeedPoint));
        assert_eq!(
            command.prompt(),
            "Enter distance property label <Distance2>:"
        );
        assert!(matches!(
            command.on_text_input("Dist9"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_point(DVec3::ONE), CmdResult::NeedPoint));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert_eq!(command.options.distance_label.as_deref(), Some("Dist9"));
    }

    #[test]
    fn an_angle_label_equal_to_the_distance_label_is_refused() {
        let mut command = polar();
        for answer in ["L", "Dist9"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert!(matches!(
            command.on_text_input("dist9"),
            Some(CmdResult::ReportError(reason))
                if reason == "The distance and angle labels must differ."
        ));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
        assert_eq!(command.options.angle_label, None);
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.angle_label.as_deref(), Some("C"));
    }

    #[test]
    fn an_angle_label_equal_to_the_offered_default_is_refused() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(
            command.on_text_input("Distance2"),
            Some(CmdResult::ReportError(reason))
                if reason == "The distance and angle labels must differ."
        ));
        assert_eq!(command.prompt(), "Enter angle property label <Angle2>:");
    }

    #[test]
    fn a_label_answer_is_trimmed() {
        let mut command = polar();
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("  Dist9  "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.distance_label.as_deref(), Some("Dist9"));
        assert!(matches!(
            command.on_text_input("   "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.options.angle_label, None);
    }

    #[test]
    fn reopening_the_label_option_clears_both_answers() {
        let mut command = polar();
        for answer in ["L", "Dist9", "C"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter distance property label <Distance2>:"
        );
        assert_eq!(command.options.distance_label, None);
        assert_eq!(command.options.angle_label, None);
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.distance_label, None);
        assert_eq!(command.options.angle_label, None);
    }

    #[test]
    fn an_xy_takes_a_name_and_two_distance_labels() {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY)
            .with_name("XY2".into())
            .with_xy_labels("X Distance3".into(), "Y Distance3".into());
        assert!(matches!(
            command.on_text_input("N"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter parameter name <XY2>:");
        assert!(matches!(
            command.on_text_input(" Frame "),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter horizontal distance property label <X Distance3>:"
        );
        assert!(matches!(
            command.on_text_input("Width X"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "Enter vertical distance property label <Y Distance3>:"
        );
        assert!(matches!(
            command.on_text_input("Width Y"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.prompt(),
            "BPARAMETER XY  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
        );
        assert_eq!(command.options.name.as_deref(), Some("Frame"));
        assert_eq!(command.options.horizontal_label.as_deref(), Some("Width X"));
        assert_eq!(command.options.vertical_label.as_deref(), Some("Width Y"));
    }

    #[test]
    fn an_xy_keeps_the_offered_defaults_when_the_answers_are_empty() {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY)
            .with_name("XY2".into())
            .with_xy_labels("X Distance3".into(), "Y Distance3".into());
        for keyword in ["N", "L"] {
            assert!(matches!(
                command.on_text_input(keyword),
                Some(CmdResult::NeedPoint)
            ));
            assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        }
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.name, None);
        assert_eq!(command.options.horizontal_label, None);
        assert_eq!(command.options.vertical_label, None);
    }

    #[test]
    fn an_xy_refuses_a_vertical_label_that_repeats_the_horizontal_one() {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY)
            .with_name("XY2".into())
            .with_xy_labels("X Distance3".into(), "Y Distance3".into());
        assert!(matches!(
            command.on_text_input("L"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Width"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input(" width "),
            Some(CmdResult::ReportError(reason))
                if reason == "The horizontal and vertical labels must differ."
        ));
        assert_eq!(command.options.vertical_label, None);
        assert_eq!(
            command.prompt(),
            "Enter vertical distance property label <Y Distance3>:",
            "the question stays open for a label that differs"
        );
    }

    #[test]
    fn an_xy_refuses_a_horizontal_label_that_collides_with_the_offered_vertical_one() {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY)
            .with_name("XY2".into())
            .with_xy_labels("X Distance3".into(), "Y Distance3".into());
        for answer in ["L", "Y Distance3"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert!(matches!(
            command.on_enter(),
            CmdResult::ReportError(reason)
                if reason == "The horizontal and vertical labels must differ."
        ));
        assert_eq!(command.options.vertical_label, None);
    }

    #[test]
    fn an_xy_reads_the_offered_default_labels_in_any_case() {
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY)
            .with_name("XY2".into())
            .with_xy_labels("X Distance3".into(), "Y Distance3".into());
        for answer in ["L", "x distance3", "y distance3"] {
            assert!(matches!(
                command.on_text_input(answer),
                Some(CmdResult::NeedPoint)
            ));
        }
        assert_eq!(command.options.horizontal_label, None);
        assert_eq!(command.options.vertical_label, None);
        assert_eq!(
            command.prompt(),
            "BPARAMETER XY  Specify base point or [Name/Label/Chain/Description/Palette/Value set]:"
        );
    }

    #[test]
    fn base_and_value_set_keywords_are_refused_where_the_type_does_not_offer_them() {
        for kind in [Kind::Polar, Kind::XY] {
            let mut command = DistanceParameterCommand::new(0, Handle::new(1), kind);
            assert!(
                matches!(
                    command.on_text_input("B"),
                    Some(CmdResult::ReportError(reason))
                        if reason == "Point or option keyword required."
                ),
                "{} asks no base-location question",
                kind.name()
            );
        }
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::XY);
        assert!(
            matches!(command.on_text_input("V"), Some(CmdResult::NeedPoint)),
            "XY offers a horizontal value set"
        );
        assert_eq!(
            command.prompt(),
            "Enter horizontal value set type [None/List/Increment] <None>:"
        );
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(
            command.prompt(),
            "Enter vertical value set type [None/List/Increment] <None>:",
            "Enter at the horizontal stage goes straight to the vertical stage"
        );
        let mut command = DistanceParameterCommand::new(0, Handle::new(1), Kind::Polar);
        assert!(
            matches!(command.on_text_input("V"), Some(CmdResult::NeedPoint)),
            "Polar offers a value set"
        );
        assert_eq!(
            command.prompt(),
            "Enter distance value set type [None/List/Increment] <None>:"
        );
    }

    #[test]
    fn the_labels_travel_with_the_placement() {
        let mut command = polar();
        for answer in ["L", "Dist9", "C"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        for point in [DVec3::ZERO, DVec3::new(4., 0., 0.), DVec3::new(1., 1., 0.)] {
            assert!(matches!(
                command.on_point(point),
                CmdResult::NeedPoint | CmdResult::PlaceBlockDistanceParameter { .. }
            ));
        }
        assert!(matches!(
            command.finish(DVec3::new(1., 1., 0.), None),
            CmdResult::PlaceBlockDistanceParameter { options, .. }
                if options.distance_label.as_deref() == Some("Dist9")
                    && options.angle_label.as_deref() == Some("C")
        ));
    }

    #[test]
    fn a_chain_answer_asks_whether_associated_actions_run() {
        let mut command = linear();
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
            "BPARAMETER Linear  Specify start point or [Name/Label/Chain/Description/Base/Palette/Value set]:"
        );
    }

    #[test]
    fn an_empty_chain_answer_keeps_actions_unchained() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(command.on_enter(), CmdResult::NeedPoint));
        assert_eq!(command.options.chain, None);
    }

    #[test]
    fn a_description_answer_takes_its_text_as_written() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(command.prompt(), "Enter property description:");
        assert_eq!(command.input_kind(), InputKind::FreeText);
        assert!(matches!(
            command.on_text_input("  ramp  length  "),
            Some(CmdResult::NeedPoint)
        ));
        assert_eq!(
            command.options.description.as_deref(),
            Some("  ramp  length  ")
        );
    }

    #[test]
    fn a_palette_answer_lists_in_the_palette_by_default() {
        let mut command = linear();
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
        let mut command = linear();
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
        let mut command = linear();
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
        let Some(CmdResult::ReportError(error)) = linear().on_text_input("Z") else {
            panic!("expected a refusal");
        };
        assert_eq!(error, "Point or option keyword required.");
    }

    #[test]
    fn options_can_be_answered_in_any_order() {
        let mut command = linear();
        for answer in ["P", "No", "D", "audit note", "CHAIN", "Yes"] {
            assert!(
                matches!(command.on_text_input(answer), Some(CmdResult::NeedPoint)),
                "{answer}"
            );
        }
        assert_eq!(command.options.chain, Some(true));
        assert_eq!(command.options.description.as_deref(), Some("audit note"));
        assert_eq!(command.options.palette, Some(false));
    }

    #[test]
    fn a_click_while_an_option_is_open_keeps_the_question() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_point(DVec3::new(1., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert!(
            command.base.is_none(),
            "the click did not become the base point"
        );
        assert_eq!(
            command.prompt(),
            "Evaluate associated actions when parameter is edited by another action? [Yes/No] <No>:"
        );
    }

    #[test]
    fn a_keyword_after_the_base_point_is_not_an_option() {
        let mut command = linear();
        assert!(matches!(
            command.on_point(DVec3::new(1., 2., 0.)),
            CmdResult::NeedPoint
        ));
        assert!(command.on_text_input("C").is_none());
    }

    #[test]
    fn reopening_an_option_keeps_the_default_it_shows() {
        let mut command = linear();
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

    #[test]
    fn the_answers_travel_with_the_placement() {
        let mut command = linear();
        assert!(matches!(
            command.on_text_input("C"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("Yes"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("D"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("ramp"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("P"),
            Some(CmdResult::NeedPoint)
        ));
        assert!(matches!(
            command.on_text_input("No"),
            Some(CmdResult::NeedPoint)
        ));
        for point in [DVec3::ZERO, DVec3::new(4., 0., 0.), DVec3::new(2., 2., 0.)] {
            assert!(matches!(
                command.on_point(point),
                CmdResult::NeedPoint | CmdResult::PlaceBlockDistanceParameter { .. }
            ));
        }
        assert!(matches!(
            command.finish(DVec3::new(2., 2., 0.), None),
            CmdResult::PlaceBlockDistanceParameter { options, .. }
                if options.chain == Some(true)
                    && options.description.as_deref() == Some("ramp")
                    && options.palette == Some(false)
        ));
    }

    /// A picked point where a count is wanted is not a count at all, so the
    /// refusal says that rather than reciting the list.
    #[test]
    fn a_pick_at_the_grip_count_prompt_is_refused_as_not_an_integer() {
        let mut command = at_grip_count();
        assert!(matches!(
            command.on_point(DVec3::new(9., 6., 0.)),
            CmdResult::ReportError(reason) if reason == "Requires an integer value."
        ));
        assert!(matches!(
            command.on_text_input("1,8"),
            Some(CmdResult::ReportError(reason)) if reason == "Requires an integer value."
        ));
        assert!(matches!(
            command.on_text_input("2.5"),
            Some(CmdResult::ReportError(reason)) if reason == "Requires an integer value."
        ));
    }

    /// A whole number the option list does not offer stays a list problem.
    #[test]
    fn a_count_outside_the_list_is_refused_with_the_list() {
        for count in ["3", "-1", "8", "+2", "04", "99999999999999999999"] {
            let mut command = at_grip_count();
            assert!(
                matches!(
                    command.on_text_input(count),
                    Some(CmdResult::ReportError(reason))
                        if reason == "Enter the number of grips: 0, 1, 2 or 4."
                ),
                "{count}"
            );
        }
    }
}
