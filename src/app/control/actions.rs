use super::*;
use crate::scene::model::object::PropValue;

fn control_color(value: &Value) -> Result<acadrust::types::Color, Value> {
    if let Some(index) = value.as_i64().and_then(|v| i16::try_from(v).ok()) {
        return Ok(acadrust::types::Color::from_index(index));
    }
    if let Some(rgb) = value
        .get("rgb")
        .and_then(Value::as_array)
        .filter(|v| v.len() == 3)
    {
        let component = |i: usize| rgb[i].as_u64().and_then(|v| u8::try_from(v).ok());
        return match (component(0), component(1), component(2)) {
            (Some(r), Some(g), Some(b)) => Ok(acadrust::types::Color::from_rgb(r, g, b)),
            _ => Err(failure(
                "invalid_color",
                "RGB components must be 0 through 255",
            )),
        };
    }
    Err(failure(
        "invalid_color",
        "Use an index number or {rgb:[r,g,b]}",
    ))
}

fn control_lineweight(value: &Value) -> Result<acadrust::types::LineWeight, Value> {
    let raw = value
        .as_i64()
        .and_then(|v| i16::try_from(v).ok())
        .ok_or_else(|| failure("invalid_lineweight", "Use the raw lineweight value"))?;
    let candidate = acadrust::types::LineWeight::from_value(raw);
    crate::ui::properties::lw_options()
        .iter()
        .any(|v| v.0 == candidate)
        .then_some(candidate)
        .ok_or_else(|| failure("invalid_lineweight", "Value is not a standard lineweight"))
}

