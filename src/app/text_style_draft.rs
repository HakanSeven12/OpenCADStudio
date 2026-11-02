//! The text style manager's working copy of the selected style.
//!
//! The fields show rounded numbers, so writing them back as typed would round
//! every value of a style the user only looked at. The draft keeps the style's
//! exact values and the text each number field showed when it was loaded; only
//! a field whose text the user changed is parsed and written (#2413).

use crate::entities::text_support::{TTF_FLAG_BOLD, TTF_FLAG_ITALIC};
use codec::tables::TextStyle;
use std::borrow::Cow;

/// A number field: the text shown when the style was loaded, the text in the
/// field now, and the values it accepts.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct NumberField {
    shown: String,
    pub(super) text: String,
    min: f64,
    max: f64,
}

impl NumberField {
    fn new(shown: String, min: f64, max: f64) -> Self {
        Self {
            text: shown.clone(),
            shown,
            min,
            max,
        }
    }

    pub(super) fn is_modified(&self) -> bool {
        self.text.trim() != self.shown.trim()
    }

    /// The stored value as it was shown, once the field holds another.
    pub(super) fn was(&self) -> Option<String> {
        self.is_modified().then(|| self.shown.clone())
    }

    /// The typed value, when the user changed the field to an accepted one.
    fn typed(&self) -> Option<f64> {
        self.is_modified()
            .then(|| self.text.trim().parse::<f64>().ok())
            .flatten()
            .filter(|value| value.is_finite() && (self.min..=self.max).contains(value))
    }

    /// Why the typed text cannot be applied. An untouched field is never
    /// wrong, even when the drawing stores a value outside the range.
    pub(super) fn error(&self) -> Option<Cow<'static, str>> {
        if !self.is_modified() || self.typed().is_some() {
            return None;
        }
        Some(if !self.max.is_finite() {
            crate::t!("Enter 0 or a positive number.")
        } else if self.min < 0.0 {
            // Only the oblique angle goes either way of upright.
            crate::tf!(
                "Must be between {}° and {}°.",
                signed(self.min),
                signed(self.max)
            )
        } else {
            crate::tf!(
                "Use digits and a decimal point. Enter a value from {} to {}.",
                self.min,
                self.max
            )
        })
    }
}

/// `value` with a true minus sign, as a message writes a negative number.
fn signed(value: f64) -> String {
    value.to_string().replacen('-', "\u{2212}", 1)
}

/// The regular face file of common TrueType families, as other programs
/// find them. A style stores the file; the family alone is not enough.
const USUAL_FACE_FILES: &[(&str, &str)] = &[
    ("Arial", "arial.ttf"),
    ("Arial Narrow", "arialn.ttf"),
    ("Calibri", "calibri.ttf"),
    ("Century Gothic", "gothic.ttf"),
    ("Consolas", "consola.ttf"),
    ("Courier New", "cour.ttf"),
    ("Georgia", "georgia.ttf"),
    ("ISOCPEUR", "isocpeur.ttf"),
    ("Segoe UI", "segoeui.ttf"),
    ("Tahoma", "tahoma.ttf"),
    ("Times New Roman", "times.ttf"),
    ("Trebuchet MS", "trebuc.ttf"),
    ("Verdana", "verdana.ttf"),
];

/// The usual face file of `family`, if it is a common one.
fn usual_face_file(family: &str) -> Option<&'static str> {
    USUAL_FACE_FILES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(family.trim()))
        .map(|(_, file)| *file)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct TextStyleDraft {
    /// The selected style with its exact stored values. Toggles and font
    /// fields edit it directly; they hold no rounded text.
    pub(super) style: TextStyle,
    pub(super) width: NumberField,
    /// Shown in degrees, stored in radians.
    pub(super) oblique: NumberField,
    pub(super) height: NumberField,
    /// "Use Big Font" is on. The style stores only the file, so the switch
    /// lives here: on with an empty file while the user picks one.
    pub(super) big_font_on: bool,
    /// The big font the switch turned off, given back when it is turned on.
    big_font_off: String,
    /// What the last font switch turned off, to say so.
    pub(super) turned_off: Option<String>,
}

