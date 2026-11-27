//! Name patterns as the command line and the filters read them: `*` matches
//! any run of characters (including none) and `?` one character, ignoring
//! case.

/// Case-insensitive name match: `*` matches any run (including empty),
/// `?` matches exactly one char.
pub fn wildcard_match(name: &str, pattern: &str) -> bool {
    wildcard_match_inner(name, pattern, false)
}

/// [`wildcard_match`] where `#` also matches one digit (or a literal `#`), as
/// the selection filters read it.
pub fn wildcard_match_with_digits(name: &str, pattern: &str) -> bool {
    wildcard_match_inner(name, pattern, true)
}

fn wildcard_match_inner(name: &str, pattern: &str, digits: bool) -> bool {
    let n: Vec<char> = name.chars().flat_map(|c| c.to_lowercase()).collect();
    let p: Vec<char> = pattern.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut si = 0usize;
    let mut pi = 0usize;
    let mut star: Option<usize> = None;
    let mut match_idx = 0usize;
    while si < n.len() {
        // `*` first: a name may hold a literal `*` (anonymous blocks).
        if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            match_idx = si;
            pi += 1;
        } else if pi < p.len()
            && (p[pi] == '?'
                || p[pi] == n[si]
                || (digits && p[pi] == '#' && n[si].is_ascii_digit()))
        {
            si += 1;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            match_idx += 1;
            si = match_idx;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}
