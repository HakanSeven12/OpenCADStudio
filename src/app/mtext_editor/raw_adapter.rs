//! Borrowed MTEXT source index. No normalization, owned document, or codec replacement.
//! Positions count logical Unicode scalars, not bytes, glyphs, or graphemes.
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Text,
    Escape,
    Unicode,
    Percent,
    Open,
    Close,
    Break,
    Column,
    Tab,
    Ignored,
    Control(char),
    Stack,
    Field,
    Unknown(char),
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Token {
    pub raw: Range<usize>,
    pub kind: Kind,
    pub visible: Option<Range<usize>>,
    pub partner: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Protection {
    UnsafeUnicode,
    Unknown,
    Incomplete,
    InvalidArgument,
    UnbalancedGroup,
    PreserveOnly,
    Stack,
    Field,
    UncertainSuffix,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Protected {
    pub raw: Range<usize>,
    pub reason: Protection,
}

#[derive(Clone, Debug)]
pub(super) struct Unit {
    pub raw: Range<usize>,
    pub visible: Range<usize>,
    pub kind: Kind,
}

/// Metadata for one exact borrowed source revision. Unknown-width structures
/// invalidate the remaining absolute mappings; None never means zero width.
#[derive(Debug)]
pub(super) struct SourceIndex<'a> {
    pub raw: &'a str,
    pub tokens: Vec<Token>,
    pub units: Vec<Unit>,
    pub protected: Vec<Protected>,
    pub visible_len: Option<usize>,
}

impl<'a> SourceIndex<'a> {
    pub fn new(raw: &'a str) -> Self {
        Scanner::new(raw).scan()
    }

    /// Inverse of the visible→raw mapping; the tests use it to prove both
    /// directions agree.
    #[cfg(test)]
    pub fn visible_at_raw(&self, byte: usize) -> Option<usize> {
        if !self.raw.is_char_boundary(byte) {
            return None;
        }
        if byte == 0 {
            return Some(0);
        }
        self.units
            .iter()
            .find_map(|u| endpoint(&u.raw, &u.visible, byte))
            .or_else(|| {
                self.tokens
                    .iter()
                    .find_map(|t| endpoint(&t.raw, t.visible.as_ref()?, byte))
            })
    }

    /// Zero-width controls create several possible boundaries. Affinity is
    /// deliberately NOT guessed here. Stack interiors have no raw boundary.
    pub fn raw_positions(&self, visible: usize) -> Vec<usize> {
        let mut result = if visible == 0 { vec![0] } else { vec![] };
        let mut add = |raw: &Range<usize>, span: &Range<usize>| {
            if visible == span.start {
                result.push(raw.start);
            }
            if visible == span.end {
                result.push(raw.end);
            }
        };
        for u in &self.units {
            add(&u.raw, &u.visible);
        }
        for t in &self.tokens {
            if let Some(span) = &t.visible {
                add(&t.raw, span);
            }
        }
        result.sort_unstable();
        result.dedup();
        result
    }
}

#[cfg(test)]
fn endpoint(raw: &Range<usize>, visible: &Range<usize>, byte: usize) -> Option<usize> {
    if byte == raw.start {
        Some(visible.start)
    } else if byte == raw.end {
        Some(visible.end)
    } else {
        None
    }
}

struct Scanner<'a> {
    index: SourceIndex<'a>,
    pos: usize,
    visible: usize,
    uncertain: Option<usize>,
    groups: Vec<usize>,
}

impl<'a> Scanner<'a> {
    fn new(raw: &'a str) -> Self {
        Self {
            index: SourceIndex {
                raw,
                tokens: vec![],
                units: vec![],
                protected: vec![],
                visible_len: None,
            },
            pos: 0,
            visible: 0,
            uncertain: None,
            groups: vec![],
        }
    }

