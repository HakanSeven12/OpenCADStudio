//! Windows Alt codes: hold Alt, type a number on the numpad, release Alt
//! (Alt+0178 = ², Alt+253 = ²). Windows sends the character after the Alt
//! release, and winit drops a character that follows a key release (#3317),
//! so the code is read here from the key events and the character is typed
//! back as a Unicode key press, which reaches whichever field has focus.

use iced::keyboard::{self, Key, key::Code, key::Named, key::Physical};

/// The numpad digits typed since Alt went down; `None` when no code is
/// being typed.
#[derive(Debug, Default)]
pub(crate) struct AltCode {
    digits: Option<String>,
}

impl AltCode {
    /// Feed one key event; returns the digits of a finished code when Alt
    /// is released after them.
    pub(crate) fn feed(&mut self, event: &keyboard::Event) -> Option<String> {
        match event {
            keyboard::Event::KeyPressed {
                key: Key::Named(Named::Alt),
                modifiers,
                repeat,
                ..
            } => {
                if !*repeat {
                    // Ctrl+Alt is AltGr, which has no Alt codes.
                    self.digits = (!modifiers.control()).then(String::new);
                }
                None
            }
            keyboard::Event::KeyPressed {
                physical_key,
                modifiers,
                ..
            } => {
                let digit =
                    numpad_digit(physical_key).filter(|_| modifiers.alt() && !modifiers.control());
                match (digit, self.digits.as_mut()) {
                    (Some(d), Some(digits)) => digits.push(d),
                    _ => self.digits = None,
                }
                None
            }
            keyboard::Event::KeyReleased {
                key: Key::Named(Named::Alt),
                ..
            } => self.digits.take().filter(|d| !d.is_empty()),
            _ => None,
        }
    }
}

/// The digit of a numpad key, read from the physical key so it counts
/// with NumLock off too.
fn numpad_digit(key: &Physical) -> Option<char> {
    let Physical::Code(code) = key else {
        return None;
    };
    let n = match code {
        Code::Numpad0 => 0,
        Code::Numpad1 => 1,
        Code::Numpad2 => 2,
        Code::Numpad3 => 3,
        Code::Numpad4 => 4,
        Code::Numpad5 => 5,
        Code::Numpad6 => 6,
        Code::Numpad7 => 7,
        Code::Numpad8 => 8,
        Code::Numpad9 => 9,
        _ => return None,
    };
    char::from_digit(n, 10)
}

/// The character of Alt code `digits`: a leading 0 reads it in the ANSI
/// code page `ansi`, otherwise in the OEM code page `oem`.
pub(crate) fn decode(digits: &str, ansi: u32, oem: u32) -> Option<char> {
    use windows_sys::Win32::Globalization::{MB_USEGLYPHCHARS, MultiByteToWideChar};

    // Windows keeps the code modulo 256.
    let byte = (digits.parse::<u32>().ok()? % 256) as u8;
    if byte == 0 {
        return None;
    }
    // MB_USEGLYPHCHARS: OEM 1-31 are glyphs (☺, ♥), as Windows types them.
    let (page, flags) = if digits.starts_with('0') {
        (ansi, 0)
    } else {
        (oem, MB_USEGLYPHCHARS)
    };
    let mut wide = [0u16; 2];
    // SAFETY: one input byte, output buffer of 2 units, lengths passed.
    let n = unsafe { MultiByteToWideChar(page, flags, &byte, 1, wide.as_mut_ptr(), 2) };
    char::decode_utf16(wide[..n.max(0) as usize].iter().copied())
        .next()?
        .ok()
        // U+FFFD: the byte has no character, as with ANSI page 65001 (UTF-8).
        .filter(|c| !c.is_control() && *c != char::REPLACEMENT_CHARACTER)
}

/// Type `ch` into the focused window as a Unicode key press.
fn type_char(ch: char) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput,
    };

    let mut buf = [0u16; 2];
    let inputs: Vec<INPUT> = ch
        .encode_utf16(&mut buf)
        .iter()
        .flat_map(|&unit| {
            [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP].map(|flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: 0,
                        wScan: unit,
                        dwFlags: flags,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            })
        })
        .collect();
    // SAFETY: a valid slice of keyboard inputs and its element size.
    unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );
    }
}

