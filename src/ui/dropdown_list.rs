//! Shared keyboard-navigable dropdown components:
//! - Enhanced `pick_list` and `combo_box` drop-in replacements with `ArrowUp` / `ArrowDown`
//!   navigation, `Enter` selection, `Escape` dismissal, automatic scroll-into-view, and
//!   type-ahead search for non-searchable lists.
//! - `DropdownList` container widget for custom and ribbon dropdown panels, providing
//!   shared `ArrowUp` / `ArrowDown` navigation, `Enter` activation, `Escape` dismissal,
//!   type-ahead search (when no search box is present), and automatic scroll-into-view.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::text::paragraph;
use iced::advanced::text::{self, Text};
use iced::advanced::widget::operation::{self, Operation};
use iced::advanced::widget::tree::{self, Tree};
use iced::advanced::{Overlay, Shell, Widget};
use iced::alignment;
use iced::border;
use iced::keyboard;
use iced::keyboard::key::{self, Named};
use iced::mouse;
use iced::overlay;
use iced::time::{Duration, Instant};
use iced::touch;
use iced::widget::scrollable::{self, AbsoluteOffset, Scrollable};
use iced::widget::text_input::{self, TextInput};
use iced::window;
use iced::{
    Element, Event, Length, Padding, Pixels, Point, Rectangle, Size, Theme, Vector,
};

use std::borrow::Borrow;
use std::cell::{Cell, RefCell};
use std::fmt::Display;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

pub use iced::widget::combo_box::Catalog as ComboBoxCatalog;
pub use iced::widget::pick_list::{
    default, Catalog, Handle, Icon, Status, Style, StyleFn,
};

static DROPDOWN_CAPTURED_ARROW: AtomicBool = AtomicBool::new(false);
static DROPDOWN_CAPTURED_ESCAPE: AtomicBool = AtomicBool::new(false);

/// Record that an open dropdown captured an `ArrowUp` or `ArrowDown` key press,
/// preventing the global `CommandLineArrowProbe` from stepping command history.
pub fn mark_dropdown_arrow_captured() {
    DROPDOWN_CAPTURED_ARROW.store(true, Ordering::Relaxed);
}

/// Take and clear the dropdown arrow-capture flag.
pub fn take_dropdown_arrow_captured() -> bool {
    DROPDOWN_CAPTURED_ARROW.swap(false, Ordering::Relaxed)
}

/// Record that an open dropdown captured an `Escape` key press to close itself,
/// preventing the global shortcut handler from closing the parent modal dialog
/// or cancelling the active command.
pub fn mark_dropdown_escape_captured() {
    DROPDOWN_CAPTURED_ESCAPE.store(true, Ordering::Relaxed);
}

/// Take and clear the dropdown escape-capture flag.
pub fn take_dropdown_escape_captured() -> bool {
    DROPDOWN_CAPTURED_ESCAPE.swap(false, Ordering::Relaxed)
}

/// Type-ahead buffer for dropdown lists without a dedicated search input.
///
/// Accumulates typed characters within [`Self::TIMEOUT`] and matches option
/// labels case-insensitively (prefix match first, cycling when the same letter
/// is pressed repeatedly, with substring match as fallback).
#[derive(Debug, Clone, Default)]
pub struct TypeAhead {
    buffer: String,
    last_key: Option<Instant>,
}

impl TypeAhead {
    pub const TIMEOUT: Duration = Duration::from_millis(900);

    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.last_key = None;
    }

    pub fn buffer(&self) -> &str {
        &self.buffer
    }

    pub fn push_char<'a, I>(
        &mut self,
        ch: char,
        now: Instant,
        current: Option<usize>,
        labels: I,
    ) -> Option<usize>
    where
        I: IntoIterator<Item = &'a str>,
    {
        if self
            .last_key
            .is_some_and(|prev| now.saturating_duration_since(prev) > Self::TIMEOUT)
        {
            self.buffer.clear();
        }
        self.last_key = Some(now);
        self.buffer.push(ch);

        let items: Vec<&'a str> = labels.into_iter().collect();
        Self::find_match(&self.buffer, current, &items, true)
    }

    pub fn backspace<'a, I>(
        &mut self,
        now: Instant,
        current: Option<usize>,
        labels: I,
    ) -> Option<usize>
    where
        I: IntoIterator<Item = &'a str>,
    {
        if self
            .last_key
            .is_some_and(|prev| now.saturating_duration_since(prev) > Self::TIMEOUT)
        {
            self.buffer.clear();
            return None;
        }
        self.last_key = Some(now);
        self.buffer.pop();
        if self.buffer.is_empty() {
            return None;
        }
        let items: Vec<&'a str> = labels.into_iter().collect();
        Self::find_match(&self.buffer, current, &items, false)
    }

    fn find_match(
        raw_query: &str,
        current: Option<usize>,
        items: &[&str],
        advance_on_single: bool,
    ) -> Option<usize> {
        if items.is_empty() {
            return None;
        }
        let query = raw_query.to_lowercase();
        if query.is_empty() {
            return current;
        }

        let len = items.len();
        let mut chars = query.chars();
        let first_char = chars.next()?;
        let all_same_char = query.chars().count() > 1 && chars.all(|c| c == first_char);

        // If the user presses the same letter repeatedly (e.g. "lll"), first check
        // if an item literally starts with "lll"; if not, cycle to the next item
        // starting with "l".
        if all_same_char {
            let start = current.unwrap_or(0);
            for offset in 0..len {
                let idx = (start + offset) % len;
                if items[idx].trim().to_lowercase().starts_with(&query) {
                    return Some(idx);
                }
            }
            let prefix = first_char.to_string();
            let next_start = current.map_or(0, |c| (c + 1) % len);
            for offset in 0..len {
                let idx = (next_start + offset) % len;
                if items[idx].trim().to_lowercase().starts_with(&prefix) {
                    return Some(idx);
                }
            }
        }

        // Prefix match: if a single fresh character was typed, start searching
        // after `current` so pressing a letter when already on a matching item
        // moves to the next match (wrapping back to `current` if it's the only one).
        let start = if query.chars().count() == 1 && advance_on_single {
            current.map_or(0, |c| (c + 1) % len)
        } else {
            current.unwrap_or(0)
        };

        for offset in 0..len {
            let idx = (start + offset) % len;
            if items[idx].trim().to_lowercase().starts_with(&query) {
                return Some(idx);
            }
        }

        // Substring fallback: find any item containing `query`.
        for offset in 0..len {
            let idx = (start + offset) % len;
            if items[idx].to_lowercase().contains(&query) {
                return Some(idx);
            }
        }

        None
    }
}

/// Widget operation that scrolls the outermost [`Scrollable`] so `[row_top, row_bottom]`
/// (relative to the scrollable content's top edge) is within the visible viewport.
struct EnsureVisible {
    row_top: f32,
    row_bottom: f32,
    done: bool,
}

impl Operation for EnsureVisible {
    fn traverse(&mut self, _operate: &mut dyn FnMut(&mut dyn Operation)) {}

    fn scrollable(
        &mut self,
        _id: Option<& iced::advanced::widget::Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn operation::Scrollable,
    ) {
        if self.done {
            return;
        }
        self.done = true;

        if bounds.height <= 0.0 || content_bounds.height <= bounds.height {
            return;
        }

        let view_top = translation.y;
        let view_bottom = translation.y + bounds.height;

        if self.row_top < view_top {
            state.scroll_to(AbsoluteOffset {
                x: None,
                y: Some(self.row_top.max(0.0)),
            });
        } else if self.row_bottom > view_bottom {
            state.scroll_to(AbsoluteOffset {
                x: None,
                y: Some((self.row_bottom - bounds.height).max(0.0)),
            });
        }
    }
}

