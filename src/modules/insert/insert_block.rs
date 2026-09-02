use acadrust::entities::{AttributeDefinition, AttributeEntity, Entity, Insert};
use acadrust::types::Vector3;
use acadrust::EntityType;
use glam::{DVec3, Vec3};
use crate::t;

use crate::command::{CadCommand, CmdResult, InputKind, WorkingPlane};
use crate::modules::{IconKind, ModuleEvent, ToolDef};
use crate::scene::model::wire_model::WireModel;

pub fn tool() -> ToolDef {
    ToolDef {
        id: "INSERT",
        label: "Insert Block",
        icon: IconKind::Svg(include_bytes!("../../../assets/icons/blocks/insert.svg")),
        event: ModuleEvent::Command("INSERT".to_string()),
    }
}

enum Step {
    Name,
    Point {
        name: String,
    },
    FillAttr {
        attdefs: Vec<AttributeDefinition>,
        idx: usize,
        values: Vec<(usize, String)>,
    },
}

/// Which numeric value the insertion-point step is currently waiting for after
/// a Scale / Rotate keyword.
#[derive(Clone, Copy)]
enum AwaitKind {
    Scale,
    Rotation,
}

pub struct InsertBlockCommand {
    picker: crate::modules::insert::picker::BlockPicker,
    step: Step,
    /// Uniform X/Y scale applied to the placed block (default 1).
    x_scale: f64,
    y_scale: f64,
    /// Rotation applied to the placed block, in radians (default 0).
    rotation_rad: f64,
    /// Set while a Scale/Rotate value is being typed at the insertion step.
    awaiting: Option<AwaitKind>,
    /// Pending Insert entity stored while attr-filling is in progress.
    pending_insert: Option<Insert>,
    /// Optional drag preview: the block's wire geometry plus the base point it
    /// is measured from, so `on_preview_wires` can rubber-band it to the
    /// cursor. Set by paste-as-block; empty for a plain INSERT.
    preview: Option<(Vec<WireModel>, Vec3)>,
    plane: WorkingPlane,
}

impl InsertBlockCommand {
    /// Construct with explicit usage ranking. `usage` maps uppercase block name → (frequency, MRU position).
    /// `cliprompt_lines` directly controls suggestion count (relation per spec).
    pub fn new_with_usage(
        available: Vec<String>,
        usage_rank: rustc_hash::FxHashMap<String, (u32, usize)>,
        cliprompt_lines: u8,
    ) -> Self {
        let limit = (cliprompt_lines as usize).clamp(0, crate::modules::insert::picker::MAX_SUGGESTIONS);
        let picker = crate::modules::insert::picker::BlockPicker::new(available, usage_rank, limit);
        Self {
            picker,
            step: Step::Name,
            x_scale: 1.0,
            y_scale: 1.0,
            rotation_rad: 0.0,
            awaiting: None,
            pending_insert: None,
            preview: None,
            plane: WorkingPlane::default(),
        }
    }

    /// Start the command already locked to `name`, skipping the name prompt and
    /// going straight to "specify insertion point". `preview_wires` (measured
    /// from `base`) rubber-band under the cursor. Used by paste-as-block, which
    /// has just defined the block and only needs the drop point.
    pub fn new_for_block(name: String, preview_wires: Vec<WireModel>, base: Vec3) -> Self {
        // Minimal picker for the locked-name path; not used for Name step.
        let picker = crate::modules::insert::picker::BlockPicker::new(
            vec![name.clone()],
            rustc_hash::FxHashMap::default(),
            0,
        );
        Self {
            picker,
            step: Step::Point { name },
            x_scale: 1.0,
            y_scale: 1.0,
            rotation_rad: 0.0,
            awaiting: None,
            pending_insert: None,
            preview: Some((preview_wires, base)),
            plane: WorkingPlane::default(),
        }
    }

}

impl CadCommand for InsertBlockCommand {
    fn set_working_plane(&mut self, plane: WorkingPlane) {
        self.plane = plane;
    }

