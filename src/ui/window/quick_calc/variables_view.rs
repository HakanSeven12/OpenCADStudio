//! The Variables section: its toolbar, the inline Variable Definition form
//! and the variables listed by category.

use std::collections::HashMap;

use iced::widget::{button, column, container, mouse_area, row, text, text_input, tooltip, Space};
use iced::{font, Background, Border, Color, Element, Fill, Font, Theme};

use super::view::{
    field_style, icon, m, mix, muted, segment, segmented, strong, text_button, tokens, STRONG,
};
use super::{FormMsg, QuickCalcMsg, QuickCalcPalette, VarForm, VarKind, VarMsg, Variable};
use crate::app::calc::{CalcContext, CalcError, Value};
use crate::app::Message;
use crate::ui::icons::{TRI_DOWN, TRI_RIGHT};

const NEW: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/var_new.svg");
const EDIT: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/var_edit.svg");
const RETURN: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/var_return.svg");

const TOOLBAR_H: f32 = 34.0;
const CATEGORY_H: f32 = 24.0;
const ROW_H: f32 = 26.0;
const HINT_H: f32 = 36.0;

/// The type badge's letters: italic serif, as math.
const BADGE: Font = Font {
    family: font::Family::Serif,
    style: font::Style::Italic,
    ..Font::DEFAULT
};

fn v(msg: VarMsg) -> Message {
    m(QuickCalcMsg::Var(msg))
}

fn f(msg: FormMsg) -> Message {
    v(VarMsg::Form(msg))
}

/// A tooltip as on the ribbon.
fn tip<'a>(content: impl Into<Element<'a, Message>>, label: String) -> Element<'a, Message> {
    super::view::tip(content, label)
}

/// A toolbar icon button; without a message it is dimmed and does nothing.
fn tool<'a>(bytes: &'static [u8], label: String, msg: Option<VarMsg>) -> Element<'a, Message> {
    let enabled = msg.is_some();
    let glyph = if enabled {
        icon(bytes, 17.0, |t| t.muted)
    } else {
        icon(bytes, 17.0, super::view::disabled)
    };
    let b = button(container(glyph).center(Fill))
        .width(28)
        .height(28)
        .padding(0)
        .on_press_maybe(msg.map(v))
        .style(|theme: &Theme, status| {
            let t = tokens(theme);
            button::Style {
                background: matches!(status, button::Status::Hovered | button::Status::Pressed)
                    .then_some(Background::Color(t.strong)),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                text_color: t.text,
                ..Default::default()
            }
        });
    tip(b, label)
}

fn toolbar<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let selected = p.selected_var.is_some();
    let when = |msg: VarMsg| selected.then_some(msg);
    let row = row![
        tool(
            NEW,
            crate::t!("New variable").into_owned(),
            Some(VarMsg::New)
        ),
        tool(
            EDIT,
            crate::t!("Edit variable").into_owned(),
            when(VarMsg::Edit)
        ),
        tool(
            crate::ui::icons::TRASH,
            crate::t!("Delete variable").into_owned(),
            when(VarMsg::Delete)
        ),
        Space::new().width(Fill),
        tool(
            RETURN,
            crate::t!("Return variable to the input").into_owned(),
            when(VarMsg::Return)
        ),
    ]
    .spacing(2)
    .align_y(iced::Center);
    container(row).padding([3, 6]).width(Fill).into()
}

fn label<'a>(s: &str) -> Element<'a, Message> {
    container(muted(crate::t!(s).into_owned(), 10.5))
        .width(62)
        .into()
}

fn chip<'a>(label: String, on: bool, msg: Message) -> Element<'a, Message> {
    button(container(text(label).size(11)).center_y(Fill))
        .height(22)
        .padding([0, 8])
        .on_press(msg)
        .style(move |theme: &Theme, status| {
            let t = tokens(theme);
            button::Style {
                background: on.then_some(Background::Color(t.accent.scale_alpha(0.16))),
                border: Border {
                    color: if on { t.accent } else { t.line },
                    width: 1.0,
                    radius: 11.0.into(),
                },
                text_color: if on || matches!(status, button::Status::Hovered) {
                    t.text
                } else {
                    t.muted
                },
                ..Default::default()
            }
        })
        .into()
}

fn field<'a>(
    value: &str,
    placeholder: String,
    on_input: fn(String) -> FormMsg,
) -> text_input::TextInput<'a, Message> {
    text_input(&placeholder, value)
        .on_input(move |s| f(on_input(s)))
        .on_submit(f(FormMsg::Save))
        .size(12)
        .padding([5, 7])
        .style(|theme, status| field_style(theme, status, false))
}

