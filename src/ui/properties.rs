//! Properties panel — OpenCADStudio-style editable object properties.
//!
//! Shows two sections (General + Geometry) for the selected entity.
//! • Layer      → combo_box  (options from document layer table)
//! • Color      → inline color picker  (ByLayer / ByBlock / ACI palette)
//! • Lineweight → combo_box  (standard CAD lineweight list)
//! • Linetype   → read-only for now
//! • Geometry   → text_input per coordinate / dimension field

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::fmt;

use crate::ui::ROW_H;
use acadrust::types::{Color as AcadColor, LineWeight};
use acadrust::Handle;
use iced::widget::{button, column, combo_box, container, row, scrollable, text, text_input};
use iced::{Background, Border, Color, Element, Length, Padding, Theme};

// ── Row-height-derived constants ─────────────────────────────────────────
const FONT_SZ: f32 = ROW_H * 0.42; // ≈11 px
const COMBO_PAD_V: f32 = (ROW_H - FONT_SZ * 1.3 - 2.0) / 2.0; // fills combo to ROW_H
const SWATCH_SZ: f32 = ROW_H * 0.54; // ≈14 px color swatch

use crate::app::Message;
use crate::scene::model::object::{PropSection, PropValue};

const VARIES_LABEL: &str = "*VARIES*";

// ── Linetype item (name + ASCII art for combo_box) ───────────────────────

#[derive(Clone, PartialEq, Debug)]
pub struct LinetypeItem {
    pub name: String,
    pub art: String,
}

impl fmt::Display for LinetypeItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.art.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}  {}", self.name, self.art)
        }
    }
}

// ── Lineweight wrapper (needs ToString for combo_box) ─────────────────────

#[derive(Clone, PartialEq, Debug)]
pub struct LwItem(pub LineWeight);

impl fmt::Display for LwItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            LineWeight::ByLayer => write!(f, "ByLayer"),
            LineWeight::ByBlock => write!(f, "ByBlock"),
            LineWeight::Default => write!(f, "Default"),
            LineWeight::Value(v) => write!(f, "{:.2} mm", v as f64 / 100.0),
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct SelectionGroup {
    pub label: String,
    pub handles: Vec<Handle>,
}

impl fmt::Display for SelectionGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// All standard CAD lineweight options for the combobox.
pub fn lw_options() -> Vec<LwItem> {
    [
        LineWeight::ByLayer,
        LineWeight::ByBlock,
        LineWeight::Default,
        LineWeight::Value(0),
        LineWeight::Value(5),
        LineWeight::Value(9),
        LineWeight::Value(13),
        LineWeight::Value(15),
        LineWeight::Value(18),
        LineWeight::Value(20),
        LineWeight::Value(25),
        LineWeight::Value(30),
        LineWeight::Value(35),
        LineWeight::Value(40),
        LineWeight::Value(50),
        LineWeight::Value(53),
        LineWeight::Value(60),
        LineWeight::Value(70),
        LineWeight::Value(80),
        LineWeight::Value(90),
        LineWeight::Value(100),
        LineWeight::Value(106),
        LineWeight::Value(120),
        LineWeight::Value(140),
        LineWeight::Value(158),
        LineWeight::Value(200),
        LineWeight::Value(211),
    ]
    .iter()
    .copied()
    .map(LwItem)
    .collect()
}

/// Edit-buffer key for a block attribute value, keyed by its tag. Kept in one
/// place so the live-input handler and the row renderer agree. The `\x01`
/// sentinel guarantees no collision with a geometry field's `&'static str` key.
pub fn attr_edit_key(tag: &str) -> String {
    format!("\x01attr\x01{tag}")
}

// ── PropertiesPanel ───────────────────────────────────────────────────────

#[derive(Clone)]
pub struct PropertiesPanel {
    pub sections: Vec<PropSection>,
    pub title: String,
    pub selection_groups: Vec<SelectionGroup>,
    pub selected_group: Option<SelectionGroup>,
    /// Linetype items (name + ASCII art) from the document — used for combo_box options.
    pub linetype_items: Vec<LinetypeItem>,
    pub selection_group_combo: combo_box::State<SelectionGroup>,
    pub choice_combos: HashMap<String, combo_box::State<String>>,
    pub layer_combo: combo_box::State<String>,
    pub lineweight_combo: combo_box::State<LwItem>,
    pub linetype_combo: combo_box::State<LinetypeItem>,
    pub hatch_pattern_combo: combo_box::State<String>,
    /// In-progress text edits keyed by `field` name.
    pub edit_buf: HashMap<String, String>,
    /// Entity handles this panel was built for. `refresh_properties` compares
    /// it against the new selection to decide whether an uncommitted `edit_buf`
    /// may carry over: same selection → keep (survive a commit-triggered
    /// rebuild); different selection → drop, so a stale value can't display or
    /// commit onto a different entity (e.g. two title blocks sharing a `REV1`
    /// tag).
    pub source_handles: Vec<Handle>,
    /// Whether the quick color picker dropdown is open.
    pub color_picker_open: bool,
    /// Whether the full 16×16 ACI palette is expanded inside the color picker.
    pub color_palette_open: bool,
    /// Whether the MTEXT background-colour picker dropdown is open. Separate
    /// from `color_picker_open` so the entity colour and the background colour
    /// pickers are independent.
    pub bg_color_picker_open: bool,
    /// Field name of the generic per-field colour picker currently open (e.g. a
    /// hatch gradient colour), or `None`. Keeps each field's picker independent.
    pub open_color_field: Option<String>,
    /// Which vertex a multi-vertex entity (polyline) is focused on — driven by
    /// the Current Vertex ◀ / ▶ stepper. Reset to 0 when the selection changes.
    pub prop_vertex: usize,
    /// Coordinate groups ("Position", "Scale", …) the user expanded into their
    /// component X/Y/Z rows. Collapsed by default; keyed `section:base` and
    /// carried across panel rebuilds so the state survives edits and selection
    /// changes.
    pub expanded_groups: HashSet<String>,
    /// Whether the editable-dropdown (block Name) option list is open.
    pub edit_choice_open: bool,
}