fn color_value(color: acadrust::types::Color) -> Value {
    if let Some((r, g, b)) = color.rgb() {
        json!({"rgb":[r,g,b]})
    } else {
        json!(color.index())
    }
}
pub(super) const NAMES: &[&str] = &[
    "close_modal",
    "close_document",
    "toggle_properties",
    "toggle_layers",
    "toggle_grid",
    "toggle_snap",
    "toggle_ortho",
    "mtext_insert",
    "mtext_commit",
    "mtext_cancel",
    "text_input",
    "text_commit",
    "layer_visible",
    "layer_locked",
    "layer_frozen",
    "layer_current",
    "view_home",
    "zoom_extents",
    "undo",
    "redo",
];
impl OpenCADStudio {
    pub(super) fn control_properties(&mut self) -> Value {
        self.refresh_properties();
        json!({"ok":true,"sections":self.tabs[self.active_tab].properties.sections.iter().map(|s|json!({"title":s.title,"properties":s.props.iter().map(|p|{
            let (kind,value,options)=match &p.value{
                PropValue::ReadOnly(v)|PropValue::ReadOnlyWithTooltip{value:v,..}=>("readonly",json!(v),Value::Null),
                PropValue::EditText(v)=>("number",json!(v),Value::Null),
                PropValue::PlainText(v)=>("text",json!(v),Value::Null),
                PropValue::Choice{selected,options}=>("choice",json!(selected),json!(options)),
                PropValue::EditChoice{value,options}=>("editable_choice",json!(value),json!(options)),
                PropValue::LayerChoice(v)=>("layer",json!(v),Value::Null),
                PropValue::LinetypeChoice(v)=>("linetype",json!(v),Value::Null),
                PropValue::BoolToggle{value,..}=>("bool",json!(value),Value::Null),
                PropValue::ColorChoice(value)|PropValue::NamedColorChoice{color:value,..}=>("color",color_value(*value),Value::Null),
                PropValue::ColorVaries=>("color",Value::Null,Value::Null),
                PropValue::LwChoice(value)|PropValue::FieldLwChoice{value,..}=>("lineweight",json!(value.value()),json!(crate::ui::properties::lw_options().iter().map(|v|v.0.value()).collect::<Vec<_>>())),
                PropValue::LwVaries|PropValue::FieldLwVaries{..}=>("lineweight",Value::Null,json!(crate::ui::properties::lw_options().iter().map(|v|v.0.value()).collect::<Vec<_>>())),
                PropValue::AttrText{tag,value}=>("attribute",json!({"tag":tag,"value":value}),Value::Null),
                other=>("specialized",json!(format!("{other:?}")),Value::Null),
            };json!({"id":p.field,"label":p.label,"kind":kind,"value":value,"options":options})
        }).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
    pub(super) fn control_set_property(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        self.refresh_properties();
        let field = string(req, "field")?;
        let p = self.tabs[self.active_tab]
            .properties
            .sections
            .iter()
            .flat_map(|s| &s.props)
            .find(|p| p.field == field)
            .cloned()
            .ok_or_else(|| {
                failure(
                    "unknown_property",
                    "Read properties for the current selection",
                )
            })?;
        let value = req["value"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| req["value"].to_string());
        Ok(match p.value {
            PropValue::EditText(_) | PropValue::PlainText(_) | PropValue::EditChoice { .. } => {
                let input = self.update(Message::PropGeomInput {
                    field: p.field,
                    value,
                });
                let commit = self.update(Message::PropGeomCommit(p.field));
                Task::batch([input, commit])
            }
            PropValue::Choice { options, .. } => {
                if !options.contains(&value) {
                    return Err(failure(
                        "invalid_choice",
                        "Value is not an available option",
                    ));
                }
                self.update(Message::PropGeomChoiceChanged {
                    field: p.field,
                    value,
                })
            }
            PropValue::LayerChoice(_) => self.update(Message::PropLayerChanged(value)),
            PropValue::LinetypeChoice(_) => self.update(Message::PropLinetypeChanged(value)),
            PropValue::HatchPatternChoice(_) => {
                self.update(Message::PropHatchPatternChanged(value))
            }
            PropValue::BoolToggle { field, value: old } => {
                let v = req["value"]
                    .as_bool()
                    .ok_or_else(|| failure("invalid_value", "Expected boolean"))?;
                if old != v {
                    self.update(Message::PropBoolToggle(field))
                } else {
                    Task::none()
                }
            }
            PropValue::AttrText { tag, .. } => {
                let input = self.update(Message::PropAttrInput {
                    tag: tag.clone(),
                    value,
                });
                let commit = self.update(Message::PropAttrCommit(tag));
                Task::batch([input, commit])
            }
            PropValue::ColorChoice(_)
            | PropValue::NamedColorChoice { .. }
            | PropValue::ColorVaries => {
                let color = control_color(&req["value"])?;
                if p.field == "background_color" {
                    self.update(Message::PropBgColorChanged(color))
                } else if matches!(
                    p.field,
                    "gradient_color_1"
                        | "gradient_color_2"
                        | "dim_line_color"
                        | "dim_ext_line_color"
                        | "dim_text_color"
                        | "dim_text_fill_color"
                        | "line_color"
                        | "text_color"
                        | "block_content_color"
                        | "background_fill_color"
                ) {
                    self.update(Message::PropColorFieldChanged {
                        field: p.field.into(),
                        color,
                    })
                } else {
                    self.update(Message::PropColorChanged(color))
                }
            }
            PropValue::LwChoice(_) | PropValue::LwVaries => {
                self.update(Message::PropLwChanged(control_lineweight(&req["value"])?))
            }
            PropValue::FieldLwChoice { field, .. } | PropValue::FieldLwVaries { field } => self
                .update(Message::PropFieldLwChanged {
                    field,
                    value: control_lineweight(&req["value"])?,
                }),
            PropValue::ReadOnly(_) | PropValue::ReadOnlyWithTooltip { .. } => {
                return Err(failure("readonly_property", "Property cannot be edited"))
            }
            _ => {
                return Err(failure(
                    "unsupported_property",
                    "Use the corresponding command for this property type",
                ))
            }
        })
    }
    pub(super) fn control_ui_action(&mut self, req: &Value) -> Result<Task<Message>, Value> {
        let name = string(req, "name")?;
        let msg = match name {
            "close_modal" => Message::CloseModal,
            "close_document" => Message::TabClose(self.active_tab),
            "toggle_properties" => Message::ToggleProperties,
            "toggle_layers" => Message::ToggleLayers,
            "toggle_grid" => Message::ToggleGrid,
            "toggle_snap" => Message::ToggleSnapEnabled,
            "toggle_ortho" => Message::ToggleOrtho,
            "mtext_insert" => {
                if self.mtext_editor.is_none() {
                    return Err(failure("editor_closed", "MText editor is closed"));
                }
                Message::MTextInsert(string(req, "value")?.into())
            }
            "mtext_commit" => Message::MTextOk,
            "mtext_cancel" => Message::MTextCancel,
            "text_input" => Message::TextInlineInput(string(req, "value")?.into()),
            "text_commit" => Message::TextInlineOk,
            "pointer_move" | "pointer_press" | "pointer_release" => {
                let x = req["x"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| failure("invalid_point", "Missing finite x"))?
                    as f32;
                let y = req["y"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| failure("invalid_point", "Missing finite y"))?
                    as f32;
                let size = self.tabs[self.active_tab].scene.selection.borrow().vp_size;
                if x < 0. || y < 0. || x > size.0 || y > size.1 {
                    return Err(failure(
                        "outside_viewport",
                        "Pointer coordinates are outside viewport_size",
                    ));
                }
                let move_task = self.update(Message::ViewportMove(iced::Point::new(x, y)));
                let event = match name {
                    "pointer_press" => self.update(Message::ViewportLeftPress),
                    "pointer_release" => self.update(Message::ViewportLeftRelease),
                    _ => Task::none(),
                };
                return Ok(Task::batch([move_task, event]));
            }
            "view_home" => Message::ViewCubeHome,
            "zoom_extents" => return Ok(self.dispatch_command("ZOOM EXTENTS")),
            "undo" => Message::Undo,
            "redo" => Message::Redo,
            "layer_visible" | "layer_locked" | "layer_frozen" | "layer_current" => {
                let layer = string(req, "layer")?;
                let index = self.tabs[self.active_tab]
                    .scene
                    .document
                    .layers
                    .iter()
                    .position(|l| l.name == layer)
                    .ok_or_else(|| failure("unknown_layer", "Layer does not exist"))?;
                match name {
                    "layer_visible" => Message::LayerToggleVisible(index),
                    "layer_locked" => Message::LayerToggleLock(index),
                    "layer_frozen" => Message::LayerToggleFreeze(index),
                    _ => {
                        let select = self.update(Message::LayerSelect(index));
                        let current = self.update(Message::LayerSetCurrent);
                        return Ok(Task::batch([select, current]));
                    }
                }
            }
            _ => {
                return Err(failure(
                    "unknown_action",
                    "Read commands.actions for supported action IDs",
                ))
            }
        };
        Ok(self.update(msg))
    }
}
