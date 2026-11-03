//! The Quick Access Toolbar: the items the user chose (`RibbonLayout::qat`),
//! each drawn by its kind, with the customize button at the end.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use rustc_hash::FxHashMap as HashMap;

use iced::widget::tooltip::Position as TipPos;
use iced::widget::{button, container, row, text, tooltip, Space};
use iced::{Background, Element, Fill, Length, Theme};

use super::layout::{QatBuiltin, QatItem};
use super::ribbon_menu::{check_row, fit_width, MenuTarget, RightPress};
use super::widgets::{
    combo_btn_style, layer_combo_face, make_tip, qat_split_part_style, quick_access_btn,
    render_history_control, split_outline, tip_style, top_hist_btn_style, CENTER_LAYER_COMBO,
    DIM_LAYER_COMBO, LAYER_COMBO_ID, MLEADER_LAYER_COMBO, PROP_COLOR_ID, PROP_LINETYPE_ID,
    PROP_LW_ID, SMALL_ICON, TOP_HIST_GAP,
};
use super::{Ribbon, REDO_HISTORY_ID, UNDO_HISTORY_ID};
use crate::app::Message;
use crate::modules::IconKind;
use crate::ui::icons;
use crate::ui::style::{chrome, ribbon_metrics as metrics};
use crate::ui::wrap_bar::PosReport;

/// The layer combo in the toolbar; it opens the ribbon combo's list.
pub(crate) const QAT_LAYER_COMBO_ID: &str = "QAT_LAYER_COMBO";
/// The Switch Windows and User Interface menus.
pub(crate) const QAT_SWITCH_ID: &str = "QAT_SWITCH_WINDOWS";
pub(crate) const QAT_UI_ID: &str = "QAT_USER_INTERFACE";
/// The customize button and its menu, and More Commands.
pub(crate) const QAT_CUSTOMIZE_ID: &str = "QAT_CUSTOMIZE";
pub(crate) const QAT_PICKER_ID: &str = "QAT_MORE_COMMANDS";
/// More Commands' search field.
pub const QAT_PICKER_INPUT: &str = "qat_more_commands_input";
/// A bar with a ▾ under it.
const CUSTOMIZE_ICON: &str = "M6 7h12 M8.5 11.5L12 15l3.5-3.5";
/// Starts the dropdown id of a split item's list.
const SPLIT_PREFIX: &str = "qat:";
const TIP_DELAY: Duration = Duration::from_millis(400);
/// Two overlapping windows.
const SWITCH_WINDOWS_ICON: &str = "M3 5h12v10H3z M9 9h12v10H9z";
/// A window with a bar along its top and a side bar.
const USER_INTERFACE_ICON: &str = "M3 4h18v16H3z M3 9h18 M8 9v11";

/// A ribbon list the toolbar can hold.
struct QatList {
    control: QatBuiltin,
    /// Its id on the toolbar.
    id: &'static str,
    /// The ribbon list whose dropdown it opens.
    source: &'static str,
    /// The system variable it sets, which More Commands finds it by.
    variable: &'static str,
    /// Its name, as its tooltip and More Commands show.
    name: fn() -> String,
}

/// The ribbon lists the toolbar can hold.
static LISTS: [QatList; 11] = {
    const fn list(
        control: QatBuiltin,
        id: &'static str,
        source: &'static str,
        variable: &'static str,
        name: fn() -> String,
    ) -> QatList {
        QatList {
            control,
            id,
            source,
            variable,
            name,
        }
    }
    [
        list(
            QatBuiltin::LayerCombo,
            QAT_LAYER_COMBO_ID,
            LAYER_COMBO_ID,
            "CLAYER",
            || crate::t!("Layer").into_owned(),
        ),
        list(
            QatBuiltin::Color,
            "QAT_COLOR",
            PROP_COLOR_ID,
            "CECOLOR",
            || crate::t!("Color").into_owned(),
        ),
        list(
            QatBuiltin::Linetype,
            "QAT_LINETYPE",
            PROP_LINETYPE_ID,
            "CELTYPE",
            || crate::t!("Linetype").into_owned(),
        ),
        list(
            QatBuiltin::Lineweight,
            "QAT_LINEWEIGHT",
            PROP_LW_ID,
            "CELWEIGHT",
            || crate::t!("Lineweight").into_owned(),
        ),
        list(
            QatBuiltin::TextStyle,
            "QAT_TEXT_STYLE",
            "TEXT_STYLE_COMBO",
            "TEXTSTYLE",
            || crate::t!("Text Style").into_owned(),
        ),
        list(
            QatBuiltin::DimStyle,
            "QAT_DIM_STYLE",
            "DIM_STYLE_COMBO",
            "DIMSTYLE",
            || crate::t!("Dimension Style").into_owned(),
        ),
        list(
            QatBuiltin::MleaderStyle,
            "QAT_MLEADER_STYLE",
            "MLEADER_STYLE_COMBO",
            "CMLEADERSTYLE",
            || crate::t!("Multileader Style").into_owned(),
        ),
        list(
            QatBuiltin::TableStyle,
            "QAT_TABLE_STYLE",
            "TABLE_STYLE_COMBO",
            "CTABLESTYLE",
            || crate::t!("Table Style").into_owned(),
        ),
        list(
            QatBuiltin::DimLayer,
            "QAT_DIM_LAYER",
            DIM_LAYER_COMBO,
            "DIMLAYER",
            || crate::t!("Dimension Layer").into_owned(),
        ),
        list(
            QatBuiltin::MleaderLayer,
            "QAT_MLEADER_LAYER",
            MLEADER_LAYER_COMBO,
            "MLEADERLAYER",
            || crate::t!("Multileader Layer").into_owned(),
        ),
        list(
            QatBuiltin::CenterLayer,
            "QAT_CENTER_LAYER",
            CENTER_LAYER_COMBO,
            "CENTERLAYER",
            || crate::tr!("ribbon-menu", "center-layer"),
        ),
    ]
};

fn list_of(control: QatBuiltin) -> Option<&'static QatList> {
    LISTS.iter().find(|list| list.control == control)
}

/// The toolbar id and the ribbon list of `control`, if it is a list.
pub(super) fn list_ids(control: QatBuiltin) -> Option<(&'static str, &'static str)> {
    list_of(control).map(|list| (list.id, list.source))
}

/// The ribbon list the toolbar list `id` opens, if it is one.
pub(crate) fn list_source(id: &str) -> Option<&'static str> {
    LISTS
        .iter()
        .find(|list| list.id == id)
        .map(|list| list.source)
}

/// The toolbar control copying the ribbon list `ribbon_id`, if the toolbar
/// can hold it.
pub(super) fn list_control(ribbon_id: &str) -> Option<QatBuiltin> {
    LISTS
        .iter()
        .find(|list| list.source == ribbon_id)
        .map(|list| list.control)
}

/// Every list the toolbar can hold, with the system variable it sets.
pub(super) fn list_controls() -> impl Iterator<Item = (QatBuiltin, &'static str)> {
    LISTS.iter().map(|list| (list.control, list.variable))
}

/// The name of the list `control` (empty if it is not one).
pub(super) fn list_name(control: QatBuiltin) -> String {
    list_of(control)
        .map(|list| (list.name)())
        .unwrap_or_default()
}

/// A ribbon list (`ribbon_id`) whose right-click offers to put it on the
/// toolbar or take it off.
pub(super) fn list_target<'a>(
    ribbon_id: &str,
    child: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    super::ribbon_menu::tool_target(list_control(ribbon_id).map(QatItem::Builtin), child)
}

