//! Graphic Attributes palette: line attributes and the associative fill of
//! the selected objects.

use crate::app::config::DockSide;
use crate::app::{ColorPickTarget, Message};
use crate::scene::Scene;
use crate::ui::dock::{DockMsg, PanelId};
use crate::ui::window::gradient_editor::{GradientEditorState, GradientMsg};
use acadrust::entities::{EntityCommon, Hatch, HatchGradientPattern};
use acadrust::types::{Color as AcadColor, LineWeight, Transparency};
use acadrust::{CadDocument, EntityType, Handle};
use iced::widget::{
    button, image, column, combo_box, container, mouse_area, row, slider, text, text_input, tooltip,
    Space,
};
use iced::{Background, Border, Element, Fill, Length, Padding, Theme};
use rustc_hash::{FxHashMap, FxHashSet};
use std::cell::RefCell;

type DrawDepth = FxHashMap<u64, [f32; 2]>;

const PALETTE_CONTROL_ICON_SIZE: f32 = 20.0;
const COMPACT_BUTTON_HEIGHT: f32 = 22.0;
const TRANSPARENCY_ICON: &[u8] =
    include_bytes!("../../../assets/icons/attributes/transparency.svg");
const LINEWEIGHT_ICON: &[u8] = include_bytes!("../../../assets/icons/attributes/lineweight.svg");

/// A usable object linetype scale (CELTSCALE): any positive finite value,
/// rounded to six decimals so arithmetic noise does not reach the drawing.
/// Typed values such as the ISO pen widths 0.13, 0.18, 0.25 and 0.35 are
/// kept as entered.
pub(crate) fn normalized_linetype_scale(value: f64) -> Option<f64> {
    let value = (value * 1.0e6).round() / 1.0e6;
    (value.is_finite() && value > 0.0).then_some(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphicAttribute {
    Varies,
    None,
    Solid,
    Hatch,
    Gradient,
}

impl GraphicAttribute {
    pub const ALL: [Self; 4] = [Self::None, Self::Solid, Self::Hatch, Self::Gradient];

    fn of(hatch: &Hatch) -> Self {
        if hatch.gradient_color.enabled {
            Self::Gradient
        } else if hatch.is_solid {
            Self::Solid
        } else {
            Self::Hatch
        }
    }

    fn icon(self) -> &'static [u8] {
        match self {
            Self::Varies | Self::None => {
                include_bytes!("../../../assets/icons/hatch/hatch_none.svg")
            }
            Self::Solid => include_bytes!("../../../assets/icons/hatch/hatch_solid.svg"),
            Self::Hatch => include_bytes!("../../../assets/icons/hatch/hatch_lines.svg"),
            Self::Gradient => include_bytes!("../../../assets/icons/hatch/hatch_gradient.svg"),
        }
    }
}

impl std::fmt::Display for GraphicAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            Self::Varies => "*VARIES*".into(),
            Self::None => crate::t!("None"),
            Self::Solid => crate::t!("Solid"),
            Self::Hatch => crate::t!("Hatch"),
            Self::Gradient => crate::t!("Gradient"),
        })
    }
}

/// The palette's drop-down menus; at most one is open at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Menu {
    Header,
    Fill,
    LineColor,
    LineLinetype,
    LineLineweight,
    LineTransparency,
    FillColor,
    FillTransparency,
}

#[derive(Debug, Clone)]
pub enum GraphicAttributesMsg {
    ToggleMenu(Menu),
    CloseMenu,
    SetAllByLayer,
    SetAllByBlock,
    RemoveReferences,
    CreateLayer,
    /// Put new fills on the current layer instead of their object's layer.
    ToggleFillOnCurrentLayer,
    Fill(GraphicAttribute),
    LineColor(AcadColor),
    LineLinetype(String),
    LineLineweight(LineWeight),
    LineLinetypeScale(f64),
    /// Text typed in the linetype scale field, applied on Enter.
    LineLinetypeScaleInput(String),
    LineLinetypeScaleSubmit,
    LineTransparency(Transparency),
    FillColor(AcadColor),
    FillTransparency(Transparency),
    HatchEditorOpen,
    HatchEditorClose,
    HatchPatternSearch(String),
    HatchPatternFocus(usize),
    HatchPattern(String),
    HatchPatternConfirm,
    Gradient(GradientMsg),
}

fn msg(message: GraphicAttributesMsg) -> Message {
    Message::GraphicAttributes(message)
}

/// The hatch pattern flyout, opened for one selection.
#[derive(Debug)]
pub struct HatchEditorState {
    pub handles: Vec<Handle>,
    pub search: String,
    pub focus: usize,
}

/// A fill request waiting for the user to decide whether its open
/// boundaries are closed first.
#[derive(Debug)]
pub struct PendingFillClose {
    pub fill: GraphicAttribute,
    pub open_boundaries: Vec<Handle>,
    pub selection: Vec<Handle>,
}

#[derive(Debug, Default)]
pub struct GraphicAttributesState {
    pub open_menu: Option<Menu>,
    pub hatch_editor: Option<HatchEditorState>,
    pub gradient_editor: Option<GradientEditorState>,
    pub pending_fill_close: Option<PendingFillClose>,
    /// New fills go to the current layer instead of their object's layer.
    pub fill_on_current_layer: bool,
    /// Text typed in the linetype scale field, until Enter applies it.
    pub linetype_scale_input: Option<String>,
    /// Whether the linetype scale field has focus (its value was selected).
    pub linetype_scale_focused: bool,
    /// View-side caches; `view` runs after every message, so neither the
    /// fill lookup nor the gradient preview is rebuilt unless its input changed.
    fill_index: RefCell<Option<(u64, FillIndex)>>,
    gradient_preview: RefCell<Option<(HatchGradientPattern, Option<image::Handle>)>>,
}

impl GraphicAttributesState {
    pub fn close_popups(&mut self) {
        self.open_menu = None;
        self.hatch_editor = None;
        self.gradient_editor = None;
    }

