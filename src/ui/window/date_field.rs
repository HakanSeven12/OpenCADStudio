//! The date field of plugin dialogs and panels (`DialogControl::Date`, API
//! v33): a text field and a calendar button beside it. The button opens a
//! month calendar under the field; a click on a day writes the date into the
//! field as the plugin's format lays it out, and "Today" writes today's.
//!
//! The field stays a text field: typing works as in any other, and the
//! calendar opens at the month the text names (today's when it names none).
//! Dates are counted here without a date library: the proleptic Gregorian
//! calendar, weeks starting on Monday.

use iced::widget::{button, column, container, row, text, Space};
use iced::{Background, Border, Element, Length, Theme};

use crate::app::Message;
use crate::ui::style::chrome;

/// The format of a date field that names none.
pub const DEFAULT_FORMAT: &str = "YYYY-MM-DD";

/// What a date field's calendar asks for.
#[derive(Debug, Clone, PartialEq)]
pub enum DateMsg {
    /// The calendar button: open the calendar, or close it.
    Toggle,
    /// Show this year and month.
    Month(i32, u32),
    /// A day was picked: the field's new text.
    Pick(String),
    /// Closed without a pick (a click beside it, Escape).
    Close,
}

/// The calendar a dialog or panel shows: the field's id and the year and
/// month on screen.
pub type OpenCalendar = Option<(String, i32, u32)>;

/// A day: year, month 1–12, day 1–31.
pub type Day = (i32, u32, u32);

fn effective(format: &str) -> &str {
    if format.trim().is_empty() {
        DEFAULT_FORMAT
    } else {
        format
    }
}

