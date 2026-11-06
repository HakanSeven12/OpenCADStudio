//! Grid fitting of the pixel-grid icon art for the size it is drawn at.
//!
//! The 16 px and 32 px icon sets are drawn on a pixel grid: strokes are
//! 1 px wide on `.5` coordinates and fills sit on whole pixels. At 100 %
//! and 200 % that grid lands on the screen's pixels, but at 125 %, 150 % or
//! 175 % the scale moves every coordinate off it, so 1 px lines turn into
//! two half-lit pixels (or, drawn crisp, into a mix of 1 px and 2 px lines).
//!
//! [`fit`] rewrites an icon for an exact physical size `N` instead, the way
//! font hinting does for glyphs, before it is rasterised:
//!
//! - Every line gets the same weight `W`: 1 px below 175 %, 2 px from 175 %
//!   up (see [`line_weight`]). A stroke's points move to the centre of the
//!   pixel column the art's pixel lands in (or onto a pixel edge for an even
//!   `W`); curve control points move with their point.
//! - Fill edges, mask and clip edges map through the same grid as the
//!   strokes, so fills and lines keep their order: where one art pixel
//!   becomes two screen pixels a 1 px gap may open beside a line, and from
//!   175 % (2 px lines over 1.75 px art pixels) a line may reach 1 px into
//!   its neighbour, but lines and fills never trade places.
//! - Small filled rectangles (grips) keep one size everywhere —
//!   `round(w × s)` px, never 2 × 3 next to 3 × 3 — centred where they were,
//!   and the mask cut-outs around them keep an even gap on every side.
//!
//! The rewrite is done once per icon, size and theme and cached by the
//! caller. Anything it cannot read yields `None`; the caller then draws the
//! art unsnapped.

use std::fmt::Write as _;

use svgtypes::{SimplePathSegment as Seg, SimplifyingPathParser};

