//! Import Text: a text (.txt) or Rich Text (.rtf) file put in at the caret.
//!
//! A text file is read as UTF-8 or UTF-16 when it starts with a byte order
//! mark and in the system's ANSI code page otherwise; each line break is a
//! paragraph break, a tab stays a tab, and `\`, `{` and `}` are written as
//! `\\`, `\{` and `\}`. A Rich Text file keeps its paragraphs, tabs, bold
//! and italic (as the font's `|b1`/`|i1`), underline (`\L`), colours (an
//! index colour where the RGB is one, else a true colour), fonts (with
//! their character set and pitch) and sizes relative to the first one
//! (`\H2x;`).

/// The Windows character set of the system's ANSI code page (Turkish 162,
/// Cyrillic 204, …); 0 (Western) elsewhere.
pub(crate) fn system_charset() -> u8 {
    match ansi_code_page() {
        1250 => 238,
        1251 => 204,
        1253 => 161,
        1254 => 162,
        1255 => 177,
        1256 => 178,
        1257 => 186,
        1258 => 163,
        874 => 222,
        932 => 128,
        936 => 134,
        949 => 129,
        950 => 136,
        _ => 0,
    }
}

fn ansi_code_page() -> u32 {
    #[cfg(windows)]
    {
        // SAFETY: GetACP takes no arguments and only reads the system setting.
        unsafe { windows_sys::Win32::Globalization::GetACP() }
    }
    #[cfg(not(windows))]
    {
        1252
    }
}

/// The encoding of a Windows code page.
fn code_page_encoding(page: u32) -> &'static encoding_rs::Encoding {
    let label = match page {
        874 => "windows-874",
        932 => "shift_jis",
        936 => "gbk",
        949 => "euc-kr",
        950 => "big5",
        1250..=1258 => return encoding_rs::Encoding::for_label(format!("windows-{page}").as_bytes())
            .unwrap_or(encoding_rs::WINDOWS_1252),
        _ => "windows-1252",
    };
    encoding_rs::Encoding::for_label(label.as_bytes()).unwrap_or(encoding_rs::WINDOWS_1252)
}

/// The text of a text file's bytes.
fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return encoding_rs::UTF_16LE.decode_without_bom_handling(rest).0.into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return encoding_rs::UTF_16BE.decode_without_bom_handling(rest).0.into_owned();
    }
    code_page_encoding(ansi_code_page())
        .decode_without_bom_handling(bytes)
        .0
        .into_owned()
}

/// Plain text as MTEXT source.
fn escape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\P");
            }
            '\n' => out.push_str("\\P"),
            c if c.is_control() && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// The MTEXT source of a text or Rich Text file's bytes, by its extension.
pub(crate) fn file_source(path: &std::path::Path, bytes: &[u8]) -> String {
    let rtf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("rtf"))
        || bytes.starts_with(b"{\\rtf");
    if rtf {
        rtf_source(bytes)
    } else {
        escape(&decode(bytes))
    }
}

/// One RTF group's character state.
#[derive(Clone, Debug, Default, PartialEq)]
struct RtfState {
    font: usize,
    size: u32,
    bold: bool,
    italic: bool,
    underline: bool,
    color: usize,
    /// `\ucN`: characters skipped after a `\u`.
    skip: usize,
    /// A destination whose text is not shown (font table, colour table,
    /// pictures, `\*` groups).
    hidden: bool,
}

/// The ACI index of an exact RGB of the first seven colours, else none.
fn aci_of(rgb: (u8, u8, u8)) -> Option<u16> {
    match rgb {
        (255, 0, 0) => Some(1),
        (255, 255, 0) => Some(2),
        (0, 255, 0) => Some(3),
        (0, 255, 255) => Some(4),
        (0, 0, 255) => Some(5),
        (255, 0, 255) => Some(6),
        (255, 255, 255) | (0, 0, 0) => Some(7),
        _ => None,
    }
}

struct RtfFont {
    name: String,
    charset: Option<u16>,
    pitch: u8,
}