/// The inline Variable Definition form.
fn form<'a>(
    p: &QuickCalcPalette,
    form: &VarForm,
    check: Result<Value, String>,
) -> Element<'a, Message> {
    let constant = form.kind == VarKind::Constant;
    let kind = segmented(vec![
        segment(
            crate::t!("Constant").into_owned(),
            constant,
            QuickCalcMsg::Var(VarMsg::Form(FormMsg::Kind(VarKind::Constant))),
        ),
        segment(
            crate::t!("Function").into_owned(),
            !constant,
            QuickCalcMsg::Var(VarMsg::Form(FormMsg::Kind(VarKind::Function))),
        ),
    ]);
    // The name field shows a fixed `$` inside its border.
    let name = container(
        row![
            text("$").size(12).style(|theme: &Theme| text::Style {
                color: Some(tokens(theme).accent),
            }),
            text_input("", &form.name)
                .on_input(|s| f(FormMsg::Name(s)))
                .on_submit(f(FormMsg::Save))
                .size(12)
                .padding([5, 1])
                .style(|theme, status| {
                    let mut s = field_style(theme, status, false);
                    s.border.width = 0.0;
                    s.background = Background::Color(Color::TRANSPARENT);
                    s
                }),
        ]
        .align_y(iced::Center),
    )
    .padding(iced::Padding {
        left: 7.0,
        ..iced::Padding::ZERO
    })
    .style(|theme: &Theme| {
        let t = tokens(theme);
        container::Style {
            background: Some(Background::Color(t.field)),
            border: Border {
                color: t.line,
                width: 1.0,
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    });
    let mut categories = p.categories();
    if !categories.contains(&form.category) {
        categories.push(form.category.clone());
    }
    let choosing_new = form.new_category.is_some();
    let mut chips = row![].spacing(4);
    for category in categories {
        let on = !choosing_new && category == form.category;
        chips = chips.push(chip(category.clone(), on, f(FormMsg::Category(category))));
    }
    chips = chips.push(chip(
        crate::t!("+ New").into_owned(),
        choosing_new,
        f(FormMsg::NewCategory),
    ));
    let mut grid = column![
        row![label("Type"), kind].align_y(iced::Center),
        row![label("Name"), name].align_y(iced::Center),
        row![label("Category"), chips.wrap()].align_y(iced::Center),
    ]
    .spacing(8);
    if let Some(new) = &form.new_category {
        grid = grid.push(
            row![
                label("New category"),
                field(
                    new,
                    crate::t!("e.g. Stairs").into_owned(),
                    FormMsg::NewCategoryName,
                )
            ]
            .align_y(iced::Center),
        );
    }
    let what = if constant { "Value" } else { "Expression" };
    grid = grid.push(
        row![label(what), field(&form.expr, String::new(), FormMsg::Expr)].align_y(iced::Center),
    );
    let result: Element<'a, Message> = match &check {
        Ok(value) => row![
            strong(format!("= {}", value.display()), 11.0),
            muted(
                if constant {
                    crate::t!("stored as a fixed value")
                } else {
                    crate::t!("now, recalculated each time it is used")
                }
                .into_owned(),
                11.0
            ),
        ]
        .spacing(5)
        .into(),
        Err(problem) => text(problem.clone())
            .size(11)
            .style(|theme: &Theme| text::Style {
                color: Some(tokens(theme).danger_text),
            })
            .into(),
    };
    grid = grid.push(row![Space::new().width(62), result]);
    grid = grid.push(
        row![
            label("Description"),
            field(
                &form.description,
                crate::t!("Optional, shown as a tooltip").into_owned(),
                FormMsg::Description,
            )
        ]
        .align_y(iced::Center),
    );
    let save_label = if form.editing.is_some() {
        crate::t!("Save")
    } else {
        crate::t!("Add")
    };
    let valid = check.is_ok();
    let save = button(text(save_label.into_owned()).size(11).font(STRONG))
        .padding([4, 12])
        .on_press_maybe(valid.then(|| f(FormMsg::Save)))
        .style(move |theme: &Theme, status| {
            let t = tokens(theme);
            let hovered = matches!(status, button::Status::Hovered);
            button::Style {
                background: Some(Background::Color(if !valid {
                    t.accent.scale_alpha(0.4)
                } else if hovered {
                    mix(t.accent, Color::WHITE, 0.1)
                } else {
                    t.accent
                })),
                border: Border {
                    color: t.accent,
                    width: 1.0,
                    radius: 3.0.into(),
                },
                text_color: t.on_accent,
                ..Default::default()
            }
        });
    let title = if form.editing.is_some() {
        crate::t!("Edit variable")
    } else {
        crate::t!("New variable")
    };
    container(
        column![
            strong(title.into_owned(), 11.5),
            grid,
            row![
                Space::new().width(Fill),
                text_button(
                    text(crate::t!("Cancel")).size(11),
                    QuickCalcMsg::Var(VarMsg::Form(FormMsg::Cancel))
                ),
                save,
            ]
            .spacing(6),
        ]
        .spacing(8),
    )
    .padding(10)
    .width(Fill)
    .style(|theme: &Theme| {
        let t = tokens(theme);
        container::Style {
            background: Some(Background::Color(t.weak)),
            border: Border {
                color: t.line,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..Default::default()
        }
    })
    .into()
}

/// What a row shows and says: the value, or `!` for a function that can't
/// be calculated; the tooltip holds the description, the function and the
/// error.
fn row_of<'a>(
    var: &Variable,
    value: Option<Value>,
    error: Option<&CalcError>,
    selected: bool,
) -> Element<'a, Message> {
    let (badge, kind) = match var.kind {
        VarKind::Constant => ("x", crate::t!("Constant")),
        VarKind::Function => ("f", crate::t!("Function")),
    };
    let shown = match (value, error) {
        (Some(value), _) => value.display(),
        (None, _) => "!".to_string(),
    };
    let broken = value.is_none();
    let content = row![
        container(
            text(badge)
                .size(13)
                .font(BADGE)
                .style(|theme: &Theme| text::Style {
                    color: Some(tokens(theme).muted),
                })
        )
        .width(14)
        .center_x(14),
        text(format!("${}", var.name))
            .size(12)
            .style(|theme: &Theme| text::Style {
                color: Some(tokens(theme).accent),
            }),
        Space::new().width(Fill),
        text(shown).size(12).style(move |theme: &Theme| {
            let t = tokens(theme);
            text::Style {
                color: Some(if broken { t.danger_text } else { t.text }),
            }
        }),
    ]
    .spacing(6)
    .align_y(iced::Center);
    let body = container(content)
        .center_y(ROW_H)
        .padding(iced::Padding {
            left: 14.0,
            right: 10.0,
            top: 0.0,
            bottom: 0.0,
        })
        .width(Fill)
        .style(move |theme: &Theme| container::Style {
            background: selected.then(|| Background::Color(tokens(theme).accent.scale_alpha(0.16))),
            ..Default::default()
        });
    let area = mouse_area(body)
        .on_press(v(VarMsg::Select(var.name.clone())))
        .on_double_click(v(VarMsg::Insert(var.name.clone())))
        .interaction(iced::mouse::Interaction::Pointer);
    let mut lines = vec![];
    if !var.description.is_empty() {
        lines.push(var.description.clone());
    }
    lines.push(kind.into_owned());
    if var.kind == VarKind::Function {
        lines.push(crate::tf!("Function: {}", var.expr).into_owned());
    }
    if let Some(error) = error {
        lines.push(error.message.clone());
    }
    lines.push(crate::t!("Double-click to insert").into_owned());
    tooltip(
        area,
        crate::ui::ribbon::tooltip_content(lines.join("\n")),
        tooltip::Position::Bottom,
    )
    .delay(std::time::Duration::from_millis(400))
    .style(crate::ui::ribbon::tooltip_style)
    .into()
}

