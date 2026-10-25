// TEXTALIGN translates text using rendered glyph ink extents. Rotated text uses
// its world-axis box, rather than a box in the text's own frame.

use codec::entities::{TextHorizontalAlignment as HA, TextVerticalAlignment as VA};
use codec::{EntityType, Handle};
use glam::DVec3;

use crate::command::{CadCommand, CmdOption, CmdResult, InputKind};
use crate::scene::Scene;
use crate::t;

#[derive(Clone, Copy)]
enum Alignment {
    Left,
    Center,
    Right,
}

impl Alignment {
    fn label(self) -> String {
        match self {
            Self::Left => t!("Left").into_owned(),
            Self::Center => t!("Center").into_owned(),
            Self::Right => t!("Right").into_owned(),
        }
    }

    fn fraction(self) -> f64 {
        match self {
            Self::Left => 0.0,
            Self::Center => 0.5,
            Self::Right => 1.0,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SpacingMode {
    Vertical,
    Horizontal,
    Distribute,
    Spacing,
}

impl SpacingMode {
    fn label(self) -> String {
        match self {
            Self::Vertical => t!("Current Vertical").into_owned(),
            Self::Horizontal => t!("Current Horizontal").into_owned(),
            Self::Distribute => t!("Distribute").into_owned(),
            Self::Spacing => t!("Set spacing").into_owned(),
        }
    }
}

#[derive(Clone, Copy)]
struct Settings {
    alignment: Alignment,
    mode: SpacingMode,
    spacing: f64,
}

static SETTINGS: std::sync::Mutex<Settings> = std::sync::Mutex::new(Settings {
    alignment: Alignment::Left,
    mode: SpacingMode::Vertical,
    spacing: 1.0,
});

#[derive(Clone, Copy, PartialEq)]
enum Step {
    Select,
    Base,
    Point,
    Second,
    Alignment,
    Options,
    Spacing,
}

struct TextItem {
    handle: Handle,
    entity: EntityType,
    min: DVec3,
    max: DVec3,
}

fn is_text(entity: &EntityType) -> bool {
    matches!(
        entity,
        EntityType::Text(_) | EntityType::MText(_) | EntityType::AttributeDefinition(_)
    )
}

/// Rotation, cap height and width factor the glyph quads were laid out with.
/// MTEXT uses its nominal height; inline height or width changes are not seen.
fn glyph_frame(entity: &EntityType, document: &codec::CadDocument) -> (f64, f64, f64) {
    match entity {
        EntityType::Text(text) => {
            let placement = crate::entities::text::text_run_placement_at_scale(text, document, 1.0);
            (
                f64::from(placement.rotation),
                f64::from(placement.height),
                f64::from(placement.width_factor),
            )
        }
        EntityType::AttributeDefinition(text) => (text.rotation, text.height, text.width_factor),
        EntityType::MText(text) => (text.rotation, text.height, 1.0),
        _ => (0.0, 0.0, 1.0),
    }
}

impl TextItem {
    fn from_scene(scene: &Scene, handle: Handle) -> Option<Self> {
        let entity = scene.document.get_entity(handle)?;
        if !is_text(entity) {
            return None;
        }
        // Reuse the glyph quads produced for picking and zoom, including the
        // low coordinates so large drawing coordinates retain their precision.
        // Bounds are taken in the text's own frame, where every quad is its
        // glyph's ink box plus the SDF margin, so the margin comes off exactly.
        let (rotation, height, width_factor) = glyph_frame(entity, &scene.document);
        let (sin, cos) = rotation.sin_cos();
        let wires = scene.wire_models_for(&[handle]);
        let mut lo = DVec3::splat(f64::INFINITY);
        let mut hi = DVec3::splat(f64::NEG_INFINITY);
        for vertex in wires.iter().flat_map(|wire| &wire.text_verts) {
            let x = vertex.pos[0] as f64 + vertex.pos_low[0] as f64;
            let y = vertex.pos[1] as f64 + vertex.pos_low[1] as f64;
            let z = vertex.pos[2] as f64 + vertex.pos_low[2] as f64;
            let local = DVec3::new(x * cos + y * sin, y * cos - x * sin, z);
            if local.is_finite() {
                lo = lo.min(local);
                hi = hi.max(local);
            }
        }
        if !(lo.is_finite() && hi.is_finite()) {
            return None;
        }
        let margin = f64::from(crate::scene::text::sdf_atlas::SPREAD_UNITS) * height / 9.0;
        let inset = DVec3::new(margin * width_factor.abs(), margin, 0.0);
        let (lo, hi) = ((lo + inset).min(hi - inset), (hi - inset).max(lo + inset));
        let mut min = DVec3::splat(f64::INFINITY);
        let mut max = DVec3::splat(f64::NEG_INFINITY);
        for (x, y) in [(lo.x, lo.y), (hi.x, lo.y), (hi.x, hi.y), (lo.x, hi.y)] {
            let corner = DVec3::new(x * cos - y * sin, x * sin + y * cos, lo.z);
            min = min.min(corner);
            max = max.max(corner);
        }
        (min.is_finite() && max.is_finite()).then(|| Self {
            handle,
            entity: entity.clone(),
            min,
            max,
        })
    }

    fn edge(&self, settings: Settings) -> DVec3 {
        let mut edge = self.min;
        if settings.mode == SpacingMode::Horizontal {
            edge.y += (self.max.y - self.min.y) * settings.alignment.fraction();
        } else {
            edge.x += (self.max.x - self.min.x) * settings.alignment.fraction();
        }
        edge.z = 0.0;
        edge
    }
}

pub struct TextAlignCommand {
    step: Step,
    return_step: Step,
    settings: Settings,
    handles: Vec<Handle>,
    items: Vec<TextItem>,
    selection_dirty: bool,
    picked_handle: Option<Handle>,
    picked: Option<TextItem>,
    base: Option<TextItem>,
    point: Option<DVec3>,
}

impl TextAlignCommand {
    pub fn new(handles: Vec<Handle>) -> Self {
        Self {
            step: Step::Select,
            return_step: Step::Select,
            settings: *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()),
            handles,
            items: Vec::new(),
            selection_dirty: true,
            picked_handle: None,
            picked: None,
            base: None,
            point: None,
        }
    }

    pub fn settings_message(&self) -> String {
        t!(
            "Current settings: Align = %{alignment}, Spacing Mode = %{spacing}",
            alignment = self.settings.alignment.label(),
            spacing = self.settings.mode.label()
        )
        .into_owned()
    }

    fn remember(&self) {
        *SETTINGS.lock().unwrap_or_else(|e| e.into_inner()) = self.settings;
    }

    fn origin(&self) -> Option<DVec3> {
        self.base
            .as_ref()
            .map(|base| base.edge(self.settings))
            .or(self.point)
    }

    fn commit(&self, end: DVec3) -> CmdResult {
        let Some(origin) = self.origin() else {
            return CmdResult::NeedPoint;
        };
        let direction = DVec3::new(end.x - origin.x, end.y - origin.y, 0.0);
        if !direction.is_finite() || direction.length_squared() < 1e-18 {
            return CmdResult::NeedPoint;
        }
        let mode = self.settings.mode;
        // A direction (almost) across the spacing axis has no usable slope.
        let across = 1e-6 * direction.length();
        if (mode == SpacingMode::Vertical && direction.y.abs() < across)
            || (mode == SpacingMode::Horizontal && direction.x.abs() < across)
        {
            return CmdResult::NeedPoint;
        }
        let fixed = self.base.as_ref().map(|base| base.handle);
        let base_selected = self.items.iter().any(|item| Some(item.handle) == fixed);
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|item| Some(item.handle) != fixed)
            .collect();
        if matches!(mode, SpacingMode::Distribute | SpacingMode::Spacing) {
            items.sort_by(|a, b| {
                let projection =
                    |item: &TextItem| (item.edge(self.settings) - origin).dot(direction);
                projection(a)
                    .total_cmp(&projection(b))
                    .then_with(|| a.handle.value().cmp(&b.handle.value()))
            });
        }
        // A selected base reserves the first distribution slot without moving.
        let intervals = (items.len() + usize::from(base_selected))
            .saturating_sub(1)
            .max(1);
        let mut replacements = Vec::new();
        for (index, item) in items.iter().enumerate() {
            let edge = item.edge(self.settings);
            let delta = match mode {
                SpacingMode::Vertical => DVec3::new(
                    origin.x + (edge.y - origin.y) * direction.x / direction.y - edge.x,
                    0.0,
                    0.0,
                ),
                SpacingMode::Horizontal => DVec3::new(
                    0.0,
                    origin.y + (edge.x - origin.x) * direction.y / direction.x - edge.y,
                    0.0,
                ),
                SpacingMode::Distribute => {
                    let slot = index + usize::from(base_selected);
                    origin + direction * (slot as f64 / intervals as f64) - edge
                }
                SpacingMode::Spacing => {
                    let distance = self.settings.spacing * (index + 1) as f64;
                    origin + direction.normalize() * distance - edge
                }
            };
            if !delta.is_finite() {
                return CmdResult::NeedPoint;
            }
            if delta.length_squared() > 1e-18 {
                let mut entity = item.entity.clone();
                translate(&mut entity, delta);
                replacements.push((item.handle, vec![entity]));
            }
        }
        if replacements.is_empty() {
            CmdResult::Cancel
        } else {
            // The standard replacement driver records the entire batch once.
            CmdResult::ReplaceMany(replacements, vec![])
        }
    }
}

fn translate(entity: &mut EntityType, delta: DVec3) {
    let normal = match entity {
        EntityType::Text(text) => Some(text.normal),
        EntityType::AttributeDefinition(text) => Some(text.normal),
        _ => None,
    };
    let (dx, dy, dz) = normal.map_or((delta.x, delta.y, delta.z), |normal| {
        crate::scene::view::transform::wcs_point_to_ocs(
            (delta.x, delta.y, delta.z),
            (normal.x, normal.y, normal.z),
        )
    });
    let shift = |point: &mut codec::types::Vector3| {
        point.x += dx;
        point.y += dy;
        point.z += dz;
    };
    match entity {
        EntityType::Text(text) => {
            shift(&mut text.insertion_point);
            let plain_left = matches!(text.horizontal_alignment, HA::Left)
                && matches!(text.vertical_alignment, VA::Baseline);
            if !plain_left {
                if let Some(point) = &mut text.alignment_point {
                    shift(point);
                }
            }
        }
        EntityType::MText(text) => shift(&mut text.insertion_point),
        EntityType::AttributeDefinition(text) => {
            shift(&mut text.insertion_point);
            shift(&mut text.alignment_point);
        }
        _ => {}
    }
}

impl CadCommand for TextAlignCommand {
    fn name(&self) -> &'static str {
        "TEXTALIGN"
    }