    /// The fills and fill kind of `selected`, from an index of associative
    /// fills that is rebuilt only when the scene's geometry changes.
    pub(crate) fn fills(&self, scene: &Scene, selected: &[Handle]) -> (Vec<Handle>, GraphicAttribute) {
        // Geometry epochs are unique across all scenes, so the epoch alone
        // identifies both the tab and the state of its drawing.
        let key = scene.geometry_epoch;
        let mut cache = self.fill_index.borrow_mut();
        if cache.as_ref().is_none_or(|(cached, _)| *cached != key) {
            *cache = Some((key, FillIndex::build(&scene.document)));
        }
        let index = &cache.as_ref().expect("fill index was just built").1;
        (
            index.fill_handles(&scene.document, selected),
            index.current(&scene.document, selected),
        )
    }

    fn gradient_preview(&self, hatch: &Hatch) -> Element<'static, Message> {
        let mut cache = self.gradient_preview.borrow_mut();
        if cache
            .as_ref()
            .is_none_or(|(gradient, _)| *gradient != hatch.gradient_color)
        {
            *cache = Some((
                hatch.gradient_color.clone(),
                crate::ui::window::gradient_editor::compact_preview(hatch),
            ));
        }
        let handle = cache.as_ref().and_then(|(_, handle)| handle.clone());
        crate::ui::window::gradient_editor::preview_image(handle)
    }

    /// Whether a flyout or question is bound to the selection it was opened for.
    pub fn tracks_selection(&self) -> bool {
        self.hatch_editor.is_some()
            || self.gradient_editor.is_some()
            || self.pending_fill_close.is_some()
    }

    /// Drop flyouts and questions that were opened for another selection.
    pub fn retain_selection(&mut self, selected: &[Handle]) {
        let same = |handles: &[Handle]| {
            handles.len() == selected.len() && selected.iter().all(|h| handles.contains(h))
        };
        if self.gradient_editor.as_ref().is_some_and(|e| !same(&e.handles)) {
            self.gradient_editor = None;
        }
        if self.hatch_editor.as_ref().is_some_and(|e| !same(&e.handles)) {
            self.hatch_editor = None;
        }
        if self.pending_fill_close.as_ref().is_some_and(|p| !same(&p.selection)) {
            self.pending_fill_close = None;
        }
    }

    pub fn fill_close_prompt(&self) -> Option<String> {
        let count = self.pending_fill_close.as_ref()?.open_boundaries.len();
        Some(if count == 1 {
            crate::t!("The selected object is open. Close it before creating the fill? [Yes / No] <Yes>")
                .into_owned()
        } else {
            crate::tf!(
                "{count} selected objects are open. Close them before creating the fill? [Yes / No] <Yes>"
            )
            .into_owned()
        })
    }
}

/// Associative HATCH entities per boundary object, in document order.
#[derive(Debug, Default)]
pub(crate) struct FillIndex {
    by_boundary: FxHashMap<Handle, Vec<Handle>>,
}

impl FillIndex {
    pub(crate) fn build(document: &CadDocument) -> Self {
        let mut by_boundary: FxHashMap<Handle, Vec<Handle>> = FxHashMap::default();
        for entity in document.entities() {
            let EntityType::Hatch(hatch) = entity else {
                continue;
            };
            if !hatch.is_associative {
                continue;
            }
            for boundary in hatch.paths.iter().flat_map(|path| &path.boundary_handles) {
                let fills = by_boundary.entry(*boundary).or_default();
                if !fills.contains(&hatch.common.handle) {
                    fills.push(hatch.common.handle);
                }
            }
        }
        Self { by_boundary }
    }

    /// The fill kind of each selected object: its own kind for a selected
    /// HATCH, otherwise the kind of the first associative HATCH it bounds.
    pub(crate) fn current(&self, document: &CadDocument, selected: &[Handle]) -> GraphicAttribute {
        let fill_of = |handle: &Handle| {
            let own = match document.get_entity(*handle) {
                Some(EntityType::Hatch(hatch)) => return GraphicAttribute::of(hatch),
                _ => None,
            };
            own.or_else(|| {
                self.by_boundary.get(handle)?.iter().find_map(|fill| {
                    match document.get_entity(*fill)? {
                        EntityType::Hatch(hatch) => Some(GraphicAttribute::of(hatch)),
                        _ => None,
                    }
                })
            })
            .unwrap_or(GraphicAttribute::None)
        };
        let Some((first, rest)) = selected.split_first() else {
            return GraphicAttribute::None;
        };
        let first = fill_of(first);
        if rest.iter().any(|handle| fill_of(handle) != first) {
            GraphicAttribute::Varies
        } else {
            first
        }
    }

    /// HATCH entities that are selected or associated with a selected boundary.
    pub(crate) fn fill_handles(&self, document: &CadDocument, selected: &[Handle]) -> Vec<Handle> {
        let mut seen = FxHashSet::default();
        let mut fills = Vec::new();
        for handle in selected {
            if matches!(document.get_entity(*handle), Some(EntityType::Hatch(_)))
                && seen.insert(*handle)
            {
                fills.push(*handle);
            }
            for fill in self.by_boundary.get(handle).into_iter().flatten() {
                if document.get_entity(*fill).is_some() && seen.insert(*fill) {
                    fills.push(*fill);
                }
            }
        }
        fills
    }
}

/// The fill kind of the selection; see [`FillIndex::current`].
#[cfg(test)]
pub(crate) fn current(document: &CadDocument, selected: &[Handle]) -> GraphicAttribute {
    FillIndex::build(document).current(document, selected)
}

/// HATCH entities that are selected or associated with a selected boundary.
/// Scans the whole drawing; the palette view uses its cached [`FillIndex`].
pub(crate) fn fill_handles(document: &CadDocument, selected: &[Handle]) -> Vec<Handle> {
    FillIndex::build(document).fill_handles(document, selected)
}

