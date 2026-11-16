//! Paragraph settings written as the format writes them: one `\p…;` code at
//! the start of a paragraph, holding only the values that differ from the
//! paragraph before it (each value stays in force until set again), in the
//! order `i l r b a s q t`. The first such code of the text is `\px…;`, the
//! later ones `\p…;`.
use super::raw_adapter::{EditError, Kind, SourceIndex};
use super::raw_edit::editor_document;
use super::raw_operations::apply_patches;
use super::raw_span::number;
use document::entities::mtext_format::{
    parse_mtext, MTextLineSpacing, MTextParagraphAlignment, MTextSpan, ParagraphProperties,
    TabStop,
};

/// The paragraph values in force: each set value stays until set again.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct InForce {
    pub first: f64,
    pub left: f64,
    pub right: f64,
    pub before: f64,
    pub after: f64,
    pub spacing: MTextLineSpacing,
    pub alignment: MTextParagraphAlignment,
    pub tabs: Vec<TabStop>,
}

impl Default for InForce {
    fn default() -> Self {
        Self {
            first: 0.0,
            left: 0.0,
            right: 0.0,
            before: 0.0,
            after: 0.0,
            spacing: MTextLineSpacing::Default,
            alignment: MTextParagraphAlignment::Default,
            tabs: Vec::new(),
        }
    }
}

/// A tab list of one stop at 0 is no tab stop: `t0` clears the stops.
fn tabs_in_force(tabs: &[TabStop]) -> Vec<TabStop> {
    match tabs {
        [TabStop::Left(p)] if *p == 0.0 => Vec::new(),
        tabs => tabs.to_vec(),
    }
}

impl InForce {
    /// The values in force once `p` sets its own.
    pub(super) fn after(&self, p: &ParagraphProperties) -> Self {
        let mut next = self.clone();
        if let Some(v) = p.first_line_indent {
            next.first = v;
        }
        if let Some(v) = p.left_margin {
            next.left = v;
        }
        if let Some(v) = p.right_margin {
            next.right = v;
        }
        if let Some(v) = p.spacing_before {
            next.before = v;
        }
        if let Some(v) = p.spacing_after {
            next.after = v;
        }
        if let Some(v) = p.line_spacing {
            next.spacing = v;
        }
        if let Some(v) = p.alignment {
            next.alignment = v;
        }
        if !p.tab_stops.is_empty() {
            next.tabs = tabs_in_force(&p.tab_stops);
        }
        next
    }

    /// The code values setting `self` where `previous` is in force.
    fn changes_from(&self, previous: &Self) -> Vec<String> {
        let mut out = Vec::new();
        let mut put = |key: &str, a: f64, b: f64| {
            if a != b {
                out.push(format!("{key}{}", number(a)));
            }
        };
        put("i", self.first, previous.first);
        put("l", self.left, previous.left);
        put("r", self.right, previous.right);
        put("b", self.before, previous.before);
        put("a", self.after, previous.after);
        if self.spacing != previous.spacing {
            out.push(match self.spacing {
                MTextLineSpacing::Default => "s*".to_string(),
                MTextLineSpacing::Exact(v) => format!("se{}", number(v)),
                MTextLineSpacing::AtLeast(v) => format!("sa{}", number(v)),
                MTextLineSpacing::Multiple(v) => format!("sm{}", number(v)),
            });
        }
        if self.alignment != previous.alignment {
            out.push(
                match self.alignment {
                    MTextParagraphAlignment::Default => "q*",
                    MTextParagraphAlignment::Left => "ql",
                    MTextParagraphAlignment::Center => "qc",
                    MTextParagraphAlignment::Right => "qr",
                    MTextParagraphAlignment::Justified => "qj",
                    MTextParagraphAlignment::Distributed => "qd",
                }
                .to_string(),
            );
        }
        if self.tabs != previous.tabs {
            let list: Vec<String> = if self.tabs.is_empty() {
                vec!["0".to_string()]
            } else {
                self.tabs.iter().map(|tab| tab.to_string()).collect()
            };
            out.push(format!("t{}", list.join(",")));
        }
        out
    }
}

/// The values in force in each paragraph of `raw`, with its text.
pub(super) fn paragraphs_in_force(raw: &str) -> Vec<(InForce, Vec<MTextSpan>)> {
    let mut state = InForce::default();
    editor_document(raw)
        .paragraphs
        .into_iter()
        .map(|p| {
            state = state.after(&p.properties);
            (state.clone(), p.spans)
        })
        .collect()
}

/// The paragraph values a code such as `\pxqc;` sets.
pub(super) fn code_setting(code: &str) -> ParagraphProperties {
    parse_mtext(&format!("{code}x"), true)
        .paragraphs
        .first()
        .map(|p| p.properties.clone())
        .unwrap_or_default()
}

/// `raw` with every paragraph's values in force set by `change` (paragraph
/// number, values) and all its paragraph settings rewritten as the format
/// writes them. The text comes out as it was and each paragraph with exactly
/// the values asked for, or the change is refused.
pub(super) fn rewrite(
    raw: &str,
    change: impl Fn(usize, &mut InForce),
) -> Result<String, EditError> {
    let index = SourceIndex::new(raw);
    let count = index.validate(false)?;
    let mut want = paragraphs_in_force(raw);
    for (n, (state, _)) in want.iter_mut().enumerate() {
        change(n, state);
    }
    let starts: Vec<usize> = std::iter::once(0)
        .chain(
            index
                .units
                .iter()
                .filter(|u| matches!(u.kind, Kind::Break | Kind::Column))
                .map(|u| u.raw.end),
        )
        .collect();
    if starts.len() != want.len() {
        return Err(EditError::ParagraphBoundary);
    }
    let mut patches: Vec<(std::ops::Range<usize>, String)> = index
        .tokens
        .iter()
        .filter(|t| t.kind == Kind::Control('p'))
        .map(|t| (t.raw.clone(), String::new()))
        .collect();
    let mut previous = InForce::default();
    let mut first = true;
    for (start, (state, _)) in starts.iter().zip(&want) {
        let values = state.changes_from(&previous);
        if !values.is_empty() {
            let x = if first { "x" } else { "" };
            first = false;
            patches.push((*start..*start, format!("\\p{x}{};", values.join(","))));
        }
        previous = state.clone();
    }
    let out = apply_patches(raw, patches)?;
    index.certify(&out, count)?;
    if paragraphs_in_force(&out) != want {
        return Err(EditError::ParagraphBoundary);
    }
    Ok(out)
}