/// The art size (16 or 32) of pixel-grid icon art, read from its root tag.
pub(super) fn art_size(text: &str) -> Option<u32> {
    let root = &text[..text.find('>')?];
    if !root.starts_with("<svg") {
        return None;
    }
    [16, 32]
        .into_iter()
        .find(|n| root.contains(&format!(r#"viewBox="0 0 {n} {n}""#)))
}

/// The weight of every line, in physical px, at scale `s` (physical px per
/// art px): 1 below 1.75, 2 from 1.75 up, and so on.
pub(super) fn line_weight(s: f64) -> u32 {
    ((s + 0.25).floor() as u32).max(1)
}

/// The art (`art` px square) rewritten as `n` × `n` physical px with every
/// edge on a pixel. `None` when the SVG holds anything but the elements the
/// icon sets use, or a path that does not parse.
pub(super) fn fit(text: &str, art: u32, n: u32) -> Option<String> {
    let grid = Grid::new(art, n);
    let grips = collect_grips(text, &grid)?;
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    // Inside a <mask> or <clipPath>.
    let mut hidden = 0usize;
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let after = &rest[open..];
        let close = after.find('>')? + 1;
        let tag = &after[..close];
        let name = tag_name(tag);
        let self_closing = tag.ends_with("/>");
        match name {
            "svg" => out.push_str(&root_tag(tag, &grid)),
            "mask" | "clipPath" => {
                hidden += usize::from(!self_closing);
                out.push_str(&region_tag(tag, &grid));
            }
            "/mask" | "/clipPath" => {
                hidden = hidden.saturating_sub(1);
                out.push_str(tag);
            }
            "path" => {
                let role = Role::of(tag, hidden > 0);
                out.push_str(&path_tag(tag, role, &grid, &grips)?);
            }
            "defs" | "/defs" | "g" | "/g" | "/svg" | "/path" => out.push_str(tag),
            _ => return None,
        }
        rest = &after[close..];
    }
    out.push_str(rest);
    Some(out)
}

/// The map from art coordinates to physical pixels.
struct Grid {
    art: f64,
    n: u32,
    /// Physical px per art px.
    s: f64,
    weight: u32,
}

impl Grid {
    fn new(art: u32, n: u32) -> Self {
        let s = f64::from(n) / f64::from(art);
        Self {
            art: f64::from(art),
            n,
            s,
            weight: line_weight(s),
        }
    }

    fn odd(&self) -> bool {
        self.weight % 2 == 1
    }

    /// Where an edge of the art grid (a whole art pixel, or any fill
    /// vertex) lands: art column `k` covers `[edge(k), edge(k + 1))`.
    fn edge(&self, v: f64) -> f64 {
        if self.odd() {
            ((v + 0.5) * self.s).floor()
        } else {
            (v * self.s).round()
        }
    }

    /// Where a stroke's centre line lands: a stroke through art column `k`
    /// starts at `edge(k)`, so it meets the fills next to it.
    fn line(&self, v: f64) -> f64 {
        if self.odd() {
            (v * self.s).floor() + 0.5
        } else {
            ((v - 0.5) * self.s).round() + f64::from(self.weight / 2)
        }
    }

    /// A small filled rectangle `w` × `h` centred on `(cx, cy)`: its size is
    /// fixed by `w` and `h` alone, so every grip comes out the same.
    fn small_rect(&self, cx: f64, cy: f64, w: f64, h: f64) -> Rect {
        let w2 = (w * self.s).round().max(1.0);
        let h2 = (h * self.s).round().max(1.0);
        Rect {
            x0: (cx * self.s - w2 / 2.0).round(),
            y0: (cy * self.s - h2 / 2.0).round(),
            w: w2,
            h: h2,
        }
    }

    /// Whether a rectangle `w` × `h` counts as small (a grip or a dot).
    fn is_small(&self, w: f64, h: f64) -> bool {
        w.max(h) <= self.art / 5.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A stroked line: `fill="none"` with a stroke.
    Stroke,
    /// A filled shape outside masks and clip paths.
    Fill,
    /// A shape inside a mask or clip path.
    Region,
}

impl Role {
    fn of(tag: &str, hidden: bool) -> Self {
        let stroked = attr(tag, "stroke").is_some_and(|stroke| stroke != "none")
            && attr(tag, "fill") == Some("none");
        if stroked {
            Role::Stroke
        } else if hidden {
            Role::Region
        } else {
            Role::Fill
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    x0: f64,
    y0: f64,
    w: f64,
    h: f64,
}

/// A grip as drawn (`art`) and as fitted (`fitted`).
struct Grip {
    art: Rect,
    fitted: Rect,
}

/// The small filled rectangles outside masks, fitted: the mask cut-outs
/// around them are fitted to match.
fn collect_grips(text: &str, grid: &Grid) -> Option<Vec<Grip>> {
    let mut grips = Vec::new();
    let mut hidden = 0usize;
    let mut rest = text;
    while let Some(open) = rest.find('<') {
        let after = &rest[open..];
        let close = after.find('>')? + 1;
        let tag = &after[..close];
        match tag_name(tag) {
            "mask" | "clipPath" if !tag.ends_with("/>") => hidden += 1,
            "/mask" | "/clipPath" => hidden = hidden.saturating_sub(1),
            "path" if hidden == 0 && Role::of(tag, false) == Role::Fill => {
                for sub in subpaths(attr(tag, "d")?)? {
                    if let Some(r) = as_rect(&sub) {
                        if grid.is_small(r.w, r.h) {
                            let fitted =
                                grid.small_rect(r.x0 + r.w / 2.0, r.y0 + r.h / 2.0, r.w, r.h);
                            grips.push(Grip { art: r, fitted });
                        }
                    }
                }
            }
            _ => {}
        }
        rest = &after[close..];
    }
    Some(grips)
}

/// `path` for `<path …>`, `/mask` for `</mask>`.
fn tag_name(tag: &str) -> &str {
    let body = &tag[1..];
    let end = body
        .char_indices()
        .find(|&(i, c)| c.is_whitespace() || c == '>' || c == '/' && i > 0)
        .map_or(body.len(), |(i, _)| i);
    &body[..end]
}

/// The value of attribute `name` in `tag`.
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let key = format!(" {name}=\"");
    let start = tag.find(&key)? + key.len();
    Some(&tag[start..start + tag[start..].find('"')?])
}

/// `tag` with attribute `name` set to `value` (it must already be there).
fn with_attr(tag: &str, name: &str, value: &str) -> String {
    let key = format!(" {name}=\"");
    let Some(start) = tag.find(&key).map(|i| i + key.len()) else {
        return tag.to_string();
    };
    let Some(len) = tag[start..].find('"') else {
        return tag.to_string();
    };
    format!("{}{value}{}", &tag[..start], &tag[start + len..])
}

fn root_tag(tag: &str, grid: &Grid) -> String {
    let n = grid.n.to_string();
    let tag = with_attr(tag, "width", &n);
    let tag = with_attr(&tag, "height", &n);
    let tag = with_attr(&tag, "viewBox", &format!("0 0 {n} {n}"));
    with_attr(&tag, "stroke-width", &grid.weight.to_string())
}

/// A `<mask>` or `<clipPath>` tag with its user-space region fitted.
fn region_tag(tag: &str, grid: &Grid) -> String {
    let mut tag = tag.to_string();
    let x = attr(&tag, "x").and_then(|v| v.parse::<f64>().ok());
    let y = attr(&tag, "y").and_then(|v| v.parse::<f64>().ok());
    let w = attr(&tag, "width").and_then(|v| v.parse::<f64>().ok());
    let h = attr(&tag, "height").and_then(|v| v.parse::<f64>().ok());
    if let (Some(x), Some(w)) = (x, w) {
        let x0 = grid.edge(x);
        tag = with_attr(&tag, "x", &num(x0));
        tag = with_attr(&tag, "width", &num(grid.edge(x + w) - x0));
    }
    if let (Some(y), Some(h)) = (y, h) {
        let y0 = grid.edge(y);
        tag = with_attr(&tag, "y", &num(y0));
        tag = with_attr(&tag, "height", &num(grid.edge(y + h) - y0));
    }
    tag
}

fn path_tag(tag: &str, role: Role, grid: &Grid, grips: &[Grip]) -> Option<String> {
    let d = attr(tag, "d")?;
    // Black in a mask cuts the masked art away: the gaps round grips.
    let cuts = role == Role::Region && matches!(attr(tag, "fill"), Some("#000" | "#000000"));
    let mut fitted = String::with_capacity(d.len() + 16);
    for sub in subpaths(d)? {
        match (role, as_rect(&sub)) {
            (Role::Fill, Some(r)) if grid.is_small(r.w, r.h) => {
                let fit = grid.small_rect(r.x0 + r.w / 2.0, r.y0 + r.h / 2.0, r.w, r.h);
                write_rect(&mut fitted, fit);
            }
            (Role::Fill | Role::Region, Some(r)) => {
                let cut = if cuts { cut_out(r, grid, grips) } else { None };
                let fit = cut.unwrap_or_else(|| {
                    let (x0, y0) = (grid.edge(r.x0), grid.edge(r.y0));
                    Rect {
                        x0,
                        y0,
                        w: (grid.edge(r.x0 + r.w) - x0).max(1.0),
                        h: (grid.edge(r.y0 + r.h) - y0).max(1.0),
                    }
                });
                write_rect(&mut fitted, fit);
            }
            _ if is_closed_curve(&sub) => write_closed_curve(&mut fitted, &sub, role, grid),
            _ => write_fitted(&mut fitted, &sub, role, grid),
        }
    }
    let mut tag = with_attr(tag, "d", &fitted);
    if let Some(dashes) = attr(&tag, "stroke-dasharray") {
        let scaled: Vec<String> = dashes
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|part| !part.is_empty())
            .map(|part| {
                part.parse::<f64>()
                    .map(|v| num((v * grid.s).round().max(1.0)))
            })
            .collect::<Result<_, _>>()
            .ok()?;
        tag = with_attr(&tag, "stroke-dasharray", &scaled.join(" "));
    }
    Some(tag)
}

/// A mask cut-out centred on a grip: the grip as fitted, grown by the gap
/// around it (at least 1 px), so the gap stays even on every side.
fn cut_out(r: Rect, grid: &Grid, grips: &[Grip]) -> Option<Rect> {
    /// The widest gap round a grip, in art px.
    const MAX_GAP: f64 = 2.0;
    let (cx, cy) = (r.x0 + r.w / 2.0, r.y0 + r.h / 2.0);
    let grip = grips.iter().find(|g| {
        let (gap_x, gap_y) = ((r.w - g.art.w) / 2.0, (r.h - g.art.h) / 2.0);
        (g.art.x0 + g.art.w / 2.0 - cx).abs() < 1e-6
            && (g.art.y0 + g.art.h / 2.0 - cy).abs() < 1e-6
            && (0.0..=MAX_GAP).contains(&gap_x)
            && (0.0..=MAX_GAP).contains(&gap_y)
            && gap_x + gap_y > 0.0
    })?;
    let gap_x = ((r.w - grip.art.w) / 2.0 * grid.s).round().max(1.0);
    let gap_y = ((r.h - grip.art.h) / 2.0 * grid.s).round().max(1.0);
    Some(Rect {
        x0: grip.fitted.x0 - gap_x,
        y0: grip.fitted.y0 - gap_y,
        w: grip.fitted.w + 2.0 * gap_x,
        h: grip.fitted.h + 2.0 * gap_y,
    })
}

/// The subpaths of `d`, as absolute moves, lines, curves and closes.
fn subpaths(d: &str) -> Option<Vec<Vec<Seg>>> {
    let mut subs: Vec<Vec<Seg>> = Vec::new();
    for segment in SimplifyingPathParser::from(d) {
        let segment = segment.ok()?;
        if matches!(segment, Seg::MoveTo { .. }) || subs.is_empty() {
            subs.push(Vec::new());
        }
        subs.last_mut()?.push(segment);
    }
    Some(subs)
}

/// The subpath as an axis-aligned rectangle, if it is one.
fn as_rect(sub: &[Seg]) -> Option<Rect> {
    let mut points = Vec::with_capacity(5);
    for segment in sub {
        match *segment {
            Seg::MoveTo { x, y } | Seg::LineTo { x, y } => points.push((x, y)),
            Seg::ClosePath => {}
            _ => return None,
        }
    }
    if points.len() == 5 && points[0] == points[4] {
        points.pop();
    }
    if points.len() != 4 {
        return None;
    }
    for i in 0..4 {
        let (a, b) = (points[i], points[(i + 1) % 4]);
        if a.0 != b.0 && a.1 != b.1 {
            return None;
        }
    }
    let (x0, x1) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p.0), hi.max(p.0))
    });
    let (y0, y1) = points.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
        (lo.min(p.1), hi.max(p.1))
    });
    (x1 > x0 && y1 > y0).then_some(Rect {
        x0,
        y0,
        w: x1 - x0,
        h: y1 - y0,
    })
}