/// Selected non-HATCH objects plus the boundaries of selected associative
/// HATCH entities.
pub(crate) fn line_handles(document: &CadDocument, selected: &[Handle]) -> Vec<Handle> {
    let mut handles = Vec::new();
    let mut seen = FxHashSet::default();
    for handle in selected {
        match document.get_entity(*handle) {
            Some(EntityType::Hatch(hatch)) if hatch.is_associative => {
                for boundary in hatch.paths.iter().flat_map(|path| &path.boundary_handles) {
                    if seen.insert(*boundary)
                        && document
                            .get_entity(*boundary)
                            .is_some_and(|entity| !matches!(entity, EntityType::Hatch(_)))
                    {
                        handles.push(*boundary);
                    }
                }
            }
            Some(entity) if !matches!(entity, EntityType::Hatch(_)) && seen.insert(*handle) => {
                handles.push(*handle);
            }
            _ => {}
        }
    }
    handles
}

/// The handle drawn on top, which decides the value shown for a selection.
pub(crate) fn top_handle(handles: &[Handle], draw_depth: &DrawDepth) -> Option<Handle> {
    handles.iter().copied().max_by(|left, right| {
        let depth = |handle: &Handle| draw_depth.get(&handle.value()).map_or(0.0, |d| d[0]);
        depth(left).total_cmp(&depth(right))
    })
}

/// The value of the topmost object and whether any of `handles` differs.
fn shared_value<'a, T: PartialEq>(
    document: &'a CadDocument,
    handles: &[Handle],
    draw_depth: &DrawDepth,
    value: impl Fn(&EntityCommon) -> T,
) -> Option<(&'a EntityCommon, T, bool)> {
    let top = document.get_entity(top_handle(handles, draw_depth)?)?.common();
    let shared = value(top);
    let varies = handles.iter().any(|handle| {
        document
            .get_entity(*handle)
            .is_some_and(|entity| value(entity.common()) != shared)
    });
    Some((top, shared, varies))
}

fn current_layer_name(document: &CadDocument) -> &str {
    if document.header.current_layer_name.is_empty() {
        "0"
    } else {
        &document.header.current_layer_name
    }
}

/// Layer of the topmost object, or the current layer without a selection.
fn top_layer<'a>(document: &'a CadDocument, handles: &[Handle], draw_depth: &DrawDepth) -> &'a str {
    top_handle(handles, draw_depth)
        .and_then(|handle| document.get_entity(handle))
        .map_or_else(
            || current_layer_name(document),
            |entity| entity.common().layer.as_str(),
        )
}

#[derive(Clone, Copy)]
enum PropertyMode {
    ByLayer,
    ByBlock,
    Custom,
    Varies,
}

impl PropertyMode {
    fn button_label(self) -> &'static str {
        match self {
            Self::ByLayer => "L",
            Self::ByBlock => "B",
            Self::Custom => "C",
            Self::Varies => "V",
        }
    }
}

struct ColorInfo {
    color: AcadColor,
    display: iced::Color,
    varies: bool,
}

impl ColorInfo {
    fn mode(&self) -> PropertyMode {
        match (self.varies, self.color) {
            (true, _) => PropertyMode::Varies,
            (false, AcadColor::ByLayer) => PropertyMode::ByLayer,
            (false, AcadColor::ByBlock) => PropertyMode::ByBlock,
            _ => PropertyMode::Custom,
        }
    }
}

struct TransparencyInfo {
    value: u8,
    stored: Transparency,
    varies: bool,
}

impl TransparencyInfo {
    fn new(document: &CadDocument, layer: &str, stored: Transparency, varies: bool) -> Self {
        let effective = if stored.is_by_layer() {
            document
                .layers
                .get(layer)
                .map_or(stored, |layer| layer.transparency)
        } else if stored.is_by_block() {
            Transparency::from_percent(0.0)
        } else {
            stored
        };
        Self {
            value: (effective.as_percent() * 100.0).round().clamp(0.0, 90.0) as u8,
            stored,
            varies,
        }
    }

    fn mode(&self) -> PropertyMode {
        if self.varies {
            PropertyMode::Varies
        } else if self.stored.is_by_layer() {
            PropertyMode::ByLayer
        } else if self.stored.is_by_block() {
            PropertyMode::ByBlock
        } else {
            PropertyMode::Custom
        }
    }
}

fn color_info(document: &CadDocument, handles: &[Handle], draw_depth: &DrawDepth) -> Option<ColorInfo> {
    let (top, color, varies) = shared_value(document, handles, draw_depth, |c| c.color)?;
    let rgba =
        crate::scene::view::render::render_style_for_common_viewport(document, top, None).0;
    Some(ColorInfo {
        color,
        display: iced::Color::from_rgba(rgba[0], rgba[1], rgba[2], 1.0),
        varies,
    })
}

/// Line colour for new objects (CECOLOR) when nothing is selected.
fn current_color_info(document: &CadDocument) -> ColorInfo {
    let color = document.header.current_entity_color;
    let shown = match color {
        AcadColor::ByLayer => document
            .layers
            .get(current_layer_name(document))
            .map_or(color, |layer| layer.color),
        _ => color,
    };
    ColorInfo {
        color,
        display: crate::ui::properties::acad_color_display(shown).0,
        varies: false,
    }
}

fn transparency_info(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &DrawDepth,
) -> Option<TransparencyInfo> {
    let (top, stored, varies) = shared_value(document, handles, draw_depth, |c| c.transparency)?;
    Some(TransparencyInfo::new(document, &top.layer, stored, varies))
}

fn effective_lineweight(
    document: &CadDocument,
    handles: &[Handle],
    draw_depth: &DrawDepth,
    stored: LineWeight,
) -> LineWeight {
    if !matches!(stored, LineWeight::ByLayer) {
        return stored;
    }
    document
        .layers
        .get(top_layer(document, handles, draw_depth))
        .map_or(LineWeight::Default, |layer| layer.line_weight)
}

