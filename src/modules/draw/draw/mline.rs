// MLINE command — create a styled group of parallel lines.

use acadrust::entities::{MLine, MLineFlags, MLineJustification, MLineSegment, MLineVertex};
use acadrust::objects::MLineStyle;
use acadrust::types::Vector3;
use acadrust::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, WorkingPlane};
use crate::scene::model::wire_model::WireModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Start,
    Next,
    Justification,
    Scale,
    Style,
}

pub struct MlineCommand {
    points: Vec<DVec3>,
    scale: f64,
    justification: MLineJustification,
    step: Step,
    style_name: String,
    style_handle: Option<Handle>,
    styles: Vec<(Handle, MLineStyle)>,
    notice: Option<String>,
    plane: WorkingPlane,
}

impl MlineCommand {
    pub fn with_styles(
        mut styles: Vec<(Handle, MLineStyle)>,
        style_name: impl Into<String>,
        scale: f64,
        justification: i16,
    ) -> Self {
        let requested = style_name.into();
        if styles.is_empty() {
            styles.push((Handle::NULL, MLineStyle::standard()));
        }
        let selected = styles
            .iter()
            .find(|(_, style)| style.name.eq_ignore_ascii_case(&requested))
            .or_else(|| styles.first());
        let (style_handle, style_name) = selected
            .map(|(handle, style)| {
                (
                    (!handle.is_null()).then_some(*handle),
                    style.name.clone(),
                )
            })
            .unwrap_or((None, requested));
        Self {
            points: Vec::new(),
            scale: if scale.is_finite() && scale != 0.0 {
                scale
            } else {
                1.0
            },
            justification: MLineJustification::from(justification),
            step: Step::Start,
            style_name,
            style_handle,
            styles,
            notice: None,
            plane: WorkingPlane::default(),
        }
    }

    fn selected_style(&self) -> Option<&MLineStyle> {
        self.styles
            .iter()
            .find(|(handle, style)| {
                self.style_handle == Some(*handle)
                    || style.name.eq_ignore_ascii_case(&self.style_name)
            })
            .map(|(_, style)| style)
    }

    fn justification_name(&self) -> String {
        match self.justification {
            MLineJustification::Top => crate::t!("Top").into_owned(),
            MLineJustification::Zero => crate::t!("Zero").into_owned(),
            MLineJustification::Bottom => crate::t!("Bottom").into_owned(),
        }
    }

    fn select_style(&mut self, name: &str) -> bool {
        let Some((handle, style)) = self
            .styles
            .iter()
            .find(|(_, style)| style.name.eq_ignore_ascii_case(name.trim()))
        else {
            return false;
        };
        self.style_handle = (!handle.is_null()).then_some(*handle);
        self.style_name = style.name.clone();
        true
    }

    fn commit(&self, closed: bool) -> Option<EntityType> {
        let style = self.selected_style()?;
        let local: Vec<DVec3> = self
            .points
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect();
        let entity = build_mline(
            &local,
            self.scale,
            self.justification,
            closed,
            style,
            self.style_handle,
        );
        Some(self.plane.place_entity(entity))
    }

    fn preview(&self, cursor: DVec3) -> Option<WireModel> {
        let style = self.selected_style()?;
        let mut local: Vec<DVec3> = self
            .points
            .iter()
            .map(|point| self.plane.to_local(*point))
            .collect();
        let cursor_local = self.plane.to_local(cursor);
        if local
            .last()
            .is_none_or(|last| last.distance_squared(cursor_local) > 1.0e-20)
        {
            local.push(cursor_local);
        }
        if local.len() < 2 {
            return None;
        }
        let EntityType::MLine(mline) = build_mline(
            &local,
            self.scale,
            self.justification,
            false,
            style,
            self.style_handle,
        ) else {
            return None;
        };
        let mut points = Vec::new();
        for (line_index, line) in
            crate::entities::mline::mline_lines_with_style(&mline, style)
                .into_iter()
                .enumerate()
        {
            if line_index > 0 {
                points.push([f64::NAN; 3]);
            }
            points.extend(line.points.into_iter().map(|point| {
                if point[0].is_nan() {
                    [f64::NAN; 3]
                } else {
                    let world = self
                        .plane
                        .to_world(DVec3::new(point[0], point[1], point[2]));
                    world.to_array()
                }
            }));
        }
        let (points, points_low) =
            crate::scene::convert::tessellate::points_to_ds(points);
        let fill_tris = crate::entities::mline::mline_fill_triangles_with_style(&mline, style)
            .into_iter()
            .map(|point| {
                let world = self
                    .plane
                    .to_world(DVec3::new(point[0], point[1], point[2]));
                world.to_array()
            })
            .collect::<Vec<_>>();
        let (fill_tris, fill_tris_low) =
            crate::scene::convert::tessellate::points_to_ds(fill_tris);
        Some(WireModel {
            point_marker: None,
            taper_widths: Vec::new(),
            world_width: 0.0,
            depth_override: None,
            display_visible: true,
            plot_visible: true,
            fill_is_3d: false,
            fill_is_2d_solid: true,
            render_instance: None,
            pick_tris: Vec::new(),
            pick_tris_low: Vec::new(),
            dash_from_start: false,
            dash_align_end: None,
            text_verts: Vec::new(),
            name: "mline_preview".into(),
            points,
            points_low,
            color: WireModel::CYAN,
            selected: false,
            pattern_length: 0.0,
            pattern: [0.0; 8],
            line_weight_px: 1.0,
            snap_pts: Vec::new(),
            tangent_geoms: Vec::new(),
            aci: 0,
            key_vertices: Vec::new(),
            aabb: WireModel::UNBOUNDED_AABB,
            plinegen: true,
            fill_tris,
            fill_tris_low,
        })
    }
}

