//! Properties panel — OpenCADStudio-style editable object properties.
//!
//! Shows two sections (General + Geometry) for the selected entity.
//! • Layer      → combo_box  (options from document layer table)
//! • Color      → inline color picker  (ByLayer / ByBlock / ACI palette)
//! • Lineweight → combo_box  (standard CAD lineweight list)
//! • Linetype   → read-only for now
//! • Geometry   → text_input per coordinate / dimension field

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{fmt, sync::Arc};

use crate::ui::ROW_H;
use acadrust::types::{Color as AcadColor, LineWeight};
use acadrust::Handle;
use iced::widget::{
    button, canvas, column, combo_box, container, mouse_area, row, scrollable, text, text_input,
    tooltip, Space,
};
use iced::{
    mouse, Background, Border, Color, Element, Length, Padding, Point, Rectangle, Size, Theme,
};
use crate::t;

// ── Row-height-derived constants ─────────────────────────────────────────
const FONT_SZ: f32 = ROW_H * 0.42; // ≈11 px
const COMBO_PAD_V: f32 = (ROW_H - FONT_SZ * 1.3 - 2.0) / 2.0; // fills combo to ROW_H
const SWATCH_SZ: f32 = ROW_H * 0.54; // ≈14 px color swatch
const PATTERN_CARD_W: f32 = 158.0;
const PATTERN_PREVIEW_H: f32 = 58.0;
const PATTERN_PICKER_W: f32 = 348.0;
const PATTERN_PICKER_H: f32 = 720.0;

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
            LineWeight::ByLayer => write!(f, "{}", crate::t!("ByLayer")),
            LineWeight::ByBlock => write!(f, "{}", crate::t!("ByBlock")),
            LineWeight::Default => write!(f, "{}", crate::t!("Default")),
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

#[derive(Clone)]
struct HatchPatternPreview {
    pattern: crate::scene::model::hatch_model::HatchPattern,
}

impl canvas::Program<Message> for HatchPatternPreview {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &iced::Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        use crate::scene::model::hatch_model::{HatchModel, HatchPattern};

        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let palette = theme.palette();
        let pad = 4.0;
        let sample = canvas::Path::rectangle(
            Point::new(pad, pad),
            Size::new(
                (bounds.width - pad * 2.0).max(0.0),
                (bounds.height - pad * 2.0).max(0.0),
            ),
        );
        frame.fill(&sample, palette.background.base.color);

        match &self.pattern {
            HatchPattern::Solid => {
                frame.fill(&sample, palette.background.base.text.scale_alpha(0.72));
            }
            HatchPattern::Gradient { .. } => {
                frame.fill(&sample, palette.primary.weak.color);
            }
            HatchPattern::Pattern(_) => {
                let model = HatchModel {
                    render_instance: None,
                    world_origin: [0.0, 0.0],
                    boundary: Arc::new(vec![
                        [pad, pad],
                        [bounds.width - pad, pad],
                        [bounds.width - pad, bounds.height - pad],
                        [pad, bounds.height - pad],
                    ]),
                    boundary_wcs: None,
                    fill_plane: None,
                    fill_plane_boundary: None,
                    boundary_exterior: None,
                    boundary_sources: None,
                    boundary_paths: None,
                    style: acadrust::entities::HatchStyleType::Normal,
                    pattern: self.pattern.clone(),
                    name: String::new(),
                    color: [1.0; 4],
                    aci: 0,
                    line_weight_px: 1.0,
                    angle_offset: 0.0,
                    scale: hatch_preview_scale(&self.pattern),
                    draw_depth: 0.0,
                };
                let stroke = canvas::Stroke::default()
                    .with_color(palette.background.base.text)
                    .with_width(1.0);
                for segment in model.pattern_segments() {
                    frame.stroke(
                        &canvas::Path::line(
                            Point::new(segment[0][0] as f32, bounds.height - segment[0][1] as f32),
                            Point::new(segment[1][0] as f32, bounds.height - segment[1][1] as f32),
                        ),
                        stroke.clone(),
                    );
                }
            }
        }

        frame.stroke(
            &sample,
            canvas::Stroke::default()
                .with_color(palette.background.neutral.color)
                .with_width(1.0),
        );
        vec![frame.into_geometry()]
    }
}

