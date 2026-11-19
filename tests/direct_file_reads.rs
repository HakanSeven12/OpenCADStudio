//! A guard against new direct file reads in `src/`.
//!
//! A file read that runs on the UI thread stops the window while it reads, and
//! a panic in it stops the app. A read of a user's file belongs on a worker
//! (`background_task` in `src/app/update/file.rs`), which catches the panic
//! and keeps the window alive.
//!
//! This test cannot know which thread a call runs on. It counts the direct
//! read calls in each production source file and compares each count with
//! `tests/fixtures/direct_file_reads/allowed.txt`:
//!
//! - a count above its number in the list fails, and the message says what to
//!   do;
//! - a count below its number passes, as does a listed file that is gone, so
//!   that removing a read never fails;
//! - `the_list_has_no_unused_allowance` (ignored; run it with `--ignored`)
//!   lists the numbers that can go down.
//!
//! Comments, string literals and test code are not counted: files named
//! `*_tests.rs`, `*_test.rs`, `tests.rs` or `test.rs`, folders named `tests`,
//! items behind `#[test]` or a `cfg` that needs `test`, and the files of
//! modules declared behind such a `cfg`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The list of allowed counts, from the root of the repository.
const LIST: &str = "tests/fixtures/direct_file_reads/allowed.txt";

/// The calls that are counted, each as the last segments of its path:
/// `std::fs::read(..)` and `fs::read(..)` are both `fs::read`.
const CALLS: [&[&str]; 6] = [
    &["fs", "read"],
    &["fs", "read_to_string"],
    &["File", "open"],
    &["image", "open"],
    &["ImageReader", "open"],
    &["Mmap", "map"],
];

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    /// `::`
    Sep,
    Punct(char),
}

/// The tokens of Rust source with their 1-based lines. Comments are dropped,
/// and a string or character literal is one token, so nothing inside them is
/// read as code.
fn tokens(source: &str) -> Vec<(Tok, usize)> {
    let c: Vec<char> = source.chars().collect();
    let mut out = Vec::new();
    let mut line = 1;
    let mut i = 0;
    let at = |i: usize| c.get(i).copied().unwrap_or('\0');
    while i < c.len() {
        let ch = c[i];
        if ch == '\n' {
            line += 1;
            i += 1;
        } else if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && at(i + 1) == '/' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && at(i + 1) == '*' {
            let mut depth = 1;
            i += 2;
            while i < c.len() && depth > 0 {
                if c[i] == '/' && at(i + 1) == '*' {
                    depth += 1;
                    i += 2;
                } else if c[i] == '*' && at(i + 1) == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    if c[i] == '\n' {
                        line += 1;
                    }
                    i += 1;
                }
            }
        } else if ch == '"' {
            let start = line;
            let mut text = String::new();
            i += 1;
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' && i + 1 < c.len() {
                    text.push(c[i]);
                    i += 1;
                }
                if c[i] == '\n' {
                    line += 1;
                }
                text.push(c[i]);
                i += 1;
            }
            i += 1;
            out.push((Tok::Str(text), start));
        } else if ch == '\'' {
            if at(i + 1) == '\\' {
                // An escaped character literal: to its closing quote.
                i += 3;
                while i < c.len() && c[i] != '\'' && c[i] != '\n' {
                    i += 1;
                }
                i += 1;
                out.push((Tok::Str(String::new()), line));
            } else if at(i + 2) == '\'' {
                i += 3;
                out.push((Tok::Str(String::new()), line));
            } else {
                // A lifetime or a label: its name follows as an identifier.
                i += 1;
            }
        } else if ch.is_alphanumeric() || ch == '_' {
            let start = i;
            while i < c.len() && (c[i].is_alphanumeric() || c[i] == '_') {
                i += 1;
            }
            let word: String = c[start..i].iter().collect();
            let raw = matches!(word.as_str(), "r" | "br" | "cr");
            let prefix = raw || matches!(word.as_str(), "b" | "c");
            let mut hashes = 0;
            while raw && at(i + hashes) == '#' {
                hashes += 1;
            }
            if raw && at(i + hashes) == '"' {
                // A raw string: to a quote that the same number of `#` follow.
                let start = line;
                let mut text = String::new();
                i += hashes + 1;
                while i < c.len() {
                    if c[i] == '"' && (1..=hashes).all(|k| at(i + k) == '#') {
                        break;
                    }
                    if c[i] == '\n' {
                        line += 1;
                    }
                    text.push(c[i]);
                    i += 1;
                }
                i += hashes + 1;
                out.push((Tok::Str(text), start));
            } else if prefix && hashes == 0 && (at(i) == '"' || at(i) == '\'') {
                // `b"…"`, `c"…"`, `b'…'`: the literal follows as its own token.
            } else {
                out.push((Tok::Ident(word), line));
            }
        } else if ch == ':' && at(i + 1) == ':' {
            out.push((Tok::Sep, line));
            i += 2;
        } else {
            out.push((Tok::Punct(ch), line));
            i += 1;
        }
    }
    out
}

