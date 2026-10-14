// MVSETUP — set a drawing up for plotting.
//
// In model space without paper space it draws the sheet border at the
// drawing scale:
//   Enable paper space? [No/Yes] <Y>:
//   Enter the scale factor <1>:
//   Enter the paper width:
//   Enter the paper height:
// On a layout it lays out viewports or scales them:
//   Enter an option [Create/Delete objects/Scale viewports]:
//   Enter viewport layout [Single/Engineering/Array] <Single>:
//   Specify first corner of bounding area for viewport(s) <Sheet>:
//   Specify opposite corner:
//   Enter number of viewports in X direction <2>:     (Array)
//   Enter number of viewports in Y direction <2>:     (Array)
//   Specify distance between viewports in X direction <0>:
//   Specify distance between viewports in Y direction <0>:
//   Select the objects to delete:
//   Select the viewports to scale:
//   Enter the number of paper space units <1>:
//   Enter the number of model space units <1>:

use crate::t;
use codec::entities::{LwPolyline, LwVertex, Viewport};
use codec::types::{Vector2, Vector3};
use codec::{EntityType, Handle};
use glam::{DVec2, DVec3};

use crate::command::{CadCommand, CmdOption, CmdResult};

#[derive(Clone, Copy, PartialEq)]
enum Step {
    PaperSpace,
    Scale,
    Width,
    Height,
    Option,
    Arrangement,
    FirstCorner,
    SecondCorner,
    Columns,
    Rows,
    GapX,
    GapY,
    Delete,
    Viewports,
    PaperUnits,
    ModelUnits,
}

#[derive(Clone, Copy, PartialEq)]
enum Arrangement {
    Single,
    Engineering,
    Array,
}

pub struct MvsetupCommand {
    step: Step,
    /// The layout model space switches to when paper space is enabled.
    layout: String,
    paper_bounds: ((f64, f64), (f64, f64)),
    scale: f64,
    width: f64,
    arrangement: Arrangement,
    first: DVec2,
    area: (DVec2, DVec2),
    columns: u32,
    rows: u32,
    gap_x: f64,
    picked: Vec<Handle>,
    paper_units: f64,
}

impl MvsetupCommand {
    /// `layout` is `None` on a layout, or the layout to switch to from model
    /// space; `paper_bounds` is the sheet's printable area.
    pub fn new(layout: Option<String>, paper_bounds: ((f64, f64), (f64, f64))) -> Self {
        Self {
            step: if layout.is_some() {
                Step::PaperSpace
            } else {
                Step::Option
            },
            layout: layout.unwrap_or_default(),
            paper_bounds,
            scale: 1.0,
            width: 0.0,
            arrangement: Arrangement::Single,
            first: DVec2::ZERO,
            area: (DVec2::ZERO, DVec2::ZERO),
            columns: 2,
            rows: 2,
            gap_x: 0.0,
            picked: Vec::new(),
            paper_units: 1.0,
        }
    }

    fn after_area(&mut self) -> CmdResult {
        self.step = match self.arrangement {
            Arrangement::Single => return self.create(0.0),
            Arrangement::Engineering => Step::GapX,
            Arrangement::Array => Step::Columns,
        };
        CmdResult::NeedPoint
    }

    /// The viewports of the chosen arrangement, `gap_y` apart vertically.
    fn create(&self, gap_y: f64) -> CmdResult {
        let (columns, rows, gap_x) = match self.arrangement {
            Arrangement::Single => (1, 1, 0.0),
            Arrangement::Engineering => (2, 2, self.gap_x),
            Arrangement::Array => (self.columns, self.rows, self.gap_x),
        };
        let (min, max) = self.area;
        let width = (max.x - min.x - gap_x * (columns - 1) as f64) / columns as f64;
        let height = (max.y - min.y - gap_y * (rows - 1) as f64) / rows as f64;
        if width <= 1e-6 || height <= 1e-6 {
            return CmdResult::ReportError(
                t!("MVSETUP  The viewports do not fit in the bounding area.").into_owned(),
            );
        }
        // Engineering: top view over front view, right view beside it, and
        // an isometric view in the remaining corner.
        let directions = [
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, -1.0, 1.0),
            Vector3::new(0.0, -1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ];
        let mut viewports = Vec::new();
        for row in 0..rows {
            for column in 0..columns {
                let mut viewport = Viewport::new();
                viewport.center = Vector3::new(
                    min.x + column as f64 * (width + gap_x) + width / 2.0,
                    max.y - row as f64 * (height + gap_y) - height / 2.0,
                    0.0,
                );
                viewport.width = width;
                viewport.height = height;
                viewport.id = 2;
                if self.arrangement == Arrangement::Engineering {
                    viewport.view_direction = directions[(row * 2 + column) as usize].clone();
                }
                viewports.push(EntityType::Viewport(viewport));
            }
        }
        CmdResult::CommitEntitiesAndExit(viewports)
    }

