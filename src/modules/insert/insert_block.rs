//! Block insertion: `-INSERT` on the command line, a click on the Blocks
//! palette, and paste-as-block.
//!
//! `-INSERT` follows the reference's prompts:
//!   Enter block name or [?] <last>:
//!   Units: Feet   Conversion:    1.0000
//!   Specify insertion point or [Basepoint/Scale/X/Y/Z/Rotate/Explode/REpeat]:
//!   Enter X scale factor, specify opposite corner, or [Corner/XYZ] <1>:
//!   Enter Y scale factor <use X scale factor>:
//!   Specify rotation angle <0>:
//! A palette click asks only what its options leave open, with the shorter
//! option list `[Basepoint/Scale/X/Y/Z/Rotate]`.

use codec::entities::{AttributeDefinition, AttributeEntity, Entity, Insert};
use codec::types::Vector3;
use codec::EntityType;
use glam::{DVec3, Vec3};
use crate::t;

use crate::app::settings::BlockInsertOptions;
use crate::command::{CadCommand, CmdResult, DynField, InputKind, WorkingPlane};
use crate::modules::IconKind;
use crate::scene::model::wire_model::WireModel;

/// The ribbon's Insert button: its face opens the Blocks palette, its arrow
/// the gallery of the drawing's blocks.
pub const GALLERY_ID: &str = "INSERT_GALLERY";
pub const ICON: IconKind = IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg"));
pub const GALLERY_ITEMS: &[(&str, &str, IconKind)] = &[("INSERT", "Insert Block", ICON)];

pub fn gallery() -> crate::modules::RibbonItem {
    crate::modules::RibbonItem::LargeDropdown {
        id: GALLERY_ID,
        label: "Insert Block",
        icon: ICON,
        items: GALLERY_ITEMS.to_vec(),
        default: "INSERT",
    }
}