    fn scan(mut self) -> SourceIndex<'a> {
        while self.pos < self.index.raw.len() {
            let start = self.pos;
            let rest = &self.index.raw[start..];
            let ch = rest.chars().next().unwrap();
            match ch {
                '\\' => self.backslash(),
                '{' => {
                    let i = self.emit(start + 1, Kind::Open, Some(0));
                    self.groups.push(i);
                }
                '}' => {
                    let i = self.emit(start + 1, Kind::Close, Some(0));
                    if let Some(open) = self.groups.pop() {
                        self.index.tokens[open].partner = Some(i);
                        self.index.tokens[i].partner = Some(open);
                    } else {
                        self.protect(start..start + 1, Protection::UnbalancedGroup, true);
                    }
                }
                '%' if rest.starts_with("%<") => self.field(),
                '%' if rest.starts_with("%%") => self.percent(),
                '^' if rest.starts_with("^I") => {
                    self.emit(start + 2, Kind::Tab, Some(1));
                }
                '^' if rest.starts_with("^J") => {
                    self.emit(start + 2, Kind::Break, Some(1));
                }
                '^' if rest.starts_with("^M") => {
                    self.emit(start + 2, Kind::Ignored, Some(0));
                }
                '\n' => {
                    self.emit(start + 1, Kind::Break, Some(1));
                }
                '\t' => {
                    self.emit(start + 1, Kind::Tab, Some(1));
                }
                c if unsupported(c) => {
                    self.emit(start + c.len_utf8(), Kind::Invalid, None);
                    self.protect(start..self.pos, Protection::InvalidArgument, true);
                }
                _ => self.text(),
            }
        }
        while let Some(open) = self.groups.pop() {
            self.protect(
                self.index.tokens[open].raw.start..self.index.raw.len(),
                Protection::UnbalancedGroup,
                true,
            );
        }
        if let Some(start) = self.uncertain {
            self.index.units.retain(|u| u.raw.end <= start);
            for token in &mut self.index.tokens {
                if token.raw.end > start {
                    token.visible = None;
                }
            }
            self.index.protected.push(Protected {
                raw: start..self.index.raw.len(),
                reason: Protection::UncertainSuffix,
            });
        } else {
            self.index.visible_len = Some(self.visible);
        }
        self.index
    }

    fn emit(&mut self, end: usize, kind: Kind, width: Option<usize>) -> usize {
        let start = self.pos;
        debug_assert!(end > start && self.index.raw.is_char_boundary(end));
        debug_assert_eq!(self.index.tokens.last().map_or(0, |t| t.raw.end), start);
        let visible = if self.uncertain.is_none() {
            width.map(|w| self.visible..self.visible + w)
        } else {
            None
        };
        if width.is_none() {
            self.uncertain.get_or_insert(start);
        }
        if let Some(span) = &visible {
            if kind == Kind::Text {
                for (n, (byte, ch)) in self.index.raw[start..end].char_indices().enumerate() {
                    self.index.units.push(Unit {
                        raw: start + byte..start + byte + ch.len_utf8(),
                        visible: span.start + n..span.start + n + 1,
                        kind,
                    });
                }
            } else if !span.is_empty() {
                self.index.units.push(Unit {
                    raw: start..end,
                    visible: span.clone(),
                    kind,
                });
            }
            self.visible = span.end;
        }
        self.index.tokens.push(Token {
            raw: start..end,
            kind,
            visible,
            partner: None,
        });
        self.pos = end;
        self.index.tokens.len() - 1
    }

    fn protect(&mut self, raw: Range<usize>, reason: Protection, uncertain: bool) {
        if uncertain {
            self.uncertain = Some(self.uncertain.map_or(raw.start, |s| s.min(raw.start)));
        }
        self.index.protected.push(Protected { raw, reason });
    }

    fn text(&mut self) {
        let start = self.pos;
        let mut end = start;
        let mut width = 0;
        for (byte, ch) in self.index.raw[start..].char_indices() {
            let rest = &self.index.raw[start + byte..];
            if matches!(ch, '\\' | '{' | '}' | '\n' | '\t')
                || unsupported(ch)
                || ["%%", "%<", "^I", "^J", "^M"]
                    .iter()
                    .any(|s| rest.starts_with(s))
            {
                break;
            }
            end = start + byte + ch.len_utf8();
            width += 1;
        }
        self.emit(end, Kind::Text, Some(width));
    }

    fn backslash(&mut self) {
        let start = self.pos;
        let Some(code) = self.index.raw[start + 1..].chars().next() else {
            self.emit(start + 1, Kind::Invalid, None);
            self.protect(start..self.pos, Protection::Incomplete, true);
            return;
        };
        let end = start + 1 + code.len_utf8();
        match code {
            '\\' | '{' | '}' | ';' | '~' => {
                self.emit(end, Kind::Escape, Some(1));
            }
            'U' if self.index.raw[end..].starts_with('+') => self.unicode(),
            'P' => {
                self.emit(end, Kind::Break, Some(1));
            }
            'N' => {
                self.emit(end, Kind::Column, Some(1));
            }
            'L' | 'l' | 'O' | 'o' | 'K' | 'k' => {
                self.emit(end, Kind::Control(code), Some(0));
            }
            'X' => {
                self.emit(end, Kind::Control(code), Some(0));
                self.protect(start..end, Protection::PreserveOnly, false);
            }
            'C' | 'c' | 'f' | 'F' | 'H' | 'W' | 'w' | 'T' | 'Q' | 'A' | 'b' | 'p' | 't' | 'B'
            | 'S' | 's' => self.argument(code, end),
            _ => {
                // No invented payload delimiter for unknown controls.
                self.emit(end, Kind::Unknown(code), None);
                self.protect(start..end, Protection::Unknown, true);
            }
        }
    }

    fn unicode(&mut self) {
        let start = self.pos;
        let digits_start = start + 3;
        let hex = self.index.raw[digits_start..]
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .count();
        // The codec reads exactly four digits; a fifth one is plain text.
        let digits = &self.index.raw[digits_start..digits_start + hex.min(4)];
        let valid = digits.len() == 4
            && u32::from_str_radix(digits, 16)
                .ok()
                .and_then(char::from_u32)
                .is_some();
        // An invalid escape (and its run of digits) stays protected as a whole.
        let end = digits_start + if valid { 4 } else { hex };
        let syntax_glyph = matches!(u32::from_str_radix(digits, 16), Ok(0x5c | 0x7b | 0x7d));
        self.emit(end, Kind::Unicode, valid.then_some(1));
        if !valid {
            self.protect(start..end, Protection::InvalidArgument, true);
        } else if syntax_glyph {
            self.protect(start..end, Protection::UnsafeUnicode, false);
        }
    }

    fn percent(&mut self) {
        let start = self.pos;
        let rest = &self.index.raw[start..];
        if rest.starts_with("%%%%") {
            self.emit(start + 4, Kind::Percent, Some(1));
        } else if let Some(code) = rest[2..].chars().next() {
            let valid = matches!(code, 'd' | 'D' | 'p' | 'P' | 'c' | 'C' | '%');
            self.emit(
                start + 2 + code.len_utf8(),
                if valid {
                    Kind::Percent
                } else {
                    Kind::Unknown(code)
                },
                valid.then_some(1),
            );
            if !valid {
                self.protect(start..self.pos, Protection::Unknown, true);
            }
        } else {
            self.emit(start + 2, Kind::Invalid, None);
            self.protect(start..self.pos, Protection::Incomplete, true);
        }
    }

    fn argument(&mut self, code: char, argument_start: usize) {
        let start = self.pos;
        let kind = if matches!(code, 'S' | 's') {
            Kind::Stack
        } else {
            Kind::Control(code)
        };
        let Some(semi) = self.index.raw[argument_start..].find(';') else {
            self.emit(self.index.raw.len(), kind, None);
            self.protect(start..self.pos, Protection::Incomplete, true);
            return;
        };
        let value_end = argument_start + semi;
        let value = &self.index.raw[argument_start..value_end];
        let mut end = value_end + 1;
        let lexical = !value
            .chars()
            .any(|c| matches!(c, '\\' | '{' | '}') || c.is_control())
            && !value.contains("%<");
        let width = if !lexical {
            None
        } else if kind == Kind::Stack {
            stack_width(value)
        } else {
            valid_argument(code, value).then_some(0)
        };
        let mut secondary_valid = true;
        if code == 'C' && width.is_some() {
            let rest = &self.index.raw[end..];
            if rest.starts_with(';') {
                end += 1;
            } else {
                let n = rest.find(';').unwrap_or(rest.len());
                if let Ok(color) = rest[..n].parse::<i32>() {
                    secondary_valid = (0..=256).contains(&color) && n < rest.len();
                    end += n + usize::from(n < rest.len());
                }
            }
        }
        let width = width.filter(|_| secondary_valid);
        self.emit(end, kind, width);
        if width.is_none() {
            self.protect(start..end, Protection::InvalidArgument, true);
        } else if kind == Kind::Stack {
            self.protect(start..end, Protection::Stack, false);
        } else if matches!(code, 'p' | 't' | 'B') {
            self.protect(start..end, Protection::PreserveOnly, false);
        }
    }

    fn field(&mut self) {
        let start = self.pos;
        let mut end = start;
        let mut depth = 0;
        let mut complete = false;
        while end < self.index.raw.len() {
            let rest = &self.index.raw[end..];
            if rest.starts_with("%<") {
                depth += 1;
                end += 2;
            } else if rest.starts_with(">%") {
                depth -= 1;
                end += 2;
                if depth == 0 {
                    complete = true;
                    break;
                }
            } else {
                end += rest.chars().next().unwrap().len_utf8();
            }
        }
        self.emit(end, Kind::Field, None);
        self.protect(start..end, Protection::Field, true);
        if !complete {
            self.protect(start..end, Protection::Incomplete, true);
        }
    }
}