/// What a command item shows.
#[derive(Clone, Copy)]
pub(super) enum FaceIcon {
    /// One of the toolbar's own chrome icons, tinted for the tab row.
    Chrome(&'static [u8]),
    /// A ribbon or menu icon, in its own colours.
    Tool(IconKind),
    /// No icon anywhere: the button shows the command name.
    Text,
}

/// A ribbon dropdown's item: command, label, icon.
pub(super) type DropdownItem = (&'static str, &'static str, IconKind);

/// What a command item of the toolbar is.
#[derive(Clone)]
pub(super) enum QatLook {
    /// A ribbon dropdown: its face runs the variant last chosen.
    Split {
        id: &'static str,
        label: &'static str,
        items: Rc<[DropdownItem]>,
        default: &'static str,
    },
    /// A plain button; greyed out and inert unless OCS has its command
    /// (`runs`).
    Plain { face: Face, runs: bool },
}

/// The command items' looks, for the language they were worked out in.
#[derive(Default)]
pub(super) struct QatCache {
    generation: Option<u64>,
    looks: HashMap<String, QatLook>,
}

/// The cache, as the ribbon holds it.
pub(super) type QatLooks = RefCell<QatCache>;

/// The icon and label of a command item.
#[derive(Clone)]
pub(super) struct Face {
    /// The label, localized, on one line.
    pub label: String,
    pub icon: FaceIcon,
}

/// Batch Plot's icon: the Layout tools' plot icon, as no ribbon button runs it.
static PLOT_ICON: &[u8] = include_bytes!("../../../assets/icons/plot.svg");

/// The toolbar's own file buttons keep the icons and labels they always had.
static FILE_FACES: [(&str, &[u8], &str); 6] = [
    ("QNEW", icons::DOC_NEW, "New"),
    ("OPEN", icons::FOLDER_OPEN, "Open"),
    ("SAVE", icons::SAVE, "Save"),
    ("QSAVE", icons::SAVE, "Save"),
    ("SAVEAS", icons::FILE_EXPORT, "Save As"),
    ("PRINT", icons::PRINT, "Print"),
];

impl Ribbon {
    /// The icon and label of the command item `cmd`: the toolbar's own file
    /// buttons first, then the ribbon button that runs it, then its menu-bar
    /// entry; with none of them its name.
    pub(super) fn command_face(&self, cmd: &str) -> Face {
        let cmd = cmd.trim();
        if let Some(&(_, icon, label)) = FILE_FACES
            .iter()
            .find(|(command, _, _)| command.eq_ignore_ascii_case(cmd))
        {
            return Face {
                label: crate::t!(label).into_owned(),
                icon: FaceIcon::Chrome(icon),
            };
        }
        // Commands with no ribbon button take a fitting icon of their own.
        if cmd.eq_ignore_ascii_case("PUBLISH") {
            return Face {
                label: crate::tr!("ribbon-menu", "qat-batch-plot"),
                icon: FaceIcon::Tool(IconKind::Svg(PLOT_ICON)),
            };
        }
        if cmd.eq_ignore_ascii_case("PREVIEW") {
            return Face {
                label: crate::tr!("ribbon-menu", "qat-plot-preview"),
                icon: FaceIcon::Chrome(icons::PREVIEW),
            };
        }
        if let Some((label, icon)) = self.ribbon_face(cmd) {
            return Face {
                label: crate::t!(label).replace('\n', " "),
                icon: FaceIcon::Tool(icon),
            };
        }
        if let Some((label, icon)) = crate::ui::menu_bar::entry_for_line(cmd) {
            return Face {
                label: label.trim_end_matches('.').to_string(),
                icon: icon.map_or(FaceIcon::Text, |icon| FaceIcon::Tool(IconKind::Svg(icon))),
            };
        }
        if let Some(icon) = crate::modules::registry::command_icon(cmd) {
            return Face {
                label: cmd.to_string(),
                icon: FaceIcon::Tool(IconKind::Svg(icon)),
            };
        }
        Face {
            label: cmd.to_string(),
            icon: FaceIcon::Text,
        }
    }

    /// What the command item `cmd` is: a split item for a ribbon dropdown,
    /// else a button with its face. Worked out once per command and kept
    /// until the language or the tabs change, as it searches every tab and
    /// the menu bar.
    pub(super) fn qat_look(&self, cmd: &str) -> QatLook {
        let generation = crate::i18n::language_generation();
        let key = cmd.trim().to_ascii_uppercase();
        {
            let mut cache = self.qat_cache.borrow_mut();
            if cache.generation != Some(generation) {
                cache.looks.clear();
                cache.generation = Some(generation);
            }
            if let Some(look) = cache.looks.get(&key) {
                return look.clone();
            }
        }
        let look = match self.ribbon_dropdown(cmd) {
            Some((id, label, items, default)) => QatLook::Split {
                id,
                label,
                items: items.into(),
                default,
            },
            None => QatLook::Plain {
                face: self.command_face(cmd),
                // A ribbon button's command works even when it is not a
                // registered command (`_SSMTOGGLE`, `DIMEDIT;O`, plugins').
                runs: crate::ui::menu_bar::implemented(cmd) || self.ribbon_face(cmd).is_some(),
            },
        };
        self.qat_cache.borrow_mut().looks.insert(key, look.clone());
        look
    }

    /// Whether OCS runs what the Quick Access `item` stands for: a built-in
    /// control or a split item (a ribbon dropdown) always, a command when
    /// OCS has it.
    pub(super) fn item_runs(&self, item: &QatItem) -> bool {
        match item {
            QatItem::Command(cmd) => match self.qat_look(cmd) {
                QatLook::Split { .. } => true,
                QatLook::Plain { runs, .. } => runs,
            },
            QatItem::Builtin(_) | QatItem::Separator => true,
        }
    }

    /// Whether the customize menu lets `item` be ticked or unticked: a
    /// command OCS does not have yet is greyed out, as the menu bar has it,
    /// and cannot be added, but one already on the toolbar can come off.
    pub(super) fn can_toggle(&self, item: &QatItem) -> bool {
        self.layout.qat_has(item) || self.item_runs(item)
    }

    /// Forget the command items' looks: the tabs changed.
    pub(super) fn forget_qat_looks(&mut self) {
        self.qat_cache.get_mut().looks.clear();
    }

    /// The toolbar's items, left to right.
    pub(super) fn qat_items(
        &self,
        undo_count: usize,
        redo_count: usize,
    ) -> Vec<Element<'_, Message>> {
        let items = self.layout.qat.iter().map(|item| match item {
            QatItem::Command(cmd) => match self.qat_look(cmd) {
                QatLook::Split {
                    id,
                    label,
                    items,
                    default,
                } => self.qat_split(id, label, &items, default),
                QatLook::Plain { face, runs } => quick_access_btn(face, cmd.clone(), runs),
            },
            QatItem::Builtin(QatBuiltin::Undo) => {
                render_history_control("Undo", UNDO_HISTORY_ID, undo_count, &self.open_dropdown)
            }
            QatItem::Builtin(QatBuiltin::Redo) => {
                render_history_control("Redo", REDO_HISTORY_ID, redo_count, &self.open_dropdown)
            }
            QatItem::Builtin(QatBuiltin::LayerCombo) => super::keytips::anchor_with(
                || super::keytips::quick_access_anchor(QAT_LAYER_COMBO_ID),
                container(layer_combo_face(
                    QAT_LAYER_COMBO_ID,
                    &self.layer_infos,
                    &self.active_layer,
                    self.open_dropdown.as_deref() == Some(QAT_LAYER_COMBO_ID),
                    metrics::QAT_COMBO_W,
                    false,
                ))
                .padding([0.0, metrics::QAT_COMBO_MARGIN]),
            ),
            QatItem::Builtin(QatBuiltin::SwitchWindows) => self.qat_menu_button(
                QAT_SWITCH_ID,
                SWITCH_WINDOWS_ICON,
                crate::tr!("ribbon-menu", "switch-windows"),
            ),
            QatItem::Builtin(QatBuiltin::UserInterface) => self.qat_menu_button(
                QAT_UI_ID,
                USER_INTERFACE_ICON,
                crate::tr!("ribbon-menu", "user-interface"),
            ),
            QatItem::Builtin(control) => self.qat_list(*control),
            QatItem::Separator => separator(),
        });
        // Right-clicked, an item offers to leave the toolbar.
        items
            .enumerate()
            .map(|(index, item)| RightPress::for_target(MenuTarget::Qat(index), item).into())
            .chain(std::iter::once(self.customize_button()))
            .collect()
    }

