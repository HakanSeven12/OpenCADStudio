//! Web URL deep links (`?file=`).
//!
//! The web build had no way to share or embed a link that opens a drawing:
//! `location.search` was never read. `?file=<value>` now opens a drawing at
//! boot, where `<value>` is a URL-encoded `http(s)` URL to fetch or a bare
//! drawing name to look up in the OPFS recent copies.
//!
//! The pure half (query parsing, scheme gate, trailing-name extraction) is
//! target-independent and unit-tested on the native host, mirroring how
//! `web_autosave` keeps its comparison logic testable; the fetch/open half is
//! wasm-only. Security mirrors the remote-raster fetch: only `http(s)` is
//! fetched (never `file:`, `javascript:`, or any other scheme — refused with
//! a console warning), the body is read with the same capped streaming reader
//! (64 MB cap, 8 s deadline, honest network/CORS/size/status classification),
//! and the URL must end in a `.dwg`/`.dxf` name like a drop. Fetched bytes
//! then enter the guarded drop-open queue (`web_drop::enqueue_open`), so the
//! deep link cannot bypass the open slot guard or the parse + recovery
//! machinery every other open goes through.

/// What `?file=` asked for, after validation.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// An absolute `http(s)` URL whose trailing path segment is a
    /// `.dwg`/`.dxf` name; `name` is that decoded segment.
    Url { url: String, name: String },
    /// A bare drawing name (no scheme) — resolved against the OPFS recent
    /// copies, the same store `open_recent_web` reads.
    RecentName(String),
}

/// Why a present `?file=` value was refused.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRefusal {
    /// A scheme other than `http`/`https` (`file:`, `javascript:`, `data:`,
    /// or a Windows drive-letter colon) — the same gate the remote-image
    /// fetch applies; never fetched, only warned about.
    Scheme,
    /// Started with `http(s)://` but did not parse as an absolute URL.
    BadUrl,
    /// Parsed, but the trailing path segment is not a `.dwg`/`.dxf` name.
    NotDrawing,
}

/// Result of parsing the page's `location.search` for the deep-link param.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedLink {
    /// No (non-empty) `file` param — boot proceeds normally.
    Absent,
    /// A validated target to open.
    Target(LinkTarget),
    /// Present but refused; the raw value is kept for the console warning.
    Ignored(LinkRefusal, String),
}

/// Extract and validate the `file` query parameter from a raw `search`
/// string (with or without the leading `?`). Values are form-decoded
/// (`%XX` and `+`), trimmed, and scheme-gated: `http(s)` URLs must parse and
/// end in a `.dwg`/`.dxf` path segment; any other scheme-like prefix is
/// refused (never dereferenced); anything else is treated as a bare drawing
/// name for the OPFS recent-copy lookup.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub fn parse_link(search: &str) -> ParsedLink {
    let search = search.strip_prefix('?').unwrap_or(search);
    // `form_urlencoded` yields `Cow<str>` pairs in order; the FIRST `file`
    // param decides — a duplicated one must not turn into two opens.
    let value = url::form_urlencoded::parse(search.as_bytes())
        .find(|(key, _)| key == "file")
        .map(|(_, value)| value.into_owned());
    let Some(value) = value else {
        return ParsedLink::Absent;
    };
    let value = value.trim();
    if value.is_empty() {
        return ParsedLink::Absent;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let parsed = match url::Url::parse(value) {
            Ok(parsed) => parsed,
            Err(_) => return ParsedLink::Ignored(LinkRefusal::BadUrl, value.to_string()),
        };
        return match trailing_drawing_name(&parsed) {
            Some(name) => ParsedLink::Target(LinkTarget::Url {
                url: parsed.to_string(),
                name,
            }),
            None => ParsedLink::Ignored(LinkRefusal::NotDrawing, value.to_string()),
        };
    }
    if let Some(_scheme) = scheme_prefix(value) {
        // Never dereference: the gate the remote-image fetch applies, so a
        // `file:`/`javascript:`/drive-letter value dies here, warned only.
        return ParsedLink::Ignored(LinkRefusal::Scheme, value.to_string());
    }
    ParsedLink::Target(LinkTarget::RecentName(value.to_string()))
}

/// The console warning for a refused param (mirrors the drop queues'
/// "ignored …" warnings, which also only reach the console).
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // unit-tested on native
pub fn refusal_warning(refusal: LinkRefusal, value: &str) -> String {
    match refusal {
        LinkRefusal::Scheme => {
            format!("ocs: ignored ?file=\"{value}\" (only http(s) URLs are fetched)")
        }
        LinkRefusal::BadUrl => {
            format!("ocs: ignored ?file=\"{value}\" (not a valid absolute URL)")
        }
        LinkRefusal::NotDrawing => {
            format!("ocs: ignored ?file=\"{value}\" (URL must end in a .dwg/.dxf name)")
        }
    }
}

