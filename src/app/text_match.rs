//! Text matching shared by Find and Replace and the text editor's Find and
//! Replace window: case, whole words, wildcards, diacritics and half/full
//! width forms.

/// How a search text is compared.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MatchOptions {
    pub match_case: bool,
    pub whole_words: bool,
    pub wildcards: bool,
    /// On: an accented letter only matches itself. Off: it matches its base
    /// letter.
    pub diacritics: bool,
    /// On: the full-width and half-width forms of a character match each
    /// other. Off: each matches only itself.
    pub half_full: bool,
}

/// A character as the options compare it.
fn folded(c: char, o: &MatchOptions) -> char {
    let mut c = c;
    if o.half_full {
        if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
            c = char::from_u32(c as u32 - 0xFEE0).unwrap_or(c);
        } else if c == '\u{3000}' {
            c = ' ';
        }
    }
    if !o.diacritics {
        c = base_letter(c);
    }
    if !o.match_case {
        c = c.to_lowercase().next().unwrap_or(c);
    }
    c
}

/// The letter without its diacritic, for the Latin letters text uses most.
fn base_letter(c: char) -> char {
    const FROM: &str = "ÀÁÂÃÄÅàáâãäåÇçĆćČčÈÉÊËèéêëĞğÌÍÎÏìíîïİıÑñŃńÒÓÔÕÖØòóôõöøŞşŠšŚśÙÚÛÜùúûüÝýÿŽžŹźŻż";
    const TO: &str = "AAAAAAaaaaaaCcCcCcEEEEeeeeGgIIIIiiiiIiNnNnOOOOOOooooooSsSsSsUUUUuuuuYyyZzZzZz";
    FROM.chars()
        .position(|f| f == c)
        .and_then(|n| TO.chars().nth(n))
        .unwrap_or(c)
}

fn is_word(c: Option<char>) -> bool {
    c.is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// One wildcard token.
enum Tok {
    Lit(char),
    Any,
    Run,
    Digit,
    Alpha,
    NonAlnum,
    /// `[...]` / `[~...]`: ranges and single characters, negated.
    Class(Vec<(char, char)>, bool),
}

fn tokens(pattern: &str, wildcards: bool) -> Vec<Tok> {
    if !wildcards {
        return pattern.chars().map(Tok::Lit).collect();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        out.push(match c {
            '`' if i < chars.len() => {
                i += 1;
                Tok::Lit(chars[i - 1])
            }
            '*' => Tok::Run,
            '?' => Tok::Any,
            '#' => Tok::Digit,
            '@' => Tok::Alpha,
            '.' => Tok::NonAlnum,
            '[' => {
                let negate = chars.get(i) == Some(&'~');
                if negate {
                    i += 1;
                }
                let mut ranges = Vec::new();
                while i < chars.len() && chars[i] != ']' {
                    let lo = chars[i];
                    if chars.get(i + 1) == Some(&'-') && chars.get(i + 2).is_some_and(|c| *c != ']')
                    {
                        ranges.push((lo, chars[i + 2]));
                        i += 3;
                    } else {
                        ranges.push((lo, lo));
                        i += 1;
                    }
                }
                i += 1;
                Tok::Class(ranges, negate)
            }
            other => Tok::Lit(other),
        });
    }
    out
}

fn token_matches(t: &Tok, c: char, o: &MatchOptions) -> bool {
    match t {
        Tok::Lit(l) => folded(c, o) == folded(*l, o),
        Tok::Any => c != '\n',
        Tok::Digit => c.is_ascii_digit(),
        Tok::Alpha => c.is_alphabetic(),
        Tok::NonAlnum => !c.is_alphanumeric(),
        Tok::Class(ranges, negate) => {
            let f = folded(c, o);
            let hit = ranges.iter().any(|(lo, hi)| {
                let (lo, hi) = (folded(*lo, o), folded(*hi, o));
                (lo..=hi).contains(&f)
            });
            hit != *negate
        }
        Tok::Run => true,
    }
}

/// The end of the shortest match of `p` starting at `i`.
fn match_at(chars: &[Option<char>], i: usize, p: &[Tok], o: &MatchOptions) -> Option<usize> {
    let Some((first, rest)) = p.split_first() else {
        return Some(i);
    };
    if let Tok::Run = first {
        return (i..=chars.len()).find_map(|j| match_at(chars, j, rest, o));
    }
    let c = chars.get(i).copied().flatten()?;
    if token_matches(first, c, o) {
        match_at(chars, i + 1, rest, o)
    } else {
        None
    }
}

/// Every match of `pattern` in `chars` as (start, end), left to right and not
/// overlapping. `None` cells (a stacked fraction) never match.
pub(crate) fn find_ranges(
    chars: &[Option<char>],
    pattern: &str,
    o: &MatchOptions,
) -> Vec<(usize, usize)> {
    let toks = tokens(pattern, o.wildcards);
    if toks.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        match match_at(chars, at, &toks, o) {
            Some(end)
                if end > at
                    && (!o.whole_words
                        || (!is_word(at.checked_sub(1).and_then(|p| chars[p]))
                            && !is_word(chars.get(end).copied().flatten()))) =>
            {
                out.push((at, end));
                at = end;
            }
            _ => at += 1,
        }
    }
    out
}

/// The matches in a string, as byte ranges.
pub(crate) fn str_ranges(text: &str, pattern: &str, o: &MatchOptions) -> Vec<(usize, usize)> {
    let mut offsets: Vec<usize> = text.char_indices().map(|(at, _)| at).collect();
    offsets.push(text.len());
    let chars: Vec<Option<char>> = text.chars().map(Some).collect();
    find_ranges(&chars, pattern, o)
        .into_iter()
        .map(|(s, e)| (offsets[s], offsets[e]))
        .collect()
}

/// `text` with the given byte ranges (left to right) replaced.
pub(crate) fn replace_ranges(text: &str, ranges: &[(usize, usize)], with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for (s, e) in ranges {
        out.push_str(&text[cursor..*s]);
        out.push_str(with);
        cursor = *e;
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(text: &str, pattern: &str, o: MatchOptions) -> usize {
        str_ranges(text, pattern, &o).len()
    }

    #[test]
    fn options_fold_as_the_find_window_describes() {
        let plain = MatchOptions {
            diacritics: true,
            ..Default::default()
        };
        assert_eq!(n("Alpha alphabet", "alpha", plain), 2);
        assert_eq!(n("Älpha", "alpha", plain), 0);
        let loose = MatchOptions { diacritics: false, ..plain };
        assert_eq!(n("Älpha", "alpha", loose), 1);
        assert_eq!(n("\u{FF21}\u{FF4C}\u{FF50}\u{FF48}\u{FF41}", "alpha", plain), 0);
        let width = MatchOptions { half_full: true, ..plain };
        assert_eq!(n("\u{FF21}\u{FF4C}\u{FF50}\u{FF48}\u{FF41}", "alpha", width), 1);
        let wild = MatchOptions { wildcards: true, ..plain };
        assert_eq!(n("beta Alphabet", "Alph*", wild), 1);
        assert_eq!(n("beta Alphabet", "Alph?", wild), 1);
        assert_eq!(n("beta Alphabet", "[a-b]eta", wild), 1);
        let whole = MatchOptions { whole_words: true, ..plain };
        assert_eq!(n("beta Alphabet alpha", "alpha", whole), 1);
    }
}
