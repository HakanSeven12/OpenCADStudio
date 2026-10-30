//! The app side of the QuickCalc pallet: system variables, the clipboard,
//! the command line, picks and the profile settings. The pallet's own state
//! and logic are `ui::window::quick_calc`.

use std::collections::HashMap;
use std::sync::Arc;

use glam::DVec3;
use iced::{Element, Task};

use super::{DocumentTab, Message, OpenCADStudio};
use crate::app::calc::{CalcContext, SysVar, SysVars};
use crate::command::CadCommand;
use crate::modules::draw::inquiry::calc_pick::{CalcPickCommand, PickKind};
use crate::ui::dock::PanelId;
use crate::ui::window::quick_calc::{full_text, FormMsg, QuickCalcMsg, VarMsg, INPUT_ID};

/// The drawing's system variables `getvar` can read, as of now.
pub(super) fn sysvars(tab: &DocumentTab) -> SysVars {
    let h = &tab.scene.document.header;
    let n = |v: f64| SysVar::Number(v);
    let i = |v: i16| SysVar::Number(f64::from(v));
    let mut map: HashMap<&'static str, SysVar> = HashMap::from([
        ("lunits", i(h.linear_unit_format)),
        ("luprec", i(h.linear_unit_precision)),
        ("aunits", i(h.angular_unit_format)),
        ("auprec", i(h.angular_unit_precision)),
        ("angbase", n(h.angle_base)),
        ("angdir", i(h.angle_direction)),
        ("insunits", i(h.insertion_units)),
        ("measurement", i(h.measurement)),
        ("ltscale", n(h.linetype_scale)),
        ("celtscale", n(h.current_entity_linetype_scale)),
        ("textsize", n(h.text_height)),
        ("filletrad", n(h.fillet_radius)),
        ("chamfera", n(h.chamfer_distance_a)),
        ("chamferb", n(h.chamfer_distance_b)),
        ("pdmode", i(h.point_display_mode)),
        ("pdsize", n(h.point_display_size)),
        ("plinewid", n(h.polyline_width)),
        ("tracewid", n(h.trace_width)),
        ("sketchinc", n(h.sketch_increment)),
        ("thickness", n(h.thickness)),
        ("elevation", n(h.elevation)),
        ("dimscale", n(h.dim_scale)),
        ("mirrtext", n(f64::from(u8::from(h.mirror_text)))),
        ("osmode", n(f64::from(h.object_snap_mode))),
        ("userr1", n(h.user_real1)),
        ("userr2", n(h.user_real2)),
        ("userr3", n(h.user_real3)),
        ("userr4", n(h.user_real4)),
        ("userr5", n(h.user_real5)),
        ("useri1", i(h.user_int1)),
        ("useri2", i(h.user_int2)),
        ("useri3", i(h.user_int3)),
        ("useri4", i(h.user_int4)),
        ("useri5", i(h.user_int5)),
        ("clayer", SysVar::Text),
        ("ctextstyle", SysVar::Text),
        ("celtype", SysVar::Text),
    ]);
    let base = h.model_space_insertion_base;
    map.insert("insbase", SysVar::Point(DVec3::new(base.x, base.y, base.z)));
    let (lo, hi) = (h.model_space_limits_min, h.model_space_limits_max);
    map.insert("limmin", SysVar::Point(DVec3::new(lo.x, lo.y, 0.0)));
    map.insert("limmax", SysVar::Point(DVec3::new(hi.x, hi.y, 0.0)));
    if let Some((min, max)) = tab.scene.model_space_extents() {
        map.insert("extmin", SysVar::Point(min.as_dvec3()));
        map.insert("extmax", SysVar::Point(max.as_dvec3()));
    }
    SysVars(Arc::new(move |name: &str| map.get(name).copied()))
}

/// Millimetres per drawing unit; a unitless drawing counts as millimetres.
pub(super) fn base_unit(tab: &DocumentTab) -> f64 {
    super::properties::insunits_to_mm(tab.scene.document.header.insertion_units).unwrap_or(1.0)
}