/// Scroll the outermost `Scrollable` inside `widget` so that `[row_top, row_bottom]`
/// is visible inside its viewport.
pub fn ensure_scrollable_row_visible<Message, Theme, Renderer>(
    widget: &mut (dyn Widget<Message, Theme, Renderer> + '_),
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &Renderer,
    row_top: f32,
    row_bottom: f32,
) where
    Renderer: iced::advanced::Renderer,
{
    let mut op = EnsureVisible {
        row_top,
        row_bottom,
        done: false,
    };
    widget.operate(tree, layout, renderer, &mut op);
}

// ── Enhanced Menu Overlay (used by PickList and ComboBox) ──────────────────

pub mod menu {
    use super::*;
    pub use iced::overlay::menu::{default, Catalog, Style, StyleFn};

    /// Local state of a dropdown [`Menu`].
    #[derive(Debug)]
    pub struct State {
        tree: Tree,
        pub(super) last_scrolled_option: Option<usize>,
        pub(super) type_ahead: TypeAhead,
    }

    impl State {
        pub fn new() -> Self {
            Self {
                tree: Tree::empty(),
                last_scrolled_option: None,
                type_ahead: TypeAhead::new(),
            }
        }
    }

    impl Default for State {
        fn default() -> Self {
            Self::new()
        }
    }

    /// A list of selectable options with keyboard navigation, auto-scrolling, and type-ahead.
    pub struct Menu<'a, 'b, T, Message, Theme = iced::Theme, Renderer = iced::Renderer>
    where
        Theme: Catalog,
        Renderer: text::Renderer,
        'b: 'a,
    {
        state: &'a mut State,
        options: &'a [T],
        hovered_option: &'a mut Option<usize>,
        to_string: &'a dyn Fn(&T) -> String,
        on_selected: Box<dyn FnMut(T) -> Message + 'a>,
        on_option_hovered: Option<&'a dyn Fn(T) -> Message>,
        on_escape: Option<Box<dyn FnMut() -> Option<Message> + 'a>>,
        keyboard_enabled: bool,
        width: f32,
        padding: Padding,
        text_size: Option<Pixels>,
        line_height: text::LineHeight,
        shaping: text::Shaping,
        ellipsis: text::Ellipsis,
        font: Option<Renderer::Font>,
        class: &'a <Theme as Catalog>::Class<'b>,
    }

    impl<'a, 'b, T, Message, Theme, Renderer> Menu<'a, 'b, T, Message, Theme, Renderer>
    where
        T: Clone,
        Message: 'a,
        Theme: Catalog + 'a,
        Renderer: text::Renderer + 'a,
        'b: 'a,
    {
        pub fn new(
            state: &'a mut State,
            options: &'a [T],
            hovered_option: &'a mut Option<usize>,
            to_string: &'a dyn Fn(&T) -> String,
            on_selected: impl FnMut(T) -> Message + 'a,
            on_option_hovered: Option<&'a dyn Fn(T) -> Message>,
            class: &'a <Theme as Catalog>::Class<'b>,
        ) -> Self {
            Self {
                state,
                options,
                hovered_option,
                to_string,
                on_selected: Box::new(on_selected),
                on_option_hovered,
                on_escape: None,
                keyboard_enabled: false,
                width: 0.0,
                padding: Padding::ZERO,
                text_size: None,
                line_height: text::LineHeight::default(),
                shaping: text::Shaping::default(),
                ellipsis: text::Ellipsis::default(),
                font: None,
                class,
            }
        }

        pub fn keyboard_navigation(
            mut self,
            on_escape: impl FnMut() -> Option<Message> + 'a,
        ) -> Self {
            self.keyboard_enabled = true;
            self.on_escape = Some(Box::new(on_escape));
            self
        }

        pub fn width(mut self, width: f32) -> Self {
            self.width = width;
            self
        }

        pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
            self.padding = padding.into();
            self
        }

        pub fn text_size(mut self, text_size: impl Into<Pixels>) -> Self {
            self.text_size = Some(text_size.into());
            self
        }

        pub fn line_height(mut self, line_height: impl Into<text::LineHeight>) -> Self {
            self.line_height = line_height.into();
            self
        }

        pub fn shaping(mut self, shaping: text::Shaping) -> Self {
            self.shaping = shaping;
            self
        }

        pub fn ellipsis(mut self, ellipsis: text::Ellipsis) -> Self {
            self.ellipsis = ellipsis;
            self
        }

        pub fn font(mut self, font: impl Into<Renderer::Font>) -> Self {
            self.font = Some(font.into());
            self
        }

        pub fn overlay(
            self,
            position: Point,
            viewport: Rectangle,
            target_height: f32,
            menu_height: Length,
        ) -> overlay::Element<'a, Message, Theme, Renderer> {
            overlay::Element::new(Box::new(MenuOverlay::new(
                position,
                viewport,
                self,
                target_height,
                menu_height,
            )))
        }
    }

    struct MenuOverlay<'a, 'b, Message, Theme, Renderer>
    where
        Theme: Catalog,
        Renderer: text::Renderer,
    {
        position: Point,
        viewport: Rectangle,
        tree: &'a mut Tree,
        list: Scrollable<'a, Message, Theme, Renderer>,
        hovered_option: Rc<Cell<Option<usize>>>,
        last_scrolled_option: &'a mut Option<usize>,
        option_count: usize,
        padding: Padding,
        text_size: Option<Pixels>,
        line_height: text::LineHeight,
        width: f32,
        target_height: f32,
        class: &'a <Theme as Catalog>::Class<'b>,
    }

    impl<'a, 'b, Message, Theme, Renderer> MenuOverlay<'a, 'b, Message, Theme, Renderer>
    where
        Message: 'a,
        Theme: Catalog + scrollable::Catalog + 'a,
        Renderer: text::Renderer + 'a,
        'b: 'a,
    {
        pub fn new<T>(
            position: Point,
            viewport: Rectangle,
            menu: Menu<'a, 'b, T, Message, Theme, Renderer>,
            target_height: f32,
            menu_height: Length,
        ) -> Self
        where
            T: Clone,
        {
            let Menu {
                state,
                options,
                hovered_option,
                to_string,
                on_selected,
                on_option_hovered,
                on_escape,
                keyboard_enabled,
                width,
                padding,
                font,
                text_size,
                line_height,
                shaping,
                ellipsis,
                class,
            } = menu;

            let option_count = options.len();
            let shared_hover = Rc::new(Cell::new(*hovered_option));
            let last_scrolled_cell = Rc::new(Cell::new(state.last_scrolled_option));

            let mut list = Scrollable::new(List {
                options,
                hovered_option,
                shared_hover: Rc::clone(&shared_hover),
                last_scrolled_cell: Rc::clone(&last_scrolled_cell),
                type_ahead: &mut state.type_ahead,
                to_string,
                on_selected,
                on_option_hovered,
                on_escape,
                keyboard_enabled,
                font,
                text_size,
                line_height,
                shaping,
                ellipsis,
                padding,
                class,
            })
            .height(menu_height);

            state.tree.diff(&mut list as &mut dyn Widget<_, _, _>);

            Self {
                position,
                viewport,
                tree: &mut state.tree,
                list,
                hovered_option: shared_hover,
                last_scrolled_option: &mut state.last_scrolled_option,
                option_count,
                padding,
                text_size,
                line_height,
                width,
                target_height,
                class,
            }
        }

        fn sync_scroll(&mut self, layout: Layout<'_>, renderer: &Renderer)
        where
            Renderer: iced::advanced::Renderer,
        {
            let current = self.hovered_option.get();
            if current != *self.last_scrolled_option {
                *self.last_scrolled_option = current;
                if let Some(idx) = current {
                    if idx < self.option_count {
                        let text_size = self.text_size.unwrap_or_else(|| renderer.default_size());
                        let option_height =
                            f32::from(self.line_height.to_absolute(text_size)) + self.padding.y();
                        let row_top = option_height * idx as f32;
                        let row_bottom = row_top + option_height;
                        ensure_scrollable_row_visible(
                            &mut self.list,
                            self.tree,
                            layout,
                            renderer,
                            row_top,
                            row_bottom,
                        );
                    }
                }
            }
        }
    }

    impl<'a, 'b, Message, Theme, Renderer> Overlay<Message, Theme, Renderer>
        for MenuOverlay<'a, 'b, Message, Theme, Renderer>
    where
        Message: 'a,
        Theme: Catalog + 'a,
        Renderer: text::Renderer + iced::advanced::Renderer + 'a,
        'b: 'a,
    {
        fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
            let space_below = bounds.height - (self.position.y + self.target_height);
            let space_above = self.position.y;

            let limits = layout::Limits::new(
                Size::ZERO,
                Size::new(
                    bounds.width - self.position.x,
                    if space_below > space_above {
                        space_below
                    } else {
                        space_above
                    },
                ),
            )
            .width(self.width);

            let node = self.list.layout(self.tree, renderer, &limits);
            let size = node.size();

            node.move_to(if space_below > space_above {
                self.position + Vector::new(0.0, self.target_height)
            } else {
                self.position - Vector::new(0.0, size.height)
            })
        }

        fn update(
            &mut self,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            shell: &mut Shell<'_, Message>,
        ) {
            self.sync_scroll(layout, renderer);

            let bounds = layout.bounds();
            let prev_scrolled = *self.last_scrolled_option;
            self.list
                .update(self.tree, event, layout, cursor, renderer, shell, &bounds);

            // If List::update changed hovered_option via keyboard or mouse, update scroll.
            if self.hovered_option.get() != prev_scrolled {
                self.sync_scroll(layout, renderer);
            }
        }

        fn mouse_interaction(
            &self,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            self.list
                .mouse_interaction(self.tree, layout, cursor, &self.viewport, renderer)
        }

        fn draw(
            &self,
            renderer: &mut Renderer,
            theme: &Theme,
            defaults: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
        ) {
            let bounds = layout.bounds();
            let style = Catalog::style(theme, self.class);

            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: style.border,
                    shadow: style.shadow,
                    ..renderer::Quad::default()
                },
                style.background,
            );

            self.list
                .draw(self.tree, renderer, theme, defaults, layout, cursor, &bounds);
        }
    }

    struct List<'a, 'b, T, Message, Theme, Renderer>
    where
        Theme: Catalog,
        Renderer: text::Renderer,
    {
        options: &'a [T],
        hovered_option: &'a mut Option<usize>,
        shared_hover: Rc<Cell<Option<usize>>>,
        last_scrolled_cell: Rc<Cell<Option<usize>>>,
        type_ahead: &'a mut TypeAhead,
        to_string: &'a dyn Fn(&T) -> String,
        on_selected: Box<dyn FnMut(T) -> Message + 'a>,
        on_option_hovered: Option<&'a dyn Fn(T) -> Message>,
        on_escape: Option<Box<dyn FnMut() -> Option<Message> + 'a>>,
        keyboard_enabled: bool,
        padding: Padding,
        text_size: Option<Pixels>,
        line_height: text::LineHeight,
        shaping: text::Shaping,
        ellipsis: text::Ellipsis,
        font: Option<Renderer::Font>,
        class: &'a <Theme as Catalog>::Class<'b>,
    }

    struct ListState {
        is_hovered: Option<bool>,
    }

    impl<T, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
        for List<'_, '_, T, Message, Theme, Renderer>
    where
        T: Clone,
        Theme: Catalog,
        Renderer: text::Renderer,
    {
        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<Option<bool>>()
        }

        fn state(&self) -> tree::State {
            tree::State::new(ListState { is_hovered: None })
        }

        fn size(&self) -> Size<Length> {
            Size {
                width: Length::Fill,
                height: Length::Shrink,
            }
        }

        fn layout(
            &mut self,
            _tree: &mut Tree,
            renderer: &Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            let text_size = self.text_size.unwrap_or_else(|| renderer.default_size());
            let text_line_height = self.line_height.to_absolute(text_size);

            let size = {
                let intrinsic = Size::new(
                    0.0,
                    (f32::from(text_line_height) + self.padding.y()) * self.options.len() as f32,
                );

                limits.resolve(Length::Fill, Length::Shrink, intrinsic)
            };

            layout::Node::new(size)
        }

        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            shell: &mut Shell<'_, Message>,
            _viewport: &Rectangle,
        ) {
            match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                    if cursor.is_over(layout.bounds()) {
                        if let Some(index) = *self.hovered_option {
                            if let Some(option) = self.options.get(index) {
                                shell.publish((self.on_selected)(option.clone()));
                                shell.capture_event();
                            }
                        }
                    }
                }
                Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                    if let Some(cursor_position) = cursor.position_in(layout.bounds()) {
                        let text_size = self.text_size.unwrap_or_else(|| renderer.default_size());
                        let option_height =
                            f32::from(self.line_height.to_absolute(text_size)) + self.padding.y();

                        let new_hovered_option = (cursor_position.y / option_height) as usize;
                        if new_hovered_option < self.options.len() {
                            if *self.hovered_option != Some(new_hovered_option) {
                                if let Some(option) = self.options.get(new_hovered_option) {
                                    if let Some(on_option_hovered) = self.on_option_hovered {
                                        shell.publish(on_option_hovered(option.clone()));
                                    }
                                    shell.request_redraw();
                                }
                            }

                            *self.hovered_option = Some(new_hovered_option);
                            // Do not auto-scroll on mouse hover.
                            self.last_scrolled_cell.set(Some(new_hovered_option));
                            self.shared_hover.set(Some(new_hovered_option));
                        }
                    }
                }
                Event::Touch(touch::Event::FingerPressed { .. }) => {
                    if let Some(cursor_position) = cursor.position_in(layout.bounds()) {
                        let text_size = self.text_size.unwrap_or_else(|| renderer.default_size());
                        let option_height =
                            f32::from(self.line_height.to_absolute(text_size)) + self.padding.y();

                        *self.hovered_option = Some((cursor_position.y / option_height) as usize);
                        self.shared_hover.set(*self.hovered_option);

                        if let Some(index) = *self.hovered_option {
                            if let Some(option) = self.options.get(index) {
                                shell.publish((self.on_selected)(option.clone()));
                                shell.capture_event();
                            }
                        }
                    }
                }
                Event::Keyboard(keyboard::Event::KeyPressed {
                    key,
                    modified_key,
                    modifiers,
                    ..
                }) if self.keyboard_enabled => {
                    let len = self.options.len();
                    match key.as_ref() {
                        keyboard::Key::Named(Named::ArrowDown) => {
                            if len > 0 {
                                let next = match *self.hovered_option {
                                    Some(i) if i + 1 < len => i + 1,
                                    _ => 0,
                                };
                                *self.hovered_option = Some(next);
                                self.shared_hover.set(Some(next));
                                if let Some(on_option_hovered) = self.on_option_hovered {
                                    if let Some(option) = self.options.get(next) {
                                        shell.publish(on_option_hovered(option.clone()));
                                    }
                                }
                            }
                            mark_dropdown_arrow_captured();
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::ArrowUp) => {
                            if len > 0 {
                                let prev = match *self.hovered_option {
                                    Some(0) | None => len - 1,
                                    Some(i) => i - 1,
                                };
                                *self.hovered_option = Some(prev);
                                self.shared_hover.set(Some(prev));
                                if let Some(on_option_hovered) = self.on_option_hovered {
                                    if let Some(option) = self.options.get(prev) {
                                        shell.publish(on_option_hovered(option.clone()));
                                    }
                                }
                            }
                            mark_dropdown_arrow_captured();
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::Home) => {
                            if len > 0 {
                                *self.hovered_option = Some(0);
                                self.shared_hover.set(Some(0));
                            }
                            mark_dropdown_arrow_captured();
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::End) => {
                            if len > 0 {
                                *self.hovered_option = Some(len - 1);
                                self.shared_hover.set(Some(len - 1));
                            }
                            mark_dropdown_arrow_captured();
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::PageDown) => {
                            if len > 0 {
                                let next = self
                                    .hovered_option
                                    .map_or(0, |i| (i + 8).min(len - 1));
                                *self.hovered_option = Some(next);
                                self.shared_hover.set(Some(next));
                            }
                            mark_dropdown_arrow_captured();
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::PageUp) => {
                            if len > 0 {
                                let prev = self
                                    .hovered_option
                                    .map_or(0, |i| i.saturating_sub(8));
                                *self.hovered_option = Some(prev);
                                self.shared_hover.set(Some(prev));
                            }
                            mark_dropdown_arrow_captured();
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::Enter) => {
                            if let Some(index) = *self.hovered_option {
                                if let Some(option) = self.options.get(index) {
                                    shell.publish((self.on_selected)(option.clone()));
                                }
                            }
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::Escape) => {
                            if let Some(on_escape) = &mut self.on_escape {
                                if let Some(msg) = on_escape() {
                                    shell.publish(msg);
                                }
                            }
                            mark_dropdown_escape_captured();
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        keyboard::Key::Named(Named::Backspace) => {
                            let labels: Vec<String> =
                                self.options.iter().map(self.to_string).collect();
                            if let Some(matched) = self.type_ahead.backspace(
                                Instant::now(),
                                *self.hovered_option,
                                labels.iter().map(String::as_str),
                            ) {
                                *self.hovered_option = Some(matched);
                                self.shared_hover.set(Some(matched));
                                if let Some(on_option_hovered) = self.on_option_hovered {
                                    if let Some(option) = self.options.get(matched) {
                                        shell.publish(on_option_hovered(option.clone()));
                                    }
                                }
                            }
                            shell.capture_event();
                            shell.request_redraw();
                        }
                        _ => {
                            if !modifiers.control() && !modifiers.alt() && !modifiers.logo() {
                                let ch = match modified_key.as_ref() {
                                    keyboard::Key::Character(s) => s.chars().next(),
                                    keyboard::Key::Named(Named::Space) => {
                                        (!self.type_ahead.buffer().is_empty()).then_some(' ')
                                    }
                                    _ => None,
                                };
                                if let Some(ch) = ch {
                                    if !ch.is_control() {
                                        let labels: Vec<String> =
                                            self.options.iter().map(self.to_string).collect();
                                        if let Some(matched) = self.type_ahead.push_char(
                                            ch,
                                            Instant::now(),
                                            *self.hovered_option,
                                            labels.iter().map(String::as_str),
                                        ) {
                                            *self.hovered_option = Some(matched);
                                            self.shared_hover.set(Some(matched));
                                            if let Some(on_option_hovered) = self.on_option_hovered {
                                                if let Some(option) = self.options.get(matched) {
                                                    shell.publish(on_option_hovered(option.clone()));
                                                }
                                            }
                                        }
                                        shell.capture_event();
                                        shell.request_redraw();
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }

            let state = tree.state.downcast_mut::<ListState>();

            if let Event::Window(window::Event::RedrawRequested(_now)) = event {
                state.is_hovered = Some(cursor.is_over(layout.bounds()));
            } else if state
                .is_hovered
                .is_some_and(|is_hovered| is_hovered != cursor.is_over(layout.bounds()))
            {
                shell.request_redraw();
            }
        }

        fn mouse_interaction(
            &self,
            _tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            _viewport: &Rectangle,
            _renderer: &Renderer,
        ) -> mouse::Interaction {
            if cursor.is_over(layout.bounds()) {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::default()
            }
        }

        fn draw(
            &self,
            _tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            _style: &renderer::Style,
            layout: Layout<'_>,
            _cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            let style = Catalog::style(theme, self.class);
            let bounds = layout.bounds();

            let text_size = self.text_size.unwrap_or_else(|| renderer.default_size());
            let option_height =
                f32::from(self.line_height.to_absolute(text_size)) + self.padding.y();

            let offset = (viewport.y - bounds.y).max(0.0);
            let start = (offset / option_height) as usize;
            let end = ((offset + viewport.height) / option_height).ceil() as usize;

            let visible_options = &self.options[start.min(self.options.len())..end.min(self.options.len())];

            for (i, option) in visible_options.iter().enumerate() {
                let i = start + i;
                let is_selected = *self.hovered_option == Some(i);

                let bounds = Rectangle {
                    x: bounds.x,
                    y: bounds.y + (option_height * i as f32),
                    width: bounds.width,
                    height: option_height,
                };

                if is_selected {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: Rectangle {
                                x: bounds.x + style.border.width,
                                width: bounds.width - style.border.width * 2.0,
                                ..bounds
                            },
                            border: border::rounded(style.border.radius),
                            ..renderer::Quad::default()
                        },
                        style.selected_background,
                    );
                }

                renderer.fill_text(
                    Text {
                        content: (self.to_string)(option),
                        bounds: Size::new(bounds.width - self.padding.x(), bounds.height),
                        size: text_size,
                        line_height: self.line_height,
                        font: self.font.unwrap_or_else(|| renderer.default_font()),
                        align_x: text::Alignment::Default,
                        align_y: alignment::Vertical::Center,
                        shaping: self.shaping,
                        wrapping: text::Wrapping::None,
                        ellipsis: self.ellipsis,
                        hint_factor: renderer.hint_factor(),
                    },
                    Point::new(bounds.x + self.padding.left, bounds.center_y()),
                    if is_selected {
                        style.selected_text_color
                    } else {
                        style.text_color
                    },
                    *viewport,
                );
            }
        }
    }

    impl<'a, 'b, T, Message, Theme, Renderer> From<List<'a, 'b, T, Message, Theme, Renderer>>
        for Element<'a, Message, Theme, Renderer>
    where
        T: Clone,
        Message: 'a,
        Theme: 'a + Catalog,
        Renderer: 'a + text::Renderer,
        'b: 'a,
    {
        fn from(list: List<'a, 'b, T, Message, Theme, Renderer>) -> Self {
            Element::new(list)
        }
    }
}

// ── Enhanced PickList ──────────────────────────────────────────────────────

/// Creates a new [`PickList`] with keyboard navigation (`ArrowUp`/`ArrowDown`, `Enter`,
/// `Escape`, and type-ahead search).
pub fn pick_list<'a, T, L, V, Message, Theme, Renderer>(
    selected: Option<V>,
    options: L,
    to_string: impl Fn(&T) -> String + 'a,
) -> PickList<'a, T, L, V, Message, Theme, Renderer>
where
    T: PartialEq + Clone + 'a,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
    Message: Clone,
    Theme: Catalog,
    Renderer: text::Renderer,
{
    PickList::new(selected, options, to_string)
}