pub(super) fn variables<'a>(
    p: &QuickCalcPalette,
    ctx: &CalcContext,
    errors: &HashMap<String, CalcError>,
    check: Option<Result<Value, String>>,
) -> Element<'a, Message> {
    let mut body = column![toolbar(p)];
    if let (Some(open), Some(check)) = (&p.form, check) {
        body = body.push(container(form(p, open, check)).padding([6, 8]));
    }
    let mut list = column![].padding([4, 0]);
    for category in p.categories() {
        let open = !p.collapsed.contains(&category);
        let members: Vec<&Variable> = p
            .variables
            .iter()
            .filter(|var| var.category == category)
            .collect();
        let header = button(
            row![
                icon(if open { TRI_DOWN } else { TRI_RIGHT }, 9.0, |t| t.muted),
                strong(category.clone(), 11.0).width(Fill),
                muted(members.len().to_string(), 11.0),
            ]
            .spacing(6)
            .align_y(iced::Center),
        )
        .width(Fill)
        .height(CATEGORY_H)
        .padding([0, 8])
        .on_press(v(VarMsg::ToggleCategory(category.clone())))
        .style(|theme: &Theme, status| {
            let t = tokens(theme);
            button::Style {
                background: matches!(status, button::Status::Hovered)
                    .then_some(Background::Color(t.weak)),
                text_color: t.text,
                ..Default::default()
            }
        });
        list = list.push(header);
        if open {
            for var in members {
                let value = match var.kind {
                    VarKind::Constant => var.value.map(Value::from),
                    VarKind::Function => ctx.vars.get(&var.name).copied(),
                };
                let selected = p.selected_var.as_deref() == Some(var.name.as_str());
                list = list.push(row_of(var, value, errors.get(&var.name), selected));
            }
        }
    }
    body.push(list)
        .push(
            container(muted(
                crate::t!("Double-click to insert. Typing $name = value adds a constant too.")
                    .into_owned(),
                10.5,
            ))
            .padding([2, 8]),
        )
        .into()
}

/// The section body's height as laid out, for the pallet's sections budget.
pub(super) fn height(p: &QuickCalcPalette) -> f32 {
    let rows: f32 = p
        .categories()
        .iter()
        .map(|c| {
            CATEGORY_H
                + if p.collapsed.contains(c) {
                    0.0
                } else {
                    p.variables.iter().filter(|v| &v.category == c).count() as f32 * ROW_H
                }
        })
        .sum();
    let form = match &p.form {
        Some(f) => 300.0 + if f.new_category.is_some() { 34.0 } else { 0.0 },
        None => 0.0,
    };
    TOOLBAR_H + form + 8.0 + rows + HINT_H
}