impl Default for PropertiesPanel {
    fn default() -> Self {
        Self {
            sections: vec![],
            title: String::new(),
            selection_groups: vec![],
            selected_group: None,
            linetype_items: vec![],
            selection_group_combo: combo_box::State::new(vec![]),
            choice_combos: HashMap::default(),
            layer_combo: combo_box::State::new(vec![]),
            lineweight_combo: combo_box::State::new(lw_options()),
            linetype_combo: combo_box::State::new(vec![]),
            hatch_pattern_combo: combo_box::State::new(crate::scene::model::hatch_patterns::names()),
            edit_buf: HashMap::default(),
            source_handles: vec![],
            color_picker_open: false,
            color_palette_open: false,
            bg_color_picker_open: false,
            open_color_field: None,
            prop_vertex: 0,
            expanded_groups: HashSet::default(),
            edit_choice_open: false,
        }
    }
}

impl PropertiesPanel {
    pub fn empty() -> Self {
        Self {
            title: "No Selection".into(),
            ..Default::default()
        }
    }

    pub fn selected_handles(&self) -> Vec<Handle> {
        self.selected_group
            .as_ref()
            .map(|group| group.handles.clone())
            .unwrap_or_default()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // ── Header ──────────────────────────────────────────────────────────
        let header = container(text("Properties").size(12))
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.extended_palette().background.weak.color,
                )),
                ..Default::default()
            })
            .width(Length::Fill)
            .padding([6, 10]);

        // ── Title bar (entity type / "No Selection") ─────────────────────
        let title_content: Element<'_, Message> = if self.selection_groups.is_empty() {
            text(crate::ui::text_util::elide(&self.title, 34))
                .size(FONT_SZ)
                .style(muted_text_style)
                .into()
        } else {
            combo_box(
                &self.selection_group_combo,
                "",
                self.selected_group.as_ref(),
                Message::PropSelectionGroupChanged,
            )
            .size(FONT_SZ)
            .padding([2, 6])
            .input_style(combo_input_style)
            .on_open(Message::PropColorPickerClose)
            .width(Length::Fill)
            .into()
        };

        let title_bar = container(title_content)
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                background: Some(Background::Color(palette.background.weakest.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
                }
            })
            .width(Length::Fill)
            .padding([4, 10]);

        // ── Content ─────────────────────────────────────────────────────────
        let content: Element<'_, Message> = if self.sections.is_empty() {
            container(
                text("Select an object to view properties")
                    .size(10)
                    .style(hint_text_style),
            )
            .padding([10, 10])
            .into()
        } else {
            let mut col = column![].spacing(0);
            for section in &self.sections {
                col = col.push(self.render_section(section));
            }
            scrollable(col).into()
        };

        container(column![header, title_bar, content])
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                background: Some(Background::Color(palette.background.base.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
                }
            })
            .width(250)
            .height(Length::Fill)
            .into()
    }

    /// Compact floating panel for Quick Properties: the title plus the same
    /// editable section rows as the docked panel, sized to its content.
    /// Returns `None` when nothing is selected.
    pub fn quick_view(&self) -> Option<Element<'_, Message>> {
        if self.sections.is_empty() {
            return None;
        }
        let title = container(
            text(crate::ui::text_util::elide(&self.title, 34))
                .size(FONT_SZ)
                .style(muted_text_style),
        )
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                background: Some(Background::Color(palette.background.weakest.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
                }
            })
            .width(Length::Fill)
            .padding([4, 10]);

        let mut col = column![title].spacing(0);
        for section in &self.sections {
            col = col.push(self.render_section(section));
        }

        Some(
            container(col)
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.base.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 3.0.into(),
                    },
                    ..Default::default()
                    }
                })
                .width(230)
                .into(),
        )
    }

    // ── Section renderer ──────────────────────────────────────────────────

    fn render_section<'a>(&'a self, section: &'a PropSection) -> Element<'a, Message> {
        // Section header
        let hdr = container(text(&section.title).size(10))
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                background: Some(Background::Color(palette.background.weak.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
                }
            })
            .width(Length::Fill)
            .padding([3, 8]);

        let mut col = column![hdr].spacing(0);

        // Consecutive "<Base> X / <Base> Y [/ <Base> Z]" text rows collapse
        // into one clickable summary row; clicking expands the components.
        let mut idx = 0;
        while idx < section.props.len() {
            let group_len = if section.title == "View" {
                0
            } else {
                coord_group_len(&section.props, idx)
            };
            if group_len >= 2 {
                let base = coord_base(&section.props[idx].label);
                let key = format!("{}:{}", section.title, base);
                let expanded = self.expanded_groups.contains(&key);
                let joined = section.props[idx..idx + group_len]
                    .iter()
                    .map(prop_text_value)
                    .collect::<Vec<_>>()
                    .join(", ");
                col = col.push(render_group_row(base, key, expanded, joined));
                if expanded {
                    for prop in &section.props[idx..idx + group_len] {
                        col = col.push(self.render_prop_row(prop, coord_component(&prop.label)));
                    }
                }
                idx += group_len;
            } else {
                let prop = &section.props[idx];
                col = col.push(self.render_prop_row(prop, &prop.label));
                idx += 1;
            }
        }

        col.into()
    }

    /// Render one property row with an explicit display label (the grouped
    /// coordinate rows shorten "Position X" to "X").
    fn render_prop_row<'a>(
        &'a self,
        prop: &'a crate::scene::model::object::Property,
        label: &'a str,
    ) -> Element<'a, Message> {
        match &prop.value {
            PropValue::ColorChoice(color) => self.render_color_row(label, prop.field, *color),
            PropValue::ColorVaries => self.render_color_varies_row(label),
            PropValue::LayerChoice(layer) => self.render_layer_row(label, layer),
            PropValue::LwChoice(lw) => self.render_lw_row(label, *lw),
            PropValue::LwVaries => self.render_lw_varies_row(label),
            PropValue::LinetypeChoice(lt) => self.render_linetype_row(label, lt),
            PropValue::Choice { selected, options } => {
                self.render_choice_row(label, prop.field, selected, options)
            }
            PropValue::EditChoice { value, options } => {
                self.render_edit_choice_row(label, prop.field, value, options)
            }
            PropValue::BoolToggle { field, value } => render_bool_row(label, *field, *value),
            PropValue::Stepper { display, .. } => render_stepper_row(label, display),
            PropValue::EditText(val) => self.render_edit_row(label, prop.field, val),
            PropValue::ReadOnly(val) => render_ro_row(label, val),
            PropValue::HatchPatternChoice(current) => {
                self.render_hatch_pattern_row(label, current)
            }
            PropValue::AttrText { tag, value } => self.render_attr_row(tag, value),
        }
    }

    // ── Layer row (combo_box) ─────────────────────────────────────────────

    fn render_layer_row<'a>(&'a self, label: &'a str, current: &'a str) -> Element<'a, Message> {
        let selected = if current == VARIES_LABEL {
            None
        } else {
            Some(current.to_string())
        };
        let combo = combo_box(
            &self.layer_combo,
            VARIES_LABEL,
            selected.as_ref(),
            Message::PropLayerChanged,
        )
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 6.0,
            right: 6.0,
        })
        .input_style(combo_input_style)
        .on_open(Message::PropColorPickerClose)
        .width(Length::Fill);

        prop_row_widget(label, combo.into())
    }

    // ── Color row (custom picker) ─────────────────────────────────────────

    fn render_color_row<'a>(
        &'a self,
        label: &'a str,
        field: &'static str,
        color: AcadColor,
    ) -> Element<'a, Message> {
        // MTEXT background colour uses its own picker state + messages so it
        // routes to `background_color`, not the entity's main colour.
        if field == "background_color" {
            let selector = crate::ui::color_select::color_selector(
                color,
                self.bg_color_picker_open,
                crate::ui::color_select::ColorExtras {
                    by_layer: true,
                    by_block: true,
                },
                Message::PropBgColorChanged,
                Message::PropBgColorPickerToggle,
                // "More Colors…" opens the full palette window targeting the
                // background colour — this used to just close the picker
                // (#415).
                Message::OpenColorWindow(crate::app::ColorPickTarget::PropertiesBg),
            );
            return prop_row_widget(label, selector);
        }
        // Generic per-field colour picker — routes to the named field, not the
        // entity's main colour. Used by hatch gradient colours and the dim-line
        // colour override (Leader / Dimension). Dim colours legitimately take
        // ByLayer / ByBlock; gradient colours do not.
        if field == "gradient_color_1" || field == "gradient_color_2" || field == "dim_line_color" {
            let open = self.open_color_field.as_deref() == Some(field);
            let fsel = field.to_string();
            let extras = if field == "dim_line_color" {
                crate::ui::color_select::ColorExtras {
                    by_layer: true,
                    by_block: true,
                }
            } else {
                crate::ui::color_select::ColorExtras {
                    by_layer: false,
                    by_block: false,
                }
            };
            let selector = crate::ui::color_select::color_selector(
                color,
                open,
                extras,
                move |c| Message::PropColorFieldChanged {
                    field: fsel.clone(),
                    color: c,
                },
                Message::PropColorFieldToggle(field.to_string()),
                Message::PropColorFieldToggle(field.to_string()),
            );
            return prop_row_widget(label, selector);
        }
        let selector = crate::ui::color_select::color_selector(
            color,
            self.color_picker_open,
            crate::ui::color_select::ColorExtras {
                by_layer: true,
                by_block: true,
            },
            Message::PropColorChanged,
            Message::PropColorPickerToggle,
            Message::OpenColorWindow(crate::app::ColorPickTarget::Properties),
        );
        prop_row_widget(label, selector)
    }

    fn render_color_varies_row<'a>(&'a self, label: &'a str) -> Element<'a, Message> {
        let color_btn = button(
            row![
                container(text("?").size(10))
                    .style(move |theme: &Theme| {
                        let palette = theme.extended_palette();
                        container::Style {
                        background: Some(Background::Color(palette.background.strong.color)),
                        border: Border {
                            color: palette.background.neutral.color,
                            width: 1.0,
                            radius: 2.0.into()
                        },
                        text_color: Some(palette.background.strong.text),
                        ..Default::default()
                        }
                    })
                    .width(SWATCH_SZ)
                    .height(SWATCH_SZ)
                    .align_x(iced::Center)
                    .align_y(iced::Center),
                text(VARIES_LABEL).size(FONT_SZ),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(Message::PropColorPickerToggle)
        .style(combo_btn_style)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 6.0,
            right: 6.0,
        })
        .width(Length::Fill);

        let color_row = prop_row_widget(label, color_btn.into());
        if self.color_picker_open {
            column![color_row, self.render_color_picker()]
                .spacing(0)
                .into()
        } else {
            color_row
        }
    }

    fn render_color_picker(&self) -> Element<'_, Message> {
        color_picker_dropdown(
            self.color_palette_open,
            Message::PropColorPaletteToggle,
            Some(Message::PropColorChanged(AcadColor::ByLayer)),
            Some(Message::PropColorChanged(AcadColor::ByBlock)),
            |aci| Message::PropColorChanged(AcadColor::Index(aci)),
        )
    }

    // ── Lineweight row (combo_box) ────────────────────────────────────────

    fn render_lw_row<'a>(&'a self, label: &'a str, lw: LineWeight) -> Element<'a, Message> {
        let selected = LwItem(lw);
        let combo = combo_box(
            &self.lineweight_combo,
            "",
            Some(&selected),
            |item: LwItem| Message::PropLwChanged(item.0),
        )
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 6.0,
            right: 6.0,
        })
        .input_style(combo_input_style)
        .on_open(Message::PropColorPickerClose)
        .width(Length::Fill);

        prop_row_widget(label, combo.into())
    }

    fn render_lw_varies_row<'a>(&'a self, label: &'a str) -> Element<'a, Message> {
        let combo = combo_box(
            &self.lineweight_combo,
            VARIES_LABEL,
            None,
            |item: LwItem| Message::PropLwChanged(item.0),
        )
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 6.0,
            right: 6.0,
        })
        .input_style(combo_input_style)
        .on_open(Message::PropColorPickerClose)
        .width(Length::Fill);

        prop_row_widget(label, combo.into())
    }

    // ── Linetype row (combo_box) ──────────────────────────────────────────

    fn render_linetype_row<'a>(&'a self, label: &'a str, current: &'a str) -> Element<'a, Message> {
        // Normalise: empty string = "ByLayer"
        let display = if current.is_empty() {
            "ByLayer"
        } else {
            current
        };
        let selected = self
            .linetype_items
            .iter()
            .find(|item| item.name.eq_ignore_ascii_case(display))
            .cloned();
        let combo = combo_box(
            &self.linetype_combo,
            VARIES_LABEL,
            selected.as_ref(),
            |item: LinetypeItem| Message::PropLinetypeChanged(item.name),
        )
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 6.0,
            right: 6.0,
        })
        .input_style(combo_input_style)
        .on_open(Message::PropColorPickerClose)
        .width(Length::Fill);

        prop_row_widget(label, combo.into())
    }

    fn render_choice_row<'a>(
        &'a self,
        label: &'a str,
        field: &'static str,
        current: &'a str,
        _options: &'a [String],
    ) -> Element<'a, Message> {
        let Some(state) = self.choice_combos.get(field) else {
            return render_ro_row(label, current);
        };

        let selected = if current == VARIES_LABEL {
            None
        } else {
            Some(current.to_string())
        };
        let combo = combo_box(state, VARIES_LABEL, selected.as_ref(), move |value| {
            Message::PropGeomChoiceChanged { field, value }
        })
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 6.0,
            right: 6.0,
        })
        .input_style(combo_input_style)
        .on_open(Message::PropColorPickerClose)
        .width(Length::Fill);

        prop_row_widget(label, combo.into())
    }

    // ── Editable geometry row (text_input) ────────────────────────────────

    fn render_edit_row<'a>(
        &'a self,
        label: &'a str,
        field: &'static str,
        entity_val: &'a str,
    ) -> Element<'a, Message> {
        let display = self
            .edit_buf
            .get(field)
            .map(|s| s.as_str())
            .unwrap_or(entity_val);

        let ti = text_input("", display)
            .on_input(move |v| Message::PropGeomInput { field, value: v })
            .on_submit(Message::PropGeomCommit(field))
            .size(FONT_SZ)
            .style(text_input_style)
            .padding([3, 6])
            .width(Length::Fill);

        prop_row_widget(label, ti.into())
    }

    /// Editable dropdown row (block reference Name): a text field with a caret
    /// button in one bordered control. Typing + Enter commits through the
    /// normal PropGeomCommit path (existing name → re-point, new name →
    /// rename); the caret opens a floating list of the definitions (always
    /// downward, via the shared `floating_below` mechanic) and picking one
    /// applies through PropGeomChoiceChanged. Typed text filters the list.
    fn render_edit_choice_row<'a>(
        &'a self,
        label: &'a str,
        field: &'static str,
        entity_val: &'a str,
        options: &'a [String],
    ) -> Element<'a, Message> {
        let typed = self.edit_buf.get(field);
        let display = typed.map(|s| s.as_str()).unwrap_or(entity_val);

        let input = text_input("", display)
            .on_input(move |v| Message::PropGeomInput { field, value: v })
            .on_submit(Message::PropGeomCommit(field))
            .size(FONT_SZ)
            .style(|theme: &Theme, status| text_input::Style {
                // The wrapping container draws the border; keep the input flat
                // so field + caret read as one control.
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 0.0.into(),
                },
                ..text_input_style(theme, status)
            })
            .padding([3, 6])
            .width(Length::Fill);
        let caret = button(
            container(if self.edit_choice_open {
                crate::ui::icons::themed_arrow_up(FONT_SZ)
            } else {
                crate::ui::icons::themed_arrow_down(FONT_SZ)
            })
            .height(Length::Fill)
            .align_y(iced::Center),
        )
        .on_press(Message::PropEditChoiceToggle)
        .style(|theme: &Theme, status| {
            let palette = theme.extended_palette();
            let pair = match status {
                button::Status::Hovered | button::Status::Pressed => palette.background.weak,
                _ => palette.background.base,
            };
            button::Style {
            background: Some(Background::Color(pair.color)),
            text_color: pair.text,
            border: Border::default(),
            ..Default::default()
            }
        })
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 3.0,
            right: 3.0,
        })
        .height(Length::Fixed(ROW_H - 6.0));
        let head = container(row![input, caret].align_y(iced::Center))
            .style(|theme: &Theme| {
                let palette = theme.extended_palette();
                container::Style {
                background: Some(Background::Color(palette.background.base.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
                }
            })
            .width(Length::Fill);

        if !self.edit_choice_open {
            return prop_row_widget(label, head.into());
        }

        // Open list: all definitions, filtered by any typed text.
        let filter = typed.map(|s| s.to_lowercase());
        let mut list = column![].spacing(1);
        for opt in options {
            if let Some(f) = &filter {
                if !opt.to_lowercase().contains(f.as_str()) {
                    continue;
                }
            }
            let value = opt.clone();
            list = list.push(
                button(text(opt.as_str()).size(FONT_SZ))
                    .on_press(Message::PropGeomChoiceChanged { field, value })
                    .style(button::subtle)
                    .padding([2, 6])
                    .width(Length::Fill),
            );
        }
        let popup = container(scrollable(list).height(Length::Shrink))
            .style(container::bordered_box)
            .padding(2)
            .width(200)
            .max_height(220.0);

        prop_row_widget(
            label,
            crate::ui::color_select::floating_below(head.into(), popup.into()),
        )
    }

    /// One editable row for a block attribute: the tag is the row label and the
    /// text box edits the value. Routing rides the tag (a runtime string), so
    /// this uses the dedicated `PropAttr*` messages instead of the geometry
    /// path whose field key is `&'static str`. The row label is the tag itself.
    fn render_attr_row<'a>(&'a self, tag: &'a str, entity_val: &'a str) -> Element<'a, Message> {
        let key = attr_edit_key(tag);
        let display = self
            .edit_buf
            .get(&key)
            .map(|s| s.as_str())
            .unwrap_or(entity_val);

        let tag_for_input = tag.to_string();
        let ti = text_input("", display)
            .on_input(move |v| Message::PropAttrInput {
                tag: tag_for_input.clone(),
                value: v,
            })
            .on_submit(Message::PropAttrCommit(tag.to_string()))
            .size(FONT_SZ)
            .style(text_input_style)
            .padding([3, 6])
            .width(Length::Fill);

        prop_row_widget(tag, ti.into())
    }

    fn render_hatch_pattern_row<'a>(
        &'a self,
        label: &'a str,
        current: &'a str,
    ) -> Element<'a, Message> {
        let selected = if current == VARIES_LABEL {
            None
        } else {
            Some(current.to_string())
        };
        // iced hides `selected` while the combo has focus and shows its
        // placeholder instead. Use the real pattern name for a single hatch;
        // a fixed VARIES placeholder made a correct ANSI31 value look mixed as
        // soon as the dropdown opened (#476).
        let placeholder = if current == VARIES_LABEL {
            VARIES_LABEL
        } else {
            current
        };
        let combo = combo_box(
            &self.hatch_pattern_combo,
            placeholder,
            selected.as_ref(),
            Message::PropHatchPatternChanged,
        )
        .size(FONT_SZ)
        .padding(Padding {
            top: COMBO_PAD_V,
            bottom: COMBO_PAD_V,
            left: 6.0,
            right: 6.0,
        })
        .input_style(combo_input_style)
        .on_open(Message::PropColorPickerClose)
        .width(Length::Fill);

        prop_row_widget(label, combo.into())
    }
}

