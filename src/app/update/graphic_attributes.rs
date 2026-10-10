//! Graphic Attributes palette: applies line attributes and associative fills
//! to the selection, or to the creation defaults when nothing is selected.

use crate::app::{Message, OpenCADStudio};
use crate::scene::model::hatch_model::{GradientKind, HatchPattern};
use crate::ui::window::gradient_editor::{GradientColorMode, GradientEditorState, GradientMsg};
use crate::ui::window::graphic_attributes::{
    self as palette, GraphicAttribute, GraphicAttributesMsg, HatchEditorState, PendingFillClose,
};
use codec::types::{Color as AcadColor, LineWeight, Transparency};
use codec::{EntityType, Handle};
use iced::Task;
use rustc_hash::{FxHashMap, FxHashSet};

/// Which objects of the selection a palette control edits.
#[derive(Clone, Copy)]
enum Part {
    /// Non-HATCH objects and the boundaries of selected associative fills.
    Line,
    /// Selected HATCH entities and the fills associated with selected objects.
    Fill,
    Both,
}

/// The first line pattern of the catalogue, used when a fill becomes a hatch.
fn default_hatch_pattern() -> Option<&'static crate::scene::model::hatch_patterns::PatternEntry> {
    crate::scene::model::hatch_patterns::catalog()
        .iter()
        .find(|entry| matches!(entry.gpu, HatchPattern::Pattern(_)))
}

/// Pattern name and render pattern of a new fill; `None` for no fill.
fn fill_pattern(value: GraphicAttribute) -> Option<(String, HatchPattern)> {
    match value {
        GraphicAttribute::Solid => Some(("SOLID".into(), HatchPattern::Solid)),
        GraphicAttribute::Hatch => {
            default_hatch_pattern().map(|entry| (entry.name.clone(), entry.gpu.clone()))
        }
        GraphicAttribute::Gradient => Some((
            "LINEAR".into(),
            HatchPattern::Gradient {
                angle_deg: 0.0,
                color2: [0.18, 0.18, 0.18, 0.0],
                kind: GradientKind::Linear,
                invert: false,
                shift: 0.0,
            },
        )),
        GraphicAttribute::None | GraphicAttribute::Varies => None,
    }
}

/// Convert a selected HATCH to another fill kind in place.
fn set_hatch_graphic_attribute(
    hatch: &mut codec::entities::Hatch,
    value: GraphicAttribute,
    visible_color: [f32; 4],
) {
    match value {
        GraphicAttribute::Solid => {
            hatch.pattern = codec::entities::HatchPattern::new("SOLID");
            hatch.pattern_type = codec::entities::HatchPatternType::Predefined;
            hatch.is_solid = true;
            hatch.gradient_color.enabled = false;
        }
        GraphicAttribute::Hatch => {
            let Some(entry) = default_hatch_pattern() else {
                return;
            };
            let mut pattern = crate::scene::model::hatch_patterns::build_dxf_pattern(entry);
            crate::entities::hatch::scale_pattern_geometry(
                &mut pattern,
                hatch.pattern_scale.max(1.0e-6),
            );
            crate::entities::hatch::rotate_pattern_geometry(&mut pattern, hatch.pattern_angle);
            let origin = hatch.pattern_origin();
            crate::entities::hatch::translate_pattern_geometry(&mut pattern, origin.x, origin.y);
            hatch.pattern = pattern;
            hatch.pattern_type = codec::entities::HatchPatternType::Predefined;
            hatch.is_solid = false;
            hatch.gradient_color.enabled = false;
        }
        GraphicAttribute::Gradient => {
            let channel = |value: f32| (value * 255.0).round().clamp(0.0, 255.0) as u8;
            hatch.is_solid = true;
            hatch.gradient_color.enabled = true;
            hatch.gradient_color.name = "LINEAR".into();
            hatch.gradient_color.angle = 0.0;
            hatch.pattern_angle = 0.0;
            hatch.gradient_color.shift = 0.0;
            hatch.gradient_color.is_single_color = false;
            hatch.gradient_color.colors = vec![
                codec::entities::hatch::GradientColorEntry {
                    value: 0.0,
                    color: AcadColor::Rgb {
                        r: channel(visible_color[0]),
                        g: channel(visible_color[1]),
                        b: channel(visible_color[2]),
                    },
                },
                codec::entities::hatch::GradientColorEntry {
                    value: 1.0,
                    color: AcadColor::Rgb {
                        r: 46,
                        g: 46,
                        b: 46,
                    },
                },
            ];
        }
        GraphicAttribute::None | GraphicAttribute::Varies => {}
    }
}

fn is_open_fill_boundary(entity: &EntityType) -> bool {
    match entity {
        EntityType::LwPolyline(polyline) => !polyline.is_closed && polyline.vertices.len() >= 3,
        EntityType::Polyline(polyline) => {
            !polyline.flags.is_closed() && polyline.vertices.len() >= 3
        }
        EntityType::Polyline2D(polyline) => {
            !polyline.is_closed() && polyline.vertices.len() >= 3
        }
        EntityType::Polyline3D(polyline) => {
            !polyline.flags.closed && polyline.vertices.len() >= 3
        }
        _ => false,
    }
}

fn close_fill_boundary(entity: &mut EntityType) -> bool {
    match entity {
        EntityType::LwPolyline(polyline) if !polyline.is_closed => {
            polyline.is_closed = true;
            true
        }
        EntityType::Polyline(polyline) if !polyline.flags.is_closed() => {
            polyline.flags.set_closed(true);
            true
        }
        EntityType::Polyline2D(polyline) if !polyline.is_closed() => {
            polyline.flags.set_closed(true);
            true
        }
        EntityType::Polyline3D(polyline) if !polyline.flags.closed => {
            polyline.flags.closed = true;
            true
        }
        _ => false,
    }
}

/// Line attributes with ByLayer / ByBlock resolved to the values they show.
struct ConcreteGraphicAttributes {
    color: AcadColor,
    linetype: String,
    lineweight: LineWeight,
    transparency: Transparency,
}

fn concrete_graphic_attributes(
    color: AcadColor,
    linetype: &str,
    lineweight: LineWeight,
    transparency: Transparency,
    layer: Option<&codec::tables::Layer>,
) -> ConcreteGraphicAttributes {
    let layer_color = layer.map_or(AcadColor::Index(7), |layer| match layer.color {
        AcadColor::ByLayer | AcadColor::ByBlock => AcadColor::Index(7),
        color => color,
    });
    let layer_linetype = layer
        .map(|layer| layer.line_type.as_str())
        .filter(|value| {
            !value.is_empty()
                && !value.eq_ignore_ascii_case("ByLayer")
                && !value.eq_ignore_ascii_case("ByBlock")
        })
        .unwrap_or("Continuous");
    let layer_lineweight = layer.map_or(LineWeight::Default, |layer| match layer.line_weight {
        LineWeight::ByLayer | LineWeight::ByBlock => LineWeight::Default,
        value => value,
    });
    let layer_transparency = layer
        .map(|layer| layer.transparency)
        .filter(|value| !value.is_by_layer() && !value.is_by_block())
        .unwrap_or_else(|| Transparency::from_percent(0.0));

    ConcreteGraphicAttributes {
        color: match color {
            AcadColor::ByLayer => layer_color,
            AcadColor::ByBlock => AcadColor::Index(7),
            value => value,
        },
        linetype: if linetype.is_empty() || linetype.eq_ignore_ascii_case("ByLayer") {
            layer_linetype.to_owned()
        } else if linetype.eq_ignore_ascii_case("ByBlock") {
            "Continuous".to_owned()
        } else {
            linetype.to_owned()
        },
        lineweight: match lineweight {
            LineWeight::ByLayer => layer_lineweight,
            LineWeight::ByBlock => LineWeight::Default,
            value => value,
        },
        transparency: if transparency.is_by_layer() {
            layer_transparency
        } else if transparency.is_by_block() {
            Transparency::from_percent(0.0)
        } else {
            transparency
        },
    }
}