/// The drawing unit as the preview names it (`in mm`).
fn unit_label(tab: &DocumentTab) -> &'static str {
    match tab.scene.document.header.insertion_units {
        1 => "in",
        2 => "ft",
        3 => "mi",
        5 => "cm",
        6 => "m",
        7 => "km",
        10 => "yd",
        14 => "dm",
        13 => "µm",
        _ => "mm",
    }
}

fn focus_input<T: Send + 'static>() -> Task<T> {
    let id = iced::widget::Id::new(INPUT_ID);
    Task::batch([
        iced::widget::operation::focus(id.clone()),
        iced::widget::operation::move_cursor_to_end(id),
    ])
}

fn copy_to_clipboard(text: String) -> Task<Message> {
    #[cfg(target_arch = "wasm32")]
    {
        crate::sys::write_clipboard_text(&text);
        Task::none()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        iced::clipboard::write(text).discard()
    }
}

impl OpenCADStudio {
    pub(in crate::app) fn quick_calc_view<'a>(
        &'a self,
        tab: &'a DocumentTab,
        width: f32,
        chrome: crate::ui::dock::Chrome,
    ) -> Element<'a, Message> {
        let pick_step = tab.active_cmd.as_ref().and_then(|c| c.quick_calc_step());
        crate::ui::window::quick_calc::view(
            &self.quick_calc,
            crate::ui::window::quick_calc::Inputs {
                sysvars: Some(sysvars(tab)),
                base_unit: base_unit(tab),
                unit_label: unit_label(tab),
                pick_step,
                theme: self.active_theme.clone(),
            },
            width,
            chrome,
        )
    }

    fn quick_calc_context(&self) -> CalcContext {
        let tab = &self.tabs[self.active_tab];
        self.quick_calc.context(base_unit(tab), Some(sysvars(tab)))
    }

    /// QUICKCALC / QC: show the pallet (docked on the right the first time)
    /// with the input focused, or close it when it is showing.
    pub(in crate::app) fn toggle_quick_calc(&mut self) -> Task<Message> {
        if self.dock_panel_visible(PanelId::QuickCalc) {
            return self.on_dock(crate::ui::dock::DockMsg::Close(PanelId::QuickCalc));
        }
        self.quick_calc.show = true;
        self.dock_open_at_default(PanelId::QuickCalc);
        self.save_config();
        focus_input()
    }

    fn save_quick_calc_settings(&mut self) {
        self.persist_settings_if_changed();
    }

    /// Escape and Ctrl+Enter reach the app as shortcuts even while the input
    /// has focus. Returns the task when QuickCalc handled the key.
    pub(in crate::app) fn quick_calc_shortcut(&mut self, key: &str) -> Option<Task<Message>> {
        if !self.dock_panel_visible(PanelId::QuickCalc) {
            return None;
        }
        let base = key.rsplit('+').next().unwrap_or(key);
        // An open flyout closes first, wherever the focus is.
        if base == "ESCAPE" && self.quick_calc.flyout.is_some() {
            self.quick_calc.flyout = None;
            return Some(Task::none());
        }
        // Escape in the variable form cancels it.
        if base == "ESCAPE" && self.quick_calc.typing && self.quick_calc.form.is_some() {
            self.quick_calc.form = None;
            return Some(Task::none());
        }
        if !self.quick_calc.typing {
            return None;
        }
        let accel = key.starts_with("CTRL+") || key.starts_with("CMD+");
        if base == "ESCAPE" && self.tabs[self.active_tab].active_cmd.is_none() {
            self.quick_calc.typing = false;
            self.quick_calc.input.clear();
            self.quick_calc.hist_idx = None;
            return Some(Task::none());
        }
        if accel && matches!(base, "ENTER" | "RETURN") {
            return Some(self.on_quick_calc(QuickCalcMsg::PasteToCommandLine));
        }
        None
    }

    pub(in crate::app) fn on_quick_calc(&mut self, msg: QuickCalcMsg) -> Task<Message> {
        // Only these evaluate the input; building the context (every
        // variable, the drawing's system variables) for a keystroke is waste.
        let ctx = matches!(
            msg,
            QuickCalcMsg::Submit
                | QuickCalcMsg::Key(crate::ui::window::quick_calc::Key::Eval)
                | QuickCalcMsg::PasteToCommandLine
                | QuickCalcMsg::Copy
                | QuickCalcMsg::Memory(_)
        )
        .then(|| self.quick_calc_context());
        let ctx = || ctx.as_ref().expect("built for this message");
        let qc = &mut self.quick_calc;
        // Doing anything else in the pallet closes an open flyout.
        if !matches!(
            msg,
            QuickCalcMsg::Flyout(_)
                | QuickCalcMsg::CloseFlyout
                | QuickCalcMsg::Key(_)
                | QuickCalcMsg::Memory(_)
                | QuickCalcMsg::MemoryInsert(_)
                | QuickCalcMsg::FlashExpired
                | QuickCalcMsg::HistoryStep(_)
        ) {
            qc.flyout = None;
        }
        match msg {
            QuickCalcMsg::Input(text) => {
                qc.input = text;
                qc.hist_idx = None;
                qc.typing = true;
                Task::none()
            }
            QuickCalcMsg::Submit | QuickCalcMsg::Key(crate::ui::window::quick_calc::Key::Eval) => {
                qc.typing = true;
                if qc.submit(ctx()).is_some_and(|done| done.assign.is_some()) {
                    self.save_quick_calc_settings();
                }
                focus_input()
            }
            QuickCalcMsg::PasteToCommandLine => {
                let Some(value) = qc.current_value(ctx()) else {
                    qc.set_flash(crate::t!("Nothing to paste yet").into_owned());
                    return Task::none();
                };
                let text = full_text(&value);
                qc.set_flash(
                    crate::tf!("Sent {} to the command line", value.display()).into_owned(),
                );
                qc.typing = false;
                self.command_line.input.push_str(&text);
                iced::widget::operation::focus(iced::widget::Id::new(
                    crate::ui::command_line::CMD_INPUT_ID,
                ))
            }
            QuickCalcMsg::Copy => match qc.current_value(ctx()) {
                Some(value) => {
                    qc.set_flash(crate::tf!("Copied {}", value.display()).into_owned());
                    copy_to_clipboard(full_text(&value))
                }
                None => {
                    qc.set_flash(crate::t!("Nothing to copy yet").into_owned());
                    Task::none()
                }
            },
            QuickCalcMsg::HistoryCopy(i) => match qc.history.get(i) {
                Some(entry) => {
                    let value = entry.value;
                    qc.set_flash(crate::tf!("Copied {}", value.display()).into_owned());
                    copy_to_clipboard(full_text(&value))
                }
                None => Task::none(),
            },
            QuickCalcMsg::Pick(kind) => self.quick_calc_pick(kind),
            QuickCalcMsg::CancelPick => {
                let i = self.active_tab;
                if self.tabs[i]
                    .active_cmd
                    .as_ref()
                    .is_some_and(|c| c.quick_calc_step().is_some())
                {
                    return self.update(Message::CommandEscape);
                }
                self.quick_calc.pick = None;
                Task::none()
            }
            QuickCalcMsg::Picked(value) => {
                qc.pick = None;
                qc.insert_value(&value);
                qc.typing = true;
                focus_input()
            }
            QuickCalcMsg::ToggleAngleMode => {
                qc.toggle_angle_mode();
                self.save_quick_calc_settings();
                Task::none()
            }
            QuickCalcMsg::ClearHistory => {
                qc.history.clear();
                qc.hist_idx = None;
                Task::none()
            }
            QuickCalcMsg::HistoryLoad(i) => {
                if let Some(entry) = qc.history.get(i) {
                    qc.input = entry.expr.clone();
                    qc.hist_idx = Some(i);
                    qc.typing = true;
                }
                focus_input()
            }
            QuickCalcMsg::HistoryStep(up) => {
                qc.history_step(up);
                iced::widget::operation::move_cursor_to_end(iced::widget::Id::new(INPUT_ID))
            }
            QuickCalcMsg::Key(key) => {
                if let Some(status) = qc.press(key) {
                    qc.set_flash(status);
                }
                qc.typing = true;
                focus_input()
            }
            QuickCalcMsg::Memory(op) => {
                if let Some(status) = qc.memory_op(op, ctx()) {
                    qc.set_flash(status);
                }
                qc.typing = true;
                focus_input()
            }
            QuickCalcMsg::MemoryInsert(index) => {
                qc.memory_insert(index);
                qc.typing = true;
                focus_input()
            }
            QuickCalcMsg::SetKeypadMode(mode) => {
                qc.set_keypad_mode(mode);
                self.save_quick_calc_settings();
                Task::none()
            }
            QuickCalcMsg::Flyout(flyout) => {
                qc.toggle_flyout(flyout);
                Task::none()
            }
            QuickCalcMsg::CloseFlyout => {
                qc.flyout = None;
                Task::none()
            }
            QuickCalcMsg::Toggle(section) => {
                qc.toggle(section);
                self.save_quick_calc_settings();
                Task::none()
            }
            QuickCalcMsg::UnitCat(cat) => {
                qc.set_unit_cat(cat);
                Task::none()
            }
            QuickCalcMsg::UnitFrom(unit) => {
                qc.unit_from = unit;
                Task::none()
            }
            QuickCalcMsg::UnitTo(unit) => {
                qc.unit_to = unit;
                Task::none()
            }
            QuickCalcMsg::UnitValue(value) => {
                qc.unit_value = value;
                Task::none()
            }
            QuickCalcMsg::UnitSwap => {
                (qc.unit_from, qc.unit_to) = (qc.unit_to, qc.unit_from);
                Task::none()
            }
            QuickCalcMsg::UnitInsert => {
                if let Some(literal) = qc.converted_literal() {
                    qc.append(&literal);
                    qc.typing = true;
                }
                focus_input()
            }
            QuickCalcMsg::Var(msg) => self.on_quick_calc_variable(msg),
            QuickCalcMsg::FlashExpired => {
                qc.expire_flash();
                Task::none()
            }
        }
    }

    fn on_quick_calc_variable(&mut self, msg: VarMsg) -> Task<Message> {
        let tab = &self.tabs[self.active_tab];
        let base = base_unit(tab);
        // Only New and Save evaluate; the snapshot of the drawing's system
        // variables is built for them alone.
        let sys = matches!(msg, VarMsg::New | VarMsg::Form(FormMsg::Save)).then(|| sysvars(tab));
        let qc = &mut self.quick_calc;
        let insert = |qc: &mut crate::ui::window::quick_calc::QuickCalcPalette, name: &str| {
            qc.append(&format!("${name}"));
            qc.typing = true;
            focus_input()
        };
        match msg {
            VarMsg::New => {
                let ctx = qc.context(base, sys);
                let value = match qc.preview(&ctx) {
                    crate::ui::window::quick_calc::Preview::Ok(value) => Some(value),
                    _ => None,
                };
                qc.open_new_form(value);
                Task::none()
            }
            VarMsg::Edit => {
                qc.open_edit_form();
                Task::none()
            }
            VarMsg::Delete => {
                if let Some(name) = qc.delete_selected() {
                    qc.set_flash(crate::tf!("Deleted ${}", name).into_owned());
                    self.save_quick_calc_settings();
                }
                Task::none()
            }
            VarMsg::Return => match qc.selected_var.clone() {
                Some(name) => insert(qc, &name),
                None => Task::none(),
            },
            VarMsg::Select(name) => {
                qc.selected_var = Some(name);
                Task::none()
            }
            VarMsg::Insert(name) => {
                qc.selected_var = Some(name.clone());
                insert(qc, &name)
            }
            VarMsg::ToggleCategory(category) => {
                qc.toggle_category(category);
                Task::none()
            }
            VarMsg::Form(FormMsg::Save) => {
                if qc.save_form(base, sys).is_some() {
                    self.save_quick_calc_settings();
                }
                Task::none()
            }
            VarMsg::Form(FormMsg::Cancel) => {
                qc.form = None;
                Task::none()
            }
            VarMsg::Form(msg) => {
                qc.form_msg(msg);
                qc.typing = true;
                Task::none()
            }
        }
    }

    /// Start a pick in the drawing. Only one command runs at a time, so a
    /// pick waits until the current command has finished.
    fn quick_calc_pick(&mut self, kind: PickKind) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].active_cmd.is_some() {
            self.quick_calc
                .set_flash(crate::t!("Finish the current command first").into_owned());
            return Task::none();
        }
        self.quick_calc.pick = Some(kind);
        self.quick_calc.typing = false;
        let cmd = CalcPickCommand::new(kind, self.quick_calc.mode);
        self.command_line.push_info(&cmd.prompt());
        self.tabs[i].active_cmd = Some(Box::new(cmd));
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::calc::Value;
    use crate::ui::window::quick_calc::{Flyout, KeypadMode, MemoryOp};

    /// A millimetre drawing with the pallet open.
    fn app() -> OpenCADStudio {
        let mut app = OpenCADStudio::new_for_test();
        let _ = app.automation_op(r#"{"op":"new"}"#);
        let i = app.active_tab;
        app.tabs[i].scene.document.header.insertion_units = 4;
        app
    }

    fn send(app: &mut OpenCADStudio, msgs: impl IntoIterator<Item = QuickCalcMsg>) {
        for msg in msgs {
            let _ = app.on_quick_calc(msg);
        }
    }

    fn enter(app: &mut OpenCADStudio, input: &str) {
        send(
            app,
            [QuickCalcMsg::Input(input.into()), QuickCalcMsg::Submit],
        );
    }

    #[test]
    fn quickcalc_toggles_and_cal_stays_a_command() {
        let mut app = app();
        let _ = app.dispatch_command("QUICKCALC");
        assert!(app.dock_panel_visible(PanelId::QuickCalc));
        let _ = app.dispatch_command("QC");
        assert!(!app.dock_panel_visible(PanelId::QuickCalc));
        let _ = app.dispatch_command("CAL 2+3");
        let last = &app.command_line.history.last().unwrap().text;
        assert!(last.ends_with("= 5"), "{last}");
    }

    #[test]
    fn floats_tall_enough_for_the_keypad() {
        let app = app();
        let float_height = crate::ui::window::quick_calc::float_height();
        let workspace = |h: f32| {
            app.tabs[app.active_tab]
                .scene
                .selection
                .borrow_mut()
                .view
                .vp_size
                .1 = h;
        };
        // A short workspace: other pallets float at 60 % of it.
        workspace(700.0);
        let (_, h) = app.dock_float_size(PanelId::QuickCalc);
        assert!(h >= float_height, "{h} < {float_height}");
        assert!(app.dock_float_size(PanelId::Layers).1 < float_height);
        // Never taller than the workspace.
        workspace(300.0);
        assert_eq!(app.dock_float_size(PanelId::QuickCalc).1, 300.0);
    }

    #[test]
    fn variables_are_global_and_getvar_reads_the_drawing() {
        let mut app = app();
        let i = app.active_tab;
        enter(&mut app, "$wall = 3.6m");
        enter(&mut app, "$wall = $wall / 2");
        // Kept in the user settings, not in the drawing.
        assert!(app.tabs[i].scene.named_parameters().is_empty());
        let saved = &app.current_settings().quick_calc.variables;
        assert_eq!(saved.len(), 1);
        assert_eq!(
            saved[0].value,
            Some(crate::ui::window::quick_calc::StoredValue::Number(1800.0))
        );
        // Delete removes it at once and says so.
        send(
            &mut app,
            [
                QuickCalcMsg::Var(VarMsg::Select("wall".into())),
                QuickCalcMsg::Var(VarMsg::Delete),
            ],
        );
        assert!(app.quick_calc.variables.is_empty());
        assert!(app.current_settings().quick_calc.variables.is_empty());
        assert!(app.quick_calc.flash.is_some());
        enter(&mut app, "getvar(insunits) + getvar(luprec)");
        let expected = 4.0 + f64::from(app.tabs[i].scene.document.header.linear_unit_precision);
        assert_eq!(app.quick_calc.ans, Some(Value::Number(expected)));
    }

    #[test]
    fn picks_run_as_a_command() {
        let mut app = app();
        let i = app.active_tab;
        // Not while another command runs.
        let _ = app.dispatch_command("LINE");
        send(&mut app, [QuickCalcMsg::Pick(PickKind::Point)]);
        assert_eq!(app.tabs[i].active_cmd.as_ref().unwrap().name(), "LINE");
        let _ = app.update(Message::CommandEscape);
        send(
            &mut app,
            [
                QuickCalcMsg::Input("wall + ".into()),
                QuickCalcMsg::Pick(PickKind::Distance),
            ],
        );
        let cmd = app.tabs[i].active_cmd.as_mut().unwrap();
        assert_eq!(cmd.quick_calc_step(), Some(1));
        cmd.on_point(DVec3::ZERO);
        let done = cmd.on_point(DVec3::new(0.0, 3600.0, 0.0));
        let _ = app.apply_cmd_result(done);
        assert!(app.tabs[i].active_cmd.is_none());
        // The routed message, as the runtime delivers it.
        send(&mut app, [QuickCalcMsg::Picked(Value::Number(3600.0))]);
        assert_eq!(app.quick_calc.input, "wall + 3600");
    }

    #[test]
    fn keyboard_paste_and_memory() {
        let mut app = app();
        app.quick_calc.show = true;
        app.dock_open_at_default(PanelId::QuickCalc);
        // Ctrl+Enter sends the value to the command line as a coordinate.
        send(&mut app, [QuickCalcMsg::Input("[1,2] * 2".into())]);
        assert!(app.quick_calc_shortcut("CTRL+ENTER").is_some());
        assert_eq!(app.command_line.input, "2,4");
        // Escape closes a flyout first, then clears what is being typed.
        send(
            &mut app,
            [
                QuickCalcMsg::SetKeypadMode(KeypadMode::Scientific),
                QuickCalcMsg::Input("2 * 435".into()),
                QuickCalcMsg::Flyout(Flyout::Trigonometry),
            ],
        );
        assert!(app.quick_calc_shortcut("ESCAPE").is_some());
        assert_eq!(app.quick_calc.flyout, None);
        send(&mut app, [QuickCalcMsg::Memory(MemoryOp::Store)]);
        assert!(app.quick_calc_shortcut("ESCAPE").is_some());
        assert!(app.quick_calc.input.is_empty());
        // Not while typing elsewhere.
        app.quick_calc.input = "12".into();
        app.quick_calc.typing = false;
        assert!(app.quick_calc_shortcut("ESCAPE").is_none());
        // The memory flyout inserts a stored value.
        send(
            &mut app,
            [
                QuickCalcMsg::Input(String::new()),
                QuickCalcMsg::Flyout(Flyout::Memory),
                QuickCalcMsg::MemoryInsert(0),
            ],
        );
        assert_eq!(app.quick_calc.input, "870");
        assert_eq!(app.quick_calc.flyout, None);
    }
}