    fn prompt(&self) -> String {
        match self.step {
            Step::Select => t!("Select text objects to align [alIgnment/Options]:"),
            Step::Base => t!("Select text object to align to [Point]:"),
            Step::Point => t!("Specify point:"),
            Step::Second => t!("Pick second point or [Options]:"),
            Step::Alignment => t!("Select alignment [Left/Center/Right] <Left>:"),
            Step::Options => t!(
                "Select spacing mode [Distribute/Set spacing/Current vertical/Current horizontal]:"
            ),
            Step::Spacing => t!("Specify spacing distance:"),
        }
        .into_owned()
    }

    fn options(&self) -> Vec<CmdOption> {
        let options = match self.step {
            Step::Select => vec![(t!("alIgnment"), "I"), (t!("Options"), "O")],
            Step::Base => vec![(t!("Point"), "P")],
            Step::Second => vec![(t!("Options"), "O")],
            Step::Alignment => vec![(t!("Left"), "L"), (t!("Center"), "C"), (t!("Right"), "R")],
            Step::Options => vec![
                (t!("Distribute"), "D"),
                (t!("Set spacing"), "S"),
                (t!("Current vertical"), "V"),
                (t!("Current horizontal"), "H"),
            ],
            _ => vec![],
        };
        options
            .into_iter()
            .map(|(label, keyword)| CmdOption::new(label.as_ref(), keyword))
            .collect()
    }