    /// The button at the end of the toolbar that opens its customize menu.
    fn customize_button(&self) -> Element<'_, Message> {
        let open = matches!(
            self.open_dropdown.as_deref(),
            Some(QAT_CUSTOMIZE_ID | QAT_PICKER_ID)
        );
        let btn = button(
            container(icons::line_icon(CUSTOMIZE_ICON, metrics::STRIP_ICON))
                .center_x(Fill)
                .center_y(Fill),
        )
        .on_press(Message::ToggleRibbonDropdown(QAT_CUSTOMIZE_ID.to_string()))
        .style(move |theme: &Theme, status| top_hist_btn_style(theme, true, open, status))
        .width(Length::Fixed(metrics::QAT_CUSTOMIZE_W))
        .height(metrics::QAT_BUTTON)
        .padding(0);
        let btn = tipped(btn, crate::tr!("ribbon-menu", "customize-qat"), open);
        PosReport::new(QAT_CUSTOMIZE_ID, btn).into()
    }

    /// The customize menu or More Commands, while open: what the toolbar
    /// holds, the menu bar and where the toolbar shows.
    pub fn qat_customize_overlay(
        &self,
        win: (f32, f32),
        show_menu_bar: bool,
        ribbon_closed: bool,
    ) -> Option<Element<'_, Message>> {
        match self.open_dropdown.as_deref()? {
            QAT_CUSTOMIZE_ID => Some(self.customize_menu(win, show_menu_bar, ribbon_closed)),
            QAT_PICKER_ID => Some(self.more_commands(win)),
            _ => None,
        }
    }

    fn customize_menu(
        &self,
        win: (f32, f32),
        show_menu_bar: bool,
        ribbon_closed: bool,
    ) -> Element<'_, Message> {
        let title = crate::tr!("ribbon-menu", "customize-qat");
        let more = crate::tr!("ribbon-menu", "more-commands");
        let menu_bar = if show_menu_bar {
            crate::tr!("ribbon-menu", "hide-menu-bar")
        } else {
            crate::tr!("ribbon-menu", "show-menu-bar")
        };
        let place = if self.layout.qat_below {
            crate::tr!("ribbon-menu", "show-above")
        } else {
            crate::tr!("ribbon-menu", "show-below")
        };
        let standard = standard_items();
        let width = fit_width(standard.iter().map(|(label, _)| label.as_str()).chain([
            title.as_str(),
            more.as_str(),
            menu_bar.as_str(),
            place.as_str(),
        ]));
        use crate::ui::dropdown_list::{DropdownList, DropdownRow};
        let mut items: Vec<DropdownRow<'_, Message>> = vec![DropdownRow::header(menu_header(title))];
        items.extend(standard.into_iter().map(|(label, item)| {
            let checked = self.layout.qat_has(&item);
            let on_press = self
                .can_toggle(&item)
                .then(|| Message::RibbonQat(QatMsg::Toggle(item)));
            let el: Element<'_, Message> = check_row(label.clone(), checked, on_press.clone()).into();
            if let Some(msg) = on_press {
                DropdownRow::item(el, label, msg)
            } else {
                DropdownRow::header(el)
            }
        }));
        items.push(DropdownRow::header(menu_separator()));
        let more_msg = Message::RibbonQat(QatMsg::OpenPicker);
        items.push(DropdownRow::item(
            check_row(more.clone(), false, Some(more_msg.clone())),
            more,
            more_msg,
        ));
        let mb_msg = Message::RibbonMenu(super::RibbonMenuMsg::ShowMenuBar(!show_menu_bar));
        items.push(DropdownRow::item(
            check_row(menu_bar.clone(), false, Some(mb_msg.clone())),
            menu_bar,
            mb_msg,
        ));
        let place_el = place_row(place.clone(), self.layout.qat_below, ribbon_closed);
        if ribbon_closed {
            items.push(DropdownRow::header(place_el));
        } else {
            items.push(DropdownRow::item(
                place_el,
                place,
                Message::RibbonQat(QatMsg::SetBelow(!self.layout.qat_below)),
            ));
        }
        let list = DropdownList::new(items).on_escape(Message::CloseRibbonDropdown);
        let panel = container(list)
            .style(super::widgets::popup_panel_style)
            .padding(icons::hairline())
            .width(Length::Fixed(width));
        self.place_dropdown(QAT_CUSTOMIZE_ID, panel.into(), width, win.0)
    }

    /// More Commands: the command search's box and results; a result is
    /// added to the toolbar instead of run.
    fn more_commands(&self, win: (f32, f32)) -> Element<'_, Message> {
        use crate::ui::dropdown_list::{DropdownList, DropdownRow};
        let input = iced::widget::text_input(
            &crate::tr!("ribbon-menu", "find-command-to-add"),
            &self.qat_query,
        )
        .id(iced::widget::Id::new(QAT_PICKER_INPUT))
        .on_input(|query| Message::RibbonQat(QatMsg::PickerInput(query)))
        .size(metrics::QAT_PICKER_INPUT_SIZE)
        .padding(metrics::QAT_PICKER_INPUT_PAD);
        // Worked out when the query changes, not on every frame.
        let candidates = &self.qat_found;
        let list_h = (candidates.len().max(1) as f32 * metrics::QAT_PICKER_ROW_H)
            .min(metrics::QAT_PICKER_LIST_H);
        let rows: Vec<DropdownRow<'_, Message>> =
            if candidates.is_empty() && !self.qat_query.trim().is_empty() {
                vec![DropdownRow::header(
                    container(
                        text(crate::tr!("ribbon-menu", "no-matches"))
                            .size(metrics::QAT_TEXT_SIZE)
                            .style(|theme: &Theme| text::Style {
                                color: Some(chrome::muted(theme)),
                            }),
                    )
                    .padding(metrics::QAT_PICKER_EMPTY_PAD),
                )]
            } else {
                candidates
                    .iter()
                    .map(|candidate| {
                        let held = self.layout.qat_has(&candidate.item);
                        let icon: Element<'_, Message> = match candidate.icon {
                            Some(icon) => super::widgets::make_icon(icon, SMALL_ICON),
                            None => Space::new().width(SMALL_ICON).height(SMALL_ICON).into(),
                        };
                        let label = iced::widget::column![
                            text(candidate.label.clone()).size(metrics::QAT_PICKER_LABEL_SIZE),
                            text(candidate.path.clone())
                                .size(metrics::QAT_PICKER_PATH_SIZE)
                                .style(|theme: &Theme| text::Style {
                                    color: Some(chrome::muted(theme)),
                                }),
                        ];
                        let mut content = row![icon, label]
                            .spacing(metrics::QAT_PICKER_ICON_GAP)
                            .align_y(iced::Center);
                        if held {
                            content = content.push(Space::new().width(Fill)).push(
                                icons::themed_disabled(icons::CHECK, metrics::QAT_MENU_CHECK),
                            );
                        }
                        let msg = (!held)
                            .then(|| Message::RibbonQat(QatMsg::Add(candidate.item.clone())));
                        let btn = button(content)
                            .on_press_maybe(msg.clone())
                            .style(super::widgets::popup_row_style)
                            .width(Fill)
                            .padding(metrics::QAT_PICKER_ROW_PAD);
                        if let Some(msg) = msg {
                            DropdownRow::item(btn, candidate.label.clone(), msg)
                        } else {
                            DropdownRow::header(btn)
                        }
                    })
                    .collect()
            };
        let list = DropdownList::with_options(
            rows,
            Length::Fixed(list_h),
            Length::Fill,
            0.0,
            0.0,
        )
        .query(self.qat_query.clone())
        .on_escape(Message::CloseRibbonDropdown);
        let panel = container(
            iced::widget::column![
                menu_header(crate::tr!("ribbon-menu", "more-commands-title")),
                container(input).padding(metrics::QAT_PICKER_INPUT_PAD),
                list
            ]
            .spacing(metrics::GAP),
        )
        .style(super::widgets::popup_panel_style)
        .width(Length::Fixed(metrics::QAT_PICKER_W));
        self.place_dropdown(QAT_CUSTOMIZE_ID, panel.into(), metrics::QAT_PICKER_W, win.0)
    }

    /// The toolbar as one flow that wraps.
    pub(super) fn qat_row(
        &self,
        is_start: bool,
        undo_count: usize,
        redo_count: usize,
    ) -> Element<'_, Message> {
        let row: Element<'_, Message> =
            iced::widget::Row::with_children(self.qat_items(undo_count, redo_count))
                .spacing(TOP_HIST_GAP)
                .align_y(iced::Center)
                .wrap()
                .vertical_spacing(0.0)
                .into();
        // The Start tab has no drawing: the whole toolbar fades, as the
        // tabs do, and takes no clicks.
        if is_start {
            iced::widget::stack![row, super::start_shield(chrome::bar)].into()
        } else {
            row
        }
    }

    /// The toolbar in its own row below the ribbon: `bar` coloured, its
    /// items centred in it, with a line of `page` on top when `line`.
    /// Right-clicked on its empty space it offers to move back up.
    pub(super) fn qat_below_row(
        &self,
        is_start: bool,
        undo_count: usize,
        redo_count: usize,
        line: bool,
    ) -> Element<'_, Message> {
        let bar = container(self.qat_row(is_start, undo_count, redo_count))
            .center_y(metrics::QAT_ROW_H)
            .padding([0.0, metrics::QAT_ROW_PAD])
            .width(Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(Background::Color(chrome::bar(theme))),
                ..Default::default()
            });
        let bar: Element<'_, Message> = if line {
            iced::widget::column![
                container(Space::new().width(Fill).height(icons::hairline())).style(
                    |theme: &Theme| container::Style {
                        background: Some(Background::Color(chrome::page(theme))),
                        ..Default::default()
                    }
                ),
                bar
            ]
            .into()
        } else {
            bar.into()
        };
        RightPress::for_target(MenuTarget::TabRow, bar).into()
    }

    /// The command the face of the split item `id` runs: the one last
    /// chosen from its list, else the dropdown's default.
    pub(super) fn qat_face_command(
        &self,
        id: &str,
        items: &[(&'static str, &'static str, IconKind)],
        default: &'static str,
    ) -> &'static str {
        self.layout
            .qat_faces
            .get(id)
            .and_then(|chosen| items.iter().find(|(cmd, _, _)| cmd == chosen))
            .map_or(default, |(cmd, _, _)| *cmd)
    }

    /// A split item: its face runs the variant last chosen, its arrow opens
    /// the list the ribbon dropdown `id` shows.
    fn qat_split(
        &self,
        id: &'static str,
        label: &'static str,
        items: &[(&'static str, &'static str, IconKind)],
        default: &'static str,
    ) -> Element<'_, Message> {
        let face_cmd = self.qat_face_command(id, items, default);
        let (face_label, face_icon_kind) = items
            .iter()
            .find(|(cmd, _, _)| *cmd == face_cmd)
            .map_or((label, None), |(_, label, icon)| (*label, Some(*icon)));
        let open_id = split_dropdown_id(id);
        let open = self.open_dropdown.as_deref() == Some(open_id.as_str());
        let face = Face {
            label: crate::t!(face_label).into_owned(),
            icon: face_icon_kind.map_or(FaceIcon::Text, FaceIcon::Tool),
        };
        let face_btn = button(
            container(face_icon(&face, SMALL_ICON, true))
                .center_x(Fill)
                .center_y(Fill),
        )
        .on_press(Message::MenuCommand(face_cmd.to_string()))
        .style(move |theme: &Theme, status| qat_split_part_style(theme, true, open, status))
        .width(Length::Fixed(metrics::QAT_BUTTON))
        .height(metrics::QAT_BUTTON)
        .padding(0);
        let face_btn = tooltip(
            face_btn,
            make_tip(command_tip(&face.label, face_cmd)),
            TipPos::Bottom,
        )
        .gap(6.0)
        .delay(TIP_DELAY)
        .style(tip_style);
        let arrow = button(
            container(icons::dropdown_arrow())
                .center_x(Fill)
                .center_y(Fill),
        )
        .on_press(Message::ToggleRibbonDropdown(open_id.clone()))
        .style(move |theme: &Theme, status| qat_split_part_style(theme, true, open, status))
        .width(Length::Fixed(metrics::QAT_SPLIT_ARROW_W))
        .height(metrics::QAT_BUTTON)
        .padding(0);
        let arrow = tooltip(
            arrow,
            make_tip(crate::t!(label).replace('\n', " ")),
            TipPos::Bottom,
        )
        .gap(6.0)
        .delay(TIP_DELAY)
        .style(tip_style);
        let parts: Element<'_, Message> = row![face_btn, arrow].into();
        let parts = split_outline(parts);
        let anchor = super::keytips::quick_access_anchor(id);
        super::keytips::anchor_with(move || anchor, PosReport::owned(open_id, parts))
    }

    /// A ribbon list on the toolbar (`control`): its current value, with
    /// the colour of a colour or layer, opening the ribbon list's dropdown.
    fn qat_list(&self, control: QatBuiltin) -> Element<'_, Message> {
        let Some((id, _)) = list_ids(control) else {
            return Space::new().into();
        };
        let layer = |name: &str| {
            let label = if name.is_empty() {
                crate::t!("Use Current").into_owned()
            } else {
                name.to_string()
            };
            (label, Some(self.layer_color(name)))
        };
        let (label, swatch) = match control {
            QatBuiltin::Color => {
                // ByLayer shows the current layer's colour, as on the ribbon.
                let swatch = super::layer_color(&self.layer_infos, &self.active_layer)
                    .filter(|_| self.active_color == codec::types::Color::ByLayer)
                    .unwrap_or_else(|| {
                        crate::ui::properties::acad_color_display(self.active_color).0
                    });
                (
                    crate::ui::color_select::color_display_name(self.active_color),
                    Some(Some(swatch)),
                )
            }
            QatBuiltin::Linetype => (
                crate::ui::properties::linetype_display_name(&self.active_linetype),
                None,
            ),
            QatBuiltin::Lineweight => (
                crate::ui::properties::LwItem(self.active_lineweight).to_string(),
                None,
            ),
            QatBuiltin::TextStyle => (self.active_text_style.clone(), None),
            QatBuiltin::DimStyle => (self.active_dim_style.clone(), None),
            QatBuiltin::MleaderStyle => (self.active_mleader_style.clone(), None),
            QatBuiltin::TableStyle => (self.active_table_style.clone(), None),
            QatBuiltin::DimLayer => layer(&self.dim_layer),
            QatBuiltin::MleaderLayer => layer(&self.mleader_layer),
            QatBuiltin::CenterLayer => layer(&self.center_layer),
            // Drawn by `qat_items` with their own faces.
            QatBuiltin::Undo
            | QatBuiltin::Redo
            | QatBuiltin::LayerCombo
            | QatBuiltin::SwitchWindows
            | QatBuiltin::UserInterface => return Space::new().into(),
        };
        let width = match control {
            QatBuiltin::Color | QatBuiltin::Linetype | QatBuiltin::Lineweight => {
                metrics::QAT_PROP_COMBO_W
            }
            _ => metrics::QAT_STYLE_COMBO_W,
        };
        let open = self.open_dropdown.as_deref() == Some(id);
        // A layer list shows its layer's colour, or a blank for Use Current.
        let swatch: Element<'_, Message> = match swatch {
            Some(Some(color)) => container(Space::new())
                .width(metrics::QAT_COMBO_SWATCH)
                .height(metrics::QAT_COMBO_SWATCH)
                .style(move |theme: &Theme| container::Style {
                    background: Some(Background::Color(color)),
                    border: iced::Border {
                        color: chrome::border(theme),
                        width: icons::hairline(),
                        radius: 1.0.into(),
                    },
                    ..Default::default()
                })
                .into(),
            Some(None) => Space::new()
                .width(metrics::QAT_COMBO_SWATCH)
                .height(metrics::QAT_COMBO_SWATCH)
                .into(),
            None => Space::new().width(0).into(),
        };
        let btn = button(
            row![
                swatch,
                container(
                    text(label)
                        .size(11)
                        .wrapping(iced::advanced::text::Wrapping::None)
                )
                .width(Fill)
                .clip(true),
                icons::dropdown_arrow(),
            ]
            .spacing(4)
            .align_y(iced::Center),
        )
        .on_press(Message::ToggleRibbonDropdown(id.to_string()))
        .style(move |theme: &Theme, status| combo_btn_style(theme, open, status, 3.0))
        .padding([0, 8])
        .height(metrics::COMBO_H)
        .width(Length::Fixed(width));
        let btn = tipped(btn, list_name(control), open);
        let anchor = super::keytips::quick_access_anchor(id);
        super::keytips::anchor_with(
            move || anchor,
            container(PosReport::new(id, btn)).padding([0.0, metrics::QAT_COMBO_MARGIN]),
        )
    }

    /// An icon with a ▾ that opens the menu `id`.
    fn qat_menu_button(
        &self,
        id: &'static str,
        icon: &'static str,
        tip: String,
    ) -> Element<'_, Message> {
        let open = self.open_dropdown.as_deref() == Some(id);
        let btn = button(
            row![icons::line_icon(icon, SMALL_ICON), icons::dropdown_arrow()]
                .spacing(metrics::QAT_ARROW_GAP)
                .align_y(iced::Center),
        )
        .on_press(Message::ToggleRibbonDropdown(id.to_string()))
        .style(move |theme: &Theme, status| top_hist_btn_style(theme, true, open, status))
        .width(Length::Fixed(metrics::QAT_MENU_BUTTON_W))
        .height(metrics::QAT_BUTTON)
        .padding([0.0, metrics::SMALL_PAD]);
        let btn = tipped(btn, tip, open);
        let anchor = super::keytips::quick_access_anchor(id);
        super::keytips::anchor_with(move || anchor, PosReport::new(id, btn))
    }

    /// The open Quick Access list or menu `open_id`, if it is one: a split
    /// item's variants, the layer list, the open drawings or the user
    /// interface switches. None of them opens on the Start tab.
    pub(super) fn qat_overlay(
        &self,
        open_id: &str,
        win: (f32, f32),
        is_start: bool,
        drawings: &[(usize, String, bool)],
    ) -> Option<Element<'_, Message>> {
        // The Start tab has no drawing: none of them opens there.
        if is_start {
            return None;
        }
        if let Some(id) = open_id.strip_prefix(SPLIT_PREFIX) {
            let (id, _, items, default) = self.ribbon_dropdown(id)?;
            let current = self.qat_face_command(id, items, default);
            let (panel, width) = super::dropdown_list(id, items, current, None, |cmd| {
                Message::RibbonQat(QatMsg::Pick {
                    item: id.to_string(),
                    cmd,
                })
            });
            return Some(self.place_dropdown(open_id, panel, width, win.0));
        }
        match open_id {
            QAT_SWITCH_ID => {
                let rows: Vec<(String, bool, Option<Message>)> = drawings
                    .iter()
                    .map(|(index, name, current)| {
                        (name.clone(), *current, Some(Message::TabSwitch(*index)))
                    })
                    .collect();
                let rows = if rows.is_empty() {
                    vec![(crate::tr!("ribbon-menu", "no-open-drawings"), false, None)]
                } else {
                    rows
                };
                Some(self.qat_menu(QAT_SWITCH_ID, rows, win))
            }
            QAT_UI_ID => {
                let rows = [
                    (
                        crate::tr!("ribbon-menu", "viewcube"),
                        "NAVVCUBE",
                        self.show_viewcube,
                    ),
                    (
                        crate::tr!("ribbon-menu", "ucs-icon"),
                        "UCSICON",
                        self.show_ucs_icon,
                    ),
                    (
                        crate::tr!("ribbon-menu", "file-tabs"),
                        "FILETAB",
                        self.show_file_tabs,
                    ),
                    (
                        crate::tr!("ribbon-menu", "layout-tabs"),
                        "LAYOUTTAB",
                        self.show_layout_tabs,
                    ),
                    // OCS has no switch for these yet: greyed out.
                    (
                        crate::tr!("ribbon-menu", "command-line"),
                        "COMMANDLINE",
                        false,
                    ),
                    (crate::tr!("ribbon-menu", "status-bar"), "STATUSBAR", false),
                ]
                .into_iter()
                .map(|(label, cmd, on)| {
                    let on_press = crate::ui::menu_bar::implemented(cmd)
                        .then(|| Message::MenuCommand(cmd.to_string()));
                    (label, on, on_press)
                })
                .collect();
                Some(self.qat_menu(QAT_UI_ID, rows, win))
            }
            _ => None,
        }
    }

    /// A Quick Access menu of checkable `rows` under the button `id`.
    fn qat_menu(
        &self,
        id: &str,
        rows: Vec<(String, bool, Option<Message>)>,
        win: (f32, f32),
    ) -> Element<'_, Message> {
        use crate::ui::dropdown_list::{DropdownList, DropdownRow};
        let width = fit_width(rows.iter().map(|(label, _, _)| label.as_str()));
        let mut initial_selected: Option<usize> = None;
        let items: Vec<DropdownRow<'_, Message>> = rows
            .into_iter()
            .enumerate()
            .map(|(idx, (label, checked, on_press))| {
                let el: Element<'_, Message> =
                    check_row(label.clone(), checked, on_press.clone()).into();
                if let Some(msg) = on_press {
                    if checked && initial_selected.is_none() {
                        initial_selected = Some(idx);
                    }
                    DropdownRow::item(el, label, msg)
                } else {
                    DropdownRow::header(el)
                }
            })
            .collect();
        let list = DropdownList::new(items)
            .selected(initial_selected)
            .on_escape(Message::CloseRibbonDropdown);
        let panel = container(list)
            .style(super::widgets::popup_panel_style)
            .padding(icons::hairline())
            .width(Length::Fixed(width));
        self.place_dropdown(id, panel.into(), width, win.0)
    }

    /// Apply a Quick Access message. Returns whether the toolbar changed and
    /// should be saved, and what to run.
    pub fn on_qat(&mut self, msg: QatMsg) -> QatOutcome {
        match msg {
            QatMsg::Pick { item, cmd } => {
                self.close_dropdown();
                self.layout.qat_faces.insert(item, cmd.to_string());
                QatOutcome {
                    save: true,
                    run: Some(Message::MenuCommand(cmd.to_string())),
                    ..QatOutcome::default()
                }
            }
            QatMsg::Toggle(item) => {
                self.layout.qat_toggle(item);
                QatOutcome {
                    save: true,
                    ..QatOutcome::default()
                }
            }
            QatMsg::SetBelow(below) => {
                self.close_dropdown();
                self.layout.qat_below = below;
                QatOutcome {
                    save: true,
                    ..QatOutcome::default()
                }
            }
            QatMsg::OpenPicker => {
                self.qat_query.clear();
                self.qat_found.clear();
                self.open_dropdown = Some(QAT_PICKER_ID.to_string());
                QatOutcome {
                    focus: true,
                    ..QatOutcome::default()
                }
            }
            QatMsg::PickerInput(query) => {
                self.qat_found = self.qat_candidates(&query);
                self.qat_query = query;
                QatOutcome::default()
            }
            QatMsg::PickerSubmit => {
                let first = self
                    .qat_found
                    .iter()
                    .find(|candidate| !self.layout.qat_has(&candidate.item))
                    .map(|candidate| candidate.item.clone());
                match first {
                    Some(item) => self.on_qat(QatMsg::Add(item)),
                    None => QatOutcome::default(),
                }
            }
            QatMsg::Add(item) => {
                let save = self.layout.qat_add(item);
                self.close_dropdown();
                self.qat_query.clear();
                self.qat_found.clear();
                QatOutcome {
                    save,
                    ..QatOutcome::default()
                }
            }
        }
    }
}