/// `day` laid out by `format`: `DD` the day and `MM` the month in two
/// digits, `YYYY` the year in four, anything else as written.
pub fn format_date(format: &str, (year, month, day): Day) -> String {
    let format = effective(format);
    let mut out = String::new();
    let mut rest = format;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("YYYY") {
            out.push_str(&format!("{year:04}"));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("MM") {
            out.push_str(&format!("{month:02}"));
            rest = r;
        } else if let Some(r) = rest.strip_prefix("DD") {
            out.push_str(&format!("{day:02}"));
            rest = r;
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    out
}

/// The day `text` names in `format`, if it names a real one. Day and month
/// may have one digit or two, the separators must be those of the format.
pub fn parse_date(format: &str, text: &str) -> Option<Day> {
    let format = effective(format);
    let mut rest_f = format;
    let mut rest_t = text.trim();
    let (mut year, mut month, mut day) = (None, None, None);
    let digits = |s: &str, most: usize| -> Option<(u32, usize)> {
        let n = s
            .chars()
            .take(most)
            .take_while(char::is_ascii_digit)
            .count();
        if n == 0 {
            return None;
        }
        s[..n].parse().ok().map(|v| (v, n))
    };
    while !rest_f.is_empty() {
        if let Some(r) = rest_f.strip_prefix("YYYY") {
            let (v, n) = digits(rest_t, 4).filter(|&(_, n)| n == 4)?;
            year = Some(v as i32);
            rest_t = &rest_t[n..];
            rest_f = r;
        } else if let Some(r) = rest_f.strip_prefix("MM") {
            let (v, n) = digits(rest_t, 2)?;
            month = Some(v);
            rest_t = &rest_t[n..];
            rest_f = r;
        } else if let Some(r) = rest_f.strip_prefix("DD") {
            let (v, n) = digits(rest_t, 2)?;
            day = Some(v);
            rest_t = &rest_t[n..];
            rest_f = r;
        } else {
            let c = rest_f.chars().next().unwrap();
            rest_t = rest_t.strip_prefix(c)?;
            rest_f = &rest_f[c.len_utf8()..];
        }
    }
    let (year, month, day) = (year?, month?, day?);
    (rest_t.is_empty()
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day))
    .then_some((year, month, day))
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days in `month` of `year`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01 to `day` (H. Hinnant's `days_from_civil`).
fn days_from_civil((year, month, day): Day) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The day `days` after 1970-01-01 (`civil_from_days`).
fn civil_from_days(days: i64) -> Day {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

/// The weekday of `day`, 0 = Monday.
pub fn weekday(day: Day) -> u32 {
    // 1970-01-01 was a Thursday.
    (days_from_civil(day) + 3).rem_euclid(7) as u32
}

/// The month `delta` months after `month` of `year`.
pub fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let index = year * 12 + month as i32 - 1 + delta;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

/// Today in the local time zone.
pub fn local_today() -> Day {
    #[cfg(target_arch = "wasm32")]
    {
        let now = js_sys::Date::new_0();
        (
            now.get_full_year() as i32,
            now.get_month() + 1,
            now.get_date(),
        )
    }
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let now = secs as libc::time_t;
        // SAFETY: `localtime_r` only writes the `tm` it is given.
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        if !unsafe { libc::localtime_r(&now, &mut tm) }.is_null() {
            return (tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32);
        }
        civil_from_days((secs / 86_400) as i64)
    }
    #[cfg(not(any(unix, target_arch = "wasm32")))]
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let local = secs / 86_400.0 + crate::entities::field::utc_offset_days();
        civil_from_days(local.floor() as i64)
    }
}

/// The year and month a calendar opens at for a field showing `text`.
pub fn opening_month(format: &str, text: &str) -> (i32, u32) {
    let (year, month, _) = parse_date(format, text).unwrap_or_else(local_today);
    (year, month)
}

/// The format of date field `id` among `controls`, searching rows, columns
/// and field groups.
pub fn date_format(controls: &[ocs_plugin_api::dialog::DialogControl], id: &str) -> Option<String> {
    use ocs_plugin_api::dialog::DialogControl;
    controls.iter().find_map(|c| match c {
        DialogControl::Date {
            id: cid, format, ..
        } if cid == id => Some(format.clone()),
        DialogControl::Fields { controls, .. }
        | DialogControl::Row { controls, .. }
        | DialogControl::Column { controls, .. } => date_format(controls, id),
        _ => None,
    })
}

/// One step of date field `id`'s calendar in a dialog or panel whose open
/// calendar is `open` and whose field texts are `texts`. Returns the text of
/// a picked day, which is then in the field.
pub fn step(
    open: &mut OpenCalendar,
    texts: &mut std::collections::HashMap<String, String>,
    controls: &[ocs_plugin_api::dialog::DialogControl],
    id: &str,
    msg: DateMsg,
) -> Option<String> {
    let is_open = open.as_ref().is_some_and(|(o, _, _)| o == id);
    match msg {
        DateMsg::Toggle if is_open => *open = None,
        DateMsg::Toggle => {
            let format = date_format(controls, id).unwrap_or_default();
            let text = texts.get(id).map(String::as_str).unwrap_or("");
            let (year, month) = opening_month(&format, text);
            *open = Some((id.to_string(), year, month));
        }
        DateMsg::Month(year, month) if is_open => *open = Some((id.to_string(), year, month)),
        DateMsg::Month(..) => {}
        DateMsg::Pick(text) => {
            *open = None;
            texts.insert(id.to_string(), text.clone());
            return Some(text);
        }
        DateMsg::Close if is_open => *open = None,
        DateMsg::Close => {}
    }
    None
}

fn month_name(month: u32) -> String {
    match month {
        1 => crate::tr!("plugin-dialog", "month-1"),
        2 => crate::tr!("plugin-dialog", "month-2"),
        3 => crate::tr!("plugin-dialog", "month-3"),
        4 => crate::tr!("plugin-dialog", "month-4"),
        5 => crate::tr!("plugin-dialog", "month-5"),
        6 => crate::tr!("plugin-dialog", "month-6"),
        7 => crate::tr!("plugin-dialog", "month-7"),
        8 => crate::tr!("plugin-dialog", "month-8"),
        9 => crate::tr!("plugin-dialog", "month-9"),
        10 => crate::tr!("plugin-dialog", "month-10"),
        11 => crate::tr!("plugin-dialog", "month-11"),
        _ => crate::tr!("plugin-dialog", "month-12"),
    }
}

/// The short name of weekday `index`, 0 = Monday.
fn weekday_name(index: u32) -> String {
    match index {
        0 => crate::tr!("plugin-dialog", "weekday-1"),
        1 => crate::tr!("plugin-dialog", "weekday-2"),
        2 => crate::tr!("plugin-dialog", "weekday-3"),
        3 => crate::tr!("plugin-dialog", "weekday-4"),
        4 => crate::tr!("plugin-dialog", "weekday-5"),
        5 => crate::tr!("plugin-dialog", "weekday-6"),
        _ => crate::tr!("plugin-dialog", "weekday-7"),
    }
}

const CALENDAR_SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="#000" stroke-width="1.2" stroke-linejoin="round" stroke-linecap="round"><rect x="2" y="3" width="12" height="11" rx="1.2"/><path d="M2 6.5h12M5 1.6v2.8M11 1.6v2.8"/><path d="M4.8 9h1.2M7.4 9h1.2M10 9h1.2M4.8 11.5h1.2M7.4 11.5h1.2" stroke-width="1.4"/></svg>"##;

const CELL_W: f32 = 30.0;
const CELL_H: f32 = 24.0;
/// The calendar's width: seven days and its padding.
pub const CALENDAR_W: f32 = CELL_W * 7.0 + 16.0;

/// The calendar button beside a date field.
pub fn calendar_button<'a>(
    disabled: bool,
    on_date: impl Fn(DateMsg) -> Message,
) -> Element<'a, Message> {
    let icon = crate::ui::icons::tinted(CALENDAR_SVG, 14.0, chrome::text);
    let b = button(container(icon).center(Length::Fill))
        .width(Length::Fixed(26.0))
        .height(Length::Fixed(24.0))
        .padding(0)
        .style(crate::ui::style::form::button_style(false))
        .on_press_maybe((!disabled).then(|| on_date(DateMsg::Toggle)));
    crate::ui::kit::tooltip(
        b,
        crate::tr!("plugin-dialog", "calendar"),
        crate::ui::kit::TipPosition::Top,
    )
}