/// RFC 3986 scheme prefix (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`)
/// of `value`, if any. A single letter before a colon matches (so
/// `C:\drawings\plan.dwg` is refused as a scheme, which is correct on the
/// web: a native path is meaningless there and must not become a fetch).
fn scheme_prefix(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    if bytes.is_empty() || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let end = bytes.iter().position(|&byte| byte == b':')?;
    let scheme = &value[..end];
    let valid = !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
    valid.then(|| scheme.to_string())
}

/// The URL's trailing path segment, percent-decoded, when it is a
/// `.dwg`/`.dxf` drawing name. Same extension convention as the drop filter
/// (`web_drop::is_drawing_name`): a dot-less segment is its own extension.
fn trailing_drawing_name(url: &url::Url) -> Option<String> {
    // `Url::path()` keeps the percent-encoding, so decode it here; query and
    // fragment are already excluded by `path()`.
    let segment = percent_decode(url.path().rsplit('/').next().unwrap_or_default());
    if !is_drawing_name(&segment) {
        return None;
    }
    Some(segment)
}

/// Same convention as `web_drop::is_drawing_name` (which lives in the
/// wasm-gated drop module, so the rule is restated here for the pure parser).
fn is_drawing_name(name: &str) -> bool {
    matches!(
        name.rsplit('.')
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "dwg" | "dxf"
    )
}

