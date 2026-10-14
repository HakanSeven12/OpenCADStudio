// ADJUST / -IMAGEADJUST / PDFADJUST / DWFADJUST / DGNADJUST — brightness,
// contrast and fade of raster images; fade, contrast and monochrome of
// PDF, DWF and DGN underlays.
//
//   Select image or underlay:
//   Enter image option [Contrast/Fade/Brightness] <Brightness>:
//   Enter brightness value (0-100) <50>:
//
//   Enter PDF underlay option [Fade/Contrast/Monochrome] <Fade>:
//   Enter fade value (0-100) <25>:
//   Monochrome? [Yes/No] <No>:
//
// An image value prompt offers the first image's own value. The underlay
// option and the fade / contrast values offered are the last ones used in
// the session, whatever the underlay kind. The host applies the value to
// every chosen object as one undo step.

use std::sync::Mutex;

use codec::entities::{UnderlayDisplayFlags, UnderlayType};
use codec::types::Handle;
use codec::EntityType;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::t;

pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/xadjust.svg"));
pub fn tool() -> ToolDef {
    ToolDef {
        id: "ADJUST",
        label: "Adjust",
        icon: ICON,
        event: ModuleEvent::Command("ADJUST".to_string()),
    }
}

/// What a command takes: `A` images or underlays, `I` images, `P` / `W` /
/// `G` PDF / DWF / DGN underlays.
pub fn keeps(mode: &str) -> fn(&EntityType) -> bool {
    fn any(e: &EntityType) -> bool {
        matches!(e, EntityType::RasterImage(_) | EntityType::Underlay(_))
    }
    fn image(e: &EntityType) -> bool {
        matches!(e, EntityType::RasterImage(_))
    }
    fn pdf(e: &EntityType) -> bool {
        matches!(e, EntityType::Underlay(u) if u.underlay_type == UnderlayType::Pdf)
    }
    fn dwf(e: &EntityType) -> bool {
        matches!(e, EntityType::Underlay(u) if u.underlay_type == UnderlayType::Dwf)
    }
    fn dgn(e: &EntityType) -> bool {
        matches!(e, EntityType::Underlay(u) if u.underlay_type == UnderlayType::Dgn)
    }
    match mode {
        "I" => image,
        "P" => pdf,
        "W" => dwf,
        "G" => dgn,
        _ => any,
    }
}

/// The command each mode belongs to.
pub fn command_name(mode: &str) -> &'static str {
    match mode {
        "I" => "-IMAGEADJUST",
        "P" => "PDFADJUST",
        "W" => "DWFADJUST",
        "G" => "DGNADJUST",
        _ => "ADJUST",
    }
}

/// The selection prompt of each command.
pub fn select_prompt(mode: &str) -> &'static str {
    match mode {
        "I" => "Select image(s):",
        "P" => "Select PDF underlay:",
        "W" => "Select DWF underlay:",
        "G" => "Select DGN underlay:",
        _ => "Select image or underlay:",
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Prop {
    Brightness,
    Contrast,
    Fade,
    Monochrome,
}

impl Prop {
    fn label(self) -> &'static str {
        match self {
            Prop::Brightness => "Brightness",
            Prop::Contrast => "Contrast",
            Prop::Fade => "Fade",
            Prop::Monochrome => "Monochrome",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Prop::Brightness => "BRIGHTNESS",
            Prop::Contrast => "CONTRAST",
            Prop::Fade => "FADE",
            Prop::Monochrome => "MONOCHROME",
        }
    }
}

const IMAGE_OPTIONS: [Prop; 3] = [Prop::Contrast, Prop::Fade, Prop::Brightness];
const UNDERLAY_OPTIONS: [Prop; 3] = [Prop::Fade, Prop::Contrast, Prop::Monochrome];

/// The underlay option, fade and contrast last used in the session.
static LAST_UNDERLAY: Mutex<(usize, u8, u8)> = Mutex::new((0, 25, 75));