impl Default for TextStyleDraft {
    fn default() -> Self {
        Self::load(&TextStyle::new("Standard"))
    }
}

impl TextStyleDraft {
    pub(super) fn load(style: &TextStyle) -> Self {
        Self {
            style: style.clone(),
            width: NumberField::new(format!("{:.4}", style.width_factor), 0.01, 100.0),
            oblique: NumberField::new(
                format!("{:.2}", style.oblique_angle.to_degrees()),
                -85.0,
                85.0,
            ),
            height: NumberField::new(format!("{:.4}", style.height), 0.0, f64::INFINITY),
            big_font_on: !style.big_font_file.trim().is_empty(),
            big_font_off: String::new(),
            turned_off: None,
        }
    }

    /// How many number fields hold text that cannot be applied.
    pub(super) fn invalid_fields(&self) -> usize {
        [&self.width, &self.oblique, &self.height]
            .into_iter()
            .filter(|field| field.error().is_some())
            .count()
    }

    /// Turn "Use Big Font" on or off. Off clears the stored file; on gives
    /// back the file it cleared.
    pub(super) fn toggle_big_font(&mut self) {
        self.big_font_on = !self.big_font_on;
        if self.big_font_on {
            self.style.big_font_file = std::mem::take(&mut self.big_font_off);
        } else {
            self.big_font_off = std::mem::take(&mut self.style.big_font_file);
        }
    }

    /// Switch to a stroke (SHX) font. A stroke font has no bold or italic face,
    /// so those TrueType bits go; the character set and pitch bits stay.
    pub(super) fn pick_stroke_font(&mut self, file: String) {
        let style = &mut self.style;
        style.font_file = file;
        style.true_type_font.clear();
        style.true_type_font_flags &= !(TTF_FLAG_BOLD | TTF_FLAG_ITALIC);
        self.turned_off = None;
    }

    /// Switch to a TrueType family. The font file names the family's regular
    /// face (bold and italic are flags): the installed file, else the usual
    /// file of a common family (the web has no installed files), else empty.
    /// Vertical text and a big font apply to stroke fonts only.
    pub(super) fn pick_system_font(&mut self, family: String) {
        let style = &mut self.style;
        let mut off = Vec::new();
        if style.is_vertical {
            off.push(crate::t!("Vertical").into_owned());
        }
        if self.big_font_on {
            let file = style.big_font_file.trim();
            off.push(match file {
                "" => crate::t!("Big font").into_owned(),
                file => format!("{} ({file})", crate::t!("Big font")),
            });
        }
        self.turned_off = (!off.is_empty()).then(|| {
            let list = off.join(", ");
            crate::tf!("Turned off for the TrueType font: {list}.").into_owned()
        });
        style.font_file = crate::scene::text::sysfont::face_file(&family, false, false)
            .or_else(|| usual_face_file(&family).map(str::to_string))
            .unwrap_or_default();
        style.true_type_font = family;
        style.is_vertical = false;
        style.big_font_file.clear();
        self.big_font_on = false;
    }

    /// Set the TrueType face: the bold and italic flags, other bits kept.
    pub(super) fn set_face(&mut self, bold: bool, italic: bool) {
        let flags = &mut self.style.true_type_font_flags;
        *flags &= !(TTF_FLAG_BOLD | TTF_FLAG_ITALIC);
        if bold {
            *flags |= TTF_FLAG_BOLD;
        }
        if italic {
            *flags |= TTF_FLAG_ITALIC;
        }
    }