/// What the drawing knows about its blocks, for `-INSERT`'s name step.
#[derive(Clone, Default)]
pub struct BlockCatalog {
    /// Named (user) blocks, sorted.
    pub user: Vec<String>,
    /// How many anonymous (`*`) blocks the drawing holds.
    pub unnamed: usize,
    /// Each user block's `Units: …   Conversion: …` line.
    pub units: rustc_hash::FxHashMap<String, String>,
    /// The folder `-INSERT` reports as its search path.
    pub search_path: String,
}

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// `-INSERT`.
    Classic,
    /// A Blocks palette click.
    Palette,
    /// Paste-as-block: only the drop point.
    Paste,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Preset {
    Base,
    Scale,
    X,
    Y,
    Z,
    Rotate,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Ask {
    Explode,
    Repeat,
    RepeatScale,
    RepeatRotate,
}

enum Step {
    Name,
    ListPattern,
    Point,
    Preset(Preset),
    YesNo(Ask),
    ScaleX,
    Corner,
    XyzX,
    XyzY,
    XyzZ,
    ScaleY,
    Uniform,
    Rotation,
    FillAttr {
        attdefs: Vec<AttributeDefinition>,
        idx: usize,
        values: Vec<(usize, String)>,
    },
}

pub struct InsertBlockCommand {
    mode: Mode,
    picker: crate::modules::insert::picker::BlockPicker,
    catalog: BlockCatalog,
    default_name: String,
    step: Step,
    name: String,
    /// Block preview geometry, in block coordinates.
    preview: Vec<WireModel>,
    /// The point of the block that lands on the insertion point (Basepoint).
    base: DVec3,
    /// Scales / rotation fixed before the point (options, palette fields).
    preset_scale: [Option<f64>; 3],
    preset_rotation: Option<f64>,
    /// Palette: the scale is asked after the point, uniformly.
    ask_uniform: bool,
    explode: bool,
    repeat: bool,
    repeat_scale: bool,
    repeat_rotation: bool,
    placed: usize,
    point: Option<DVec3>,
    scale: [f64; 3],
    rotation: f64,
    pending_insert: Option<Insert>,
    plane: WorkingPlane,
}

impl InsertBlockCommand {
    fn base_state(mode: Mode, picker: crate::modules::insert::picker::BlockPicker) -> Self {
        Self {
            mode,
            picker,
            catalog: BlockCatalog::default(),
            default_name: String::new(),
            step: Step::Name,
            name: String::new(),
            preview: Vec::new(),
            base: DVec3::ZERO,
            preset_scale: [None; 3],
            preset_rotation: None,
            ask_uniform: false,
            explode: false,
            repeat: false,
            repeat_scale: false,
            repeat_rotation: false,
            placed: 0,
            point: None,
            scale: [1.0; 3],
            rotation: 0.0,
            pending_insert: None,
            plane: WorkingPlane::default(),
        }
    }

    /// `-INSERT`: the name step first. `default_name` is INSNAME.
    pub fn classic(
        available: Vec<String>,
        usage_rank: rustc_hash::FxHashMap<String, (u32, usize)>,
        cliprompt_lines: u8,
        catalog: BlockCatalog,
        default_name: String,
    ) -> Self {
        let limit = (cliprompt_lines as usize).clamp(0, crate::modules::insert::picker::MAX_SUGGESTIONS);
        let picker = crate::modules::insert::picker::BlockPicker::new(available, usage_rank, limit);
        let mut command = Self::base_state(Mode::Classic, picker);
        command.catalog = catalog;
        command.default_name = default_name;
        command
    }

    /// `-INSERT` already given a block (a drawing just inserted as a block).
    pub fn classic_for_block(name: String, preview: Vec<WireModel>, units_line: String) -> Self {
        let mut command = Self::base_state(Mode::Classic, Self::single_picker(&name));
        command.catalog.units.insert(name.to_ascii_uppercase(), units_line);
        command.name = name;
        command.preview = preview;
        command.step = Step::Point;
        command
    }

    /// A Blocks palette click with the palette's options.
    pub fn palette(name: String, preview: Vec<WireModel>, options: &BlockInsertOptions) -> Self {
        let mut command = Self::base_state(Mode::Palette, Self::single_picker(&name));
        command.name = name;
        command.preview = preview;
        if !options.scale {
            let x = options.x;
            command.preset_scale = if options.uniform {
                [Some(x); 3]
            } else {
                [Some(options.x), Some(options.y), Some(options.z)]
            };
        } else {
            command.ask_uniform = options.uniform;
        }
        if !options.rotation {
            command.preset_rotation = Some(options.angle.to_radians());
        }
        command.explode = options.explode;
        command.repeat = options.repeat;
        command.repeat_scale = options.scale;
        command.repeat_rotation = options.rotation;
        command.step = Step::Point;
        if !options.insertion_point {
            command.point = Some(DVec3::ZERO);
        }
        command
    }

    /// Paste-as-block: the block is placed at the picked point.
    pub fn new_for_block(name: String, preview_wires: Vec<WireModel>, base: Vec3) -> Self {
        let mut command = Self::base_state(Mode::Paste, Self::single_picker(&name));
        command.name = name;
        command.preview = preview_wires;
        command.base = base.as_dvec3();
        command.preset_scale = [Some(1.0); 3];
        command.preset_rotation = Some(0.0);
        command.step = Step::Point;
        command
    }

    fn single_picker(name: &str) -> crate::modules::insert::picker::BlockPicker {
        crate::modules::insert::picker::BlockPicker::new(
            vec![name.to_string()],
            rustc_hash::FxHashMap::default(),
            0,
        )
    }

    /// A block dropped from the palette: it goes in at `point` (world).
    pub fn set_point(&mut self, point: DVec3) {
        self.point = Some(point);
    }

    /// A palette click with the insertion point turned off and nothing to
    /// ask: the block goes in at once. The host applies this result before
    /// showing a prompt.
    pub fn initial(&mut self) -> Option<CmdResult> {
        if self.point.is_some() && matches!(self.step, Step::Point) {
            return Some(self.after_point());
        }
        None
    }

    fn options_text(&self) -> &'static str {
        match self.mode {
            Mode::Classic => "[Basepoint/Scale/X/Y/Z/Rotate/Explode/REpeat]",
            _ => "[Basepoint/Scale/X/Y/Z/Rotate]",
        }
    }

    /// After the insertion point: ask what is still open, else place.
    fn after_point(&mut self) -> CmdResult {
        if self.preset_scale[0].is_none() {
            self.step = if self.ask_uniform { Step::Uniform } else { Step::ScaleX };
            return CmdResult::NeedPoint;
        }
        self.scale = [
            self.preset_scale[0].unwrap_or(1.0),
            self.preset_scale[1].unwrap_or(1.0),
            self.preset_scale[2].unwrap_or(1.0),
        ];
        self.after_scale()
    }

    fn after_scale(&mut self) -> CmdResult {
        match self.preset_rotation {
            Some(r) => {
                self.rotation = r;
                self.place()
            }
            None => {
                self.step = Step::Rotation;
                CmdResult::NeedPoint
            }
        }
    }

    /// Build the reference at the chosen point and hand it over.
    fn place(&mut self) -> CmdResult {
        let Some(point) = self.point.take() else {
            return CmdResult::NeedPoint;
        };
        let local = self.plane.to_local(point);
        let offset = self.local_offset(self.base);
        let mut ins = Insert::new(
            self.name.clone(),
            Vector3::new(local.x - offset.x, local.y - offset.y, local.z - offset.z),
        );
        ins.set_x_scale(self.scale[0]);
        ins.set_y_scale(self.scale[1]);
        ins.set_z_scale(self.scale[2]);
        ins.rotation = self.rotation;
        if !self.plane.is_identity() {
            ins.apply_transform(&self.plane.to_world_transform());
        }
        self.placed += 1;
        // Repeating: later blocks keep the first one's scale / rotation
        // unless each block is to be asked.
        if self.repeat {
            if !self.repeat_scale {
                self.preset_scale = self.scale.map(Some);
            }
            if !self.repeat_rotation {
                self.preset_rotation = Some(self.rotation);
            }
            self.step = Step::Point;
        }
        if self.explode {
            return CmdResult::CommitExplodedInsert {
                insert: EntityType::Insert(ins),
                keep_going: self.repeat,
            };
        }
        let block_name = self.name.clone();
        self.pending_insert = Some(ins);
        CmdResult::AttreqNeeded { block_name }
    }

    /// The scaled, rotated block-space offset of `p`.
    fn local_offset(&self, p: DVec3) -> DVec3 {
        let s = DVec3::new(p.x * self.scale[0], p.y * self.scale[1], p.z * self.scale[2]);
        let (sin, cos) = self.rotation.sin_cos();
        DVec3::new(s.x * cos - s.y * sin, s.x * sin + s.y * cos, s.z)
    }

    fn number(text: &str) -> Option<f64> {
        let t = text.trim().replace(',', ".");
        t.parse::<f64>().ok().filter(|v| v.is_finite())
    }

    fn yes_no(text: &str, default: bool) -> Option<bool> {
        match text.trim().to_ascii_uppercase().as_str() {
            "" => Some(default),
            "Y" | "YES" => Some(true),
            "N" | "NO" => Some(false),
            _ => None,
        }
    }

    /// `-INSERT ?`: the reference's listing.
    fn listing(&self, pattern: &str) -> String {
        let pattern = if pattern.trim().is_empty() { "*" } else { pattern.trim() };
        let mut out = String::from("Defined blocks.");
        let mut shown = 0;
        for name in &self.catalog.user {
            if pattern.split(',').any(|p| crate::io::xref_model::wildcard_match(name, p.trim())) {
                out.push_str(&format!("\n  \"{name}\""));
                shown += 1;
            }
        }
        let _ = shown;
        out.push_str(&format!(
            "\n\nUser     Unnamed\nBlocks   Blocks\n{:>5}{:>9}",
            self.catalog.user.len(),
            self.catalog.unnamed
        ));
        out
    }

    /// The block name chosen at the name step.
    fn choose(&mut self, name: String) -> CmdResult {
        let units = self
            .catalog
            .units
            .get(&name.to_ascii_uppercase())
            .cloned()
            .unwrap_or_default();
        self.name = name;
        self.step = Step::Point;
        if units.is_empty() {
            CmdResult::NeedPoint
        } else {
            CmdResult::ReportMeasurement(units)
        }
    }

    fn not_found(&self, name: &str) -> CmdResult {
        let file = if name.to_ascii_lowercase().ends_with(".dwg") {
            name.to_string()
        } else {
            format!("{name}.dwg")
        };
        let path = std::path::Path::new(&file);
        if path.is_file() {
            return CmdResult::Dispatch(format!("_-INSERTFILE {}", path.display()));
        }
        CmdResult::CancelWithMessage(format!(
            "\"{file}\": Can't find file in search path:\n  {} (current directory)\n*Invalid*",
            self.catalog.search_path
        ))
    }
}

