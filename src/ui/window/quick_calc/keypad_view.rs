//! The keypad, always shown under the input: memory row, Standard or Scientific keys, and the
//! flyouts (trigonometry, functions, points, memory) that open over them.

use iced::widget::text::Span;
use iced::widget::{
    button, column, container, mouse_area, opaque, rich_text, row, scrollable, stack, text, Space,
};
use iced::{font, mouse, Background, Border, Color, Element, Fill, Font, Padding, Shadow, Theme};

use super::view::{icon, m, mix, muted, strong, tip, tokens, POINT, STRONG};
use super::{Flyout, Key, KeypadMode, MemoryOp, QuickCalcMsg, QuickCalcPalette};
use crate::app::calc::Value;
use crate::app::Message;
use crate::ui::icons::TRI_DOWN;

const BACKSPACE: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/backspace.svg");
const TRIGONOMETRY: &[u8] = include_bytes!("../../../../assets/icons/quickcalc/trigonometry.svg");

const GAP: f32 = 3.0;
const STANDARD_KEY_H: f32 = 36.0;
const SCIENTIFIC_KEY_H: f32 = 34.0;
const SIGNATURE_KEY_H: f32 = 40.0;
const MEMORY_ROW_H: f32 = 28.0;
const FLYOUT_ROW_H: f32 = 30.0;
const ROW_GAP: f32 = 4.0;

/// Math letters (x, y, e, n, f): italic serif, a little larger.
pub(super) const MATH: Font = Font {
    family: font::Family::Serif,
    style: font::Style::Italic,
    ..Font::DEFAULT
};

/// A key label: runs of plain text and math letters.
type Label = Vec<(String, bool)>;

fn plain(s: &str) -> Label {
    vec![(s.to_string(), false)]
}

/// `parts` alternate plain and math, starting with plain: `lbl(&["", "x", "²"])`
/// is an italic *x* followed by a plain ².
fn lbl(parts: &[&str]) -> Label {
    parts
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, s)| (s.to_string(), i % 2 == 1))
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Look {
    /// Function keys.
    Function,
    /// Digits and the decimal point: a lighter fill.
    Digit,
    /// ÷ × − +.
    Operator,
    /// =.
    Equals,
    /// 2nd / hyp while on.
    On,
}

fn key_style(look: Look) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let t = tokens(theme);
        let hovered = matches!(status, button::Status::Hovered);
        let pressed = matches!(status, button::Status::Pressed);
        let (fill, border, color) = match look {
            Look::Digit => (
                if hovered { t.digit_hover } else { t.digit },
                t.key_line,
                t.text,
            ),
            Look::Equals => (
                if hovered {
                    mix(t.accent, Color::WHITE, 0.1)
                } else {
                    t.accent
                },
                t.accent,
                t.on_accent,
            ),
            Look::On => (t.accent.scale_alpha(0.18), t.accent, t.text),
            Look::Function | Look::Operator => (
                if hovered { t.key_hover } else { t.key },
                t.key_line,
                t.text,
            ),
        };
        button::Style {
            background: Some(Background::Color(if pressed {
                fill.scale_alpha(0.82)
            } else {
                fill
            })),
            border: Border {
                color: border,
                width: 1.0,
                radius: 4.0.into(),
            },
            text_color: color,
            ..Default::default()
        }
    }
}

fn label_element<'a>(label: Label, look: Look) -> Element<'a, Message> {
    let (size, font) = match look {
        Look::Digit => (16.0, STRONG),
        Look::Operator => (18.0, Font::DEFAULT),
        Look::Equals => (20.0, Font::DEFAULT),
        Look::Function | Look::On => (13.0, STRONG),
    };
    let spans: Vec<Span<'a, (), Font>> = label
        .into_iter()
        .map(|(s, math)| {
            if math {
                Span::new(s).font(MATH).size(size * 1.2)
            } else {
                Span::new(s).font(font).size(size)
            }
        })
        .collect();
    rich_text(spans).size(size).into()
}