fn hatch_preview_scale(pattern: &crate::scene::model::hatch_model::HatchPattern) -> f32 {
    use crate::scene::model::hatch_model::HatchPattern;

    let HatchPattern::Pattern(families) = pattern else {
        return 1.0;
    };
    let spacing = families
        .iter()
        .filter_map(|family| {
            let spacing = family.dy.abs();
            (spacing > 1.0e-4).then_some(spacing)
        })
        .fold(f32::INFINITY, f32::min);
    if spacing.is_finite() {
        (8.0 / spacing).clamp(0.01, 100.0)
    } else {
        1.0
    }
}

fn hatch_pattern_matches(
    entry: &crate::scene::model::hatch_patterns::PatternEntry,
    search: &str,
) -> bool {
    let query = search.trim();
    query.is_empty()
        || entry.name.to_lowercase().contains(&query.to_lowercase())
        || entry.description.to_lowercase().contains(&query.to_lowercase())
}

pub(crate) fn filtered_hatch_patterns(
    search: &str,
) -> Vec<&'static crate::scene::model::hatch_patterns::PatternEntry> {
    crate::scene::model::hatch_patterns::catalog()
        .iter()
        .filter(|entry| hatch_pattern_matches(entry, search))
        .collect()
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

/// Edit-buffer / active-field key identifying which value a row edits. Geometry
/// and common fields key on their `&'static str` field name; block attribute
/// rows key on the attribute's tag. One typed enum replaces the old
/// `\x01attr\x01…` sentinel prefix, so the two key namespaces can't collide by
/// construction.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FieldKey {
    /// A geometry / common editable value field, keyed by its field name.
    Geom(&'static str),
    /// A block attribute value field, keyed by its tag.
    Attr(String),
}

impl FieldKey {
    /// The text-input widget id backing this key (the shape used by the row
    /// renderer and the `PropSyncActive` handler). Geometry keys map straight
    /// onto their field id; attribute keys map onto the tag's field id.
    pub fn widget_id(&self) -> iced::widget::Id {
        match self {
            FieldKey::Geom(field) => prop_geom_field_id(field),
            FieldKey::Attr(tag) => prop_attr_field_id(tag),
        }
    }
}

/// Edit-buffer / active-field key for a block attribute value, keyed by its
/// tag. Kept in one place so the live-input handler and the row renderer agree.
pub fn attr_edit_key(tag: &str) -> FieldKey {
    FieldKey::Attr(tag.to_string())
}

/// Text-input widget id for a geometry/common editable value field. The same
/// id (built from the `&'static str` key) is used by the row renderer to tag
/// the input and by the update handler to focus / select it on click.
pub fn prop_geom_field_id(field: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("props-geom-field-{field}"))
}

/// Text-input widget id for a block attribute value field, keyed by the tag.
pub fn prop_attr_field_id(tag: &str) -> iced::widget::Id {
    iced::widget::Id::from(format!("props-attr-field-{tag}"))
}

/// Keeps the active-row highlight keyed to real keyboard focus: returns true
/// only while the currently-focused widget IS the text input of the active
/// field. Focus on any other widget (another property field, a non-property
/// widget, or nothing at all) clears the marker. Setting the key for a newly
/// focused property field is handled by the `PropSyncActive` handler before
/// this runs.
pub fn active_key_focused(
    active_key: Option<&FieldKey>,
    focused: Option<&iced::widget::Id>,
) -> bool {
    match (active_key, focused) {
        (Some(key), Some(focused_id)) => focused_id == &key.widget_id(),
        _ => false,
    }
}

/// Precomputes the focused-id → [`FieldKey`] map for every editable value row
/// in the given sections. Building it once when the panel's sections are
/// assembled lets a `PropSyncActive` event map a focused text-input id back to
/// its field key in O(1) instead of re-scanning the sections. The block-Name
/// caret-dropdown (`EditChoice`) is editable but deliberately carries no id and
/// is excluded here, mirroring the renderer.
pub fn build_field_key_map(
    sections: &[PropSection],
) -> HashMap<iced::widget::Id, FieldKey> {
    let mut map = HashMap::default();
    for section in sections {
        for prop in &section.props {
            let key = match &prop.value {
                PropValue::EditText(_) | PropValue::PlainText(_) => {
                    Some(FieldKey::Geom(prop.field))
                }
                PropValue::AttrText { tag, .. } => Some(FieldKey::Attr(tag.clone())),
                _ => None,
            };
            if let Some(key) = key {
                map.insert(key.widget_id(), key);
            }
        }
    }
    map
}