fn is(tokens: &[(Tok, usize)], i: usize, ch: char) -> bool {
    matches!(tokens.get(i), Some((Tok::Punct(c), _)) if *c == ch)
}

fn ident(tokens: &[(Tok, usize)], i: usize) -> Option<&str> {
    match tokens.get(i) {
        Some((Tok::Ident(name), _)) => Some(name),
        _ => None,
    }
}

/// The index after the bracket that closes the one at `open`.
fn after_close(tokens: &[(Tok, usize)], open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while i < tokens.len() {
        if let Tok::Punct(ch) = tokens[i].0 {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return i + 1;
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    tokens.len()
}

/// Does the `cfg` predicate that starts at `i` hold only in a test build?
/// Returns the answer and the index after the predicate.
fn needs_test(tokens: &[(Tok, usize)], i: usize) -> (bool, usize) {
    let Some(name) = ident(tokens, i) else {
        return (false, i + 1);
    };
    if !is(tokens, i + 1, '(') {
        // `test`, or `feature = "x"`.
        let mut end = i + 1;
        if is(tokens, end, '=') {
            end += 2;
        }
        return (name == "test", end);
    }
    let end = after_close(tokens, i + 1);
    let mut members = Vec::new();
    let mut j = i + 2;
    while j + 1 < end {
        let (member, next) = needs_test(tokens, j);
        members.push(member);
        j = next;
        if is(tokens, j, ',') {
            j += 1;
        }
    }
    let answer = match name {
        "all" => members.iter().any(|m| *m),
        "any" => !members.is_empty() && members.iter().all(|m| *m),
        _ => false,
    };
    (answer, end)
}

/// Is the attribute whose tokens are `inner` (between `[` and `]`) one that
/// marks test code: `test`, a path that ends in `test` (`tokio::test`), or a
/// `cfg` that needs `test`?
fn marks_test(inner: &[(Tok, usize)]) -> bool {
    if ident(inner, 0) == Some("cfg") && is(inner, 1, '(') {
        return needs_test(inner, 2).0;
    }
    let path_only = inner
        .iter()
        .all(|(t, _)| matches!(t, Tok::Ident(_) | Tok::Sep));
    path_only && ident(inner, inner.len().saturating_sub(1)) == Some("test")
}

/// A module declared without a body behind a test attribute: its files are
/// test code.
#[derive(Debug, PartialEq)]
struct TestModule {
    name: String,
    /// The value of a `#[path = "…"]` attribute on it.
    path: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
struct Scan {
    /// The line of each counted call.
    lines: Vec<usize>,
    test_modules: Vec<TestModule>,
}

/// The direct read calls in the production code of one source file.
fn scan(source: &str) -> Scan {
    let t = tokens(source);
    let mut found = Scan::default();
    let mut i = 0;
    while i < t.len() {
        if is(&t, i, '#') {
            // A run of attributes, then the item they belong to.
            let mut test = false;
            let mut path = None;
            let mut j = i;
            loop {
                let inner_attr = is(&t, j + 1, '!') && is(&t, j + 2, '[');
                let open = if inner_attr { j + 2 } else { j + 1 };
                if !is(&t, j, '#') || !is(&t, open, '[') {
                    break;
                }
                let end = after_close(&t, open);
                let inner = &t[open + 1..end.saturating_sub(1).max(open + 1)];
                if marks_test(inner) {
                    if inner_attr {
                        // `#![cfg(test)]`: the rest of the file is test code.
                        return found;
                    }
                    test = true;
                }
                if ident(inner, 0) == Some("path") && is(inner, 1, '=') {
                    if let Some((Tok::Str(value), _)) = inner.get(2) {
                        path = Some(value.clone());
                    }
                }
                j = end;
            }
            if j == i {
                i += 1;
                continue;
            }
            if !test {
                i = j;
                continue;
            }
            // Skip the item: to its `;`, or to the end of its first block.
            let mut k = j;
            let mut depth = 0usize;
            while k < t.len() {
                match t[k].0 {
                    Tok::Punct('{') if depth == 0 => {
                        k = after_close(&t, k);
                        break;
                    }
                    Tok::Punct(';') if depth == 0 => {
                        let name = (k >= 2 && ident(&t, k - 2) == Some("mod"))
                            .then(|| ident(&t, k - 1))
                            .flatten();
                        if let Some(name) = name {
                            found.test_modules.push(TestModule {
                                name: name.to_string(),
                                path: path.take(),
                            });
                        }
                        k += 1;
                        break;
                    }
                    Tok::Punct('(' | '[') => depth += 1,
                    Tok::Punct(')' | ']') => depth = depth.saturating_sub(1),
                    _ => {}
                }
                k += 1;
            }
            i = k;
            continue;
        }
        for call in CALLS {
            let last = i + 2 * (call.len() - 1);
            let names = call
                .iter()
                .enumerate()
                .all(|(n, name)| ident(&t, i + 2 * n) == Some(*name));
            let seps =
                (0..call.len() - 1).all(|n| t.get(i + 2 * n + 1).map(|x| &x.0) == Some(&Tok::Sep));
            if names && seps && is(&t, last + 1, '(') {
                found.lines.push(t[i].1);
            }
        }
        i += 1;
    }
    found
}

/// Is this path test code by its name alone?
fn test_by_name(relative: &Path) -> bool {
    let file = relative.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let in_tests_folder = relative
        .parent()
        .is_some_and(|p| p.components().any(|c| c.as_os_str() == "tests"));
    in_tests_folder
        || file.ends_with("_tests.rs")
        || file.ends_with("_test.rs")
        || file == "tests.rs"
        || file == "test.rs"
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot list {}: {e}", dir.display()))
        .map(|entry| entry.expect("a folder entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// A path as the list writes it: from the root, with `/` on every system.
fn listed_name(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// The lines of the direct read calls in each production file under
/// `root/src`, for the files that have one.
fn direct_reads(root: &Path) -> BTreeMap<String, Vec<usize>> {
    let mut files = Vec::new();
    rust_files(&root.join("src"), &mut files);
    let mut counts = BTreeMap::new();
    let mut test_files = BTreeSet::new();
    let mut test_folders = Vec::new();
    for path in &files {
        if test_by_name(path.strip_prefix(root).unwrap_or(path)) {
            continue;
        }
        let bytes =
            std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let found = scan(&String::from_utf8_lossy(&bytes));
        let folder = path.parent().unwrap_or(root);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        // Where the file's child modules are.
        let children = match stem {
            "mod" | "lib" | "main" => folder.to_path_buf(),
            _ => folder.join(stem),
        };
        for module in found.test_modules {
            match module.path {
                Some(value) => {
                    let mut target = folder.to_path_buf();
                    target.extend(value.split(['/', '\\']));
                    test_files.insert(listed_name(root, &target));
                }
                None => {
                    test_files.insert(listed_name(
                        root,
                        &children.join(format!("{}.rs", module.name)),
                    ));
                    test_folders.push(format!(
                        "{}/",
                        listed_name(root, &children.join(&module.name))
                    ));
                }
            }
        }
        if !found.lines.is_empty() {
            counts.insert(listed_name(root, path), found.lines);
        }
    }
    counts.retain(|name, _| {
        !test_files.contains(name) && !test_folders.iter().any(|folder| name.starts_with(folder))
    });
    counts
}

/// The list: `<count> <path>` on each line; `#` starts a comment.
fn allowed(text: &str) -> Result<BTreeMap<String, usize>, String> {
    let mut list = BTreeMap::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let entry = line
            .split_once(char::is_whitespace)
            .and_then(|(count, path)| Some((count.parse::<usize>().ok()?, path.trim())));
        let Some((count, path)) = entry else {
            return Err(format!(
                "{LIST}:{}: expected `<count> <path>`, found `{line}`",
                n + 1
            ));
        };
        if list.insert(path.to_string(), count).is_some() {
            return Err(format!(
                "{LIST}:{}: `{path}` is in the list two times",
                n + 1
            ));
        }
    }
    Ok(list)
}

/// One line for each file that has more direct reads than the list allows.
fn above(reads: &BTreeMap<String, Vec<usize>>, list: &BTreeMap<String, usize>) -> Vec<String> {
    reads
        .iter()
        .filter_map(|(file, lines)| {
            let limit = list.get(file).copied().unwrap_or(0);
            let at: Vec<String> = lines.iter().map(usize::to_string).collect();
            (lines.len() > limit).then(|| {
                format!(
                    "  {file}: {} direct file read(s), on line(s) {}; the list allows {limit}",
                    lines.len(),
                    at.join(", ")
                )
            })
        })
        .collect()
}

/// A GitHub Actions error annotation for each file above its number, so that
/// a pull request shows the message on the file, not only in the job's log.
/// GitHub shows at most ten for a step.
fn annotations(
    reads: &BTreeMap<String, Vec<usize>>,
    list: &BTreeMap<String, usize>,
) -> Vec<String> {
    reads
        .iter()
        .filter(|(file, lines)| lines.len() > list.get(*file).copied().unwrap_or(0))
        .take(10)
        .map(|(file, lines)| {
            let limit = list.get(file).copied().unwrap_or(0);
            format!(
                "::error file={file},line={},title=New direct file read::{file} has {} direct file read(s); \
                 {LIST} allows {limit}. Run a read of a user's file through background_task \
                 (src/app/update/file.rs). If the read is already on a worker, or reads a small file of the app \
                 itself, raise the number in the list and give the reason.",
                lines.last().copied().unwrap_or(1),
                lines.len()
            )
        })
        .collect()
}

/// One line for each number in the list that can go down.
fn unused(reads: &BTreeMap<String, Vec<usize>>, list: &BTreeMap<String, usize>) -> Vec<String> {
    list.iter()
        .filter_map(|(file, limit)| {
            let now = reads.get(file).map_or(0, Vec::len);
            (now < *limit).then(|| format!("  {file}: the list allows {limit}, the file has {now}"))
        })
        .collect()
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The direct reads under `root/src`, and the list at `root`.
fn reads_and_list(root: &Path) -> (BTreeMap<String, Vec<usize>>, BTreeMap<String, usize>) {
    let mut path = root.to_path_buf();
    path.extend(LIST.split('/'));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let list = allowed(&text).unwrap_or_else(|problem| panic!("{problem}"));
    (direct_reads(root), list)
}

#[test]
fn no_source_file_has_more_direct_file_reads_than_the_list_allows() {
    let (reads, list) = reads_and_list(&root());
    let found = above(&reads, &list);
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        for annotation in annotations(&reads, &list) {
            println!("{annotation}");
        }
    }
    assert!(
        found.is_empty(),
        "\nNew direct file reads in src/:\n{}\n\n\
         A file read on the UI thread stops the window while it reads, and a panic in it stops the app.\n\
         - If the file comes from the user (a drawing, an import, a picture, a font, a list), run the read\n  \
           through `background_task` (src/app/update/file.rs); `read_picture_with` and its callers show the pattern.\n\
         - If the read already runs on a worker, or reads a small file of the app itself (a setting, a lock,\n  \
           a cache), raise that file's number in {LIST} and give the reason after a `#`.\n\
         This test counts calls in each file; it cannot know the thread.\n",
        found.join("\n")
    );
}

#[test]
#[ignore = "housekeeping: lists the numbers in the allowed list that can go down"]
fn the_list_has_no_unused_allowance() {
    let (reads, list) = reads_and_list(&root());
    let found = unused(&reads, &list);
    assert!(
        found.is_empty(),
        "\nThese numbers in {LIST} can go down (remove a line whose file has no direct read):\n{}\n",
        found.join("\n")
    );
}

#[test]
fn the_scan_finds_this_repository_s_source() {
    // A scan that reads nothing would pass the guard without checking anything.
    let mut files = Vec::new();
    rust_files(&root().join("src"), &mut files);
    assert!(
        files.len() > 100,
        "only {} .rs files under src/",
        files.len()
    );
    assert!(
        !direct_reads(&root()).is_empty(),
        "no direct file read found in src/: the scan does not see the source"
    );
}

fn lines_of(source: &str) -> Vec<usize> {
    scan(source).lines
}

#[test]
fn each_listed_call_is_counted_on_its_line() {
    let source = "fn f(p: &Path) {\n\
                  let a = std::fs::read(p);\n\
                  let b = fs::read_to_string(p);\n\
                  let c = std::fs::File::open(p);\n\
                  let d = File::open(p);\n\
                  let e = image::open(p);\n\
                  let f = image::ImageReader::open(p);\n\
                  let g = unsafe { memmap2::Mmap::map(&file) };\n\
                  }\n";
    assert_eq!(lines_of(source), vec![2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn a_call_split_over_lines_or_spaced_is_counted() {
    assert_eq!(lines_of("let a = std::fs::read\n    (p);"), vec![1]);
    assert_eq!(lines_of("let a = fs :: read (p);"), vec![1]);
    assert_eq!(lines_of("a.and_then(|p| std::fs::read(p).ok())"), vec![1]);
}

#[test]
fn other_calls_and_names_are_not_counted() {
    for source in [
        "std::fs::read_dir(p);",
        "std::fs::read_link(p);",
        "std::fs::write(p, b);",
        "File::create(p);",
        "MyFile::open(p);",
        "offs::read(p);",
        "self.fs.read(p);",
        "fs::read_to_string_lossy(p);",
        "use std::fs::read;",
        "let read = fs::read;",
    ] {
        assert_eq!(lines_of(source), Vec::<usize>::new(), "{source}");
    }
}

#[test]
fn comments_and_literals_are_not_counted() {
    for source in [
        "// std::fs::read(p)\n",
        "/// Uses `std::fs::read(p)`.\nfn f() {}\n",
        "/* std::fs::read(p) /* nested */ fs::read(p) */\n",
        "let s = \"std::fs::read(p)\";",
        "let s = \"a \\\" std::fs::read(p)\";",
        "let s = r\"std::fs::read(p)\";",
        "let s = r#\"a \" std::fs::read(p)\"#;",
        "let s = br##\"a \"# std::fs::read(p)\"##;",
        "let s = b\"std::fs::read(p)\";",
    ] {
        assert_eq!(lines_of(source), Vec::<usize>::new(), "{source}");
    }
}

#[test]
fn code_after_a_comment_or_a_literal_is_counted() {
    assert_eq!(lines_of("/* a */ std::fs::read(p);"), vec![1]);
    assert_eq!(lines_of("let s = \"a\\\\\"; std::fs::read(p);"), vec![1]);
    assert_eq!(lines_of("let s = r#\"a\"#;\nstd::fs::read(p);"), vec![2]);
    assert_eq!(
        lines_of("let q = '\"'; std::fs::read(p); let r = '\"';"),
        vec![1]
    );
    assert_eq!(lines_of("let q = '\\''; std::fs::read(p);"), vec![1]);
    assert_eq!(
        lines_of("let q = '\\\"'; std::fs::read(p); let r = '\\\"';"),
        vec![1]
    );
    assert_eq!(lines_of("let q = b'\"'; std::fs::read(p);"), vec![1]);
    assert_eq!(
        lines_of("fn f<'a>(p: &'a str) { std::fs::read(p); }"),
        vec![1]
    );
    assert_eq!(lines_of("let r#type = 1; std::fs::read(p);"), vec![1]);
    assert_eq!(lines_of("let s = \"a\nb\";\nstd::fs::read(p);"), vec![3]);
    assert_eq!(
        lines_of("/* a\nb */\nstd::fs::read(p);\r\nfs::read(q);"),
        vec![3, 4]
    );
}

#[test]
fn test_code_is_not_counted() {
    for source in [
        "#[cfg(test)]\nmod tests {\n fn f() { std::fs::read(p); }\n}\n",
        "#[cfg(test)]\nfn helper() { if a { std::fs::read(p); } }\n",
        "#[test]\nfn reads() { std::fs::read(p); }\n",
        "#[tokio::test]\nasync fn reads() { std::fs::read(p); }\n",
        "#[test]\n#[ignore = \"slow\"]\nfn reads() { std::fs::read(p); }\n",
        "#[allow(dead_code)]\n#[cfg(test)]\nfn helper(a: [u8; 2]) { std::fs::read(p); }\n",
        "#[cfg(all(test, not(target_arch = \"wasm32\")))]\nmod tests { fn f() { fs::read(p); } }\n",
        "#[cfg(all(not(target_arch = \"wasm32\"), test))]\nmod tests { fn f() { fs::read(p); } }\n",
        "#[cfg(all(test, feature = \"host\"))]\nmod tests { fn f() { fs::read(p); } }\n",
        "#[cfg(any(test, all(test, windows)))]\nmod tests { fn f() { fs::read(p); } }\n",
        "#[cfg(test)]\nimpl A { fn f() { fs::read(p); } }\n",
        "#[cfg(test)]\nconst A: [u8; 2] = [1, 2];\n",
        "#![cfg(test)]\nfn f() { fs::read(p); }\n",
        "#![cfg(test)]\nfn f() {}\nfn g() { fs::read(p); }\n",
        "#[cfg(test)]\nconst A: [u8; 2] = { fs::read(p); [1, 2] };\n",
    ] {
        assert_eq!(lines_of(source), Vec::<usize>::new(), "{source}");
    }
}

#[test]
fn production_code_next_to_test_code_is_counted() {
    for (source, lines) in [
        (
            "#[cfg(test)]\nmod tests { fn f() { fs::read(p); } }\nfn g() { fs::read(p); }\n",
            vec![3],
        ),
        (
            "#[cfg(test)]\nuse std::fs;\nfn g() { fs::read(p); }\n",
            vec![3],
        ),
        (
            "#[cfg(test)]\nconst A: [u8; 2] = [1, 2];\nfn g() { fs::read(p); }\n",
            vec![3],
        ),
        ("#[cfg(not(test))]\nfn g() { fs::read(p); }\n", vec![2]),
        (
            "#[cfg(all(not(target_arch = \"wasm32\"), not(test)))]\nfn g() { fs::read(p); }\n",
            vec![2],
        ),
        (
            "#[cfg(any(test, not(target_arch = \"wasm32\")))]\nfn g() { fs::read(p); }\n",
            vec![2],
        ),
        (
            "#[cfg(any(target_os = \"windows\", test))]\nfn g() { fs::read(p); }\n",
            vec![2],
        ),
        (
            "#[cfg(feature = \"test\")]\nfn g() { fs::read(p); }\n",
            vec![2],
        ),
        (
            "#[cfg_attr(test, allow(dead_code))]\nfn g() { fs::read(p); }\n",
            vec![2],
        ),
        (
            "#[derive(Debug)]\nstruct A;\nfn g() { fs::read(p); }\n",
            vec![3],
        ),
        ("#![allow(dead_code)]\nfn g() { fs::read(p); }\n", vec![2]),
    ] {
        assert_eq!(lines_of(source), lines, "{source}");
    }
}

#[test]
fn a_module_declared_for_tests_is_reported_with_its_path() {
    let found = scan(
        "#[cfg(test)]\nmod helpers;\n\
         #[cfg(all(test, feature = \"x\"))]\n#[path = \"save_audit.rs\"]\nmod audit;\n\
         #[path = \"real.rs\"]\nmod real;\nmod plain;\n\
         #[cfg(test)]\nuse helpers::a;\n",
    );
    assert_eq!(
        found.test_modules,
        vec![
            TestModule {
                name: "helpers".into(),
                path: None
            },
            TestModule {
                name: "audit".into(),
                path: Some("save_audit.rs".into())
            },
        ]
    );
}

/// A folder under the system's temporary folder, removed when it is dropped.
struct Tree(PathBuf);

impl Tree {
    fn new(name: &str, files: &[(&str, &str)]) -> Self {
        let root =
            std::env::temp_dir().join(format!("ocs-direct-reads-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (file, text) in files {
            let mut path = root.clone();
            path.extend(file.split('/'));
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("a folder");
            std::fs::write(path, text).expect("a file");
        }
        Self(root)
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn test_files_and_the_files_of_test_modules_are_left_out() {
    let read = "fn f() { std::fs::read(p); }\n";
    let tree = Tree::new(
        "files",
        &[
            (
                "src/main.rs",
                "#[cfg(test)]\nmod fixtures;\nmod app;\nfn main() { std::fs::read(p); }\n",
            ),
            ("src/fixtures.rs", read),
            ("src/fixtures/deep.rs", read),
            ("src/fixtures_real.rs", read),
            (
                "src/app/mod.rs",
                "#[cfg(test)]\nmod checks;\n#[cfg(test)]\n#[path = \"odd/name.rs\"]\nmod odd;\n",
            ),
            ("src/app/checks/mod.rs", read),
            ("src/app/odd/name.rs", read),
            ("src/app/odd/other.rs", read),
            (
                "src/app/io.rs",
                "#[cfg(test)]\nmod probe;\nfn f() { fs::read(p); fs::read(q); }\n",
            ),
            ("src/app/io/probe.rs", read),
            ("src/app/io/real.rs", read),
            ("src/app/io_tests.rs", read),
            ("src/app/tests.rs", read),
            ("src/app/tests/a.rs", read),
            ("src/app/notes.txt", read),
            ("crates/other/src/lib.rs", read),
        ],
    );
    let found: Vec<(String, usize)> = direct_reads(&tree.0)
        .into_iter()
        .map(|(file, lines)| (file, lines.len()))
        .collect();
    let expected = [
        ("src/app/io.rs", 2),
        ("src/app/io/real.rs", 1),
        ("src/app/odd/other.rs", 1),
        ("src/fixtures_real.rs", 1),
        ("src/main.rs", 1),
    ];
    assert_eq!(
        found,
        expected.map(|(file, count)| (file.to_string(), count))
    );
}

fn reads_of(entries: &[(&str, usize)]) -> BTreeMap<String, Vec<usize>> {
    entries
        .iter()
        .map(|(file, count)| (file.to_string(), (1..=*count).collect()))
        .collect()
}

#[test]
fn the_list_and_the_source_are_read_from_the_same_root() {
    let tree = Tree::new(
        "root",
        &[
            (
                "src/a.rs",
                "fn f() { fs::read(p); }\nfn g() { fs::read(q); }\n",
            ),
            ("src/b.rs", "fn f() { fs::read(p); }\n"),
            (
                LIST,
                "1 src/a.rs # a lock file\n1 src/b.rs\n2 src/gone.rs\n",
            ),
        ],
    );
    let (reads, list) = reads_and_list(&tree.0);
    let found = above(&reads, &list);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("src/a.rs: 2 direct file read(s), on line(s) 1, 2; the list allows 1")
    );
    assert_eq!(annotations(&reads, &list).len(), 1);
    assert_eq!(unused(&reads, &list).len(), 1);
}

#[test]
fn only_a_count_above_the_list_is_reported() {
    let list = allowed("# comment\n\n2 src/a.rs\n1 src/b.rs  # a lock file\n3 src/gone.rs\n")
        .expect("a list");
    assert_eq!(list.len(), 3);
    // Equal, lower, and a listed file that is gone: nothing.
    assert!(above(&reads_of(&[("src/a.rs", 2), ("src/b.rs", 0)]), &list).is_empty());
    assert!(above(&reads_of(&[("src/a.rs", 1)]), &list).is_empty());
    // One more in a listed file, and a file that is not in the list.
    let found = above(
        &reads_of(&[("src/a.rs", 3), ("src/b.rs", 1), ("src/new.rs", 1)]),
        &list,
    );
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0].contains("src/a.rs: 3 direct file read(s), on line(s) 1, 2, 3; the list allows 2")
    );
    assert!(found[1].contains("src/new.rs: 1 direct file read(s), on line(s) 1; the list allows 0"));
}

#[test]
fn an_annotation_names_the_file_and_is_one_line() {
    let list = allowed("2 src/a.rs\n").expect("a list");
    assert!(annotations(&reads_of(&[("src/a.rs", 2)]), &list).is_empty());
    let found = annotations(&reads_of(&[("src/a.rs", 3), ("src/new.rs", 1)]), &list);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0]
        .starts_with("::error file=src/a.rs,line=3,title=New direct file read::src/a.rs has 3 "));
    assert!(found[1].starts_with("::error file=src/new.rs,line=1,title="));
    assert!(found.iter().all(|a| !a.contains('\n')));
    // GitHub shows at most ten annotations for a step.
    let many: Vec<String> = (0..12).map(|n| format!("src/f{n:02}.rs")).collect();
    let many: Vec<(&str, usize)> = many.iter().map(|f| (f.as_str(), 1)).collect();
    assert_eq!(annotations(&reads_of(&many), &list).len(), 10);
}

#[test]
fn the_housekeeping_check_reports_only_numbers_that_can_go_down() {
    let list = allowed("2 src/a.rs\n1 src/b.rs\n3 src/gone.rs\n").expect("a list");
    let found = unused(&reads_of(&[("src/a.rs", 2), ("src/b.rs", 5)]), &list);
    assert_eq!(
        found,
        vec!["  src/gone.rs: the list allows 3, the file has 0".to_string()]
    );
}

#[test]
fn a_list_line_that_is_not_a_count_and_a_path_is_refused() {
    assert!(allowed("src/a.rs\n").is_err());
    assert!(allowed("two src/a.rs\n").is_err());
    assert!(allowed("1 src/a.rs\n2 src/a.rs\n").is_err());
    assert_eq!(
        allowed("1 src/a.rs\r\n").expect("a list").get("src/a.rs"),
        Some(&1)
    );
}
