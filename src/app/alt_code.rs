//! Windows Alt codes: hold Alt, type a number on the numpad, release Alt
//! (Alt+0178 = ², Alt+253 = ²). Windows sends the character after the Alt
//! release, and winit drops a character that follows a key release (#3317),
//! so the code is read here from the key events and the character is typed
//! back as a Unicode key press, which reaches whichever field has focus.

use iced::keyboard::{self, key::Code, key::Named, key::Physical, Key};

/// The numpad digits typed since Alt went down; `None` when no code is
/// being typed.
#[derive(Debug, Default)]
pub(crate) struct AltCode {
    digits: Option<String>,
}

impl AltCode {
    /// Feed one key event; returns the digits of a finished code when Alt
    /// is released after them.
    pub(crate) fn feed(&mut self, _event: &keyboard::Event) -> Option<String> {
        None
    }
}

/// The character of Alt code `digits`: a leading 0 reads it in the ANSI
/// code page `ansi`, otherwise in the OEM code page `oem`.
pub(crate) fn decode(_digits: &str, _ansi: u32, _oem: u32) -> Option<char> {
    None
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
        assert_eq!(type_code(&mut alt, "253", Modifiers::ALT).as_deref(), Some("253"));
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
        alt.feed(&press(Key::Character("1".into()), Code::Numpad1, Modifiers::ALT));
        alt.feed(&press(Key::Character("f".into()), Code::KeyF, Modifiers::ALT));
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