/// Watch the window's key events and type the character of each finished
/// Alt code.
pub(crate) fn observe(event: &keyboard::Event) {
    use std::sync::Mutex;
    use windows_sys::Win32::Globalization::{GetACP, GetOEMCP};

    static STATE: Mutex<AltCode> = Mutex::new(AltCode { digits: None });
    let Some(digits) = STATE.lock().ok().and_then(|mut s| s.feed(event)) else {
        return;
    };
    // shortcut: Windows reads Alt+0nnn in the keyboard layout's ANSI page, not
    // the system one; differs only when they disagree, read the layout if reported.
    // SAFETY: plain getters with no arguments.
    let (ansi, oem) = unsafe { (GetACP(), GetOEMCP()) };
    if let Some(ch) = decode(&digits, ansi, oem) {
        type_char(ch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::{Location, Modifiers};

    fn press(key: Key, physical: Code, modifiers: Modifiers) -> keyboard::Event {
        keyboard::Event::KeyPressed {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Code(physical),
            location: Location::Standard,
            modifiers,
            text: None,
            repeat: false,
        }
    }

    fn release(key: Key, physical: Code, modifiers: Modifiers) -> keyboard::Event {
        keyboard::Event::KeyReleased {
            key: key.clone(),
            modified_key: key,
            physical_key: Physical::Code(physical),
            location: Location::Standard,
            modifiers,
        }
    }

    fn alt_down() -> keyboard::Event {
        press(Key::Named(Named::Alt), Code::AltLeft, Modifiers::ALT)
    }

    fn alt_up() -> keyboard::Event {
        release(Key::Named(Named::Alt), Code::AltLeft, Modifiers::empty())
    }

    fn numpad(digit: char) -> Code {
        match digit {
            '0' => Code::Numpad0,
            '1' => Code::Numpad1,
            '2' => Code::Numpad2,
            '3' => Code::Numpad3,
            '4' => Code::Numpad4,
            '5' => Code::Numpad5,
            '6' => Code::Numpad6,
            '7' => Code::Numpad7,
            '8' => Code::Numpad8,
            _ => Code::Numpad9,
        }
    }

    /// Alt held, `digits` typed on the numpad (each pressed and released),
    /// then `modifiers` as they stand when a digit goes down.
    fn type_code(alt: &mut AltCode, digits: &str, modifiers: Modifiers) -> Option<String> {
        let mut out = alt.feed(&alt_down());
        for digit in digits.chars() {
            let key = Key::Character(digit.to_string().into());
            out = out.or(alt.feed(&press(key.clone(), numpad(digit), modifiers)));
            out = out.or(alt.feed(&release(key, numpad(digit), modifiers)));
        }
        out.or(alt.feed(&alt_up()))
    }

    #[test]
    fn alt_and_numpad_digits_give_the_code_on_alt_release() {
        let mut alt = AltCode::default();
        assert_eq!(
            type_code(&mut alt, "0178", Modifiers::ALT).as_deref(),
            Some("0178")
        );
        // The next code starts afresh.
        assert_eq!(
            type_code(&mut alt, "253", Modifiers::ALT).as_deref(),
            Some("253")
        );
    }

    #[test]
    fn numpad_keys_count_with_num_lock_off() {
        // NumLock off: Numpad2 arrives as ArrowDown, its physical key stays.
        let mut alt = AltCode::default();
        alt.feed(&alt_down());
        let down = Key::Named(Named::ArrowDown);
        alt.feed(&press(down.clone(), Code::Numpad2, Modifiers::ALT));
        alt.feed(&release(down, Code::Numpad2, Modifiers::ALT));
        assert_eq!(alt.feed(&alt_up()).as_deref(), Some("2"));
    }

    #[test]
    fn a_lone_alt_tap_gives_no_code() {
        let mut alt = AltCode::default();
        alt.feed(&alt_down());
        assert_eq!(alt.feed(&alt_up()), None);
    }

    #[test]
    fn another_key_while_alt_is_held_cancels_the_code() {
        let mut alt = AltCode::default();
        alt.feed(&alt_down());
        alt.feed(&press(
            Key::Character("1".into()),
            Code::Numpad1,
            Modifiers::ALT,
        ));
        alt.feed(&press(
            Key::Character("f".into()),
            Code::KeyF,
            Modifiers::ALT,
        ));
        assert_eq!(alt.feed(&alt_up()), None);
    }

    #[test]
    fn altgr_with_numpad_digits_gives_no_code() {
        // AltGr reaches Windows apps as Ctrl+Alt.
        let mut alt = AltCode::default();
        let ctrl_alt = Modifiers::ALT | Modifiers::CTRL;
        assert_eq!(type_code(&mut alt, "65", ctrl_alt), None);
    }

    #[test]
    fn a_leading_zero_reads_the_ansi_code_page() {
        assert_eq!(decode("0178", 1252, 437), Some('²'));
        assert_eq!(decode("0128", 1252, 437), Some('€'));
        // Turkish ANSI.
        assert_eq!(decode("0253", 1254, 857), Some('ı'));
    }

    #[test]
    fn no_leading_zero_reads_the_oem_code_page() {
        assert_eq!(decode("253", 1252, 437), Some('²'));
        assert_eq!(decode("178", 1252, 437), Some('▓'));
        assert_eq!(decode("65", 1252, 437), Some('A'));
        // Above 255 wraps, as in Windows: 321 = 65.
        assert_eq!(decode("321", 1252, 437), Some('A'));
    }

    #[test]
    fn a_code_that_types_nothing_gives_no_character() {
        assert_eq!(decode("0", 1252, 437), None);
        assert_eq!(decode("256", 1252, 437), None);
        // Alt+09 would be a tab: not typed.
        assert_eq!(decode("09", 1252, 437), None);
    }
}