    fn sheet_border(&self, height: f64) -> CmdResult {
        let (width, height) = (self.width * self.scale, height * self.scale);
        let mut border = LwPolyline::new();
        border.is_closed = true;
        border.vertices = [(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)]
            .into_iter()
            .map(|(x, y)| LwVertex::new(Vector2::new(x, y)))
            .collect();
        CmdResult::CommitAndZoom {
            entity: EntityType::LwPolyline(border),
            p1: DVec3::ZERO,
            p2: DVec3::new(width, height, 0.0),
        }
    }
}

/// A positive number, or `default` for an empty answer.
fn positive(text: &str, default: f64) -> Option<f64> {
    if text.is_empty() {
        return Some(default);
    }
    crate::entities::common::parse_typed_length(text).filter(|value| *value > 0.0)
}

impl CadCommand for MvsetupCommand {
    fn name(&self) -> &'static str {
        "MVSETUP"
    }

    fn prompt(&self) -> String {
        t!(match self.step {
            Step::PaperSpace => "MVSETUP  Enable paper space? [No/Yes] <Y>:",
            Step::Scale => "MVSETUP  Enter the scale factor <1>:",
            Step::Width => "MVSETUP  Enter the paper width:",
            Step::Height => "MVSETUP  Enter the paper height:",
            Step::Option => "MVSETUP  Enter an option [Create/Delete objects/Scale viewports]:",
            Step::Arrangement => {
                "MVSETUP  Enter viewport layout [Single/Engineering/Array] <Single>:"
            }
            Step::FirstCorner => {
                "MVSETUP  Specify first corner of bounding area for viewport(s) <Sheet>:"
            }
            Step::SecondCorner => "MVSETUP  Specify opposite corner:",
            Step::Columns => "MVSETUP  Enter number of viewports in X direction <2>:",
            Step::Rows => "MVSETUP  Enter number of viewports in Y direction <2>:",
            Step::GapX => "MVSETUP  Specify distance between viewports in X direction <0>:",
            Step::GapY => "MVSETUP  Specify distance between viewports in Y direction <0>:",
            Step::Delete => "MVSETUP  Select the objects to delete:",
            Step::Viewports => "MVSETUP  Select the viewports to scale:",
            Step::PaperUnits => "MVSETUP  Enter the number of paper space units <1>:",
            Step::ModelUnits => "MVSETUP  Enter the number of model space units <1>:",
        })
        .into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::PaperSpace => vec![CmdOption::new("No", "N"), CmdOption::new("Yes", "Y")],
            Step::Option => vec![
                CmdOption::new("Create", "C"),
                CmdOption::new("Delete objects", "D"),
                CmdOption::new("Scale viewports", "S"),
            ],
            Step::Arrangement => vec![
                CmdOption::new("Single", "S"),
                CmdOption::new("Engineering", "E"),
                CmdOption::new("Array", "A"),
            ],
            _ => Vec::new(),
        }
    }

    fn wants_text_input(&self) -> bool {
        !matches!(self.step, Step::Delete | Step::Viewports)
    }

    fn needs_entity_pick(&self) -> bool {
        matches!(self.step, Step::Delete | Step::Viewports)
    }

    fn entity_pick_highlights_hover(&self) -> bool {
        true
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if !handle.is_null() && !self.picked.contains(&handle) {
            self.picked.push(handle);
        }
        CmdResult::NeedPoint
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim();
        let word = text.trim_start_matches('_').to_ascii_uppercase();
        match self.step {
            Step::PaperSpace => match word.as_str() {
                "N" | "NO" => self.step = Step::Scale,
                "" | "Y" | "YES" => {
                    self.step = Step::Option;
                    return Some(CmdResult::MviewSwitchLayout(self.layout.clone()));
                }
                _ => return None,
            },
            Step::Scale => {
                self.scale = positive(text, 1.0)?;
                self.step = Step::Width;
            }
            Step::Width => {
                self.width = positive(text, f64::NAN).filter(|value| value.is_finite())?;
                self.step = Step::Height;
            }
            Step::Height => {
                let height = positive(text, f64::NAN).filter(|value| value.is_finite())?;
                return Some(self.sheet_border(height));
            }
            Step::Option => match word.as_str() {
                "C" | "CREATE" => self.step = Step::Arrangement,
                "D" | "DELETE" => self.step = Step::Delete,
                "S" | "SCALE" => self.step = Step::Viewports,
                _ => return None,
            },
            Step::Arrangement => {
                self.arrangement = match word.as_str() {
                    "" | "S" | "SINGLE" | "1" => Arrangement::Single,
                    "E" | "ENGINEERING" | "2" => Arrangement::Engineering,
                    "A" | "ARRAY" | "3" => Arrangement::Array,
                    _ => return None,
                };
                self.step = Step::FirstCorner;
            }
            Step::FirstCorner if text.is_empty() => {
                let ((x0, y0), (x1, y1)) = self.paper_bounds;
                self.area = (
                    DVec2::new(x0.min(x1), y0.min(y1)),
                    DVec2::new(x0.max(x1), y0.max(y1)),
                );
                return Some(self.after_area());
            }
            Step::Columns | Step::Rows => {
                let count = if text.is_empty() {
                    2
                } else {
                    text.parse::<u32>()
                        .ok()
                        .filter(|count| (1..=100).contains(count))?
                };
                if self.step == Step::Columns {
                    self.columns = count;
                    self.step = Step::Rows;
                } else {
                    self.rows = count;
                    self.step = Step::GapX;
                }
            }
            Step::GapX | Step::GapY => {
                let gap = if text.is_empty() {
                    0.0
                } else {
                    crate::entities::common::parse_typed_length(text).filter(|v| *v >= 0.0)?
                };
                if self.step == Step::GapY {
                    return Some(self.create(gap));
                }
                self.gap_x = gap;
                self.step = Step::GapY;
            }
            Step::PaperUnits => {
                self.paper_units = positive(text, 1.0)?;
                self.step = Step::ModelUnits;
            }
            Step::ModelUnits => {
                let model_units = positive(text, 1.0)?;
                return Some(CmdResult::ScaleViewports {
                    handles: std::mem::take(&mut self.picked),
                    factor: self.paper_units / model_units,
                });
            }
            Step::FirstCorner | Step::SecondCorner | Step::Delete | Step::Viewports => return None,
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::FirstCorner => {
                self.first = pt.truncate();
                self.step = Step::SecondCorner;
                CmdResult::NeedPoint
            }
            Step::SecondCorner => {
                let second = pt.truncate();
                self.area = (self.first.min(second), self.first.max(second));
                self.after_area()
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Delete => {
                self.step = Step::Option;
                let erased = std::mem::take(&mut self.picked);
                if erased.is_empty() {
                    return CmdResult::NeedPoint;
                }
                CmdResult::ReplaceManyContinue(
                    erased
                        .into_iter()
                        .map(|handle| (handle, Vec::new()))
                        .collect(),
                )
            }
            Step::Viewports if self.picked.is_empty() => CmdResult::Cancel,
            Step::Viewports => {
                self.step = Step::PaperUnits;
                CmdResult::NeedPoint
            }
            Step::Option | Step::Width | Step::Height | Step::SecondCorner => CmdResult::Cancel,
            _ => self.on_text_input("").unwrap_or(CmdResult::NeedPoint),
        }
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["MVSETUP"]
});
