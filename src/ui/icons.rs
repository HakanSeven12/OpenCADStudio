//! Shared SVG rendering for monochrome UI chrome and multi-colour tool icons.
//!
//! Dropdown carets and the undo/redo controls used to be drawn as Unicode
//! glyphs (`▾`, `▲`, `↶`, `↷`). Those depend on the active text font carrying
//! the glyph: on desktop the system fallback fonts supply them, but the web
//! build bundles only Fira Sans, which lacks them, so they rendered as empty
//! boxes. Drawing them from SVG instead makes the chrome font-independent.

use std::cell::RefCell;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::svg::{self as core_svg, Renderer as _};
use iced::advanced::widget::{Tree, Widget};
use iced::widget::{container, svg, Space};
use iced::{
    Color, ContentFit, Element, Length, Point, Radians, Rectangle, Renderer,
    Size, Theme,
};
use rustc_hash::FxHashMap;

const TRI_DOWN: &[u8] = include_bytes!("../../assets/icons/ui/tri_down.svg");
const TRI_UP: &[u8] = include_bytes!("../../assets/icons/ui/tri_up.svg");
const TRI_RIGHT: &[u8] = include_bytes!("../../assets/icons/ui/tri_right.svg");
const TRI_LEFT: &[u8] = include_bytes!("../../assets/icons/ui/tri_left.svg");
const HOME: &[u8] = include_bytes!("../../assets/icons/ui/home.svg");
const UNDO: &[u8] = include_bytes!("../../assets/icons/ui/undo.svg");
const REDO: &[u8] = include_bytes!("../../assets/icons/ui/redo.svg");

// OSNAP marker symbols. Rendered as SVG (not Unicode glyphs) so the snap menu
// shows the right shapes on the web build, whose bundled Fira Sans lacks the
// geometric glyphs and rendered them as tofu boxes. (#138)
const OSNAP_ENDPOINT: &[u8] = include_bytes!("../../assets/icons/osnap/endpoint.svg");
const OSNAP_MIDPOINT: &[u8] = include_bytes!("../../assets/icons/osnap/midpoint.svg");
const OSNAP_CENTER: &[u8] = include_bytes!("../../assets/icons/osnap/center.svg");
const OSNAP_NODE: &[u8] = include_bytes!("../../assets/icons/osnap/node.svg");
const OSNAP_QUADRANT: &[u8] = include_bytes!("../../assets/icons/osnap/quadrant.svg");
const OSNAP_INTERSECTION: &[u8] = include_bytes!("../../assets/icons/osnap/intersection.svg");
const OSNAP_EXTENSION: &[u8] = include_bytes!("../../assets/icons/osnap/extension.svg");
const OSNAP_INSERTION: &[u8] = include_bytes!("../../assets/icons/osnap/insertion.svg");
const OSNAP_PERPENDICULAR: &[u8] =
    include_bytes!("../../assets/icons/osnap/perpendicular.svg");
const OSNAP_TANGENT: &[u8] = include_bytes!("../../assets/icons/osnap/tangent.svg");
const OSNAP_NEAREST: &[u8] = include_bytes!("../../assets/icons/osnap/nearest.svg");
const OSNAP_APPARENT: &[u8] = include_bytes!("../../assets/icons/osnap/apparent.svg");
const OSNAP_PARALLEL: &[u8] = include_bytes!("../../assets/icons/osnap/parallel.svg");
const OSNAP_GRID: &[u8] = include_bytes!("../../assets/icons/osnap/grid.svg");

const LAY_ON: &[u8] = include_bytes!("../../assets/icons/layers/layon.svg");
const LAY_OFF: &[u8] = include_bytes!("../../assets/icons/layers/layoff.svg");
const LAY_FRZ: &[u8] = include_bytes!("../../assets/icons/layers/layfrz.svg");
const LAY_THW: &[u8] = include_bytes!("../../assets/icons/layers/laythw.svg");
const LAY_LCK: &[u8] = include_bytes!("../../assets/icons/layers/laylck.svg");
const LAY_ULK: &[u8] = include_bytes!("../../assets/icons/layers/layulk.svg");