fn write_rect(out: &mut String, r: Rect) {
    let _ = write!(
        out,
        "M{} {}H{}V{}H{}Z",
        num(r.x0),
        num(r.y0),
        num(r.x0 + r.w),
        num(r.y0 + r.h),
        num(r.x0)
    );
}

/// Whether the subpath is closed and holds a curve: a circle, an ellipse or
/// a closed rounded shape.
fn is_closed_curve(sub: &[Seg]) -> bool {
    let curved = sub
        .iter()
        .any(|s| matches!(s, Seg::CurveTo { .. } | Seg::Quadratic { .. }));
    let first = sub.iter().find_map(|s| match *s {
        Seg::MoveTo { x, y } => Some((x, y)),
        _ => None,
    });
    let last = sub.iter().rev().find_map(|s| match *s {
        Seg::MoveTo { x, y }
        | Seg::LineTo { x, y }
        | Seg::CurveTo { x, y, .. }
        | Seg::Quadratic { x, y, .. } => Some((x, y)),
        Seg::ClosePath => None,
    });
    let closed = matches!(sub.last(), Some(Seg::ClosePath))
        || first
            .zip(last)
            .is_some_and(|(a, b)| (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6);
    curved && closed
}

/// A closed curve fitted as one shape: scaled uniformly, so a circle stays
/// a circle, to a whole number of pixels across, with its outer edge on the
/// pixel grid (pixel centres for a stroke of odd weight, pixel edges for a
/// fill). Snapping each point on its own instead would bend a small circle
/// out of round by up to half a pixel.
fn write_closed_curve(out: &mut String, sub: &[Seg], role: Role, grid: &Grid) {
    let points = sub.iter().filter_map(|s| match *s {
        Seg::MoveTo { x, y }
        | Seg::LineTo { x, y }
        | Seg::CurveTo { x, y, .. }
        | Seg::Quadratic { x, y, .. } => Some((x, y)),
        Seg::ClosePath => None,
    });
    let (mut lo, mut hi) = ((f64::MAX, f64::MAX), (f64::MIN, f64::MIN));
    for (x, y) in points {
        lo = (lo.0.min(x), lo.1.min(y));
        hi = (hi.0.max(x), hi.1.max(y));
    }
    let across = (hi.0 - lo.0).max(hi.1 - lo.1);
    if across <= 0.0 {
        return write_fitted(out, sub, role, grid);
    }
    // One scale for both axes, from the wider one.
    let k = (across * grid.s).round().max(1.0) / across;
    // The lowest edge, at the scaled centre, moved onto the grid.
    let on_grid = |v: f64| {
        if role == Role::Stroke && grid.odd() {
            (v - 0.5).round() + 0.5
        } else {
            v.round()
        }
    };
    let origin = (
        on_grid((lo.0 + hi.0) / 2.0 * grid.s - (hi.0 - lo.0) * k / 2.0),
        on_grid((lo.1 + hi.1) / 2.0 * grid.s - (hi.1 - lo.1) * k / 2.0),
    );
    let map = |x: f64, y: f64| (origin.0 + (x - lo.0) * k, origin.1 + (y - lo.1) * k);
    for segment in sub {
        match *segment {
            Seg::MoveTo { x, y } => {
                let p = map(x, y);
                let _ = write!(out, "M{} {}", num(p.0), num(p.1));
            }
            Seg::LineTo { x, y } => {
                let p = map(x, y);
                let _ = write!(out, "L{} {}", num(p.0), num(p.1));
            }
            Seg::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let (a, b, p) = (map(x1, y1), map(x2, y2), map(x, y));
                let _ = write!(
                    out,
                    "C{} {} {} {} {} {}",
                    num(a.0),
                    num(a.1),
                    num(b.0),
                    num(b.1),
                    num(p.0),
                    num(p.1)
                );
            }
            Seg::Quadratic { x1, y1, x, y } => {
                let (a, p) = (map(x1, y1), map(x, y));
                let _ = write!(out, "Q{} {} {} {}", num(a.0), num(a.1), num(p.0), num(p.1));
            }
            Seg::ClosePath => out.push('Z'),
        }
    }
}