/// A Quick Access message.
#[derive(Clone, Debug)]
pub enum QatMsg {
    /// From the list of the split item `item`: run `cmd`, which its face
    /// shows from now on.
    Pick { item: String, cmd: &'static str },
    /// Add the item to the end of the toolbar, or take it off.
    Toggle(QatItem),
    /// Show the toolbar below the ribbon (`true`) or in the tab row.
    SetBelow(bool),
    /// Open More Commands.
    OpenPicker,
    /// More Commands' query.
    PickerInput(String),
    /// Enter in More Commands: add the first result not on the toolbar.
    PickerSubmit,
    /// Add the item at the end of the toolbar, unless it is there already.
    Add(QatItem),
}

/// What the app does after a Quick Access message.
#[derive(Default)]
pub struct QatOutcome {
    /// The toolbar changed and should be saved.
    pub save: bool,
    /// A message to run.
    pub run: Option<Message>,
    /// Focus More Commands' search field.
    pub focus: bool,
}

/// The commands the customize menu offers, with their labels.
fn standard_items() -> Vec<(String, QatItem)> {
    let command = |cmd: &str| QatItem::Command(cmd.to_string());
    vec![
        (crate::tr!("ribbon-menu", "qat-new"), command("QNEW")),
        (crate::tr!("ribbon-menu", "qat-open"), command("OPEN")),
        (crate::tr!("ribbon-menu", "qat-save"), command("SAVE")),
        (crate::tr!("ribbon-menu", "qat-save-as"), command("SAVEAS")),
        // The toolbar's Print button is what plots.
        (crate::tr!("ribbon-menu", "qat-plot"), command("PRINT")),
        (
            crate::tr!("ribbon-menu", "qat-undo"),
            QatItem::Builtin(QatBuiltin::Undo),
        ),
        (
            crate::tr!("ribbon-menu", "qat-redo"),
            QatItem::Builtin(QatBuiltin::Redo),
        ),
        (
            crate::tr!("ribbon-menu", "qat-measure"),
            command("MEASURE_MENU"),
        ),
        (
            crate::tr!("ribbon-menu", "switch-windows"),
            QatItem::Builtin(QatBuiltin::SwitchWindows),
        ),
        (
            crate::tr!("ribbon-menu", "user-interface"),
            QatItem::Builtin(QatBuiltin::UserInterface),
        ),
        (
            crate::tr!("ribbon-menu", "qat-batch-plot"),
            command("PUBLISH"),
        ),
        (
            crate::t!("Layer").into_owned(),
            QatItem::Builtin(QatBuiltin::LayerCombo),
        ),
        (
            crate::tr!("ribbon-menu", "qat-match-properties"),
            command("MATCHPROP"),
        ),
        (
            crate::tr!("ribbon-menu", "qat-plot-preview"),
            command("PREVIEW"),
        ),
        (
            crate::tr!("ribbon-menu", "qat-properties"),
            command("PROPERTIES"),
        ),
        (
            crate::t!("Sheet Set Manager").into_owned(),
            command("SHEETSET"),
        ),
    ]
}

/// A menu's title: semibold, with a line under it.
fn menu_header<'a>(title: String) -> Element<'a, Message> {
    let line =
        container(Space::new().width(Fill).height(icons::hairline())).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(chrome::border(theme))),
                ..Default::default()
            }
        });
    iced::widget::column![
        container(
            text(title)
                .size(metrics::QAT_MENU_TITLE_SIZE)
                .font(iced::Font::DEFAULT.weight(iced::font::Weight::Semibold))
                .style(|theme: &Theme| text::Style {
                    color: Some(chrome::strong(theme)),
                }),
        )
        .padding(metrics::QAT_MENU_TITLE_PAD),
        line,
    ]
    .width(Fill)
    .into()
}