fn current_linetype_name(document: &CadDocument) -> &str {
    if !document.header.current_linetype_name.is_empty() {
        document.header.current_linetype_name.as_str()
    } else if !document.header.current_linetype_handle.is_null() {
        document
            .line_types
            .iter()
            .find(|line_type| line_type.handle == document.header.current_linetype_handle)
            .map_or("ByLayer", |line_type| line_type.name.as_str())
    } else {
        "ByLayer"
    }
}

/// The linetype actually drawn: ByLayer resolves to the layer's linetype,
/// ByBlock to Continuous.
fn effective_linetype<'a>(document: &'a CadDocument, layer: &str, logical: &'a str) -> &'a str {
    if logical.is_empty() || logical.eq_ignore_ascii_case("ByLayer") {
        document
            .layers
            .get(if layer.is_empty() { "0" } else { layer })
            .map_or("Continuous", |layer| layer.line_type.as_str())
    } else if logical.eq_ignore_ascii_case("ByBlock") {
        "Continuous"
    } else {
        logical
    }
}

fn is_continuous(name: &str) -> bool {
    name.eq_ignore_ascii_case("Continuous") || name.eq_ignore_ascii_case("Solid")
}

fn linetype_preview_art(document: &CadDocument, linetype: &str) -> String {
    if is_continuous(linetype) {
        return "_".repeat(80);
    }
    let art = document
        .line_types
        .iter()
        .find(|line_type| line_type.name.eq_ignore_ascii_case(linetype))
        .map(|line_type| {
            if line_type.elements.is_empty() {
                "____________".to_string()
            } else {
                crate::io::linetypes::extract_pattern(&line_type.description)
            }
        })
        .unwrap_or_default();
    let pattern_start = art.split_whitespace().find_map(|part| {
        (part
            .chars()
            .filter(|character| matches!(character, '_' | '.' | '-' | '/' | '\\' | '|' | '~'))
            .count()
            >= 2)
        .then(|| art.find(part))
        .flatten()
    });
    let pattern = pattern_start.map_or(art.as_str(), |start| &art[start..]);
    if pattern.is_empty() {
        return String::new();
    }
    let mut full_width = pattern.to_string();
    while full_width.chars().count() < 80 {
        full_width.push(' ');
        full_width.push_str(pattern);
    }
    full_width
}

/// The topmost selected fill of the given kind.
fn top_fill_hatch<'a>(
    document: &'a CadDocument,
    fills: &[Handle],
    draw_depth: &DrawDepth,
    kind: GraphicAttribute,
) -> Option<&'a Hatch> {
    let fills: Vec<_> = fills
        .iter()
        .copied()
        .filter(|handle| {
            matches!(document.get_entity(*handle),
                Some(EntityType::Hatch(hatch)) if GraphicAttribute::of(hatch) == kind)
        })
        .collect();
    match document.get_entity(top_handle(&fills, draw_depth)?)? {
        EntityType::Hatch(hatch) => Some(hatch),
        _ => None,
    }
}

fn gradients_vary(document: &CadDocument, fills: &[Handle]) -> bool {
    let mut gradients = fills
        .iter()
        .filter_map(|handle| match document.get_entity(*handle) {
            Some(EntityType::Hatch(hatch)) if hatch.gradient_color.enabled => {
                Some(&hatch.gradient_color)
            }
            _ => None,
        });
    let Some(first) = gradients.next() else {
        return false;
    };
    gradients.any(|gradient| gradient != first)
}

fn popup_alignment(side: DockSide) -> iced_aw::drop_down::Alignment {
    match side {
        DockSide::Left => iced_aw::drop_down::Alignment::BottomEnd,
        DockSide::Right => iced_aw::drop_down::Alignment::BottomStart,
    }
}

fn compact_button_content(label: &'static str) -> Element<'static, Message> {
    container(text(label).size(crate::ui::ROW_H * 0.42))
        .width(12)
        .height(Fill)
        .align_x(iced::Center)
        .align_y(iced::Center)
        .into()
}

fn popup_item<'a>(label: std::borrow::Cow<'a, str>, message: Message) -> Element<'a, Message> {
    button(text(label).size(11))
        .on_press(message)
        .style(crate::ui::color_select::list_row_style)
        .width(Fill)
        .padding([2, 4])
        .into()
}

/// The small L / B / C / V button next to a control, with its drop-down.
fn mode_menu<'a>(
    side: DockSide,
    mode: PropertyMode,
    menu: Menu,
    open_menu: Option<Menu>,
    height: f32,
    items: Vec<(std::borrow::Cow<'a, str>, Message)>,
) -> Element<'a, Message> {
    let open = open_menu == Some(menu);
    let more = button(compact_button_content(mode.button_label()))
        .on_press(msg(GraphicAttributesMsg::ToggleMenu(menu)))
        .style(move |theme: &Theme, status| {
            if open {
                button::primary(theme, status)
            } else {
                button::secondary(theme, status)
            }
        })
        .height(height)
        .padding([1, 3]);
    if !open {
        return more.into();
    }
    let popup = container(column(
        items
            .into_iter()
            .map(|(label, message)| popup_item(label, message)),
    ))
    .style(crate::ui::color_select::popup_panel_style)
    .padding(2);
    iced_aw::DropDown::new(more, popup, true)
        .width(Length::Fixed(110.0))
        .height(Length::Shrink)
        .alignment(popup_alignment(side))
        .offset(2.0)
        .on_dismiss(msg(GraphicAttributesMsg::CloseMenu))
        .into()
}