// ── Shared color picker widget ────────────────────────────────────────────

/// Builds the color picker dropdown content (standard swatches + optional
/// ByLayer/ByBlock + "More Colors…" expanding to full ACI palette).
/// Use this from both the Properties panel and the Layer Manager.
pub fn color_picker_dropdown<'a>(
    palette_open: bool,
    palette_toggle_msg: Message,
    by_layer_msg: Option<Message>,
    by_block_msg: Option<Message>,
    on_aci: impl Fn(u8) -> Message + 'a,
) -> Element<'a, Message> {
    // ByLayer / ByBlock row (optional)
    let extras: Option<Element<'a, Message>> = match (by_layer_msg, by_block_msg) {
        (Some(bl), Some(bb)) => Some(
            row![
                picker_text_btn("ByLayer", bl),
                picker_text_btn("ByBlock", bb)
            ]
            .spacing(4)
            .into(),
        ),
        (Some(bl), None) => Some(picker_text_btn("ByLayer", bl)),
        (None, Some(bb)) => Some(picker_text_btn("ByBlock", bb)),
        (None, None) => None,
    };

    // 9 standard ACI swatches (1-9)
    let standard: Element<'a, Message> = (1u8..=9u8)
        .fold(row![].spacing(2), |r, idx| {
            let c = AcadColor::Index(idx);
            let (bg, _) = acad_color_display(c);
            let msg = on_aci(idx);
            r.push(
                button(text("").width(18).height(18))
                    .on_press(msg)
                    .style(move |theme: &Theme, status| button::Style {
                        background: Some(Background::Color(bg)),
                        border: Border {
                            color: if matches!(status, button::Status::Hovered) {
                                theme.extended_palette().primary.base.color
                            } else {
                                theme.extended_palette().background.neutral.color
                            },
                            width: if matches!(status, button::Status::Hovered) {
                                1.5
                            } else {
                                1.0
                            },
                            radius: 2.0.into(),
                        },
                        text_color: theme.extended_palette().background.base.text,
                        ..Default::default()
                    })
                    .padding(0),
            )
        })
        .into();

    // "More Colors…" toggle button
    let more_btn = button(
        row![
            if palette_open {
                crate::ui::icons::themed_arrow_up(9.0)
            } else {
                crate::ui::icons::themed_arrow_down(9.0)
            },
            text(if palette_open { "Less" } else { "More Colors…" })
                .size(10)
                .style(hint_text_style),
        ]
        .spacing(4)
        .align_y(iced::Center),
    )
    .on_press(palette_toggle_msg)
    .style(button::subtle)
    .padding([2, 6])
    .width(Length::Fill);

    let inner = if let Some(e) = extras {
        column![e, standard, more_btn].spacing(4)
    } else {
        column![standard, more_btn].spacing(4)
    };

    let mut col = column![container(inner)
        .style(|theme: &Theme| {
            let palette = theme.extended_palette();
            container::Style {
            background: Some(Background::Color(palette.background.base.color)),
            border: Border {
                color: palette.background.neutral.color,
                width: 1.0,
                radius: 0.0.into()
            },
            ..Default::default()
            }
        })
        .padding([6, 8])
        .width(Length::Fill)]
    .spacing(0);

    // Full ACI palette (expanded)
    if palette_open {
        const COLS: u16 = 16;
        let mut rows = column![].spacing(1);
        let mut idx: u16 = 1;
        while idx <= 255 {
            let mut r = row![].spacing(1);
            for _ in 0..COLS {
                if idx > 255 {
                    break;
                }
                let ci = idx as u8;
                let (bg, _) = acad_color_display(AcadColor::Index(ci));
                let msg = on_aci(ci);
                r = r.push(
                    button(text("").width(12).height(12))
                        .on_press(msg)
                        .style(move |theme: &Theme, status| button::Style {
                            background: Some(Background::Color(bg)),
                            border: Border {
                                color: if matches!(status, button::Status::Hovered) {
                                    theme.extended_palette().primary.base.color
                                } else {
                                    theme.extended_palette().background.neutral.color
                                },
                                width: if matches!(status, button::Status::Hovered) {
                                    1.5
                                } else {
                                    1.0
                                },
                                radius: 1.0.into(),
                            },
                            text_color: theme.extended_palette().background.base.text,
                            ..Default::default()
                        })
                        .padding(0),
                );
                idx += 1;
            }
            rows = rows.push(r);
        }
        col = col.push(
            container(scrollable(rows).height(160))
                .style(|theme: &Theme| {
                    let palette = theme.extended_palette();
                    container::Style {
                    background: Some(Background::Color(palette.background.base.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 0.0.into(),
                    },
                    ..Default::default()
                    }
                })
                .padding([4, 6])
                .width(Length::Fill),
        );
    }

    col.into()
}