pub struct PickList<'a, T, L, V, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    T: PartialEq + Clone,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
    Theme: Catalog,
    Renderer: text::Renderer,
{
    options: L,
    to_string: Box<dyn Fn(&T) -> String + 'a>,
    on_select: Option<Box<dyn Fn(T) -> Message + 'a>>,
    on_open: Option<Message>,
    on_close: Option<Message>,
    placeholder: Option<String>,
    selected: Option<V>,
    width: Length,
    padding: Padding,
    text_size: Option<Pixels>,
    line_height: text::LineHeight,
    shaping: text::Shaping,
    ellipsis: text::Ellipsis,
    font: Option<Renderer::Font>,
    handle: Handle<Renderer::Font>,
    class: <Theme as Catalog>::Class<'a>,
    menu_class: <Theme as menu::Catalog>::Class<'a>,
    last_status: Option<Status>,
    menu_height: Length,
}

impl<'a, T, L, V, Message, Theme, Renderer> PickList<'a, T, L, V, Message, Theme, Renderer>
where
    T: PartialEq + Clone,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
    Message: Clone,
    Theme: Catalog,
    Renderer: text::Renderer,
{
    pub fn new(selected: Option<V>, options: L, to_string: impl Fn(&T) -> String + 'a) -> Self {
        Self {
            to_string: Box::new(to_string),
            on_select: None,
            on_open: None,
            on_close: None,
            options,
            placeholder: None,
            selected,
            width: Length::Shrink,
            padding: iced::widget::button::DEFAULT_PADDING,
            text_size: None,
            line_height: text::LineHeight::default(),
            shaping: text::Shaping::default(),
            ellipsis: text::Ellipsis::End,
            font: None,
            handle: Handle::default(),
            class: <Theme as Catalog>::default(),
            menu_class: <Theme as Catalog>::default_menu(),
            last_status: None,
            menu_height: Length::Shrink,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn menu_height(mut self, menu_height: impl Into<Length>) -> Self {
        self.menu_height = menu_height.into();
        self
    }

    pub fn padding<P: Into<Padding>>(mut self, padding: P) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn text_size(mut self, size: impl Into<Pixels>) -> Self {
        self.text_size = Some(size.into());
        self
    }

    pub fn line_height(mut self, line_height: impl Into<text::LineHeight>) -> Self {
        self.line_height = line_height.into();
        self
    }

    pub fn shaping(mut self, shaping: text::Shaping) -> Self {
        self.shaping = shaping;
        self
    }

    pub fn ellipsis(mut self, ellipsis: text::Ellipsis) -> Self {
        self.ellipsis = ellipsis;
        self
    }

    pub fn font(mut self, font: impl Into<Renderer::Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    pub fn handle(mut self, handle: Handle<Renderer::Font>) -> Self {
        self.handle = handle;
        self
    }

    pub fn on_select(mut self, on_select: impl Fn(T) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }

    pub fn on_open(mut self, on_open: Message) -> Self {
        self.on_open = Some(on_open);
        self
    }

    pub fn on_close(mut self, on_close: Message) -> Self {
        self.on_close = Some(on_close);
        self
    }

    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self
    where
        <Theme as Catalog>::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }

    #[must_use]
    pub fn menu_style(mut self, style: impl Fn(&Theme) -> menu::Style + 'a) -> Self
    where
        <Theme as menu::Catalog>::Class<'a>: From<menu::StyleFn<'a, Theme>>,
    {
        self.menu_class = (Box::new(style) as menu::StyleFn<'a, Theme>).into();
        self
    }

    #[must_use]
    pub fn class(mut self, class: impl Into<<Theme as Catalog>::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    #[must_use]
    pub fn menu_class(mut self, class: impl Into<<Theme as menu::Catalog>::Class<'a>>) -> Self {
        self.menu_class = class.into();
        self
    }
}

#[derive(Debug)]
struct PickListState<P: text::Paragraph> {
    menu: menu::State,
    keyboard_modifiers: keyboard::Modifiers,
    is_open: bool,
    hovered_option: Option<usize>,
    options: Vec<paragraph::Plain<P>>,
    placeholder: paragraph::Plain<P>,
}

impl<P: text::Paragraph> PickListState<P> {
    fn new() -> Self {
        Self {
            menu: menu::State::default(),
            keyboard_modifiers: keyboard::Modifiers::default(),
            is_open: false,
            hovered_option: None,
            options: Vec::new(),
            placeholder: paragraph::Plain::default(),
        }
    }
}

impl<P: text::Paragraph> Default for PickListState<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, T, L, V, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for PickList<'a, T, L, V, Message, Theme, Renderer>
where
    T: Clone + PartialEq + 'a,
    L: Borrow<[T]>,
    V: Borrow<T>,
    Message: Clone + 'a,
    Theme: Catalog + 'a,
    Renderer: text::Renderer + 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<PickListState<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(PickListState::<Renderer::Paragraph>::new())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<PickListState<Renderer::Paragraph>>();

        let font = self.font.unwrap_or_else(|| renderer.default_font());
        let text_size = self.text_size.unwrap_or_else(|| renderer.default_size());
        let options = self.options.borrow();

        let option_text = Text {
            content: "",
            bounds: Size::new(
                limits.max().width,
                self.line_height.to_absolute(text_size).into(),
            ),
            size: text_size,
            line_height: self.line_height,
            font,
            align_x: text::Alignment::Default,
            align_y: alignment::Vertical::Center,
            shaping: self.shaping,
            wrapping: text::Wrapping::None,
            ellipsis: self.ellipsis,
            hint_factor: renderer.hint_factor(),
        };

        if let Some(placeholder) = &self.placeholder {
            let _ = state.placeholder.update(Text {
                content: placeholder,
                ..option_text
            });
        }

        let max_width = match self.width {
            Length::Shrink => {
                state.options.resize_with(options.len(), Default::default);

                for (option, paragraph) in options.iter().zip(state.options.iter_mut()) {
                    let label = (self.to_string)(option);

                    let _ = paragraph.update(Text {
                        content: &label,
                        ..option_text
                    });
                }

                let labels_width = state.options.iter().fold(0.0, |width, paragraph| {
                    f32::max(width, paragraph.min_width())
                });

                labels_width.max(
                    self.placeholder
                        .as_ref()
                        .map(|_| state.placeholder.min_width())
                        .unwrap_or(0.0),
                )
            }
            _ => 0.0,
        };

        let size = {
            let intrinsic = Size::new(
                max_width + text_size.0 + self.padding.left,
                f32::from(self.line_height.to_absolute(text_size)),
            );

            limits
                .width(self.width)
                .shrink(self.padding)
                .resolve(self.width, Length::Shrink, intrinsic)
                .expand(self.padding)
        };

        layout::Node::new(size)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<PickListState<Renderer::Paragraph>>();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if state.is_open {
                    state.is_open = false;
                    state.menu.type_ahead.clear();

                    if let Some(on_close) = &self.on_close {
                        shell.publish(on_close.clone());
                    }

                    shell.capture_event();
                } else if cursor.is_over(layout.bounds()) {
                    let selected = self.selected.as_ref().map(Borrow::borrow);
                    let options = self.options.borrow();

                    state.is_open = true;
                    state.hovered_option = options
                        .iter()
                        .position(|option| Some(option) == selected)
                        .or_else(|| (!options.is_empty()).then_some(0));
                    state.menu.last_scrolled_option = None;
                    state.menu.type_ahead.clear();

                    if let Some(on_open) = &self.on_open {
                        shell.publish(on_open.clone());
                    }

                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { y, .. },
            }) => {
                let Some(on_select) = &self.on_select else {
                    return;
                };

                if state.keyboard_modifiers.command()
                    && cursor.is_over(layout.bounds())
                    && !state.is_open
                {
                    fn find_next<'a, T: PartialEq>(
                        selected: &'a T,
                        mut options: impl Iterator<Item = &'a T>,
                    ) -> Option<&'a T> {
                        let _ = options.find(|&option| option == selected);
                        options.next()
                    }

                    let options = self.options.borrow();
                    let selected = self.selected.as_ref().map(Borrow::borrow);

                    let next_option = if *y < 0.0 {
                        if let Some(selected) = selected {
                            find_next(selected, options.iter())
                        } else {
                            options.first()
                        }
                    } else if *y > 0.0 {
                        if let Some(selected) = selected {
                            find_next(selected, options.iter().rev())
                        } else {
                            options.last()
                        }
                    } else {
                        None
                    };

                    if let Some(next_option) = next_option {
                        shell.publish(on_select(next_option.clone()));
                    }

                    shell.capture_event();
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.keyboard_modifiers = *modifiers;
            }
            _ => {}
        }

        let status = {
            let is_hovered = cursor.is_over(layout.bounds());

            if self.on_select.is_none() {
                Status::Disabled
            } else if state.is_open {
                Status::Opened { is_hovered }
            } else if is_hovered {
                Status::Hovered
            } else {
                Status::Active
            }
        };

        if let Event::Window(window::Event::RedrawRequested(_now)) = event {
            self.last_status = Some(status);
        } else if self
            .last_status
            .is_some_and(|last_status| last_status != status)
        {
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        if cursor.is_over(bounds) {
            if self.on_select.is_some() {
                mouse::Interaction::Pointer
            } else {
                mouse::Interaction::Idle
            }
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let font = self.font.unwrap_or_else(|| renderer.default_font());
        let selected = self.selected.as_ref().map(Borrow::borrow);
        let state = tree.state.downcast_ref::<PickListState<Renderer::Paragraph>>();

        let bounds = layout.bounds();

        let style = Catalog::style(
            theme,
            &self.class,
            self.last_status.unwrap_or(Status::Active),
        );

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: style.border,
                ..renderer::Quad::default()
            },
            style.background,
        );

        let handle = match &self.handle {
            Handle::Arrow { size } => Some((
                Renderer::ICON_FONT,
                Renderer::ARROW_DOWN_ICON,
                *size,
                text::LineHeight::default(),
                text::Shaping::Basic,
            )),
            Handle::Static(Icon {
                font,
                code_point,
                size,
                line_height,
                shaping,
            }) => Some((*font, *code_point, *size, *line_height, *shaping)),
            Handle::Dynamic { open, closed } => {
                if state.is_open {
                    Some((
                        open.font,
                        open.code_point,
                        open.size,
                        open.line_height,
                        open.shaping,
                    ))
                } else {
                    Some((
                        closed.font,
                        closed.code_point,
                        closed.size,
                        closed.line_height,
                        closed.shaping,
                    ))
                }
            }
            Handle::None => None,
        };

        if let Some((font, code_point, size, line_height, shaping)) = handle {
            let size = size.unwrap_or_else(|| renderer.default_size());

            renderer.fill_text(
                Text {
                    content: code_point.to_string(),
                    size,
                    line_height,
                    font,
                    bounds: Size::new(bounds.width, f32::from(line_height.to_absolute(size))),
                    align_x: text::Alignment::Right,
                    align_y: alignment::Vertical::Center,
                    shaping,
                    wrapping: text::Wrapping::None,
                    ellipsis: text::Ellipsis::None,
                    hint_factor: None,
                },
                Point::new(
                    bounds.x + bounds.width - self.padding.right,
                    bounds.center_y(),
                ),
                style.handle_color,
                *viewport,
            );
        }

        let label = selected.map(&self.to_string);

        if let Some(label) = label.or_else(|| self.placeholder.clone()) {
            let text_size = self.text_size.unwrap_or_else(|| renderer.default_size());

            renderer.fill_text(
                Text {
                    content: label,
                    size: text_size,
                    line_height: self.line_height,
                    font,
                    bounds: Size::new(
                        bounds.width - self.padding.x(),
                        f32::from(self.line_height.to_absolute(text_size)),
                    ),
                    align_x: text::Alignment::Default,
                    align_y: alignment::Vertical::Center,
                    shaping: self.shaping,
                    wrapping: text::Wrapping::None,
                    ellipsis: self.ellipsis,
                    hint_factor: renderer.hint_factor(),
                },
                Point::new(bounds.x + self.padding.left, bounds.center_y()),
                if selected.is_some() {
                    style.text_color
                } else {
                    style.placeholder_color
                },
                *viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let Some(on_select) = &self.on_select else {
            return None;
        };

        let state = tree.state.downcast_mut::<PickListState<Renderer::Paragraph>>();
        let font = self.font.unwrap_or_else(|| renderer.default_font());

        if state.is_open {
            let bounds = layout.bounds();
            let on_close = self.on_close.clone();
            let is_open_flag = Rc::new(Cell::new(true));
            let is_open_for_select = Rc::clone(&is_open_flag);
            let is_open_for_esc = Rc::clone(&is_open_flag);

            let is_open_ptr: *mut bool = &mut state.is_open;

            let mut menu = menu::Menu::new(
                &mut state.menu,
                self.options.borrow(),
                &mut state.hovered_option,
                &self.to_string,
                move |option| {
                    is_open_for_select.set(false);
                    // SAFETY: `is_open_ptr` points to `state.is_open` which lives for `'b`,
                    // matching the lifetime of the returned `overlay::Element<'b, ...>`.
                    unsafe {
                        *is_open_ptr = false;
                    }
                    (on_select)(option)
                },
                None,
                &self.menu_class,
            )
            .keyboard_navigation(move || {
                is_open_for_esc.set(false);
                unsafe {
                    *is_open_ptr = false;
                }
                on_close.clone()
            })
            .width(bounds.width)
            .padding(self.padding)
            .font(font)
            .ellipsis(self.ellipsis)
            .shaping(self.shaping);

            if let Some(text_size) = self.text_size {
                menu = menu.text_size(text_size);
            }

            Some(menu.overlay(
                layout.position() + translation,
                *viewport,
                bounds.height,
                self.menu_height,
            ))
        } else {
            None
        }
    }
}

impl<'a, T, L, V, Message, Theme, Renderer> From<PickList<'a, T, L, V, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    T: Clone + PartialEq + 'a,
    L: Borrow<[T]> + 'a,
    V: Borrow<T> + 'a,
    Message: Clone + 'a,
    Theme: Catalog + 'a,
    Renderer: text::Renderer + 'a,
{
    fn from(pick_list: PickList<'a, T, L, V, Message, Theme, Renderer>) -> Self {
        Self::new(pick_list)
    }
}

// ── Enhanced ComboBox ──────────────────────────────────────────────────────

pub mod combo_box {
    use super::*;
    pub use super::ComboBoxCatalog as Catalog;

    pub fn combo_box<'a, T, Message, Theme, Renderer>(
        state: &'a State<T>,
        placeholder: &str,
        selection: Option<&T>,
        on_selected: impl Fn(T) -> Message + 'a,
    ) -> ComboBox<'a, T, Message, Theme, Renderer>
    where
        T: std::fmt::Display + Clone,
        Theme: Catalog,
        Renderer: text::Renderer,
    {
        ComboBox::new(state, placeholder, selection, on_selected)
    }

    pub struct ComboBox<'a, T, Message, Theme = iced::Theme, Renderer = iced::Renderer>
    where
        Theme: Catalog,
        Renderer: text::Renderer,
    {
        state: &'a State<T>,
        text_input: TextInput<'a, TextInputEvent, Theme, Renderer>,
        font: Option<Renderer::Font>,
        selection_str: String,
        selection: text_input::Value,
        on_selected: Box<dyn Fn(T) -> Message + 'a>,
        on_option_hovered: Option<Box<dyn Fn(T) -> Message + 'a>>,
        on_open: Option<Message>,
        on_close: Option<Message>,
        on_input: Option<Box<dyn Fn(String) -> Message + 'a>>,
        padding: Padding,
        size: Option<f32>,
        shaping: text::Shaping,
        ellipsis: text::Ellipsis,
        menu_class: <Theme as menu::Catalog>::Class<'a>,
        menu_height: Length,
    }

    impl<'a, T, Message, Theme, Renderer> ComboBox<'a, T, Message, Theme, Renderer>
    where
        T: std::fmt::Display + Clone,
        Theme: Catalog,
        Renderer: text::Renderer,
    {
        pub fn new(
            state: &'a State<T>,
            placeholder: &str,
            selection: Option<&T>,
            on_selected: impl Fn(T) -> Message + 'a,
        ) -> Self {
            let text_input = TextInput::new(placeholder, &state.value())
                .on_input(TextInputEvent::TextChanged)
                .class(Theme::default_input());

            let selection_str = selection.map(T::to_string).unwrap_or_default();

            Self {
                state,
                text_input,
                font: None,
                selection: text_input::Value::new(&selection_str),
                selection_str,
                on_selected: Box::new(on_selected),
                on_option_hovered: None,
                on_input: None,
                on_open: None,
                on_close: None,
                padding: text_input::DEFAULT_PADDING,
                size: None,
                shaping: text::Shaping::default(),
                ellipsis: text::Ellipsis::End,
                menu_class: <Theme as Catalog>::default_menu(),
                menu_height: Length::Shrink,
            }
        }

        pub fn on_input(mut self, on_input: impl Fn(String) -> Message + 'a) -> Self {
            self.on_input = Some(Box::new(on_input));
            self
        }

        pub fn on_option_hovered(
            mut self,
            on_option_hovered: impl Fn(T) -> Message + 'a,
        ) -> Self {
            self.on_option_hovered = Some(Box::new(on_option_hovered));
            self
        }

        pub fn on_open(mut self, message: Message) -> Self {
            self.on_open = Some(message);
            self
        }

        pub fn on_close(mut self, message: Message) -> Self {
            self.on_close = Some(message);
            self
        }

        pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
            self.padding = padding.into();
            self.text_input = self.text_input.padding(self.padding);
            self
        }

        pub fn font(mut self, font: Renderer::Font) -> Self {
            self.text_input = self.text_input.font(font);
            self.font = Some(font);
            self
        }

        pub fn icon(mut self, icon: text_input::Icon<Renderer::Font>) -> Self {
            self.text_input = self.text_input.icon(icon);
            self
        }

        pub fn size(mut self, size: impl Into<Pixels>) -> Self {
            let size = size.into();
            self.text_input = self.text_input.size(size);
            self.size = Some(size.0);
            self
        }

        pub fn line_height(self, line_height: impl Into<text::LineHeight>) -> Self {
            Self {
                text_input: self.text_input.line_height(line_height),
                ..self
            }
        }

        pub fn width(self, width: impl Into<Length>) -> Self {
            Self {
                text_input: self.text_input.width(width),
                ..self
            }
        }

        pub fn menu_height(mut self, menu_height: impl Into<Length>) -> Self {
            self.menu_height = menu_height.into();
            self
        }

        pub fn shaping(mut self, shaping: text::Shaping) -> Self {
            self.shaping = shaping;
            self
        }

        pub fn ellipsis(mut self, ellipsis: text::Ellipsis) -> Self {
            self.ellipsis = ellipsis;
            self
        }

        #[must_use]
        pub fn input_style(
            mut self,
            style: impl Fn(&Theme, text_input::Status) -> text_input::Style + 'a,
        ) -> Self
        where
            <Theme as text_input::Catalog>::Class<'a>: From<text_input::StyleFn<'a, Theme>>,
        {
            self.text_input = self.text_input.style(style);
            self
        }

        #[must_use]
        pub fn menu_style(mut self, style: impl Fn(&Theme) -> menu::Style + 'a) -> Self
        where
            <Theme as menu::Catalog>::Class<'a>: From<menu::StyleFn<'a, Theme>>,
        {
            self.menu_class = (Box::new(style) as menu::StyleFn<'a, Theme>).into();
            self
        }

        #[must_use]
        pub fn input_class(
            mut self,
            class: impl Into<<Theme as text_input::Catalog>::Class<'a>>,
        ) -> Self {
            self.text_input = self.text_input.class(class);
            self
        }

        #[must_use]
        pub fn menu_class(
            mut self,
            class: impl Into<<Theme as menu::Catalog>::Class<'a>>,
        ) -> Self {
            self.menu_class = class.into();
            self
        }
    }

    #[derive(Debug, Clone)]
    pub struct State<T> {
        options: Vec<T>,
        inner: RefCell<Inner<T>>,
    }

    #[derive(Debug, Clone)]
    struct Inner<T> {
        value: String,
        option_matchers: Vec<String>,
        filtered_options: Filtered<T>,
    }

    #[derive(Debug, Clone)]
    struct Filtered<T> {
        options: Vec<T>,
        updated: Instant,
    }

    impl<T> State<T>
    where
        T: Display + Clone,
    {
        pub fn new(options: Vec<T>) -> Self {
            Self::with_selection(options, None)
        }

        pub fn with_selection(options: Vec<T>, selection: Option<&T>) -> Self {
            let value = selection.map(T::to_string).unwrap_or_default();
            let option_matchers = build_matchers(&options);
            let filtered_options = Filtered::new(
                search(&options, &option_matchers, &value)
                    .cloned()
                    .collect(),
            );

            Self {
                options,
                inner: RefCell::new(Inner {
                    value,
                    option_matchers,
                    filtered_options,
                }),
            }
        }

        pub fn options(&self) -> &[T] {
            &self.options
        }

        pub fn push(&mut self, new_option: T) {
            let mut inner = self.inner.borrow_mut();

            inner.option_matchers.push(build_matcher(&new_option));
            self.options.push(new_option);

            inner.filtered_options = Filtered::new(
                search(&self.options, &inner.option_matchers, &inner.value)
                    .cloned()
                    .collect(),
            );
        }

        pub fn into_options(self) -> Vec<T> {
            self.options
        }

        fn value(&self) -> String {
            self.inner.borrow().value.clone()
        }

        fn with_inner<O>(&self, f: impl FnOnce(&Inner<T>) -> O) -> O {
            let inner = self.inner.borrow();
            f(&inner)
        }

        fn with_inner_mut(&self, f: impl FnOnce(&mut Inner<T>)) {
            let mut inner = self.inner.borrow_mut();
            f(&mut inner);
        }

        fn sync_filtered_options(&self, options: &mut Filtered<T>) {
            self.inner.borrow().filtered_options.sync(options);
        }
    }

    impl<T> Default for State<T>
    where
        T: Display + Clone,
    {
        fn default() -> Self {
            Self::new(Vec::new())
        }
    }

    impl<T> Filtered<T>
    where
        T: Clone,
    {
        fn new(options: Vec<T>) -> Self {
            Self {
                options,
                updated: Instant::now(),
            }
        }

        fn empty() -> Self {
            Self {
                options: vec![],
                updated: Instant::now(),
            }
        }

        fn update(&mut self, options: Vec<T>) {
            self.options = options;
            self.updated = Instant::now();
        }

        fn sync(&self, other: &mut Filtered<T>) {
            if other.updated != self.updated {
                *other = self.clone();
            }
        }
    }

    struct ComboBoxMenu<T> {
        menu: menu::State,
        hovered_option: Option<usize>,
        new_selection: Option<T>,
        filtered_options: Filtered<T>,
    }

    #[derive(Debug, Clone)]
    enum TextInputEvent {
        TextChanged(String),
    }

    impl<T, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
        for ComboBox<'_, T, Message, Theme, Renderer>
    where
        T: Display + Clone + 'static,
        Message: Clone,
        Theme: Catalog,
        Renderer: text::Renderer,
    {
        fn size(&self) -> Size<Length> {
            Widget::<TextInputEvent, Theme, Renderer>::size(&self.text_input)
        }

        fn layout(
            &mut self,
            tree: &mut Tree,
            renderer: &Renderer,
            limits: &layout::Limits,
        ) -> layout::Node {
            let is_focused = {
                let text_input_state = tree.children[0]
                    .state
                    .downcast_ref::<text_input::State<Renderer::Paragraph>>();

                text_input_state.is_focused()
            };

            self.text_input.layout(
                &mut tree.children[0],
                renderer,
                limits,
                (!is_focused).then_some(&self.selection),
            )
        }

        fn tag(&self) -> tree::Tag {
            tree::Tag::of::<ComboBoxMenu<T>>()
        }

        fn state(&self) -> tree::State {
            tree::State::new(ComboBoxMenu::<T> {
                menu: menu::State::new(),
                filtered_options: Filtered::empty(),
                hovered_option: Some(0),
                new_selection: None,
            })
        }

        fn diff(&mut self, tree: &mut Tree) {
            tree.diff_children(&mut [&mut self.text_input as &mut dyn Widget<_, _, _>]);
        }

        fn update(
            &mut self,
            tree: &mut Tree,
            event: &Event,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            renderer: &Renderer,
            shell: &mut Shell<'_, Message>,
            viewport: &Rectangle,
        ) {
            let menu = tree.state.downcast_mut::<ComboBoxMenu<T>>();

            let started_focused = {
                let text_input_state = tree.children[0]
                    .state
                    .downcast_ref::<text_input::State<Renderer::Paragraph>>();

                text_input_state.is_focused()
            };
            let mut published_message_to_shell = false;

            let mut local_messages = Vec::new();
            let mut local_shell = shell.local(&mut local_messages);

            self.text_input.update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                &mut local_shell,
                viewport,
            );

            if local_shell.is_event_captured() {
                shell.capture_event();
            }

            shell.request_redraw_at(local_shell.redraw_request());
            shell.request_input_method(local_shell.input_method());
            shell.clipboard_mut().merge(local_shell.clipboard_mut());

            for message in local_messages {
                let TextInputEvent::TextChanged(new_value) = message;

                if let Some(on_input) = &self.on_input {
                    shell.publish((on_input)(new_value.clone()));
                }

                self.state.with_inner_mut(|state| {
                    menu.hovered_option = Some(0);
                    menu.menu.last_scrolled_option = None;
                    state.value = new_value;

                    state.filtered_options.update(
                        search(&self.state.options, &state.option_matchers, &state.value)
                            .cloned()
                            .collect(),
                    );
                });
                shell.invalidate_layout();
                shell.request_redraw();
            }

            let is_focused = {
                let text_input_state = tree.children[0]
                    .state
                    .downcast_ref::<text_input::State<Renderer::Paragraph>>();

                text_input_state.is_focused()
            };

            // If Escape closed the focused ComboBox, reset its filter and mark Escape captured
            // so the parent dialog is not dismissed.
            if started_focused
                && !is_focused
                && matches!(
                    event,
                    Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(Named::Escape),
                        ..
                    })
                )
            {
                self.state.with_inner_mut(|state| {
                    state.value.clear();
                    state.filtered_options.update(self.state.options.clone());
                });
                mark_dropdown_escape_captured();
            }

            if is_focused {
                if !started_focused {
                    // When opening the ComboBox, highlight the currently selected item
                    // (or first option) and scroll it into view.
                    self.state.with_inner_mut(|state| {
                        state.value.clear();
                        state.filtered_options.update(self.state.options.clone());
                        menu.hovered_option = state
                            .filtered_options
                            .options
                            .iter()
                            .position(|opt| opt.to_string() == self.selection_str)
                            .or_else(|| (!state.filtered_options.options.is_empty()).then_some(0));
                        menu.menu.last_scrolled_option = None;
                    });
                }

                self.state.with_inner(|state| {
                    if !started_focused {
                        if let Some(on_option_hovered) = &mut self.on_option_hovered {
                            let hovered_option = menu.hovered_option.unwrap_or(0);

                            if let Some(option) = state.filtered_options.options.get(hovered_option) {
                                shell.publish(on_option_hovered(option.clone()));
                                published_message_to_shell = true;
                            }
                        }
                    }

                    if let Event::Keyboard(keyboard::Event::KeyPressed {
                        key: keyboard::Key::Named(named_key),
                        modifiers,
                        ..
                    }) = event
                    {
                        let shift_modifier = modifiers.shift();
                        match (named_key, shift_modifier) {
                            (key::Named::Enter, _) => {
                                if let Some(index) = &menu.hovered_option {
                                    if let Some(option) = state.filtered_options.options.get(*index) {
                                        menu.new_selection = Some(option.clone());
                                    }
                                }

                                shell.capture_event();
                                shell.request_redraw();
                            }
                            (key::Named::ArrowUp, _) | (key::Named::Tab, true) => {
                                if let Some(index) = &mut menu.hovered_option {
                                    if *index == 0 {
                                        *index =
                                            state.filtered_options.options.len().saturating_sub(1);
                                    } else {
                                        *index = index.saturating_sub(1);
                                    }
                                } else {
                                    menu.hovered_option = Some(0);
                                }
                                menu.menu.last_scrolled_option = None;

                                if let Some(on_option_hovered) = &mut self.on_option_hovered {
                                    if let Some(option) = menu
                                        .hovered_option
                                        .and_then(|index| state.filtered_options.options.get(index))
                                    {
                                        shell.publish((on_option_hovered)(option.clone()));
                                        published_message_to_shell = true;
                                    }
                                }

                                mark_dropdown_arrow_captured();
                                shell.capture_event();
                                shell.request_redraw();
                            }
                            (key::Named::ArrowDown, _) | (key::Named::Tab, false)
                                if !modifiers.shift() =>
                            {
                                if let Some(index) = &mut menu.hovered_option {
                                    if *index
                                        >= state.filtered_options.options.len().saturating_sub(1)
                                    {
                                        *index = 0;
                                    } else {
                                        *index = index.saturating_add(1).min(
                                            state.filtered_options.options.len().saturating_sub(1),
                                        );
                                    }
                                } else {
                                    menu.hovered_option = Some(0);
                                }
                                menu.menu.last_scrolled_option = None;

                                if let Some(on_option_hovered) = &mut self.on_option_hovered {
                                    if let Some(option) = menu
                                        .hovered_option
                                        .and_then(|index| state.filtered_options.options.get(index))
                                    {
                                        shell.publish((on_option_hovered)(option.clone()));
                                        published_message_to_shell = true;
                                    }
                                }

                                mark_dropdown_arrow_captured();
                                shell.capture_event();
                                shell.request_redraw();
                            }
                            _ => {}
                        }
                    }
                });
            }

            self.state.with_inner_mut(|state| {
                if let Some(selection) = menu.new_selection.take() {
                    state.value = String::new();
                    state.filtered_options.update(self.state.options.clone());
                    menu.menu = menu::State::default();

                    shell.publish((self.on_selected)(selection));
                    published_message_to_shell = true;

                    let mut local_messages = Vec::new();
                    let mut local_shell = shell.local(&mut local_messages);
                    self.text_input.update(
                        &mut tree.children[0],
                        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                        layout,
                        mouse::Cursor::Unavailable,
                        renderer,
                        &mut local_shell,
                        viewport,
                    );
                    shell.request_input_method(local_shell.input_method());
                }
            });

            let is_focused = {
                let text_input_state = tree.children[0]
                    .state
                    .downcast_ref::<text_input::State<Renderer::Paragraph>>();

                text_input_state.is_focused()
            };

            if started_focused != is_focused {
                shell.invalidate_widgets();

                if !published_message_to_shell {
                    if is_focused {
                        if let Some(on_open) = self.on_open.take() {
                            shell.publish(on_open);
                        }
                    } else if let Some(on_close) = self.on_close.take() {
                        shell.publish(on_close);
                    }
                }
            }
        }

        fn mouse_interaction(
            &self,
            tree: &Tree,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
            renderer: &Renderer,
        ) -> mouse::Interaction {
            self.text_input
                .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
        }

        fn draw(
            &self,
            tree: &Tree,
            renderer: &mut Renderer,
            theme: &Theme,
            _style: &renderer::Style,
            layout: Layout<'_>,
            cursor: mouse::Cursor,
            viewport: &Rectangle,
        ) {
            let is_focused = {
                let text_input_state = tree.children[0]
                    .state
                    .downcast_ref::<text_input::State<Renderer::Paragraph>>();

                text_input_state.is_focused()
            };

            let selection = if is_focused || self.selection.is_empty() {
                None
            } else {
                Some(&self.selection)
            };

            self.text_input.draw(
                &tree.children[0],
                renderer,
                theme,
                layout,
                cursor,
                selection,
                viewport,
            );
        }

        fn overlay<'b>(
            &'b mut self,
            tree: &'b mut Tree,
            layout: Layout<'_>,
            _renderer: &Renderer,
            viewport: &Rectangle,
            translation: Vector,
        ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
            let is_focused = {
                let text_input_state = tree.children[0]
                    .state
                    .downcast_ref::<text_input::State<Renderer::Paragraph>>();

                text_input_state.is_focused()
            };

            if is_focused {
                let ComboBoxMenu {
                    menu,
                    filtered_options,
                    hovered_option,
                    ..
                } = tree.state.downcast_mut::<ComboBoxMenu<T>>();

                self.state.sync_filtered_options(filtered_options);

                if filtered_options.options.is_empty() {
                    None
                } else {
                    let bounds = layout.bounds();

                    let mut menu = menu::Menu::new(
                        menu,
                        &filtered_options.options,
                        hovered_option,
                        &T::to_string,
                        |selection| {
                            self.state.with_inner_mut(|state| {
                                state.value = String::new();
                                state.filtered_options.update(self.state.options.clone());
                            });

                            tree.children[0]
                                .state
                                .downcast_mut::<text_input::State<Renderer::Paragraph>>()
                                .unfocus();

                            (self.on_selected)(selection)
                        },
                        self.on_option_hovered.as_deref(),
                        &self.menu_class,
                    )
                    .width(bounds.width)
                    .padding(self.padding)
                    .shaping(self.shaping)
                    .ellipsis(self.ellipsis);

                    if let Some(font) = self.font {
                        menu = menu.font(font);
                    }

                    if let Some(size) = self.size {
                        menu = menu.text_size(size);
                    }

                    Some(menu.overlay(
                        layout.position() + translation,
                        *viewport,
                        bounds.height,
                        self.menu_height,
                    ))
                }
            } else {
                None
            }
        }
    }

    impl<'a, T, Message, Theme, Renderer> From<ComboBox<'a, T, Message, Theme, Renderer>>
        for Element<'a, Message, Theme, Renderer>
    where
        T: Display + Clone + 'static,
        Message: Clone + 'a,
        Theme: Catalog + 'a,
        Renderer: text::Renderer + 'a,
    {
        fn from(combo_box: ComboBox<'a, T, Message, Theme, Renderer>) -> Self {
            Self::new(combo_box)
        }
    }

    fn search<'a, T, A>(
        options: impl IntoIterator<Item = T> + 'a,
        option_matchers: impl IntoIterator<Item = &'a A> + 'a,
        query: &'a str,
    ) -> impl Iterator<Item = T> + 'a
    where
        A: AsRef<str> + 'a,
    {
        let query: Vec<String> = query
            .to_lowercase()
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        options
            .into_iter()
            .zip(option_matchers)
            .filter_map(move |(option, matcher)| {
                if query.iter().all(|part| matcher.as_ref().contains(part)) {
                    Some(option)
                } else {
                    None
                }
            })
    }

    fn build_matchers<'a, T>(options: impl IntoIterator<Item = T> + 'a) -> Vec<String>
    where
        T: Display + 'a,
    {
        options.into_iter().map(build_matcher).collect()
    }

    fn build_matcher<T>(option: T) -> String
    where
        T: Display,
    {
        option.to_string().to_lowercase()
    }
}

