//! Turning the mouse wheel over the ribbon steps through its tabs: down (or
//! right) to the next tab, up (or left) to the one before, stopping at the
//! first and the last. Tabs that do not show are passed over.
//!
//! The ribbon is wrapped in [`WheelTabs`], which turns a wheel turn that no
//! widget inside took (a scrolling list, say) into `Message::RibbonWheel`;
//! [`Ribbon::on_wheel`] then decides whether, and where, to step.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::time::Instant;
use iced::{keyboard, Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};
use std::time::Duration;

use super::{Opener, Ribbon, RibbonMinimize};
use crate::app::Message;

/// Pixels of touchpad scrolling that make one step.
const PIXELS_PER_STEP: f32 = 50.0;
/// A pause longer than this starts a touchpad's count afresh.
const SCROLL_PAUSE: Duration = Duration::from_millis(300);

/// The wheel's leftover toward the next step.
#[derive(Default, Debug)]
pub(super) struct WheelState {
    /// Steps counted so far, forward positive; a step is taken at ±1.
    pending: f32,
    /// When the last wheel turn came.
    last: Option<Instant>,
}

/// How far a wheel turn moves through the tabs, forward positive: a line is
/// one step, `PIXELS_PER_STEP` pixels are one. The larger of the two axes
/// counts, so a sideways swipe works too. Wheel down and swipe right
/// (negative deltas) go forward.
fn steps(delta: mouse::ScrollDelta) -> f32 {
    let (x, y, scale) = match delta {
        mouse::ScrollDelta::Lines { x, y } => (x, y, 1.0),
        mouse::ScrollDelta::Pixels { x, y } => (x, y, PIXELS_PER_STEP),
    };
    let along = if y.abs() >= x.abs() { y } else { x };
    -along / scale
}

impl Ribbon {
    /// Indices of the modules whose tab shows, in the order they show.
    pub(super) fn shown_tabs(&self) -> Vec<usize> {
        self.modules
            .iter()
            .enumerate()
            .filter(|(i, module)| self.tab_shows(*i, module.id()))
            .map(|(i, _)| i)
            .collect()
    }

    /// A wheel turn over the ribbon. Returns whether another tab came up.
    ///
    /// Nothing moves while the option is off, or while a dropdown, the
    /// right-click menu or the search results are open: the turn is meant
    /// for them. An open panel peek closes as the tab changes.
    pub fn on_wheel(&mut self, delta: mouse::ScrollDelta, now: Instant) -> bool {
        if !self.layout.wheel_switches_tabs
            || self.open_dropdown.is_some()
            || self.menu_open()
            || self.search_results_open()
        {
            self.wheel = WheelState::default();
            return false;
        }
        let turn = steps(delta);
        if turn == 0.0 {
            return false;
        }
        // A pause, or a turn the other way, starts the count afresh.
        let paused = self
            .wheel
            .last
            .is_none_or(|last| now.saturating_duration_since(last) > SCROLL_PAUSE);
        if paused || turn.signum() != self.wheel.pending.signum() {
            self.wheel.pending = 0.0;
        }
        self.wheel.last = Some(now);
        self.wheel.pending += turn;
        if self.wheel.pending.abs() < 1.0 {
            return false;
        }
        // One tab per turn: a fast spin does not skip tabs unseen.
        let forward = self.wheel.pending > 0.0;
        self.wheel.pending = 0.0;
        let shown = self.shown_tabs();
        let next = if forward {
            shown.iter().copied().find(|&i| i > self.active)
        } else {
            shown.iter().rev().copied().find(|&i| i < self.active)
        };
        let Some(next) = next else {
            return false;
        };
        self.step_to_tab(next);
        true
    }

    /// Bring tab `index` up as the wheel does: a popup of the minimized
    /// ribbon follows it, a panel peek closes.
    fn step_to_tab(&mut self, index: usize) {
        self.close_dropdown();
        if self.layout.minimize == RibbonMinimize::Tabs && self.popup_open() {
            self.open_popup(Opener::Tab(index));
            return;
        }
        // A popup due to open from the tab under the pointer would bring
        // that tab back.
        self.close_popup();
        self.select(index);
    }
}

/// The ribbon, sending `Message::RibbonWheel` for a wheel turn over it that
/// nothing inside took and that no modifier key changes.
pub(super) struct WheelTabs<'a> {
    content: Element<'a, Message>,
    enabled: bool,
}

impl<'a> WheelTabs<'a> {
    pub(super) fn new(content: impl Into<Element<'a, Message>>, enabled: bool) -> Self {
        Self {
            content: content.into(),
            enabled,
        }
    }
}

#[derive(Default)]
struct Modifiers(keyboard::Modifiers);