fn color_control<'a>(
    info: ColorInfo,
    side: DockSide,
    menu: Menu,
    open_menu: Option<Menu>,
    target: ColorPickTarget,
    changed: fn(AcadColor) -> GraphicAttributesMsg,
) -> Element<'a, Message> {
    // The L / B / C / V button already shows ByLayer, ByBlock and *VARIES*,
    // so the swatch carries no text.
    let display = info.display;
    let swatch = button(Space::new().width(Fill).height(Fill))
        .on_press(Message::OpenColorWindow(target.clone(), info.color))
        .width(Fill)
        .height(COMPACT_BUTTON_HEIGHT)
        .padding(0)
        .style(move |theme: &Theme, status| button::Style {
            background: Some(Background::Color(display)),
            border: Border {
                color: if matches!(status, button::Status::Hovered) {
                    theme.palette().primary.base.color
                } else {
                    theme.palette().background.neutral.color
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });
    let more = mode_menu(
        side,
        info.mode(),
        menu,
        open_menu,
        COMPACT_BUTTON_HEIGHT,
        vec![
            ("ByLayer".into(), msg(changed(AcadColor::ByLayer))),
            ("ByBlock".into(), msg(changed(AcadColor::ByBlock))),
            (crate::t!("Custom…"), Message::OpenColorWindow(target, info.color)),
        ],
    );
    row![swatch, more].spacing(4).align_y(iced::Center).into()
}

fn transparency_control<'a>(
    info: TransparencyInfo,
    side: DockSide,
    menu: Menu,
    open_menu: Option<Menu>,
    changed: fn(Transparency) -> GraphicAttributesMsg,
) -> Element<'a, Message> {
    let value_label = if info.varies {
        "*VARIES*".to_string()
    } else {
        format!("{}%", info.value)
    };
    let percent = |value: u8| Transparency::from_percent(f64::from(value) / 100.0);
    let control = row![
        crate::ui::icons::semantic(TRANSPARENCY_ICON, PALETTE_CONTROL_ICON_SIZE),
        text(value_label).size(10).width(38),
        slider(0..=90, i32::from(info.value), move |value| {
            msg(changed(percent(value as u8)))
        })
        .width(Fill),
    ]
    .spacing(5)
    .align_y(iced::Center)
    .height(crate::ui::ROW_H);
    let more = mode_menu(
        side,
        info.mode(),
        menu,
        open_menu,
        crate::ui::ROW_H,
        vec![
            ("ByLayer".into(), msg(changed(Transparency::BY_LAYER))),
            ("ByBlock".into(), msg(changed(Transparency::BY_BLOCK))),
            (crate::t!("Custom"), msg(changed(percent(info.value)))),
        ],
    );
    row![control, more].spacing(4).align_y(iced::Center).into()
}

fn header_menu(side: DockSide, open: bool, fill_on_current_layer: bool) -> Element<'static, Message> {
    let menu_button = button(crate::ui::icons::themed_secondary(
        crate::ui::icons::MENU,
        12.0,
    ))
    .on_press(msg(GraphicAttributesMsg::ToggleMenu(Menu::Header)))
    .style(move |theme: &Theme, status| {
        if open {
            button::primary(theme, status)
        } else {
            button::subtle(theme, status)
        }
    })
    .padding([3, 5]);
    if !open {
        return menu_button.into();
    }

    let item = |label: &'static str, message| {
        button(text(crate::t!(label)).size(11))
            .on_press(msg(message))
            .style(crate::ui::color_select::list_row_style)
            .width(Fill)
            .padding([4, 8])
    };
    let popup = container(column![
        item("Set all attributes ByLayer", GraphicAttributesMsg::SetAllByLayer),
        item("Set all attributes ByBlock", GraphicAttributesMsg::SetAllByBlock),
        item("Remove ByLayer / ByBlock", GraphicAttributesMsg::RemoveReferences),
        item("Create Layer with active settings", GraphicAttributesMsg::CreateLayer),
        container(iced::widget::rule::horizontal(1)).padding([2, 4]),
        container(
            iced::widget::checkbox(fill_on_current_layer)
                .label(crate::t!("Create fills on the current layer"))
                .on_toggle(|_| msg(GraphicAttributesMsg::ToggleFillOnCurrentLayer))
                .size(12)
                .text_size(11),
        )
        .padding([4, 8]),
    ])
    .style(crate::ui::color_select::popup_panel_style)
    .padding(2);
    iced_aw::DropDown::new(menu_button, popup, true)
        .width(Length::Fixed(220.0))
        .height(Length::Shrink)
        .alignment(popup_alignment(side))
        .offset(2.0)
        .on_dismiss(msg(GraphicAttributesMsg::CloseMenu))
        .into()
}

fn header<'a>(
    side: DockSide,
    auto_collapse: bool,
    menu_open: bool,
    fill_on_current_layer: bool,
) -> Element<'a, Message> {
    let pin_icon = if auto_collapse {
        crate::ui::icons::themed_primary_weak_text(crate::ui::icons::PIN, 12.0)
    } else {
        crate::ui::icons::themed_secondary(crate::ui::icons::PIN, 12.0)
    };
    let pin = button(pin_icon)
        .on_press(Message::Dock(DockMsg::AutoCollapseToggle(
            PanelId::GraphicAttributes,
        )))
        .style(button::subtle)
        .padding([3, 5]);
    let pin = tooltip(pin, text(crate::t!("Auto")).size(10), tooltip::Position::Bottom).gap(4);
    let close = button(crate::ui::icons::themed_secondary(
        crate::ui::icons::CLOSE,
        12.0,
    ))
    .on_press(Message::Dock(DockMsg::Close(PanelId::GraphicAttributes)))
    .style(button::subtle)
    .padding([3, 5]);
    let close = tooltip(close, text(crate::t!("Close")).size(10), tooltip::Position::Bottom).gap(4);
    mouse_area(
        container(
            row![
                text(crate::t!("Graphic Attributes")).size(12),
                Space::new().width(Fill),
                header_menu(side, menu_open, fill_on_current_layer),
                pin,
                close
            ]
            .spacing(3)
            .align_y(iced::Center),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(theme.palette().background.weak.color)),
            ..Default::default()
        })
        .width(Fill)
        .padding([3, 6]),
    )
    .on_press(Message::Dock(DockMsg::DockGrab(PanelId::GraphicAttributes)))
    .interaction(iced::mouse::Interaction::Grab)
    .into()
}