pub use combo_box::combo_box;

// ── Shared DropdownList Widget for Custom & Ribbon Dropdowns ───────────────

/// One row in a [`DropdownList`]: either a selectable item (with its text label for
/// type-ahead and the message emitted on `Enter`) or a non-selectable header/divider.
pub struct DropdownRow<'a, Message> {
    pub element: Element<'a, Message>,
    pub label: Option<String>,
    pub on_select: Option<Message>,
}

impl<'a, Message> DropdownRow<'a, Message> {
    /// A selectable option row.
    pub fn item(
        element: impl Into<Element<'a, Message>>,
        label: impl Into<String>,
        on_select: Message,
    ) -> Self {
        Self {
            element: element.into(),
            label: Some(label.into()),
            on_select: Some(on_select),
        }
    }

    /// A non-selectable header, separator, or decorative row.
    pub fn header(element: impl Into<Element<'a, Message>>) -> Self {
        Self {
            element: element.into(),
            label: None,
            on_select: None,
        }
    }
}

#[derive(Default)]
struct SharedHighlight {
    highlighted: Cell<Option<usize>>,
    keyboard_active: Cell<bool>,
}

/// Shared keyboard-navigable, auto-scrolling list widget for custom and ribbon dropdowns.
pub struct DropdownList<'a, Message> {
    scrollable: Element<'a, Message>,
    meta: Vec<Option<(String, Message)>>,
    shared: Rc<SharedHighlight>,
    initial_selected: Option<usize>,
    select_first_on_empty_query: bool,
    query: Option<String>,
    on_escape: Option<Message>,
}