impl CadCommand for InsertBlockCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        match self.mode {
            Mode::Paste => "INSERT",
            _ => "-INSERT",
        }
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Name => {
                if self.default_name.is_empty() {
                    "Enter block name or [?]:".to_string()
                } else {
                    format!("Enter block name or [?] <{}>:", self.default_name)
                }
            }
            Step::ListPattern => "Enter block(s) to list <*>:".to_string(),
            Step::Point => match self.mode {
                Mode::Paste => t!(
                    "INSERT  Specify insertion point for \"%{name}\"  [Scale/Rotate]:",
                    name = self.name
                )
                .into_owned(),
                _ => format!("Specify insertion point or {}:", self.options_text()),
            },
            Step::Preset(Preset::Base) => "Specify base point:".to_string(),
            Step::Preset(Preset::Scale) => match self.mode {
                Mode::Paste => t!("INSERT  Specify scale factor <1>:").into_owned(),
                _ => "Specify scale factor for XYZ axes <1>:".to_string(),
            },
            Step::Preset(Preset::X) => "Specify X scale factor <1>:".to_string(),
            Step::Preset(Preset::Y) => "Specify Y scale factor <1>:".to_string(),
            Step::Preset(Preset::Z) => "Specify Z scale factor <1>:".to_string(),
            Step::Preset(Preset::Rotate) => match self.mode {
                Mode::Paste => t!("INSERT  Specify rotation angle <0>:").into_owned(),
                _ => "Specify rotation angle <0>:".to_string(),
            },
            Step::YesNo(Ask::Explode) => "Explode block [Yes/No] <Yes>:".to_string(),
            Step::YesNo(Ask::Repeat) => "Repeat insertion of block [Yes/No] <Yes>:".to_string(),
            Step::YesNo(Ask::RepeatScale) => {
                "Enter a scale factor for each block [Yes/No] <No>".to_string()
            }
            Step::YesNo(Ask::RepeatRotate) => {
                "Enter a rotation angle for each block [Yes/No] <No>".to_string()
            }
            Step::ScaleX => {
                "Enter X scale factor, specify opposite corner, or [Corner/XYZ] <1>:".to_string()
            }
            Step::Corner => "Specify opposite corner:".to_string(),
            Step::XyzX => "Specify X scale factor or [Corner] <1>:".to_string(),
            Step::XyzY | Step::ScaleY => "Enter Y scale factor <use X scale factor>:".to_string(),
            Step::XyzZ => "Specify Z scale factor or <use X scale factor>:".to_string(),
            Step::Uniform => "Specify scale factor for XYZ axes <1>:".to_string(),
            Step::Rotation => "Specify rotation angle <0>:".to_string(),
            Step::FillAttr { attdefs, idx, .. } => {
                if let Some(ad) = attdefs.get(*idx) {
                    let default_hint = if ad.default_value.is_empty() {
                        String::new()
                    } else {
                        format!("  <{}>", ad.default_value)
                    };
                    let prompt_text = if ad.prompt.is_empty() {
                        ad.tag.as_str()
                    } else {
                        ad.prompt.as_str()
                    };
                    t!(
                        "INSERT  %{prompt}%{hint}:",
                        prompt = prompt_text,
                        hint = default_hint
                    )
                    .into_owned()
                } else {
                    t!("INSERT  Filling attributes...").into_owned()
                }
            }
        }
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        match self.step {
            Step::Point => {
                self.point = Some(pt);
                self.after_point()
            }
            Step::Preset(Preset::Base) => {
                self.base = self.plane.to_local(pt);
                self.step = Step::Point;
                CmdResult::NeedPoint
            }
            Step::ScaleX | Step::Corner | Step::XyzX => {
                let Some(point) = self.point else {
                    return CmdResult::NeedPoint;
                };
                let d = self.plane.to_local(pt) - self.plane.to_local(point);
                if d.x.abs() < 1e-12 || d.y.abs() < 1e-12 {
                    return CmdResult::ReportError("Value must be nonzero.".to_string());
                }
                self.scale = [d.x, d.y, d.x];
                self.after_scale()
            }
            Step::Rotation => {
                let Some(point) = self.point else {
                    return CmdResult::NeedPoint;
                };
                match self.plane.angle(point, pt) {
                    Some(angle) => {
                        self.rotation = angle;
                        self.place()
                    }
                    None => CmdResult::NeedPoint,
                }
            }
            _ => CmdResult::NeedPoint,
        }
    }

    fn options(&self) -> Vec<crate::command::CmdOption> {
        match &self.step {
            Step::Name => self
                .picker
                .filtered()
                .iter()
                .map(|n| crate::command::CmdOption::new(n, n))
                .collect(),
            _ => Vec::new(),
        }
    }

    fn on_live_input(&mut self, input: &str) -> bool {
        if !matches!(self.step, Step::Name) {
            return false;
        }
        let needle = input.trim();
        if needle == self.picker.needle() || needle == "?" {
            return false;
        }
        self.picker.set_needle(needle.to_string());
        true
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Name => {
                if self.default_name.is_empty() {
                    CmdResult::Cancel
                } else {
                    let name = self.default_name.clone();
                    match self.picker.contains_name(&name) {
                        Some(canonical) => self.choose(canonical),
                        None => self.not_found(&name),
                    }
                }
            }
            Step::ListPattern => CmdResult::Measurement(self.listing("*")),
            Step::Point => {
                if self.repeat && self.placed > 0 {
                    CmdResult::Cancel
                } else if self.mode == Mode::Paste {
                    CmdResult::Cancel
                } else {
                    CmdResult::ReportError("Point or option keyword required.".to_string())
                }
            }
            Step::Preset(_) => {
                self.step = Step::Point;
                CmdResult::NeedPoint
            }
            Step::YesNo(ask) => self.answer(ask, None),
            Step::ScaleX | Step::XyzX => {
                self.scale[0] = 1.0;
                self.step = if matches!(self.step, Step::XyzX) { Step::XyzY } else { Step::ScaleY };
                CmdResult::NeedPoint
            }
            Step::XyzY => {
                self.scale[1] = self.scale[0];
                self.step = Step::XyzZ;
                CmdResult::NeedPoint
            }
            Step::XyzZ => {
                self.scale[2] = self.scale[0];
                self.after_scale()
            }
            Step::ScaleY => {
                self.scale[1] = self.scale[0];
                self.scale[2] = self.scale[0];
                self.after_scale()
            }
            Step::Uniform => {
                self.scale = [1.0; 3];
                self.after_scale()
            }
            Step::Rotation => {
                self.rotation = 0.0;
                self.place()
            }
            Step::Corner => CmdResult::NeedPoint,
            Step::FillAttr { .. } => self.accept_attr_value(""),
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::FillAttr { .. } => InputKind::FreeText,
            Step::Name | Step::ListPattern | Step::YesNo(_) => InputKind::SingleToken,
            Step::Point | Step::Preset(Preset::Base) | Step::Corner => InputKind::Point,
            _ => InputKind::SingleToken,
        }
    }

    fn dyn_field(&self) -> DynField {
        match self.step {
            Step::Rotation | Step::Preset(Preset::Rotate) => DynField::Angle,
            Step::ScaleX
            | Step::ScaleY
            | Step::XyzX
            | Step::XyzY
            | Step::XyzZ
            | Step::Uniform
            | Step::Preset(_) => DynField::Scalar,
            _ => DynField::Point,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        matches!(self.step, Step::Point)
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let t = text.trim();
        let up = t.to_ascii_uppercase();
        Some(match self.step {
            Step::Name => {
                if t == "?" {
                    self.step = Step::ListPattern;
                    return Some(CmdResult::NeedPoint);
                }
                if t.is_empty() {
                    self.picker.set_needle(String::new());
                    return Some(CmdResult::NeedPoint);
                }
                match self.picker.contains_name(t) {
                    Some(canonical) => self.choose(canonical),
                    None => self.not_found(t),
                }
            }
            Step::ListPattern => CmdResult::Measurement(self.listing(t)),
            Step::Point => match up.as_str() {
                "B" | "BASEPOINT" => {
                    self.step = Step::Preset(Preset::Base);
                    CmdResult::NeedPoint
                }
                "S" | "SCALE" => {
                    self.step = Step::Preset(Preset::Scale);
                    CmdResult::NeedPoint
                }
                "X" => {
                    self.step = Step::Preset(Preset::X);
                    CmdResult::NeedPoint
                }
                "Y" => {
                    self.step = Step::Preset(Preset::Y);
                    CmdResult::NeedPoint
                }
                "Z" => {
                    self.step = Step::Preset(Preset::Z);
                    CmdResult::NeedPoint
                }
                "R" | "ROTATE" => {
                    self.step = Step::Preset(Preset::Rotate);
                    CmdResult::NeedPoint
                }
                "E" | "EXPLODE" if self.mode == Mode::Classic => {
                    self.step = Step::YesNo(Ask::Explode);
                    CmdResult::NeedPoint
                }
                "RE" | "REPEAT" if self.mode == Mode::Classic => {
                    self.step = Step::YesNo(Ask::Repeat);
                    CmdResult::NeedPoint
                }
                _ => CmdResult::ReportError("Invalid option keyword.".to_string()),
            },
            Step::Preset(kind) => {
                if kind == Preset::Rotate {
                    match crate::entities::common::parse_typed_angle(t) {
                        Some(angle) => self.preset_rotation = Some(angle),
                        None => {
                            return Some(CmdResult::ReportError(
                                "Requires valid numeric angle or second point.".to_string(),
                            ))
                        }
                    }
                } else {
                    match Self::number(t) {
                        Some(v) if v != 0.0 => match kind {
                            Preset::Scale => self.preset_scale = [Some(v); 3],
                            Preset::X => self.preset_scale[0] = Some(v),
                            Preset::Y => self.preset_scale[1] = Some(v),
                            Preset::Z => self.preset_scale[2] = Some(v),
                            _ => {}
                        },
                        Some(_) => {
                            return Some(CmdResult::ReportError("Value must be nonzero.".to_string()))
                        }
                        None => {
                            return Some(CmdResult::ReportError(
                                "Requires numeric value.".to_string(),
                            ))
                        }
                    }
                    // A single axis given: the others stay at 1.
                    if self.preset_scale.iter().any(Option::is_some) {
                        for axis in &mut self.preset_scale {
                            axis.get_or_insert(1.0);
                        }
                    }
                }
                self.step = Step::Point;
                CmdResult::NeedPoint
            }
            Step::YesNo(ask) => self.answer(ask, Some(t)),
            Step::ScaleX | Step::XyzX => match up.as_str() {
                "C" | "CORNER" => {
                    self.step = Step::Corner;
                    CmdResult::NeedPoint
                }
                "XYZ" if matches!(self.step, Step::ScaleX) => {
                    self.step = Step::XyzX;
                    CmdResult::NeedPoint
                }
                _ => match Self::number(t) {
                    Some(v) if v != 0.0 => {
                        self.scale[0] = v;
                        self.step = if matches!(self.step, Step::XyzX) { Step::XyzY } else { Step::ScaleY };
                        CmdResult::NeedPoint
                    }
                    Some(_) => CmdResult::ReportError("Value must be nonzero.".to_string()),
                    None => CmdResult::ReportError("Requires numeric value.".to_string()),
                },
            },
            Step::ScaleY | Step::XyzY | Step::XyzZ | Step::Uniform => match Self::number(t) {
                Some(v) if v != 0.0 => match self.step {
                    Step::ScaleY => {
                        self.scale[1] = v;
                        self.scale[2] = self.scale[0];
                        self.after_scale()
                    }
                    Step::XyzY => {
                        self.scale[1] = v;
                        self.step = Step::XyzZ;
                        CmdResult::NeedPoint
                    }
                    Step::XyzZ => {
                        self.scale[2] = v;
                        self.after_scale()
                    }
                    _ => {
                        self.scale = [v; 3];
                        self.after_scale()
                    }
                },
                Some(_) => CmdResult::ReportError("Value must be nonzero.".to_string()),
                None => CmdResult::ReportError("Requires numeric value.".to_string()),
            },
            Step::Rotation => match crate::entities::common::parse_typed_angle(t) {
                Some(angle) => {
                    self.rotation = angle;
                    self.place()
                }
                None => CmdResult::ReportError(
                    "Requires valid numeric angle or second point.".to_string(),
                ),
            },
            Step::Corner => CmdResult::NeedPoint,
            Step::FillAttr { .. } => self.accept_attr_value(text),
        })
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> {
        if self.preview.is_empty() || !matches!(self.step, Step::Point) {
            return Vec::new();
        }
        let scale = [
            self.preset_scale[0].unwrap_or(1.0),
            self.preset_scale[1].unwrap_or(1.0),
            self.preset_scale[2].unwrap_or(1.0),
        ];
        let (sin, cos) = self.preset_rotation.unwrap_or(0.0).sin_cos();
        let base = self.base;
        let origin = self.plane.to_local(pt);
        let plane = self.plane;
        self.preview
            .iter()
            .map(|w| {
                w.mapped(|p| {
                    let d = p - base;
                    let s = DVec3::new(d.x * scale[0], d.y * scale[1], d.z * scale[2]);
                    let r = DVec3::new(s.x * cos - s.y * sin, s.x * sin + s.y * cos, s.z);
                    plane.to_world(origin + r)
                })
            })
            .collect()
    }

    fn attreq_continue(&self) -> bool {
        self.repeat
    }

    fn attreq_set_attdefs(&mut self, attdefs: Vec<AttributeDefinition>) -> Option<codec::EntityType> {
        self.step = Step::FillAttr {
            attdefs,
            idx: 0,
            values: vec![],
        };
        self.advance_automatic_attributes()
    }

    fn attreq_take_insert(&mut self) -> Option<codec::EntityType> {
        self.pending_insert.take().map(EntityType::Insert)
    }
}

impl InsertBlockCommand {
    fn answer(&mut self, ask: Ask, text: Option<&str>) -> CmdResult {
        let default = !matches!(ask, Ask::RepeatScale | Ask::RepeatRotate);
        let Some(yes) = Self::yes_no(text.unwrap_or(""), default) else {
            return CmdResult::ReportError("Invalid option keyword.".to_string());
        };
        self.step = match ask {
            Ask::Explode => {
                self.explode = yes;
                Step::Point
            }
            Ask::Repeat => {
                self.repeat = yes;
                if yes { Step::YesNo(Ask::RepeatScale) } else { Step::Point }
            }
            Ask::RepeatScale => {
                self.repeat_scale = yes;
                Step::YesNo(Ask::RepeatRotate)
            }
            Ask::RepeatRotate => {
                self.repeat_rotation = yes;
                Step::Point
            }
        };
        CmdResult::NeedPoint
    }

    fn accept_attr_value(&mut self, text: &str) -> CmdResult {
        let (attdef_idx, default) = match &self.step {
            Step::FillAttr { attdefs, idx, .. } => {
                let Some(ad) = attdefs.get(*idx) else {
                    return CmdResult::Cancel;
                };
                (*idx, ad.default_value.clone())
            }
            _ => return CmdResult::Cancel,
        };
        let value = if text.trim().is_empty() {
            default
        } else {
            text.trim().to_string()
        };
        if let Step::FillAttr {
            ref mut values,
            ref mut idx,
            ..
        } = self.step
        {
            values.push((attdef_idx, value));
            *idx = attdef_idx + 1;
        }
        match self.advance_automatic_attributes() {
            Some(entity) if self.repeat => {
                self.step = Step::Point;
                CmdResult::CommitEntity(entity)
            }
            Some(entity) => CmdResult::CommitAndExit(entity),
            None => CmdResult::NeedPoint,
        }
    }

    fn advance_automatic_attributes(&mut self) -> Option<EntityType> {
        loop {
            let next = match &self.step {
                Step::FillAttr { attdefs, idx, .. } => attdefs.get(*idx).map(|ad| {
                    (*idx, ad.flags.constant, ad.flags.preset, ad.default_value.clone())
                }),
                _ => return None,
            };
            let Some((attdef_idx, constant, preset, default)) = next else {
                return self.finish_insert();
            };
            if !constant && !preset {
                return None;
            }
            if let Step::FillAttr { idx, values, .. } = &mut self.step {
                if !constant {
                    values.push((attdef_idx, default));
                }
                *idx += 1;
            }
        }
    }

    fn finish_insert(&mut self) -> Option<EntityType> {
        let (attdefs, values) = match &self.step {
            Step::FillAttr { attdefs, values, .. } => (attdefs.clone(), values.clone()),
            _ => return None,
        };
        let mut insert = self.pending_insert.take()?;
        let transform = insert.get_transform();
        for (attdef_idx, value) in values {
            let Some(attdef) = attdefs.get(attdef_idx) else {
                continue;
            };
            let mut attribute = AttributeEntity::from_definition(attdef, Some(value));
            attribute.apply_transform(&transform);
            insert.attributes.push(attribute);
        }
        if self.repeat {
            self.step = Step::Point;
        }
        Some(EntityType::Insert(insert))
    }
}

// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["INSERT", "-INSERT"] });
