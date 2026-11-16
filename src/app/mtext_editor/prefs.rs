//! The in-place text editor's own settings, kept with the profile: lists,
//! AutoStack and the stack defaults, spell checking while typing, what the
//! editor shows (WYSIWYG, background, highlight colour) and whether a style
//! change on a formatted text is asked about.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// How a stack is drawn (Stack Properties > Style).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StackStyle {
    /// `x/y` over a horizontal bar.
    #[default]
    Horizontal,
    /// `x#y`, slanted.
    Diagonal,
    /// `x^y`, one over the other with no bar.
    Tolerance,
    /// `x~.y`, lined up on the decimal point.
    Decimal,
}

/// Where a stack sits on its line (Stack Properties > Position): `\A0;`
/// bottom, `\A1;` centre, `\A2;` top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StackPosition {
    Top,
    #[default]
    Center,
    Bottom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MTextEditorPrefs {
    /// Allow Bullets and Lists.
    pub allow_lists: bool,
    /// Allow Auto Bullets and Numbering.
    pub allow_auto_lists: bool,
    /// Misspelled words are underlined while typing.
    pub spell_check: bool,
    /// Always Display as WYSIWYG (MTEXTFIXED other than 2).
    pub wysiwyg: bool,
    /// Show Background: the text box is filled while editing.
    pub show_background: bool,
    /// Text Highlight Color: the selection's fill (None: the theme's).
    pub highlight: Option<[u8; 3]>,
    /// AutoStack: numbers typed around `/`, `#` or `^` followed by a space
    /// stack.
    pub auto_stack: bool,
    /// AutoStack makes `x/y` a diagonal fraction (else a horizontal one).
    pub auto_stack_diagonal: bool,
    /// AutoStack drops the blank before the fraction (`1 x/y` → `1x/y`).
    pub auto_stack_trim_blank: bool,
    /// The stack text size, percent of the text height.
    pub stack_size: u8,
    pub stack_style: StackStyle,
    pub stack_position: StackPosition,
    /// "Always change the text style" of the Text Style Change question.
    pub always_change_style: bool,
    /// "Always turn Bullets and Lists on or off" of the Bullets and Lists
    /// question.
    pub always_toggle_lists: bool,
}

impl Default for MTextEditorPrefs {
    fn default() -> Self {
        Self {
            allow_lists: true,
            allow_auto_lists: true,
            spell_check: true,
            wysiwyg: true,
            show_background: false,
            highlight: None,
            auto_stack: true,
            auto_stack_diagonal: false,
            auto_stack_trim_blank: false,
            stack_size: 70,
            stack_style: StackStyle::Horizontal,
            stack_position: StackPosition::Center,
            always_change_style: false,
            always_toggle_lists: false,
        }
    }
}

static PREFS: Mutex<Option<MTextEditorPrefs>> = Mutex::new(None);

/// The settings in force.
pub(crate) fn prefs() -> MTextEditorPrefs {
    PREFS
        .lock()
        .map(|p| p.clone().unwrap_or_default())
        .unwrap_or_default()
}

/// Put `prefs` in force.
pub(crate) fn set_prefs(prefs: MTextEditorPrefs) {
    super::raw_list::set_lists_allowed(prefs.allow_lists);
    super::raw_list::set_auto_lists_allowed(prefs.allow_auto_lists);
    if let Ok(mut slot) = PREFS.lock() {
        *slot = Some(prefs);
    }
}

/// Change the settings in force.
pub(crate) fn update_prefs(change: impl FnOnce(&mut MTextEditorPrefs)) {
    let mut p = prefs();
    change(&mut p);
    set_prefs(p);
}