fn last_underlay() -> (usize, u8, u8) {
    LAST_UNDERLAY.lock().map(|v| *v).unwrap_or((0, 25, 75))
}

enum Subject {
    Image {
        brightness: u8,
        contrast: u8,
        fade: u8,
    },
    Underlay {
        kind: &'static str,
        monochrome: bool,
    },
}

pub struct AdjustCommand {
    handles: Vec<Handle>,
    subject: Subject,
    prop: Option<Prop>,
}

/// Why a chosen set cannot be adjusted together.
pub const MIXED: &str =
    "*Invalid selection set. Select underlay objects of the same file type only.*";

impl AdjustCommand {
    /// The command for the chosen objects (the first one supplies the image
    /// values and the monochrome state), or the reason they do not go
    /// together. `None` when nothing adjustable was chosen.
    pub fn for_objects(objects: &[(Handle, EntityType)]) -> Option<Result<Self, &'static str>> {
        let (_, first) = objects.first()?;
        let kind = |e: &EntityType| match e {
            EntityType::Underlay(u) => Some(u.underlay_type),
            _ => None,
        };
        if objects.iter().any(|(_, e)| kind(e) != kind(first)) {
            return Some(Err(MIXED));
        }
        let subject = match first {
            EntityType::RasterImage(img) => Subject::Image {
                brightness: img.brightness,
                contrast: img.contrast,
                fade: img.fade,
            },
            EntityType::Underlay(u) => Subject::Underlay {
                kind: match u.underlay_type {
                    UnderlayType::Pdf => "PDF",
                    UnderlayType::Dwf => "DWF",
                    UnderlayType::Dgn => "DGN",
                },
                monochrome: u.flags.contains(UnderlayDisplayFlags::MONOCHROME),
            },
            _ => return None,
        };
        Some(Ok(Self {
            handles: objects.iter().map(|(h, _)| *h).collect(),
            subject,
            prop: None,
        }))
    }

    fn options_list(&self) -> &'static [Prop; 3] {
        match self.subject {
            Subject::Image { .. } => &IMAGE_OPTIONS,
            Subject::Underlay { .. } => &UNDERLAY_OPTIONS,
        }
    }

    fn default_option(&self) -> Prop {
        match self.subject {
            Subject::Image { .. } => Prop::Brightness,
            Subject::Underlay { .. } => UNDERLAY_OPTIONS[last_underlay().0.min(2)],
        }
    }

    fn default_value(&self, prop: Prop) -> u8 {
        match (&self.subject, prop) {
            (Subject::Image { brightness, .. }, Prop::Brightness) => *brightness,
            (Subject::Image { contrast, .. }, Prop::Contrast) => *contrast,
            (Subject::Image { fade, .. }, _) => *fade,
            (Subject::Underlay { .. }, Prop::Contrast) => last_underlay().2,
            (Subject::Underlay { .. }, _) => last_underlay().1,
        }
    }

    fn apply(&self, prop: Prop, value: u8) -> CmdResult {
        if let Subject::Underlay { .. } = self.subject {
            if let Ok(mut last) = LAST_UNDERLAY.lock() {
                last.0 = UNDERLAY_OPTIONS.iter().position(|p| *p == prop).unwrap_or(0);
                match prop {
                    Prop::Fade => last.1 = value,
                    Prop::Contrast => last.2 = value,
                    _ => {}
                }
            }
        }
        let handles: Vec<String> = self
            .handles
            .iter()
            .map(|h| format!("{:X}", h.value()))
            .collect();
        CmdResult::Dispatch(format!(
            "_ADJUSTAPPLY {} {} {}",
            prop.key(),
            value,
            handles.join(" ")
        ))
    }
}

/// Whether `text` names `label` (its key letter or a longer prefix, in
/// English or in the interface language).
fn names(text: &str, label: &str) -> bool {
    let english = label.to_uppercase();
    let local = t!(label).to_uppercase();
    !text.is_empty() && (english.starts_with(text) || local.starts_with(text))
}