/// The month calendar of a field showing `current` in `format`, at `shown`
/// (year, month).
pub fn calendar<'a>(
    format: &str,
    current: &str,
    shown: (i32, u32),
    on_date: impl Fn(DateMsg) -> Message,
) -> Element<'a, Message> {
    let (year, month) = shown;
    let picked = parse_date(format, current);
    let today = local_today();
    let nav = |label: &'static str, tip: String, target: (i32, u32)| -> Element<'a, Message> {
        let b = button(container(text(label).size(14)).center(Length::Fill))
            .width(Length::Fixed(CELL_W))
            .height(Length::Fixed(CELL_H))
            .padding(0)
            .style(crate::ui::kit::dropdown_row_style)
            .on_press(on_date(DateMsg::Month(target.0, target.1)));
        crate::ui::kit::tooltip(b, tip, crate::ui::kit::TipPosition::Top)
    };
    let header = row![
        nav(
            "‹",
            crate::tr!("plugin-dialog", "previous-month"),
            shift_month(year, month, -1)
        ),
        container(
            text(crate::i18n::plain(crate::tr!(
                "plugin-dialog",
                "month-year",
                month = month_name(month),
                year = year.to_string()
            )))
            .size(12)
        )
        .center_x(Length::Fill)
        .center_y(Length::Fixed(CELL_H)),
        nav(
            "›",
            crate::tr!("plugin-dialog", "next-month"),
            shift_month(year, month, 1)
        ),
    ]
    .align_y(iced::Alignment::Center);
    let mut names = row![];
    for index in 0..7 {
        names = names.push(
            container(
                text(weekday_name(index))
                    .size(10)
                    .style(|theme: &Theme| text::Style {
                        color: Some(chrome::muted(theme)),
                    }),
            )
            .center_x(Length::Fixed(CELL_W))
            .center_y(Length::Fixed(18.0)),
        );
    }
    let first = weekday((year, month, 1)) as i64;
    let days = i64::from(days_in_month(year, month));
    let weeks = (first + days + 6) / 7;
    let mut grid = column![];
    for week in 0..weeks {
        let mut line = row![];
        for column in 0..7 {
            let n = week * 7 + column - first + 1;
            if !(1..=days).contains(&n) {
                line = line.push(Space::new().width(CELL_W).height(CELL_H));
                continue;
            }
            let day = (year, month, n as u32);
            let is_picked = picked == Some(day);
            let is_today = today == day;
            let label = text(n.to_string()).size(12);
            line = line.push(
                button(container(label).center(Length::Fill))
                    .width(Length::Fixed(CELL_W))
                    .height(Length::Fixed(CELL_H))
                    .padding(0)
                    .style(day_style(is_picked, is_today))
                    .on_press(on_date(DateMsg::Pick(format_date(format, day)))),
            );
        }
        grid = grid.push(line);
    }
    let today_button = button(text(crate::tr!("plugin-dialog", "today")).size(12))
        .padding([3, 10])
        .style(crate::ui::style::form::button_style(false))
        .on_press(on_date(DateMsg::Pick(format_date(format, today))));
    container(
        column![
            header,
            names,
            grid,
            container(today_button).center_x(Length::Fill)
        ]
        .spacing(4),
    )
    .padding(8)
    .width(Length::Fixed(CALENDAR_W))
    .style(crate::ui::kit::menu::frame_style)
    .into()
}