fn key<'a>(label: Label, msg: QuickCalcMsg, look: Look, height: f32) -> Element<'a, Message> {
    button(container(label_element(label, look)).center(Fill))
        .width(Fill)
        .height(height)
        .padding(0)
        .on_press(m(msg))
        .style(key_style(look))
        .into()
}

fn k<'a>(label: Label, k: Key, look: Look, height: f32) -> Element<'a, Message> {
    key(label, QuickCalcMsg::Key(k), look, height)
}

fn backspace<'a>(height: f32) -> Element<'a, Message> {
    button(container(icon(BACKSPACE, 18.0, |t| t.text)).center(Fill))
        .width(Fill)
        .height(height)
        .padding(0)
        .on_press(m(QuickCalcMsg::Key(Key::Back)))
        .style(key_style(Look::Function))
        .into()
}

fn grid<'a>(keys: Vec<Element<'a, Message>>, columns: usize) -> Element<'a, Message> {
    let mut rows = column![].spacing(GAP);
    let mut keys = keys.into_iter().peekable();
    while keys.peek().is_some() {
        rows = rows.push(row(keys.by_ref().take(columns).collect::<Vec<_>>()).spacing(GAP));
    }
    rows.into()
}

fn digit<'a>(d: &'static str, height: f32) -> Element<'a, Message> {
    k(plain(d), Key::Text(d), Look::Digit, height)
}

fn operator<'a>(label: &str, ins: &'static str, height: f32) -> Element<'a, Message> {
    k(plain(label), Key::Text(ins), Look::Operator, height)
}

fn equals<'a>(height: f32) -> Element<'a, Message> {
    k(plain("="), Key::Eval, Look::Equals, height)
}

fn standard<'a>() -> Element<'a, Message> {
    let h = STANDARD_KEY_H;
    let f = |label: Label, key: Key| k(label, key, Look::Function, h);
    grid(
        vec![
            f(plain("%"), Key::Postfix("/100")),
            f(plain("CE"), Key::ClearEntry),
            f(plain("C"), Key::Clear),
            backspace(h),
            f(lbl(&["¹⁄", "x"]), Key::Prefix("1/")),
            f(lbl(&["", "x", "²"]), Key::Postfix("^2")),
            f(lbl(&["²√", "x"]), Key::Func("sqrt")),
            operator("÷", " / ", h),
            digit("7", h),
            digit("8", h),
            digit("9", h),
            operator("×", " * ", h),
            digit("4", h),
            digit("5", h),
            digit("6", h),
            operator("−", " - ", h),
            digit("1", h),
            digit("2", h),
            digit("3", h),
            operator("+", " + ", h),
            f(plain("+/−"), Key::Negate),
            digit("0", h),
            digit(".", h),
            equals(h),
        ],
        4,
    )
}

fn second_key<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let look = if p.second { Look::On } else { Look::Function };
    k(plain("2ⁿᵈ"), Key::Second, look, SCIENTIFIC_KEY_H)
}