// ── Standalone helpers ────────────────────────────────────────────────────

/// A boolean toggle button row (for "Invisible" etc.).
fn render_stepper_row<'a>(label: &'a str, display: &'a str) -> Element<'a, Message> {
    let arrow = |glyph: &'static str, delta: i8| {
        button(text(glyph).size(FONT_SZ))
            .on_press(Message::PropVertexStep(delta))
            .padding([0, 6])
            .style(|theme: &Theme, status| {
                let palette = theme.extended_palette();
                let pair = match status {
                    button::Status::Hovered | button::Status::Pressed => palette.background.weak,
                    _ => palette.background.base,
                };
                button::Style {
                    background: Some(Background::Color(pair.color)),
                    border: Border {
                        color: palette.background.neutral.color,
                        width: 1.0,
                        radius: 2.0.into(),
                    },
                    text_color: pair.text,
                    ..Default::default()
                }
            })
    };
    let widget = iced::widget::row![
        arrow("◀", -1),
        text(display)
            .size(FONT_SZ)
            .width(Length::Fill)
            .align_x(iced::Center),
        arrow("▶", 1),
    ]
    .spacing(4)
    .align_y(iced::Center);
    prop_row_widget(label, widget.into())
}

fn render_bool_row<'a>(label: &'a str, field: &'static str, value: bool) -> Element<'a, Message> {
    let btn_label = if value { "Yes" } else { "No" };
    let btn =
        button(
            text(btn_label)
                .size(FONT_SZ)
                .style(move |theme: &Theme| iced::widget::text::Style {
                    color: value.then_some(theme.extended_palette().warning.base.color),
                }),
        )
        .on_press(Message::PropBoolToggle(field))
        .style(move |theme: &Theme, status| {
            let palette = theme.extended_palette();
            let pair = match status {
                button::Status::Hovered | button::Status::Pressed => palette.background.weak,
                _ => palette.background.base,
            };
            button::Style {
                background: Some(Background::Color(pair.color)),
                border: Border {
                    color: palette.background.neutral.color,
                    width: 1.0,
                    radius: 2.0.into(),
                },
                text_color: if value {
                    palette.warning.base.color
                } else {
                    pair.text
                },
                ..Default::default()
            }
        })
        .padding([2, 6])
        .width(Length::Fill);

    prop_row_widget(label, btn.into())
}