    fn input_kind(&self) -> InputKind {
        if matches!(self.step, Step::Alignment | Step::Options | Step::Spacing) {
            InputKind::SingleToken
        } else {
            InputKind::Point
        }
    }

    fn point_step_accepts_keywords(&self) -> bool {
        true
    }

    fn is_selection_gathering(&self) -> bool {
        self.step == Step::Select
    }

    fn selection_entity_filter(&self) -> Option<fn(&EntityType) -> bool> {
        Some(is_text)
    }

    fn selection_rejects_locked_layers(&self) -> bool {
        true
    }

    fn wants_point_pick_context(&self) -> bool {
        true
    }

    fn needs_entity_pick(&self) -> bool {
        self.step == Step::Base
    }

    fn inject_before_entity_pick(&self) -> bool {
        true
    }

    fn inject_picked_entity(&mut self, entity: EntityType) {
        self.picked_handle = Some(entity.common().handle);
    }

    fn refresh_point_pick_scene(&mut self, scene: &Scene) {
        if self.selection_dirty {
            self.items = self
                .handles
                .iter()
                .filter(|h| !scene.is_layer_locked(**h))
                .filter_map(|h| TextItem::from_scene(scene, *h))
                .collect();
            self.selection_dirty = false;
        }
        if let Some(handle) = self.picked_handle.take() {
            self.picked = TextItem::from_scene(scene, handle);
        }
    }