    fn name(&self) -> &'static str {
        "INSERT"
    }

    fn prompt(&self) -> String {
        match &self.step {
            Step::Name => {
                if self.picker.is_empty() {
                    return t!("INSERT  Enter block name:").into_owned();
                }
                let needle = self.picker.needle();
                let filtered = self.picker.filtered();
                if !needle.is_empty() && filtered.is_empty() {
                    return t!(
                        "INSERT  No matching blocks for \"%{needle}\"",
                        needle = needle
                    )
                    .into_owned();
                }
                if needle.is_empty() {
                    let total = self.picker.total();
                    let shown = filtered.len();
                    if total <= shown {
                        t!("INSERT  Enter block name:").into_owned()
                    } else {
                        t!(
                            "INSERT  Enter block name:  [%{shown} of %{total} — type to search]",
                            shown = shown,
                            total = total
                        )
                        .into_owned()
                    }
                } else {
                    t!(
                        "INSERT  Enter block name:  \"%{needle}\"  [%{shown} matches]",
                        needle = needle,
                        shown = filtered.len()
                    )
                    .into_owned()
                }
            }
            Step::Point { name } => match self.awaiting {
                Some(AwaitKind::Scale) => t!("INSERT  Specify scale factor <1>:").into_owned(),
                Some(AwaitKind::Rotation) => t!("INSERT  Specify rotation angle <0>:").into_owned(),
                None => t!(
                    "INSERT  Specify insertion point for \"%{name}\"  [Scale/Rotate]:",
                    name = name
                )
                .into_owned(),
            },
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
        match &self.step {
            Step::Name => CmdResult::NeedPoint,
            Step::Point { name } => {
                let point = self.plane.to_local(pt);
                let mut ins = Insert::new(
                    name.clone(),
                    Vector3::new(point.x, point.y, point.z),
                );
                ins.set_x_scale(self.x_scale);
                ins.set_y_scale(self.y_scale);
                ins.rotation = self.rotation_rad;
                ins.apply_transform(&self.plane.to_world_transform());
                let block_name = name.clone();
                self.pending_insert = Some(ins);
                // Signal the host to check for attdefs.
                CmdResult::AttreqNeeded { block_name }
            }
            Step::FillAttr { .. } => CmdResult::NeedPoint,
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
        // Performance: only recompute if needle actually changed; picker
        // uses upper/lower caches and partial sort so per-keystroke is <0.2ms.
        let needle = input.trim();
        if needle == self.picker.needle() {
            return false;
        }
        self.picker.set_needle(needle.to_string());
        true
    }

    fn on_enter(&mut self) -> CmdResult {
        match &self.step {
            // A bare Enter while a scale/rotation value is awaited keeps the
            // current default instead of cancelling the command.
            Step::Point { .. } if self.awaiting.is_some() => {
                self.awaiting = None;
                CmdResult::NeedPoint
            }
            Step::Name | Step::Point { .. } => CmdResult::Cancel,
            Step::FillAttr { .. } => {
                // Treat Enter as accepting the default.
                self.accept_attr_value("")
            }
        }
    }

    fn input_kind(&self) -> InputKind {
        match self.step {
            Step::FillAttr { .. } => InputKind::FreeText,
            Step::Name | Step::Point { .. } => InputKind::SingleToken,
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        // The insertion-point step also takes Scale / Rotate keywords while
        // still accepting a point pick. Once a value is being typed (awaiting),
        // route the whole number through `on_text_input` instead.
        matches!(self.step, Step::Point { .. }) && self.awaiting.is_none()
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        match &self.step {
            Step::Name => {
                let name = text.trim();
                // Empty input: reset needle to show default ranked list (2.1).
                // Consumed so prompt+buttons refresh, not stale filter.
                if name.is_empty() {
                    self.picker.set_needle(String::new());
                    return Some(CmdResult::NeedPoint);
                }
                // Exact block name (case-insensitive) → accept and go to point step.
                // This path is used both for typed exact names and for CmdOption
                // button clicks (Message::CommandOptionPick feeds the keyword through
                // on_text_input). Returning NeedPoint keeps the prompt updated.
                if let Some(canonical) = self.picker.contains_name(name) {
                    self.step = Step::Point { name: canonical };
                    return Some(CmdResult::NeedPoint);
                }
                // Not an exact match → treat as incremental search needle.
                // Update filter and re-render prompt + option buttons. Must return
                // Some(NeedPoint) (consumed) not None, otherwise the driver would
                // offer the same text to the command a second time.
                self.picker.set_needle(name.to_string());
                Some(CmdResult::NeedPoint)
            }
            Step::FillAttr { .. } => Some(self.accept_attr_value(text)),
            Step::Point { .. } => {
                // Typing the value awaited after a Scale / Rotate keyword. The
                // two read differently: a scale is a plain multiplier with no
                // unit to write it in, while a rotation is an angle and takes
                // whichever convention the drawing is set to.
                if let Some(kind) = self.awaiting {
                    match kind {
                        AwaitKind::Scale => {
                            if let Ok(v) = text.trim().parse::<f64>() {
                                if v != 0.0 {
                                    self.x_scale = v;
                                    self.y_scale = v;
                                }
                            }
                        }
                        AwaitKind::Rotation => {
                            if let Some(v) = crate::entities::common::parse_typed_angle(text) {
                                self.rotation_rad = v;
                            }
                        }
                    }
                    self.awaiting = None;
                    return Some(CmdResult::NeedPoint);
                }
                match text.trim().to_uppercase().as_str() {
                    "S" | "SCALE" => {
                        self.awaiting = Some(AwaitKind::Scale);
                        Some(CmdResult::NeedPoint)
                    }
                    "R" | "ROTATE" => {
                        self.awaiting = Some(AwaitKind::Rotation);
                        Some(CmdResult::NeedPoint)
                    }
                    _ => None,
                }
            }
        }
    }

    fn on_preview_wires(&mut self, pt: DVec3) -> Vec<WireModel> { let pt = pt.as_vec3();
        match (&self.step, &self.preview) {
            (Step::Point { .. }, Some((wires, base))) => {
                let delta = pt - *base;
                wires.iter().map(|w| w.translated(delta)).collect()
            }
            _ => vec![],
        }
    }

    fn attreq_set_attdefs(
        &mut self,
        attdefs: Vec<AttributeDefinition>,
    ) -> Option<acadrust::EntityType> {
        self.step = Step::FillAttr {
            attdefs,
            idx: 0,
            values: vec![],
        };
        self.advance_automatic_attributes()
    }

    fn attreq_take_insert(&mut self) -> Option<acadrust::EntityType> {
        self.pending_insert
            .take()
            .map(|ins| EntityType::Insert(ins))
    }
}

impl InsertBlockCommand {
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
            Step::FillAttr {
                attdefs, values, ..
            } => (attdefs.clone(), values.clone()),
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
        Some(EntityType::Insert(insert))
    }
}


// ── Autocomplete registry ─────────────────────────────────
inventory::submit!(crate::command::CommandRegistration { names: &["INSERT"] });  // InsertBlockCommand