/// Returns a task that sweeps the widget tree for the currently-focused widget
/// and reports its `Id` via [`Message::PropSyncActive`], which the update
/// handler uses to reconcile the active-row marker against real focus. Mirrors
/// the `WebFieldCopy` operation idiom: `focusable` fires for every focusable
/// widget, and we record the one that `is_focused()`.
pub fn sync_active_field_task() -> iced::Task<Message> {
    use iced::advanced::widget::operation::{Focusable, Outcome};
    use iced::advanced::widget::{Id, Operation};
    #[derive(Default)]
    struct FocusedId {
        focused: Option<Id>,
    }
    impl Operation<Option<Id>> for FocusedId {
        fn focusable(
            &mut self,
            id: Option<&Id>,
            _bounds: iced::Rectangle,
            state: &mut dyn Focusable,
        ) {
            if state.is_focused() && self.focused.is_none() {
                self.focused = id.cloned();
            }
        }
        fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<Option<Id>>)) {
            operate(self);
        }
        fn finish(&self) -> Outcome<Option<Id>> {
            Outcome::Some(self.focused.clone())
        }
    }
    iced::advanced::widget::operate(FocusedId::default()).map(Message::PropSyncActive)
}

/// A translated choice label paired with the unchanged value stored in the
/// drawing and emitted by the properties panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalizedChoice {
    pub raw: String,
    label: String,
}

impl LocalizedChoice {
    pub fn new(raw: String) -> Self {
        let label = crate::i18n::translate(&raw).into_owned();
        Self { raw, label }
    }
}

impl fmt::Display for LocalizedChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
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
    pub choice_combos: HashMap<String, combo_box::State<LocalizedChoice>>,
    pub layer_combo: combo_box::State<String>,
    pub lineweight_combo: combo_box::State<LwItem>,
    pub linetype_combo: combo_box::State<LinetypeItem>,
    /// Whether the visual hatch-pattern picker is open.
    pub hatch_pattern_picker_open: bool,
    /// Case-insensitive filter for the visual hatch-pattern picker.
    pub hatch_pattern_search: String,
    /// Keyboard/hover focus inside the filtered visual pattern grid.
    pub hatch_pattern_focus: usize,
    /// In-progress text edits keyed by [`FieldKey`].
    pub edit_buf: HashMap<FieldKey, String>,
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
    /// Draw the Current Vertex indicator only after the user changes the
    /// stepper for the current selection.
    pub prop_vertex_indicator_active: bool,
    /// Coordinate groups ("Position", "Scale", …) the user expanded into their
    /// component X/Y/Z rows. Collapsed by default; keyed `section:base` and
    /// carried across panel rebuilds so the state survives edits and selection
    /// changes.
    pub expanded_groups: HashSet<String>,
    /// Whether the editable-dropdown (block Name) option list is open.
    pub edit_choice_open: bool,
    /// Field key of the value row currently being edited, or `None`. Marked when
    /// the user focuses an editable value field and cleared on commit / when the
    /// selection changes, so the active row can be highlighted. Geometry fields
    /// use [`FieldKey::Geom`]; attribute rows use [`FieldKey::Attr`].
    pub active_field: Option<FieldKey>,
    /// Focused-text-input id → [`FieldKey`] for the editable value rows in
    /// [`PropertiesPanel::sections`], precomputed by [`build_field_key_map`]
    /// when the panel is rebuilt. Gives the `PropSyncActive` handler an O(1)
    /// id→key lookup. Derived from `sections`: rebuild it alongside them.
    pub field_key_by_id: HashMap<iced::widget::Id, FieldKey>,
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
            hatch_pattern_picker_open: false,
            hatch_pattern_search: String::new(),
            hatch_pattern_focus: 0,
            edit_buf: HashMap::default(),
            source_handles: vec![],
            color_picker_open: false,
            color_palette_open: false,
            bg_color_picker_open: false,
            open_color_field: None,
            prop_vertex: 0,
            prop_vertex_indicator_active: false,
            expanded_groups: HashSet::default(),
            edit_choice_open: false,
            active_field: None,
            field_key_by_id: HashMap::default(),
        }
    }
}