fn scientific<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let h = SCIENTIFIC_KEY_H;
    let f = |label: Label, key: Key| k(label, key, Look::Function, h);
    let second = p.second;
    // 2nd swaps a key for its second function.
    let swap = |first: (Label, Key), other: (Label, Key)| {
        let (label, key) = if second { other } else { first };
        f(label, key)
    };
    grid(
        vec![
            second_key(p),
            f(plain("π"), Key::Text("pi")),
            f(lbl(&["", "e"]), Key::Text("e")),
            f(plain("C"), Key::Clear),
            backspace(h),
            swap(
                (lbl(&["", "x", "²"]), Key::Postfix("^2")),
                (lbl(&["", "x", "³"]), Key::Postfix("^3")),
            ),
            f(lbl(&["¹⁄", "x"]), Key::Prefix("1/")),
            f(lbl(&["|", "x", "|"]), Key::Func("abs")),
            f(plain("exp"), Key::Func("exp")),
            f(plain("mod"), Key::Text(" mod ")),
            swap(
                (lbl(&["²√", "x"]), Key::Func("sqrt")),
                (lbl(&["³√", "x"]), Key::Func("cbrt")),
            ),
            f(plain("("), Key::Text("(")),
            f(plain(")"), Key::Text(")")),
            f(lbl(&["", "n", "!"]), Key::Func("fact")),
            operator("÷", " / ", h),
            swap(
                (lbl(&["", "x", "ʸ"]), Key::Text("^")),
                (lbl(&["ʸ√", "x"]), Key::TwoArg("root")),
            ),
            digit("7", h),
            digit("8", h),
            digit("9", h),
            operator("×", " * ", h),
            swap(
                (plain("10ˣ"), Key::Prefix("10^")),
                (plain("2ˣ"), Key::Prefix("2^")),
            ),
            digit("4", h),
            digit("5", h),
            digit("6", h),
            operator("−", " - ", h),
            swap(
                (plain("log"), Key::Func("log")),
                (lbl(&["logᵧ", "x"]), Key::TwoArg("logb")),
            ),
            digit("1", h),
            digit("2", h),
            digit("3", h),
            operator("+", " + ", h),
            swap(
                (plain("ln"), Key::Func("ln")),
                (lbl(&["", "e", "ˣ"]), Key::Func("exp")),
            ),
            f(plain("+/−"), Key::Negate),
            digit("0", h),
            digit(".", h),
            equals(h),
        ],
        5,
    )
}

/// One memory row button: no border, a fill only on hover, dimmed while
/// there is nothing in memory to act on.
fn memory_button<'a>(
    content: Element<'a, Message>,
    msg: QuickCalcMsg,
    enabled: bool,
    open: bool,
) -> Element<'a, Message> {
    button(container(content).center(Fill))
        .width(Fill)
        .height(MEMORY_ROW_H)
        .padding(0)
        .on_press_maybe(enabled.then_some(m(msg)))
        .style(move |theme: &Theme, status| {
            let t = tokens(theme);
            let disabled = matches!(status, button::Status::Disabled);
            button::Style {
                background: (open
                    || matches!(status, button::Status::Hovered | button::Status::Pressed))
                .then_some(Background::Color(t.strong)),
                border: Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                text_color: if disabled {
                    super::view::disabled(&t)
                } else {
                    t.text
                },
                ..Default::default()
            }
        })
        .into()
}

fn memory_row<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let has = !p.memory.is_empty();
    let label = |s: &str| -> Element<'a, Message> {
        text(s.to_string())
            .size(crate::ui::style::text::UI)
            .font(STRONG)
            .into()
    };
    let op = |s: &str, op: MemoryOp, enabled: bool| {
        memory_button(label(s), QuickCalcMsg::Memory(op), enabled, false)
    };
    let list_open = p.flyout == Some(Flyout::Memory);
    let list = memory_button(
        row![
            text("M").size(crate::ui::style::text::UI).font(STRONG),
            // The svg doesn't take the button's text colour, so it is
            // dimmed here to match the disabled label.
            if has {
                icon(TRI_DOWN, 10.0, |t| t.text)
            } else {
                icon(TRI_DOWN, 10.0, super::view::disabled)
            }
        ]
        .spacing(2)
        .align_y(iced::Center)
        .into(),
        QuickCalcMsg::Flyout(Flyout::Memory),
        has,
        list_open,
    );
    row![
        op("MC", MemoryOp::Clear, has),
        op("MR", MemoryOp::Recall, has),
        op("M+", MemoryOp::Add, true),
        op("M−", MemoryOp::Subtract, true),
        op("MS", MemoryOp::Store, true),
        list,
    ]
    .spacing(2)
    .into()
}