/// A Rich Text file's text as MTEXT source.
fn rtf_source(bytes: &[u8]) -> String {
    let text: String = bytes.iter().map(|&b| char::from(b)).collect();
    let chars: Vec<char> = text.chars().collect();
    let mut fonts: Vec<(usize, RtfFont)> = Vec::new();
    let mut colors: Vec<Option<(u8, u8, u8)>> = Vec::new();
    let mut stack: Vec<RtfState> = vec![RtfState {
        skip: 1,
        ..Default::default()
    }];
    let mut out = String::new();
    // What the output is in now, written before each character as it changes.
    let mut written: Option<RtfState> = None;
    let mut base_size: Option<u32> = None;
    let mut ansi = code_page_encoding(ansi_code_page());
    let mut pending_skip = 0usize;
    let mut i = 0;
    // Font table and colour table parsing state.
    let mut table: Option<&'static str> = None;
    let mut font_entry: Option<(usize, RtfFont)> = None;
    let mut rgb = (0u8, 0u8, 0u8);
    let mut rgb_set = false;

    let emit = |out: &mut String, written: &mut Option<RtfState>, state: &RtfState, base: Option<u32>, c: &str, fonts: &[(usize, RtfFont)], colors: &[Option<(u8, u8, u8)>]| {
        if written.as_ref() != Some(state) {
            if written.is_some() {
                out.push('}');
            }
            out.push('{');
            if let Some((_, font)) = fonts.iter().find(|(n, _)| *n == state.font) {
                out.push_str(&format!(
                    "\\f{}|b{}|i{}|c{}|p{};",
                    font.name.trim(),
                    u8::from(state.bold),
                    u8::from(state.italic),
                    font.charset.unwrap_or_else(|| u16::from(system_charset())),
                    if font.pitch == 0 {
                        super::raw_tools::pitch_of(font.name.trim())
                    } else {
                        font.pitch
                    }
                ));
            }
            if state.underline {
                out.push_str("\\L");
            }
            if let Some(Some(rgb)) = colors.get(state.color).filter(|_| state.color > 0) {
                match aci_of(*rgb) {
                    Some(n) => out.push_str(&format!("\\C{n};")),
                    None => out.push_str(&format!(
                        "\\c{};",
                        u32::from(rgb.0) | (u32::from(rgb.1) << 8) | (u32::from(rgb.2) << 16)
                    )),
                }
            }
            if let (Some(base), size) = (base, state.size) {
                if size > 0 && size != base {
                    let factor = f64::from(size) / f64::from(base);
                    out.push_str(&format!(
                        "\\H{}x;",
                        super::raw_span::number((factor * 1e6).round() / 1e6)
                    ));
                }
            }
            *written = Some(state.clone());
        }
        out.push_str(c);
    };

    while i < chars.len() {
        let c = chars[i];
        match c {
            '{' => {
                let mut next = stack.last().cloned().unwrap_or_default();
                if chars.get(i + 1) == Some(&'\\') && chars.get(i + 2) == Some(&'*') {
                    next.hidden = true;
                }
                stack.push(next);
                i += 1;
            }
            '}' => {
                if let Some((n, font)) = font_entry.take() {
                    fonts.push((n, font));
                }
                stack.pop();
                if stack.is_empty() {
                    break;
                }
                if table.is_some() && !stack.last().is_some_and(|s| s.hidden) {
                    table = None;
                }
                i += 1;
            }
            '\\' => {
                let next = chars.get(i + 1).copied().unwrap_or(' ');
                if matches!(next, '\\' | '{' | '}') {
                    let state = stack.last().cloned().unwrap_or_default();
                    if !state.hidden {
                        let s = escape(&next.to_string());
                        emit(&mut out, &mut written, &state, base_size, &s, &fonts, &colors);
                    }
                    i += 2;
                    continue;
                }
                if next == '\'' {
                    let hex: String = chars.iter().skip(i + 2).take(2).collect();
                    i += 4;
                    if pending_skip > 0 {
                        pending_skip -= 1;
                        continue;
                    }
                    let state = stack.last().cloned().unwrap_or_default();
                    if let (Ok(byte), false) = (u8::from_str_radix(&hex, 16), state.hidden) {
                        let decoded = ansi.decode_without_bom_handling(&[byte]).0.into_owned();
                        if let Some(font) = font_entry.as_mut() {
                            font.1.name.push_str(&decoded);
                        } else {
                            emit(&mut out, &mut written, &state, base_size, &escape(&decoded), &fonts, &colors);
                        }
                    }
                    continue;
                }
                // A control word and its number.
                let mut k = i + 1;
                while k < chars.len() && chars[k].is_ascii_alphabetic() {
                    k += 1;
                }
                let word: String = chars[i + 1..k].iter().collect();
                let mut m = k;
                if m < chars.len() && (chars[m] == '-' || chars[m].is_ascii_digit()) {
                    m += 1;
                    while m < chars.len() && chars[m].is_ascii_digit() {
                        m += 1;
                    }
                }
                let number: Option<i64> = chars[k..m].iter().collect::<String>().parse().ok();
                if m < chars.len() && chars[m] == ' ' {
                    m += 1;
                }
                if word.is_empty() {
                    // `\*` and other symbols.
                    i = k + 1;
                    continue;
                }
                i = m;
                let state = stack.last_mut().expect("rtf group");
                match word.as_str() {
                    "fonttbl" => {
                        table = Some("font");
                        state.hidden = true;
                    }
                    "colortbl" => {
                        table = Some("color");
                        state.hidden = true;
                    }
                    "stylesheet" | "info" | "pict" | "header" | "footer" | "listtable"
                    | "listoverridetable" | "generator" => state.hidden = true,
                    "ansicpg" => {
                        if let Some(n) = number {
                            ansi = code_page_encoding(n as u32);
                        }
                    }
                    "f" if table == Some("font") => {
                        font_entry = Some((
                            number.unwrap_or(0) as usize,
                            RtfFont {
                                name: String::new(),
                                charset: None,
                                pitch: 0,
                            },
                        ));
                    }
                    "froman" if table == Some("font") => {
                        if let Some(f) = font_entry.as_mut() {
                            f.1.pitch = 18;
                        }
                    }
                    "fswiss" if table == Some("font") => {
                        if let Some(f) = font_entry.as_mut() {
                            f.1.pitch = 34;
                        }
                    }
                    "fcharset" => {
                        if let Some(f) = font_entry.as_mut() {
                            f.1.charset = number.map(|n| n as u16);
                        }
                    }
                    "red" => {
                        rgb.0 = number.unwrap_or(0) as u8;
                        rgb_set = true;
                    }
                    "green" => {
                        rgb.1 = number.unwrap_or(0) as u8;
                        rgb_set = true;
                    }
                    "blue" => {
                        rgb.2 = number.unwrap_or(0) as u8;
                        rgb_set = true;
                    }
                    "f" => state.font = number.unwrap_or(0) as usize,
                    "fs" => {
                        state.size = number.unwrap_or(24) as u32;
                        base_size.get_or_insert(state.size);
                    }
                    "b" => state.bold = number != Some(0),
                    "i" => state.italic = number != Some(0),
                    "ul" => state.underline = number != Some(0),
                    "ulnone" => state.underline = false,
                    "cf" => state.color = number.unwrap_or(0) as usize,
                    "plain" => {
                        state.bold = false;
                        state.italic = false;
                        state.underline = false;
                        state.color = 0;
                    }
                    "uc" => state.skip = number.unwrap_or(1) as usize,
                    "u" => {
                        let state = state.clone();
                        if let Some(c) = number
                            .map(|n| if n < 0 { n + 65536 } else { n })
                            .and_then(|n| char::from_u32(n as u32))
                        {
                            if !state.hidden {
                                emit(&mut out, &mut written, &state, base_size, &escape(&c.to_string()), &fonts, &colors);
                            }
                        }
                        pending_skip = state.skip;
                    }
                    "par" | "line" => {
                        let state = state.clone();
                        if !state.hidden {
                            emit(&mut out, &mut written, &state, base_size, "\\P", &fonts, &colors);
                        }
                    }
                    "tab" => {
                        let state = state.clone();
                        if !state.hidden {
                            emit(&mut out, &mut written, &state, base_size, "\t", &fonts, &colors);
                        }
                    }
                    _ => {}
                }
            }
            '\r' | '\n' => i += 1,
            ';' if table == Some("color") => {
                colors.push(rgb_set.then_some(rgb));
                rgb = (0, 0, 0);
                rgb_set = false;
                i += 1;
            }
            ';' if table == Some("font") => {
                if let Some(f) = font_entry.take() {
                    fonts.push(f);
                }
                i += 1;
            }
            c => {
                i += 1;
                if pending_skip > 0 {
                    pending_skip -= 1;
                    continue;
                }
                if let Some(font) = font_entry.as_mut() {
                    font.1.name.push(c);
                    continue;
                }
                let state = stack.last().cloned().unwrap_or_default();
                if !state.hidden {
                    emit(&mut out, &mut written, &state, base_size, &escape(&c.to_string()), &fonts, &colors);
                }
            }
        }
    }
    if written.is_some() {
        out.push('}');
    }
    out
}