impl<'a, Message: Clone + 'a> DropdownList<'a, Message> {
    pub fn new(rows: Vec<DropdownRow<'a, Message>>) -> Self {
        Self::with_options(
            rows,
            Length::Shrink,
            Length::Fill,
            0.0,
            Padding::ZERO,
        )
    }

    pub fn with_options(
        rows: Vec<DropdownRow<'a, Message>>,
        height: impl Into<Length>,
        width: impl Into<Length>,
        spacing: impl Into<Pixels>,
        padding: impl Into<Padding>,
    ) -> Self {
        let mut elements = Vec::with_capacity(rows.len());
        let mut meta = Vec::with_capacity(rows.len());
        for row in rows {
            elements.push(row.element);
            meta.push(match (row.label, row.on_select) {
                (Some(l), Some(m)) => Some((l, m)),
                _ => None,
            });
        }
        let shared = Rc::new(SharedHighlight::default());
        let width = width.into();
        let inner = DropdownListInner {
            children: elements,
            selectable: meta.iter().map(Option::is_some).collect(),
            shared: Rc::clone(&shared),
            spacing: spacing.into().0,
            padding: padding.into(),
            width,
        };
        let scrollable = Scrollable::new(Element::new(inner))
            .height(height)
            .width(width)
            .into();

        Self {
            scrollable,
            meta,
            shared,
            initial_selected: None,
            select_first_on_empty_query: true,
            query: None,
            on_escape: None,
        }
    }

    /// Set the initially highlighted row index when the dropdown opens.
    pub fn selected(mut self, index: Option<usize>) -> Self {
        self.initial_selected = index;
        self
    }

    /// Control whether the first selectable row is highlighted when `initial_selected` is `None`.
    pub fn select_first_by_default(mut self, select_first: bool) -> Self {
        self.select_first_on_empty_query = select_first;
        self
    }

    /// Mark this dropdown as controlled by a search `text_input` with current filter `query`.
    /// When `query` is set, character keys are left for the `text_input` and changing `query`
    /// resets the highlight to the first matching row.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    /// Message to publish when `Escape` is pressed while this dropdown is open.
    pub fn on_escape(mut self, msg: Message) -> Self {
        self.on_escape = Some(msg);
        self
    }

    fn is_selectable(&self, idx: usize) -> bool {
        self.meta.get(idx).is_some_and(Option::is_some)
    }

    fn first_selectable(&self) -> Option<usize> {
        self.meta.iter().position(Option::is_some)
    }

    fn first_matching_selectable(&self, query: &str) -> Option<usize> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return self.first_selectable();
        }
        self.meta
            .iter()
            .enumerate()
            .position(|(_, m)| {
                if let Some((label, _)) = m {
                    label.to_lowercase().contains(&q)
                } else {
                    false
                }
            })
            .or_else(|| self.first_selectable())
    }

    fn last_selectable(&self) -> Option<usize> {
        self.meta.iter().rposition(Option::is_some)
    }

    fn next_selectable(&self, current: Option<usize>) -> Option<usize> {
        let len = self.meta.len();
        if len == 0 {
            return None;
        }
        let start = current.map_or(0, |c| (c + 1) % len);
        for offset in 0..len {
            let idx = (start + offset) % len;
            if self.is_selectable(idx) {
                return Some(idx);
            }
        }
        None
    }

    fn prev_selectable(&self, current: Option<usize>) -> Option<usize> {
        let len = self.meta.len();
        if len == 0 {
            return None;
        }
        let start = match current {
            Some(0) | None => len - 1,
            Some(c) => c - 1,
        };
        for offset in 0..len {
            let idx = (start + len - (offset % len)) % len;
            if self.is_selectable(idx) {
                return Some(idx);
            }
        }
        None
    }
}