impl CadCommand for MlineCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "MLINE"
    }

    fn prompt(&self) -> String {
        let notice = self
            .notice
            .as_ref()
            .map(|value| format!("{value}\n"))
            .unwrap_or_default();
        let body = match self.step {
            Step::Start => crate::tf!(
                "MLINE  Current settings: Justification = {}, Scale = {}, Style = {}\nSpecify start point or [Justification/Scale/STyle]:",
                self.justification_name(), self.scale, self.style_name
            )
            .into_owned(),
            Step::Next if self.points.len() >= 3 => {
                crate::t!("MLINE  Specify next point or [Close/Undo]:").into_owned()
            }
            Step::Next => crate::t!("MLINE  Specify next point or [Undo]:").into_owned(),
            Step::Justification => crate::tf!(
                "MLINE  Enter justification type [Top/Zero/Bottom] <{}>:",
                self.justification_name()
            )
            .into_owned(),
            Step::Scale => {
                crate::tf!("MLINE  Enter scale factor <{}>:", self.scale).into_owned()
            }
            Step::Style => {
                crate::tf!("MLINE  Enter style name or [?] <{}>:", self.style_name).into_owned()
            }
        };
        format!("{notice}{body}")
    }

    fn options(&self) -> Vec<CmdOption> {
        match self.step {
            Step::Start => vec![
                CmdOption::new(crate::t!("Justification").as_ref(), "J"),
                CmdOption::new(crate::t!("Scale").as_ref(), "S"),
                CmdOption::new(crate::t!("Style").as_ref(), "ST"),
            ],
            Step::Next if self.points.len() >= 3 => vec![
                CmdOption::new(crate::t!("Close").as_ref(), "C"),
                CmdOption::new(crate::t!("Undo").as_ref(), "U"),
            ],
            Step::Next => vec![CmdOption::new(crate::t!("Undo").as_ref(), "U")],
            Step::Justification => vec![
                CmdOption::new(crate::t!("Top").as_ref(), "T"),
                CmdOption::new(crate::t!("Zero").as_ref(), "Z"),
                CmdOption::new(crate::t!("Bottom").as_ref(), "B"),
            ],
            Step::Scale => Vec::new(),
            Step::Style => self
                .styles
                .iter()
                .map(|(_, style)| CmdOption::new(&style.name, &style.name))
                .collect(),
        }
    }

    fn wants_text_input(&self) -> bool {
        true
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Start | Step::Next)
    }

    fn mline_settings(&self) -> Option<(f64, i16, String, Option<Handle>)> {
        Some((
            self.scale,
            self.justification as i16,
            self.style_name.clone(),
            self.style_handle,
        ))
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let token = text.trim();
        let upper = token.to_uppercase();
        self.notice = None;
        match self.step {
            Step::Start => match upper.as_str() {
                "J" | "JUSTIFICATION" => self.step = Step::Justification,
                "S" | "SCALE" => self.step = Step::Scale,
                "ST" | "STYLE" => self.step = Step::Style,
                _ => return None,
            },
            Step::Next => match upper.as_str() {
                "U" | "UNDO" => {
                    self.points.pop();
                    if self.points.is_empty() {
                        self.step = Step::Start;
                    }
                }
                "C" | "CLOSE" if self.points.len() >= 3 => {
                    return self.commit(true).map(CmdResult::CommitAndExit);
                }
                _ => return None,
            },
            Step::Justification => {
                self.justification = match upper.as_str() {
                    "T" | "TOP" => MLineJustification::Top,
                    "Z" | "ZERO" => MLineJustification::Zero,
                    "B" | "BOTTOM" => MLineJustification::Bottom,
                    _ => return None,
                };
                self.step = Step::Start;
            }
            Step::Scale => {
                let value = token.replace(',', ".").parse::<f64>().ok()?;
                if !value.is_finite() || value == 0.0 {
                    return None;
                }
                self.scale = value;
                self.step = Step::Start;
            }
            Step::Style => {
                if token == "?" {
                    self.notice = Some(crate::tf!(
                        "Loaded multiline styles: {}",
                        self.styles
                            .iter()
                            .map(|(_, style)| style.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                    .into_owned());
                } else if self.select_style(token) {
                    self.step = Step::Start;
                } else {
                    self.notice = Some(
                        crate::t!(
                            "Multiline style \"%{style}\" was not found.",
                            style = token
                        )
                        .into_owned(),
                    );
                }
            }
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_point(&mut self, point: DVec3) -> CmdResult {
        if !matches!(self.step, Step::Start | Step::Next) {
            return CmdResult::NeedPoint;
        }
        if self
            .points
            .last()
            .is_some_and(|last| last.distance_squared(point) <= 1.0e-20)
        {
            return CmdResult::NeedPoint;
        }
        self.points.push(point);
        self.step = Step::Next;
        CmdResult::NeedPoint
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Start if self.points.is_empty() => CmdResult::Cancel,
            Step::Next if self.points.len() >= 2 => self
                .commit(false)
                .map(CmdResult::CommitAndExit)
                .unwrap_or(CmdResult::Cancel),
            Step::Justification | Step::Scale | Step::Style => {
                self.step = Step::Start;
                CmdResult::NeedPoint
            }
            _ => CmdResult::Cancel,
        }
    }

    fn on_undo_step(&mut self) -> Option<CmdResult> {
        if self.points.is_empty() {
            return None;
        }
        self.points.pop();
        if self.points.is_empty() {
            self.step = Step::Start;
        }
        Some(CmdResult::NeedPoint)
    }

    fn on_mouse_move(&mut self, point: DVec3) -> Option<WireModel> {
        (!self.points.is_empty()).then(|| self.preview(point)).flatten()
    }
}

pub(crate) fn sync_mline_element_parameters(mline: &mut MLine, style: &MLineStyle) {
    let offsets: Vec<f64> = if style.elements.is_empty() {
        vec![0.5, -0.5]
    } else {
        style.elements.iter().map(|element| element.offset).collect()
    };
    let minimum = offsets.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = offsets
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let shift = match mline.justification {
        MLineJustification::Top => -maximum,
        MLineJustification::Zero => 0.0,
        MLineJustification::Bottom => -minimum,
    };
    let normal = glam::DVec3::new(mline.normal.x, mline.normal.y, mline.normal.z)
        .normalize_or(glam::DVec3::Z);
    for vertex in &mut mline.vertices {
        let direction = glam::DVec3::new(vertex.direction.x, vertex.direction.y, vertex.direction.z)
            .normalize_or(glam::DVec3::X);
        let miter = glam::DVec3::new(vertex.miter.x, vertex.miter.y, vertex.miter.z)
            .normalize_or(glam::DVec3::Y);
        let factor = miter.dot(normal.cross(direction)).abs().max(1.0e-9);
        vertex
            .segments
            .resize_with(offsets.len(), MLineSegment::new);
        vertex.segments.truncate(offsets.len());
        for (segment, offset) in vertex.segments.iter_mut().zip(&offsets) {
            let value = (offset + shift) * mline.scale_factor / factor;
            if let Some(first) = segment.parameters.first_mut() {
                *first = value;
            } else {
                segment.parameters.extend([value, 0.0]);
            }
        }
    }
    mline.style_element_count = offsets.len();
}

fn build_mline(
    points: &[DVec3],
    scale: f64,
    justification: MLineJustification,
    closed: bool,
    style: &MLineStyle,
    style_handle: Option<Handle>,
) -> EntityType {
    let mut mline = MLine::new();
    mline.scale_factor = scale;
    mline.justification = justification;
    mline.style_name = style.name.clone();
    mline.style_handle = style_handle;
    mline.style_element_count = style.elements.len().max(1);
    for point in points {
        let mut vertex = MLineVertex::new(Vector3::new(point.x, point.y, point.z));
        vertex.init_segments(mline.style_element_count);
        mline.vertices.push(vertex);
    }
    mline.flags.set(MLineFlags::CLOSED, closed);
    crate::entities::mline::rebuild_mline_geometry(&mut mline);
    sync_mline_element_parameters(&mut mline, style);
    EntityType::MLine(mline)
}

inventory::submit!(crate::command::CommandRegistration { names: &["MLINE"] });