fn fill_picker<'a>(current: GraphicAttribute, open: bool) -> Element<'a, Message> {
    let picker = button(
        row![
            crate::ui::icons::semantic(current.icon(), 14.0),
            text(current.to_string()).size(11),
            Space::new().width(Fill),
            crate::ui::icons::themed_arrow_toggle(open, 9.0),
        ]
        .spacing(5)
        .align_y(iced::Center),
    )
    .on_press(msg(GraphicAttributesMsg::ToggleMenu(Menu::Fill)))
    .style(move |theme: &Theme, _| {
        let palette = theme.palette();
        button::Style {
            background: Some(Background::Color(palette.background.base.color)),
            text_color: palette.background.base.text,
            border: Border {
                color: if open {
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
    .height(crate::ui::ROW_H)
    .width(Fill)
    .padding([3, 6]);
    if !open {
        return picker.into();
    }
    let rows = column(GraphicAttribute::ALL.into_iter().map(|value| {
        button(
            row![
                crate::ui::icons::semantic(value.icon(), 14.0),
                text(value.to_string()).size(11),
            ]
            .spacing(5)
            .align_y(iced::Center),
        )
        .on_press(msg(GraphicAttributesMsg::Fill(value)))
        .style(crate::ui::color_select::list_row_style)
        .width(Fill)
        .padding([2, 4])
        .into()
    }));
    let popup = container(rows)
        .style(crate::ui::color_select::popup_panel_style)
        .padding(2);
    crate::ui::color_select::drop_down_below(
        picker.into(),
        popup.into(),
        None,
        Length::Shrink,
        msg(GraphicAttributesMsg::CloseMenu),
    )
}

/// A preview button plus the `⋮` button that toggles its editor flyout.
fn preview_row<'a>(
    preview: Element<'a, Message>,
    editor_open: bool,
    toggle: GraphicAttributesMsg,
    tooltip_label: std::borrow::Cow<'static, str>,
    flyout: Option<(Element<'a, Message>, f32, Option<f32>)>,
    side: DockSide,
) -> Element<'a, Message> {
    let preview = button(preview)
        .on_press(msg(toggle.clone()))
        .width(Fill)
        .height(COMPACT_BUTTON_HEIGHT)
        .padding(0)
        .style(move |theme: &Theme, status| button::Style {
            background: Some(Background::Color(theme.palette().background.base.color)),
            border: Border {
                color: if editor_open
                    || matches!(status, button::Status::Hovered | button::Status::Pressed)
                {
                    theme.palette().primary.base.color
                } else {
                    theme.palette().background.neutral.color
                },
                width: 1.0,
                radius: 2.0.into(),
            },
            ..Default::default()
        });
    let edit = button(compact_button_content("⋮"))
        .on_press(msg(toggle))
        .style(move |theme: &Theme, status| {
            if editor_open {
                button::primary(theme, status)
            } else {
                button::secondary(theme, status)
            }
        })
        .height(COMPACT_BUTTON_HEIGHT)
        .padding([1, 3]);
    let edit = tooltip(edit, text(tooltip_label).size(10), tooltip::Position::Bottom);
    let edit: Element<'a, Message> = match flyout {
        Some((popup, width, height)) => {
            let mut drop_down = iced_aw::DropDown::new(edit, popup, true)
                .width(Length::Fixed(width))
                .alignment(popup_alignment(side))
                .offset(4.0);
            if let Some(height) = height {
                drop_down = drop_down
                    .height(Length::Fixed(height))
                    .on_dismiss(msg(GraphicAttributesMsg::HatchEditorClose));
            }
            drop_down.into()
        }
        None => edit.into(),
    };
    row![preview, edit].spacing(4).align_y(iced::Center).into()
}

fn linetype_row<'a>(
    document: &'a CadDocument,
    properties: &'a crate::ui::properties::PropertiesPanel,
    linetype: &str,
    varies: bool,
    layer: &str,
    side: DockSide,
    open_menu: Option<Menu>,
) -> Element<'a, Message> {
    let selected = (!varies).then(|| crate::ui::properties::LinetypeItem {
        // An empty name shows only the preview art in the closed control;
        // the menu items keep their full names.
        name: String::new(),
        art: linetype_preview_art(document, effective_linetype(document, layer, linetype)),
    });
    let combo = combo_box(
        &properties.linetype_combo,
        "*VARIES*",
        selected.as_ref(),
        |item: crate::ui::properties::LinetypeItem| {
            msg(GraphicAttributesMsg::LineLinetype(item.name))
        },
    )
    .size(crate::ui::ROW_H * 0.42)
    .padding(Padding {
        top: 3.0,
        bottom: 3.0,
        left: 6.0,
        right: 6.0,
    })
    .input_style(crate::ui::properties::combo_input_style)
    .width(Fill);
    let mode = if varies {
        PropertyMode::Varies
    } else if linetype.eq_ignore_ascii_case("ByLayer") {
        PropertyMode::ByLayer
    } else if linetype.eq_ignore_ascii_case("ByBlock") {
        PropertyMode::ByBlock
    } else {
        PropertyMode::Custom
    };
    let more = mode_menu(
        side,
        mode,
        Menu::LineLinetype,
        open_menu,
        COMPACT_BUTTON_HEIGHT,
        vec![
            ("ByLayer".into(), msg(GraphicAttributesMsg::LineLinetype("ByLayer".into()))),
            ("ByBlock".into(), msg(GraphicAttributesMsg::LineLinetype("ByBlock".into()))),
        ],
    );
    row![crate::ui::wide_menu::wide_menu(combo, 220.0), more]
        .spacing(4)
        .align_y(iced::Center)
        .into()
}

/// Widget id of the linetype scale field, for select-all on focus.
pub(crate) const LINETYPE_SCALE_FIELD: &str = "graphic-linetype-scale";

/// Like a Properties field: clicking selects the value, Enter applies it.
fn linetype_scale_field<'a>(
    scale: f64,
    varies: bool,
    continuous: bool,
    typed: Option<&str>,
) -> Element<'a, Message> {
    let shown = match typed {
        Some(typed) => typed.to_string(),
        None if varies => String::new(),
        None => normalized_linetype_scale(scale).unwrap_or(1.0).to_string(),
    };
    let mut field = text_input(if varies { "*VARIES*" } else { "" }, &shown)
        .id(iced::widget::Id::new(LINETYPE_SCALE_FIELD))
        .size(crate::ui::ROW_H * 0.42)
        .padding([5, 5])
        .style(move |theme: &Theme, status| {
            let mut style = crate::ui::properties::combo_input_style(theme, status);
            if continuous {
                let disabled = theme.palette().background.base.text.scale_alpha(0.42);
                style.value = disabled;
                style.placeholder = disabled;
            }
            style
        })
        .width(Fill);
    if !continuous {
        field = field
            .on_input(|value| msg(GraphicAttributesMsg::LineLinetypeScaleInput(value)))
            .on_submit(msg(GraphicAttributesMsg::LineLinetypeScaleSubmit));
    }
    let icon = GraphicAttribute::Solid.icon();
    let icon = if continuous {
        crate::ui::icons::themed_disabled(icon, PALETTE_CONTROL_ICON_SIZE)
    } else {
        crate::ui::icons::semantic(icon, PALETTE_CONTROL_ICON_SIZE)
    };
    row![icon, field]
        .spacing(3)
        .align_y(iced::Center)
        .width(Length::FillPortion(1))
        .into()
}