impl PropertiesPanel {
    pub fn empty() -> Self {
        Self {
            title: t!("No selection").into_owned(),
            ..Default::default()
        }
    }

    /// Maps a focused text-input widget id back to the [`FieldKey`] of that
    /// property row, if the id belongs to an editable value field currently
    /// shown. O(1) via the [`PropertiesPanel::field_key_by_id`] map.
    pub fn prop_field_key_for_id(&self, id: &iced::widget::Id) -> Option<FieldKey> {
        self.field_key_by_id.get(id).cloned()
    }

    pub fn selected_handles(&self) -> Vec<Handle> {
        self.selected_group
            .as_ref()
            .map(|group| group.handles.clone())
            .unwrap_or_default()
    }

    pub fn view(&self, width: f32, auto_collapse: bool) -> Element<'_, Message> {
        use crate::ui::dock::{DockMsg, PanelId};
        // ── Header ──────────────────────────────────────────────────────────
        let pin_icon = if auto_collapse {
            crate::ui::icons::themed_primary_weak_text(crate::ui::icons::PIN, 12.0)
        } else {
            crate::ui::icons::themed_secondary(crate::ui::icons::PIN, 12.0)
        };
        let pin = button(pin_icon)
            .on_press(Message::Dock(DockMsg::AutoCollapseToggle(PanelId::Properties)))
            .style(move |theme: &Theme, status| {
                let mut style = button::subtle(theme, status);
                if auto_collapse {
                    let palette = theme.palette();
                    style.background = Some(Background::Color(palette.primary.weak.color));
                    style.text_color = palette.primary.weak.text;
                    style.border.color = palette.primary.base.color;
                    style.border.width = 1.0;
                }
                style
            })
            .padding([3, 5]);
        let pin = tooltip(pin, text(t!("Auto")).size(10), tooltip::Position::Bottom).gap(4);

        let close = button(crate::ui::icons::themed_secondary(
            crate::ui::icons::CLOSE,
            12.0,
        ))
        .on_press(Message::Dock(DockMsg::Close(PanelId::Properties)))
        .style(button::subtle)
        .padding([3, 5]);
        let close = tooltip(
            close,
            text(t!("Close")).size(10),
            tooltip::Position::Bottom,
        )
        .gap(4);