/// A 1 px line across a menu.
fn menu_separator<'a>() -> Element<'a, Message> {
    container(
        container(Space::new().width(Fill).height(icons::hairline())).style(|theme: &Theme| {
            container::Style {
                background: Some(Background::Color(chrome::border(theme))),
                ..Default::default()
            }
        }),
    )
    .padding(metrics::QAT_MENU_SEP_PAD)
    .into()
}

/// Show Below / Above the Ribbon: disabled while the ribbon is closed.
fn place_row<'a>(label: String, below: bool, ribbon_closed: bool) -> Element<'a, Message> {
    let row = check_row(
        label,
        false,
        (!ribbon_closed).then_some(Message::RibbonQat(QatMsg::SetBelow(!below))),
    );
    if ribbon_closed {
        tooltip(
            row,
            make_tip(crate::tr!("ribbon-menu", "qat-place-unavailable")),
            TipPos::Right,
        )
        .delay(TIP_DELAY)
        .style(tip_style)
        .into()
    } else {
        row.into()
    }
}

/// The dropdown id of the list of the split item `id`.
fn split_dropdown_id(id: &str) -> String {
    format!("{SPLIT_PREFIX}{id}")
}

/// A command's tooltip: its label and the command it runs.
pub(super) fn command_tip(label: &str, cmd: &str) -> String {
    format!("{label}\n{} {cmd}", crate::t!("Command:"))
}