impl CadCommand for AdjustCommand {
    fn name(&self) -> &'static str {
        "ADJUST"
    }

    fn prompt(&self) -> String {
        match (self.prop, &self.subject) {
            (None, Subject::Image { .. }) => {
                t!("Enter image option [Contrast/Fade/Brightness] <Brightness>:").into_owned()
            }
            (None, Subject::Underlay { kind, .. }) => t!(
                "Enter %{kind} underlay option [Fade/Contrast/Monochrome] <%{option}>:",
                kind = kind,
                option = t!(self.default_option().label())
            )
            .into_owned(),
            (Some(Prop::Monochrome), Subject::Underlay { monochrome, .. }) => t!(
                "Monochrome? [Yes/No] <%{value}>:",
                value = if *monochrome { t!("Yes") } else { t!("No") }
            )
            .into_owned(),
            (Some(Prop::Brightness), _) => t!(
                "Enter brightness value (0-100) <%{value}>:",
                value = self.default_value(Prop::Brightness)
            )
            .into_owned(),
            (Some(Prop::Contrast), _) => t!(
                "Enter contrast value (0-100) <%{value}>:",
                value = self.default_value(Prop::Contrast)
            )
            .into_owned(),
            (Some(_), _) => t!(
                "Enter fade value (0-100) <%{value}>:",
                value = self.default_value(Prop::Fade)
            )
            .into_owned(),
        }
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.prop {
            None => self
                .options_list()
                .iter()
                .map(|p| CmdOption::new(p.label(), &p.label()[..1]))
                .collect(),
            Some(Prop::Monochrome) => vec![CmdOption::new("Yes", "Y"), CmdOption::new("No", "N")],
            Some(_) => Vec::new(),
        }
    }

    fn input_kind(&self) -> InputKind {
        InputKind::SingleToken
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim().to_uppercase();
        let invalid = || {
            Some(CmdResult::ReportError(
                t!("Invalid option keyword.").into_owned(),
            ))
        };
        match self.prop {
            None => {
                let chosen = if text.is_empty() {
                    Some(self.default_option())
                } else {
                    self.options_list()
                        .iter()
                        .copied()
                        .find(|p| names(&text, p.label()))
                };
                let Some(prop) = chosen else {
                    return invalid();
                };
                self.prop = Some(prop);
                Some(CmdResult::NeedPoint)
            }
            Some(Prop::Monochrome) => {
                let Subject::Underlay { monochrome, .. } = self.subject else {
                    return Some(CmdResult::Cancel);
                };
                let on = if text.is_empty() {
                    monochrome
                } else if names(&text, "Yes") {
                    true
                } else if names(&text, "No") {
                    false
                } else {
                    return invalid();
                };
                Some(self.apply(Prop::Monochrome, on as u8))
            }
            Some(prop) => {
                let value = if text.is_empty() {
                    Some(self.default_value(prop))
                } else {
                    text.parse::<u8>().ok().filter(|v| *v <= 100)
                };
                match (value, &self.subject) {
                    (Some(v), _) => Some(self.apply(prop, v)),
                    (None, Subject::Image { .. }) => Some(CmdResult::ReportError(
                        t!("Value must be an integer between 0 and 100.").into_owned(),
                    )),
                    (None, Subject::Underlay { .. }) => Some(CmdResult::ReportError(
                        crate::tf!("Requires an integer between {min} and {max}.", min = 0, max = 100)
                            .into_owned(),
                    )),
                }
            }
        }
    }

    fn on_point(&mut self, _pt: glam::DVec3) -> CmdResult {
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        self.on_text_input("").unwrap_or(CmdResult::Cancel)
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["IMAGEADJUST", "-IMAGEADJUST", "PDFADJUST", "DWFADJUST", "DGNADJUST"]
});