/// A day: the picked one filled in the accent colour, today framed in it,
/// every day lit under the pointer.
fn day_style(picked: bool, today: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let accent = chrome::accent(theme);
        button::Style {
            background: if picked {
                Some(Background::Color(accent))
            } else if hovered {
                Some(Background::Color(chrome::hover(theme)))
            } else {
                None
            },
            text_color: if picked {
                iced::Color::WHITE
            } else if hovered {
                chrome::strong(theme)
            } else {
                chrome::text(theme)
            },
            border: Border {
                color: if today && !picked {
                    accent
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if today && !picked { 1.0 } else { 0.0 },
                radius: 3.0.into(),
            },
            ..Default::default()
        }
    }
}

/// `field` (the text input and the calendar button) with the calendar
/// under it while `shown` is `Some`. The widget tree keeps its shape either
/// way, so the text input keeps its focus.
pub fn with_calendar<'a>(
    field: Element<'a, Message>,
    format: &str,
    current: &str,
    shown: Option<(i32, u32)>,
    on_date: impl Fn(DateMsg) -> Message,
) -> Element<'a, Message> {
    let popup = calendar(format, current, shown.unwrap_or((2000, 1)), &on_date);
    crate::ui::color_select::editable_drop_down_below(
        field,
        popup,
        Some(Length::Fixed(CALENDAR_W)),
        Length::Shrink,
        on_date(DateMsg::Close),
        shown.is_some(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_is_laid_out_and_read_back_in_the_plugins_format() {
        assert_eq!(format_date("DD.MM.YYYY", (2026, 11, 3)), "03.11.2026");
        assert_eq!(format_date("", (2026, 11, 3)), "2026-11-03");
        assert_eq!(format_date("MM/DD/YYYY", (2026, 1, 31)), "01/31/2026");
        assert_eq!(parse_date("DD.MM.YYYY", "3.11.2026"), Some((2026, 11, 3)));
        assert_eq!(
            parse_date("DD.MM.YYYY", " 26.11.2026 "),
            Some((2026, 11, 26))
        );
        assert_eq!(parse_date("", "2024-02-29"), Some((2024, 2, 29)));
        // Not a day, or not in the format.
        assert_eq!(parse_date("", "2023-02-29"), None);
        assert_eq!(parse_date("DD.MM.YYYY", "26.13.2026"), None);
        assert_eq!(parse_date("DD.MM.YYYY", "2026-11-26"), None);
        assert_eq!(parse_date("DD.MM.YYYY", "26.11.26"), None);
        assert_eq!(parse_date("DD.MM.YYYY", ""), None);
    }

    #[test]
    fn weekdays_month_lengths_and_month_steps() {
        assert_eq!(weekday((2026, 11, 26)), 3, "a Thursday");
        assert_eq!(weekday((2026, 11, 1)), 6, "a Sunday");
        assert_eq!(weekday((2000, 1, 1)), 5, "a Saturday");
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        for days in [-1, 0, 19_000, 20_418] {
            assert_eq!(days_from_civil(civil_from_days(days)), days);
        }
        assert_eq!(civil_from_days(20_418), (2025, 11, 26));
    }

    #[test]
    fn the_calendar_opens_at_the_month_the_field_names() {
        assert_eq!(opening_month("DD.MM.YYYY", "05.03.2027"), (2027, 3));
        let (year, month, _) = local_today();
        assert_eq!(opening_month("DD.MM.YYYY", "soon"), (year, month));
    }
}