// Monochrome chrome glyphs (replace Unicode glyphs in buttons / menus / toolbars).
// All are black-on-transparent; recolour them at the call site with [`tinted`].
pub const CHECK: &[u8] = include_bytes!("../../assets/icons/ui/check.svg");
pub const CLOSE: &[u8] = include_bytes!("../../assets/icons/ui/close.svg");
pub const PLUS: &[u8] = include_bytes!("../../assets/icons/ui/plus.svg");
pub const MINUS: &[u8] = include_bytes!("../../assets/icons/ui/minus.svg");
pub const TRASH: &[u8] = include_bytes!("../../assets/icons/ui/trash.svg");
pub const COPY: &[u8] = include_bytes!("../../assets/icons/ui/copy.svg");
pub const MENU: &[u8] = include_bytes!("../../assets/icons/ui/menu.svg");
pub const MOVE: &[u8] = include_bytes!("../../assets/icons/ui/move.svg");
pub const RESIZE: &[u8] = include_bytes!("../../assets/icons/ui/resize.svg");
pub const SPLIT_V: &[u8] = include_bytes!("../../assets/icons/ui/split_v.svg");
pub const SPLIT_H: &[u8] = include_bytes!("../../assets/icons/ui/split_h.svg");
pub const GRID: &[u8] = include_bytes!("../../assets/icons/ui/grid.svg");
pub const SNAP: &[u8] = include_bytes!("../../assets/icons/ui/snap.svg");
pub const DOC_NEW: &[u8] = include_bytes!("../../assets/icons/ui/doc_new.svg");
pub const FOLDER_OPEN: &[u8] = include_bytes!("../../assets/icons/ui/folder_open.svg");
pub const SAVE: &[u8] = include_bytes!("../../assets/icons/ui/save.svg");
pub const FILE_EXPORT: &[u8] = include_bytes!("../../assets/icons/ui/file_export.svg");
pub const PRINT: &[u8] = include_bytes!("../../assets/icons/ui/print.svg");
pub const HEART: &[u8] = include_bytes!("../../assets/icons/ui/heart.svg");
pub const DOT: &[u8] = include_bytes!("../../assets/icons/ui/dot.svg");
pub const DIRTY_DOT: &[u8] = include_bytes!("../../assets/icons/ui/dirty_dot.svg");
pub const ARROW_LONG_RIGHT: &[u8] = include_bytes!("../../assets/icons/ui/arrow_long_right.svg");

// ── Status-bar toggle icons (issue #216) ──────────────────────────────────
pub const ST_ORTHO: &[u8] = include_bytes!("../../assets/icons/status/ortho.svg");
pub const ST_POLAR: &[u8] = include_bytes!("../../assets/icons/status/polar.svg");
pub const ST_OSNAP: &[u8] = include_bytes!("../../assets/icons/status/osnap.svg");
pub const ST_OTRACK: &[u8] = include_bytes!("../../assets/icons/status/otrack.svg");
pub const ST_DYN: &[u8] = include_bytes!("../../assets/icons/status/dyn.svg");
pub const ST_LWT: &[u8] = include_bytes!("../../assets/icons/status/lwt.svg");
pub const ST_TRANSPARENCY: &[u8] = include_bytes!("../../assets/icons/status/transparency.svg");
pub const ST_ISOLATE: &[u8] = include_bytes!("../../assets/icons/status/isolate.svg");
pub const ST_QUICKPROPS: &[u8] = include_bytes!("../../assets/icons/status/quickprops.svg");
pub const ST_FILTER: &[u8] = include_bytes!("../../assets/icons/status/filter.svg");
pub const ST_SELCYCLE: &[u8] = include_bytes!("../../assets/icons/status/selcycle.svg");
pub const ST_CLEANSCREEN: &[u8] = include_bytes!("../../assets/icons/status/cleanscreen.svg");

// Tool SVGs share a small source palette. These colours are semantic rather
// than literal: cyan is the accent, pale grey is foreground, yellow is warning,
// and so on. `SemanticIcon` resolves those roles from Iced's active extended
// palette at draw time, retaining the artwork's multiple colours across themes.
const SEMANTIC_CACHE_LIMIT: usize = 2048;

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct SemanticCacheKey {
    address: usize,
    length: usize,
    palette: [[u8; 4]; 6],
}

thread_local! {
    static SEMANTIC_CACHE: RefCell<FxHashMap<SemanticCacheKey, svg::Handle>> =
        RefCell::new(FxHashMap::default());
}

#[derive(Clone, Copy)]
struct SemanticColors {
    background: [u8; 7],
    text: [u8; 7],
    primary_weak: [u8; 7],
    primary: [u8; 7],
    primary_strong: [u8; 7],
    secondary_weak: [u8; 7],
    secondary: [u8; 7],
    secondary_strong: [u8; 7],
    success_weak: [u8; 7],
    success: [u8; 7],
    warning: [u8; 7],
    warning_strong: [u8; 7],
    danger_weak: [u8; 7],
    danger: [u8; 7],
}

