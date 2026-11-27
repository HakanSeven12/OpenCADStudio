//! Turns calculator input into tokens, each with its byte span in the input.

use std::ops::Range;

use super::CalcError;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TokKind {
    Number(f64),
    Ident(String),
    /// `$name`, lowercased: variables are case-insensitive.
    Var(String),
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Eq,
    /// `'` — feet, only meaningful straight after a number.
    Feet,
    /// `"` — inches, only meaningful straight after a number.
    Inches,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Token {
    pub kind: TokKind,
    pub span: Range<usize>,
}

pub(super) fn tokenize(input: &str) -> Result<Vec<Token>, CalcError> {
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        if c == '$' {
            let starts = |b: u8| b.is_ascii_alphabetic() || b == b'_';
            if !bytes.get(start + 1).copied().is_some_and(starts) {
                return Err(CalcError::new(
                    start..start + 1,
                    crate::t!("Type a variable name after $"),
                ));
            }
            let mut end = start + 1;
            while bytes
                .get(end)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                end += 1;
            }
            while chars.peek().is_some_and(|&(i, _)| i < end) {
                chars.next();
            }
            tokens.push(Token {
                kind: TokKind::Var(input[start + 1..end].to_ascii_lowercase()),
                span: start..end,
            });
            continue;
        }
        let single = match c {
            '+' => Some(TokKind::Plus),
            '-' => Some(TokKind::Minus),
            '*' => Some(TokKind::Star),
            '/' => Some(TokKind::Slash),
            '^' => Some(TokKind::Caret),
            '(' => Some(TokKind::LParen),
            ')' => Some(TokKind::RParen),
            '[' => Some(TokKind::LBracket),
            ']' => Some(TokKind::RBracket),
            ',' => Some(TokKind::Comma),
            '=' => Some(TokKind::Eq),
            '\'' => Some(TokKind::Feet),
            '"' => Some(TokKind::Inches),
            _ => None,
        };
        if let Some(kind) = single {
            tokens.push(Token {
                kind,
                span: start..start + 1,
            });
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let mut end = start + 1;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b'.') {
                end += 1;
            }
            // Scientific notation (`1e-3`, `2.5E10`), only when a digit
            // follows, so `2em` stays a number with a suffix.
            if end < bytes.len() && (bytes[end] | 0x20) == b'e' {
                let mut j = end + 1;
                if j < bytes.len() && (bytes[j] == b'+' || bytes[j] == b'-') {
                    j += 1;
                }
                if j < bytes.len() && bytes[j].is_ascii_digit() {
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    end = j;
                }
            }
            while chars.peek().is_some_and(|&(i, _)| i < end) {
                chars.next();
            }
            let text = &input[start..end];
            let value = text
                .parse::<f64>()
                .map_err(|_| CalcError::new(start..end, crate::tf!("invalid number '{}'", text)))?;
            tokens.push(Token {
                kind: TokKind::Number(value),
                span: start..end,
            });
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut end = start + c.len_utf8();
            while let Some(&(i, d)) = chars.peek() {
                if d.is_alphanumeric() || d == '_' {
                    end = i + d.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token {
                kind: TokKind::Ident(input[start..end].to_string()),
                span: start..end,
            });
            continue;
        }
        return Err(CalcError::new(
            start..start + c.len_utf8(),
            crate::tf!("unexpected character '{}'", c),
        ));
    }
    Ok(tokens)
}