/// `btn` with its tooltip, which it leaves out while its menu is `open`:
/// the tip would cover the menu's title.
fn tipped<'a>(
    btn: impl Into<Element<'a, Message>>,
    tip: String,
    open: bool,
) -> Element<'a, Message> {
    if open {
        return btn.into();
    }
    tooltip(btn, make_tip(tip), TipPos::Bottom)
        .gap(6.0)
        .delay(TIP_DELAY)
        .style(tip_style)
        .into()
}

/// A 1 px line between two items.
fn separator<'a>() -> Element<'a, Message> {
    container(
        container(
            Space::new()
                .width(icons::hairline())
                .height(metrics::QAT_SEPARATOR_H),
        )
        .style(|theme: &Theme| container::Style {
            background: Some(Background::Color(chrome::border(theme))),
            ..Default::default()
        }),
    )
    .padding([0.0, metrics::QAT_SEPARATOR_MARGIN])
    .into()
}

/// The icon of a command item, greyed out unless `enabled`; a command with
/// no icon shows its name.
pub(super) fn face_icon<'a>(face: &Face, size: f32, enabled: bool) -> Element<'a, Message> {
    match face.icon {
        FaceIcon::Chrome(bytes) if enabled => icons::chrome(bytes, size),
        FaceIcon::Chrome(bytes) => icons::chrome_disabled(bytes, size),
        FaceIcon::Tool(IconKind::Svg(bytes)) if enabled => icons::tool(bytes, size),
        FaceIcon::Tool(IconKind::Svg(bytes)) => icons::tool_disabled(bytes, size),
        FaceIcon::Tool(IconKind::Glyph(glyph)) => text(glyph).size(size * 0.7).into(),
        FaceIcon::Text => row![text(face.label.clone())
            .size(metrics::QAT_TEXT_SIZE)
            .wrapping(iced::widget::text::Wrapping::None)
            .style(move |theme: &Theme| text::Style {
                color: (!enabled).then(|| chrome::muted(theme)),
            })]
        .into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ribbon_list_has_its_toolbar_copy() {
        for control in list_controls().map(|(control, _)| control) {
            let (id, source) = list_ids(control).expect("a list");
            assert_eq!(list_source(id), Some(source));
            assert_eq!(list_control(source), Some(control));
            assert!(!list_name(control).is_empty());
        }
        assert!(list_ids(QatBuiltin::Undo).is_none());
        assert!(list_source(QAT_SWITCH_ID).is_none());
    }

    #[test]
    fn a_toolbar_list_opens_its_ribbon_list_from_any_tab() {
        let mut ribbon = Ribbon::default();
        // The Annotate styles open from the Draw tab too.
        ribbon.select(0);
        for control in list_controls().map(|(control, _)| control) {
            ribbon.layout.qat_add(QatItem::Builtin(control));
            let (id, _) = list_ids(control).unwrap();
            ribbon.toggle_dropdown(id);
            let opens = ribbon
                .dropdown_overlay(&[], &[], (1400.0, 900.0), false, &[], &[], &[])
                .is_some();
            assert!(opens, "{id}");
            ribbon.close_dropdown();
        }
    }

    #[test]
    fn a_toolbar_list_opens_with_its_panel_hidden() {
        let mut ribbon = Ribbon::default();
        ribbon
            .layout
            .hidden_panels
            .insert("annotate/Dimensions".into());
        ribbon
            .layout
            .qat_add(QatItem::Builtin(QatBuiltin::DimStyle));
        ribbon.toggle_dropdown("QAT_DIM_STYLE");
        assert!(ribbon
            .dropdown_overlay(&[], &[], (1400.0, 900.0), false, &[], &[], &[])
            .is_some());
        // More Commands still says where it is.
        let found = ribbon
            .qat_candidates("dimension style")
            .into_iter()
            .find(|candidate| candidate.item == QatItem::Builtin(QatBuiltin::DimStyle))
            .expect("the dimension style list");
        assert!(found.path.contains("Dimensions"), "{}", found.path);
    }

    #[test]
    fn a_right_clicked_ribbon_list_goes_on_the_toolbar() {
        use super::super::ribbon_menu::RibbonMenuMsg;
        let mut ribbon = Ribbon::default();
        let color = QatItem::Builtin(QatBuiltin::Color);
        assert_eq!(list_control(PROP_COLOR_ID), Some(QatBuiltin::Color));
        assert!(ribbon.on_menu(RibbonMenuMsg::QatToggle(color.clone())));
        assert!(ribbon.layout.qat_has(&color));
        assert!(ribbon.on_menu(RibbonMenuMsg::QatToggle(color.clone())));
        assert!(!ribbon.layout.qat_has(&color));
    }

    #[test]
    fn more_commands_finds_the_ribbon_lists() {
        let ribbon = Ribbon::default();
        let found = |query: &str| -> Vec<QatItem> {
            ribbon
                .qat_candidates(query)
                .into_iter()
                .map(|candidate| candidate.item)
                .collect()
        };
        assert!(found("dimension style").contains(&QatItem::Builtin(QatBuiltin::DimStyle)));
        // By the system variable it sets, too.
        assert!(found("celtype").contains(&QatItem::Builtin(QatBuiltin::Linetype)));
        assert!(found("center layer").contains(&QatItem::Builtin(QatBuiltin::CenterLayer)));
        let lineweight = ribbon
            .qat_candidates("lineweight")
            .into_iter()
            .find(|candidate| candidate.item == QatItem::Builtin(QatBuiltin::Lineweight))
            .expect("the lineweight list");
        assert!(
            lineweight.path.contains("Properties"),
            "{}",
            lineweight.path
        );
    }

    #[test]
    fn the_default_toolbar_keeps_its_icons_and_labels() {
        let ribbon = Ribbon::default();
        let new = ribbon.command_face("QNEW");
        assert_eq!(new.label, "New");
        assert!(matches!(new.icon, FaceIcon::Chrome(icon) if icon == icons::DOC_NEW));
        assert_eq!(ribbon.command_face("PRINT").label, "Print");
    }

    #[test]
    fn a_ribbon_command_takes_the_ribbon_buttons_face() {
        let ribbon = Ribbon::default();
        let face = ribbon.command_face("HATCH");
        assert!(matches!(face.icon, FaceIcon::Tool(_)));
        assert!(!face.label.is_empty());
        // A slide-out tool and a dropdown item are found too.
        assert_eq!(ribbon.command_face("SPLINE").label, "Spline Fit");
        assert_eq!(ribbon.command_face("AREA").label, "Area");
        // A dropdown by its id shows the dropdown.
        assert_eq!(
            ribbon.command_face("MEASURE_MENU").label,
            crate::t!("Measure")
        );
    }

    #[test]
    fn a_menu_command_takes_the_menu_entry_and_an_unknown_one_its_name() {
        let ribbon = Ribbon::default();
        assert_eq!(ribbon.command_face("ZOOM E").label, "Extents");
        let unknown = ribbon.command_face("NOSUCHCOMMAND");
        assert_eq!(unknown.label, "NOSUCHCOMMAND");
        assert!(matches!(unknown.icon, FaceIcon::Text));
    }

    #[test]
    fn a_split_item_runs_the_variant_last_chosen() {
        let mut ribbon = Ribbon::default();
        ribbon.layout.qat = vec![QatItem::Command("MEASURE_MENU".into())];
        let (id, _, items, default) = ribbon.ribbon_dropdown("measure_menu").unwrap();
        assert_eq!(id, "MEASURE_MENU");
        assert_eq!(ribbon.qat_face_command(id, items, default), "DIST");

        let out = ribbon.on_qat(QatMsg::Pick {
            item: id.to_string(),
            cmd: "AREA",
        });
        assert!(out.save);
        assert!(matches!(out.run, Some(Message::MenuCommand(ref cmd)) if cmd == "AREA"));
        let (id, _, items, default) = ribbon.ribbon_dropdown(id).unwrap();
        assert_eq!(ribbon.qat_face_command(id, items, default), "AREA");

        // A saved face the list no longer has falls back to the default.
        ribbon
            .layout
            .qat_faces
            .insert("MEASURE_MENU".to_string(), "GONE".to_string());
        let (id, _, items, default) = ribbon.ribbon_dropdown("MEASURE_MENU").unwrap();
        assert_eq!(ribbon.qat_face_command(id, items, default), "DIST");
    }

    #[test]
    fn a_plain_command_is_not_a_split_item() {
        let ribbon = Ribbon::default();
        assert!(ribbon.ribbon_dropdown("LINE").is_none());
        // Hatch has variants on the ribbon, so it is one.
        assert!(ribbon.ribbon_dropdown("HATCH").is_some());
        assert!(ribbon.ribbon_dropdown("ZOOM E").is_none());
    }

    #[test]
    fn no_quick_access_menu_opens_without_a_drawing() {
        for id in [
            QAT_SWITCH_ID,
            QAT_UI_ID,
            QAT_CUSTOMIZE_ID,
            QAT_PICKER_ID,
            QAT_LAYER_COMBO_ID,
            &split_dropdown_id("MEASURE_MENU"),
        ] {
            assert!(!super::super::works_without_drawing(id), "{id}");
        }
        let ribbon = Ribbon::default();
        assert!(ribbon
            .qat_overlay(QAT_SWITCH_ID, (800.0, 600.0), true, &[])
            .is_none());
    }

    #[test]
    fn the_customize_menu_ticks_and_toggles_its_commands() {
        let mut ribbon = Ribbon::default();
        let standard = standard_items();
        // The default toolbar ticks New to Redo, Plot being its Print.
        let ticked: Vec<bool> = standard
            .iter()
            .map(|(_, item)| ribbon.layout.qat_has(item))
            .collect();
        assert_eq!(&ticked[..7], [true; 7]);
        assert!(ticked[7..].iter().all(|on| !on));

        let layer = QatItem::Builtin(QatBuiltin::LayerCombo);
        assert!(ribbon.on_qat(QatMsg::Toggle(layer.clone())).save);
        assert_eq!(ribbon.layout.qat.last(), Some(&layer));
        ribbon.on_qat(QatMsg::Toggle(layer.clone()));
        assert!(!ribbon.layout.qat_has(&layer));
    }

    #[test]
    fn more_commands_adds_a_found_command_once() {
        let mut ribbon = Ribbon::default();
        assert!(ribbon.on_qat(QatMsg::OpenPicker).focus);
        assert_eq!(ribbon.open_dropdown.as_deref(), Some(QAT_PICKER_ID));
        ribbon.on_qat(QatMsg::PickerInput("hatch".into()));
        let candidates = ribbon.qat_candidates("hatch");
        assert!(candidates
            .iter()
            .any(|c| c.item.same(&QatItem::Command("HATCH".into()))));

        assert!(ribbon.on_qat(QatMsg::PickerSubmit).save);
        assert!(ribbon.open_dropdown.is_none());
        let added = ribbon.layout.qat.last().cloned().unwrap();
        // Adding it again changes nothing.
        let len = ribbon.layout.qat.len();
        assert!(!ribbon.on_qat(QatMsg::Add(added)).save);
        assert_eq!(ribbon.layout.qat.len(), len);
    }

    #[test]
    fn the_toolbar_moves_below_the_ribbon() {
        let mut ribbon = Ribbon::default();
        assert!(ribbon.on_qat(QatMsg::SetBelow(true)).save);
        assert!(ribbon.layout.qat_below);
    }

    #[test]
    fn a_command_items_look_is_worked_out_once_and_forgotten_with_the_tabs() {
        let mut ribbon = Ribbon::default();
        assert!(matches!(
            ribbon.qat_look("measure_menu"),
            QatLook::Split {
                id: "MEASURE_MENU",
                ..
            }
        ));
        assert!(matches!(
            ribbon.qat_look("LINE"),
            QatLook::Plain { runs: true, .. }
        ));
        assert_eq!(ribbon.qat_cache.borrow().looks.len(), 2);
        // The same command in other case is the same entry.
        ribbon.qat_look("line");
        assert_eq!(ribbon.qat_cache.borrow().looks.len(), 2);
        ribbon.set_modules(crate::plugin::all_ribbon_modules());
        assert!(ribbon.qat_cache.borrow().looks.is_empty());
    }

    #[test]
    fn more_commands_finds_on_input_not_on_every_frame() {
        let mut ribbon = Ribbon::default();
        ribbon.on_qat(QatMsg::OpenPicker);
        assert!(ribbon.qat_found.is_empty());
        ribbon.on_qat(QatMsg::PickerInput("hatch".into()));
        assert!(!ribbon.qat_found.is_empty());
        ribbon.on_qat(QatMsg::PickerInput(String::new()));
        assert!(ribbon.qat_found.is_empty());
    }

    #[test]
    fn the_customize_list_has_layer_and_sheet_sets_and_no_zoom_extents() {
        let items = standard_items();
        let commands: Vec<String> = items
            .iter()
            .filter_map(|(_, item)| match item {
                QatItem::Command(cmd) => Some(cmd.clone()),
                _ => None,
            })
            .collect();
        assert!(!commands.iter().any(|cmd| cmd.starts_with("ZOOM")));
        // Sheet Set Manager comes right after Properties, last.
        assert_eq!(&commands[commands.len() - 2..], ["PROPERTIES", "SHEETSET"]);
        let layer = items
            .iter()
            .find(|(_, item)| *item == QatItem::Builtin(QatBuiltin::LayerCombo))
            .unwrap();
        assert_eq!(layer.0, crate::t!("Layer"));
    }

    #[test]
    fn a_command_ocs_lacks_is_greyed_out_and_has_no_key_tip() {
        let mut ribbon = Ribbon::default();
        let preview = QatItem::Command("PREVIEW".into());
        assert!(!ribbon.item_runs(&preview));
        assert!(ribbon.item_runs(&QatItem::Command("SHEETSET".into())));
        assert!(ribbon.item_runs(&QatItem::Command("MEASURE_MENU".into())));
        assert!(ribbon.item_runs(&QatItem::Builtin(QatBuiltin::UserInterface)));
        assert!(matches!(
            ribbon.qat_look("PREVIEW"),
            QatLook::Plain { runs: false, .. }
        ));

        // A ribbon button's command runs although it is not registered.
        assert!(!crate::ui::menu_bar::implemented("_SSMTOGGLE"));
        assert!(ribbon.item_runs(&QatItem::Command("_SSMTOGGLE".into())));
        assert!(!ribbon.item_runs(&QatItem::Command("NOSUCHCOMMAND".into())));

        // The customize menu cannot add it, but can take it off again.
        assert!(!ribbon.can_toggle(&preview));
        ribbon.layout.qat = vec![QatItem::Command("SAVE".into()), preview.clone()];
        assert!(ribbon.can_toggle(&preview));
        let tips = ribbon.qat_tips();
        assert_eq!(tips.len(), 1);
        assert_eq!(tips[0].target, "SAVE");
    }

    #[test]
    fn batch_plot_and_plot_preview_have_icons() {
        let ribbon = Ribbon::default();
        let publish = ribbon.command_face("PUBLISH");
        assert!(matches!(publish.icon, FaceIcon::Tool(IconKind::Svg(_))));
        assert_eq!(publish.label, crate::tr!("ribbon-menu", "qat-batch-plot"));
        assert!(matches!(
            ribbon.command_face("PREVIEW").icon,
            FaceIcon::Chrome(_)
        ));
    }
}