/// A subpath with its points fitted to the grid; curve control points move
/// by the same amount as the point they belong to.
fn write_fitted(out: &mut String, sub: &[Seg], role: Role, grid: &Grid) {
    let fit = |x: f64, y: f64| match role {
        Role::Stroke => (grid.line(x), grid.line(y)),
        Role::Fill | Role::Region => (grid.edge(x), grid.edge(y)),
    };
    // The previous point, as drawn and as fitted.
    let mut last = ((0.0, 0.0), (0.0, 0.0));
    for segment in sub {
        match *segment {
            Seg::MoveTo { x, y } => {
                let p = fit(x, y);
                let _ = write!(out, "M{} {}", num(p.0), num(p.1));
                last = ((x, y), p);
            }
            Seg::LineTo { x, y } => {
                let p = fit(x, y);
                let _ = write!(out, "L{} {}", num(p.0), num(p.1));
                last = ((x, y), p);
            }
            Seg::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
            } => {
                let p = fit(x, y);
                let start = offset(last.0, last.1, grid.s);
                let end = offset((x, y), p, grid.s);
                let _ = write!(
                    out,
                    "C{} {} {} {} {} {}",
                    num(x1 * grid.s + start.0),
                    num(y1 * grid.s + start.1),
                    num(x2 * grid.s + end.0),
                    num(y2 * grid.s + end.1),
                    num(p.0),
                    num(p.1)
                );
                last = ((x, y), p);
            }
            Seg::Quadratic { x1, y1, x, y } => {
                let p = fit(x, y);
                let start = offset(last.0, last.1, grid.s);
                let end = offset((x, y), p, grid.s);
                let _ = write!(
                    out,
                    "Q{} {} {} {}",
                    num(x1 * grid.s + (start.0 + end.0) / 2.0),
                    num(y1 * grid.s + (start.1 + end.1) / 2.0),
                    num(p.0),
                    num(p.1)
                );
                last = ((x, y), p);
            }
            Seg::ClosePath => out.push('Z'),
        }
    }
}