fn lineweight_field<'a>(
    properties: &'a crate::ui::properties::PropertiesPanel,
    stored: LineWeight,
    effective: LineWeight,
    varies: bool,
    side: DockSide,
    open_menu: Option<Menu>,
) -> Element<'a, Message> {
    let selected = (!varies).then_some(crate::ui::properties::LwItem(effective));
    let mode = if varies {
        PropertyMode::Varies
    } else {
        match stored {
            LineWeight::ByLayer => PropertyMode::ByLayer,
            LineWeight::ByBlock => PropertyMode::ByBlock,
            _ => PropertyMode::Custom,
        }
    };
    let picker = combo_box(
        &properties.lineweight_combo,
        "*VARIES*",
        selected.as_ref(),
        |item: crate::ui::properties::LwItem| msg(GraphicAttributesMsg::LineLineweight(item.0)),
    )
    .size(crate::ui::ROW_H * 0.42)
    .padding(Padding {
        top: 3.0,
        bottom: 3.0,
        left: 6.0,
        right: 6.0,
    })
    .input_style(crate::ui::properties::combo_input_style)
    .width(Fill);
    let more = mode_menu(
        side,
        mode,
        Menu::LineLineweight,
        open_menu,
        COMPACT_BUTTON_HEIGHT,
        vec![
            ("ByLayer".into(), msg(GraphicAttributesMsg::LineLineweight(LineWeight::ByLayer))),
            ("ByBlock".into(), msg(GraphicAttributesMsg::LineLineweight(LineWeight::ByBlock))),
        ],
    );
    row![
        crate::ui::icons::semantic(LINEWEIGHT_ICON, PALETTE_CONTROL_ICON_SIZE),
        picker,
        more,
    ]
    .spacing(3)
    .align_y(iced::Center)
    .width(Length::FillPortion(1))
    .into()
}

/// The Line section: values of the selection's line objects, or the
/// creation defaults when nothing is selected.
fn line_section<'a>(
    document: &'a CadDocument,
    properties: &'a crate::ui::properties::PropertiesPanel,
    handles: &[Handle],
    no_selection: bool,
    draw_depth: &DrawDepth,
    side: DockSide,
    state: &GraphicAttributesState,
) -> Vec<Element<'a, Message>> {
    let open_menu = state.open_menu;
    let typed_scale = state.linetype_scale_input.as_deref();
    let mut rows = Vec::new();
    if handles.is_empty() && !no_selection {
        return rows;
    }
    let layer = top_layer(document, handles, draw_depth).to_string();
    let header = &document.header;
    let (linetype, linetype_varies) = shared_value(document, handles, draw_depth, |c| {
        c.linetype.clone()
    })
    .map_or_else(
        || (current_linetype_name(document).to_string(), false),
        |(_, value, varies)| (if value.is_empty() { "ByLayer".into() } else { value }, varies),
    );
    let (scale, scale_varies) =
        shared_value(document, handles, draw_depth, |c| c.linetype_scale).map_or(
            (header.current_entity_linetype_scale, false),
            |(_, value, varies)| (value, varies),
        );
    let (lineweight, lineweight_varies) =
        shared_value(document, handles, draw_depth, |c| c.line_weight).map_or(
            (LineWeight::from_value(header.current_line_weight), false),
            |(_, value, varies)| (value, varies),
        );
    let color = color_info(document, handles, draw_depth)
        .unwrap_or_else(|| current_color_info(document));
    let transparency = transparency_info(document, handles, draw_depth).unwrap_or_else(|| {
        TransparencyInfo::new(document, &layer, document.current_entity_transparency(), false)
    });
    let continuous =
        !linetype_varies && is_continuous(effective_linetype(document, &layer, &linetype));

    rows.push(linetype_row(
        document,
        properties,
        &linetype,
        linetype_varies,
        &layer,
        side,
        open_menu,
    ));
    rows.push(color_control(
        color,
        side,
        Menu::LineColor,
        open_menu,
        ColorPickTarget::GraphicAttributesLine,
        GraphicAttributesMsg::LineColor,
    ));
    let effective = effective_lineweight(document, handles, draw_depth, lineweight);
    rows.push(
        row![
            linetype_scale_field(scale, scale_varies, continuous, typed_scale),
            lineweight_field(
                properties,
                lineweight,
                effective,
                lineweight_varies,
                side,
                open_menu
            ),
        ]
        .spacing(12)
        .align_y(iced::Center)
        .height(crate::ui::ROW_H)
        .into(),
    );
    rows.push(transparency_control(
        transparency,
        side,
        Menu::LineTransparency,
        open_menu,
        GraphicAttributesMsg::LineTransparency,
    ));
    rows
}