        let header = mouse_area(
            container(
                row![
                    text(t!("Properties")).size(12),
                    Space::new().width(Length::Fill),
                    pin,
                    close,
                ]
                .spacing(3)
                .align_y(iced::Center),
            )
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(
                    theme.palette().background.weak.color,
                )),
                ..Default::default()
            })
            .width(Length::Fill)
            .padding([3, 6]),
        )
        .on_press(Message::Dock(DockMsg::DockGrab(PanelId::Properties)))
        .interaction(iced::mouse::Interaction::Grab);

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
                let palette = theme.palette();
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
                text(t!("Select an object to view properties"))
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
                let palette = theme.palette();
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
            .width(Length::Fixed(width))
            .height(Length::Fill)
            .into()
    }

    /// Compact floating panel for Quick Properties: the title plus the same
    /// editable section rows as the docked panel, sized to its content.
    /// Returns `None` when nothing is selected.
    pub fn quick_view(&self) -> Option<Element<'_, Message>> {
        if self.source_handles.is_empty() || self.sections.is_empty() {
            return None;
        }
        let title = container(
            text(crate::ui::text_util::elide(&self.title, 34))
                .size(FONT_SZ)
                .style(muted_text_style),
        )
            .style(|theme: &Theme| {
                let palette = theme.palette();
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

        let mut sections = column![].spacing(0);
        for section in &self.sections {
            sections = sections.push(self.render_section(section));
        }

        let content = scrollable(sections).height(Length::Shrink);

        Some(
            container(column![title, content].spacing(0))
                .style(|theme: &Theme| {
                    let palette = theme.palette();
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
                let palette = theme.palette();
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
            PropValue::EditText(val) | PropValue::PlainText(val) => {
                self.render_edit_row(label, prop.field, val)
            }
            PropValue::ReadOnly(val) if prop.field == "annotative_scale" => {
                render_annotative_scale_row(label, val)
            }
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
                Message::OpenColorWindow(
                    crate::app::ColorPickTarget::PropertiesBg,
                    color,
                ),
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
            Message::OpenColorWindow(
                crate::app::ColorPickTarget::Properties,
                color,
            ),
        );
        prop_row_widget(label, selector)
    }

    fn render_color_varies_row<'a>(&'a self, label: &'a str) -> Element<'a, Message> {
        let color_btn = button(
            row![
                container(text("?").size(10))
                    .style(move |theme: &Theme| {
                        let palette = theme.palette();
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
            Some(LocalizedChoice::new(current.to_string()))
        };
        let combo = combo_box(state, VARIES_LABEL, selected.as_ref(), move |choice| {
            Message::PropGeomChoiceChanged {
                field,
                value: choice.raw,
            }
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
        let key = FieldKey::Geom(field);
        let display = self
            .edit_buf
            .get(&key)
            .map(|s| s.as_str())
            .unwrap_or(entity_val);

        let active = self.active_field.as_ref() == Some(&key);
        let ti = text_input("", display)
            .id(prop_geom_field_id(field))
            .on_input(move |v| Message::PropGeomInput { field, value: v })
            .on_submit(Message::PropGeomCommit(field))
            .size(FONT_SZ)
            .style(text_input_style)
            .padding([3, 6])
            .width(Length::Fill);

        prop_row_with_active(label, ti.into(), active)
    }

    /// Editable dropdown row (block reference Name): a text field with a caret
    /// button in one bordered control. Typing + Enter commits through the
    /// normal PropGeomCommit path (existing name → re-point, new name →
    /// rename); the caret opens a dropdown list of the definitions and picking one
    /// applies through PropGeomChoiceChanged. Typed text filters the list.
    fn render_edit_choice_row<'a>(
        &'a self,
        label: &'a str,
        field: &'static str,
        entity_val: &'a str,
        options: &'a [String],
    ) -> Element<'a, Message> {
        let typed = self.edit_buf.get(&FieldKey::Geom(field));
        let display = typed.map(|s| s.as_str()).unwrap_or(entity_val);

        // NOTE: unlike `render_edit_row` / `render_attr_row`, this input is
        // deliberately NOT wrapped in a click-to-focus mouse_area. It is a
        // caret-dropdown: clicking should place the caret / open the list, and
        // the auto Select-all-on-focus would fight that. Don't "fix" it.
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
            let palette = theme.palette();
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
                let palette = theme.palette();
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
            .height(Length::Fit.max(220.0));

        prop_row_widget(
            label,
            crate::ui::color_select::drop_down_below(
                head.into(),
                popup.into(),
                Length::Fixed(200.0),
                Length::Shrink,
                Message::PropEditChoiceToggle,
            ),
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

        let active = self.active_field.as_ref() == Some(&key);
        let tag_for_input = tag.to_string();
        let ti = text_input("", display)
            .id(prop_attr_field_id(tag))
            .on_input(move |v| Message::PropAttrInput {
                tag: tag_for_input.clone(),
                value: v,
            })
            .on_submit(Message::PropAttrCommit(tag.to_string()))
            .size(FONT_SZ)
            .style(text_input_style)
            .padding([3, 6])
            .width(Length::Fill);

        prop_row_with_active(tag, ti.into(), active)
    }

    fn render_hatch_pattern_row<'a>(
        &'a self,
        label: &'a str,
        current: &'a str,
    ) -> Element<'a, Message> {
        let head = button(
            row![
                text(crate::ui::text_util::elide(current, 16))
                    .size(FONT_SZ)
                    .width(Length::Fill),
                if self.hatch_pattern_picker_open {
                    crate::ui::icons::themed_arrow_up(FONT_SZ)
                } else {
                    crate::ui::icons::themed_arrow_down(FONT_SZ)
                },
            ]
            .align_y(iced::Center),
        )
        .on_press(Message::PropHatchPatternPickerToggle(current.to_string()))
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: Some(Background::Color(if hovered {
                    palette.background.weak.color
                } else {
                    palette.background.base.color
                })),
                text_color: palette.background.base.text,
                border: Border {
                    color: if self.hatch_pattern_picker_open {
                        palette.primary.base.color
                    } else {
                        palette.background.neutral.color
                    },
                    width: 1.0,
                    radius: 2.0.into(),
                },
                ..Default::default()
            }
        })
        .padding([COMBO_PAD_V, 6.0])
        .width(Length::Fill);

        if !self.hatch_pattern_picker_open {
            return prop_row_widget(label, head.into());
        }

        let search = text_input(t!("Search patterns…").as_ref(), &self.hatch_pattern_search)
            .id(iced::widget::Id::new("hatch-pattern-search"))
            .on_input(Message::PropHatchPatternSearchChanged)
            .on_submit(Message::PropHatchPatternConfirm)
            .size(FONT_SZ)
            .padding([5, 7])
            .width(Length::Fill);

        let mut grid = column![].spacing(6);
        let visible = filtered_hatch_patterns(&self.hatch_pattern_search);
        for (row_index, pair) in visible.chunks(2).enumerate() {
            let mut cards = row![].spacing(6);
            for (column_index, entry) in pair.iter().enumerate() {
                let index = row_index * 2 + column_index;
                let selected = current.eq_ignore_ascii_case(&entry.name);
                let focused = self.hatch_pattern_focus == index;
                let name = entry.name.clone();
                let preview = canvas(HatchPatternPreview {
                    pattern: entry.gpu.clone(),
                })
                .width(Length::Fill)
                .height(PATTERN_PREVIEW_H);
                let card = button(
                    column![
                        preview,
                        container(text(crate::ui::text_util::elide(&entry.name, 20)).size(FONT_SZ))
                            .width(Length::Fill)
                            .align_x(iced::Center),
                    ]
                    .spacing(3),
                )
                .on_press(Message::PropHatchPatternChanged(name))
                .style(move |theme: &Theme, status| {
                    let palette = theme.palette();
                    let hovered =
                        matches!(status, button::Status::Hovered | button::Status::Pressed);
                    let pair = if selected {
                        palette.primary.weak
                    } else if hovered || focused {
                        palette.background.strong
                    } else {
                        palette.background.weak
                    };
                    button::Style {
                        background: Some(Background::Color(pair.color)),
                        text_color: pair.text,
                        border: Border {
                            color: if selected || focused {
                                palette.primary.base.color
                            } else {
                                palette.background.neutral.color
                            },
                            width: if selected || focused { 2.0 } else { 1.0 },
                            radius: 4.0.into(),
                        },
                        ..Default::default()
                    }
                })
                .padding(5)
                .width(PATTERN_CARD_W);
                cards = cards.push(
                    mouse_area(card).on_enter(Message::PropHatchPatternFocus(index)),
                );
            }
            grid = grid.push(cards);
        }

        let results: Element<'_, Message> = if visible.is_empty() {
            container(
                text(t!("No matching patterns"))
                    .size(FONT_SZ)
                    .style(hint_text_style),
            )
            .padding(12)
            .width(Length::Fill)
            .center_x(Length::Fill)
            .into()
        } else {
            scrollable(grid)
                .height(Length::Fill)
                .width(Length::Fill)
                .into()
        };
        let popup = container(column![search, results].spacing(7))
            .style(container::bordered_box)
            .padding(8)
            .width(PATTERN_PICKER_W)
            .height(Length::Fixed(PATTERN_PICKER_H));

        prop_row_widget(
            label,
            crate::ui::color_select::drop_down_below(
                head.into(),
                popup.into(),
                Length::Fixed(PATTERN_PICKER_W),
                Length::Fixed(PATTERN_PICKER_H),
                Message::PropHatchPatternPickerToggle(current.to_string()),
            ),
        )
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
                                theme.palette().primary.base.color
                            } else {
                                theme.palette().background.neutral.color
                            },
                            width: if matches!(status, button::Status::Hovered) {
                                1.5
                            } else {
                                1.0
                            },
                            radius: 2.0.into(),
                        },
                        text_color: theme.palette().background.base.text,
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
            text(if palette_open {
                t!("Less").into_owned()
            } else {
                t!("More Colors…").into_owned()
            })
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
            let palette = theme.palette();
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
                                    theme.palette().primary.base.color
                                } else {
                                    theme.palette().background.neutral.color
                                },
                                width: if matches!(status, button::Status::Hovered) {
                                    1.5
                                } else {
                                    1.0
                                },
                                radius: 1.0.into(),
                            },
                            text_color: theme.palette().background.base.text,
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
                    let palette = theme.palette();
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
                let palette = theme.palette();
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
    let btn_label = if value {
        t!("Yes").into_owned()
    } else {
        t!("No").into_owned()
    };
    let btn =
        button(
            text(btn_label)
                .size(FONT_SZ)
                .style(move |theme: &Theme| iced::widget::text::Style {
                    color: value.then_some(theme.palette().warning.base.color),
                }),
        )
        .on_press(Message::PropBoolToggle(field))
        .style(move |theme: &Theme, status| {
            let palette = theme.palette();
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
                theme.palette().background.weakest.color,
            )),
            ..Default::default()
        })
        .width(Length::FillPortion(5))
        .height(Length::Fixed(ROW_H))
        .align_y(iced::Center);

    // The field copies the value, so the locally-built `joined` is fine here.
    let value_field = crate::ui::read_only::field(&joined, FONT_SZ, Length::Fill);
    let value_col = container(value_field)
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(
                theme.palette().background.base.color,
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
                color: theme.palette().background.neutral.color,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}