/// How far fitting moved `art` (scaled by `s`) to `fitted`.
fn offset(art: (f64, f64), fitted: (f64, f64), s: f64) -> (f64, f64) {
    (fitted.0 - art.0 * s, fitted.1 - art.1 * s)
}

/// `v` written compactly: at most three decimals, no `-0`.
fn num(v: f64) -> String {
    let v = (v * 1000.0).round() / 1000.0;
    if v == 0.0 {
        "0".to_string()
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLYLINE_32: &str = include_str!("../../../assets/icons/32/polyline.svg");

    #[test]
    fn line_weight_follows_the_scale() {
        assert_eq!(line_weight(1.0), 1);
        assert_eq!(line_weight(1.25), 1);
        assert_eq!(line_weight(1.5), 1);
        assert_eq!(line_weight(1.75), 2);
        assert_eq!(line_weight(2.0), 2);
        assert_eq!(line_weight(0.875), 1);
    }

    #[test]
    fn the_art_size_comes_from_the_view_box() {
        assert_eq!(art_size(POLYLINE_32), Some(32));
        assert_eq!(art_size(r#"<svg viewBox="0 0 24 24"/>"#), None);
    }

    /// The `<path …/>` tags of `svg` that contain `marker`.
    fn paths_with<'a>(svg: &'a str, marker: &str) -> Vec<&'a str> {
        svg.split("<path")
            .skip(1)
            .map(|rest| &rest[..rest.find("/>").unwrap()])
            .filter(|tag| tag.contains(marker))
            .collect()
    }

    #[test]
    fn strokes_land_on_pixel_centres_at_125_percent() {
        let fitted = fit(POLYLINE_32, 32, 40).unwrap();
        assert!(fitted.contains(r#"width="40" height="40" viewBox="0 0 40 40""#));
        assert!(fitted.contains(r#"stroke-width="1""#));
        let strokes = paths_with(&fitted, r##"stroke="#ffffff""##);
        assert_eq!(strokes.len(), 1);
        let d = attr(strokes[0], "d").unwrap();
        for value in d
            .split(|c: char| c.is_ascii_alphabetic() || c.is_whitespace())
            .filter(|v| !v.is_empty())
        {
            let v: f64 = value.parse().unwrap();
            assert_eq!(v.fract(), 0.5, "{value} in {d}");
        }
    }

    #[test]
    fn grips_keep_one_size_and_an_even_gap() {
        let polyline_16 = include_str!("../../../assets/icons/polyline.svg");
        for (art, source, n) in [
            (16, polyline_16, 20),
            (16, polyline_16, 24),
            (16, polyline_16, 28),
            (32, POLYLINE_32, 40),
            (32, POLYLINE_32, 48),
            (32, POLYLINE_32, 56),
        ] {
            let grid = Grid::new(art, n);
            let fitted = fit(source, art, n).unwrap();
            let grips = collect_grips(source, &grid).unwrap();
            assert!(!grips.is_empty());
            let first = grips[0].fitted;
            for grip in &grips {
                assert_eq!((grip.fitted.w, grip.fitted.h), (first.w, first.h), "{n} px");
                assert_eq!(grip.fitted.x0.fract(), 0.0);
                assert!(fitted.contains(&format!(
                    "M{} {}H",
                    num(grip.fitted.x0),
                    num(grip.fitted.y0)
                )));
                let cut = cut_out(
                    Rect {
                        x0: grip.art.x0 - 1.0,
                        y0: grip.art.y0 - 1.0,
                        w: grip.art.w + 2.0,
                        h: grip.art.h + 2.0,
                    },
                    &grid,
                    &grips,
                )
                .unwrap();
                assert_eq!(
                    grip.fitted.x0 - cut.x0,
                    cut.x0 + cut.w - grip.fitted.x0 - grip.fitted.w
                );
                assert!(grip.fitted.x0 - cut.x0 >= 1.0);
            }
        }
    }

    #[test]
    fn a_fill_inside_a_frame_never_overlaps_it() {
        // A stroked frame round a fill that fills it exactly. When one art
        // pixel becomes two screen pixels, a 1 px line cannot fill both, so
        // a 1 px gap may open on one side. From 175 % lines are 2 px while
        // an art pixel is only 1.75 px, so a line may reach 1 px into the
        // fill next to it, but never further.
        let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32" stroke-width="1"><path d="M2.5 2.5H29.5V29.5H2.5Z" fill="none" stroke="#ffffff"/><path d="M3 3H29V29H3Z" fill="#ffffff" fill-opacity="0.12"/></svg>"##;
        for n in [24, 28, 40, 48, 56, 64] {
            let grid = Grid::new(32, n);
            assert!(fit(source, 32, n).is_some());
            let half = f64::from(grid.weight) / 2.0;
            // The pixels the left and right lines cover, and the fill.
            let (left_end, right_start) = (grid.line(2.5) + half, grid.line(29.5) - half);
            let (fill_start, fill_end) = (grid.edge(3.0), grid.edge(29.0));
            let gaps = [fill_start - left_end, right_start - fill_end];
            let overlap = f64::from(grid.weight - 1);
            for gap in gaps {
                assert!((-overlap..=1.0).contains(&gap), "{n} px: gaps {gaps:?}");
            }
        }
    }

    #[test]
    fn every_icon_fits_every_scale_and_still_parses() {
        fn svgs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    svgs(&path, out);
                } else if path.extension().is_some_and(|ext| ext == "svg") {
                    out.push(path);
                }
            }
        }
        let mut files = Vec::new();
        svgs(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icons"),
            &mut files,
        );
        let mut fitted = 0;
        for path in files {
            let text = std::fs::read_to_string(&path).unwrap();
            let Some(art) = art_size(&text) else {
                continue;
            };
            // 75 % to 200 % of the art's own size.
            for percent in [75, 100, 125, 150, 175, 200] {
                let n = art * percent / 100;
                let out = fit(&text, art, n)
                    .unwrap_or_else(|| panic!("{} does not fit {n} px", path.display()));
                let tree = resvg::usvg::Tree::from_str(&out, &resvg::usvg::Options::default())
                    .unwrap_or_else(|error| panic!("{} at {n} px: {error}", path.display()));
                assert_eq!(tree.size().width(), n as f32, "{}", path.display());
                fitted += 1;
            }
        }
        assert!(fitted > 800 * 6, "only {fitted} fitted");
    }

    #[test]
    fn small_circles_stay_round() {
        // A ring 6 px across, as in the array icons.
        let source = r##"<svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 32 32" stroke-width="1"><path d="M18.5 5.5A3 3 0 1 1 12.5 5.5A3 3 0 1 1 18.5 5.5Z" fill="none" stroke="#ffffff"/></svg>"##;
        for n in [24, 28, 40, 48, 56, 64] {
            let fitted = fit(source, 32, n).unwrap();
            let d = attr(paths_with(&fitted, "stroke=")[0], "d").unwrap();
            let mut on_curve = Vec::new();
            for segment in SimplifyingPathParser::from(d) {
                match segment.unwrap() {
                    Seg::MoveTo { x, y } | Seg::LineTo { x, y } | Seg::CurveTo { x, y, .. } => {
                        on_curve.push((x, y))
                    }
                    _ => {}
                }
            }
            let (min_x, max_x) = on_curve.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                (lo.min(p.0), hi.max(p.0))
            });
            let (min_y, max_y) = on_curve.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| {
                (lo.min(p.1), hi.max(p.1))
            });
            assert!(
                ((max_x - min_x) - (max_y - min_y)).abs() < 1e-3,
                "{n} px: {d}"
            );
            if Grid::new(32, n).odd() {
                for edge in [min_x, max_x, min_y, max_y] {
                    assert!(
                        ((edge - 0.5).fract()).abs() < 1e-3,
                        "{n} px: edge {edge} in {d}"
                    );
                }
            }
        }
    }

    #[test]
    fn unknown_elements_are_not_fitted() {
        let source = r#"<svg width="16" height="16" viewBox="0 0 16 16" stroke-width="1"><circle cx="8" cy="8" r="4"/></svg>"#;
        assert!(fit(source, 16, 20).is_none());
    }
}