impl<'a> Widget<Message, Theme, Renderer> for WheelTabs<'a> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Modifiers>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Modifiers::default())
    }

    fn diff(&mut self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if let Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) = event {
            tree.state.downcast_mut::<Modifiers>().0 = *modifiers;
        }
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
        if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event {
            let plain = tree.state.downcast_ref::<Modifiers>().0.is_empty();
            if self.enabled
                && plain
                && !shell.is_event_captured()
                && cursor.is_over(layout.bounds())
            {
                shell.publish(Message::RibbonWheel(*delta));
                shell.capture_event();
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
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
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<WheelTabs<'a>> for Element<'a, Message> {
    fn from(w: WheelTabs<'a>) -> Self {
        Element::new(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ribbon::{MenuTarget, MinimizeMsg, RibbonMenuMsg};
    use iced::widget::{container, scrollable, text};
    use iced::{Fill, Point};

    const DOWN: mouse::ScrollDelta = mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 };
    const UP: mouse::ScrollDelta = mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 };

    /// The ribbon on its first shown tab.
    fn ribbon() -> Ribbon {
        let mut ribbon = Ribbon::default();
        let first = ribbon.shown_tabs()[0];
        ribbon.select(first);
        ribbon
    }

    #[test]
    fn the_wheel_steps_through_the_shown_tabs_and_stops_at_the_ends() {
        let mut ribbon = ribbon();
        let shown = ribbon.shown_tabs();
        assert!(shown.len() > 2, "enough tabs to step through");
        let now = Instant::now();

        // Up on the first tab stays there.
        assert!(!ribbon.on_wheel(UP, now));
        assert_eq!(ribbon.active, shown[0]);

        for &next in &shown[1..] {
            assert!(ribbon.on_wheel(DOWN, now));
            assert_eq!(ribbon.active, next);
        }
        // Down on the last tab stays there.
        assert!(!ribbon.on_wheel(DOWN, now));
        assert_eq!(ribbon.active, *shown.last().unwrap());

        assert!(ribbon.on_wheel(UP, now));
        assert_eq!(ribbon.active, shown[shown.len() - 2]);
    }

    #[test]
    fn tabs_that_do_not_show_are_passed_over() {
        let mut ribbon = ribbon();
        let shown = ribbon.shown_tabs();
        // Neither the Layout module nor a contextual tab out of context has
        // a tab on the row.
        for id in ["layout", "block_editor", "table_cell", "text_editor"] {
            if let Some(index) = ribbon.modules.iter().position(|m| m.id() == id) {
                assert!(!shown.contains(&index), "{id} shows no tab");
            }
        }
        // A tab the user hid is passed over.
        let hidden = shown[1];
        let id = ribbon.modules[hidden].id().to_string();
        ribbon.layout.hidden_tabs.insert(id);
        assert!(ribbon.on_wheel(DOWN, Instant::now()));
        assert_eq!(ribbon.active, shown[2]);
    }

    #[test]
    fn a_sideways_swipe_steps_too() {
        let mut ribbon = ribbon();
        let shown = ribbon.shown_tabs();
        let now = Instant::now();
        let right = mouse::ScrollDelta::Lines { x: -1.0, y: 0.0 };
        assert!(ribbon.on_wheel(right, now));
        assert_eq!(ribbon.active, shown[1]);
        let left = mouse::ScrollDelta::Lines { x: 1.0, y: 0.0 };
        assert!(ribbon.on_wheel(left, now));
        assert_eq!(ribbon.active, shown[0]);
    }

    #[test]
    fn touchpad_pixels_add_up_to_a_step() {
        let mut ribbon = ribbon();
        let shown = ribbon.shown_tabs();
        let t0 = Instant::now();
        let px = |y: f32| mouse::ScrollDelta::Pixels { x: 0.0, y };

        assert!(!ribbon.on_wheel(px(-30.0), t0));
        assert_eq!(ribbon.active, shown[0]);
        assert!(ribbon.on_wheel(px(-30.0), t0 + Duration::from_millis(20)));
        assert_eq!(ribbon.active, shown[1]);

        // A pause starts the count afresh.
        assert!(!ribbon.on_wheel(px(-30.0), t0 + Duration::from_millis(40)));
        let later = t0 + Duration::from_millis(40) + SCROLL_PAUSE * 2;
        assert!(!ribbon.on_wheel(px(-30.0), later));
        assert_eq!(ribbon.active, shown[1]);

        // So does a turn the other way.
        assert!(!ribbon.on_wheel(px(40.0), later + Duration::from_millis(10)));
        assert!(!ribbon.on_wheel(px(-40.0), later + Duration::from_millis(20)));
        assert_eq!(ribbon.active, shown[1]);
    }

    #[test]
    fn a_fast_spin_moves_one_tab_per_turn() {
        let mut ribbon = ribbon();
        let shown = ribbon.shown_tabs();
        let spin = mouse::ScrollDelta::Lines { x: 0.0, y: -5.0 };
        assert!(ribbon.on_wheel(spin, Instant::now()));
        assert_eq!(ribbon.active, shown[1]);
    }

    #[test]
    fn the_wheel_leaves_the_tabs_alone_when_off_or_meant_for_a_list() {
        let now = Instant::now();

        let mut ribbon = ribbon();
        let first = ribbon.active;
        ribbon.on_menu(RibbonMenuMsg::SetWheelTabs(false));
        assert!(!ribbon.on_wheel(DOWN, now));
        assert_eq!(ribbon.active, first);

        let mut ribbon = self::ribbon();
        ribbon.open_dropdown = Some("CIRCLE".into());
        assert!(!ribbon.on_wheel(DOWN, now));
        assert_eq!(ribbon.active, first);

        let mut ribbon = self::ribbon();
        ribbon.on_menu(RibbonMenuMsg::Open(Point::ORIGIN, MenuTarget::Ribbon));
        assert!(!ribbon.on_wheel(DOWN, now));
        assert_eq!(ribbon.active, first);
    }

    #[test]
    fn an_open_tab_popup_follows_the_wheel() {
        let mut ribbon = ribbon();
        ribbon.on_minimize(MinimizeMsg::Toggle, Instant::now());
        assert_eq!(ribbon.minimize(), RibbonMinimize::Tabs);
        let shown = ribbon.shown_tabs();

        // Closed, only the tab changes.
        assert!(ribbon.on_wheel(DOWN, Instant::now()));
        assert_eq!(ribbon.active, shown[1]);
        assert!(!ribbon.popup_open());

        // Open, it shows the new tab's tools.
        ribbon.open_popup(Opener::Tab(shown[1]));
        assert!(ribbon.popup_open());
        assert!(ribbon.on_wheel(DOWN, Instant::now()));
        assert_eq!(ribbon.active, shown[2]);
        assert!(ribbon.popup_open());
    }

    #[test]
    fn a_hover_due_to_open_its_tab_does_not_undo_the_wheel() {
        let mut ribbon = ribbon();
        ribbon.on_minimize(MinimizeMsg::Toggle, Instant::now());
        let shown = ribbon.shown_tabs();
        let t0 = Instant::now();
        ribbon.on_minimize(MinimizeMsg::EnterOpener(Opener::Tab(shown[0])), t0);
        assert!(ribbon.on_wheel(DOWN, t0));
        ribbon.tick(t0 + Duration::from_millis(500));
        assert_eq!(ribbon.active, shown[1]);
    }

    #[test]
    fn an_open_panel_peek_closes_as_the_tab_changes() {
        let mut ribbon = ribbon();
        ribbon.collapsed_open = Some("Draw".into());
        assert!(ribbon.on_wheel(DOWN, Instant::now()));
        assert!(ribbon.collapsed_open.is_none());
    }

    #[test]
    fn stepping_from_a_tab_without_a_place_on_the_row_finds_its_neighbours() {
        let mut ribbon = ribbon();
        let Some(layout) = ribbon.modules.iter().position(|m| m.id() == "layout") else {
            return;
        };
        let shown = ribbon.shown_tabs();
        ribbon.select(layout);
        let after = shown.iter().copied().find(|&i| i > layout);
        assert_eq!(ribbon.on_wheel(DOWN, Instant::now()), after.is_some());
        if let Some(after) = after {
            assert_eq!(ribbon.active, after);
        }
    }

    fn wheel_messages(
        content: Element<'static, Message>,
        enabled: bool,
        events: Vec<Event>,
    ) -> Vec<mouse::ScrollDelta> {
        let _one = crate::headless_ui_lock();
        let element: Element<'static, Message> =
            WheelTabs::new(container(content).width(Fill).height(Fill), enabled).into();
        let mut ui = iced_test::simulator::Simulator::with_size(
            iced::Settings::default(),
            (400.0, 200.0),
            element,
        );
        ui.point_at(Point::new(200.0, 100.0));
        ui.simulate(events);
        ui.into_messages()
            .filter_map(|message| match message {
                Message::RibbonWheel(delta) => Some(delta),
                _ => None,
            })
            .collect()
    }

    fn scrolled(delta: mouse::ScrollDelta) -> Event {
        Event::Mouse(mouse::Event::WheelScrolled { delta })
    }

    #[test]
    fn a_wheel_turn_over_the_ribbon_is_sent_on() {
        let sent = wheel_messages(text("ribbon").into(), true, vec![scrolled(DOWN)]);
        assert_eq!(sent, vec![DOWN]);
        let off = wheel_messages(text("ribbon").into(), false, vec![scrolled(DOWN)]);
        assert!(off.is_empty(), "off, the turn is not sent");
    }

    #[test]
    fn a_wheel_turn_with_a_modifier_key_is_not_sent() {
        let sent = wheel_messages(
            text("ribbon").into(),
            true,
            vec![
                Event::Keyboard(keyboard::Event::ModifiersChanged(keyboard::Modifiers::CTRL)),
                scrolled(DOWN),
            ],
        );
        assert!(sent.is_empty());
    }

    #[test]
    fn a_list_inside_keeps_its_wheel_turns() {
        let long = iced::widget::column((0..50).map(|i| text(format!("row {i}")).into()));
        let list = scrollable(long).width(Fill).height(Fill);
        let sent = wheel_messages(list.into(), true, vec![scrolled(DOWN)]);
        assert!(sent.is_empty(), "the list scrolled instead");
    }
}

/// The whole ribbon, rendered.
#[cfg(test)]
mod view_tests {
    use super::*;
    use crate::ui::ribbon::RibbonLayout;
    use iced::Point;
    use iced_test::simulator::Simulator;

    const WIDTH: f32 = 2400.0;

    fn with_ui_test(test: impl FnOnce() + Send + 'static) {
        let _guard = crate::headless_ui_lock();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(test)
            .expect("spawn UI test")
            .join()
            .unwrap_or_else(|error| std::panic::resume_unwind(error));
    }

    /// The ribbon laid out `WIDTH` wide, centered or not.
    fn ribbon(centered: bool) -> Ribbon {
        let mut ribbon = Ribbon::default();
        ribbon.set_layout(RibbonLayout {
            centered,
            ..RibbonLayout::default()
        });
        ribbon.set_window_size((WIDTH, 900.0));
        ribbon
    }

    /// The bounds of the first shown tab's label.
    fn first_tab(ribbon: &Ribbon, ui: &mut Simulator<'_, Message>) -> Rectangle {
        let first = ribbon.shown_tabs()[0];
        let module = &ribbon.modules[first];
        let title = crate::i18n::ribbon_module_title(module.id(), module.title());
        ui.find(title.as_ref())
            .expect("first tab shows")
            .visible_bounds()
            .expect("first tab visible")
    }

    /// With `OCS_SAVE_RIBBON_CAPTURES` set to a folder, the ribbon is saved
    /// there as a picture to look at.
    fn capture(ui: &mut Simulator<'_, Message>, name: &str) {
        if let Some(directory) = std::env::var_os("OCS_SAVE_RIBBON_CAPTURES") {
            assert!(ui
                .snapshot(&iced::Theme::Dark)
                .expect("render ribbon")
                .matches_image(std::path::PathBuf::from(directory).join(format!("{name}.png")))
                .expect("write ribbon capture"));
        }
    }

    #[test]
    fn centering_moves_the_tabs_toward_the_middle_of_a_wide_ribbon() {
        with_ui_test(|| {
            let left_ribbon = ribbon(false);
            let mut ui = Simulator::with_size(
                iced::Settings::default(),
                (WIDTH, 260.0),
                left_ribbon.view(None, false, false, false, 0, 0, false, None),
            );
            let left = first_tab(&left_ribbon, &mut ui);
            capture(&mut ui, "ribbon-left");

            let centered_ribbon = ribbon(true);
            let mut ui = Simulator::with_size(
                iced::Settings::default(),
                (WIDTH, 260.0),
                centered_ribbon.view(None, false, false, false, 0, 0, false, None),
            );
            let centered = first_tab(&centered_ribbon, &mut ui);
            capture(&mut ui, "ribbon-centered");

            assert!(left.x < WIDTH / 4.0, "left aligned, the tabs start left");
            assert!(
                centered.x > left.x + WIDTH / 8.0,
                "centered, the tabs move right: {} vs {}",
                centered.x,
                left.x
            );
        });
    }

    #[test]
    fn a_wheel_turn_over_the_rendered_ribbon_is_sent_on() {
        with_ui_test(|| {
            let ribbon = ribbon(false);
            let mut ui = Simulator::with_size(
                iced::Settings::default(),
                (WIDTH, 260.0),
                ribbon.view(None, false, false, false, 0, 0, false, None),
            );
            let tab = first_tab(&ribbon, &mut ui);
            ui.point_at(Point::new(tab.center_x(), tab.center_y()));
            ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            })]);
            assert!(ui
                .into_messages()
                .any(|message| matches!(message, Message::RibbonWheel(_))));
        });
    }
}