/// A button that opens one of the scientific flyouts.
fn flyout_button<'a>(
    lead: Element<'a, Message>,
    label: String,
    flyout: Flyout,
    open: bool,
) -> Element<'a, Message> {
    button(
        row![
            lead,
            text(label).size(12).font(STRONG),
            icon(TRI_DOWN, 10.0, |t| t.muted),
        ]
        .spacing(6)
        .align_y(iced::Center),
    )
    .height(FLYOUT_ROW_H)
    .padding([0, 8])
    .on_press(m(QuickCalcMsg::Flyout(flyout)))
    .style(move |theme: &Theme, status| {
        let t = tokens(theme);
        button::Style {
            background: (open
                || matches!(status, button::Status::Hovered | button::Status::Pressed))
            .then_some(Background::Color(t.strong)),
            border: Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            text_color: t.text,
            ..Default::default()
        }
    })
    .into()
}

fn flyout_row<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let open = |f: Flyout| p.flyout == Some(f);
    row![
        flyout_button(
            icon(TRIGONOMETRY, 15.0, |t| t.text),
            crate::t!("Trigonometry").into_owned(),
            Flyout::Trigonometry,
            open(Flyout::Trigonometry),
        ),
        flyout_button(
            text("f").size(16).font(MATH).into(),
            crate::t!("Function").into_owned(),
            Flyout::Function,
            open(Flyout::Function),
        ),
        flyout_button(
            icon(POINT, 14.0, |t| t.text),
            crate::t!("Point").into_owned(),
            Flyout::Point,
            open(Flyout::Point),
        ),
    ]
    .spacing(2)
    .into()
}

/// A flyout's panel: raised over the keys with a border and a shadow.
/// `opaque` keeps clicks inside it from reaching the backdrop below.
fn panel<'a>(content: Element<'a, Message>) -> Element<'a, Message> {
    opaque(
        container(content)
            .padding(6)
            .width(Fill)
            .style(|theme: &Theme| {
                let t = tokens(theme);
                container::Style {
                    background: Some(Background::Color(t.fly)),
                    border: Border {
                        color: t.line,
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    shadow: Shadow {
                        color: t.shadow,
                        offset: iced::Vector::new(0.0, 8.0),
                        blur_radius: 28.0,
                    },
                    ..Default::default()
                }
            }),
    )
}

fn trigonometry<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let h = SCIENTIFIC_KEY_H;
    let trig = |name: &'static str, hyp: bool| {
        let mut shown = name.to_string();
        if hyp && p.hyp {
            shown.push('h');
        }
        if p.second {
            shown.push_str("⁻¹");
        }
        k(plain(&shown), Key::Trig { name, hyp }, Look::Function, h)
    };
    let hyp_look = if p.hyp { Look::On } else { Look::Function };
    grid(
        vec![
            second_key(p),
            trig("sin", true),
            trig("cos", true),
            trig("tan", true),
            k(plain("hyp"), Key::Hyp, hyp_look, h),
            trig("sec", false),
            trig("csc", false),
            trig("cot", false),
        ],
        4,
    )
}

fn functions<'a>() -> Element<'a, Message> {
    let h = SCIENTIFIC_KEY_H;
    let f = |label: Label, name: &'static str| k(label, Key::Func(name), Look::Function, h);
    grid(
        vec![
            f(lbl(&["|", "x", "|"]), "abs"),
            f(lbl(&["⌊", "x", "⌋"]), "floor"),
            f(lbl(&["⌈", "x", "⌉"]), "ceil"),
            f(plain("round"), "round"),
            f(plain("→deg"), "r2d"),
            f(plain("→rad"), "d2r"),
        ],
        3,
    )
}

fn points<'a>() -> Element<'a, Message> {
    let h = SCIENTIFIC_KEY_H;
    let signature = |name: &'static str, ins: &'static str, sig: &'static str| {
        button(
            column![strong(name, 13.0), muted(sig, 10.0)]
                .spacing(3)
                .align_x(iced::Center)
                .width(Fill),
        )
        .width(Fill)
        .height(SIGNATURE_KEY_H)
        .padding([5, 0])
        .on_press(m(QuickCalcMsg::Key(Key::Text(ins))))
        .style(key_style(Look::Function))
        .into()
    };
    grid(
        vec![
            operator("[", "[", h),
            operator(",", ", ", h),
            operator("]", "]", h),
            signature("dist", "dist(", "(p1, p2)"),
            signature("ang", "ang(", "(p1, p2)"),
            signature("mid", "mid(", "(p1, p2)"),
            signature("xof", "xof(", "(p)"),
            signature("yof", "yof(", "(p)"),
            signature("zof", "zof(", "(p)"),
        ],
        3,
    )
}