    fn on_selection_complete(&mut self, handles: Vec<Handle>) -> CmdResult {
        self.handles = handles;
        self.selection_dirty = true;
        CmdResult::NeedPoint
    }

    fn on_entity_pick(&mut self, handle: Handle, _pt: DVec3) -> CmdResult {
        if self.step == Step::Base {
            if let Some(item) = self.picked.take().filter(|item| item.handle == handle) {
                self.base = Some(item);
                self.step = Step::Second;
            }
        }
        CmdResult::NeedPoint
    }

    fn on_point(&mut self, pt: DVec3) -> CmdResult {
        if !pt.is_finite() {
            return CmdResult::NeedPoint;
        }
        match self.step {
            Step::Point => {
                self.point = Some(DVec3::new(pt.x, pt.y, 0.0));
                self.step = Step::Second;
                CmdResult::NeedPoint
            }
            Step::Second => self.commit(pt),
            _ => CmdResult::NeedPoint,
        }
    }

    fn on_enter(&mut self) -> CmdResult {
        match self.step {
            Step::Select if self.items.is_empty() => return CmdResult::Cancel,
            Step::Select => self.step = Step::Base,
            Step::Alignment => {
                self.settings.alignment = Alignment::Left;
                self.remember();
                self.step = self.return_step;
            }
            Step::Options => {
                self.settings.mode = SpacingMode::Vertical;
                self.remember();
                self.step = self.return_step;
            }
            _ => return CmdResult::NeedPoint,
        }
        CmdResult::NeedPoint
    }

    fn on_escape(&mut self) -> CmdResult {
        CmdResult::Cancel
    }

    fn on_text_input(&mut self, text: &str) -> Option<CmdResult> {
        let text = text.trim().to_ascii_uppercase();
        match (self.step, text.as_str()) {
            (Step::Select, "I" | "ALIGNMENT") => {
                self.return_step = self.step;
                self.step = Step::Alignment;
            }
            (Step::Select | Step::Second, "O" | "OPTIONS") => {
                self.return_step = self.step;
                self.step = Step::Options;
            }
            (Step::Base, "P" | "POINT") => self.step = Step::Point,
            (Step::Alignment, value) => {
                self.settings.alignment = match value {
                    "L" | "LEFT" => Alignment::Left,
                    "C" | "CENTER" => Alignment::Center,
                    "R" | "RIGHT" => Alignment::Right,
                    _ => return None,
                };
                self.remember();
                self.step = self.return_step;
            }
            (Step::Options, value) => {
                self.settings.mode = match value {
                    "D" | "DISTRIBUTE" => SpacingMode::Distribute,
                    "S" | "SET SPACING" => {
                        self.step = Step::Spacing;
                        SpacingMode::Spacing
                    }
                    "V" | "CV" | "CURRENT VERTICAL" => SpacingMode::Vertical,
                    "H" | "CH" | "CURRENT HORIZONTAL" => SpacingMode::Horizontal,
                    _ => return None,
                };
                if self.step != Step::Spacing {
                    self.remember();
                    self.step = self.return_step;
                }
            }
            (Step::Spacing, _) => {
                let spacing = crate::entities::common::parse_typed_length(&text)?;
                if !spacing.is_finite() || spacing <= 0.0 {
                    return Some(CmdResult::NeedPoint);
                }
                self.settings.spacing = spacing;
                self.remember();
                self.step = self.return_step;
            }
            _ => return None,
        }
        Some(CmdResult::NeedPoint)
    }
}