fn render_annotative_scale_row<'a>(
    label: &'a str,
    value: &'a str,
) -> Element<'a, Message> {
    let field = crate::ui::read_only::field(value, FONT_SZ, Length::Fill);

    let manage = button(text("...").size(FONT_SZ))
        .on_press(Message::AnnoObjectScaleOpen)
        .style(button::secondary)
        .padding([2, 7]);

    let controls = row![
        field,
        manage,
        iced::widget::space().width(10)
    ]
    .spacing(2)
    .align_y(iced::Center)
    .width(Length::Fill);

    prop_row_widget(label, controls.into())
}
fn render_ro_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    // A read-only value is shown as a non-editable but selectable field: no
    // on_input means the caret never appears, but the text can be selected
    // (carrying the full, un-truncated value) and copied with Ctrl+C.
    let field = crate::ui::read_only::field(value, FONT_SZ, Length::Fill);
    prop_row_widget(label, field)
}

/// Build a label | widget property row.
fn prop_row_widget<'a>(label: &'a str, widget: Element<'a, Message>) -> Element<'a, Message> {
    prop_row_with_active(label, widget, false)
}

/// Like [`prop_row_widget`] but tints the row as the currently-active edit
/// row when `active` is true, using the palette's `primary.weak` highlight so
/// it reads as "selected" without shouting.
fn prop_row_with_active<'a>(
    label: &'a str,
    widget: Element<'a, Message>,
    active: bool,
) -> Element<'a, Message> {
    let label_col = container(
        text(crate::ui::text_util::elide(label, 18))
            .size(FONT_SZ)
            .style(muted_text_style),
    )
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(if active {
                theme.palette().primary.weak.color
            } else {
                theme.palette().background.weakest.color
            })),
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
        .style(move |theme: &Theme| container::Style {
            background: Some(Background::Color(if active {
                theme.palette().primary.weak.color
            } else {
                theme.palette().background.base.color
            })),
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
        .style(move |theme: &Theme| container::Style {
            border: Border {
                color: if active {
                    theme.palette().primary.base.color
                } else {
                    theme.palette().background.neutral.color
                },
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// A plain text button used inside the color picker for ByLayer / ByBlock.
fn picker_text_btn(label: &str, msg: Message) -> Element<'_, Message> {
    button(text(t!(label)).size(FONT_SZ))
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
    let palette = theme.palette();
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
    let palette = theme.palette();
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

fn muted_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.72)),
    }
}

fn hint_text_style(theme: &Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().background.base.text.scale_alpha(0.48)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        active_key_focused, attr_edit_key, build_field_key_map, hatch_pattern_matches,
        hatch_preview_scale, prop_attr_field_id, prop_geom_field_id, FieldKey,
    };

    fn sample_sections() -> Vec<crate::scene::model::object::PropSection> {
        use crate::scene::model::object::{Property, PropValue};
        vec![
            crate::scene::model::object::PropSection {
                title: "Geometry".into(),
                props: vec![Property {
                    label: "X".into(),
                    field: "pos_x",
                    value: PropValue::EditText("1.0".into()),
                }],
            },
            crate::scene::model::object::PropSection {
                title: "Attributes".into(),
                props: vec![Property {
                    label: "TITLE".into(),
                    field: "0",
                    value: PropValue::AttrText {
                        tag: "TITLE".into(),
                        value: "hello".into(),
                    },
                }],
            },
            crate::scene::model::object::PropSection {
                title: "Reference".into(),
                props: vec![Property {
                    label: "Name".into(),
                    field: "name",
                    value: PropValue::EditChoice {
                        value: "Door".into(),
                        options: vec!["Door".into(), "Window".into()],
                    },
                }],
            },
        ]
    }

    #[test]
    fn focus_transition_updates_the_active_field_highlight() {
        // The highlight is keyed to real keyboard focus: it persists only while
        // the focused widget is THAT field's text input, and clears when focus
        // moves to any other widget (or nowhere).
        //
        // Focus on the active field's own input → row stays marked active.
        let focused = prop_geom_field_id("pos_x");
        assert!(active_key_focused(Some(&FieldKey::Geom("pos_x")), Some(&focused)));

        // Focus moves to another property field → superseded (cleared here; the
        // manual focus-setter then marks the new field active).
        assert!(!active_key_focused(
            Some(&FieldKey::Geom("pos_x")),
            Some(&prop_geom_field_id("pos_y"))
        ));

        // Focus moves to a non-property widget / nothing → cleared.
        assert!(!active_key_focused(
            Some(&FieldKey::Geom("pos_x")),
            Some(&prop_attr_field_id("TITLE"))
        ));
        assert!(!active_key_focused(Some(&FieldKey::Geom("pos_x")), None));

        // No active field (or one whose input isn't focused) never reads active.
        assert!(!active_key_focused(None, Some(&prop_geom_field_id("pos_x"))));
    }

    #[test]
    fn field_ids_are_distinct_across_geometries_and_attributes() {
        assert_ne!(prop_geom_field_id("pos_x"), prop_geom_field_id("pos_y"));
        assert_ne!(prop_geom_field_id("pos_x"), prop_attr_field_id("TITLE"));
        assert_eq!(prop_attr_field_id("TITLE"), prop_attr_field_id("TITLE"));
    }

    #[test]
    fn prop_field_key_for_id_maps_focus_ids_to_active_keys() {
        let sections = sample_sections();
        let map = build_field_key_map(&sections);

        // A focused geometry edit field maps back to its `&'static str` key,
        // and round-trips through `active_key_focused`.
        let geom_id = prop_geom_field_id("pos_x");
        assert_eq!(map.get(&geom_id), Some(&FieldKey::Geom("pos_x")));
        assert!(active_key_focused(map.get(&geom_id), Some(&geom_id)));

        // A focused attribute edit field maps back to its tag key.
        let attr_id = prop_attr_field_id("TITLE");
        assert_eq!(map.get(&attr_id), Some(&attr_edit_key("TITLE")));
        assert!(active_key_focused(map.get(&attr_id), Some(&attr_id)));

        // The block-Name caret-dropdown is editable but deliberately excluded.
        assert_eq!(map.get(&prop_geom_field_id("name")), None);

        // Unknown or non-property ids map to nothing.
        assert_eq!(map.get(&prop_geom_field_id("nope")), None);
        assert_eq!(map.get(&iced::widget::Id::new("elsewhere")), None);
    }

    #[test]
    fn hatch_picker_filters_names_and_descriptions() {
        let ansi31 = crate::scene::model::hatch_patterns::find("ANSI31").unwrap();

        assert!(hatch_pattern_matches(ansi31, "ansi"));
        assert!(hatch_pattern_matches(ansi31, &ansi31.description));
        assert!(!hatch_pattern_matches(ansi31, "definitely-not-a-pattern"));
    }

    #[test]
    fn hatch_preview_scale_is_finite_and_visible() {
        let ansi31 = crate::scene::model::hatch_patterns::find("ANSI31").unwrap();
        let scale = hatch_preview_scale(&ansi31.gpu);

        assert!(scale.is_finite());
        assert!((0.01..=100.0).contains(&scale));
    }
}
