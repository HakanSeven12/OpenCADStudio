//! Element nesting depth, checked before roxmltree parses a document.
//!
//! roxmltree's tokenizer recurses once per element (`parse_element` ↔
//! `parse_content`, in 0.20 and 0.21), so a document nested deeply enough
//! overflows the stack inside `Document::parse`, which aborts the process.
//! A reader of files from outside checks the depth with [`within`] first.

/// True when no element nests deeper than `max`. A linear byte scan that
/// skips comments, CDATA, PIs, doctype subsets and quoted attribute values
/// so their `<`/`>` never count. Defensive only: malformed markup may
/// overcount and reject the file, but must never undercount.
pub(crate) fn within(text: &str, max: usize) -> bool {
    let b = text.as_bytes();
    let mut i = 0usize;
    let mut depth = 0usize;
    let mut tag = false; // inside `< … >`
    let mut closing = false; // the current tag is `</…`
    let mut quote = 0u8; // inside a quoted attribute value
    let mut mark = 0u8; // 1 comment, 2 cdata, 3 PI, 4 doctype/decl
    let mut brackets = 0usize; // doctype internal subset `[ … ]`
    while i < b.len() {
        match mark {
            1 => {
                if b[i..].starts_with(b"-->") {
                    mark = 0;
                    i += 3;
                } else {
                    i += 1;
                }
                continue;
            }
            2 => {
                if b[i..].starts_with(b"]]>") {
                    mark = 0;
                    i += 3;
                } else {
                    i += 1;
                }
                continue;
            }
            3 => {
                if b[i..].starts_with(b"?>") {
                    mark = 0;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            4 => {
                match b[i] {
                    b'[' => brackets += 1,
                    b']' => brackets = brackets.saturating_sub(1),
                    b'>' if brackets == 0 => mark = 0,
                    _ => {}
                }
                i += 1;
                continue;
            }
            _ => {}
        }
        let c = b[i];
        if quote != 0 {
            if c == quote {
                quote = 0;
            }
            i += 1;
            continue;
        }
        if tag {
            match c {
                b'"' | b'\'' => quote = c,
                b'>' => {
                    tag = false;
                    if closing {
                        depth = depth.saturating_sub(1);
                    } else {
                        // Self-closing (`<a/>`, `<a />`) adds no depth.
                        let mut j = i;
                        while j > 0 && b[j - 1].is_ascii_whitespace() {
                            j -= 1;
                        }
                        if !(j > 0 && b[j - 1] == b'/') {
                            depth += 1;
                            if depth > max {
                                return false;
                            }
                        }
                    }
                }
                _ => {}
            }
            i += 1;
            continue;
        }
        if c != b'<' {
            i += 1;
            continue;
        }
        let rest = &b[i..];
        if rest.starts_with(b"<!--") {
            mark = 1;
            i += 4;
        } else if rest.starts_with(b"<![CDATA[") {
            mark = 2;
            i += 9;
        } else if rest.starts_with(b"<?") {
            mark = 3;
            i += 2;
        } else if rest.starts_with(b"<!") {
            mark = 4;
            brackets = 0;
            i += 2;
        } else if rest.starts_with(b"</") {
            tag = true;
            closing = true;
            i += 2;
        } else {
            tag = true;
            closing = false;
            i += 1;
        }
    }
    true
}