    /// The style as edited: the changed number fields written in, every other
    /// value exactly as stored.
    pub(super) fn resolved(&self) -> TextStyle {
        let mut style = self.style.clone();
        if let Some(width) = self.width.typed() {
            style.width_factor = width;
        }
        if let Some(oblique) = self.oblique.typed() {
            style.oblique_angle = oblique.to_radians();
        }
        if let Some(height) = self.height.typed() {
            style.height = height;
        }
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style() -> TextStyle {
        let mut style = TextStyle::new("Exact");
        style.width_factor = 0.123456789;
        style.oblique_angle = 0.1;
        style.height = 2.123456789;
        style
    }

    #[test]
    fn untouched_fields_keep_the_exact_values() {
        let draft = TextStyleDraft::load(&style());
        assert_eq!(draft.width.text, "0.1235");
        assert_eq!(draft.resolved(), style());
    }

    #[test]
    fn only_the_changed_field_is_written() {
        let mut draft = TextStyleDraft::load(&style());
        draft.height.text = "3.5".to_string();
        let resolved = draft.resolved();
        assert_eq!(resolved.height, 3.5);
        assert_eq!(resolved.width_factor, 0.123456789);
        assert_eq!(resolved.oblique_angle, 0.1);
    }

    #[test]
    fn a_value_out_of_range_or_not_a_number_is_kept_but_not_written() {
        let mut draft = TextStyleDraft::load(&style());
        draft.width.text = "200".to_string();
        draft.oblique.text = "1,5".to_string();
        draft.height.text = "-1".to_string();
        assert_eq!(draft.invalid_fields(), 3);
        assert!(draft.width.error().is_some_and(|error| error.contains("0.01 to 100")));
        assert_eq!(
            draft.oblique.error().as_deref(),
            Some("Must be between \u{2212}85° and 85°.")
        );
        assert_eq!(
            draft.height.error().as_deref(),
            Some("Enter 0 or a positive number.")
        );
        assert_eq!(draft.resolved(), style());
        assert_eq!(draft.width.text, "200");

        draft.width.text = "100".to_string();
        draft.oblique.text = "-85".to_string();
        draft.height.text = "0".to_string();
        assert_eq!(draft.invalid_fields(), 0);
    }

    #[test]
    fn an_untouched_value_outside_the_range_is_not_an_error() {
        let mut stored = style();
        stored.width_factor = 500.0;
        assert_eq!(TextStyleDraft::load(&stored).invalid_fields(), 0);
    }

    #[test]
    fn big_font_off_clears_the_file_and_on_gives_it_back() {
        let mut stored = style();
        stored.big_font_file = "gbcbig.shx".to_string();
        let mut draft = TextStyleDraft::load(&stored);
        assert!(draft.big_font_on);
        draft.toggle_big_font();
        assert!(draft.resolved().big_font_file.is_empty());
        draft.toggle_big_font();
        assert_eq!(draft.resolved(), stored);
    }

    #[test]
    fn a_truetype_font_says_what_it_turned_off() {
        let mut stored = style();
        stored.is_vertical = true;
        stored.big_font_file = "gbcbig.shx".to_string();
        let mut draft = TextStyleDraft::load(&stored);
        draft.pick_system_font("Arial".to_string());
        assert_eq!(
            draft.turned_off.as_deref(),
            Some("Turned off for the TrueType font: Vertical, Big font (gbcbig.shx).")
        );
        // Another TrueType font has nothing more to turn off.
        draft.pick_system_font("Verdana".to_string());
        assert_eq!(draft.turned_off, None);
        draft.pick_stroke_font("txt.shx".to_string());
        assert_eq!(draft.turned_off, None);
    }

    #[test]
    fn a_common_family_names_its_usual_file_and_another_none() {
        assert_eq!(usual_face_file("Times New Roman"), Some("times.ttf"));
        assert_eq!(usual_face_file(" arial "), Some("arial.ttf"));
        assert_eq!(usual_face_file("Noto Sans"), None);
        for (family, file) in USUAL_FACE_FILES {
            assert!(file.ends_with(".ttf"), "{family}");
        }
    }

    #[test]
    fn typing_the_shown_text_again_is_no_change() {
        let mut draft = TextStyleDraft::load(&style());
        draft.oblique.text = "9".to_string();
        draft.oblique.text = " 5.73 ".to_string();
        assert!(!draft.oblique.is_modified());
        assert_eq!(draft.resolved(), style());
    }
}