impl OpenCADStudio {
    pub(super) fn on_graphic_attributes(&mut self, message: GraphicAttributesMsg) -> Task<Message> {
        use GraphicAttributesMsg as M;
        if !matches!(
            message,
            M::ToggleMenu(_)
                | M::HatchPatternSearch(_)
                | M::HatchPatternFocus(_)
                | M::LineLinetypeScaleInput(_)
                | M::Gradient(_)
        ) {
            self.graphic_attributes.open_menu = None;
        }
        match message {
            M::ToggleMenu(menu) => {
                let state = &mut self.graphic_attributes;
                state.open_menu = (state.open_menu != Some(menu)).then_some(menu);
                Task::none()
            }
            M::CloseMenu => Task::none(),
            M::SetAllByLayer => self.on_graphic_attributes_set_all(true),
            M::SetAllByBlock => self.on_graphic_attributes_set_all(false),
            M::RemoveReferences => self.on_graphic_attributes_remove_references(),
            M::ToggleFillOnCurrentLayer => {
                self.graphic_attributes.fill_on_current_layer ^= true;
                self.save_config();
                Task::none()
            }
            M::CreateLayer => {
                use crate::command::CadCommand;
                let command = crate::command::FreeTextValuePromptCommand::new(
                    "GRAPHICATTRIBUTELAYER",
                    "Enter a name for the new layer:",
                );
                self.command_line.push_info(&command.prompt());
                self.tabs[self.active_tab].active_cmd = Some(Box::new(command));
                self.focus_cmd_input()
            }
            M::Fill(value) => self.on_graphic_attribute(value),
            M::LineColor(color) => self.on_line_color(color),
            M::LineLinetype(linetype) => self.on_line_linetype(linetype),
            M::LineLineweight(lineweight) => self.on_line_lineweight(lineweight),
            M::LineLinetypeScale(scale) => self.on_line_linetype_scale(scale),
            M::LineLinetypeScaleInput(value) => {
                self.graphic_attributes.linetype_scale_input = Some(value);
                Task::none()
            }
            M::LineLinetypeScaleSubmit => {
                match self.graphic_attributes.linetype_scale_input.take() {
                    Some(typed) => {
                        let value = crate::app::expr_eval::eval_number(&typed.replace(',', "."));
                        value.map_or_else(Task::none, |scale| self.on_line_linetype_scale(scale))
                    }
                    None => Task::none(),
                }
            }
            M::LineTransparency(transparency) => self.on_line_transparency(transparency),
            M::FillColor(color) => self.on_solid_fill_color(color),
            M::FillTransparency(transparency) => self.on_fill_transparency(transparency),
            M::HatchEditorOpen => self.open_hatch_editor(),
            M::HatchEditorClose => {
                self.graphic_attributes.hatch_editor = None;
                Task::none()
            }
            M::HatchPatternSearch(search) => {
                if let Some(editor) = &mut self.graphic_attributes.hatch_editor {
                    editor.search = search;
                    editor.focus = 0;
                }
                Task::none()
            }
            M::HatchPatternFocus(index) => {
                if let Some(editor) = &mut self.graphic_attributes.hatch_editor {
                    if index < crate::ui::properties::filtered_hatch_patterns(&editor.search).len()
                    {
                        editor.focus = index;
                    }
                }
                Task::none()
            }
            M::HatchPattern(name) => self.apply_graphic_hatch_pattern(name),
            M::HatchPatternConfirm => {
                let name = self.graphic_attributes.hatch_editor.as_ref().and_then(|editor| {
                    crate::ui::properties::filtered_hatch_patterns(&editor.search)
                        .get(editor.focus)
                        .map(|entry| entry.name.clone())
                });
                match name {
                    Some(name) => self.apply_graphic_hatch_pattern(name),
                    None => Task::none(),
                }
            }
            M::Gradient(message) => self.on_gradient_editor(message),
        }
    }

    /// Prompt answers belong to the fill question only while no command runs.
    pub(super) fn awaiting_fill_close_answer(&self) -> bool {
        self.graphic_attributes.pending_fill_close.is_some()
            && self.tabs[self.active_tab].active_cmd.is_none()
    }

    /// Answer the "close open boundaries first?" prompt; Enter means Yes.
    pub(super) fn on_graphic_fill_close_input(&mut self, input: &str) -> Task<Message> {
        match input.trim().to_ascii_uppercase().as_str() {
            "" | "Y" | "YES" => self.on_graphic_fill_close_response(true),
            "N" | "NO" => self.on_graphic_fill_close_response(false),
            _ => {
                self.command_line.push_error(crate::t!("Enter Yes or No.").as_ref());
                Task::none()
            }
        }
    }

    fn graphic_handles(&self, i: usize, part: Part) -> Vec<Handle> {
        let selected = self.tabs[i].scene.selected_handles_in_order();
        let document = &self.tabs[i].scene.document;
        match part {
            Part::Line => palette::line_handles(document, &selected),
            Part::Fill => palette::fill_handles(document, &selected),
            Part::Both => {
                let mut seen = FxHashSet::default();
                palette::line_handles(document, &selected)
                    .into_iter()
                    .chain(palette::fill_handles(document, &selected))
                    .filter(|handle| seen.insert(*handle))
                    .collect()
            }
        }
    }

    fn unlocked_graphic_handles(&self, i: usize, part: Part) -> Vec<Handle> {
        let mut handles = self.graphic_handles(i, part);
        handles.retain(|handle| !self.tabs[i].scene.is_layer_locked(*handle));
        handles
    }