impl SemanticColors {
    fn from_theme(theme: &Theme) -> Self {
        let palette = theme.extended_palette();
        Self {
            background: color_hex(palette.background.strong.color),
            text: color_hex(palette.background.base.text),
            primary_weak: color_hex(palette.primary.weak.color),
            primary: color_hex(palette.primary.base.color),
            primary_strong: color_hex(palette.primary.strong.color),
            secondary_weak: color_hex(palette.secondary.weak.color),
            secondary: color_hex(palette.secondary.base.color),
            secondary_strong: color_hex(palette.secondary.strong.color),
            success_weak: color_hex(palette.success.weak.color),
            success: color_hex(palette.success.base.color),
            warning: color_hex(palette.warning.base.color),
            warning_strong: color_hex(palette.warning.strong.color),
            danger_weak: color_hex(palette.danger.weak.color),
            danger: color_hex(palette.danger.base.color),
        }
    }
}

struct SemanticIcon {
    bytes: &'static [u8],
    size: f32,
    opacity: f32,
}

impl<M> Widget<M, Theme, Renderer> for SemanticIcon {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.size), Length::Fixed(self.size))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(
            limits,
            Length::Fixed(self.size),
            Length::Fixed(self.size),
        )
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let handle = semantic_handle(self.bytes, theme);
        let measured = renderer.measure_svg(&handle);
        if measured.width == 0 || measured.height == 0 {
            return;
        }

        let image_size = Size::new(measured.width as f32, measured.height as f32);
        let bounds = layout.bounds();
        let fitted = ContentFit::Contain.fit(image_size, bounds.size());
        let position = Point::new(
            bounds.center_x() - fitted.width / 2.0,
            bounds.center_y() - fitted.height / 2.0,
        );

        renderer.draw_svg(
            core_svg::Svg {
                handle,
                color: None,
                rotation: Radians(0.0),
                opacity: self.opacity,
            },
            Rectangle::new(position, fitted),
            bounds,
        );
    }
}

/// Render a multi-colour tool icon using semantic colours from the active theme.
pub fn semantic<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    Element::new(SemanticIcon {
        bytes,
        size,
        opacity: 1.0,
    })
}

/// Render a disabled multi-colour tool icon without flattening its colour roles.
pub fn semantic_disabled<'a, M: 'a>(
    bytes: &'static [u8],
    size: f32,
) -> Element<'a, M> {
    Element::new(SemanticIcon {
        bytes,
        size,
        opacity: 0.42,
    })
}

fn semantic_handle(bytes: &'static [u8], theme: &Theme) -> svg::Handle {
    let key = SemanticCacheKey {
        address: bytes.as_ptr() as usize,
        length: bytes.len(),
        palette: palette_key(theme),
    };

    SEMANTIC_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(handle) = cache.get(&key) {
            return handle.clone();
        }

        let handle = svg::Handle::from_memory(recolor_semantic_svg(bytes, theme));
        if cache.len() >= SEMANTIC_CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, handle.clone());
        handle
    })
}

fn palette_key(theme: &Theme) -> [[u8; 4]; 6] {
    let palette = theme.palette();
    [
        palette.background.into_rgba8(),
        palette.text.into_rgba8(),
        palette.primary.into_rgba8(),
        palette.success.into_rgba8(),
        palette.warning.into_rgba8(),
        palette.danger.into_rgba8(),
    ]
}

fn recolor_semantic_svg(source: &[u8], theme: &Theme) -> Vec<u8> {
    let colors = SemanticColors::from_theme(theme);
    let mut output = Vec::with_capacity(source.len());
    let mut index = 0;

    while index < source.len() {
        if source[index] == b'#' {
            let mut end = index + 1;
            while end < source.len() && source[end].is_ascii_hexdigit() {
                end += 1;
            }
            let digit_count = end - index - 1;
            if matches!(digit_count, 3 | 4 | 6 | 8) {
                if let Some(replacement) =
                    semantic_color(&source[index..end], &colors)
                {
                    output.extend_from_slice(replacement);
                    index = end;
                    continue;
                }
            }
        } else if starts_with_word_ignore_ascii_case(source, index, b"white") {
            output.extend_from_slice(&colors.text);
            index += 5;
            continue;
        }

        output.push(source[index]);
        index += 1;
    }

    output
}

