//! `dialog` arms and helpers, split out of the original `update.rs` (#mechanical decomposition).

#![allow(unused_imports)]
use super::util::*;
use super::{format_size, VIEWCUBE_HIT_SIZE};
use crate::app::helpers::{
    parse_coord, polar_constrain_near, ucs_rotate_vec, ucs_to_wcs, ucs_z_axis, CoordKind,
};
use crate::app::{Message, OpenCADStudio, POLY_START_DELAY_MS};
use crate::modules::ModuleEvent;
use crate::scene::model::object::GripApply;
use crate::scene::pick::grip::{find_hit_grip, find_hit_grip_paper, find_hit_grip_rte, GripEdit};
use crate::scene::{
    self, hover_id, CubeRegion, Scene, VIEWCUBE_DRAW_PX, VIEWCUBE_PAD, VIEWCUBE_PX,
};
use crate::ui::window::block_palette::BlockPaletteMsg;
use crate::ui::PropertiesPanel;
use acadrust::types::Color as AcadColor;
use acadrust::{EntityType as AcadEntityType, Handle};
use iced::time::Instant;
use iced::{mouse, Point, Task};

fn set_hatch_graphic_attribute(
    hatch: &mut acadrust::entities::Hatch,
    value: crate::ui::window::graphic_attributes::GraphicAttribute,
    visible_color: [f32; 4],
) {
    use crate::ui::window::graphic_attributes::GraphicAttribute;
    match value {
        GraphicAttribute::Solid => {
            hatch.pattern = acadrust::entities::HatchPattern::new("SOLID");
            hatch.pattern_type = acadrust::entities::HatchPatternType::Predefined;
            hatch.is_solid = true;
            hatch.gradient_color.enabled = false;
        }
        GraphicAttribute::Hatch => {
            let Some(entry) = crate::scene::model::hatch_patterns::catalog()
                .iter()
                .find(|entry| {
                matches!(
                    entry.gpu,
                    crate::scene::model::hatch_model::HatchPattern::Pattern(_)
                )
                })
            else {
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
            hatch.pattern_type = acadrust::entities::HatchPatternType::Predefined;
            hatch.is_solid = false;
            hatch.gradient_color.enabled = false;
        }
        GraphicAttribute::Gradient => {
            let to_color = |rgba: [f32; 4]| acadrust::types::Color::Rgb {
                r: (rgba[0] * 255.0).round().clamp(0.0, 255.0) as u8,
                g: (rgba[1] * 255.0).round().clamp(0.0, 255.0) as u8,
                b: (rgba[2] * 255.0).round().clamp(0.0, 255.0) as u8,
            };
            hatch.is_solid = true;
            hatch.gradient_color.enabled = true;
            hatch.gradient_color.name = "LINEAR".into();
            hatch.gradient_color.angle = 0.0;
            hatch.pattern_angle = 0.0;
            hatch.gradient_color.shift = 0.0;
            hatch.gradient_color.is_single_color = false;
            hatch.gradient_color.colors = vec![
                acadrust::entities::hatch::GradientColorEntry {
                    value: 0.0,
                    color: to_color(visible_color),
                },
                acadrust::entities::hatch::GradientColorEntry {
                    value: 1.0,
                    color: acadrust::types::Color::Rgb {
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

fn is_open_fill_boundary(entity: &acadrust::EntityType) -> bool {
    match entity {
        acadrust::EntityType::LwPolyline(polyline) => {
            !polyline.is_closed && polyline.vertices.len() >= 3
        }
        acadrust::EntityType::Polyline(polyline) => {
            !polyline.flags.is_closed() && polyline.vertices.len() >= 3
        }
        acadrust::EntityType::Polyline2D(polyline) => {
            !polyline.is_closed() && polyline.vertices.len() >= 3
        }
        acadrust::EntityType::Polyline3D(polyline) => {
            !polyline.flags.closed && polyline.vertices.len() >= 3
        }
        _ => false,
    }
}

fn close_fill_boundary(entity: &mut acadrust::EntityType) -> bool {
    match entity {
        acadrust::EntityType::LwPolyline(polyline) if !polyline.is_closed => {
            polyline.is_closed = true;
            true
        }
        acadrust::EntityType::Polyline(polyline) if !polyline.flags.is_closed() => {
            polyline.flags.set_closed(true);
            true
        }
        acadrust::EntityType::Polyline2D(polyline) if !polyline.is_closed() => {
            polyline.flags.set_closed(true);
            true
        }
        acadrust::EntityType::Polyline3D(polyline) if !polyline.flags.closed => {
            polyline.flags.closed = true;
            true
        }
        _ => false,
    }
}

struct ConcreteGraphicAttributes {
    color: acadrust::types::Color,
    linetype: String,
    lineweight: acadrust::types::LineWeight,
    transparency: acadrust::types::Transparency,
}

fn concrete_graphic_attributes(
    color: acadrust::types::Color,
    linetype: &str,
    lineweight: acadrust::types::LineWeight,
    transparency: acadrust::types::Transparency,
    layer: Option<&acadrust::tables::Layer>,
) -> ConcreteGraphicAttributes {
    let layer_color = layer.map_or(acadrust::types::Color::Index(7), |layer| {
        match layer.color {
            acadrust::types::Color::ByLayer | acadrust::types::Color::ByBlock => {
                acadrust::types::Color::Index(7)
            }
            color => color,
        }
    });
    let layer_linetype = layer
        .map(|layer| layer.line_type.as_str())
        .filter(|value| {
            !value.is_empty()
                && !value.eq_ignore_ascii_case("ByLayer")
                && !value.eq_ignore_ascii_case("ByBlock")
        })
        .unwrap_or("Continuous");
    let layer_lineweight = layer.map_or(acadrust::types::LineWeight::Default, |layer| {
        match layer.line_weight {
            acadrust::types::LineWeight::ByLayer | acadrust::types::LineWeight::ByBlock => {
                acadrust::types::LineWeight::Default
            }
            value => value,
        }
    });
    let layer_transparency = layer.map_or_else(
        || acadrust::types::Transparency::from_percent(0.0),
        |layer| {
            if layer.transparency.is_by_layer() || layer.transparency.is_by_block() {
                acadrust::types::Transparency::from_percent(0.0)
            } else {
                layer.transparency
            }
        },
    );

    ConcreteGraphicAttributes {
        color: match color {
            acadrust::types::Color::ByLayer => layer_color,
            acadrust::types::Color::ByBlock => acadrust::types::Color::Index(7),
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
            acadrust::types::LineWeight::ByLayer => layer_lineweight,
            acadrust::types::LineWeight::ByBlock => acadrust::types::LineWeight::Default,
            value => value,
        },
        transparency: if transparency.is_by_layer() {
            layer_transparency
        } else if transparency.is_by_block() {
            acadrust::types::Transparency::from_percent(0.0)
        } else {
            transparency
        },
    }
}

fn valid_layer_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= 255
        && !name.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '<' | '>' | '/' | '\\' | '"' | ':' | ';' | '?' | '*' | '|' | '=' | '`'
                )
        })
}

impl OpenCADStudio {
    fn graphic_attribute_handles(&self, tab: usize) -> Vec<Handle> {
        let selected = self.tabs[tab].scene.selected_handles_in_order();
        let document = &self.tabs[tab].scene.document;
        let mut seen = rustc_hash::FxHashSet::default();
        crate::ui::window::graphic_attributes::line_handles(document, &selected)
            .into_iter()
            .chain(crate::ui::window::graphic_attributes::fill_handles(
                document, &selected,
            ))
            .filter(|handle| seen.insert(*handle))
            .collect()
    }

    fn current_graphic_attributes(&self, tab: usize) -> ConcreteGraphicAttributes {
        let document = &self.tabs[tab].scene.document;
        let handles = self.graphic_attribute_handles(tab);
        let draw_depth = self.tabs[tab].scene.draw_depth_map();
        let entity = handles
            .iter()
            .filter_map(|handle| {
                document.get_entity(*handle).map(|entity| {
                    let depth = draw_depth
                        .get(&handle.value())
                        .map_or(0.0, |value| value[0]);
                    (depth, entity)
                })
            })
            .max_by(|(left, _), (right, _)| left.total_cmp(right))
            .map(|(_, entity)| entity);

        if let Some(entity) = entity {
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
            self.tabs[tab].active_layer.as_str()
        } else {
            header.current_layer_name.as_str()
        };
        concrete_graphic_attributes(
            header.current_entity_color,
            &header.current_linetype_name,
            acadrust::types::LineWeight::from_value(header.current_line_weight),
            document.current_entity_transparency(),
            document.layers.get(layer_name),
        )
    }

    pub(super) fn on_graphic_attributes_set_all(&mut self, by_layer: bool) -> iced::Task<Message> {
        self.graphic_attributes_header_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        let (color, linetype, lineweight, transparency) = if by_layer {
            (
                acadrust::types::Color::ByLayer,
                "ByLayer",
                acadrust::types::LineWeight::ByLayer,
                acadrust::types::Transparency::BY_LAYER,
            )
        } else {
            (
                acadrust::types::Color::ByBlock,
                "ByBlock",
                acadrust::types::LineWeight::ByBlock,
                acadrust::types::Transparency::BY_BLOCK,
            )
        };
        if selected.is_empty() {
            let header = &mut self.tabs[i].scene.document.header;
            header.current_entity_color = color;
            header.current_linetype_name = linetype.to_owned();
            header.current_linetype_handle = acadrust::Handle::NULL;
            header.current_line_weight = lineweight.value();
            self.tabs[i]
                .scene
                .document
                .set_current_entity_transparency(transparency);
            self.tabs[i].dirty = true;
            self.sync_ribbon_from_selection();
            self.refresh_properties();
            return iced::Task::none();
        }

        let handles: Vec<_> = self
            .graphic_attribute_handles(i)
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Graphic Attributes", &handles, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                crate::scene::view::dispatch::apply_color(entity, color);
                crate::scene::view::dispatch::apply_common_prop(entity, "linetype", linetype);
                crate::scene::view::dispatch::apply_line_weight(entity, lineweight);
                entity.common_mut().transparency = transparency;
            }
        });
        iced::Task::none()
    }

    pub(super) fn on_graphic_attributes_remove_references(&mut self) -> iced::Task<Message> {
        self.graphic_attributes_header_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        if selected.is_empty() {
            let values = self.current_graphic_attributes(i);
            let linetype_handle = self.tabs[i]
                .scene
                .document
                .line_types
                .iter()
                .find(|linetype| linetype.name.eq_ignore_ascii_case(&values.linetype))
                .map_or(acadrust::Handle::NULL, |linetype| linetype.handle);
            let header = &mut self.tabs[i].scene.document.header;
            header.current_entity_color = values.color;
            header.current_linetype_name = values.linetype;
            header.current_linetype_handle = linetype_handle;
            header.current_line_weight = values.lineweight.value();
            self.tabs[i]
                .scene
                .document
                .set_current_entity_transparency(values.transparency);
            self.tabs[i].dirty = true;
            self.sync_ribbon_from_selection();
            self.refresh_properties();
            return iced::Task::none();
        }

        let handles: Vec<_> = self
            .graphic_attribute_handles(i)
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        let values: rustc_hash::FxHashMap<_, _> = handles
            .iter()
            .filter_map(|handle| {
                let entity = self.tabs[i].scene.document.get_entity(*handle)?;
                let common = entity.common();
                Some((
                    *handle,
                    concrete_graphic_attributes(
                        common.color,
                        &common.linetype,
                        common.line_weight,
                        common.transparency,
                        self.tabs[i].scene.document.layers.get(&common.layer),
                    ),
                ))
            })
            .collect();
        self.apply_property_op(i, "Graphic Attributes", &handles, |app, handle| {
            let Some(values) = values.get(&handle) else {
                return;
            };
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                crate::scene::view::dispatch::apply_color(entity, values.color);
                crate::scene::view::dispatch::apply_common_prop(
                    entity,
                    "linetype",
                    &values.linetype,
                );
                crate::scene::view::dispatch::apply_line_weight(entity, values.lineweight);
                entity.common_mut().transparency = values.transparency;
            }
        });
        iced::Task::none()
    }

    pub(crate) fn create_graphic_attributes_layer(
        &mut self,
        raw_name: &str,
    ) -> iced::Task<Message> {
        let i = self.active_tab;
        let name = raw_name.trim();
        if !valid_layer_name(name) {
            self.command_line
                .push_error("Graphic Attributes: enter a valid layer name without <>/\\\":;?*|=`.");
            return iced::Task::none();
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
            return iced::Task::none();
        }

        let values = self.current_graphic_attributes(i);
        let layer_name = name.to_owned();
        let undo = self.begin_layer_undo(
            i,
            "Create layer with active settings",
            std::slice::from_ref(&layer_name),
        );
        let mut layer = acadrust::tables::Layer::new(name);
        layer.handle = self.tabs[i].scene.document.allocate_handle();
        layer.color = values.color;
        layer.line_type = values.linetype;
        layer.line_weight = values.lineweight;
        layer.transparency = values.transparency;
        use acadrust::tables::Table;
        if self.tabs[i].scene.document.layers.add(layer).is_err() {
            self.command_line
                .push_error("Graphic Attributes: the layer could not be created.");
            return iced::Task::none();
        }
        self.tabs[i].dirty = true;
        self.commit_layer_undo(i, undo);
        self.refresh_layer_panel();
        self.command_line.push_output(
            crate::tf!("Created layer \"{name}\" with the active Graphic Attributes settings.")
                .as_ref(),
        );
        iced::Task::none()
    }

    pub(super) fn open_gradient_editor(&mut self) {
        use crate::ui::window::gradient_editor::GradientEditorState;
        let i = self.active_tab;
        let selected: rustc_hash::FxHashSet<_> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        let Some(hatch) = self.tabs[i].scene.document.entities().find_map(|entity| {
            let acadrust::EntityType::Hatch(hatch) = entity else {
                return None;
            };
            if !hatch.gradient_color.enabled {
                return None;
            }
            (selected.contains(&entity.common().handle)
                || hatch
                    .paths
                    .iter()
                    .flat_map(|p| p.boundary_handles.iter())
                    .any(|handle| selected.contains(handle)))
                .then_some(hatch)
        }) else {
            return;
        };
        self.gradient_editor = Some(GradientEditorState::from_hatch(
            selected.iter().copied().collect(),
            hatch,
        ));
    }

    pub(super) fn apply_gradient_editor(&mut self) -> iced::Task<Message> {
        use crate::ui::window::gradient_editor::GradientColorMode;
        let Some(state) = self.gradient_editor.take() else {
            return iced::Task::none();
        };
        let i = self.active_tab;
        let sources: rustc_hash::FxHashSet<_> = state.handles.iter().copied().collect();
        let targets: Vec<_> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let acadrust::EntityType::Hatch(h) = entity else {
                    return None;
                };
                (h.gradient_color.enabled
                    && (sources.contains(&entity.common().handle)
                        || h.paths
                            .iter()
                            .any(|p| p.boundary_handles.iter().any(|x| sources.contains(x)))))
                .then_some(entity.common().handle)
            })
            .collect();
        if targets.is_empty() {
            return iced::Task::none();
        }
        if targets
            .iter()
            .any(|handle| self.tabs[i].scene.is_layer_locked(*handle))
        {
            self.command_line
                .push_error("Gradient Fill: the fill is on a locked layer.");
            self.gradient_editor = Some(state);
            return iced::Task::none();
        }
        let angle = state.angle_radians();
        let pending = self.begin_undo(i, "Edit gradient", targets.len(), true);
        for handle in &targets {
            let Some(mut entity) = self.tabs[i].scene.document.get_entity(*handle).cloned() else {
                continue;
            };
            if let acadrust::EntityType::Hatch(h) = &mut entity {
                h.gradient_color.name = state.kind.dxf_name(state.inverted).into();
                h.gradient_color.angle = angle;
                h.pattern_angle = angle;
                h.gradient_color.shift = if state.centered { 0.0 } else { 1.0 };
                h.gradient_color.is_single_color = state.color_mode == GradientColorMode::One;
                h.gradient_color.color_tint = state.shade_tint as f64;
                h.gradient_color.colors = vec![
                    acadrust::entities::hatch::GradientColorEntry {
                        value: 0.0,
                        color: state.color_1.clone(),
                    },
                    acadrust::entities::hatch::GradientColorEntry {
                        value: 1.0,
                        color: state.color_2.clone(),
                    },
                ];
            }
            self.tabs[i].scene.update_entity(entity);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        if let Some(p) = pending {
            self.commit_undo_delta(i, p)
        }
        iced::Task::none()
    }

    pub(super) fn on_graphic_attribute(
        &mut self,
        value: crate::ui::window::graphic_attributes::GraphicAttribute,
    ) -> iced::Task<Message> {
        self.pending_graphic_fill_close = None;
        self.on_graphic_attribute_impl(value, true)
    }

    pub(super) fn on_graphic_fill_close_response(
        &mut self,
        close: bool,
    ) -> iced::Task<Message> {
        let Some((value, handles, _)) = self.pending_graphic_fill_close.take() else {
            return iced::Task::none();
        };
        if close {
            let i = self.active_tab;
            let unlocked: Vec<_> = handles
                .into_iter()
                .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
                .collect();
            if !unlocked.is_empty() {
                let pending = self.begin_undo(i, "Close fill boundaries", unlocked.len(), true);
                for handle in unlocked {
                    let Some(mut entity) =
                        self.tabs[i].scene.document.get_entity(handle).cloned()
                    else {
                        continue;
                    };
                    if close_fill_boundary(&mut entity) {
                        self.tabs[i].scene.update_entity(entity);
                    }
                }
                self.tabs[i].dirty = true;
                if let Some(pending) = pending {
                    self.commit_undo_delta(i, pending);
                }
            }
        }
        self.on_graphic_attribute_impl(value, false)
    }

    fn on_graphic_attribute_impl(
        &mut self,
        value: crate::ui::window::graphic_attributes::GraphicAttribute,
        ask_to_close: bool,
    ) -> iced::Task<Message> {
        use crate::scene::model::hatch_model::{GradientKind, HatchPattern};
        use crate::ui::window::graphic_attributes::GraphicAttribute;
        self.graphic_attribute_menu_open = false;
        self.line_color_menu_open = false;
        self.line_linetype_menu_open = false;
        self.line_lineweight_menu_open = false;
        self.line_transparency_menu_open = false;
        self.solid_fill_color_menu_open = false;
        self.fill_transparency_menu_open = false;
        self.gradient_editor = None;
        self.hatch_editor_handles = None;
        self.graphic_hatch_pattern_search.clear();
        self.graphic_hatch_pattern_focus = 0;
        if value == GraphicAttribute::Varies {
            return iced::Task::none();
        }
        let i = self.active_tab;
        let selected: Vec<_> = self.tabs[i]
            .scene
            .selected_entities()
            .into_iter()
            .map(|(h, _)| h)
            .collect();
        if selected.is_empty() {
            self.command_line
                .push_info(crate::t!("Select one or more closed objects first.").as_ref());
            return iced::Task::none();
        }
        let open_boundaries: rustc_hash::FxHashSet<_> = selected
            .iter()
            .copied()
            .filter(|handle| {
                self.tabs[i]
                    .scene
                    .document
                    .get_entity(*handle)
                    .is_some_and(is_open_fill_boundary)
            })
            .collect();
        if ask_to_close && value != GraphicAttribute::None && !open_boundaries.is_empty() {
            self.pending_graphic_fill_close =
                Some((value, open_boundaries.iter().copied().collect(), selected.clone()));
            return iced::Task::none();
        }
        let selected_handles: rustc_hash::FxHashSet<_> = selected.iter().copied().collect();
        let (associated, new_hatches, updated_hatches, skipped) = if value == GraphicAttribute::None
        {
            let associated: Vec<_> = self.tabs[i]
                .scene
                .document
                .entities()
                .filter_map(|entity| {
                    let acadrust::EntityType::Hatch(hatch) = entity else {
                        return None;
                    };
                    let directly_selected = selected_handles.contains(&entity.common().handle);
                    let selected_boundary = hatch.is_associative
                        && hatch.paths.iter().any(|path| {
                            path.boundary_handles
                                .iter()
                                .any(|handle| selected_handles.contains(handle))
                        });
                    (directly_selected || selected_boundary).then_some(entity.common().handle)
                })
                .collect();
            (associated, Vec::new(), Vec::new(), 0)
        } else {
            let working = if self.tabs[i].editing_model_space() {
                self.tabs[i].ucs_xform().working_plane()
            } else {
                crate::command::WorkingPlane::default()
            };
            let normal = working.z.normalize_or(glam::DVec3::Z);
            let storage = crate::entities::curve::ocs_plane(
                acadrust::types::Vector3::new(normal.x, normal.y, normal.z),
                working.origin.dot(normal),
            );
            let plane = crate::command::WorkingPlane::new(
                glam::DVec3::from_array(storage.origin),
                glam::DVec3::from_array(storage.x_axis),
                glam::DVec3::from_array(storage.y_axis),
            );
            let sources = self.tabs[i].scene.boundary_sources_on_plane(plane, 1.0e-6);
            let (name, pattern) = match value {
                GraphicAttribute::Solid => ("SOLID".into(), HatchPattern::Solid),
                GraphicAttribute::Hatch => {
                    let Some(entry) = crate::scene::model::hatch_patterns::catalog()
                        .iter()
                        .find(|entry| matches!(entry.gpu, HatchPattern::Pattern(_)))
                    else {
                        self.command_line
                            .push_error("Graphic Attributes: no hatch pattern is available.");
                        return iced::Task::none();
                    };
                    (entry.name.clone(), entry.gpu.clone())
                }
                GraphicAttribute::Gradient => (
                    "LINEAR".into(),
                    HatchPattern::Gradient {
                        angle_deg: 0.0,
                        color2: [0.18, 0.18, 0.18, 0.0],
                        kind: GradientKind::Linear,
                        invert: false,
                        shift: 0.0,
                    },
                ),
                GraphicAttribute::None | GraphicAttribute::Varies => unreachable!(),
            };
            let origin = self.tabs[i].scene.document.hatch_origin();
            let mut hatches = Vec::new();
            let mut updated_hatches = Vec::new();
            let mut directly_updated = rustc_hash::FxHashSet::default();
            let mut changed_boundaries = rustc_hash::FxHashSet::default();
            let mut skipped = 0;
            for handle in &selected {
                if let Some(acadrust::EntityType::Hatch(hatch)) =
                    self.tabs[i].scene.document.get_entity(*handle)
                {
                    let mut hatch = hatch.clone();
                    let visible_color =
                        crate::scene::view::render::render_style_for_common_viewport(
                        &self.tabs[i].scene.document,
                        &hatch.common,
                        None,
                    )
                    .0;
                    set_hatch_graphic_attribute(&mut hatch, value, visible_color);
                    updated_hatches.push(acadrust::EntityType::Hatch(hatch));
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
                    hatches.push(hatch);
                    changed_boundaries.insert(*handle);
                } else {
                    if !open_boundaries.contains(handle) {
                        skipped += 1;
                    }
                }
            }
            // Replace only fills belonging to boundaries that successfully
            // produced a new hatch. Existing fills on skipped objects remain.
            let associated: Vec<_> = self.tabs[i]
                .scene
                .document
                .entities()
                .filter_map(|entity| {
                    let acadrust::EntityType::Hatch(hatch) = entity else {
                        return None;
                    };
                    (hatch.is_associative
                        && !directly_updated.contains(&entity.common().handle)
                        && hatch.paths.iter().any(|path| {
                            path.boundary_handles
                                .iter()
                                .any(|handle| changed_boundaries.contains(handle))
                        }))
                    .then_some(entity.common().handle)
                })
                .collect();
            (associated, hatches, updated_hatches, skipped)
        };
        if associated
            .iter()
            .chain(updated_hatches.iter().map(|entity| &entity.common().handle))
            .any(|handle| self.tabs[i].scene.is_layer_locked(*handle))
        {
            self.command_line
                .push_error("Graphic Attributes: an associated fill is on a locked layer.");
            return iced::Task::none();
        }
        if skipped > 0 {
            let message = if skipped == 1 {
                "Graphic Attributes: 1 unsupported object was not changed.".to_owned()
            } else {
                format!(
                    "Graphic Attributes: {skipped} unsupported objects were not changed."
                )
            };
            self.command_line.push_info(&message);
        }
        if associated.is_empty() && new_hatches.is_empty() && updated_hatches.is_empty() {
            return iced::Task::none();
        }
        let pending = self.begin_undo(
            i,
            "Graphic Attributes",
            associated.len() + new_hatches.len() + updated_hatches.len(),
            true,
        );
        self.tabs[i].scene.erase_entities(&associated);
        for entity in updated_hatches {
            self.tabs[i].scene.update_entity(entity);
        }
        let layer = self.tabs[i].active_layer.clone();
        for hatch in new_hatches {
            self.tabs[i].scene.add_hatch(hatch, Some(&layer), None);
        }
        self.tabs[i].dirty = true;
        self.refresh_properties();
        if let Some(pending) = pending {
            self.commit_undo_delta(i, pending);
        }
        iced::Task::none()
    }

    pub(super) fn on_solid_fill_color(
        &mut self,
        color: acadrust::types::Color,
    ) -> iced::Task<Message> {
        self.solid_fill_color_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        let handles = crate::ui::window::graphic_attributes::fill_handles(
            &self.tabs[i].scene.document,
            &selected,
        );
        let unlocked: Vec<_> = handles
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Fill color", &unlocked, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                crate::scene::view::dispatch::apply_color(entity, color);
            }
        });
        iced::Task::none()
    }

    pub(super) fn on_line_color(&mut self, color: acadrust::types::Color) -> iced::Task<Message> {
        self.line_color_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        if selected.is_empty() {
            return self.on_ribbon_color_changed(color);
        }
        let handles = crate::ui::window::graphic_attributes::line_handles(
            &self.tabs[i].scene.document,
            &selected,
        );
        let unlocked: Vec<_> = handles
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Line color", &unlocked, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                crate::scene::view::dispatch::apply_color(entity, color);
            }
        });
        iced::Task::none()
    }

    pub(super) fn on_line_linetype(&mut self, linetype: String) -> iced::Task<Message> {
        self.line_linetype_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        if selected.is_empty() {
            return self.on_ribbon_linetype_changed(linetype);
        }
        let handles = crate::ui::window::graphic_attributes::line_handles(
            &self.tabs[i].scene.document,
            &selected,
        );
        let unlocked: Vec<_> = handles
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Line linetype", &unlocked, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                crate::scene::view::dispatch::apply_common_prop(entity, "linetype", &linetype);
            }
        });
        iced::Task::none()
    }

    pub(super) fn on_line_lineweight(
        &mut self,
        lineweight: acadrust::types::LineWeight,
    ) -> iced::Task<Message> {
        self.line_lineweight_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        if selected.is_empty() {
            self.tabs[i].scene.document.header.current_line_weight = lineweight.value();
            self.tabs[i].dirty = true;
            self.ribbon.active_lineweight = lineweight;
            self.refresh_properties();
            return iced::Task::none();
        }
        let handles = crate::ui::window::graphic_attributes::line_handles(
            &self.tabs[i].scene.document,
            &selected,
        );
        let unlocked: Vec<_> = handles
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Line weight", &unlocked, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                crate::scene::view::dispatch::apply_line_weight(entity, lineweight);
            }
        });
        iced::Task::none()
    }

    pub(super) fn on_line_linetype_scale(&mut self, scale: f64) -> iced::Task<Message> {
        let Some(scale) = crate::ui::window::graphic_attributes::normalized_linetype_scale(scale)
        else {
            return iced::Task::none();
        };
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        if selected.is_empty() {
            self.tabs[i]
                .scene
                .document
                .header
                .current_entity_linetype_scale = scale;
            self.tabs[i].dirty = true;
            self.refresh_properties();
            return iced::Task::none();
        }
        let handles = crate::ui::window::graphic_attributes::line_handles(
            &self.tabs[i].scene.document,
            &selected,
        );
        let unlocked: Vec<_> = handles
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Linetype scale", &unlocked, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                entity.common_mut().linetype_scale = scale;
            }
        });
        iced::Task::none()
    }

    pub(super) fn on_line_transparency(
        &mut self,
        transparency: acadrust::types::Transparency,
    ) -> iced::Task<Message> {
        self.line_transparency_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        if selected.is_empty() {
            if self.tabs[i]
                .scene
                .document
                .set_current_entity_transparency(transparency)
            {
                self.tabs[i].dirty = true;
                self.refresh_properties();
            }
            return iced::Task::none();
        }
        let handles = crate::ui::window::graphic_attributes::line_handles(
            &self.tabs[i].scene.document,
            &selected,
        );
        let unlocked: Vec<_> = handles
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Line transparency", &unlocked, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                entity.common_mut().transparency = transparency;
            }
        });
        iced::Task::none()
    }

    pub(super) fn on_fill_transparency(
        &mut self,
        transparency: acadrust::types::Transparency,
    ) -> iced::Task<Message> {
        self.fill_transparency_menu_open = false;
        let i = self.active_tab;
        let selected = self.tabs[i].scene.selected_handles_in_order();
        let handles = crate::ui::window::graphic_attributes::fill_handles(
            &self.tabs[i].scene.document,
            &selected,
        );
        let unlocked: Vec<_> = handles
            .into_iter()
            .filter(|handle| !self.tabs[i].scene.is_layer_locked(*handle))
            .collect();
        self.apply_property_op(i, "Fill transparency", &unlocked, |app, handle| {
            if let Some(entity) = app.tabs[i].scene.document.get_entity_mut(handle) {
                entity.common_mut().transparency = transparency;
            }
        });
        iced::Task::none()
    }

    pub(in crate::app) fn open_save_dialog_window(&mut self, tab_idx: usize) -> Task<Message> {
        // Default the format dropdown to the loaded file's own format — its
        // DWG-vs-DXF kind (from the extension) and its version (from the parsed
        // document) — so Save-As round-trips the format instead of silently
        // re-targeting it. A new/unsaved drawing has no source format, so it
        // uses the application-wide default chosen in Options (#529).
        self.save_dialog_format = if let Some(path) = &self.tabs[tab_idx].current_path {
            let document = &self.tabs[tab_idx].scene.document;
            let is_dxf = crate::io::source_is_dxf(Some(path), document);
            let version = if is_dxf {
                document.version
            } else {
                document.dwg_source_version.unwrap_or(document.version)
            };
            crate::io::format_for_version(version, is_dxf)
        } else {
            self.default_save_format.clone()
        };

        // Pre-fill the default file name from the current path or the tab name;
        // the destination folder comes from the native OS dialog that follows.
        if let Some(p) = &self.tabs[tab_idx].current_path.clone() {
            if let Some(name) = p.file_name() {
                self.save_dialog_filename = name.to_string_lossy().into_owned();
            }
        } else {
            let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
            self.save_dialog_filename = format!("{}.{ext}", self.tabs[tab_idx].tab_display_name());
        }
        if self.tabs[tab_idx].recovery_save_as_required {
            let (ext, _) = crate::io::parse_save_format(&self.save_dialog_format);
            let stem = std::path::Path::new(&self.save_dialog_filename)
                .file_stem()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.tabs[tab_idx].tab_display_name());
            self.save_dialog_filename = format!("{stem}_recovered.{ext}");
        }
        self.aec_drop_acknowledged = false;
        self.active_modal = Some(crate::app::ModalKind::SaveDialog);
        Task::none()
    }

    pub(in crate::app) fn close_save_dialog_window(&mut self) -> Task<Message> {
        self.aec_drop_acknowledged = false;
        if self.active_modal == Some(crate::app::ModalKind::SaveDialog) {
            self.active_modal = None;
            self.reset_modal_geometry();
        }
        Task::none()
    }

    pub(in crate::app) fn open_unsaved_dialog_window(&mut self) -> Task<Message> {
        self.active_modal = Some(crate::app::ModalKind::Unsaved);
        // The unsaved-changes prompt renders inside the main window, so bring
        // that window to the foreground — a close signal can arrive while the
        // app is backgrounded, leaving the prompt unseen behind other windows.
        // `gain_focus` alone is ignored by most Linux WMs (focus-stealing
        // prevention), so pair it with an urgency hint so the window is at
        // least flagged for attention when the compositor blocks the raise.
        match self.main_window {
            Some(id) => Task::batch([
                iced::window::gain_focus(id),
                iced::window::request_user_attention(
                    id,
                    Some(iced::window::UserAttention::Critical),
                ),
            ]),
            None => Task::none(),
        }
    }

    pub(in crate::app) fn close_unsaved_dialog_window(&mut self) -> Task<Message> {
        if self.active_modal == Some(crate::app::ModalKind::Unsaved) {
            self.active_modal = None;
            self.reset_modal_geometry();
        }
        Task::none()
    }

    pub(super) fn on_ribbon_tool_click(
        &mut self,
        tool_id: String,
        event: ModuleEvent,
    ) -> Task<Message> {
                // Commands use `start_allowed`; other events need a drawing
                // and stay blocked on the Start page (#299, #388, #389).
                if self.tabs[self.active_tab].is_start && !matches!(event, ModuleEvent::Command(_)) {
                    self.ribbon.close_dropdown();
                    self.command_line
                        .push_info(crate::t!("No drawing open — use New or Open first.").as_ref());
                    return Task::none();
                }
                // Dismiss any open dropdown / collapsed-panel flyout on tool use,
                // and remember this tool as its panel's last-used one.
                self.ribbon.close_dropdown();
                self.ribbon.note_panel_tool(&tool_id);
                self.ribbon.activate_tool(&tool_id);
                match event {
                    ModuleEvent::Command(cmd) => {
                        let task = self.dispatch_command(&cmd);
                        // One-shot tools (view changes, clipboard, toggles,
                        // audits…) leave nothing running: no interactive
                        // command and no dialog. Their highlight would stick
                        // forever — turn it off now. Interactive commands and
                        // dialog owners keep theirs; the command end / modal
                        // close clears those. (#355)
                        let i = self.active_tab;
                        if self.tabs[i].active_cmd.is_none()
                            && self.active_modal.is_none()
                            && !self.tabs[i].pan_mode
                            && !self.tabs[i].orbit_mode
                            && !self.tabs[i].zoom_dynamic_mode
                        {
                            self.ribbon.deactivate_tool();
                        }
                        return task;
                    }
                    ModuleEvent::OpenFileDialog => {
                        self.command_line
                            .push_info(crate::t!("Open DWG/DXF: not yet implemented.").as_ref());
                    }
                    ModuleEvent::ClearModels => {
                        let i = self.active_tab;
                        self.tabs[i].scene.clear();
                        self.tabs[i].properties = PropertiesPanel::empty();
                self.command_line
                    .push_output(crate::t!("Scene cleared.").as_ref());
                    }
                    ModuleEvent::SetVisualStyle(name) => {
                        use crate::modules::view::visual_style;
                        match visual_style::mode_for_keyword(&name) {
                            Some(mode) => return Task::done(Message::SetRenderMode(mode)),
                            None => {
                                // Name the styles that do exist, from the same
                                // list every other caller reads.
                        self.command_line
                            .push_error(crate::tf!("Unknown visual style \"{name}\".").as_ref());
                                self.command_line
                                    .push_info(crate::t!(visual_style::keyword_prompt()).as_ref());
                            }
                        }
                    }
                    ModuleEvent::ToggleLayers => {
                        return Task::done(Message::ToggleLayers);
                    }
                    ModuleEvent::PluginFileDialog {
                        command,
                        title,
                        filter_name,
                        extensions,
                    } => {
                        return Task::perform(
                            async move {
                        let exts: Vec<&str> = extensions.iter().map(|s| s.as_str()).collect();
                                let path = crate::sys::file_dialog()
                                    .set_title(title)
                                    .add_filter(filter_name, &exts)
                                    .add_filter(crate::t!("All Files").as_ref(), &["*"])
                                    .pick_file()
                                    .await
                                    .map(|h| crate::sys::handle_path(&h));
                                (command, path)
                            },
                            |(command, path)| Message::PluginFileDialogResult { command, path },
                        );
                    }
                }
                // Every non-Command event above is a one-shot (state toggle,
                // clear, dialog spawn) — nothing stays running to clear the
                // highlight later, so turn it off here. (#355)
                self.ribbon.deactivate_tool();
                Task::none()
    }

    pub(super) fn on_unsaved_dialog_discard(&mut self) -> Task<Message> {
                match self.pending_close.take() {
                    Some(crate::app::PendingClose::Tab(idx)) => {
                        let close_win = self.close_unsaved_dialog_window();
                        // Discarded — drop this tab's autosave recovery copy.
                        #[cfg(not(target_arch = "wasm32"))]
                        let _ = std::fs::remove_file(self.autosave_target(idx));
                        if self.tabs.len() == 1 {
                            self.tab_counter += 1;
                    self.tabs[0] = crate::app::document::DocumentTab::new_drawing(self.tab_counter);
                            self.active_tab = 0;
                            self.apply_display_defaults(0);
                        } else {
                            self.tabs.remove(idx);
                            if self.active_tab >= self.tabs.len() {
                                self.active_tab = self.tabs.len() - 1;
                            }
                        }
                        // The active tab is now a fresh blank or a
                        // different existing tab; sync ribbon chips so
                        // they don't keep showing the discarded tab's
                        // last selection. #21.
                        self.sync_ribbon_layers();
                        self.sync_ribbon_from_selection();
                        return Task::batch([close_win, self.continue_tab_close_queue()]);
                    }
                    Some(crate::app::PendingClose::Quit) => {
                        if let Some(idx) = self.tabs.iter().position(|t| t.dirty) {
                            self.tabs[idx].dirty = false;
                        }
                        if self.tabs.iter().any(|t| t.dirty) {
                            // More dirty tabs remain — keep window open.
                            self.pending_close = Some(crate::app::PendingClose::Quit);
                        } else {
                            let close_win = self.close_unsaved_dialog_window();
                            return Task::batch(vec![close_win, self.exit_app()]);
                        }
                    }
                    None => {}
                }
                Task::none()
    }

    pub(super) fn on_unsaved_dialog_save(&mut self) -> Task<Message> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(pending) = self.pending_close.clone() else {
                return Task::none();
            };
            let (idx, continuation) = match pending {
                crate::app::PendingClose::Tab(idx) => (idx, crate::app::SaveContinuation::CloseTab),
                crate::app::PendingClose::Quit => {
                    let Some(idx) = self.tabs.iter().position(|tab| tab.dirty) else {
                        self.pending_close = None;
                        return Task::batch([self.close_unsaved_dialog_window(), self.exit_app()]);
                    };
                    (idx, crate::app::SaveContinuation::Quit)
                }
            };

            if self.active_save_jobs.contains_key(&self.tabs[idx].id) {
                self.command_line
                    .push_info(crate::t!("Save already running for this drawing.").as_ref());
                return Task::none();
            }

            if !self.tabs[idx].recovery_save_as_required {
                if let Some(path) = self.tabs[idx].current_path.clone() {
                    let version = self.tabs[idx].scene.document.version;
                    self.prepare_native_save(idx);
                    let close = self.close_unsaved_dialog_window();
                    let save = self.queue_native_save(
                        idx,
                        path,
                        version,
                        crate::app::SavePurpose::Manual,
                        continuation,
                        false,
                        true,
                    );
                    return Task::batch([close, save]);
                }
            }

            self.active_tab = idx;
            self.save_dialog_for_unsaved = true;
            let close = self.close_unsaved_dialog_window();
            let save = self.save_with_default_format(idx);
            return Task::batch([close, save]);
        }

        #[cfg(target_arch = "wasm32")]
        {
            match self.pending_close.take() {
                Some(crate::app::PendingClose::Tab(idx)) => {
                    self.pending_close = Some(crate::app::PendingClose::Tab(idx));
                    self.save_dialog_for_unsaved = true;
                    let close = self.close_unsaved_dialog_window();
                    let save = self.save_with_default_format(idx);
                    Task::batch([close, save])
                }
                Some(crate::app::PendingClose::Quit) => {
                    if let Some(idx) = self.tabs.iter().position(|tab| tab.dirty) {
                        self.active_tab = idx;
                        self.pending_close = Some(crate::app::PendingClose::Quit);
                        self.save_dialog_for_unsaved = true;
                        let close = self.close_unsaved_dialog_window();
                        let save = self.save_with_default_format(idx);
                        Task::batch([close, save])
                    } else {
                        Task::batch([self.close_unsaved_dialog_window(), self.exit_app()])
                    }
                }
                None => Task::none(),
            }
        }
    }

    /// Build a unique block name from a file stem: the stem itself, then
    /// "stem (2)", "stem (3)", … on collisions.
    fn block_name_from_file(&self, stem: &str) -> String {
        let i = self.active_tab;
        let base = stem.trim();
        if base.is_empty() {
            return self.unique_block_name("Block");
        }
        let doc = &self.tabs[i].scene.document;
        if doc.block_records.get(base).is_none() {
            return base.to_string();
        }
        let mut n = 2;
        loop {
            let name = format!("{base} ({n})");
            if doc.block_records.get(&name).is_none() {
                return name;
            }
            n += 1;
        }
    }

    /// Load an external DWG/DXF at `path` and define its model-space contents as
    /// one new block in the active drawing. Returns the new block's name, or an
    /// error message. Nested block definitions are imported first so nested
    /// INSERTs render (AutoCAD's "inserting a drawing imports its block defs").
    fn import_file_as_block(&mut self, path: std::path::PathBuf) -> Result<String, String> {
        let doc = crate::io::load_file(&path).map_err(|e| e.to_string())?;
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Block".to_string());
        self.import_document_as_block(doc, stem)
    }

    /// Define one block in the active drawing from a loaded `CadDocument`'s
    /// model-space entities (base = the file's model-space insertion base).
    fn import_document_as_block(
        &mut self,
        doc: acadrust::CadDocument,
        stem: String,
    ) -> Result<String, String> {
        let i = self.active_tab;
        // Model-space block record handle (Layout object first, name fallback).
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .unwrap_or(acadrust::Handle::NULL);
        let mut entities: Vec<acadrust::EntityType> = if model_br.is_null() {
            Vec::new()
        } else {
            let br = doc.block_records.iter().find(|br| br.handle == model_br);
            let handles = br.map(|b| b.entity_handles.clone()).unwrap_or_default();
            if !handles.is_empty() {
                // Authoritative ownership list (DWG and well-formed DXF).
                handles
                    .iter()
                    .filter_map(|h| doc.get_entity(*h))
                    .filter(|e| {
                        !matches!(
                            e,
                            acadrust::EntityType::Block(_) | acadrust::EntityType::BlockEnd(_)
                        )
                    })
                    .cloned()
                    .collect()
            } else {
                // Legacy DXF that omits 330 group codes: treat null-owner
                // entities as model-space content (mirrors belongs_to_visible_block).
                doc.entities()
                    .filter(|e| {
                        let o = e.common().owner_handle;
                        o == model_br || o.is_null()
                    })
                    .filter(|e| {
                        !matches!(
                            e,
                            acadrust::EntityType::Block(_) | acadrust::EntityType::BlockEnd(_)
                        )
                    })
                    .cloned()
                    .collect()
            }
        };
        if entities.is_empty() {
            return Err("No model-space entities in that file.".to_string());
        }
        let base = glam::DVec3::new(
            doc.header.model_space_insertion_base.x,
            doc.header.model_space_insertion_base.y,
            doc.header.model_space_insertion_base.z,
        );
        let name = self.block_name_from_file(&stem);
        // Capture every table record needed by the top-level entities and their
        // nested block definitions. Importing only the definitions leaves
        // source-only layers, linetypes, and text/dimension styles dangling.
        let deps = crate::app::ClipboardDeps::capture(&doc, &entities);
        // Capture the imported file's nested block definitions once. When a
        // name collides with a block already in the active drawing, we must
        // *preserve both*: keep the destination's block and import the file's
        // under a unique name, then re-point every INSERT at the renamed one —
        // otherwise a nested reference silently resolves to the destination's
        // unrelated definition. (#135-style collision, but for file imports.)
        let mut defs = deps.blocks.clone();
        let mut rename_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        // Reserve every destination block name plus every imported dependency
        // name, so importing a source file that itself holds "Door" and
        // "Door (2)" cannot generate a colliding second "Door (2)".
        let mut reserved: rustc_hash::FxHashSet<String> = rustc_hash::FxHashSet::default();
        for existing in self.tabs[i].scene.document.block_records.names() {
            reserved.insert(existing.to_string());
        }
        for def in &defs {
            reserved.insert(def.name.clone());
        }
        for def in &defs {
            // Keep the name only if it is truly unused in the active drawing.
            let used_in_dest = self.tabs[i]
                .scene
                .document
                .block_records
                .get(&def.name)
                .is_some();
            let dest_name = if used_in_dest {
                let mut n = 2;
                loop {
                    let candidate = format!("{} ({})", def.name, n);
                    if reserved.insert(candidate.clone()) {
                        break candidate;
                    }
                    n += 1;
                }
            } else {
                reserved.insert(def.name.clone());
                def.name.clone()
            };
            rename_map.insert(def.name.clone(), dest_name);
        }
        // Rewrite every INSERT in the model-space entities and in every captured
        // definition, then rename the definitions and define them.
        for entity in entities
            .iter_mut()
            .chain(defs.iter_mut().flat_map(|def| def.entities.iter_mut()))
        {
            if let acadrust::EntityType::Insert(ins) = entity {
                if let Some(new_name) = rename_map.get(&ins.block_name) {
                    ins.block_name = new_name.clone();
                }
            }
        }
        for def in &mut defs {
            if let Some(new_name) = rename_map.get(&def.name) {
                def.name = new_name.clone();
            }
        }
        // Every mutation below belongs to the one INSERT FILE undo step.
        self.push_undo_snapshot(i, "INSERT FILE");
        self.merge_dependencies(i, &deps);
        for def in defs {
            self.tabs[i]
                .scene
                .define_block_raw(&def.name, def.base_point, def.entities);
        }
        self.tabs[i]
            .scene
            .define_block_from_owned_entities(entities, &name, base)?;
        self.tabs[i].scene.populate_meshes_from_document();
        self.tabs[i].dirty = true;
        Ok(name)
    }

    pub(super) fn on_block_palette(
        &mut self,
        m: crate::ui::window::block_palette::BlockPaletteMsg,
    ) -> iced::Task<Message> {
        use crate::ui::window::block_palette::{BlockEntry, BlockPaletteMsg};
        match m {
            BlockPaletteMsg::Search(s) => {
                self.block_palette.search = s;
                iced::Task::none()
            }
            BlockPaletteMsg::CyclePreviewSize => {
                self.block_palette.preview_size =
                    crate::ui::window::block_palette::cycle_preview_size(
                        self.block_palette.preview_size,
                    );
                iced::Task::none()
            }
            BlockPaletteMsg::Refresh => {
                self.refresh_block_palette();
                iced::Task::none()
            }
            BlockPaletteMsg::PickFile => iced::Task::perform(
                async {
                    let handle = rfd::AsyncFileDialog::new()
                        .set_title(crate::t!("Select Drawing to Insert as Block").as_ref())
                        .add_filter(
                            crate::t!("DWG/DXF Files").as_ref(),
                            &["dwg", "dxf", "DWG", "DXF"],
                        )
                        .pick_file()
                        .await;
                    match handle {
                        Some(h) => Ok(crate::sys::handle_path(&h)),
                        None => Err("Cancelled".to_string()),
                    }
                },
                |r| Message::BlockPalette(BlockPaletteMsg::FilePicked(r)),
            ),
            BlockPaletteMsg::FilePicked(Ok(path)) => {
                match self.import_file_as_block(path) {
                    Ok(name) => {
                        self.command_line
                            .push_output(&crate::tf!("Inserting \"{name}\" from file."));
                        self.refresh_block_palette();
                        self.start_block_placement(&name);
                    }
                    Err(e) if e != "Cancelled" => {
                        self.command_line
                            .push_error(&crate::tf!("INSERT FILE: {e}"));
                    }
                    Err(_) => {}
                }
                iced::Task::none()
            }
            BlockPaletteMsg::FilePicked(Err(e)) => {
                if e != "Cancelled" {
                    self.command_line.push_error(&e);
                }
                iced::Task::none()
            }
            BlockPaletteMsg::Insert(name) => {
                self.start_block_placement(&name);
                iced::Task::none()
            }
        }
    }

    /// Dock chrome interaction (grab / pin / resize / hover / move) applied to
    /// whichever panel the message names.
    pub(super) fn on_dock(&mut self, m: crate::ui::dock::DockMsg) -> iced::Task<Message> {
        use crate::app::config::DockSide;
        use crate::ui::dock::{DockMsg, PanelId};
        match m {
            DockMsg::DockGrab(id) => {
                self.dock_dragging = Some(id);
                self.dock_resizing = None;
                self.dock_drag_last = None;
                self.dock_drag_target = self.dock.location(id);
                self.dock_expanded = Some(id);
                iced::Task::none()
            }
            DockMsg::ResizeGrab(id) => {
                self.dock_resizing = Some(id);
                self.dock_dragging = None;
                self.dock_drag_last = None;
                self.dock_drag_target = None;
                self.dock_expanded = Some(id);
                iced::Task::none()
            }
            DockMsg::WidthReset(id) => {
                self.dock.reset_width(id);
                self.save_config();
                iced::Task::none()
            }
            DockMsg::AutoCollapseToggle(id) => {
                let on = !self.dock.auto_collapse(id);
                self.dock.set_auto_collapse(id, on);
                self.dock_expanded = if on { None } else { Some(id) };
                self.save_config();
                iced::Task::none()
            }
            DockMsg::Close(id) => {
                match id {
                    PanelId::BlockPalette => {
                        self.show_block_palette = false;
                        self.block_palette.placing = None;
                    }
                    PanelId::ExternalReferences => {
                        self.show_external_references = false;
                    }
                    PanelId::Browser => {
                        self.show_browser = false;
                    }
                    PanelId::Properties => {
                        self.show_properties = false;
                        self.ribbon.set_properties(false);
                    }
                    PanelId::GraphicAttributes => {
                        self.show_graphic_attributes = false;
                        self.ribbon.set_graphic_attributes(false);
                        self.graphic_attributes_header_menu_open = false;
                        self.graphic_attribute_menu_open = false;
                        self.gradient_editor = None;
                        self.hatch_editor_handles = None;
                        self.save_config();
                    }
                }
                if self.dock_expanded == Some(id) {
                    self.dock_expanded = None;
                }
                if self.dock_dragging == Some(id) {
                    self.dock_dragging = None;
                }
                if self.dock_resizing == Some(id) {
                    self.dock_resizing = None;
                }
                iced::Task::none()
            }
            DockMsg::Hover(id) => {
                // Ignored while dragging/resizing: the pointer is over the
                // drag preview, not a rail, so a hover must not collapse the
                // panel being dragged or repoint the resize target.
                if self.dock_dragging.is_none() && self.dock_resizing.is_none() {
                    self.dock_expanded = Some(id);
                }
                iced::Task::none()
            }
            DockMsg::HoverExit => {
                if self.dock_dragging.is_none() && self.dock_resizing.is_none() {
                    if let Some(id) = self.dock_expanded {
                        if self.dock.auto_collapse(id) {
                            self.dock_expanded = None;
                        }
                    }
                }
                iced::Task::none()
            }
            DockMsg::DragMove(point) => {
                // NOTE: xref column drags intentionally do NOT ride this
                // path: its points live in workspace space while the header
                // tracker reports header-local points, and mixing the two
                // produced a one-time jump plus a stuck drag. Column moves
                // arrive via Message::XrefColMove only.
                if self.dock_dragging.is_some() {
                    let avail = self.tabs[self.active_tab]
                        .scene
                        .selection
                        .borrow()
                        .vp_size
                        .1;
                    let side = if point.x < self.win_size.0 * 0.5 {
                        DockSide::Left
                    } else {
                        DockSide::Right
                    };
                    let index = crate::ui::dock::drop_index(
                        point.y,
                        std::cmp::max(self.dock_visible_len(side), 1),
                        avail,
                    );
                    self.dock_drag_target = Some((side, index));
                } else if let Some(id) = self.dock_resizing {
                    if let Some(last) = self.dock_drag_last {
                        let dx = point.x - last.x;
                        let delta = match self.dock.location(id) {
                            Some((DockSide::Left, _)) => dx,
                            _ => -dx,
                        };
                        let cur = self.dock.settings(id).width + delta;
                        self.dock.set_width(id, cur);
                    }
                }
                if self.dock_dragging.is_some() || self.dock_resizing.is_some() {
                    self.dock_drag_last = Some(point);
                }
                iced::Task::none()
            }
            DockMsg::DragRelease => {
                if let Some(id) = self.dock_dragging {
                    if let Some((side, index)) = self.dock_drag_target {
                        if self.dock.dock(id, side, index) {
                            self.save_config();
                        }
                    }
                }
                self.dock_dragging = None;
                self.dock_resizing = None;
                self.dock_drag_last = None;
                self.dock_drag_target = None;
                self.xref_col_drag = None;
                self.xref_col_last = None;
                self.xref_split_drag = false;
                iced::Task::none()
            }
        }
    }

    /// Whether `id` is currently rendered (not closed, not on the start screen /
    /// clean-screen viewport). Mirrors the visibility filter the edge column
    /// renderer applies before building each side's stack.
    pub(crate) fn dock_panel_visible(&self, id: crate::ui::dock::PanelId) -> bool {
        use crate::ui::dock::PanelId;
        if self.tabs[self.active_tab].is_start || self.clean_screen {
            return false;
        }
        match id {
            PanelId::Properties => self.show_properties,
            PanelId::GraphicAttributes => self.show_graphic_attributes,
            PanelId::BlockPalette => self.show_block_palette,
            PanelId::ExternalReferences => self.show_external_references,
            PanelId::Browser => self.show_browser,
        }
    }

    /// Number of panels currently rendered on `side`. Closed panels keep their
    /// stack slot but take no vertical space, so drag preview geometry must
    /// size slots against only the panels that are actually visible.
    pub(crate) fn dock_visible_len(&self, side: crate::app::config::DockSide) -> usize {
        let ids: &[crate::ui::dock::PanelId] = match side {
            crate::app::config::DockSide::Left => &self.dock.left,
            crate::app::config::DockSide::Right => &self.dock.right,
        };
        ids.iter()
            .filter(|id| self.dock_panel_visible(**id))
            .count()
    }

    /// Start placing `name` through the INSERT command, skipping the name prompt.
    fn start_block_placement(&mut self, name: &str) {
        let i = self.active_tab;
        let wires = self
            .block_palette
            .blocks
            .iter()
            .find(|b| b.name == name)
            .map(|b| b.wires.clone())
            .unwrap_or_else(|| self.tabs[i].scene.block_preview_wires(name));
        use crate::modules::insert::insert_block::InsertBlockCommand;
        let cmd = InsertBlockCommand::new_for_block(name.to_string(), wires, glam::Vec3::ZERO);
        use crate::command::CadCommand;
        self.command_line.push_info(&cmd.prompt());
        self.tabs[i].active_cmd = Some(Box::new(cmd));
        self.block_palette.placing = Some(name.to_string());
    }

    /// Rebuild the panel's block list + cached wires from the active drawing.
    pub(crate) fn refresh_block_palette(&mut self) {
        let i = self.active_tab;
        let names = self.tabs[i].scene.custom_block_names();
        self.block_palette.cached_names = names.clone();
        self.block_palette.source_tab_id = Some(self.tabs[i].id);
        self.block_palette.source_block_epoch = self.tabs[i].scene.block_epoch;
        self.block_palette.blocks = names
            .into_iter()
            .map(|name| {
                let wires = self.tabs[i].scene.block_preview_wires(&name);
                crate::ui::window::block_palette::BlockEntry { name, wires }
            })
            .collect();
    }

    /// Cheap per-update check: rebuild when the active drawing's definitions
    /// changed, even when their names happen to stay the same.
    pub(crate) fn refresh_block_palette_if_stale(&mut self) {
        if !self.show_block_palette {
            return;
        }
        let i = self.active_tab;
        // `block_epoch` advances for every block-definition change, so it
        // avoids re-scanning every block name on unrelated application updates.
        if self.block_palette.source_tab_id != Some(self.tabs[i].id)
            || self.block_palette.source_block_epoch != self.tabs[i].scene.block_epoch
        {
            self.refresh_block_palette();
        }
    }

    /// Rebuild the Reference Manager's entry list from the active drawing.
    /// Uses the tab's session sets (unloaded + stat baseline) so CLI and
    /// palette agree; writes fresh load-time mtimes back into the tab's stat
    /// cache (`Stale` detectable from the second refresh on). Records the
    /// tab's `edit_revision` so the palette auto-rescans on doc changes.
    pub(crate) fn refresh_xref_manager(&mut self) {
        let i = self.active_tab;
        let base_dir: std::path::PathBuf = self.tabs[i]
            .current_path
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        let tab_id = self.tabs[i].id;
        let revision = self.tabs[i].edit_revision;
        let (host_name, host_path) = match &self.tabs[i].current_path {
            Some(p) => (
                p.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                p.to_string_lossy().into_owned(),
            ),
            None => (String::new(), String::new()),
        };
        let baselines = {
            let tab = &self.tabs[i];
            self.xref_manager.refresh(
                &tab.scene.document,
                &base_dir,
                tab.xref_unloaded.as_set(),
                &tab.xref_stat_cache.0,
                &host_name,
                &host_path,
            )
        };
        let tab = &mut self.tabs[i];
        for (key, mtime) in baselines {
            tab.xref_stat_cache.insert(key, mtime);
        }
        // A clean rescan clears the open-time missing notice.
        if !self
            .xref_manager
            .entries
            .iter()
            .any(|e| e.status == crate::io::xref_model::RefStatus::NotFound)
        {
            tab.xref_missing = 0;
        }
        self.xref_manager.source_tab_id = Some(tab_id);
        self.xref_manager.source_edit_revision = revision;
    }

    /// Re-scan when the palette is open but showing another tab's drawing or
    /// the active drawing changed since the last scan (CLI/palette mutation,
    /// XOPEN-return edit, undo/redo — all bump `edit_revision`). Cheap:
    /// `collect_entries` stats files without re-parsing the host.
    pub(crate) fn refresh_xref_manager_if_stale(&mut self) {
        if !self.dock_panel_visible(crate::ui::dock::PanelId::ExternalReferences) {
            return;
        }
        let i = self.active_tab;
        if self.xref_manager.source_tab_id != Some(self.tabs[i].id)
            || self.xref_manager.source_edit_revision != self.tabs[i].edit_revision
        {
            self.refresh_xref_manager();
        }
    }

    /// Shared post-mutation sequence for reference ops (CLI + palette):
    /// repopulate scene caches, mirror xref layers into the Layers panel,
    /// and mark the tab dirty. Both call sites use it so the 5-line
    /// sequence cannot drift.
    pub(crate) fn post_ref_op(&mut self, i: usize) {
        self.tabs[i].scene.populate_hatches_from_document();
        self.tabs[i].scene.populate_images_from_document();
        // Image/PDF unload is intentionally session-only (the file formats do
        // not persist an unloaded bit), so keep their source entities but
        // remove their decoded render models.  A reload repopulates the cache
        // after clearing the corresponding session key.
        let hidden_images: Vec<acadrust::types::Handle> = self.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| match entity {
                acadrust::EntityType::RasterImage(image)
                    if image
                        .definition_handle
                        .is_some_and(|key| self.tabs[i].xref_unloaded.is_unloaded(key.value())) =>
                {
                    Some(entity.common().handle)
                }
                acadrust::EntityType::Underlay(underlay)
                    if matches!(
                        underlay.underlay_type,
                        acadrust::entities::UnderlayType::Pdf
                    ) && self.tabs[i]
                        .xref_unloaded
                        .is_unloaded(underlay.definition_handle.value()) =>
                {
                    Some(entity.common().handle)
                }
                _ => None,
            })
            .collect();
        for handle in hidden_images {
            self.tabs[i].scene.images.remove(&handle);
        }
        self.tabs[i].scene.populate_meshes_from_document();
        self.refresh_layer_panel();
        self.tabs[i].dirty = true;
    }

    /// Execute a toolbar [`XrefPaletteOp`] on the palette's actionable
    /// selection. Same engine fns as the CLI arms, batched with per-item
    /// report lines in the CLI wording; finishes with [`post_ref_op`] plus
    /// a palette rescan (mirror of the CLI post-op sequence).
    pub(crate) fn xref_manager_op(&mut self, op: crate::ui::window::xref_manager::XrefPaletteOp) {
        if cfg!(target_arch = "wasm32") {
            self.command_line.push_error(
                crate::t!(
                    "Reference changes are not available on web — the reference list is read-only."
                )
                .as_ref(),
            );
            return;
        }
        use crate::ui::window::xref_manager::XrefPaletteOp;
        let i = self.active_tab;
        // Full selection (nested rows included) with per-entry metadata, so
        // nested rows report per-entry errors instead of being silently
        // dropped by `actionable_selection` (mirrors the CLI arms).
        let picked: Vec<(u64, String, crate::io::xref_model::RefKind, bool)> = {
            let mut idx: Vec<usize> = self.xref_manager.selected.iter().copied().collect();
            idx.sort_unstable();
            idx.iter()
                .filter_map(|idx| {
                    self.xref_manager
                        .entries
                        .get(*idx)
                        .map(|e| (e.key, e.name.clone(), e.kind, e.parent_key.is_some()))
                })
                .collect()
        };
        if picked.is_empty() {
            self.command_line
                .push_info(crate::t!("Select a reference first.").as_ref());
            return;
        }
        let needs_host = matches!(
            op,
            XrefPaletteOp::Reload | XrefPaletteOp::Bind | XrefPaletteOp::Pathtype(_)
        );
        let host: Option<std::path::PathBuf> = self.tabs[i].current_path.clone();
        if needs_host && host.is_none() {
            self.command_line.push_error(
                crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").as_ref(),
            );
            return;
        }
        let label = match op {
            XrefPaletteOp::Open => "XREF-OPEN",
            XrefPaletteOp::Detach => "XREF-DETACH",
            XrefPaletteOp::Unload => "XREF-UNLOAD",
            XrefPaletteOp::Reload => "XREF-RELOAD",
            XrefPaletteOp::Bind => "XREF-BIND",
            XrefPaletteOp::Overlay => "XREF-OVERLAY",
            XrefPaletteOp::Attach => "XREF-ATTACH",
            XrefPaletteOp::Pathtype(_) => "XREF-PATHTYPE",
        };
        self.push_undo_snapshot(i, label);
        let mut done = 0usize;
        match op {
            XrefPaletteOp::Detach => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot detach nested reference '{}'. Detach it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::detach_reference(&mut self.tabs[i].scene.document, *key)
                    {
                        Ok(name) => {
                            self.command_line
                                .push_output(crate::tf!("XREF: detached \"{}\".", name).as_ref());
                            self.tabs[i].xref_unloaded.remove(key);
                            self.tabs[i].xref_stat_cache.remove(key);
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Unload => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot unload nested reference '{}'. Unload it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::unload_reference(&mut self.tabs[i].scene.document, *key)
                    {
                        Ok(name) => {
                            self.command_line
                                .push_output(crate::tf!("XREF: unloaded \"{}\".", name).as_ref());
                            self.tabs[i].xref_unloaded.add(*key);
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Reload => {
                let base_dir: std::path::PathBuf = host
                    .as_ref()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                // Drawing references only: image/PDF rows keep their flags
                // untouched and report per-entry (their names never match the
                // DWG-only `XrefInfo` list, so matching after the flag clear
                // would spuriously report no-match).
                let dwg_picked: Vec<(u64, String)> = picked
                    .iter()
                    .filter(|(_, _, kind, is_nested)| {
                        if *is_nested {
                            return false;
                        }
                        *kind == crate::io::xref_model::RefKind::DwgXref
                    })
                    .map(|(key, name, _, _)| (*key, name.clone()))
                    .collect();
                for (_, name, kind, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot reload nested reference '{}'. Reload it in its host drawing.",
                            name
                        ).as_ref());
                    } else if *kind != crate::io::xref_model::RefKind::DwgXref {
                        self.command_line.push_error(
                            crate::tf!("{}: reload applies to drawing references only.", name)
                                .as_ref(),
                        );
                    }
                }
                for (key, _) in &dwg_picked {
                    self.tabs[i].xref_unloaded.remove(key);
                    self.tabs[i].xref_stat_cache.remove(key);
                }
                let handles: rustc_hash::FxHashSet<acadrust::types::Handle> = self.tabs[i]
                    .scene
                    .document
                    .block_records
                    .iter()
                    .filter(|br| dwg_picked.iter().any(|(key, _)| *key == br.handle.value()))
                    .map(|br| br.handle)
                    .collect();
                let (infos, _dropped) = crate::io::xref::resolve_xrefs_for_keys(
                    &mut self.tabs[i].scene.document,
                    &base_dir,
                    &handles,
                );
                // Targeted reload updates baselines only for selected rows.
                {
                    let fresh = crate::io::xref::collect_entries_with_prev(
                        &self.tabs[i].scene.document,
                        &base_dir,
                        self.tabs[i].xref_unloaded.as_set(),
                        &self.tabs[i].xref_stat_cache.0,
                    );
                    for e in &fresh {
                        if !dwg_picked.iter().any(|(key, _)| *key == e.key) {
                            continue;
                        }
                        if e.status == crate::io::xref_model::RefStatus::Loaded {
                            if let Some(m) = e.modified {
                                self.tabs[i].xref_stat_cache.insert(e.key, m);
                            }
                        }
                    }
                }
                for (_, name) in &dwg_picked {
                    match infos.iter().find(|n| n.name.eq_ignore_ascii_case(name)) {
                        Some(info) => {
                            self.report_xref_status(info);
                            done += 1;
                        }
                        None => self.command_line.push_error(
                            crate::tf!("XREF: no references match '{}'.", name).as_ref(),
                        ),
                    }
                }
            }
            XrefPaletteOp::Overlay => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot overlay nested reference '{}'. Overlay it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::set_ref_type(
                        &mut self.tabs[i].scene.document,
                        *key,
                        crate::io::xref_model::RefType::Overlay,
                    ) {
                        Ok(name) => {
                            self.command_line.push_output(
                                crate::tf!("XREF: \"{}\" set to Overlay.", name).as_ref(),
                            );
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Attach => {
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot attach nested reference '{}'. Attach it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::set_ref_type(
                        &mut self.tabs[i].scene.document,
                        *key,
                        crate::io::xref_model::RefType::Attach,
                    ) {
                        Ok(name) => {
                            self.command_line.push_output(
                                crate::tf!("XREF: \"{}\" set to Attach.", name).as_ref(),
                            );
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Bind => {
                let base_dir: std::path::PathBuf = host
                    .as_ref()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                let host_dir: std::path::PathBuf = host
                    .as_ref()
                    .and_then(|p| p.parent().map(|p| p.to_path_buf()))
                    .unwrap_or_else(|| std::path::PathBuf::from("."));
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(
                            crate::tf!(
                            "XREF: cannot bind nested reference '{}'. Bind it in its host drawing.",
                            name
                        )
                            .as_ref(),
                        );
                        continue;
                    }
                    match crate::io::xref::bind_reference(
                        &mut self.tabs[i].scene.document,
                        *key,
                        &base_dir,
                        &host_dir,
                    ) {
                        Ok(outcome) => {
                            if outcome.unremapped == 0 {
                                self.command_line.push_output(
                                    crate::tf!("XREF: bound \"{}\".", outcome.name).as_ref(),
                                );
                            } else {
                                self.command_line.push_output(crate::tf!(
                                    "XREF: bound \"{}\" with {} unremapped style handles (see bind limitations).",
                                    outcome.name, outcome.unremapped
                                ).as_ref());
                            }
                            self.tabs[i].xref_unloaded.remove(key);
                            self.tabs[i].xref_stat_cache.remove(key);
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Pathtype(pathtype) => {
                let host = host.unwrap_or_else(|| std::path::PathBuf::from("."));
                for (key, name, _, is_nested) in &picked {
                    if *is_nested {
                        self.command_line.push_error(crate::tf!(
                            "XREF: cannot set path type for nested reference '{}'. Set it in its host drawing.",
                            name
                        ).as_ref());
                        continue;
                    }
                    match crate::io::xref::apply_pathtype(
                        &mut self.tabs[i].scene.document,
                        *key,
                        pathtype,
                        &host,
                    ) {
                        Ok(_) => {
                            self.command_line.push_output(
                                crate::tf!("XREF: Path set for \"{}\" — Reload to apply.", name)
                                    .as_ref(),
                            );
                            done += 1;
                        }
                        Err(msg) => self.command_line.push_error(msg.as_str()),
                    }
                }
            }
            XrefPaletteOp::Open => {
                // Handled in update/mod.rs XrefRowOp dispatch (navigation), not here.
            }
        }
        if done > 0 {
            self.post_ref_op(i);
        }
        self.refresh_xref_manager();
    }

    /// Reload every direct drawing reference (toolbar Reload All). Same
    /// engine path as `XRELOAD`: undo snapshot, session flags cleared,
    /// full resolve, per-ref report, stat baselines refreshed.
    pub(crate) fn xref_manager_reload_all(&mut self) {
        if cfg!(target_arch = "wasm32") {
            self.command_line.push_error(
                crate::t!(
                    "Reference changes are not available on web — the reference list is read-only."
                )
                .as_ref(),
            );
            return;
        }
        let i = self.active_tab;
        let Some(path) = self.tabs[i].current_path.clone() else {
            self.command_line.push_error(
                crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").as_ref(),
            );
            return;
        };
        let Some(base_dir) = path.parent().map(|p| p.to_path_buf()) else {
            self.command_line.push_error(
                crate::t!("XREF  Save the drawing first to resolve relative XREF paths.").as_ref(),
            );
            return;
        };
        let reload_keys: Vec<u64> = crate::io::xref::collect_entries_with_prev(
            &self.tabs[i].scene.document,
            &base_dir,
            self.tabs[i].xref_unloaded.as_set(),
            &self.tabs[i].xref_stat_cache.0,
        )
        .iter()
        .filter(|e| e.kind == crate::io::xref_model::RefKind::DwgXref && e.parent_key.is_none())
        .map(|e| e.key)
        .collect();
        self.push_undo_snapshot(i, "XREF-RELOAD");
        for key in &reload_keys {
            self.tabs[i].xref_unloaded.remove(key);
            self.tabs[i].xref_stat_cache.remove(key);
        }
        let (infos, _dropped) =
            crate::io::xref::resolve_xrefs(&mut self.tabs[i].scene.document, &base_dir);
        let fresh = crate::io::xref::collect_entries_with_prev(
            &self.tabs[i].scene.document,
            &base_dir,
            self.tabs[i].xref_unloaded.as_set(),
            &self.tabs[i].xref_stat_cache.0,
        );
        for e in &fresh {
            if e.status == crate::io::xref_model::RefStatus::Loaded {
                if let Some(m) = e.modified {
                    self.tabs[i].xref_stat_cache.insert(e.key, m);
                }
            }
        }
        for info in &infos {
            self.report_xref_status(info);
        }
        self.post_ref_op(i);
        self.refresh_xref_manager();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::OpenCADStudio;
    use acadrust::entities::Line;
    use acadrust::types::Vector3;
    use acadrust::EntityType;

    fn fresh() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        app.automation_op(r#"{"op":"new"}"#);
        app
    }

    #[test]
    fn graphic_attributes_creates_one_associative_fill_per_object() {
        use crate::ui::window::graphic_attributes::GraphicAttribute;
        use acadrust::entities::LwPolyline;
        use acadrust::types::Vector2;

        let mut app = fresh();
        let i = app.active_tab;
        let mut boundaries = Vec::new();
        // Coincident boundaries must still own independent associations.
        for offset in [0.0, 0.0] {
            let mut polyline = LwPolyline::new();
            for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
                polyline.add_point(Vector2::new(x + offset, y));
            }
            polyline.close();
            let handle = app.tabs[i]
                .scene
                .add_entity(EntityType::LwPolyline(polyline));
            app.tabs[i].scene.select_entity(handle, false);
            boundaries.push(handle);
        }

        let _ = app.on_graphic_attribute(GraphicAttribute::Solid);
        let hatches: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let EntityType::Hatch(hatch) = entity else {
                    return None;
                };
                Some(hatch)
            })
            .collect();
        assert_eq!(hatches.len(), 2);
        assert!(hatches
            .iter()
            .all(|hatch| hatch.is_solid && hatch.is_associative));
        assert!(hatches.iter().all(|hatch| hatch
            .paths
            .iter()
            .flat_map(|path| &path.boundary_handles)
            .count()
            == 1));

        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entity(boundaries[0], false);
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);
        let hatches: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let EntityType::Hatch(hatch) = entity else {
                    return None;
                };
                Some(hatch)
            })
            .collect();
        assert_eq!(hatches.len(), 2);
        assert_eq!(
            hatches
                .iter()
                .filter(|hatch| hatch.gradient_color.enabled)
                .count(),
            1
        );
        assert_eq!(
            hatches
                .iter()
                .filter(|hatch| hatch.is_solid && !hatch.gradient_color.enabled)
                .count(),
            1
        );

        // An invalid boundary is skipped while valid selected boundaries are
        // still updated.
        let line = app.tabs[i].scene.add_entity(EntityType::Line(Line::new()));
        app.tabs[i].scene.select_entity(line, false);
        let _ = app.on_graphic_attribute(GraphicAttribute::Solid);
        let hatches: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let EntityType::Hatch(hatch) = entity else {
                    return None;
                };
                Some(hatch)
            })
            .collect();
        assert_eq!(hatches.len(), 2);
        assert!(hatches
            .iter()
            .all(|hatch| hatch.is_solid && !hatch.gradient_color.enabled));
        assert!(hatches.iter().all(|hatch| hatch
            .paths
            .iter()
            .flat_map(|path| &path.boundary_handles)
            .all(|handle| *handle != line)));

        // Editing a common gradient updates each object's independent hatch
        // and records the whole operation as one undo step.
        app.tabs[i].scene.deselect_all();
        for handle in &boundaries {
            app.tabs[i].scene.select_entity(*handle, false);
        }
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);
        app.open_gradient_editor();
        let editor = app.gradient_editor.as_mut().expect("gradient editor");
        assert_eq!(editor.handles.len(), 2);
        editor.kind = crate::scene::model::hatch_model::GradientKind::Curved;
        editor.inverted = true;
        editor.angle = "405°".into();
        editor.centered = false;
        let undo_before = app.tabs[i].history.undo_stack.len();
        let _ = app.apply_gradient_editor();
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);

        let gradients: Vec<_> = app.tabs[i]
            .scene
            .document
            .entities()
            .filter_map(|entity| {
                let EntityType::Hatch(hatch) = entity else {
                    return None;
                };
                hatch.gradient_color.enabled.then_some(hatch)
            })
            .collect();
        assert_eq!(gradients.len(), 2);
        assert!(gradients.iter().all(|hatch| {
            hatch.gradient_color.name == "INVCURVED"
                && (hatch.gradient_color.angle.to_degrees() - 45.0).abs() < 1.0e-9
                && (hatch.gradient_color.shift - 1.0).abs() < 1.0e-9
        }));

        // A selection change closes the flyout and it remains closed until
        // the user explicitly opens it again.
        app.open_gradient_editor();
        assert!(app.gradient_editor.is_some());
        app.tabs[i].scene.deselect_all();
        let _ = app.update(Message::CloseGraphicAttributeDropdown);
        assert!(app.gradient_editor.is_none());

        let selected_hatch = app.tabs[i]
            .scene
            .document
            .entities()
            .find_map(|entity| {
                matches!(entity, EntityType::Hatch(_)).then_some(entity.common().handle)
            })
            .expect("gradient hatch");
        let associated_lines = crate::ui::window::graphic_attributes::line_handles(
            &app.tabs[i].scene.document,
            &[selected_hatch],
        );
        assert_eq!(associated_lines.len(), 1);
        assert!(boundaries.contains(&associated_lines[0]));
        assert_eq!(
            crate::ui::window::graphic_attributes::current(
                &app.tabs[i].scene.document,
                &[selected_hatch],
            ),
            GraphicAttribute::Gradient,
        );
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entity(selected_hatch, false);

        // A directly selected hatch is edited in place instead of being
        // rejected as an unsupported boundary object.
        let _ = app.on_graphic_attribute(GraphicAttribute::Solid);
        assert_eq!(
            crate::ui::window::graphic_attributes::current(
                &app.tabs[i].scene.document,
                &[selected_hatch],
            ),
            GraphicAttribute::Solid,
        );
        let _ = app.on_solid_fill_color(acadrust::types::Color::Index(3));
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(selected_hatch)
                .expect("solid hatch")
                .common()
                .color,
            acadrust::types::Color::Index(3),
        );
        let _ = app.on_fill_transparency(acadrust::types::Transparency::from_percent(0.35));
        assert!(
            (app.tabs[i]
            .scene
            .document
            .get_entity(selected_hatch)
            .expect("transparent solid hatch")
            .common()
            .transparency
            .as_percent()
            - 0.35)
            .abs()
                < 0.01
        );
        let _ = app.on_fill_transparency(acadrust::types::Transparency::BY_LAYER);
        assert!(app.tabs[i]
            .scene
            .document
            .get_entity(selected_hatch)
            .expect("ByLayer solid hatch")
            .common()
            .transparency
            .is_by_layer());

        // Line controls ignore HATCH entities, while fill controls ignore
        // ordinary geometry in the same mixed selection.
        app.tabs[i].scene.select_entity(line, false);
        let _ = app.on_line_color(acadrust::types::Color::Index(4));
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .color,
            acadrust::types::Color::Index(4),
        );
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(selected_hatch)
                .expect("solid hatch")
                .common()
                .color,
            acadrust::types::Color::Index(3),
        );
        let _ = app.on_solid_fill_color(acadrust::types::Color::Index(5));
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(selected_hatch)
                .expect("solid hatch")
                .common()
                .color,
            acadrust::types::Color::Index(5),
        );
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .color,
            acadrust::types::Color::Index(4),
        );
        let _ = app.on_line_linetype("ByBlock".to_string());
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .linetype,
            "ByBlock",
        );
        let _ = app.on_line_lineweight(acadrust::types::LineWeight::Value(50));
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .line_weight,
            acadrust::types::LineWeight::Value(50),
        );
        assert_ne!(
            app.tabs[i]
                .scene
                .document
                .get_entity(selected_hatch)
                .expect("solid hatch")
                .common()
                .line_weight,
            acadrust::types::LineWeight::Value(50),
        );
        let _ = app.update(Message::LineLinetypeScaleInput("2.5".to_string()));
        assert!(
            (app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .linetype_scale
                - 2.5)
                .abs()
                < f64::EPSILON
        );
        assert!(
            (app.tabs[i]
                .scene
                .document
                .get_entity(selected_hatch)
                .expect("solid hatch")
                .common()
                .linetype_scale
                - 1.0)
                .abs()
                < f64::EPSILON
        );
        let _ = app.update(Message::LineLinetypeScaleChanged(f64::NAN));
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .linetype_scale,
            2.5
        );
        let _ = app.update(Message::LineLinetypeScaleChanged(1.2000000000000002));
        assert_eq!(
            app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .linetype_scale,
            1.2
        );
        let _ = app.on_line_transparency(acadrust::types::Transparency::from_percent(0.2));
        assert!(
            (app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .transparency
                .as_percent()
                - 0.2)
                .abs()
                < 0.01
        );
        assert!(app.tabs[i]
            .scene
            .document
            .get_entity(selected_hatch)
            .expect("solid hatch")
            .common()
            .transparency
            .is_by_layer());
        let _ = app.on_fill_transparency(acadrust::types::Transparency::from_percent(0.4));
        assert!(
            (app.tabs[i]
                .scene
                .document
                .get_entity(line)
                .expect("line")
                .common()
                .transparency
                .as_percent()
                - 0.2)
                .abs()
                < 0.01
        );
        let pattern_name = crate::scene::model::hatch_patterns::catalog()
            .iter()
            .find(|entry| {
                matches!(
                    entry.gpu,
                    crate::scene::model::hatch_model::HatchPattern::Pattern(_)
                )
            })
            .expect("pattern hatch")
            .name
            .clone();
        let _ = app.update(Message::GraphicHatchPatternChanged(pattern_name.clone()));
        let selected_pattern = match app.tabs[i]
            .scene
            .document
            .get_entity(selected_hatch)
            .expect("pattern hatch")
        {
            EntityType::Hatch(hatch) => hatch.pattern.name.as_str(),
            _ => panic!("fill target is not a hatch"),
        };
        assert_eq!(selected_pattern, pattern_name);
        app.tabs[i].scene.deselect_all();
        app.tabs[i].scene.select_entity(selected_hatch, false);

        let _ = app.on_graphic_attribute(GraphicAttribute::Hatch);
        assert_eq!(
            crate::ui::window::graphic_attributes::current(
                &app.tabs[i].scene.document,
                &[selected_hatch],
            ),
            GraphicAttribute::Hatch,
        );
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);
        assert_eq!(
            crate::ui::window::graphic_attributes::current(
                &app.tabs[i].scene.document,
                &[selected_hatch],
            ),
            GraphicAttribute::Gradient,
        );
        app.open_gradient_editor();
        assert!(app.gradient_editor.is_some());
        let _ = app.on_graphic_attribute(GraphicAttribute::Hatch);
        assert!(app.gradient_editor.is_none());
        let _ = app.update(Message::HatchEditorOpen);
        assert!(app.hatch_editor_handles.is_some());
        let _ = app.on_graphic_attribute(GraphicAttribute::Gradient);
        assert!(app.hatch_editor_handles.is_none());
        assert!(app.gradient_editor.is_none());

        let undo_before = app.tabs[i].history.undo_stack.len();
        let _ = app.on_graphic_attribute(GraphicAttribute::None);
        assert!(app.tabs[i]
            .scene
            .document
            .get_entity(selected_hatch)
            .is_none());
        assert_eq!(app.tabs[i].history.undo_stack.len(), undo_before + 1);
    }

    #[test]
    fn graphic_attributes_can_close_an_open_boundary_from_the_command_prompt() {
        use crate::ui::window::graphic_attributes::GraphicAttribute;
        use acadrust::entities::LwPolyline;
        use acadrust::types::Vector2;

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

        let _ = app.update(Message::GraphicAttributeChanged(GraphicAttribute::Solid));
        assert!(app.pending_graphic_fill_close.is_some());
        assert!(!app.tabs[i].scene.document.entities().any(|entity| {
            matches!(entity, EntityType::Hatch(_))
        }));

        let _ = app.update(Message::CommandOptionPick("Y".to_string()));
        assert!(app.pending_graphic_fill_close.is_none());
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

        let _ = app.on_line_color(acadrust::types::Color::Index(2));
        let _ = app.on_line_linetype("Continuous".to_string());
        let _ = app.on_line_lineweight(acadrust::types::LineWeight::Value(35));
        let _ = app.on_line_linetype_scale(2.5);
        let _ = app.on_line_transparency(acadrust::types::Transparency::from_percent(0.3));

        let header = &app.tabs[i].scene.document.header;
        assert_eq!(
            header.current_entity_color,
            acadrust::types::Color::Index(2)
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
            layer.color = acadrust::types::Color::Index(3);
            layer.line_type = "Continuous".to_string();
            layer.line_weight = acadrust::types::LineWeight::Value(50);
            layer.transparency = acadrust::types::Transparency::from_percent(0.25);
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
        assert_eq!(common.color, acadrust::types::Color::ByBlock);
        assert_eq!(common.linetype, "ByBlock");
        assert_eq!(common.line_weight, acadrust::types::LineWeight::ByBlock);
        assert!(common.transparency.is_by_block());

        let _ = app.on_graphic_attributes_set_all(true);
        let _ = app.on_graphic_attributes_remove_references();
        let common = app.tabs[i]
            .scene
            .document
            .get_entity(handle)
            .expect("line")
            .common();
        assert_eq!(common.color, acadrust::types::Color::Index(3));
        assert_eq!(common.linetype, "Continuous");
        assert_eq!(common.line_weight, acadrust::types::LineWeight::Value(50));
        assert!((common.transparency.as_percent() - 0.25).abs() < 0.01);
    }

    #[test]
    fn graphic_attributes_create_layer_prompt_keeps_spaces_and_active_settings() {
        let mut app = fresh();
        let i = app.active_tab;
        app.tabs[i].scene.deselect_all();
        let _ = app.on_line_color(acadrust::types::Color::Index(2));
        let _ = app.on_line_linetype("Continuous".to_string());
        let _ = app.on_line_lineweight(acadrust::types::LineWeight::Value(35));
        let _ = app.on_line_transparency(acadrust::types::Transparency::from_percent(0.3));

        let _ = app.create_graphic_attributes_layer("Invalid\nLayer");
        assert!(app.tabs[i]
            .scene
            .document
            .layers
            .get("Invalid\nLayer")
            .is_none());

        let _ = app.update(Message::GraphicAttributesCreateLayer);
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
        assert_eq!(layer.color, acadrust::types::Color::Index(2));
        assert_eq!(layer.line_type, "Continuous");
        assert_eq!(layer.line_weight, acadrust::types::LineWeight::Value(35));
        assert!((layer.transparency.as_percent() - 0.3).abs() < 0.01);
    }

    /// A foreign document: the fresh scene's document plus one model-space LINE.
    fn foreign_doc(app: &OpenCADStudio) -> acadrust::CadDocument {
        let mut doc = app.tabs[app.active_tab].scene.document.clone();
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .expect("fresh document has a model-space block record");
        let mut line = Line::new();
        line.common.owner_handle = model_br;
        line.start = Vector3::new(0.0, 0.0, 0.0);
        line.end = Vector3::new(100.0, 50.0, 0.0);
        doc.add_entity(EntityType::Line(line)).unwrap();
        doc
    }

    #[test]
    fn import_document_as_block_defines_block() {
        let mut app = fresh();
        let doc = foreign_doc(&app);
        let name = app
            .import_document_as_block(doc, "Fixture".to_string())
            .unwrap();
        assert_eq!(name, "Fixture");
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .block_records
            .get("Fixture")
            .is_some());
    }

    #[test]
    fn import_document_as_block_merges_source_only_layer() {
        use acadrust::tables::Layer;

        let mut app = fresh();
        let mut doc = foreign_doc(&app);
        let mut layer = Layer::new("Imported Fixtures");
        layer.handle = doc.allocate_handle();
        doc.layers.add(layer).unwrap();
        let model = doc
            .objects
            .values()
            .find_map(|o| match o {
                acadrust::objects::ObjectType::Layout(l) if l.name == "Model" => {
                    Some(l.block_record)
                }
                _ => None,
            })
            .unwrap();
        let line = doc
            .entities_mut()
            .find(|entity| entity.common().owner_handle == model)
            .unwrap();
        line.common_mut().layer = "Imported Fixtures".to_string();

        app.import_document_as_block(doc, "Fixture".to_string())
            .unwrap();
        assert!(app.tabs[app.active_tab]
            .scene
            .document
            .layers
            .contains("Imported Fixtures"));
    }

    /// A foreign document whose model space INSERTs a `Fixture` block, whose
    /// definition itself INSERTs a `Door` block — so the imported `Door` is a
    /// *nested dependency*, not a top-level entity. The `Door` in this file is
    /// unrelated to any `Door` in the destination drawing.
    fn nested_foreign_doc(app: &OpenCADStudio) -> acadrust::CadDocument {
        use acadrust::entities::Insert;
        use acadrust::tables::BlockRecord;
        use acadrust::Handle;
        let mut doc = foreign_doc(app);

        // "Door" block definition: one LINE of geometry.
        let door_h = Handle::new(doc.next_handle());
        let mut door_br = BlockRecord::new("Door");
        door_br.handle = door_h;
        doc.block_records.add(door_br).unwrap();
        let mut door_line = Line::new();
        door_line.start = Vector3::new(0.0, 0.0, 0.0);
        door_line.end = Vector3::new(5.0, 0.0, 0.0);
        door_line.common.owner_handle = door_h;
        doc.add_entity(EntityType::Line(door_line)).unwrap();

        // "Fixture" block definition: INSERTs the nested "Door".
        let fixture_h = Handle::new(doc.next_handle());
        let mut fixture_br = BlockRecord::new("Fixture");
        fixture_br.handle = fixture_h;
        doc.block_records.add(fixture_br).unwrap();
        let mut nested = Insert::new("Door", Vector3::new(2.0, 0.0, 0.0));
        nested.common.owner_handle = fixture_h;
        doc.add_entity(EntityType::Insert(nested)).unwrap();

        // Model space references the fixture so the import captures it.
        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .expect("fresh document has a model-space block record");
        let mut top = Insert::new("Fixture", Vector3::new(0.0, 0.0, 0.0));
        top.common.owner_handle = model_br;
        doc.add_entity(EntityType::Insert(top)).unwrap();

        doc
    }

    /// The nested-INSERT's block name inside a defined block record.
    fn nested_insert_target(doc: &acadrust::CadDocument, block: &str) -> Option<String> {
        let br = doc.block_records.get(block)?;
        br.entity_handles
            .iter()
            .find_map(|h| match doc.get_entity(*h)? {
            EntityType::Insert(ins) => Some(ins.block_name.clone()),
            _ => None,
        })
    }

    /// A foreign document whose model space INSERTs a `Fixture` block whose
    /// definition INSERTs `Door (2)`, whose definition in turn INSERTs `Door`.
    /// The file therefore carries *both* `Door` and `Door (2)` as nested deps.
    fn doubly_nested_foreign_doc(app: &OpenCADStudio) -> acadrust::CadDocument {
        use acadrust::entities::Insert;
        use acadrust::tables::BlockRecord;
        use acadrust::Handle;
        let mut doc = foreign_doc(app);

        let door_h = Handle::new(doc.next_handle());
        let mut door_br = BlockRecord::new("Door");
        door_br.handle = door_h;
        doc.block_records.add(door_br).unwrap();
        let mut door_line = Line::new();
        door_line.start = Vector3::new(0.0, 0.0, 0.0);
        door_line.end = Vector3::new(5.0, 0.0, 0.0);
        door_line.common.owner_handle = door_h;
        doc.add_entity(EntityType::Line(door_line)).unwrap();

        let door2_h = Handle::new(doc.next_handle());
        let mut door2_br = BlockRecord::new("Door (2)");
        door2_br.handle = door2_h;
        doc.block_records.add(door2_br).unwrap();
        let mut mid = Insert::new("Door", Vector3::ZERO);
        mid.common.owner_handle = door2_h;
        doc.add_entity(EntityType::Insert(mid)).unwrap();

        let fixture_h = Handle::new(doc.next_handle());
        let mut fixture_br = BlockRecord::new("Fixture");
        fixture_br.handle = fixture_h;
        doc.block_records.add(fixture_br).unwrap();
        let mut nested = Insert::new("Door (2)", Vector3::new(2.0, 0.0, 0.0));
        nested.common.owner_handle = fixture_h;
        doc.add_entity(EntityType::Insert(nested)).unwrap();

        let model_br = doc
            .objects
            .values()
            .find_map(|o| {
                if let acadrust::objects::ObjectType::Layout(l) = o {
                    (l.name == "Model" && !l.block_record.is_null()).then_some(l.block_record)
                } else {
                    None
                }
            })
            .or_else(|| doc.block_records.get("*Model_Space").map(|br| br.handle))
            .expect("fresh document has a model-space block record");
        let mut top = Insert::new("Fixture", Vector3::new(0.0, 0.0, 0.0));
        top.common.owner_handle = model_br;
        doc.add_entity(EntityType::Insert(top)).unwrap();

        doc
    }

    #[test]
    fn import_reserves_source_names_so_nested_deps_cannot_collide() {
        let mut app = fresh();
        let i = app.active_tab;
        // Source file itself carries "Door" and "Door (2)"; destination already
        // has "Door". The imported "Door" must land on "Door (3)" — NOT steal
        // "Door (2)", which the source file reserves for its own definition.
        let doc = doubly_nested_foreign_doc(&app);
        let mut dest_door = Line::new();
        dest_door.start = Vector3::new(0.0, 0.0, 0.0);
        dest_door.end = Vector3::new(3.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![EntityType::Line(dest_door)],
                "Door",
                glam::DVec3::ZERO,
            )
            .unwrap();

        let _ = app
            .import_document_as_block(doc, "Imported".to_string())
            .unwrap();
        let doc = &app.tabs[i].scene.document;
        // The file's own "Door (2)" is kept intact and targets the file's
        // renamed "Door" (now "Door (3)" — "Door (2)" was taken by the source).
        assert_eq!(
            nested_insert_target(doc, "Door (2)"),
            Some("Door (3)".to_string())
        );
        // The file's plain "Door" got bumped to "Door (3)" so both stay distinct.
        assert!(
            doc.block_records.get("Door").is_some(),
            "destination Door preserved"
        );
        assert!(
            doc.block_records.get("Door (2)").is_some(),
            "source Door (2) preserved under its own name"
        );
        assert!(
            doc.block_records.get("Door (3)").is_some(),
            "source Door renamed to Door (3), not Door (2)"
        );
        assert_eq!(
            nested_insert_target(doc, "Fixture"),
            Some("Door (2)".to_string())
        );
    }

    #[test]
    fn import_nested_block_collision_preserves_both_definitions() {
        let mut app = fresh();
        let i = app.active_tab;
        // Build the foreign document from the pristine scene first, so its
        // nested "Door" is genuinely distinct from the destination's.
        let doc = nested_foreign_doc(&app);
        // Destination drawing already has its own, unrelated "Door" block.
        let mut dest_door = Line::new();
        dest_door.start = Vector3::new(0.0, 0.0, 0.0);
        dest_door.end = Vector3::new(3.0, 3.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![EntityType::Line(dest_door)],
                "Door",
                glam::DVec3::ZERO,
            )
            .unwrap();

        let name = app
            .import_document_as_block(doc, "Imported".to_string())
            .unwrap();
        assert_eq!(name, "Imported");

        let doc = &app.tabs[i].scene.document;
        // Both definitions survive: the destination's original and the imported one.
        assert!(
            doc.block_records.get("Door").is_some(),
            "destination's own Door must be preserved"
        );
        assert!(
            doc.block_records.get("Door (2)").is_some(),
            "imported nested Door must be renamed to Door (2)"
        );
        // The imported Fixture's nested INSERT must point at the imported Door (2),
        // not silently resolve to the destination's unrelated Door.
        assert_eq!(
            nested_insert_target(doc, "Fixture"),
            Some("Door (2)".to_string()),
            "Fixture must reference the renamed imported Door, not the destination's"
        );
        assert_eq!(
            nested_insert_target(doc, "Door (2)"),
            None,
            "imported Door (2) has no nested INSERTs"
        );
    }

    #[test]
    fn blockpalette_refresh_lists_and_places_block() {
        let mut app = fresh();
        let doc = foreign_doc(&app);
        let name = app
            .import_document_as_block(doc, "Fixture".to_string())
            .unwrap();
        app.refresh_block_palette();
        assert!(app.block_palette.blocks.iter().any(|b| b.name == "Fixture"));
        app.start_block_placement(&name);
        let cmd = app.tabs[app.active_tab]
            .active_cmd
            .as_ref()
            .expect("INSERT running");
        assert_eq!(cmd.name(), "INSERT");
        assert_eq!(app.block_palette.placing.as_deref(), Some("Fixture"));
    }

    #[test]
    fn blockpalette_reflects_new_block_without_reopen() {
        use acadrust::types::Transform;

        let mut app = fresh();
        let i = app.active_tab;

        // Reproduce the user-facing flow: select entities and run the BLOCK
        // command, which goes through `create_block_from_entities` (not the
        // clipboard / paste-as-block path).
        let mut line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(10.0, 0.0, 0.0);
        let first = app.tabs[i].scene.add_entity(EntityType::Line(line));
        app.tabs[i].scene.select_entity(first, false);
        app.show_block_palette = true;
        let ws = Transform::identity();
        let id = Transform::identity();
        app.tabs[i]
            .scene
            .create_block_from_entities(&[first], "First", &ws, &id)
            .unwrap();
        app.refresh_block_palette_if_stale();
        assert!(app.block_palette.blocks.iter().any(|b| b.name == "First"));

        // Create a second block on the same tab, then re-run the per-update
        // stale check. It must pick up the new block WITHOUT reopening.
        line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(9.0, 0.0, 0.0);
        let second = app.tabs[i].scene.add_entity(EntityType::Line(line));
        app.tabs[i].scene.select_entity(second, false);
        app.tabs[i]
            .scene
            .create_block_from_entities(&[second], "Second", &ws, &id)
            .unwrap();
        app.refresh_block_palette_if_stale();
        assert!(
            app.block_palette.blocks.iter().any(|b| b.name == "Second"),
            "Second must appear without reopening the panel"
        );
    }

    #[test]
    fn blockpalette_pin_toggles_autocollapse_and_close_hides() {
        let mut app = fresh();
        app.show_block_palette = true;
        let id = crate::ui::dock::PanelId::BlockPalette;
        let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(id));
        assert!(app.dock.auto_collapse(id), "pin enables auto-collapse");
        let _ = app.on_dock(crate::ui::dock::DockMsg::AutoCollapseToggle(id));
        assert!(
            !app.dock.auto_collapse(id),
            "second pin disables auto-collapse"
        );
        let _ = app.on_dock(crate::ui::dock::DockMsg::Close(id));
        assert!(!app.show_block_palette, "close dismisses the sidebar");
    }

    #[test]
    fn blockpalette_dock_moves_to_other_side_and_persists() {
        let mut app = fresh();
        // Tests load the user's persisted config; reset the dock to a known
        // state so this stays hermetic.
        app.dock = Default::default();
        let id = crate::ui::dock::PanelId::BlockPalette;
        assert_eq!(
            app.dock.location(id),
            Some((crate::app::config::DockSide::Right, 0))
        );
        let _ = app.on_dock(crate::ui::dock::DockMsg::DockGrab(id));
        app.win_size = (1600.0, 900.0).into();
        let _ = app.on_dock(crate::ui::dock::DockMsg::DragMove(iced::Point::new(
            100.0, 100.0,
        )));
        assert_eq!(
            app.dock_drag_target,
            Some((crate::app::config::DockSide::Left, 0))
        );
        let _ = app.on_dock(crate::ui::dock::DockMsg::DragRelease);
        assert_eq!(
            app.dock.location(id),
            Some((crate::app::config::DockSide::Left, 0))
        );
    }

    #[test]
    fn dock_visible_len_counts_only_rendered_panels() {
        let mut app = fresh();
        app.dock = Default::default();
        app.dock.left = vec![
            crate::ui::dock::PanelId::BlockPalette,
            crate::ui::dock::PanelId::Properties,
        ];
        // Left: block palette hidden, properties shown -> 1 visible panel.
        app.show_block_palette = false;
        app.show_properties = true;
        assert_eq!(app.dock_visible_len(crate::app::config::DockSide::Left), 1);
        // Reveal the block palette -> both count.
        app.show_block_palette = true;
        assert_eq!(app.dock_visible_len(crate::app::config::DockSide::Left), 2);
        // A hidden (closed) panel counts for nothing even when stacked.
        app.show_block_palette = false;
        assert_eq!(app.dock_visible_len(crate::app::config::DockSide::Left), 1);
    }

    #[test]
    fn dock_drag_target_ignores_hidden_panels_on_the_side() {
        // Reproduce the persisted layout that exposed a half-height ghost: two
        // panels live in the left stack but the block palette is hidden
        // (show_block_palette=false), so only Properties renders. Drag geometry
        // must count only panels that are actually visible.
        let mut app = fresh();
        app.dock = Default::default();
        app.dock.left = vec![
            crate::ui::dock::PanelId::BlockPalette,
            crate::ui::dock::PanelId::Properties,
        ];
        app.show_block_palette = false;
        app.show_properties = true;
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().vp_size = (1600.0, 900.0);
        app.win_size = (1600.0, 900.0).into();
        let id = crate::ui::dock::PanelId::Properties;
        let _ = app.on_dock(crate::ui::dock::DockMsg::DockGrab(id));
        // Pointer near the bottom of the left edge: one visible panel means a
        // single slot, so every y maps to index 0 (no top/bottom split).
        let _ = app.on_dock(crate::ui::dock::DockMsg::DragMove(iced::Point::new(
            100.0, 850.0,
        )));
        assert_eq!(
            app.dock_drag_target,
            Some((crate::app::config::DockSide::Left, 0))
        );
    }

    #[test]
    fn dock_drag_target_counts_all_visible_panels() {
        let mut app = fresh();
        app.dock = Default::default();
        app.dock.left = vec![
            crate::ui::dock::PanelId::BlockPalette,
            crate::ui::dock::PanelId::Properties,
        ];
        // Both panels shown: two real slots on the left edge. A pointer near
        // the bottom maps to the append slot (index == 2).
        app.show_block_palette = true;
        app.show_properties = true;
        let i = app.active_tab;
        app.tabs[i].scene.selection.borrow_mut().vp_size = (1600.0, 900.0);
        app.win_size = (1600.0, 900.0).into();
        let id = crate::ui::dock::PanelId::Properties;
        let _ = app.on_dock(crate::ui::dock::DockMsg::DockGrab(id));
        let _ = app.on_dock(crate::ui::dock::DockMsg::DragMove(iced::Point::new(
            100.0, 850.0,
        )));
        assert_eq!(
            app.dock_drag_target,
            Some((crate::app::config::DockSide::Left, 2))
        );
    }

    #[test]
    fn blockpalette_width_reset() {
        let mut app = fresh();
        let id = crate::ui::dock::PanelId::BlockPalette;
        app.dock.set_width(id, 500.0);
        let _ = app.on_dock(crate::ui::dock::DockMsg::WidthReset(id));
        assert_eq!(app.dock.settings(id).width, 260.0);
    }

    #[test]
    fn block_name_from_file_avoids_collisions() {
        let mut app = fresh();
        let i = app.active_tab;
        let mut line = Line::new();
        line.start = Vector3::ZERO;
        line.end = Vector3::new(1.0, 0.0, 0.0);
        app.tabs[i]
            .scene
            .define_block_from_owned_entities(
                vec![EntityType::Line(line)],
                "Chair",
                glam::DVec3::ZERO,
            )
            .unwrap();
        assert_eq!(app.block_name_from_file("Chair"), "Chair (2)");
        assert_eq!(app.block_name_from_file("Table"), "Table");
    }

    #[test]
    fn save_as_rebases_live_doc_paths() {
        // F4: on Save-As completion the live document's relative reference
        // paths are rebased onto the new base dir (same helper the save
        // snapshot used), so the session agrees with the file just written.
        let mut app = fresh();
        let i = app.active_tab;
        let dir = std::env::temp_dir().join(format!(
            "ocs_saveas_rebase_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let old_dir = dir.join("old");
        let new_dir = dir.join("new");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();
        let mut br = acadrust::tables::BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = "refs/plan.dwg".to_string();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(old_dir.join("host.dwg"));
        let tab_id = app.tabs[i].id;
        let job_id = 4242u64;
        app.active_save_jobs.insert(tab_id, job_id);
        let outcome = crate::app::SaveOutcome {
            job_id,
            tab_id,
            epoch: app.tabs[i].scene.geometry_epoch,
            revision: app.tabs[i].edit_revision,
            camera_generation: app.tabs[i].scene.camera_generation,
            path: new_dir.join("host.dwg"),
            version: app.tabs[i].scene.document.version,
            previous_autosave: None,
            set_current_path: true,
            purpose: crate::app::SavePurpose::SaveAs,
            continuation: crate::app::SaveContinuation::None,
            refreshed_preview: None,
            result: Ok(()),
        };
        let _ = app.on_save_finished(outcome);
        assert_eq!(
            app.tabs[i].current_path.as_deref(),
            Some(new_dir.join("host.dwg").as_path())
        );
        let br = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap();
        assert_eq!(br.xref_path, "../old/refs/plan.dwg");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn palette_tmpdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ocs_palette_{tag}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn palette_output(app: &OpenCADStudio, start: usize) -> String {
        app.command_line.history[start..]
            .iter()
            .map(|e| e.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn palette_rowop_nested_reports_per_entry() {
        // Behavioral matrix: every row op on a NESTED row via the GUI
        // message path. Nested rows must report per-entry errors (CLI
        // wording), never silently skip.
        use crate::app::Message;
        use crate::ui::window::xref_manager::XrefPaletteOp;
        use acadrust::tables::BlockRecord;
        let dir = palette_tmpdir("nestedmatrix");
        let mut host_doc = acadrust::CadDocument::new();
        let mut inner = BlockRecord::new("INNER");
        inner.flags.is_xref = true;
        inner.xref_path = "inner.dwg".to_string();
        host_doc.block_records.add(inner).unwrap();
        let bytes = crate::io::save_to_bytes(&host_doc, "dwg", host_doc.version).unwrap();
        std::fs::write(dir.join("host.dwg"), &bytes).unwrap();
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("HOST");
        br.flags.is_xref = true;
        br.xref_path = dir.join("host.dwg").to_string_lossy().into_owned();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        app.refresh_xref_manager();
        let idx = app
            .xref_manager
            .entries
            .iter()
            .position(|e| e.name == "INNER")
            .expect("nested INNER listed");
        assert!(app.xref_manager.entries[idx].parent_key.is_some());
        for op in [
            XrefPaletteOp::Detach,
            XrefPaletteOp::Unload,
            XrefPaletteOp::Reload,
            XrefPaletteOp::Bind,
            XrefPaletteOp::Overlay,
        ] {
            let start = app.command_line.history.len();
            let _ = app.update(Message::XrefRowOp(idx, op));
            let out = palette_output(&app, start);
            assert!(
                out.contains("INNER"),
                "op {op:?} on nested row gave: {out:?}"
            );
            assert!(
                app.tabs[i]
                    .scene
                    .document
                    .block_records
                    .get("HOST")
                    .is_some(),
                "op {op:?} must not mutate the direct reference"
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn palette_detach_unload_rowop_gui_path() {
        // Reproduction for "Detach/Unload from the palette do nothing":
        // drive the exact GUI message path (row right-click menu item).
        use crate::app::Message;
        use crate::ui::window::xref_manager::XrefPaletteOp;
        use acadrust::tables::BlockRecord;
        let mut app = fresh();
        let i = app.active_tab;
        for name in ["PLAN", "SITE"] {
            let mut br = BlockRecord::new(name);
            br.flags.is_xref = true;
            br.xref_path = format!("old/{}.dwg", name.to_lowercase());
            br.handle = app.tabs[i].scene.document.allocate_handle();
            app.tabs[i].scene.document.block_records.add(br).unwrap();
        }
        app.tabs[i].current_path = Some(std::env::temp_dir().join("ocs_repro_host.dwg"));
        app.refresh_xref_manager();
        let idx = app
            .xref_manager
            .entries
            .iter()
            .position(|e| e.name == "PLAN")
            .expect("PLAN listed");
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(idx, XrefPaletteOp::Detach));
        let out = palette_output(&app, start);
        assert!(out.contains("PLAN"), "detach output missing, got: {out:?}");
        assert!(
            app.tabs[i]
                .scene
                .document
                .block_records
                .get("PLAN")
                .is_none(),
            "PLAN definition must be gone"
        );
        let idx = app
            .xref_manager
            .entries
            .iter()
            .position(|e| e.name == "SITE")
            .expect("SITE listed");
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(idx, XrefPaletteOp::Unload));
        let out = palette_output(&app, start);
        assert!(out.contains("SITE"), "unload output missing, got: {out:?}");
        assert_eq!(
            app.xref_manager
                .entries
                .iter()
                .find(|e| e.name == "SITE")
                .unwrap()
                .status,
            crate::io::xref_model::RefStatus::Unloaded
        );
    }

    #[test]
    fn palette_overlay_and_pathtype_rowop_gui_path() {
        // Row-menu Overlay + Change-Path row ops on a direct row via the
        // GUI message path: type flag flips, saved path clears.
        use crate::app::Message;
        use crate::io::xref_model::{Pathtype, RefType};
        use crate::ui::window::xref_manager::XrefPaletteOp;
        use acadrust::tables::BlockRecord;
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = "old/plan.dwg".to_string();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(std::env::temp_dir().join("ocs_rowop_host.dwg"));
        app.refresh_xref_manager();
        let idx = app
            .xref_manager
            .entries
            .iter()
            .position(|e| e.name == "PLAN")
            .expect("PLAN listed");
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(idx, XrefPaletteOp::Overlay));
        let out = palette_output(&app, start);
        assert!(out.contains("PLAN"), "got: {out:?}");
        let br = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap();
        assert!(br.flags.is_xref_overlay && !br.flags.is_xref);
        assert_eq!(
            app.xref_manager
                .entries
                .iter()
                .find(|e| e.name == "PLAN")
                .unwrap()
                .ref_type,
            RefType::Overlay
        );
        let start = app.command_line.history.len();
        let _ = app.update(Message::XrefRowOp(
            idx,
            XrefPaletteOp::Pathtype(Pathtype::None),
        ));
        let out = palette_output(&app, start);
        assert!(out.contains("PLAN"), "got: {out:?}");
        let br = app.tabs[i]
            .scene
            .document
            .block_records
            .get("PLAN")
            .unwrap();
        assert_eq!(
            br.xref_path, "plan.dwg",
            "Remove Path strips to the bare filename"
        );
    }

    #[test]
    fn palette_reload_all_resolves_direct_refs() {
        // Toolbar Reload All against a real on-disk reference: full
        // resolve, per-ref report, entry back to Loaded.
        use acadrust::tables::BlockRecord;
        let dir = palette_tmpdir("reloadall");
        let ref_doc = acadrust::CadDocument::new();
        let bytes = crate::io::save_to_bytes(&ref_doc, "dwg", ref_doc.version).unwrap();
        std::fs::write(dir.join("plan.dwg"), &bytes).unwrap();
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("PLAN");
        br.flags.is_xref = true;
        br.xref_path = dir.join("plan.dwg").to_string_lossy().into_owned();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        let start = app.command_line.history.len();
        app.xref_manager_reload_all();
        let out = palette_output(&app, start);
        assert!(
            out.contains("PLAN"),
            "reload-all must report the reference, got: {out:?}"
        );
        let entry = app
            .xref_manager
            .entries
            .iter()
            .find(|e| e.name == "PLAN")
            .expect("PLAN listed");
        assert_eq!(
            entry.status,
            crate::io::xref_model::RefStatus::Loaded,
            "PLAN must resolve Loaded"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn find_replace_prompt_prefills_command_line() {
        // Toolbar Find and Replace hands the existing `XREF Path Find`
        // parsing a prefilled command line.
        use crate::app::Message;
        let mut app = fresh();
        let _ = app.update(Message::XrefFindReplacePrompt);
        assert_eq!(app.command_line.input, "XREF Path Find ");
    }

    #[test]
    fn palette_unload_nested_reports_per_entry() {
        // F6: palette Unload on a nested row reports the per-entry nested
        // error (CLI wording) instead of the confusing 'no loaded reference'.
        use crate::ui::window::xref_manager::XrefPaletteOp;
        use acadrust::tables::BlockRecord;
        let dir = palette_tmpdir("nested");
        let mut host_doc = acadrust::CadDocument::new();
        let mut inner = BlockRecord::new("INNER");
        inner.flags.is_xref = true;
        inner.xref_path = "inner.dwg".to_string();
        host_doc.block_records.add(inner).unwrap();
        let bytes = crate::io::save_to_bytes(&host_doc, "dwg", host_doc.version).unwrap();
        std::fs::write(dir.join("host.dwg"), &bytes).unwrap();
        let mut app = fresh();
        let i = app.active_tab;
        let mut br = BlockRecord::new("HOST");
        br.flags.is_xref = true;
        br.xref_path = dir.join("host.dwg").to_string_lossy().into_owned();
        br.handle = app.tabs[i].scene.document.allocate_handle();
        app.tabs[i].scene.document.block_records.add(br).unwrap();
        app.tabs[i].current_path = Some(dir.join("app.dwg"));
        app.refresh_xref_manager();
        let idx = app
            .xref_manager
            .entries
            .iter()
            .position(|e| e.name == "INNER")
            .expect("nested INNER listed");
        assert!(app.xref_manager.entries[idx].parent_key.is_some());
        app.xref_manager.selected.insert(idx);
        let start = app.command_line.history.len();
        app.xref_manager_op(XrefPaletteOp::Unload);
        let out = palette_output(&app, start);
        assert!(out.contains("INNER"), "got: {out:?}");
        assert_eq!(app.command_line.history.len(), start + 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn palette_reload_skips_images_without_clearing_flags() {
        // F7: palette Reload on an image/PDF row reports the drawing-only
        // error and leaves its unloaded flag untouched (no spurious
        // no-match after a flag clear).
        use crate::io::xref_model::RefKind;
        use crate::ui::window::xref_manager::XrefPaletteOp;
        use acadrust::objects::{ImageDefinition, ObjectType};
        let dir = palette_tmpdir("imgreload");
        let mut app = fresh();
        let i = app.active_tab;
        let h = app.tabs[i].scene.document.allocate_handle();
        let mut def = ImageDefinition::with_dimensions("img.png", 8, 8);
        def.handle = h;
        app.tabs[i]
            .scene
            .document
            .objects
            .insert(h, ObjectType::ImageDefinition(def));
        let mut img = acadrust::entities::RasterImage::new(
            "img.png",
            acadrust::types::Vector3::ZERO,
            8.0,
            8.0,
        );
        img.definition_handle = Some(h);
        app.tabs[i]
            .scene
            .document
            .add_entity(acadrust::EntityType::RasterImage(img))
            .unwrap();
        app.tabs[i].current_path = Some(dir.join("host.dwg"));
        app.refresh_xref_manager();
        let idx = app
            .xref_manager
            .entries
            .iter()
            .position(|e| e.kind == RefKind::Image)
            .expect("image listed");
        let key = app.xref_manager.entries[idx].key;
        app.xref_manager.selected.insert(idx);
        app.tabs[i].xref_unloaded.add(key);
        let start = app.command_line.history.len();
        app.xref_manager_op(XrefPaletteOp::Reload);
        let out = palette_output(&app, start);
        assert!(out.contains("img.png"), "got: {out:?}");
        assert_eq!(app.command_line.history.len(), start + 1);
        assert!(
            app.tabs[i].xref_unloaded.is_unloaded(key),
            "image flag must stay untouched"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