    /// Edit the unlocked `part` of the selection as one undo step.
    fn edit_graphic_part(&mut self, part: Part, label: &str, mut edit: impl FnMut(&mut EntityType)) {
        let i = self.active_tab;
        let handles = self.unlocked_graphic_handles(i, part);
        self.apply_property_op(i, label, &handles, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                edit(entity);
            }
        });
    }

    fn has_selection(&self) -> bool {
        !self.tabs[self.active_tab].scene.selected_handles_in_order().is_empty()
    }

    fn current_graphic_attributes(&self, i: usize) -> ConcreteGraphicAttributes {
        let document = &self.tabs[i].scene.document;
        let handles = self.graphic_handles(i, Part::Both);
        let draw_depth = self.tabs[i].scene.draw_depth_map();
        if let Some(entity) =
            palette::top_handle(&handles, &draw_depth).and_then(|handle| document.get_entity(handle))
        {
            let common = entity.common();
            return concrete_graphic_attributes(
                common.color,
                &common.linetype,
                common.line_weight,
                common.transparency,
                document.layers.get(&common.layer),
            );
        }

        let header = &document.header;
        let layer_name = if header.current_layer_name.is_empty() {
            self.tabs[i].active_layer.as_str()
        } else {
            header.current_layer_name.as_str()
        };
        concrete_graphic_attributes(
            header.current_entity_color,
            &header.current_linetype_name,
            LineWeight::from_value(header.current_line_weight),
            document.current_entity_transparency(),
            document.layers.get(layer_name),
        )
    }

    /// Store line attributes as the creation defaults (CECOLOR, CELTYPE,
    /// CELWEIGHT, CETRANSPARENCY).
    fn set_current_graphic_attributes(
        &mut self,
        color: AcadColor,
        linetype: String,
        lineweight: LineWeight,
        transparency: Transparency,
    ) {
        let i = self.active_tab;
        let linetype_handle = self.tabs[i]
            .scene
            .document
            .line_types
            .iter()
            .find(|line_type| line_type.name.eq_ignore_ascii_case(&linetype))
            .map_or(Handle::NULL, |line_type| line_type.handle);
        let header = &mut self.tabs[i].scene.document.header;
        header.current_entity_color = color;
        header.current_linetype_name = linetype;
        header.current_linetype_handle = linetype_handle;
        header.current_line_weight = lineweight.value();
        self.tabs[i]
            .scene
            .document
            .set_current_entity_transparency(transparency);
        self.tabs[i].dirty = true;
        self.sync_ribbon_from_selection();
        self.refresh_properties();
    }

    pub(super) fn on_graphic_attributes_set_all(&mut self, by_layer: bool) -> Task<Message> {
        let (color, linetype, lineweight, transparency) = if by_layer {
            (AcadColor::ByLayer, "ByLayer", LineWeight::ByLayer, Transparency::BY_LAYER)
        } else {
            (AcadColor::ByBlock, "ByBlock", LineWeight::ByBlock, Transparency::BY_BLOCK)
        };
        if !self.has_selection() {
            self.set_current_graphic_attributes(color, linetype.to_owned(), lineweight, transparency);
            return Task::none();
        }
        self.edit_graphic_part(Part::Both, &crate::t!("Graphic Attributes"), |entity| {
            crate::scene::view::dispatch::apply_color(entity, color);
            crate::scene::view::dispatch::apply_common_prop(entity, "linetype", linetype);
            crate::scene::view::dispatch::apply_line_weight(entity, lineweight);
            entity.common_mut().transparency = transparency;
        });
        Task::none()
    }

    pub(super) fn on_graphic_attributes_remove_references(&mut self) -> Task<Message> {
        let i = self.active_tab;
        if !self.has_selection() {
            let values = self.current_graphic_attributes(i);
            self.set_current_graphic_attributes(
                values.color,
                values.linetype,
                values.lineweight,
                values.transparency,
            );
            return Task::none();
        }

        let document = &self.tabs[i].scene.document;
        let values: FxHashMap<_, _> = self
            .unlocked_graphic_handles(i, Part::Both)
            .into_iter()
            .filter_map(|handle| {
                let common = document.get_entity(handle)?.common();
                Some((
                    handle,
                    concrete_graphic_attributes(
                        common.color,
                        &common.linetype,
                        common.line_weight,
                        common.transparency,
                        document.layers.get(&common.layer),
                    ),
                ))
            })
            .collect();
        let handles: Vec<_> = values.keys().copied().collect();
        self.apply_property_op(i, crate::t!("Graphic Attributes"), &handles, |app, handle| {
            let (Some(values), Some(entity)) = (
                values.get(&handle),
                app.tabs[i].scene.document.get_entity_mut(handle),
            ) else {
                return;
            };
            crate::scene::view::dispatch::apply_color(entity, values.color);
            crate::scene::view::dispatch::apply_common_prop(entity, "linetype", &values.linetype);
            crate::scene::view::dispatch::apply_line_weight(entity, values.lineweight);
            entity.common_mut().transparency = values.transparency;
        });
        Task::none()
    }

    pub(crate) fn create_graphic_attributes_layer(&mut self, raw_name: &str) -> Task<Message> {
        let i = self.active_tab;
        let name = raw_name.trim();
        if !crate::scene::valid_block_name(name) {
            self.command_line
                .push_error(crate::t!("Graphic Attributes: enter a valid layer name without <>/\\\":;?*|,=`.").as_ref());
            return Task::none();
        }
        if self.tabs[i]
            .scene
            .document
            .layers
            .iter()
            .any(|layer| layer.name.eq_ignore_ascii_case(name))
        {
            self.command_line.push_error(
                crate::tf!("Graphic Attributes: layer \"{name}\" already exists.").as_ref(),
            );
            return Task::none();
        }

        let values = self.current_graphic_attributes(i);
        let layer_name = name.to_owned();
        let undo = self.begin_layer_undo(
            i,
            crate::t!("Create Layer with active settings"),
            std::slice::from_ref(&layer_name),
        );
        let mut layer = codec::tables::Layer::new(name);
        layer.handle = self.tabs[i].scene.document.allocate_handle();
        layer.color = values.color;
        layer.line_type = values.linetype;
        layer.line_weight = values.lineweight;
        layer.transparency = values.transparency;
        if self.tabs[i].scene.document.layers.add(layer).is_err() {
            self.command_line
                .push_error(crate::t!("Graphic Attributes: the layer could not be created.").as_ref());
            return Task::none();
        }
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        self.refresh_layer_panel();
        self.command_line.push_output(
            crate::tf!("Created layer \"{name}\" with the active Graphic Attributes settings.")
                .as_ref(),
        );
        Task::none()
    }

    fn open_hatch_editor(&mut self) -> Task<Message> {
        self.graphic_attributes.gradient_editor = None;
        let i = self.active_tab;
        let document = &self.tabs[i].scene.document;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        let current = palette::fill_handles(document, &selected)
            .into_iter()
            .find_map(|handle| match document.get_entity(handle)? {
                EntityType::Hatch(hatch) => Some(hatch.pattern.name.as_str()),
                _ => None,
            });
        let focus = current
            .and_then(|current| {
                crate::ui::properties::filtered_hatch_patterns("")
                    .iter()
                    .position(|entry| entry.name.eq_ignore_ascii_case(current))
            })
            .unwrap_or(0);
        self.graphic_attributes.hatch_editor = Some(HatchEditorState {
            handles: selected,
            search: String::new(),
            focus,
        });
        iced::widget::operation::focus(iced::widget::Id::new("graphic-hatch-pattern-search"))
    }

    fn apply_graphic_hatch_pattern(&mut self, name: String) -> Task<Message> {
        self.graphic_attributes.hatch_editor = None;
        let i = self.active_tab;
        let handles = self.graphic_handles(i, Part::Fill);
        self.apply_hatch_pattern(i, &handles, name)
    }

    fn on_gradient_editor(&mut self, message: GradientMsg) -> Task<Message> {
        match message {
            GradientMsg::Open => {
                self.graphic_attributes.hatch_editor = None;
                self.open_gradient_editor();
            }
            GradientMsg::Cancel => self.graphic_attributes.gradient_editor = None,
            GradientMsg::Apply => return self.apply_gradient_editor(),
            message => {
                if let Some(editor) = &mut self.graphic_attributes.gradient_editor {
                    editor.update(message);
                }
            }
        }
        Task::none()
    }

    pub(super) fn open_gradient_editor(&mut self) {
        let i = self.active_tab;
        let document = &self.tabs[i].scene.document;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        let gradient = palette::fill_handles(document, &selected)
            .into_iter()
            .find_map(|handle| match document.get_entity(handle)? {
                EntityType::Hatch(hatch) if hatch.gradient_color.enabled => Some(hatch),
                _ => None,
            });
        if let Some(hatch) = gradient {
            self.graphic_attributes.gradient_editor =
                Some(GradientEditorState::from_hatch(selected, hatch));
        }
    }

    pub(super) fn apply_gradient_editor(&mut self) -> Task<Message> {
        let Some(state) = self.graphic_attributes.gradient_editor.take() else {
            return Task::none();
        };
        let i = self.active_tab;
        let document = &self.tabs[i].scene.document;
        let targets: Vec<_> = palette::fill_handles(document, &state.handles)
            .into_iter()
            .filter(|handle| {
                matches!(document.get_entity(*handle),
                    Some(EntityType::Hatch(hatch)) if hatch.gradient_color.enabled)
            })
            .collect();
        if targets.is_empty() {
            return Task::none();
        }
        if targets
            .iter()
            .any(|handle| self.tabs[i].scene.is_layer_locked(*handle))
        {
            self.command_line
                .push_error(crate::t!("Gradient Fill: the fill is on a locked layer.").as_ref());
            self.graphic_attributes.gradient_editor = Some(state);
            return Task::none();
        }
        let angle = state.angle_radians();
        let pending = self.begin_undo(i, crate::t!("Edit gradient"), targets.len(), true);
        for handle in &targets {
            let Some(mut entity) = self.tabs[i].scene.document.get_entity(*handle).cloned() else {
                continue;
            };
            if let EntityType::Hatch(hatch) = &mut entity {
                let gradient = &mut hatch.gradient_color;
                gradient.name = state.kind.dxf_name(state.inverted).into();
                gradient.angle = angle;
                gradient.shift = if state.centered { 0.0 } else { 1.0 };
                gradient.is_single_color = state.color_mode == GradientColorMode::One;
                gradient.color_tint = state.shade_tint as f64;
                gradient.colors = vec![
                    codec::entities::hatch::GradientColorEntry {
                        value: 0.0,
                        color: state.color_1,
                    },
                    codec::entities::hatch::GradientColorEntry {
                        value: 1.0,
                        color: state.color_2,
                    },
                ];
                hatch.pattern_angle = angle;
            }
            self.tabs[i].scene.update_entity(entity);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        Task::none()
    }

    /// Give every selected object the chosen fill. Open polylines first ask
    /// whether they should be closed.
    pub(super) fn on_graphic_attribute(&mut self, value: GraphicAttribute) -> Task<Message> {
        self.graphic_attributes.close_popups();
        self.graphic_attributes.pending_fill_close = None;
        if value == GraphicAttribute::Varies {
            return Task::none();
        }
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        if selected.is_empty() {
            self.command_line
                .push_info(crate::t!("Select one or more closed objects first.").as_ref());
            return Task::none();
        }
        if value != GraphicAttribute::None {
            let open_boundaries: Vec<_> = selected
                .iter()
                .copied()
                .filter(|handle| {
                    !self.tabs[i].scene.is_layer_locked(*handle)
                        && self.tabs[i]
                            .scene
                            .document
                            .get_entity(*handle)
                            .is_some_and(is_open_fill_boundary)
                })
                .collect();
            if !open_boundaries.is_empty() {
                self.graphic_attributes.pending_fill_close = Some(PendingFillClose {
                    fill: value,
                    open_boundaries,
                    selection: selected,
                });
                return Task::none();
            }
        }
        self.apply_graphic_fill(i, value, &selected, &[])
    }

    pub(super) fn on_graphic_fill_close_response(&mut self, close: bool) -> Task<Message> {
        let Some(pending) = self.graphic_attributes.pending_fill_close.take() else {
            return Task::none();
        };
        let i = self.active_tab;
        let to_close: Vec<_> = if close {
            pending
                .open_boundaries
                .into_iter()
                .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                .collect()
        } else {
            Vec::new()
        };
        self.apply_graphic_fill(i, pending.fill, &pending.selection, &to_close)
    }

    /// Close `to_close` and replace the fills of `selected`, as one undo step.
    fn apply_graphic_fill(
        &mut self,
        i: usize,
        value: GraphicAttribute,
        selected: &[Handle],
        to_close: &[Handle],
    ) -> Task<Message> {
        let (selected, locked): (Vec<Handle>, Vec<Handle>) = selected
            .iter()
            .partition(|handle| !self.tabs[i].scene.is_layer_locked(**handle));
        if !locked.is_empty() {
            self.command_line.push_info(
                crate::tf!(
                    "Graphic Attributes: {} object(s) on a locked layer were not changed.",
                    locked.len()
                )
                .as_ref(),
            );
        }
        let selected = selected.as_slice();
        let pending = self.begin_undo(i, crate::t!("Fill change"), selected.len(), true);
        let mut changed = false;
        for handle in to_close {
            let Some(mut entity) = self.tabs[i].scene.document.get_entity(*handle).cloned() else {
                continue;
            };
            if close_fill_boundary(&mut entity) {
                self.tabs[i].scene.update_entity(entity);
                changed = true;
            }
        }
        changed |= self.replace_graphic_fills(i, value, selected);
        if changed {
            self.tabs[i].dirty = true;
            self.refresh_properties();
        }
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        Task::none()
    }

    /// Replace the fills of `selected` by one associative fill per object.
    /// Returns whether the document changed.
    fn replace_graphic_fills(
        &mut self,
        i: usize,
        value: GraphicAttribute,
        selected: &[Handle],
    ) -> bool {
        let document = &self.tabs[i].scene.document;
        let (associated, new_hatches, updated_hatches, skipped) = if value == GraphicAttribute::None
        {
            (palette::fill_handles(document, selected), Vec::new(), Vec::new(), 0)
        } else {
            let Some((name, pattern)) = fill_pattern(value) else {
                self.command_line
                    .push_error(crate::t!("Graphic Attributes: no hatch pattern is available.").as_ref());
                return false;
            };
            let working = if self.tabs[i].editing_model_space() {
                self.tabs[i].ucs_xform().working_plane()
            } else {
                crate::command::WorkingPlane::default()
            };
            let normal = working.z.normalize_or(glam::DVec3::Z);
            let storage = crate::entities::curve::ocs_plane(
                codec::types::Vector3::new(normal.x, normal.y, normal.z),
                working.origin.dot(normal),
            );
            let plane = crate::command::WorkingPlane::new(
                glam::DVec3::from_array(storage.origin),
                glam::DVec3::from_array(storage.x_axis),
                glam::DVec3::from_array(storage.y_axis),
            );
            let sources = self.tabs[i].scene.boundary_sources_on_plane(plane, 1.0e-6);
            let origin = document.hatch_origin();
            let mut new_hatches = Vec::new();
            let mut updated_hatches = Vec::new();
            let mut directly_updated = FxHashSet::default();
            let mut changed_boundaries = FxHashSet::default();
            let mut skipped = 0;
            for handle in selected {
                let Some(entity) = document.get_entity(*handle) else {
                    continue;
                };
                if let EntityType::Hatch(hatch) = entity {
                    let mut hatch = hatch.clone();
                    let visible_color =
                        crate::scene::view::render::render_style_for_common_viewport(
                            document,
                            &hatch.common,
                            None,
                        )
                        .0;
                    set_hatch_graphic_attribute(&mut hatch, value, visible_color);
                    updated_hatches.push(EntityType::Hatch(hatch));
                    directly_updated.insert(*handle);
                    continue;
                }
                // Object mode only needs this object's source. Including all
                // scene sources can associate coincident, unselected objects.
                let object_sources = sources
                    .get(handle)
                    .map(|source| (*handle, source.clone()))
                    .into_iter()
                    .collect();
                let command = crate::modules::draw::draw::hatch::HatchCommand::new(
                    Vec::new(),
                    object_sources,
                    vec![*handle],
                    None,
                    plane,
                )
                .with_origin(origin)
                .with_pattern(name.clone(), pattern.clone());
                if let crate::command::CmdResult::CommitHatch(hatch) = command.finish_selected() {
                    let layer = if self.graphic_attributes.fill_on_current_layer {
                        self.tabs[i].active_layer.clone()
                    } else {
                        entity.common().layer.clone()
                    };
                    new_hatches.push((hatch, layer));
                    changed_boundaries.insert(*handle);
                } else if !is_open_fill_boundary(entity) {
                    // Open boundaries the user chose not to close are expected
                    // to stay unfilled; anything else is unsupported.
                    skipped += 1;
                }
            }
            // Replace only fills belonging to boundaries that successfully
            // produced a new hatch. Existing fills on skipped objects remain.
            let associated: Vec<_> = document
                .entities()
                .filter_map(|entity| {
                    let EntityType::Hatch(hatch) = entity else {
                        return None;
                    };
                    (hatch.is_associative
                        && !directly_updated.contains(&entity.common().handle)
                        && hatch
                            .paths
                            .iter()
                            .flat_map(|path| &path.boundary_handles)
                            .any(|handle| changed_boundaries.contains(handle)))
                    .then_some(entity.common().handle)
                })
                .collect();
            (associated, new_hatches, updated_hatches, skipped)
        };
        if associated
            .iter()
            .chain(updated_hatches.iter().map(|entity| &entity.common().handle))
            .any(|handle| self.tabs[i].scene.is_layer_locked(*handle))
        {
            self.command_line
                .push_error(crate::t!("Graphic Attributes: an associated fill is on a locked layer.").as_ref());
            return false;
        }
        if skipped == 1 {
            self.command_line
                .push_info(crate::t!("Graphic Attributes: 1 unsupported object was not changed.").as_ref());
        } else if skipped > 1 {
            self.command_line.push_info(
                crate::tf!("Graphic Attributes: {skipped} unsupported objects were not changed.")
                    .as_ref(),
            );
        }
        if associated.is_empty() && new_hatches.is_empty() && updated_hatches.is_empty() {
            return false;
        }
        self.tabs[i].scene.erase_entities(&associated);
        for entity in updated_hatches {
            self.tabs[i].scene.update_entity(entity);
        }
        for (hatch, layer) in new_hatches {
            self.tabs[i].scene.add_hatch(hatch, Some(&layer), None);
        }
        true
    }

    pub(super) fn on_solid_fill_color(&mut self, color: AcadColor) -> Task<Message> {
        self.edit_graphic_part(Part::Fill, &crate::t!("Fill color"), |entity| {
            crate::scene::view::dispatch::apply_color(entity, color);
        });
        Task::none()
    }

    pub(super) fn on_fill_transparency(&mut self, transparency: Transparency) -> Task<Message> {
        self.edit_graphic_part(Part::Fill, &crate::t!("Fill transparency"), |entity| {
            entity.common_mut().transparency = transparency;
        });
        Task::none()
    }

    pub(super) fn on_line_color(&mut self, color: AcadColor) -> Task<Message> {
        if !self.has_selection() {
            return self.on_ribbon_color_changed(color);
        }
        self.edit_graphic_part(Part::Line, &crate::t!("Line color"), |entity| {
            crate::scene::view::dispatch::apply_color(entity, color);
        });
        Task::none()
    }

    pub(super) fn on_line_linetype(&mut self, linetype: String) -> Task<Message> {
        if !self.has_selection() {
            return self.on_ribbon_linetype_changed(linetype);
        }
        self.edit_graphic_part(Part::Line, &crate::t!("Linetype"), |entity| {
            crate::scene::view::dispatch::apply_common_prop(entity, "linetype", &linetype);
        });
        Task::none()
    }

    pub(super) fn on_line_lineweight(&mut self, lineweight: LineWeight) -> Task<Message> {
        if !self.has_selection() {
            return self.on_ribbon_lineweight_changed(lineweight);
        }
        self.edit_graphic_part(Part::Line, &crate::t!("Line weight"), |entity| {
            crate::scene::view::dispatch::apply_line_weight(entity, lineweight);
        });
        Task::none()
    }

    pub(super) fn on_line_linetype_scale(&mut self, scale: f64) -> Task<Message> {
        let Some(scale) = palette::normalized_linetype_scale(scale) else {
            return Task::none();
        };
        if !self.has_selection() {
            let i = self.active_tab;
            self.tabs[i].scene.document.header.current_entity_linetype_scale = scale;
            self.tabs[i].dirty = true;
            self.refresh_properties();
            return Task::none();
        }
        self.edit_graphic_part(Part::Line, &crate::t!("Linetype scale"), |entity| {
            entity.common_mut().linetype_scale = scale;
        });
        Task::none()
    }

    pub(super) fn on_line_transparency(&mut self, transparency: Transparency) -> Task<Message> {
        if !self.has_selection() {
            let i = self.active_tab;
            if self.tabs[i]
                .scene
                .document
                .set_current_entity_transparency(transparency)
            {
                self.tabs[i].dirty = true;
                self.refresh_properties();
            }
            return Task::none();
        }
        self.edit_graphic_part(Part::Line, &crate::t!("Line transparency"), |entity| {
            entity.common_mut().transparency = transparency;
        });
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codec::entities::{Line, LwPolyline};
    use codec::types::Vector2;

    fn ga(message: GraphicAttributesMsg) -> Message {
        Message::GraphicAttributes(message)
    }

    fn fresh() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    fn add_closed_square(app: &mut OpenCADStudio, x: f64) -> Handle {
        let mut polyline = LwPolyline::new();
        for (px, py) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            polyline.add_point(Vector2::new(px + x, py));
        }
        polyline.close();
        let i = app.active_tab;
        app.tabs[i].scene.add_entity(EntityType::LwPolyline(polyline))
    }

    fn select(app: &mut OpenCADStudio, handles: &[Handle]) {
        let i = app.active_tab;
        app.tabs[i].scene.deselect_all();
        for handle in handles {
            app.tabs[i].scene.select_entity(*handle, false);
        }
    }

    fn hatches(app: &OpenCADStudio) -> Vec<&codec::entities::Hatch> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Hatch(hatch) => Some(hatch),
                _ => None,
            })
            .collect()
    }

    fn common(app: &OpenCADStudio, handle: Handle) -> &codec::entities::EntityCommon {
        app.tabs[app.active_tab]
            .scene
            .document
            .get_entity(handle)
            .expect("entity")
            .common()
    }

    fn fill_kind(app: &OpenCADStudio, handle: Handle) -> GraphicAttribute {
        palette::current(&app.tabs[app.active_tab].scene.document, &[handle])
    }

    /// A closed square with a fill of `kind`; the fill HATCH is selected.
    fn filled_square(app: &mut OpenCADStudio, kind: GraphicAttribute) -> (Handle, Handle) {
        let boundary = add_closed_square(app, 0.0);
        select(app, &[boundary]);
        let _ = app.on_graphic_attribute(kind);
        let hatch = hatches(app)[0].common.handle;
        select(app, &[hatch]);
        (boundary, hatch)
    }

    #[test]
    fn coincident_boundaries_each_get_their_own_associative_fill() {
        let mut app = fresh();
        let boundaries = [add_closed_square(&mut app, 0.0), add_closed_square(&mut app, 0.0)];
        select(&mut app, &boundaries);

        let _ = app.on_graphic_attribute(GraphicAttribute::Solid);
        let fills = hatches(&app);
        assert_eq!(fills.len(), 2);
        assert!(fills.iter().all(|hatch| hatch.is_solid && hatch.is_associative));
        assert!(fills.iter().all(|hatch| hatch
            .paths
            .iter()
            .flat_map(|path| &path.boundary_handles)
            .count()
            == 1));

        select(&mut app, &boundaries[..1]);
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);
        assert_eq!(fill_kind(&app, boundaries[0]), GraphicAttribute::Gradient);
        assert_eq!(fill_kind(&app, boundaries[1]), GraphicAttribute::Solid);
        assert_eq!(hatches(&app).len(), 2);
    }

    #[test]
    fn unsupported_objects_are_skipped_while_the_rest_is_filled() {
        let mut app = fresh();
        let i = app.active_tab;
        let boundaries = [add_closed_square(&mut app, 0.0), add_closed_square(&mut app, 20.0)];
        select(&mut app, &boundaries);
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);

        let line = app.tabs[i].scene.add_entity(EntityType::Line(Line::new()));
        select(&mut app, &[boundaries[0], line]);
        let _ = app.on_graphic_attribute(GraphicAttribute::Solid);
        assert_eq!(fill_kind(&app, boundaries[0]), GraphicAttribute::Solid);
        assert_eq!(fill_kind(&app, boundaries[1]), GraphicAttribute::Gradient);
        assert_eq!(fill_kind(&app, line), GraphicAttribute::None);
        assert_eq!(hatches(&app).len(), 2);
    }

    #[test]
    fn editing_a_shared_gradient_is_one_undo_step() {
        let mut app = fresh();
        let i = app.active_tab;
        let boundaries = [add_closed_square(&mut app, 0.0), add_closed_square(&mut app, 20.0)];
        select(&mut app, &boundaries);
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);

        app.open_gradient_editor();
        let editor = app
            .graphic_attributes
            .gradient_editor
            .as_mut()
            .expect("gradient editor");
        assert_eq!(editor.handles.len(), 2);
        editor.kind = crate::scene::model::hatch_model::GradientKind::Curved;
        editor.inverted = true;
        editor.angle = "405°".into();
        editor.centered = false;
        let undo_before = app.tabs[i].history.undo_stack.len();
        let _ = app.apply_gradient_editor();
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);

        let gradients = hatches(&app);
        assert_eq!(gradients.len(), 2);
        assert!(gradients.iter().all(|hatch| {
            hatch.gradient_color.name == "INVCURVED"
                && (hatch.gradient_color.angle.to_degrees() - 45.0).abs() < 1.0e-9
                && (hatch.gradient_color.shift - 1.0).abs() < 1.0e-9
        }));
    }

    #[test]
    fn gradient_editor_closes_when_the_selection_changes() {
        let mut app = fresh();
        filled_square(&mut app, GraphicAttribute::Gradient);
        app.open_gradient_editor();
        assert!(app.graphic_attributes.gradient_editor.is_some());

        app.tabs[app.active_tab].scene.deselect_all();
        let _ = app.update(ga(GraphicAttributesMsg::CloseMenu));
        assert!(app.graphic_attributes.gradient_editor.is_none());
    }

    #[test]
    fn a_selected_hatch_is_converted_in_place() {
        let mut app = fresh();
        let (boundary, hatch) = filled_square(&mut app, GraphicAttribute::Gradient);
        let document = &app.tabs[app.active_tab].scene.document;
        assert_eq!(palette::line_handles(document, &[hatch]), vec![boundary]);
        assert_eq!(fill_kind(&app, hatch), GraphicAttribute::Gradient);

        for kind in [GraphicAttribute::Solid, GraphicAttribute::Hatch, GraphicAttribute::Gradient] {
            let _ = app.on_graphic_attribute(kind);
            assert_eq!(fill_kind(&app, hatch), kind);
        }
        assert_eq!(hatches(&app).len(), 1);

        let pattern = default_hatch_pattern().expect("pattern hatch").name.clone();
        let _ = app.update(ga(GraphicAttributesMsg::HatchPattern(pattern.clone())));
        assert_eq!(hatches(&app)[0].pattern.name, pattern);
    }

    #[test]
    fn line_and_fill_controls_only_edit_their_own_part() {
        let mut app = fresh();
        let i = app.active_tab;
        let (_, hatch) = filled_square(&mut app, GraphicAttribute::Solid);
        let line = app.tabs[i].scene.add_entity(EntityType::Line(Line::new()));
        select(&mut app, &[hatch, line]);

        let _ = app.on_solid_fill_color(AcadColor::Index(3));
        let _ = app.on_line_color(AcadColor::Index(4));
        assert_eq!(common(&app, hatch).color, AcadColor::Index(3));
        assert_eq!(common(&app, line).color, AcadColor::Index(4));

        let _ = app.on_line_linetype("ByBlock".to_string());
        assert_eq!(common(&app, line).linetype, "ByBlock");

        let _ = app.on_line_lineweight(LineWeight::Value(50));
        assert_eq!(common(&app, line).line_weight, LineWeight::Value(50));
        assert_ne!(common(&app, hatch).line_weight, LineWeight::Value(50));

        let _ = app.on_line_linetype_scale(2.5);
        assert_eq!(common(&app, line).linetype_scale, 2.5);
        assert_eq!(common(&app, hatch).linetype_scale, 1.0);
        let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScale(f64::NAN)));
        assert_eq!(common(&app, line).linetype_scale, 2.5);

        let _ = app.on_line_transparency(Transparency::from_percent(0.2));
        let _ = app.on_fill_transparency(Transparency::from_percent(0.4));
        assert!((common(&app, line).transparency.as_percent() - 0.2).abs() < 0.01);
        assert!((common(&app, hatch).transparency.as_percent() - 0.4).abs() < 0.01);
        let _ = app.on_fill_transparency(Transparency::BY_LAYER);
        assert!(common(&app, hatch).transparency.is_by_layer());
        assert!((common(&app, line).transparency.as_percent() - 0.2).abs() < 0.01);
    }

    #[test]
    fn choosing_another_fill_closes_the_editors() {
        let mut app = fresh();
        filled_square(&mut app, GraphicAttribute::Gradient);
        app.open_gradient_editor();
        assert!(app.graphic_attributes.gradient_editor.is_some());
        let _ = app.on_graphic_attribute(GraphicAttribute::Hatch);
        assert!(app.graphic_attributes.gradient_editor.is_none());

        let _ = app.update(ga(GraphicAttributesMsg::HatchEditorOpen));
        assert!(app.graphic_attributes.hatch_editor.is_some());
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);
        assert!(app.graphic_attributes.hatch_editor.is_none());
        assert!(app.graphic_attributes.gradient_editor.is_none());
    }

    #[test]
    fn removing_the_fill_erases_it_in_one_undo_step() {
        let mut app = fresh();
        let i = app.active_tab;
        let (_, hatch) = filled_square(&mut app, GraphicAttribute::Solid);
        let undo_before = app.tabs[i].history.undo_stack.len();
        let _ = app.on_graphic_attribute(GraphicAttribute::None);
        assert!(app.tabs[i].scene.document.get_entity(hatch).is_none());
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);
    }

    #[test]
    fn graphic_attributes_can_close_an_open_boundary_from_the_command_prompt() {
        let mut app = fresh();
        let i = app.active_tab;
        let mut polyline = LwPolyline::new();
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            polyline.add_point(Vector2::new(x, y));
        }
        let boundary = app.tabs[i]
            .scene
            .add_entity(EntityType::LwPolyline(polyline));
        app.tabs[i].scene.select_entity(boundary, false);

        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        assert!(app.graphic_attributes.pending_fill_close.is_some());
        assert!(!app.tabs[i].scene.document.entities().any(|entity| {
            matches!(entity, EntityType::Hatch(_))
        }));

        let _ = app.update(Message::CommandOptionPick("Y".to_string()));
        assert!(app.graphic_attributes.pending_fill_close.is_none());
        assert!(matches!(
            app.tabs[i].scene.document.get_entity(boundary),
            Some(EntityType::LwPolyline(polyline)) if polyline.is_closed
        ));
        assert!(app.tabs[i].scene.document.entities().any(|entity| {
            matches!(entity, EntityType::Hatch(hatch) if hatch.is_solid && hatch.is_associative)
        }));
    }

    #[test]
    fn graphic_line_controls_update_creation_defaults_without_a_selection() {
        let mut app = fresh();
        let i = app.active_tab;
        app.tabs[i].scene.deselect_all();

        let _ = app.on_line_color(codec::types::Color::Index(2));
        let _ = app.on_line_linetype("Continuous".to_string());
        let _ = app.on_line_lineweight(codec::types::LineWeight::Value(35));
        let _ = app.on_line_linetype_scale(2.5);
        let _ = app.on_line_transparency(codec::types::Transparency::from_percent(0.3));

        let header = &app.tabs[i].scene.document.header;
        assert_eq!(
            header.current_entity_color,
            codec::types::Color::Index(2)
        );
        assert_eq!(header.current_linetype_name, "Continuous");
        assert_eq!(header.current_line_weight, 35);
        assert!((header.current_entity_linetype_scale - 2.5).abs() < f64::EPSILON);
        assert!(
            (app.tabs[i]
                .scene
                .document
                .current_entity_transparency()
                .as_percent()
                - 0.3)
                .abs()
                < 0.01
        );
    }

    #[test]
    fn graphic_attributes_bulk_modes_and_remove_references_preserve_visible_values() {
        let mut app = fresh();
        let i = app.active_tab;
        {
            let layer = app.tabs[i]
                .scene
                .document
                .layers
                .get_mut("0")
                .expect("default layer");
            layer.color = codec::types::Color::Index(3);
            layer.line_type = "Continuous".to_string();
            layer.line_weight = codec::types::LineWeight::Value(50);
            layer.transparency = codec::types::Transparency::from_percent(0.25);
        }
        let handle = app.tabs[i].scene.add_entity(EntityType::Line(Line::new()));
        app.tabs[i].scene.select_entity(handle, false);

        let _ = app.on_graphic_attributes_set_all(false);
        let common = app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .expect("line")
            .common();
        assert_eq!(common.color, codec::types::Color::ByBlock);
        assert_eq!(common.linetype, "ByBlock");
        assert_eq!(common.line_weight, codec::types::LineWeight::ByBlock);
        assert!(common.transparency.is_by_block());

        let _ = app.on_graphic_attributes_set_all(true);
        let _ = app.on_graphic_attributes_remove_references();
        let common = app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .expect("line")
            .common();
        assert_eq!(common.color, codec::types::Color::Index(3));
        assert_eq!(common.linetype, "Continuous");
        assert_eq!(common.line_weight, codec::types::LineWeight::Value(50));
        assert!((common.transparency.as_percent() - 0.25).abs() < 0.01);
    }

    #[test]
    fn graphic_attributes_create_layer_prompt_keeps_spaces_and_active_settings() {
        let mut app = fresh();
        let i = app.active_tab;
        app.tabs[i].scene.deselect_all();
        let _ = app.on_line_color(codec::types::Color::Index(2));
        let _ = app.on_line_linetype("Continuous".to_string());
        let _ = app.on_line_lineweight(codec::types::LineWeight::Value(35));
        let _ = app.on_line_transparency(codec::types::Transparency::from_percent(0.3));

        let _ = app.create_graphic_attributes_layer("Invalid\nLayer");
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("Invalid\nLayer")
            .is_none());

        let _ = app.update(ga(GraphicAttributesMsg::CreateLayer));
        assert_eq!(
            app.tabs[i]
                .active_cmd
                .as_ref()
                .map(|command| command.name()),
            Some("GRAPHICATTRIBUTELAYER")
        );
        let _ = app.update(Message::CommandInput("Graphic Settings".to_string()));
        let _ = app.update(Message::CommandFinalize);

        let layer = app.tabs[i]
            .scene
            .document
            .layers
            .get("Graphic Settings")
            .expect("created layer");
        assert_eq!(layer.color, codec::types::Color::Index(2));
        assert_eq!(layer.line_type, "Continuous");
        assert_eq!(layer.line_weight, codec::types::LineWeight::Value(35));
        assert!((layer.transparency.as_percent() - 0.3).abs() < 0.01);
    }

    fn open_square(app: &mut OpenCADStudio) -> Handle {
        let i = app.active_tab;
        let mut polyline = LwPolyline::new();
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
            polyline.add_point(Vector2::new(x, y));
        }
        let handle = app.tabs[i]
            .scene
            .add_entity(EntityType::LwPolyline(polyline));
        app.tabs[i].scene.select_entity(handle, false);
        handle
    }

    fn has_hatch(app: &OpenCADStudio) -> bool {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .any(|entity| matches!(entity, EntityType::Hatch(_)))
    }

    fn is_closed(app: &OpenCADStudio, handle: Handle) -> bool {
        matches!(
            app.tabs[app.active_tab].scene.document.get_entity(handle),
            Some(EntityType::LwPolyline(polyline)) if polyline.is_closed
        )
    }

    fn type_and_enter(app: &mut OpenCADStudio, input: &str) {
        let _ = app.update(Message::CommandInput(input.to_string()));
        let _ = app.update(Message::CommandSubmit);
    }

    #[test]
    fn closing_an_open_boundary_and_filling_it_is_one_undo_step() {
        let mut app = fresh();
        let i = app.active_tab;
        let boundary = open_square(&mut app);
        let undo_before = app.tabs[i].history.undo_stack.len();

        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        let _ = app.update(Message::CommandOptionPick("Y".to_string()));
        assert!(is_closed(&app, boundary));
        assert!(has_hatch(&app));
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);

        let _ = app.update(Message::Undo);
        assert!(!is_closed(&app, boundary));
        assert!(!has_hatch(&app));
    }

    #[test]
    fn typed_answers_to_the_close_prompt() {
        // Yes, typed and submitted with Enter.
        let mut app = fresh();
        let boundary = open_square(&mut app);
        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        type_and_enter(&mut app, "y");
        assert!(app.graphic_attributes.pending_fill_close.is_none());
        assert!(is_closed(&app, boundary) && has_hatch(&app));

        // No leaves the object open and unfilled.
        let mut app = fresh();
        let boundary = open_square(&mut app);
        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        type_and_enter(&mut app, "No");
        assert!(app.graphic_attributes.pending_fill_close.is_none());
        assert!(!is_closed(&app, boundary) && !has_hatch(&app));

        // Anything else keeps asking; a bare Enter takes the default Yes.
        let mut app = fresh();
        let boundary = open_square(&mut app);
        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        type_and_enter(&mut app, "maybe");
        assert!(app.graphic_attributes.pending_fill_close.is_some());
        assert!(app.tabs[app.active_tab].active_cmd.is_none());
        let _ = app.update(Message::CommandFinalize);
        assert!(is_closed(&app, boundary) && has_hatch(&app));

        // Escape cancels the request.
        let mut app = fresh();
        let boundary = open_square(&mut app);
        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        let _ = app.update(Message::CommandEscape);
        assert!(app.graphic_attributes.pending_fill_close.is_none());
        assert!(!is_closed(&app, boundary) && !has_hatch(&app));
    }

    #[test]
    fn linetype_scale_keeps_iso_pen_width_values() {
        let mut app = fresh();
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(EntityType::Line(Line::new()));
        app.tabs[i].scene.select_entity(line, false);
        for scale in [0.13, 0.18, 0.25, 0.35, 0.05] {
            let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScale(scale)));
            let stored = app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .linetype_scale;
            assert_eq!(stored, scale);
        }
        // Spinner steps do not accumulate floating-point noise.
        let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScale(0.25 + 0.1)));
        let stored = app.tabs[i]
            .scene
            .document
            .get_entity(line)
            .expect("line")
            .common()
            .linetype_scale;
        assert_eq!(stored, 0.35);
        for invalid in [0.0, -1.0, f64::INFINITY] {
            let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScale(invalid)));
        }
        let stored = app.tabs[i]
            .scene
            .document
            .get_entity(line)
            .expect("line")
            .common()
            .linetype_scale;
        assert_eq!(stored, 0.35);
    }

    #[test]
    fn graphic_attributes_starts_hidden_and_toggle_is_persisted() {
        let mut app = OpenCADStudio::new_for_test();
        app.apply_config(crate::app::config::AppConfig::default());
        assert!(!app.show_graphic_attributes);
        assert!(!app.ribbon.show_graphic_attributes);

        let _ = app.update(Message::ToggleGraphicAttributes);
        assert!(app.show_graphic_attributes);
        assert!(app.ribbon.show_graphic_attributes);
        assert!(app.current_config().show_graphic_attributes);

        let _ = app.update(Message::ToggleGraphicAttributes);
        assert!(!app.show_graphic_attributes);
        assert!(!app.ribbon.show_graphic_attributes);
        assert!(!app.current_config().show_graphic_attributes);
    }

    fn closed_square_on(app: &mut OpenCADStudio, layer: &str) -> Handle {
        let i = app.active_tab;
        if app.tabs[i].scene.document.layers.get(layer).is_none() {
            let mut new_layer = codec::tables::Layer::new(layer);
            new_layer.handle = app.tabs[i].scene.document.allocate_handle();
            let _ = app.tabs[i].scene.document.layers.add(new_layer);
        }
        let handle = open_square(app);
        if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
            if let EntityType::LwPolyline(polyline) = entity {
                polyline.is_closed = true;
            }
            entity.common_mut().layer = layer.to_string();
        }
        handle
    }

    fn fill_layers(app: &OpenCADStudio) -> Vec<String> {
        app.tabs[app.active_tab]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                EntityType::Hatch(hatch) => Some(hatch.common.layer.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn new_fills_follow_the_object_layer_unless_the_menu_option_is_set() {
        let mut app = fresh();
        closed_square_on(&mut app, "Walls");
        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        assert_eq!(fill_layers(&app), vec!["Walls".to_string()]);

        let mut app = fresh();
        let current = app.tabs[app.active_tab].active_layer.clone();
        closed_square_on(&mut app, "Walls");
        let _ = app.update(ga(GraphicAttributesMsg::ToggleFillOnCurrentLayer));
        assert!(app.current_config().graphic_fills_on_current_layer);
        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        assert_eq!(fill_layers(&app), vec![current]);
    }

    #[test]
    fn objects_on_locked_layers_get_no_fill() {
        let mut app = fresh();
        let i = app.active_tab;
        closed_square_on(&mut app, "Locked");
        closed_square_on(&mut app, "Open");
        app.tabs[i]
            .scene
            .document
            .layers
            .get_mut("Locked")
            .expect("locked layer")
            .lock();
        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        assert_eq!(fill_layers(&app), vec!["Open".to_string()]);
    }

    #[test]
    fn palette_fill_lookup_follows_document_changes() {
        let mut app = fresh();
        let i = app.active_tab;
        let boundary = closed_square_on(&mut app, "0");
        let selected = vec![boundary];
        let (fills, kind) = app.graphic_attributes.fills(&app.tabs[i].scene, &selected);
        assert!(fills.is_empty());
        assert_eq!(kind, GraphicAttribute::None);

        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        let (fills, kind) = app.graphic_attributes.fills(&app.tabs[i].scene, &selected);
        assert_eq!(fills.len(), 1);
        assert_eq!(kind, GraphicAttribute::Solid);
    }

    #[test]
    fn showing_the_palette_docks_it_below_properties_once() {
        use crate::app::config::DockSide;
        use crate::ui::dock::PanelId;
        let mut app = OpenCADStudio::new_for_test();
        app.apply_config(crate::app::config::AppConfig::default());
        assert_eq!(app.dock.location(PanelId::GraphicAttributes), None);

        let _ = app.update(Message::ToggleGraphicAttributes);
        let properties = app.dock.location(PanelId::Properties).expect("properties docked");
        assert_eq!(
            app.dock.location(PanelId::GraphicAttributes),
            Some((properties.0, properties.1 + 1))
        );

        // A place the user chose is kept when the palette is shown again.
        app.dock.dock(PanelId::GraphicAttributes, DockSide::Right, 0);
        let _ = app.update(Message::ToggleGraphicAttributes);
        let _ = app.update(Message::ToggleGraphicAttributes);
        assert_eq!(
            app.dock.location(PanelId::GraphicAttributes),
            Some((DockSide::Right, 0))
        );
    }

    fn output_since(app: &OpenCADStudio, start: usize) -> String {
        app.command_line.history[start..]
            .iter()
            .map(|entry| entry.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn undo_names_fill_and_linetype_changes() {
        let mut app = fresh();
        let boundary = add_closed_square(&mut app, 0.0);
        select(&mut app, &[boundary]);

        let _ = app.update(ga(GraphicAttributesMsg::Fill(GraphicAttribute::Solid)));
        let start = app.command_line.history.len();
        let _ = app.update(Message::Undo);
        assert!(output_since(&app, start).contains("Undo: Fill change"));

        let _ = app.update(ga(GraphicAttributesMsg::LineLinetype("Continuous".into())));
        let start = app.command_line.history.len();
        let _ = app.update(Message::Undo);
        assert!(output_since(&app, start).contains("Undo: Linetype"));
    }

    #[test]
    fn typed_linetype_scale_is_applied_on_enter() {
        let mut app = fresh();
        let i = app.active_tab;
        let line = app.tabs[i].scene.add_entity(EntityType::Line(Line::new()));
        select(&mut app, &[line]);

        // Enter without typing changes nothing.
        let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScaleSubmit));
        assert_eq!(common(&app, line).linetype_scale, 1.0);

        // Typing alone does not apply; Enter does, with a decimal comma too.
        let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScaleInput("0,35".into())));
        assert_eq!(common(&app, line).linetype_scale, 1.0);
        let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScaleSubmit));
        assert_eq!(common(&app, line).linetype_scale, 0.35);
        assert!(app.graphic_attributes.linetype_scale_input.is_none());

        // Invalid or non-positive input is dropped.
        for typed in ["abc", "0", "-2"] {
            let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScaleInput(typed.into())));
            let _ = app.update(ga(GraphicAttributesMsg::LineLinetypeScaleSubmit));
            assert_eq!(common(&app, line).linetype_scale, 0.35);
        }
    }

    #[test]
    fn clicking_the_linetype_scale_field_selects_it_once() {
        let mut app = fresh();
        let field = iced::widget::Id::new(palette::LINETYPE_SCALE_FIELD);
        let _ = app.update(Message::PropSyncActive(Some(field.clone())));
        assert!(app.graphic_attributes.linetype_scale_focused);

        // Focus elsewhere drops unsubmitted text.
        app.graphic_attributes.linetype_scale_input = Some("3".into());
        let _ = app.update(Message::PropSyncActive(None));
        assert!(!app.graphic_attributes.linetype_scale_focused);
        assert!(app.graphic_attributes.linetype_scale_input.is_none());
    }
}