fn semantic_color<'a>(
    token: &[u8],
    colors: &'a SemanticColors,
) -> Option<&'a [u8; 7]> {
    if is_one_of(token, &["#e0e0e0", "#eeeeee", "#ffffff", "#e1e1e1"]) {
        Some(&colors.text)
    } else if is_one_of(token, &["#cccccc", "#bdbdbd", "#aaaaaa"]) {
        Some(&colors.secondary_strong)
    } else if is_one_of(
        token,
        &["#888888", "#888", "#9e9e9e", "#90a4ae", "#78909c", "#7a7a7a"],
    ) {
        Some(&colors.secondary)
    } else if is_one_of(
        token,
        &[
            "#505050", "#555", "#606060", "#666", "#616161", "#546e7a",
            "#455a64", "#37474f",
        ],
    ) {
        Some(&colors.secondary_weak)
    } else if is_one_of(token, &["#1a1a1a"]) {
        Some(&colors.background)
    } else if is_one_of(token, &["#4cc9f0", "#4bc8f0", "#4a9eff", "#0099e5"]) {
        Some(&colors.primary)
    } else if is_one_of(token, &["#1565c0"]) {
        Some(&colors.primary_strong)
    } else if is_one_of(token, &["#0d47a1"]) {
        Some(&colors.primary_weak)
    } else if is_one_of(token, &["#4ccf6f"]) {
        Some(&colors.success)
    } else if is_one_of(token, &["#00695c", "#004d40"]) {
        Some(&colors.success_weak)
    } else if is_one_of(token, &["#f0c040", "#ffd740", "#fdd835"]) {
        Some(&colors.warning)
    } else if is_one_of(token, &["#f9a825"]) {
        Some(&colors.warning_strong)
    } else if is_one_of(
        token,
        &[
            "#e05050", "#ef5350", "#e06c6c", "#e53935", "#ff0000", "#e10000",
        ],
    ) {
        Some(&colors.danger)
    } else if is_one_of(token, &["#b71c1c"]) {
        Some(&colors.danger_weak)
    } else {
        None
    }
}

fn is_one_of(token: &[u8], candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| token.eq_ignore_ascii_case(candidate.as_bytes()))
}

fn starts_with_word_ignore_ascii_case(
    source: &[u8],
    index: usize,
    word: &[u8],
) -> bool {
    let Some(end) = index.checked_add(word.len()) else {
        return false;
    };
    if end > source.len()
        || !source[index..end].eq_ignore_ascii_case(word)
        || index > 0 && source[index - 1].is_ascii_alphabetic()
        || end < source.len() && source[end].is_ascii_alphabetic()
    {
        return false;
    }
    true
}

fn color_hex(color: Color) -> [u8; 7] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let [red, green, blue, _] = color.into_rgba8();
    [
        b'#',
        HEX[(red >> 4) as usize],
        HEX[(red & 0x0f) as usize],
        HEX[(green >> 4) as usize],
        HEX[(green & 0x0f) as usize],
        HEX[(blue >> 4) as usize],
        HEX[(blue & 0x0f) as usize],
    ]
}

/// Render a chrome icon with the active Iced theme's normal text color.
pub fn themed<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(theme.extended_palette().background.base.text),
        })
        .into()
}

/// Render secondary chrome with the active Iced theme's text color.
pub fn themed_secondary<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(
                theme
                    .extended_palette()
                    .background
                    .base
                    .text
                    .scale_alpha(0.72),
            ),
        })
        .into()
}

/// Render disabled chrome with the active Iced theme's text color.
pub fn themed_disabled<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(
                theme
                    .extended_palette()
                    .background
                    .base
                    .text
                    .scale_alpha(0.42),
            ),
        })
        .into()
}

/// Render an emphasized chrome icon with the active Iced theme's primary color.
pub fn themed_primary<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(theme.extended_palette().primary.base.color),
        })
        .into()
}

/// Render a positive-state chrome icon with the active Iced theme's success color.
pub fn themed_success<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(theme.extended_palette().success.base.color),
        })
        .into()
}

/// Render a warning-state chrome icon from the active Iced theme.
pub fn themed_warning<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(theme.extended_palette().warning.base.color),
        })
        .into()
}

/// Render a destructive-state chrome icon from the active Iced theme.
pub fn themed_danger<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(theme.extended_palette().danger.base.color),
        })
        .into()
}

/// Render an icon with the foreground chosen for a danger-coloured surface.
pub fn themed_danger_text<'a, M: 'a>(bytes: &'static [u8], size: f32) -> Element<'a, M> {
    svg(svg::Handle::from_memory(bytes))
        .width(size)
        .height(size)
        .style(|theme: &Theme, _| svg::Style {
            color: Some(theme.extended_palette().danger.base.text),
        })
        .into()
}

/// Fixed-width check column colored from the active Iced theme.
pub fn themed_check_cell<'a, M: 'a>(active: bool) -> Element<'a, M> {
    let inner: Element<'a, M> = if active {
        themed_primary(CHECK, 11.0)
    } else {
        Space::new().width(0).into()
    };
    container(inner).width(Length::Fixed(14.0)).into()
}