inventory::submit!(crate::command::CommandRegistration {
    names: &["TEXTALIGN"]
});

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{
        AttributeDefinition, MText, Text, TextHorizontalAlignment, TextVerticalAlignment,
    };
    use codec::types::Vector3;

    fn text(value: &str, x: f64, y: f64) -> EntityType {
        EntityType::Text(Text {
            value: value.into(),
            height: 2.5,
            insertion_point: Vector3::new(x, y, 0.0),
            ..Default::default()
        })
    }

    fn fixture() -> (Scene, Vec<Handle>) {
        let mut scene = Scene::new();
        let handles = [
            text("A", 0.0, 0.0),
            text("B", 0.0, 13.0),
            text("CCCCCC", 20.0, 30.0),
        ]
        .into_iter()
        .map(|entity| scene.add_entity(entity))
        .collect();
        (scene, handles)
    }

    fn command(
        scene: &Scene,
        handles: &[Handle],
        alignment: Alignment,
        mode: SpacingMode,
    ) -> TextAlignCommand {
        let mut command = TextAlignCommand::new(handles.to_vec());
        command.settings = Settings {
            alignment,
            mode,
            spacing: 7.0,
        };
        command.refresh_point_pick_scene(scene);
        command.on_enter();
        command
    }

    fn pick_base(command: &mut TextAlignCommand, scene: &Scene, handle: Handle) {
        command.inject_picked_entity(scene.document.get_entity(handle).unwrap().clone());
        command.refresh_point_pick_scene(scene);
        command.on_entity_pick(handle, DVec3::ZERO);
    }

    fn apply(scene: &mut Scene, result: CmdResult) {
        let CmdResult::ReplaceMany(replacements, additions) = result else {
            panic!("expected translations")
        };
        assert!(!replacements.is_empty());
        assert!(additions.is_empty());
        for (_, entities) in replacements {
            assert_eq!(entities.len(), 1);
            for entity in entities {
                assert!(scene.update_entity(entity));
            }
        }
    }

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 2e-5, "{a} != {b}");
    }

    fn vertical(alignment: Alignment) {
        let (mut scene, handles) = fixture();
        let original: Vec<_> = handles
            .iter()
            .map(|h| scene.document.get_entity(*h).unwrap().clone())
            .collect();
        let mut command = command(&scene, &handles, alignment, SpacingMode::Vertical);
        pick_base(&mut command, &scene, handles[0]);
        let origin = command.origin().unwrap();
        apply(&mut scene, command.on_point(origin + DVec3::Y * 100.0));
        for (index, handle) in handles.iter().enumerate() {
            let item = TextItem::from_scene(&scene, *handle).unwrap();
            close(item.edge(command.settings).x, origin.x);
            let EntityType::Text(after) = item.entity else {
                panic!("TEXT")
            };
            let EntityType::Text(before) = &original[index] else {
                panic!("TEXT")
            };
            assert_eq!(after.insertion_point.y, [0.0, 13.0, 30.0][index]);
            assert_eq!(after.height, before.height);
            assert_eq!(after.rotation, before.rotation);
            assert_eq!(after.value, before.value);
            assert_eq!(after.style, before.style);
            assert_eq!(after.horizontal_alignment, before.horizontal_alignment);
            if index == 0 {
                assert_eq!(after.insertion_point, before.insertion_point);
            }
        }
    }

    #[test]
    fn textalign_left_vertical_uses_glyph_edges_and_preserves_properties() {
        vertical(Alignment::Left);
    }

    #[test]
    fn textalign_box_is_the_glyph_ink_not_the_sdf_tile() {
        let (scene, handles) = fixture();
        for handle in handles {
            let item = TextItem::from_scene(&scene, handle).unwrap();
            let EntityType::Text(text) = &item.entity else {
                panic!("TEXT")
            };
            let p = crate::entities::text::text_run_placement_at_scale(text, &scene.document, 1.0);
            let ink = crate::entities::text_support::text_local_bounds(
                &p.font,
                &text.value,
                p.height,
                p.width_factor,
                p.oblique_angle,
            )
            .unwrap();
            for (got, want) in [
                (item.min.x, p.origin[0] + ink.ink_min[0] as f64),
                (item.max.x, p.origin[0] + ink.ink_max[0] as f64),
                (item.min.y, p.origin[1] + ink.ink_min[1] as f64),
                (item.max.y, p.origin[1] + ink.ink_max[1] as f64),
            ] {
                assert!((got - want).abs() < 1e-4, "{got} != {want}");
            }
        }
    }

    #[test]
    fn textalign_prompts_and_options_resolve_to_text() {
        let (scene, handles) = fixture();
        let mut command = command(&scene, &handles, Alignment::Left, SpacingMode::Vertical);
        for step in [
            Step::Select,
            Step::Base,
            Step::Point,
            Step::Second,
            Step::Alignment,
            Step::Options,
            Step::Spacing,
        ] {
            command.step = step;
            let labels = command.options().into_iter().map(|option| option.label);
            for text in std::iter::once(command.prompt()).chain(labels) {
                assert!(!text.contains("No localization"), "{text}");
            }
        }
        for mode in [
            SpacingMode::Distribute,
            SpacingMode::Spacing,
            SpacingMode::Vertical,
            SpacingMode::Horizontal,
        ] {
            assert!(!mode.label().contains("No localization"));
        }
    }

    #[test]
    fn textalign_vertical_rejects_a_near_horizontal_second_point() {
        let (scene, handles) = fixture();
        let mut command = command(&scene, &handles, Alignment::Left, SpacingMode::Vertical);
        pick_base(&mut command, &scene, handles[0]);
        let origin = command.origin().unwrap();
        let result = command.on_point(origin + DVec3::new(50.0, 2e-8, 0.0));
        assert!(matches!(result, CmdResult::NeedPoint));
    }

    #[test]
    fn textalign_right_vertical_matches_box_max_x() {
        vertical(Alignment::Right);
    }

    #[test]
    fn textalign_middle_center_translates_both_text_points() {
        let (mut scene, mut handles) = fixture();
        let EntityType::Text(mut middle) = text("Middle", 20.0, 30.0) else {
            unreachable!()
        };
        middle.horizontal_alignment = TextHorizontalAlignment::Center;
        middle.vertical_alignment = TextVerticalAlignment::Middle;
        middle.alignment_point = Some(Vector3::new(30.0, 40.0, 0.0));
        let handle = scene.add_entity(EntityType::Text(middle.clone()));
        handles.push(handle);
        let mut command = command(&scene, &handles, Alignment::Left, SpacingMode::Vertical);
        pick_base(&mut command, &scene, handles[0]);
        apply(
            &mut scene,
            command.on_point(command.origin().unwrap() + DVec3::Y * 100.0),
        );
        let EntityType::Text(after) = scene.document.get_entity(handle).unwrap() else {
            panic!("TEXT")
        };
        let delta = after.insertion_point - middle.insertion_point;
        assert!(delta.x.abs() > 1.0);
        let alignment_delta = after.alignment_point.unwrap() - middle.alignment_point.unwrap();
        close(alignment_delta.x, delta.x);
        close(alignment_delta.y, delta.y);
        close(alignment_delta.z, delta.z);
        assert_eq!(after.vertical_alignment, middle.vertical_alignment);
        assert_eq!(after.horizontal_alignment, middle.horizontal_alignment);
    }

    #[test]
    fn textalign_mtext_in_selection_moves() {
        let (mut scene, mut handles) = fixture();
        let mut mtext = MText::new();
        mtext.value = "Multiline\\Ptext".into();
        mtext.height = 2.5;
        mtext.insertion_point = Vector3::new(40.0, 15.0, 0.0);
        let handle = scene.add_entity(EntityType::MText(Box::new(mtext.clone())));
        handles.push(handle);
        let mut command = command(&scene, &handles, Alignment::Left, SpacingMode::Vertical);
        pick_base(&mut command, &scene, handles[0]);
        let origin = command.origin().unwrap();
        apply(&mut scene, command.on_point(origin + DVec3::Y * 100.0));
        let after = TextItem::from_scene(&scene, handle).unwrap();
        close(after.min.x, origin.x);
        let EntityType::MText(after) = after.entity else {
            panic!("MTEXT")
        };
        assert_ne!(after.insertion_point.x, mtext.insertion_point.x);
        assert_eq!(after.insertion_point.y, mtext.insertion_point.y);
        assert_eq!(after.value, mtext.value);
        assert_eq!(after.height, mtext.height);
    }

    #[test]
    fn textalign_rotated_text_and_attdef_align_world_box_centers() {
        let (mut scene, mut handles) = fixture();
        let EntityType::Text(mut rotated) = text("Rotated", 25.0, 50.0) else {
            unreachable!()
        };
        rotated.rotation = 1.0;
        let rotated_handle = scene.add_entity(EntityType::Text(rotated));
        let attdef = AttributeDefinition {
            tag: "TAG".into(),
            height: 2.5,
            insertion_point: Vector3::new(60.0, 20.0, 0.0),
            alignment_point: Vector3::new(60.0, 20.0, 0.0),
            ..Default::default()
        };
        let attribute_handle = scene.add_entity(EntityType::AttributeDefinition(attdef));
        handles.extend([rotated_handle, attribute_handle]);
        let mut command = command(&scene, &handles, Alignment::Center, SpacingMode::Vertical);
        pick_base(&mut command, &scene, handles[0]);
        let origin = command.origin().unwrap();
        apply(&mut scene, command.on_point(origin + DVec3::Y * 100.0));
        for handle in [rotated_handle, attribute_handle] {
            let item = TextItem::from_scene(&scene, handle).unwrap();
            close((item.min.x + item.max.x) * 0.5, origin.x);
        }
        let EntityType::Text(after) = scene.document.get_entity(rotated_handle).unwrap() else {
            panic!("TEXT")
        };
        assert_eq!(after.rotation, 1.0);
        assert_eq!(after.insertion_point.y, 50.0);
    }

    #[test]
    fn textalign_distribute_sorts_three_texts_and_spaces_evenly() {
        let (mut scene, handles) = fixture();
        let mut reversed = handles.clone();
        reversed.reverse();
        let mut command = command(&scene, &reversed, Alignment::Left, SpacingMode::Distribute);
        command.on_text_input("Point");
        command.on_point(DVec3::new(50.0, 0.0, 0.0));
        apply(&mut scene, command.on_point(DVec3::new(50.0, 40.0, 0.0)));
        for (index, handle) in handles.iter().enumerate() {
            let item = TextItem::from_scene(&scene, *handle).unwrap();
            close(item.min.x, 50.0);
            close(item.min.y, index as f64 * 20.0);
        }
    }

    #[test]
    fn textalign_horizontal_oblique_and_set_spacing_translate() {
        for (mode, direction) in [
            (SpacingMode::Horizontal, DVec3::X),
            (SpacingMode::Horizontal, DVec3::new(1.0, 1.0, 0.0)),
            (SpacingMode::Vertical, DVec3::new(1.0, 1.0, 0.0)),
            (SpacingMode::Spacing, DVec3::new(1.0, 1.0, 0.0)),
        ] {
            let (mut scene, handles) = fixture();
            let mut command = command(&scene, &handles, Alignment::Left, mode);
            command.on_text_input("Point");
            command.on_point(DVec3::new(50.0, 50.0, 0.0));
            apply(
                &mut scene,
                command.on_point(DVec3::new(50.0, 50.0, 0.0) + direction * 100.0),
            );
            for (index, handle) in handles.iter().enumerate() {
                let EntityType::Text(after) = scene.document.get_entity(*handle).unwrap() else {
                    panic!("TEXT")
                };
                if mode == SpacingMode::Horizontal {
                    close(after.insertion_point.x, [0.0, 0.0, 20.0][index]);
                } else if mode == SpacingMode::Vertical {
                    close(after.insertion_point.y, [0.0, 13.0, 30.0][index]);
                }
            }
            let mut edges: Vec<_> = handles
                .iter()
                .map(|h| {
                    TextItem::from_scene(&scene, *h)
                        .unwrap()
                        .edge(command.settings)
                })
                .collect();
            edges.sort_by(|a, b| a.x.total_cmp(&b.x));
            for edge in &edges {
                if direction == DVec3::X {
                    close(edge.y, 50.0);
                } else {
                    close(edge.x, edge.y);
                }
            }
            if mode == SpacingMode::Spacing {
                for pair in edges.windows(2) {
                    close(pair[0].distance(pair[1]), 7.0);
                }
            }
        }
    }
}