fn memory_list<'a>(p: &QuickCalcPalette) -> Element<'a, Message> {
    let rows = p.memory.iter().enumerate().map(|(i, n)| {
        button(container(text(Value::Number(*n).display()).size(20).font(STRONG)).align_right(Fill))
            .width(Fill)
            .padding([10, 12])
            .on_press(m(QuickCalcMsg::MemoryInsert(i)))
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
            })
            .into()
    });
    let clear = tip(
        button(icon(crate::ui::icons::TRASH, 16.0, |t| t.muted))
            .padding(6)
            .on_press(m(QuickCalcMsg::Memory(MemoryOp::Clear)))
            .style(super::view::plain_button_style_borderless),
        crate::t!("Clear all memory").into_owned(),
    );
    column![
        scrollable(column(rows).spacing(2)).spacing(4).height(Fill),
        row![Space::new().width(Fill), clear],
    ]
    .spacing(4)
    .height(Fill)
    .into()
}

pub(super) fn keypad<'a>(p: &'a QuickCalcPalette) -> Element<'a, Message> {
    let scientific_mode = p.keypad_mode == KeypadMode::Scientific;
    let mut body = column![memory_row(p)].spacing(ROW_GAP);
    if scientific_mode {
        body = body.push(flyout_row(p));
    }
    body = body.push(if scientific_mode {
        scientific(p)
    } else {
        standard()
    });
    // No padding of its own: the pallet's gaps above and below match the
    // frame's padding beside it, and its sides line up with the input.
    let base = container(body);
    let Some(flyout) = p.flyout else {
        return base.into();
    };
    // The memory list opens right below the memory row and covers the keys;
    // the menus open right below their own row of buttons. The offsets are
    // the same constants the rows are built with.
    let below_memory = MEMORY_ROW_H + ROW_GAP;
    let below_menus = below_memory + FLYOUT_ROW_H + ROW_GAP;
    let inset = |top: f32, bottom: f32| Padding {
        top,
        bottom,
        ..Padding::ZERO
    };
    let layer: Element<'a, Message> = match flyout {
        Flyout::Memory => container(panel(memory_list(p)))
            .padding(inset(below_memory, 0.0))
            .width(Fill)
            .height(Fill)
            .into(),
        Flyout::Trigonometry | Flyout::Function | Flyout::Point if scientific_mode => {
            let content = match flyout {
                Flyout::Trigonometry => trigonometry(p),
                Flyout::Function => functions(),
                _ => points(),
            };
            container(panel(content))
                .padding(inset(below_menus, 0.0))
                .width(Fill)
                .into()
        }
        _ => return base.into(),
    };
    // A click anywhere else on the keypad closes the flyout.
    let backdrop = mouse_area(Space::new().width(Fill).height(Fill))
        .on_press(m(QuickCalcMsg::CloseFlyout))
        .interaction(mouse::Interaction::Idle);
    stack![base, backdrop, layer].into()
}

/// The keypad body's height as laid out, for the pallet's sections budget.
pub(super) fn height(p: &QuickCalcPalette) -> f32 {
    let keys = |rows: f32, h: f32| rows * h + (rows - 1.0) * GAP;
    let menus = if p.keypad_mode == KeypadMode::Scientific {
        FLYOUT_ROW_H + ROW_GAP + keys(7.0, SCIENTIFIC_KEY_H)
    } else {
        keys(6.0, STANDARD_KEY_H)
    };
    MEMORY_ROW_H + ROW_GAP + menus
}