#[derive(Default)]
struct DropdownListState {
    highlighted: Option<usize>,
    keyboard_active: bool,
    initialized: bool,
    last_query: Option<String>,
    last_scrolled: Option<usize>,
    type_ahead: TypeAhead,
}

impl<'a, Message: Clone + 'a> DropdownList<'a, Message> {
    fn sync_state(&self, state: &mut DropdownListState) {
        if !state.initialized {
            state.initialized = true;
            state.last_query = self.query.clone();
            let query_non_empty = self.query.as_deref().is_some_and(|q| !q.trim().is_empty());
            state.highlighted = if query_non_empty {
                self.select_first_on_empty_query
                    .then(|| self.first_matching_selectable(self.query.as_deref().unwrap()))
                    .flatten()
                    .or_else(|| self.initial_selected.filter(|&i| self.is_selectable(i)))
            } else {
                self.initial_selected
                    .filter(|&i| self.is_selectable(i))
                    .or_else(|| {
                        self.select_first_on_empty_query
                            .then(|| self.first_selectable())
                            .flatten()
                    })
            };
            state.keyboard_active = state.highlighted.is_some();
            state.last_scrolled = None;
        } else if self.query != state.last_query {
            state.last_query = self.query.clone();
            let query_non_empty = self.query.as_deref().is_some_and(|q| !q.trim().is_empty());
            state.highlighted = if query_non_empty {
                self.select_first_on_empty_query
                    .then(|| self.first_matching_selectable(self.query.as_deref().unwrap()))
                    .flatten()
                    .or_else(|| self.initial_selected.filter(|&i| self.is_selectable(i)))
            } else {
                self.initial_selected
                    .filter(|&i| self.is_selectable(i))
                    .or_else(|| {
                        self.select_first_on_empty_query
                            .then(|| self.first_selectable())
                            .flatten()
                    })
            };
            state.keyboard_active = state.highlighted.is_some();
            state.last_scrolled = None;
        } else if let Some(idx) = state.highlighted {
            if !self.is_selectable(idx) {
                state.highlighted = self
                    .select_first_on_empty_query
                    .then(|| self.first_selectable())
                    .flatten();
                state.keyboard_active = state.highlighted.is_some();
                state.last_scrolled = None;
            }
        }

        self.shared.highlighted.set(state.highlighted);
        self.shared.keyboard_active.set(state.keyboard_active);
    }

    fn scroll_highlighted_into_view(
        &mut self,
        state: &mut DropdownListState,
        scrollable_tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
    ) {
        if state.highlighted != state.last_scrolled {
            state.last_scrolled = state.highlighted;
            if let Some(idx) = state.highlighted {
                if let Some(content_layout) = layout.children().next() {
                    if let Some(row_layout) = content_layout.children().nth(idx) {
                        let content_bounds = content_layout.bounds();
                        let row_bounds = row_layout.bounds();
                        let row_top = row_bounds.y - content_bounds.y;
                        let row_bottom = row_top + row_bounds.height;
                        ensure_scrollable_row_visible(
                            self.scrollable.as_widget_mut(),
                            scrollable_tree,
                            layout,
                            renderer,
                            row_top,
                            row_bottom,
                        );
                    }
                }
            }
        }
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, iced::Renderer> for DropdownList<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DropdownListState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DropdownListState::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.scrollable));
    }

    fn size(&self) -> Size<Length> {
        self.scrollable.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<DropdownListState>();
        self.sync_state(state);
        self.scrollable
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.scrollable.as_widget_mut().operate(
            &mut tree.children[0],
            layout,
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<DropdownListState>();
        self.sync_state(state);
        self.scroll_highlighted_into_view(state, &mut tree.children[0], layout, renderer);

        if let Event::Keyboard(keyboard::Event::KeyPressed {
            key,
            modified_key,
            modifiers,
            ..
        }) = event
        {
            match key.as_ref() {
                keyboard::Key::Named(Named::ArrowDown) => {
                    if let Some(next) = self.next_selectable(state.highlighted) {
                        state.highlighted = Some(next);
                        state.keyboard_active = true;
                        state.last_scrolled = None;
                        self.shared.highlighted.set(state.highlighted);
                        self.shared.keyboard_active.set(true);
                        self.scroll_highlighted_into_view(
                            state,
                            &mut tree.children[0],
                            layout,
                            renderer,
                        );
                    }
                    mark_dropdown_arrow_captured();
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                keyboard::Key::Named(Named::ArrowUp) => {
                    if let Some(prev) = self.prev_selectable(state.highlighted) {
                        state.highlighted = Some(prev);
                        state.keyboard_active = true;
                        state.last_scrolled = None;
                        self.shared.highlighted.set(state.highlighted);
                        self.shared.keyboard_active.set(true);
                        self.scroll_highlighted_into_view(
                            state,
                            &mut tree.children[0],
                            layout,
                            renderer,
                        );
                    }
                    mark_dropdown_arrow_captured();
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                keyboard::Key::Named(Named::Home) if self.query.is_none() => {
                    if let Some(first) = self.first_selectable() {
                        state.highlighted = Some(first);
                        state.keyboard_active = true;
                        state.last_scrolled = None;
                        self.shared.highlighted.set(state.highlighted);
                        self.shared.keyboard_active.set(true);
                        self.scroll_highlighted_into_view(
                            state,
                            &mut tree.children[0],
                            layout,
                            renderer,
                        );
                    }
                    mark_dropdown_arrow_captured();
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                keyboard::Key::Named(Named::End) if self.query.is_none() => {
                    if let Some(last) = self.last_selectable() {
                        state.highlighted = Some(last);
                        state.keyboard_active = true;
                        state.last_scrolled = None;
                        self.shared.highlighted.set(state.highlighted);
                        self.shared.keyboard_active.set(true);
                        self.scroll_highlighted_into_view(
                            state,
                            &mut tree.children[0],
                            layout,
                            renderer,
                        );
                    }
                    mark_dropdown_arrow_captured();
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                keyboard::Key::Named(Named::Enter) => {
                    if let Some(idx) = state.highlighted {
                        if let Some(Some((_, msg))) = self.meta.get(idx) {
                            shell.publish(msg.clone());
                            shell.capture_event();
                            shell.request_redraw();
                            return;
                        }
                    }
                }
                keyboard::Key::Named(Named::Escape) => {
                    if let Some(msg) = &self.on_escape {
                        shell.publish(msg.clone());
                    }
                    mark_dropdown_escape_captured();
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                keyboard::Key::Named(Named::Backspace) if self.query.is_none() => {
                    let selectable_indices: Vec<usize> = self
                        .meta
                        .iter()
                        .enumerate()
                        .filter_map(|(i, m)| m.as_ref().map(|_| i))
                        .collect();
                    let labels: Vec<&str> = selectable_indices
                        .iter()
                        .filter_map(|&i| self.meta[i].as_ref().map(|(l, _)| l.as_str()))
                        .collect();
                    let cur_pos = state
                        .highlighted
                        .and_then(|h| selectable_indices.iter().position(|&i| i == h));
                    let matched_pos = state
                        .type_ahead
                        .backspace(Instant::now(), cur_pos, labels.into_iter());
                    if let Some(matched_pos) = matched_pos {
                        if let Some(&row_idx) = selectable_indices.get(matched_pos) {
                            state.highlighted = Some(row_idx);
                            state.keyboard_active = true;
                            state.last_scrolled = None;
                            self.shared.highlighted.set(state.highlighted);
                            self.shared.keyboard_active.set(true);
                            self.scroll_highlighted_into_view(
                                state,
                                &mut tree.children[0],
                                layout,
                                renderer,
                            );
                        }
                    }
                    shell.capture_event();
                    shell.request_redraw();
                    return;
                }
                _ if self.query.is_none() => {
                    if !modifiers.control() && !modifiers.alt() && !modifiers.logo() {
                        let ch = match modified_key.as_ref() {
                            keyboard::Key::Character(s) => s.chars().next(),
                            keyboard::Key::Named(Named::Space) => {
                                (!state.type_ahead.buffer().is_empty()).then_some(' ')
                            }
                            _ => None,
                        };
                        if let Some(ch) = ch {
                            if !ch.is_control() {
                                let selectable_indices: Vec<usize> = self
                                    .meta
                                    .iter()
                                    .enumerate()
                                    .filter_map(|(i, m)| m.as_ref().map(|_| i))
                                    .collect();
                                let labels: Vec<&str> = selectable_indices
                                    .iter()
                                    .filter_map(|&i| self.meta[i].as_ref().map(|(l, _)| l.as_str()))
                                    .collect();
                                let cur_pos = state
                                    .highlighted
                                    .and_then(|h| selectable_indices.iter().position(|&i| i == h));
                                let matched_pos = state.type_ahead.push_char(
                                    ch,
                                    Instant::now(),
                                    cur_pos,
                                    labels.into_iter(),
                                );
                                if let Some(matched_pos) = matched_pos {
                                    if let Some(&row_idx) = selectable_indices.get(matched_pos) {
                                        state.highlighted = Some(row_idx);
                                        state.keyboard_active = true;
                                        state.last_scrolled = None;
                                        self.shared.highlighted.set(state.highlighted);
                                        self.shared.keyboard_active.set(true);
                                        self.scroll_highlighted_into_view(
                                            state,
                                            &mut tree.children[0],
                                            layout,
                                            renderer,
                                        );
                                    }
                                }
                                shell.capture_event();
                                shell.request_redraw();
                                return;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        self.scrollable.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        // Sync back any mouse-hover highlight changes recorded by `DropdownListInner`.
        if self.shared.highlighted.get() != state.highlighted
            || self.shared.keyboard_active.get() != state.keyboard_active
        {
            state.highlighted = self.shared.highlighted.get();
            state.keyboard_active = self.shared.keyboard_active.get();
            state.last_scrolled = state.highlighted;
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.scrollable.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.scrollable.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, iced::Renderer>> {
        self.scrollable.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: Clone + 'a> From<DropdownList<'a, Message>> for Element<'a, Message> {
    fn from(list: DropdownList<'a, Message>) -> Self {
        Element::new(list)
    }
}

struct DropdownListInner<'a, Message> {
    children: Vec<Element<'a, Message>>,
    selectable: Vec<bool>,
    shared: Rc<SharedHighlight>,
    spacing: f32,
    padding: Padding,
    width: Length,
}

impl<'a, Message> Widget<Message, Theme, iced::Renderer> for DropdownListInner<'a, Message> {
    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(&mut self.children);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: Length::Shrink,
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::flex::resolve(
            layout::flex::Axis::Vertical,
            renderer,
            limits,
            self.width,
            Length::Shrink,
            self.padding,
            self.spacing,
            alignment::Alignment::Start,
            &mut self.children,
            &mut tree.children,
        )
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            self.children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
                .for_each(|((child, state), layout)| {
                    child
                        .as_widget_mut()
                        .operate(state, layout, renderer, operation);
                });
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Mouse(mouse::Event::CursorMoved { .. }) = event {
            if cursor.is_over(layout.bounds()) {
                for (idx, row_layout) in layout.children().enumerate() {
                    if self.selectable.get(idx).copied().unwrap_or(false)
                        && cursor.is_over(row_layout.bounds())
                    {
                        if self.shared.highlighted.get() != Some(idx)
                            || self.shared.keyboard_active.get()
                        {
                            self.shared.highlighted.set(Some(idx));
                            self.shared.keyboard_active.set(false);
                            shell.request_redraw();
                        }
                        break;
                    }
                }
            }
        }

        let is_redraw = matches!(event, Event::Window(window::Event::RedrawRequested(_)));
        let highlighted = self.shared.highlighted.get();
        let keyboard_active = self.shared.keyboard_active.get();

        for (idx, ((child, child_tree), row_layout)) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
            .enumerate()
        {
            let row_cursor = if is_redraw && highlighted == Some(idx) {
                let b = row_layout.bounds();
                // Place the synthetic hover cursor near the right side of the row so that
                // multi-button rows (such as the Ribbon layer dropdown where the layer-select
                // button fills the right side after the icon toggles) highlight their main
                // select button, while full-width row buttons highlight across the whole row.
                let x = (b.x + b.width - 10.0).max(b.x + 1.0);
                mouse::Cursor::Available(Point::new(x, b.center_y()))
            } else if is_redraw && keyboard_active {
                mouse::Cursor::Unavailable
            } else {
                cursor
            };

            child.as_widget_mut().update(
                child_tree,
                event,
                row_layout,
                row_cursor,
                renderer,
                shell,
                viewport,
            );
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(clipped_viewport) = layout.bounds().intersection(viewport) {
            for ((child, state), layout) in self
                .children
                .iter()
                .zip(&tree.children)
                .zip(layout.children())
            {
                child.as_widget().draw(
                    state,
                    renderer,
                    theme,
                    style,
                    layout,
                    cursor,
                    &clipped_viewport,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_ahead_prefix_matching() {
        let mut ta = TypeAhead::new();
        let items = vec!["0", "Annotations", "Dimensions", "Doors", "Walls", "Windows"];
        let now = Instant::now();

        // Typing 'd' matches "Dimensions"
        let res = ta.push_char('d', now, None, items.iter().copied());
        assert_eq!(res, Some(2));
        assert_eq!(ta.buffer(), "d");

        // Typing 'o' within timeout builds "do" and matches "Doors"
        let res = ta.push_char('o', now, Some(2), items.iter().copied());
        assert_eq!(res, Some(3));
        assert_eq!(ta.buffer(), "do");
    }

    #[test]
    fn test_type_ahead_repeated_char_cycling() {
        let mut ta = TypeAhead::new();
        let items = vec!["Layer 1", "Layer 2", "Lines", "Roof", "Walls"];
        let now = Instant::now();

        // First 'l' -> Layer 1 (index 0)
        let r1 = ta.push_char('l', now, None, items.iter().copied());
        assert_eq!(r1, Some(0));

        // Second 'l' -> cycles to Layer 2 (index 1)
        let r2 = ta.push_char('l', now, Some(0), items.iter().copied());
        assert_eq!(r2, Some(1));

        // Third 'l' -> cycles to Lines (index 2)
        let r3 = ta.push_char('l', now, Some(1), items.iter().copied());
        assert_eq!(r3, Some(2));

        // Fourth 'l' -> wraps back to Layer 1 (index 0)
        let r4 = ta.push_char('l', now, Some(2), items.iter().copied());
        assert_eq!(r4, Some(0));
    }

    #[test]
    fn test_type_ahead_substring_fallback() {
        let mut ta = TypeAhead::new();
        let items = vec!["A-WALL-EXTR", "E-LITE-SWCH", "M-HVAC-DUCT"];
        let now = Instant::now();

        // Typing "lite" does not match any prefix, but matches substring of "E-LITE-SWCH"
        let mut res = None;
        for ch in "lite".chars() {
            res = ta.push_char(ch, now, None, items.iter().copied());
        }
        assert_eq!(res, Some(1));
    }

    #[test]
    fn test_type_ahead_backspace() {
        let mut ta = TypeAhead::new();
        let items = vec!["Door 1", "Door 2", "Dimension", "Wall"];
        let now = Instant::now();

        ta.push_char('d', now, None, items.iter().copied());
        ta.push_char('o', now, Some(0), items.iter().copied());
        assert_eq!(ta.buffer(), "do");

        // Backspace removes 'o', leaves 'd'
        let res = ta.backspace(now, Some(0), items.iter().copied());
        assert_eq!(ta.buffer(), "d");
        assert_eq!(res, Some(0));

        // Backspace removes 'd', leaves empty buffer
        let res2 = ta.backspace(now, Some(0), items.iter().copied());
        assert_eq!(ta.buffer(), "");
        assert_eq!(res2, None);
    }

    #[test]
    fn test_type_ahead_timeout_reset() {
        let mut ta = TypeAhead::new();
        let items = vec!["Doors", "Walls", "Windows"];
        let now = Instant::now();

        ta.push_char('d', now, None, items.iter().copied());
        assert_eq!(ta.buffer(), "d");

        // Typing after TIMEOUT resets the buffer to the new key
        let later = now + TypeAhead::TIMEOUT + Duration::from_millis(50);
        let res = ta.push_char('w', later, Some(0), items.iter().copied());
        assert_eq!(ta.buffer(), "w");
        assert_eq!(res, Some(1)); // "Walls"
    }

    #[test]
    fn test_dropdown_capture_flags() {
        assert!(!take_dropdown_arrow_captured());
        mark_dropdown_arrow_captured();
        assert!(take_dropdown_arrow_captured());
        assert!(!take_dropdown_arrow_captured());

        assert!(!take_dropdown_escape_captured());
        mark_dropdown_escape_captured();
        assert!(take_dropdown_escape_captured());
        assert!(!take_dropdown_escape_captured());
    }

    #[test]
    fn test_dropdown_list_sync_state_with_query() {
        use iced::widget::text;

        let el = |s: &'static str| -> Element<'static, i32> { text(s).into() };
        let rows = vec![
            DropdownRow::header(el("Section")),
            DropdownRow::item(el("Door"), "Door", 1),
            DropdownRow::item(el("Wall"), "Wall", 2),
            DropdownRow::item(el("Window"), "Window", 3),
        ];

        let list = DropdownList::new(rows)
            .query("wa")
            .select_first_by_default(true);

        let mut state = DropdownListState::default();
        list.sync_state(&mut state);

        // Header at index 0 is not selectable; "Wall" is at index 2, which matches "wa"
        assert_eq!(state.highlighted, Some(2));
        assert!(state.keyboard_active);
    }

    #[test]
    fn test_dropdown_list_arrow_navigation_step() {
        use iced::widget::text;

        let el = |s: &'static str| -> Element<'static, i32> { text(s).into() };
        let rows = vec![
            DropdownRow::header(el("Section 1")),
            DropdownRow::item(el("Item 1"), "Item 1", 1),
            DropdownRow::item(el("Item 2"), "Item 2", 2),
            DropdownRow::header(el("Section 2")),
            DropdownRow::item(el("Item 3"), "Item 3", 3),
        ];

        let list = DropdownList::new(rows);

        // Next selectable after Item 2 (index 2) should skip header (index 3) and go to Item 3 (index 4)
        assert_eq!(list.next_selectable(Some(2)), Some(4));

        // Prev selectable before Item 3 (index 4) should skip header (index 3) and go to Item 2 (index 2)
        assert_eq!(list.prev_selectable(Some(4)), Some(2));

        // Wrapping: next selectable after Item 3 (index 4) wraps to Item 1 (index 1)
        assert_eq!(list.next_selectable(Some(4)), Some(1));

        // Wrapping: prev selectable before Item 1 (index 1) wraps to Item 3 (index 4)
        assert_eq!(list.prev_selectable(Some(1)), Some(4));
    }
}