pub fn view<'a>(
    state: &'a GraphicAttributesState,
    scene: &'a Scene,
    properties: &'a crate::ui::properties::PropertiesPanel,
    width: f32,
    auto_collapse: bool,
    side: DockSide,
    gradient_color_picking: bool,
) -> Element<'a, Message> {
    let document = &scene.document;
    let selected = scene.selected_handles_in_order();
    let draw_depth = scene.draw_depth_map();
    let draw_depth = draw_depth.as_ref();
    let open_menu = state.open_menu;
    let line_handles = line_handles(document, &selected);
    let (fill_handles, current) = state.fills(scene, &selected);

    let mut body = column![text(crate::t!("Line")).size(11)].spacing(6);
    body = body.extend(line_section(
        document,
        properties,
        &line_handles,
        selected.is_empty(),
        draw_depth,
        side,
        state,
    ));

    body = body
        .push(Space::new().height(2))
        .push(text(crate::t!("Fill")).size(11))
        .push(fill_picker(current, open_menu == Some(Menu::Fill)));
    if !matches!(current, GraphicAttribute::None | GraphicAttribute::Gradient) {
        if let Some(info) = color_info(document, &fill_handles, draw_depth) {
            body = body.push(color_control(
                info,
                side,
                Menu::FillColor,
                open_menu,
                ColorPickTarget::GraphicAttributesSolid,
                GraphicAttributesMsg::FillColor,
            ));
        }
    }
    match current {
        GraphicAttribute::Hatch => {
            let hatch = top_fill_hatch(document, &fill_handles, draw_depth, current);
            let preview = hatch
                .and_then(|hatch| Scene::hatch_model_from_dxf(hatch, [1.0; 4]))
                .map(|model| crate::ui::properties::compact_hatch_pattern_preview(model.pattern))
                .unwrap_or_else(|| Space::new().width(Fill).height(Fill).into());
            let flyout = state.hatch_editor.as_ref().map(|editor| {
                let picker = crate::ui::properties::hatch_pattern_picker_content(
                    &editor.search,
                    editor.focus,
                    hatch.map_or("", |hatch| hatch.pattern.name.as_str()),
                    crate::ui::properties::PatternPickerMessages {
                        search_id: "graphic-hatch-pattern-search",
                        on_search: |search| msg(GraphicAttributesMsg::HatchPatternSearch(search)),
                        on_confirm: msg(GraphicAttributesMsg::HatchPatternConfirm),
                        on_focus: |index| msg(GraphicAttributesMsg::HatchPatternFocus(index)),
                        on_changed: |name| msg(GraphicAttributesMsg::HatchPattern(name)),
                    },
                );
                let popup = container(picker)
                    .padding(1)
                    .style(|theme: &Theme| container::Style {
                        background: Some(Background::Color(theme.palette().background.base.color)),
                        border: Border {
                            color: theme.palette().primary.base.color,
                            width: 1.0,
                            radius: 3.0.into(),
                        },
                        ..Default::default()
                    });
                (popup.into(), 352.0, Some(724.0))
            });
            let open = state.hatch_editor.is_some();
            body = body.push(preview_row(
                preview,
                open,
                if open {
                    GraphicAttributesMsg::HatchEditorClose
                } else {
                    GraphicAttributesMsg::HatchEditorOpen
                },
                crate::t!("Edit hatch"),
                flyout,
                side,
            ));
        }
        GraphicAttribute::Gradient => {
            if let Some(hatch) = top_fill_hatch(document, &fill_handles, draw_depth, current) {
                let preview = state.gradient_preview(hatch);
                let preview = if gradients_vary(document, &fill_handles) {
                    iced::widget::stack![
                        preview,
                        container(text("*VARIES*").size(10))
                            .width(Fill)
                            .height(Fill)
                            .align_x(iced::Left)
                            .align_y(iced::Center)
                            .padding([0, 6]),
                    ]
                    .into()
                } else {
                    preview
                };
                let flyout = state.gradient_editor.as_ref().map(|editor| {
                    (
                        crate::ui::window::gradient_editor::view(editor, gradient_color_picking),
                        390.0,
                        None,
                    )
                });
                let open = state.gradient_editor.is_some();
                body = body.push(preview_row(
                    preview,
                    open,
                    GraphicAttributesMsg::Gradient(if open {
                        GradientMsg::Cancel
                    } else {
                        GradientMsg::Open
                    }),
                    crate::t!("Edit gradient"),
                    flyout,
                    side,
                ));
            }
        }
        GraphicAttribute::None | GraphicAttribute::Solid | GraphicAttribute::Varies => {}
    }
    if current != GraphicAttribute::None {
        if let Some(info) = transparency_info(document, &fill_handles, draw_depth) {
            body = body.push(transparency_control(
                info,
                side,
                Menu::FillTransparency,
                open_menu,
                GraphicAttributesMsg::FillTransparency,
            ));
        }
    }

    container(column![
        header(
            side,
            auto_collapse,
            open_menu == Some(Menu::Header),
            state.fill_on_current_layer
        ),
        body.padding(8)
    ])
    .width(Length::Fixed(width))
    .height(Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(Background::Color(theme.palette().background.base.color)),
        ..Default::default()
    })
    .into()
}