// ── Collapsible coordinate groups (Position / Start / Scale …) ────────────

/// The X/Y/Z suffix rank of a coordinate row label, with its base ("Position
/// X" → ("Position", 0)). `None` for non-coordinate labels.
fn coord_suffix(label: &str) -> Option<(&str, usize)> {
    for (rank, suf) in [" X", " Y", " Z"].iter().enumerate() {
        if let Some(base) = label.strip_suffix(suf) {
            if !base.is_empty() {
                return Some((base, rank));
            }
        }
    }
    None
}

/// Length of the coordinate group starting at `idx`: consecutive text rows
/// labelled "<Base> X", "<Base> Y" and optionally "<Base> Z". 0/1 = no group.
fn coord_group_len(props: &[crate::scene::model::object::Property], idx: usize) -> usize {
    let groupable = |p: &crate::scene::model::object::Property| {
        matches!(p.value, PropValue::EditText(_) | PropValue::ReadOnly(_))
    };
    let Some((base, 0)) = coord_suffix(&props[idx].label) else {
        return 0;
    };
    if !groupable(&props[idx]) {
        return 0;
    }
    let mut len = 1;
    while idx + len < props.len() && len < 3 {
        match coord_suffix(&props[idx + len].label) {
            Some((b, r)) if b == base && r == len && groupable(&props[idx + len]) => len += 1,
            _ => break,
        }
    }
    if len >= 2 {
        len
    } else {
        0
    }
}