fn unsupported(ch: char) -> bool {
    (ch.is_control() && !matches!(ch, '\n' | '\t'))
        || matches!(ch, '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

fn valid_argument(code: char, value: &str) -> bool {
    match code {
        'C' => value.parse::<u16>().is_ok_and(|n| n <= 256),
        'c' => value.trim().parse::<u32>().is_ok_and(|n| n <= 0x00ff_ffff),
        'f' | 'F' => {
            let name = if code == 'F' {
                value.strip_prefix('N').unwrap_or(value)
            } else {
                value
            };
            !name
                .split(['|', ','])
                .next()
                .unwrap_or("")
                .trim()
                .is_empty()
        }
        'H' | 'W' | 'w' | 'T' | 'Q' => {
            let value = if matches!(code, 'H' | 'W' | 'w' | 'T') {
                value.strip_suffix(['x', 'X']).unwrap_or(value)
            } else {
                value
            };
            value
                .trim()
                .parse::<f64>()
                .is_ok_and(|n| n.is_finite() && (!matches!(code, 'H' | 'W' | 'w') || n > 0.0))
        }
        'A' => matches!(value, "0" | "1" | "2"),
        'b' => matches!(value.trim(), "0" | "1"),
        'p' | 't' | 'B' => !value.trim().is_empty(),
        _ => false,
    }
}

fn stack_width(value: &str) -> Option<usize> {
    let (split, sep) = value
        .char_indices()
        .find(|(_, c)| matches!(c, '/' | '#' | '^'))?;
    let numerator = &value[..split];
    let denominator = &value[split + 1..];
    let denominator = if sep == '^' {
        denominator.strip_prefix(' ').unwrap_or(denominator)
    } else {
        denominator
    };
    if (numerator.is_empty() && denominator.is_empty())
        || (sep != '^' && (numerator.is_empty() || denominator.is_empty()))
        || numerator.contains(['/', '#', '^', '%'])
        || denominator.contains(['/', '#', '^', '%'])
    {
        return None;
    }
    // Main b2dfc73fc recognizes one-sided ^ stacks. Mirror the existing
    // codec/flatten_stack slot convention without exposing stack interiors:
    // top-only has no separator slot; bottom-only keeps its leading ^ slot.
    Some(
        numerator.chars().count()
            + usize::from(!denominator.is_empty())
            + denominator.chars().count(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditError {
    NativeField,
    RawField,
    UnknownStructure,
    InvalidRange,
    AtomicStructure,
    ParagraphBoundary,
    UnsupportedInput,
    ProjectionMismatch,
    NestingLimit,
    /// Bold/italic on a stroke (SHX/LFF) face, which MTEXT cannot store.
    FontFlags,
    /// Index or fraction text that a `\S` stack cannot hold.
    StackText,
    /// An index or fraction over differently formatted characters.
    StackFormat,
    /// A fraction with an empty numerator or denominator.
    FractionParts,
    /// An edit that would change or split a native field's text.
    FieldText,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NativeField => "MTEXT: text editing is disabled for a native FIELD host; its field template must be preserved.",
            Self::RawField => "MTEXT: the text contains a protected field expression.",
            Self::UnknownStructure => "MTEXT: an unknown, incomplete or invalid control prevents safe text editing.",
            Self::InvalidRange => "MTEXT: the character range has no safe source boundaries.",
            Self::AtomicStructure => "MTEXT: this operation would split or change a protected fraction or column break.",
            Self::ParagraphBoundary => "MTEXT: this change could alter paragraph or tab-stop settings.",
            Self::UnsupportedInput => "MTEXT: the inserted text contains an unsupported control character.",
            Self::ProjectionMismatch => "MTEXT: the source and rendered character positions do not agree; the edit was not applied.",
            Self::NestingLimit => "MTEXT: this edit would exceed the safe formatting-group nesting limit.",
            Self::FontFlags => "MTEXT: bold and italic can only be stored with a TrueType font; this text uses a stroke (SHX) font.",
            Self::StackText => "MTEXT: an index or fraction holds plain characters only: no line break, fraction, backslash, brace or / # ^ % ; character.",
            Self::StackFormat => "MTEXT: an index or fraction takes one formatting; the selection mixes formats.",
            Self::FractionParts => "MTEXT: a fraction needs both a numerator and a denominator.",
            Self::FieldText => "MTEXT: a field's text cannot be changed or split; edit the text around it.",
        })
    }
}

impl SourceIndex<'_> {
    pub fn validate(&self, native_field: bool) -> Result<usize, EditError> {
        if native_field {
            return Err(EditError::NativeField);
        }
        if self.protected.iter().any(|p| p.reason == Protection::Field) {
            return Err(EditError::RawField);
        }
        if self
            .protected
            .iter()
            .any(|p| p.reason == Protection::UnsafeUnicode)
        {
            return Err(EditError::UnknownStructure);
        }
        self.visible_len.ok_or(EditError::UnknownStructure)
    }

    pub fn validate_range(&self, range: &Range<usize>) -> Result<(), EditError> {
        let count = self.validate(false)?;
        if range.start > range.end
            || range.end > count
            || self.raw_positions(range.start).is_empty()
            || self.raw_positions(range.end).is_empty()
        {
            return Err(EditError::InvalidRange);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_unicode_syntax_glyphs_remain_mapped_but_protected() {
        for raw in [r"\U+005C", r"\U+007B", r"\U+007D"] {
            let index = checked(raw);
            assert_eq!(index.visible_len, Some(1));
            assert_eq!(index.validate(false), Err(EditError::UnknownStructure));
            assert!(index
                .protected
                .iter()
                .any(|p| p.reason == Protection::UnsafeUnicode));
        }
    }
    fn checked(raw: &str) -> SourceIndex<'_> {
        let index = SourceIndex::new(raw);
        let mut next = 0;
        let mut rebuilt = String::new();
        let mut coverage = vec![0; raw.len()];
        for token in &index.tokens {
            assert_eq!(token.raw.start, next, "{raw:?}");
            assert!(token.raw.start < token.raw.end);
            assert!(raw.is_char_boundary(token.raw.start) && raw.is_char_boundary(token.raw.end));
            rebuilt.push_str(&raw[token.raw.clone()]);
            for n in &mut coverage[token.raw.clone()] {
                *n += 1;
            }
            next = token.raw.end;
        }
        assert_eq!(next, raw.len());
        assert_eq!(rebuilt.as_bytes(), raw.as_bytes());
        assert!(coverage.iter().all(|n| *n == 1));
        for p in &index.protected {
            assert!(p.raw.start < p.raw.end);
            assert!(raw.is_char_boundary(p.raw.start) && raw.is_char_boundary(p.raw.end));
        }
        index
    }
    #[test]
    fn empty_and_unicode_offsets() {
        let empty = checked("");
        assert_eq!(empty.visible_len, Some(0));
        assert_eq!(empty.raw_positions(0), vec![0]);
        let raw = "Árvíztűrő Ø± 😀 e\u{301} 👩\u{200d}🔧";
        let i = checked(raw);
        assert_eq!(i.visible_len, Some(raw.chars().count()));
        for (n, (byte, ch)) in raw.char_indices().enumerate() {
            assert_eq!(i.visible_at_raw(byte), Some(n));
            assert_eq!(i.raw_positions(n), vec![byte]);
            for b in byte + 1..byte + ch.len_utf8() {
                assert_eq!(i.visible_at_raw(b), None);
            }
        }
    }
    #[test]
    fn escapes_are_atoms_not_group_delimiters() {
        let i = checked(r"\{\}\\\;\~");
        assert_eq!(i.visible_len, Some(5));
        assert!(i.tokens.iter().all(|t| t.kind == Kind::Escape));
        for n in 0..5 {
            assert_eq!(i.raw_positions(n), vec![n * 2]);
            assert_eq!(i.visible_at_raw(n * 2 + 1), None);
        }
    }
    #[test]
    fn unicode_and_percent_escapes() {
        // Exactly four digits, as the codec reads them: `\U+1F600` is U+1F60
        // followed by a literal "0".
        let i = checked(r"\U+00D8 \U+1F600");
        assert_eq!(i.visible_len, Some(4));
        assert_eq!(i.raw_positions(1), vec![7]);
        assert_eq!(i.raw_positions(3), vec![15]);
        assert_eq!(i.raw_positions(4), vec![16]);
        assert_eq!(i.visible_at_raw(4), None);
        assert_eq!(checked(r"\U+110000").visible_len, Some(3));
        assert_eq!(checked("%%d%%P%%c%%%%").visible_len, Some(4));
    }
    #[test]
    fn nested_groups_and_font_metadata_are_lossless() {
        let i = checked(r"{\fArial|b1|i0|c238|p34|opaque;A{\Lé\l}B}");
        assert_eq!(i.visible_len, Some(3));
        assert!(i.protected.is_empty());
        for (n, t) in i.tokens.iter().enumerate() {
            if matches!(t.kind, Kind::Open | Kind::Close) {
                assert_eq!(i.tokens[t.partner.unwrap()].partner, Some(n));
            }
        }
        assert!(i.raw_positions(1).len() > 1);
    }
    #[test]
    fn known_controls_and_secondary_aci() {
        let i = checked(
            r"\C256;\c255;\H1.25x;\W0.8;\w1.2x;\T1.1;\Q15;\A1;\b1;\L\l\O\o\K\k\FNromans.shx;é",
        );
        assert_eq!(i.visible_len, Some(1));
        assert!(i.protected.is_empty());
        for raw in [r"\C1;42;Red", r"\C1;;Red", r"\C0;\LRed\l"] {
            assert_eq!(checked(raw).visible_len, Some(3));
        }
    }
    #[test]
    fn break_column_and_caret_control_positions() {
        assert_eq!(checked("a\\Pb\\Nc\n").visible_len, Some(6));
        let i = checked("a^Ib^Jc^M\t");
        assert_eq!(i.visible_len, Some(6));
        assert_eq!(i.raw_positions(5), vec![7, 9]);
        assert_eq!(i.visible_at_raw(8), None);
    }
    #[test]
    fn three_fraction_forms_are_atomic_and_protected() {
        for raw in [r"\S1/2;", r"\S1#2;", r"\S1^2;", r"\S1^ 2;"] {
            let i = checked(raw);
            assert_eq!(i.visible_len, Some(3));
            assert!(i.raw_positions(1).is_empty());
            assert!(i.raw_positions(2).is_empty());
            assert_eq!(i.raw_positions(3), vec![raw.len()]);
            assert_eq!(i.protected[0].reason, Protection::Stack);
        }
        assert_eq!(checked(r"é\Sα/β;ő").visible_len, Some(5));
    }
    #[test]
    fn tabstops_keep_bare_and_extended_spelling() {
        for raw in [
            r"\pxqc,i-1.5,l2,t4,c8,r12,D.16;A^IB",
            r"\pqc,i-1.5,l2,t4,c8,r12,D.16;A^IB",
            r"A\t4;\B1;\XB",
        ] {
            let i = checked(raw);
            assert!(i.visible_len.is_some());
            assert!(i
                .protected
                .iter()
                .all(|p| p.reason == Protection::PreserveOnly));
        }
    }
    #[test]
    fn unknown_controls_do_not_guess_payload_or_resynchronize() {
        for raw in [r"é\Zopaque;{\Ltail\l}", "é\\🧭payload;ő", "é%%zvalue"] {
            let i = checked(raw);
            assert_eq!(i.visible_len, None);
            assert_eq!(i.raw_positions(1), vec![2]);
            assert!(i.raw_positions(2).is_empty());
            assert_eq!(i.visible_at_raw(raw.len()), None);
            assert!(i.protected.iter().any(|p| p.reason == Protection::Unknown));
        }
    }
    #[test]
    fn nested_and_incomplete_fields_are_opaque() {
        for raw in [
            r"a%<\AcExpr (%<\AcVar X>% + 1)>%b",
            r"a%<\AcExpr (%<\AcVar X>%",
        ] {
            let i = checked(raw);
            assert_eq!(i.tokens[1].kind, Kind::Field);
            assert_eq!(i.visible_len, None);
            assert!(i.protected.iter().any(|p| p.reason == Protection::Field));
        }
    }
    #[test]
    fn unbalanced_groups_revoke_inner_mappings() {
        for raw in ["a{é{b}", "a}tail"] {
            let i = checked(raw);
            assert_eq!(i.visible_len, None);
            assert!(i.tokens[1..].iter().all(|t| t.visible.is_none()));
            assert!(i
                .protected
                .iter()
                .any(|p| p.reason == Protection::UnbalancedGroup));
        }
    }
    #[test]
    fn invalid_and_incomplete_input_is_never_repaired() {
        for raw in [
            "a\\",
            "a%%",
            r"a\H1.5",
            r"a\S1/2",
            r"a\C1;42",
            r"\HNaN;",
            r"\W0;",
            r"\Qinf;",
            r"\C999;",
            r"\A7;",
            r"\f;",
            r"\S/2;",
            r"\S1/;",
            r"\S1/2/3;",
            r"\p;",
            r"\U+",
            r"\U+D800",
            r"\U+12",
            "a\r\nb",
            "a\0b",
            "a\u{200e}b",
        ] {
            let i = checked(raw);
            assert_eq!(i.visible_len, None, "{raw:?}");
            assert!(!i.protected.is_empty());
        }
    }
    #[test]
    fn every_utf8_prefix_and_fragment_pair_is_partitioned() {
        let raw = r"é{\fArial|b1;Ø\P\pxqc,t4;\Sα/β;\U+1F600\Zopaque;}";
        for end in raw.char_indices().map(|(b, _)| b).chain([raw.len()]) {
            checked(&raw[..end]);
        }
        let parts = [
            "é", "😀", "{", "}", "\\", r"\H2;", r"\S1/2;", r"\Z", "%%", "%%d", "%<", ">%", "^I",
            r"\{", "\0",
        ];
        for a in parts {
            for b in parts {
                checked(&format!("{a}{b}"));
            }
        }
    }
    #[test]
    fn supported_counts_agree_with_existing_codec_cells() {
        for raw in [
            "Árvíztűrő 😀 e\u{301}",
            r"\{\}\\\;\~",
            r"\U+00D8 \U+1F600",
            r"{\fArial|b1|i0|c238;A{\Lé\l}B}",
            r"\C1;42;Red\P\c255;Blue",
            r"\pxqc,i-1.5,l2,t4,c8,r12;A^IB",
            r"a\t4;\B1;\Xb",
            r"a\S1/2;b\Sα#β;c\S3^ 4;d",
            "a^Ib^Jc^M\td",
            r"%%d%%P%%c%%%%",
            r"a\Pb\Nc",
        ] {
            let i = checked(raw);
            let doc = codec::entities::mtext_format::parse_mtext(raw, true);
            let n = super::super::doc_to_cells(&doc).len();
            assert_eq!(i.visible_len, Some(n), "{raw:?}");
            for p in 0..=n {
                for b in i.raw_positions(p) {
                    assert_eq!(i.visible_at_raw(b), Some(p));
                }
            }
        }
    }
    #[test]
    fn native_and_raw_fields_are_blocked_before_mutation() {
        assert_eq!(
            SourceIndex::new("plain").validate(true),
            Err(EditError::NativeField)
        );
        assert_eq!(
            SourceIndex::new(r"a%<\AcVar X>%b").validate(false),
            Err(EditError::RawField)
        );
    }
    #[test]
    fn uncertain_and_atomic_boundaries_are_not_exposed_as_editable() {
        assert_eq!(
            SourceIndex::new(r"a\Zopaque").validate_range(&(0..1)),
            Err(EditError::UnknownStructure)
        );
        let fraction = SourceIndex::new(r"a\S1/2;b");
        assert_eq!(
            fraction.validate_range(&(2..3)),
            Err(EditError::InvalidRange)
        );
        assert_eq!(fraction.validate_range(&(1..4)), Ok(()));
        assert_eq!(
            fraction.validate_range(&(0..99)),
            Err(EditError::InvalidRange)
        );
    }
}