/// Decode `%XX` escapes (NOT `+`, which path segments do not carry). Invalid
/// or truncated escapes pass through verbatim; undecodable bytes become
/// U+FFFD via lossy conversion — a name is display data, never a path.
fn percent_decode(value: &str) -> String {
    fn hex(byte: u8) -> Option<u8> {
        match byte {
            b'0'..=b'9' => Some(byte - b'0'),
            b'a'..=b'f' => Some(byte - b'a' + 10),
            b'A'..=b'F' => Some(byte - b'A' + 10),
            _ => None,
        }
    }
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escape = (bytes[index] == b'%' && index + 2 < bytes.len()).then(|| {
            (hex(bytes[index + 1]), hex(bytes[index + 2]))
        });
        match escape {
            Some((Some(high), Some(low))) => {
                out.push(high * 16 + low);
                index += 3;
            }
            _ => {
                out.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ── Web open path ──────────────────────────────────────────────────────────

/// How a deep link settled; the `WebLinkSettled` update arm surfaces it.
/// `pub` to match `WebAutosaveEntry` (public type in a `pub(crate)` module),
/// keeping the `Message` payload free of privacy warnings.
#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone)]
pub enum LinkOutcome {
    /// The bytes entered the guarded drop-open queue — the 50 ms poll opens
    /// them exactly like a dropped file.
    Queued,
    /// Command-line notice (no drawing opened, nothing failed): the named
    /// drawing is not in this browser's storage.
    Info(String),
    /// Command-line error: the fetch was refused or failed.
    Error(String),
}

/// Resolve the boot deep link: fetch the URL with the same capped streaming
/// reader the remote-image fetch uses, or read the OPFS recent copy, then
/// queue the bytes for the guarded open poll. Errors render the honest
/// reason (network/CORS, HTTP status, size cap, deadline).
#[cfg(target_arch = "wasm32")]
pub(crate) async fn open_link(target: LinkTarget) -> LinkOutcome {
    match target {
        LinkTarget::Url { url, name } => {
            match crate::scene::model::image_model::fetch_url_bytes(&url).await {
                Ok(bytes) => enqueue_or_full(name, bytes),
                Err(error) => LinkOutcome::Error(fetch_error_text(&url, error)),
            }
        }
        LinkTarget::RecentName(name) => match super::web_recent::read(&name).await {
            Ok(bytes) => enqueue_or_full(name, bytes),
            Err(_) => LinkOutcome::Info(format!(
                "\"{name}\" is not in this browser's recent drawings; open or drop it once and the link can reopen it."
            )),
        },
    }
}

/// Hand the bytes to the guarded open queue (the same door a drop uses), or
/// report a full queue — the fetch succeeded, so the loss must be visible.
#[cfg(target_arch = "wasm32")]
fn enqueue_or_full(name: String, bytes: Vec<u8>) -> LinkOutcome {
    if super::web_drop::enqueue_open(name.clone(), bytes) {
        LinkOutcome::Queued
    } else {
        LinkOutcome::Error(format!(
            "Deep-linked drawing \"{name}\" could not be opened: the open queue is full."
        ))
    }
}

/// Plain-English twin of the remote-image failure notes (those strings are
/// built inline in the poll arm and mention images); the classification
/// itself is shared via `RemoteFetchError`.
#[cfg(target_arch = "wasm32")]
fn fetch_error_text(url: &str, error: crate::scene::model::image_model::RemoteFetchError) -> String {
    use crate::scene::model::image_model::RemoteFetchError;
    match error {
        RemoteFetchError::Network => format!(
            "Deep-linked drawing \"{url}\" could not be fetched: the network request failed (offline, DNS, or CORS)."
        ),
        RemoteFetchError::Status(status) => format!(
            "Deep-linked drawing \"{url}\" could not be fetched: the server returned HTTP {status}."
        ),
        RemoteFetchError::TooLarge => format!(
            "Deep-linked drawing \"{url}\" was not opened: it exceeds the 64 MB remote-file limit."
        ),
        RemoteFetchError::Timeout => format!(
            "Deep-linked drawing \"{url}\" could not be fetched: the request timed out."
        ),
    }
}

/// Whether `url` shares the page's origin. Unparseable URLs (or a missing
/// window) read as foreign, so the trust notice errs on the loud side.
#[cfg(target_arch = "wasm32")]
pub(crate) fn same_origin(url: &str) -> bool {
    let Some(page) = web_sys::window().and_then(|window| window.location().origin().ok()) else {
        return false;
    };
    match url::Url::parse(url) {
        // `ascii_serialization` and `location.origin()` agree on omitting
        // default ports, so the string comparison is exact.
        Ok(parsed) => parsed.origin().ascii_serialization() == page,
        Err(_) => false,
    }
}

/// The origin to show in the trust notice — the URL's own origin, falling
/// back to the full URL when it cannot be rendered.
#[cfg(target_arch = "wasm32")]
pub(crate) fn display_origin(url: &str) -> Option<String> {
    url::Url::parse(url)
        .ok()
        .map(|parsed| parsed.origin().ascii_serialization())
        .filter(|origin| !origin.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url_target(search: &str) -> LinkTarget {
        match parse_link(search) {
            ParsedLink::Target(target) => target,
            other => panic!("expected a target, got {other:?}"),
        }
    }

    #[test]
    fn absent_when_there_is_no_file_param() {
        assert_eq!(parse_link(""), ParsedLink::Absent);
        assert_eq!(parse_link("?"), ParsedLink::Absent);
        assert_eq!(parse_link("?other=plan.dwg"), ParsedLink::Absent);
        assert_eq!(parse_link("?file="), ParsedLink::Absent);
        assert_eq!(parse_link("?file=%20"), ParsedLink::Absent);
        // Only the exact lowercase key opens; `File=` is not a deep link.
        assert_eq!(parse_link("?File=plan.dwg"), ParsedLink::Absent);
        // The first occurrence wins — a duplicate must not open two files.
        assert_eq!(
            parse_link("?file=&file=plan.dwg"),
            ParsedLink::Absent,
            "an empty first value is absent, not skipped"
        );
    }

    #[test]
    fn http_urls_are_targets_with_their_trailing_name() {
        assert_eq!(
            url_target("?file=https%3A%2F%2Fexample.com%2Fplans%2Fsite.dwg"),
            LinkTarget::Url {
                url: "https://example.com/plans/site.dwg".to_string(),
                name: "site.dwg".to_string()
            }
        );
        // Unencoded URL in the query string works too (senders should
        // encode, but browsers hand the raw text through).
        assert_eq!(
            url_target("?file=http://example.com/a.DXF"),
            LinkTarget::Url {
                url: "http://example.com/a.DXF".to_string(),
                name: "a.DXF".to_string()
            }
        );
        // Scheme check is case-insensitive; Url normalizes to lowercase.
        assert_eq!(
            url_target("?file=HTTPS://example.com/x.dwg"),
            LinkTarget::Url {
                url: "https://example.com/x.dwg".to_string(),
                name: "x.dwg".to_string()
            }
        );
        // Query and fragment never leak into the extracted name.
        assert_eq!(
            url_target("?file=https%3A%2F%2Fexample.com%2Fp.dwg%3Fv%3D1%23frag"),
            LinkTarget::Url {
                url: "https://example.com/p.dwg?v=1#frag".to_string(),
                name: "p.dwg".to_string()
            }
        );
        // Percent-encoded path segments decode into the tab name.
        assert_eq!(
            url_target("?file=https%3A%2F%2Fexample.com%2Fmy%2520plan.dwg"),
            LinkTarget::Url {
                // The param decodes to ".../my%20plan.dwg", which the URL
                // keeps encoded in its path (a raw space is invalid there).
                url: "https://example.com/my%20plan.dwg".to_string(),
                name: "my plan.dwg".to_string()
            }
        );
    }

    #[test]
    fn non_http_schemes_are_refused_not_fetched() {
        for bad in [
            "file:///C:/plan.dwg",
            "javascript:alert(1)",
            "data:text/plain,hello",
            "ftp://example.com/plan.dwg",
            "blob:https://example.com/uuid",
            // A drive-letter colon is a valid single-letter scheme; a native
            // path is meaningless on the web and must not become a fetch.
            "C:\\drawings\\plan.dwg",
        ] {
            assert_eq!(
                parse_link(&format!("?file={bad}")),
                ParsedLink::Ignored(LinkRefusal::Scheme, bad.to_string()),
                "{bad} must be refused as a scheme"
            );
        }
        // The refusal names the reason and echoes the value.
        assert_eq!(
            refusal_warning(LinkRefusal::Scheme, "javascript:alert(1)"),
            "ocs: ignored ?file=\"javascript:alert(1)\" (only http(s) URLs are fetched)"
        );
    }

    #[test]
    fn http_urls_must_end_in_a_drawing_name() {
        // No trailing segment, a directory URL, or a foreign extension.
        for search in [
            "?file=https%3A%2F%2Fexample.com%2F",
            "?file=https%3A%2F%2Fexample.com%2Fplans%2F",
            "?file=https%3A%2F%2Fexample.com%2Fplan.pdf",
            "?file=https%3A%2F%2Fexample.com%2Fplan.dwg.txt",
            // The extension check sees through dot-doubling tricks.
            "?file=https%3A%2F%2Fexample.com%2F..dwgx",
        ] {
            assert!(
                matches!(parse_link(search), ParsedLink::Ignored(LinkRefusal::NotDrawing, _)),
                "{search} must be refused as not a drawing"
            );
        }
        // A dot-less segment is its own extension, mirroring the drop filter.
        assert!(matches!(
            url_target("?file=https%3A%2F%2Fexample.com%2FDWG"),
            LinkTarget::Url { name, .. } if name == "DWG"
        ));
    }

    #[test]
    fn malformed_http_urls_are_refused() {
        assert!(matches!(
            parse_link("?file=http%3A%2F%2F"),
            ParsedLink::Ignored(LinkRefusal::BadUrl, _)
        ));
    }

    #[test]
    fn bare_names_target_the_recent_store() {
        assert_eq!(
            url_target("?file=plan.dwg"),
            LinkTarget::RecentName("plan.dwg".to_string())
        );
        // Encoded spaces and non-ASCII decode into the name.
        assert_eq!(
            url_target("?file=my+plan.dxf"),
            LinkTarget::RecentName("my plan.dxf".to_string())
        );
        assert_eq!(
            url_target("?file=%C3%A7izim.dwg"),
            LinkTarget::RecentName("çizim.dwg".to_string())
        );
        // No scheme and no colon, but slashes stay a name — the recent store
        // keys by the hashed name, so there is nothing to traverse.
        assert_eq!(
            url_target("?file=sub%2Fplan.dwg"),
            LinkTarget::RecentName("sub/plan.dwg".to_string())
        );
    }

    #[test]
    fn percent_decode_only_touches_valid_escapes() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("%2Fd%2E"), "/d.");
        // Truncated and invalid escapes pass through verbatim.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("%2"), "%2");
        // Non-ASCII text survives untouched; bad UTF-8 escapes go lossy.
        assert_eq!(percent_decode("çizim.dwg"), "çizim.dwg");
        assert_eq!(percent_decode("%ff"), "\u{fffd}");
    }

    #[test]
    fn scheme_prefix_matches_rfc3986_shape_only() {
        assert_eq!(scheme_prefix("https://x"), Some("https".to_string()));
        assert_eq!(scheme_prefix("c:\\x"), Some("c".to_string()));
        assert_eq!(
            scheme_prefix("x1+2-3.4:rest"),
            Some("x1+2-3.4".to_string())
        );
        // A colon after a non-scheme character (space, slash, digit start)
        // is not a scheme.
        assert_eq!(scheme_prefix("plan.dwg"), None);
        assert_eq!(scheme_prefix("a b:c"), None);
        assert_eq!(scheme_prefix("1http://x"), None);
        assert_eq!(scheme_prefix("no-colon"), None);
    }
}