fn coord_base(label: &str) -> &str {
    coord_suffix(label).map(|(b, _)| b).unwrap_or(label)
}

/// Short component label for an expanded row ("Position X" → indented "X").
fn coord_component(label: &str) -> &'static str {
    match coord_suffix(label) {
        Some((_, 0)) => "    X",
        Some((_, 1)) => "    Y",
        _ => "    Z",
    }
}

/// Display string of a text-valued property (grouped rows are always
/// EditText / ReadOnly — see `coord_group_len`).
fn prop_text_value(prop: &crate::scene::model::object::Property) -> String {
    match &prop.value {
        PropValue::EditText(s) | PropValue::ReadOnly(s) => s.clone(),
        _ => String::new(),
    }
}

/// The collapsed summary row of a coordinate group. The expand arrow leads
/// the label cell (clicking the cell toggles); the value cell is the same
/// read-only selectable field every other read-only row uses.
fn render_group_row(
    base: &str,
    key: String,
    expanded: bool,
    joined: String,
) -> Element<'_, Message> {
    let label_btn = button(
        container(
            row![
                if expanded {
                    crate::ui::icons::themed_arrow_down(FONT_SZ)
                } else {
                    crate::ui::icons::themed_arrow_right(FONT_SZ)
                },
                text(crate::ui::text_util::elide(base, 16))
                    .size(FONT_SZ)
                    .style(muted_text_style),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .height(Length::Fill)
        .align_y(iced::Center),
    )
    .on_press(Message::PropGroupToggle(key))
    .style(button::subtle)
    .padding(Padding {
        top: 0.0,
        bottom: 0.0,
        left: 4.0,
        right: 6.0,
    })
    .width(Length::Fill)
    .height(Length::Fixed(ROW_H));
    let label_col = container(label_btn)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weakest.color,
            )),
            ..Default::default()
        })
        .width(Length::FillPortion(5))
        .height(Length::Fixed(ROW_H))
        .align_y(iced::Center);

    // text_input copies the value, so the locally-built `joined` is fine here.
    let value_field = text_input("", &joined)
        .on_input(|_| Message::Noop)
        .size(FONT_SZ)
        .style(ro_input_style)
        .padding([3, 6])
        .width(Length::Fill);
    let value_col = container(value_field)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color,
            )),
            ..Default::default()
        })
        .width(Length::FillPortion(6))
        .height(Length::Fixed(ROW_H))
        .align_y(iced::Center)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 2.0,
            right: 2.0,
        });

    container(row![label_col, value_col])
        .height(Length::Fixed(ROW_H))
        .style(|theme: &Theme| container::Style {
            border: Border {
                color: theme.extended_palette().background.neutral.color,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn render_ro_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    // A read-only value is shown as a non-editable but selectable text field:
    // the user can select the text (which carries the full, un-truncated
    // value) and copy it with Ctrl+C. Keystrokes route to Noop, so the value
    // can be selected/copied but never edited.
    let field = text_input("", value)
        .on_input(|_| Message::Noop)
        .size(FONT_SZ)
        .style(ro_input_style)
        .padding([3, 6])
        .width(Length::Fill);
    prop_row_widget(label, field.into())
}

/// Build a label | widget property row.
fn prop_row_widget<'a>(label: &'a str, widget: Element<'a, Message>) -> Element<'a, Message> {
    let label_col = container(
        text(crate::ui::text_util::elide(label, 18))
            .size(FONT_SZ)
            .style(muted_text_style),
    )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.weakest.color,
            )),
            ..Default::default()
        })
        .width(Length::FillPortion(5))
        .height(Length::Fixed(ROW_H))
        .align_y(iced::Center)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 6.0,
            right: 6.0,
        });
    let value_col = container(widget)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.extended_palette().background.base.color,
            )),
            ..Default::default()
        })
        .width(Length::FillPortion(6))
        .height(Length::Fixed(ROW_H))
        .align_y(iced::Center)
        .padding(Padding {
            top: 0.0,
            bottom: 0.0,
            left: 2.0,
            right: 2.0,
        });
    container(row![label_col, value_col])
        .height(Length::Fixed(ROW_H))
        .style(|theme: &Theme| container::Style {
            border: Border {
                color: theme.extended_palette().background.neutral.color,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// A plain text button used inside the color picker for ByLayer / ByBlock.
fn picker_text_btn(label: &str, msg: Message) -> Element<'_, Message> {
    button(text(label).size(FONT_SZ))
        .on_press(msg)
        .style(button::secondary)
        .padding([2, 8])
        .into()
}

// ── Color display helper ──────────────────────────────────────────────────

/// Returns an (iced::Color swatch_bg, display_label) pair for an AcadColor.
pub fn acad_color_display(c: AcadColor) -> (Color, &'static str) {
    match c {
        AcadColor::None => (Color::TRANSPARENT, "None"),
        AcadColor::ByLayer => (
            Color {
                r: 0.35,
                g: 0.35,
                b: 0.35,
                a: 1.0,
            },
            "ByLayer",
        ),
        AcadColor::ByBlock => (
            Color {
                r: 0.25,
                g: 0.25,
                b: 0.45,
                a: 1.0,
            },
            "ByBlock",
        ),
        AcadColor::Index(i) => {
            let (r, g, b) = acadrust::types::aci_table::aci_to_rgb(i).unwrap_or((200, 200, 200));
            (
                Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0),
                aci_label(i),
            )
        }
        AcadColor::Rgb { r, g, b } => (
            Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0),
            "Custom",
        ),
    }
}