/// SVG bytes for an OSNAP mode's marker symbol, for the snap menu. (#138)
pub fn osnap(snap: crate::snap::SnapType) -> &'static [u8] {
    use crate::snap::SnapType as S;
    match snap {
        S::Endpoint => OSNAP_ENDPOINT,
        S::Midpoint => OSNAP_MIDPOINT,
        S::Center => OSNAP_CENTER,
        S::Node => OSNAP_NODE,
        S::Quadrant => OSNAP_QUADRANT,
        S::Intersection => OSNAP_INTERSECTION,
        S::Extension => OSNAP_EXTENSION,
        S::Insertion => OSNAP_INSERTION,
        S::Perpendicular => OSNAP_PERPENDICULAR,
        S::Tangent => OSNAP_TANGENT,
        S::Nearest => OSNAP_NEAREST,
        S::ApparentIntersection => OSNAP_APPARENT,
        S::Parallel => OSNAP_PARALLEL,
        S::Grid => OSNAP_GRID,
        // Not shown in the snap menu; fall back to a neutral marker.
        S::ObjectPick => OSNAP_NEAREST,
    }
}

/// Layer visibility icon bytes (on / off).
pub fn layer_visible(visible: bool) -> &'static [u8] {
    if visible {
        LAY_ON
    } else {
        LAY_OFF
    }
}

/// Layer freeze icon bytes (frozen / thawed).
pub fn layer_freeze(frozen: bool) -> &'static [u8] {
    if frozen {
        LAY_FRZ
    } else {
        LAY_THW
    }
}

/// Layer lock icon bytes (locked / unlocked).
pub fn layer_lock(locked: bool) -> &'static [u8] {
    if locked {
        LAY_LCK
    } else {
        LAY_ULK
    }
}

pub fn themed_arrow_down<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed(TRI_DOWN, size)
}

pub fn themed_arrow_up<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed(TRI_UP, size)
}

pub fn themed_arrow_right<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed(TRI_RIGHT, size)
}

pub fn themed_arrow_left<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed(TRI_LEFT, size)
}

pub fn themed_primary_arrow_down<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed_primary(TRI_DOWN, size)
}

pub fn themed_secondary_arrow_down<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed_secondary(TRI_DOWN, size)
}

pub fn themed_disabled_arrow_down<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed_disabled(TRI_DOWN, size)
}

pub fn themed_home<'a, M: 'a>(size: f32) -> Element<'a, M> {
    themed(HOME, size)
}

/// Caret that flips up/down with `open`.
pub fn themed_arrow_toggle<'a, M: 'a>(open: bool, size: f32) -> Element<'a, M> {
    if open {
        themed_arrow_up(size)
    } else {
        themed_arrow_down(size)
    }
}

pub fn themed_undo<'a, M: 'a>(size: f32, enabled: bool) -> Element<'a, M> {
    if enabled {
        themed(UNDO, size)
    } else {
        themed_disabled(UNDO, size)
    }
}

pub fn themed_redo<'a, M: 'a>(size: f32, enabled: bool) -> Element<'a, M> {
    if enabled {
        themed(REDO, size)
    } else {
        themed_disabled(REDO, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_svg_uses_multiple_theme_roles() {
        let source = br##"<svg>
            <path stroke="#e0e0e0"/>
            <path fill="#4cc9f0"/>
            <path fill="#f0c040"/>
            <path fill="#e05050"/>
            <path fill="#4ccf6f"/>
            <path fill="#123456"/>
        </svg>"##;
        let theme = Theme::Dark;
        let colors = SemanticColors::from_theme(&theme);
        let themed = recolor_semantic_svg(source, &theme);

        assert!(contains(&themed, &colors.text));
        assert!(contains(&themed, &colors.primary));
        assert!(contains(&themed, &colors.warning));
        assert!(contains(&themed, &colors.danger));
        assert!(contains(&themed, &colors.success));
        assert!(contains(&themed, b"#123456"));
    }

    #[test]
    fn semantic_svg_maps_named_white() {
        let theme = Theme::Dark;
        let colors = SemanticColors::from_theme(&theme);
        let themed =
            recolor_semantic_svg(br##"<path stroke="white"/>"##, &theme);

        assert!(contains(&themed, &colors.text));
    }

    fn contains(source: &[u8], needle: &[u8]) -> bool {
        source
            .windows(needle.len())
            .any(|window| window == needle)
    }
}