fn aci_label(idx: u8) -> &'static str {
    match idx {
        1 => "Red",
        2 => "Yellow",
        3 => "Green",
        4 => "Cyan",
        5 => "Blue",
        6 => "Magenta",
        7 => "White",
        8 => "Dark Gray",
        9 => "Light Gray",
        _ => "Index",
    }
}

// ── Widget style helpers ──────────────────────────────────────────────────

fn combo_btn_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();
    let pair = match status {
        button::Status::Hovered | button::Status::Pressed => palette.background.weak,
        _ => palette.background.base,
    };
    button::Style {
        background: Some(Background::Color(pair.color)),
        border: Border {
            color: palette.background.neutral.color,
            width: 1.0,
            radius: 2.0.into(),
        },
        text_color: pair.text,
        ..Default::default()
    }
}

fn text_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    let border_color = match status {
        text_input::Status::Focused { .. } => palette.primary.base.color,
        _ => palette.background.neutral.color,
    };
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 2.0.into(),
        },
        icon: Color::TRANSPARENT,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

fn combo_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    text_input_style(theme, status)
}

/// Style for a read-only-but-selectable value field: flat (no input box or
/// focus highlight, so it reads as plain text, unlike the bordered editable
/// fields) yet with a visible selection colour so Ctrl+C copy is discoverable.
fn ro_input_style(theme: &Theme, _status: text_input::Status) -> text_input::Style {
    let palette = theme.extended_palette();
    text_input::Style {
        background: Background::Color(palette.background.base.color),
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 0.0.into(),
        },
        icon: Color::TRANSPARENT,
        placeholder: palette.background.base.text.scale_alpha(0.48),
        value: palette.background.base.text,
        selection: palette.primary.base.color.scale_alpha(0.5),
    }
}

fn muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.72)),
    }
}

fn hint_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().background.base.text.scale_alpha(0.48)),
    }
}
