//! SVG reading: the shapes of a vector drawing as the drawing objects of a
//! block — every path, rectangle, circle, ellipse, line, polyline and
//! polygon as polylines in their stroke's colour and width, and a filled
//! shape as a solid hatch in the fill's colour under its outline.
//!
//! `usvg` does the SVG itself — XML, CSS, `use`, `viewBox`, units and every
//! transform — so each path arrives in the picture's own coordinates; curves
//! are then cut into short straight segments. Colours are kept as true
//! colour. Gradient fills take their first stop, and text, raster images
//! and filters are not read (text needs the system fonts and a layout
//! engine, which the metafile importer has and a vector drawing's `<text>`
//! would only approximate).
//!
//! As with a metafile, the picture is scaled to fit the size it is
//! imported into, with its top-left corner at the block's origin; SVG's y
//! runs down the page, so it runs down the drawing here too.

use codec::entities::{
    BoundaryEdge, BoundaryPath, Hatch, Polyline2D, PolylineEdge, Vertex2D,
};
use codec::types::{Color, Vector2, Vector3};
use codec::EntityType;
use resvg::tiny_skia::PathSegment;
use resvg::usvg::{self, Node, Paint};

/// A picture: its objects, in block coordinates.
pub struct Picture {
    pub entities: Vec<EntityType>,
}

/// Segments a curve is cut into, at least and at most.
const MIN_SEGMENTS: usize = 6;
const MAX_SEGMENTS: usize = 96;

/// Why a file could not be read.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// Not an SVG document.
    NotSvg,
    /// An SVG without anything this importer draws.
    Empty,
}

struct Flattener {
    /// SVG user units to drawing units.
    scale: f64,
    /// The width of the picture in SVG user units; curve fineness is a
    /// share of it.
    extent: f64,
    entities: Vec<EntityType>,
    /// Whether the path being read draws an outline.
    outline: bool,
    /// The subpaths of the path being read, in block coordinates, for its fill.
    loops: Vec<Vec<Vector2>>,
    /// Where the path being read starts in `entities`.
    outline_start: usize,
}

impl Flattener {
    fn at(&self, p: [f64; 2]) -> Vector3 {
        Vector3::new(p[0] * self.scale, -p[1] * self.scale, 0.0)
    }

    fn segments(&self, control_length: f64) -> usize {
        // A segment per 0.4 % of the picture along the control polygon,
        // rounded up so a tight curve is not left a corner.
        let n = (control_length / (0.004 * self.extent)).sqrt().ceil() * 2.0;
        (n as usize).clamp(MIN_SEGMENTS, MAX_SEGMENTS)
    }

    fn polyline(&mut self, points: &[[f64; 2]], closed: bool, width: f64, color: Color) {
        // A repeated end point (as a path that closes by returning to its
        // start) is the close, not a vertex.
        let mut points = points.to_vec();
        points.dedup_by(|a, b| a == b);
        let mut closed = closed;
        if points.len() > 2 && points.first() == points.last() {
            points.pop();
            closed = true;
        }
        if points.len() < 2 {
            return;
        }
        // A fill closes every subpath.
        self.loops.push(
            points
                .iter()
                .map(|p| {
                    let v = self.at(*p);
                    Vector2::new(v.x, v.y)
                })
                .collect(),
        );
        if !self.outline {
            return;
        }
        let width = width * self.scale;
        let mut polyline = Polyline2D::new();
        polyline.start_width = width;
        polyline.end_width = width;
        for p in &points {
            let mut vertex = Vertex2D::new(self.at(*p));
            vertex.start_width = width;
            vertex.end_width = width;
            polyline.add_vertex(vertex);
        }
        if closed {
            polyline.close();
        }
        polyline.common.color = color;
        self.entities.push(EntityType::Polyline2D(polyline));
    }

    fn path(&mut self, path: &usvg::Path) {
        let (stroke, fill) = (path.stroke(), path.fill());
        let stroke_color = stroke.and_then(|s| paint_color(s.paint()));
        let fill_color = fill.and_then(|f| paint_color(f.paint()));
        // The outline is the stroke's; a shape with only a fill is drawn in
        // the fill's colour with no width.
        let (color, width) = match (stroke_color, fill_color) {
            (Some(c), _) => (c, stroke.map_or(0.0, |s| width_in_picture(s, path))),
            (None, Some(c)) => (c, 0.0),
            (None, None) => return,
        };
        // A filled shape is a solid hatch of its subpaths in the fill's
        // colour, under its outline; a shape with only a fill has no outline.
        self.outline = stroke_color.is_some();
        self.outline_start = self.entities.len();
        self.loops.clear();
        let transform = path.abs_transform();
        let map = |p: resvg::tiny_skia::Point| {
            let mut p = p;
            transform.map_point(&mut p);
            [f64::from(p.x), f64::from(p.y)]
        };
        let mut points: Vec<[f64; 2]> = Vec::new();
        let mut closed = false;
        let mut last = [0.0, 0.0];
        for segment in path.data().segments() {
            match segment {
                PathSegment::MoveTo(p) => {
                    self.polyline(&points, closed, width, color);
                    points.clear();
                    closed = false;
                    last = map(p);
                    points.push(last);
                }
                PathSegment::LineTo(p) => {
                    last = map(p);
                    points.push(last);
                }
                PathSegment::QuadTo(c, p) => {
                    let (c, p) = (map(c), map(p));
                    let n = self.segments(distance(last, c) + distance(c, p));
                    for k in 1..=n {
                        let t = k as f64 / n as f64;
                        let u = 1.0 - t;
                        points.push([
                            u * u * last[0] + 2.0 * u * t * c[0] + t * t * p[0],
                            u * u * last[1] + 2.0 * u * t * c[1] + t * t * p[1],
                        ]);
                    }
                    last = p;
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    let (c1, c2, p) = (map(c1), map(c2), map(p));
                    let n = self.segments(distance(last, c1) + distance(c1, c2) + distance(c2, p));
                    for k in 1..=n {
                        let t = k as f64 / n as f64;
                        let u = 1.0 - t;
                        let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        points.push([
                            a * last[0] + b * c1[0] + c * c2[0] + d * p[0],
                            a * last[1] + b * c1[1] + c * c2[1] + d * p[1],
                        ]);
                    }
                    last = p;
                }
                PathSegment::Close => {
                    self.polyline(&points, true, width, color);
                    // A subpath that follows a close starts where it began.
                    let start = points.first().copied().unwrap_or(last);
                    points.clear();
                    closed = false;
                    last = start;
                    points.push(start);
                }
            }
        }
        // A lone start point left by a close is not a subpath.
        if points.len() > 1 {
            self.polyline(&points, closed, width, color);
        }
        if let Some(fill) = fill_color {
            self.fill(fill);
        }
    }

    /// The subpaths read for the current path as one solid hatch, ahead of
    /// the outline polylines it was read with.
    fn fill(&mut self, color: Color) {
        let loops: Vec<_> = self.loops.drain(..).filter(|l| l.len() >= 3).collect();
        if loops.is_empty() {
            return;
        }
        let mut hatch = Hatch::new();
        hatch.is_solid = true;
        hatch.common.color = color;
        for (k, vertices) in loops.into_iter().enumerate() {
            let mut boundary = if k == 0 { BoundaryPath::external() } else { BoundaryPath::new() };
            boundary.add_edge(BoundaryEdge::Polyline(PolylineEdge::new(vertices, true)));
            hatch.paths.push(boundary);
        }
        // Drawn before this path's outline, after everything painted earlier.
        let at = self.outline_start;
        self.entities.insert(at, EntityType::Hatch(Box::new(hatch)));
    }

    fn group(&mut self, group: &usvg::Group) {
        for node in group.children() {
            match node {
                Node::Group(g) => self.group(g),
                Node::Path(p) if p.is_visible() => self.path(p),
                // Raster images and text are not read.
                _ => {}
            }
        }
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// A stroke's width in picture units: the stroke is set in the path's own
/// units, which its transform may scale.
fn width_in_picture(stroke: &usvg::Stroke, path: &usvg::Path) -> f64 {
    let t = path.abs_transform();
    let scale = f64::from(t.sx * t.sy - t.kx * t.ky).abs().sqrt();
    f64::from(stroke.width().get()) * scale
}

/// A paint as a colour: a solid colour as it is, a gradient as its first
/// stop, a pattern as none.
fn paint_color(paint: &Paint) -> Option<Color> {
    let c = match paint {
        Paint::Color(c) => *c,
        Paint::LinearGradient(g) => g.stops().first()?.color(),
        Paint::RadialGradient(g) => g.stops().first()?.color(),
        Paint::Pattern(_) => return None,
    };
    // Black and white draw as colour 7, which contrasts with either canvas
    // background (as the WMF import does).
    if (c.red, c.green, c.blue) == (0, 0, 0) || (c.red, c.green, c.blue) == (255, 255, 255) {
        return Some(Color::Index(7));
    }
    Some(Color::Rgb { r: c.red, g: c.green, b: c.blue })
}

/// Reads an SVG (or gzip-compressed SVGZ), scaled so the picture fits
/// within `fit` (width and height, drawing units).
pub fn read(bytes: &[u8], fit: [f64; 2]) -> Result<Picture, Error> {
    let mut options = usvg::Options::default();
    // A document that names files or the network is read as drawn from
    // what it carries: no external images are fetched.
    options.image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: Box::new(|_, _, _| None),
        resolve_string: Box::new(|_, _| None),
    };
    let tree = usvg::Tree::from_data(bytes, &options).map_err(|_| Error::NotSvg)?;
    let size = tree.size();
    let (w, h) = (f64::from(size.width()), f64::from(size.height()));
    if !(w > 0.0 && h > 0.0) {
        return Err(Error::Empty);
    }
    let scale = (fit[0] / w).min(fit[1] / h);
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let mut flattener = Flattener { scale, extent: w.max(h), entities: Vec::new(), outline: true, loops: Vec::new(), outline_start: 0 };
    flattener.group(tree.root());
    if flattener.entities.is_empty() {
        return Err(Error::Empty);
    }
    Ok(Picture { entities: flattener.entities })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">
        <rect x="10" y="10" width="30" height="20" fill="none" stroke="#ff0000" stroke-width="2"/>
        <g transform="translate(50 0)"><path d="M0 0 C 10 0 10 20 0 20 L 0 0 Z" fill="#00ff00"/></g>
        <line x1="0" y1="0" x2="100" y2="50" stroke="black"/>
    </svg>"##;

    fn polylines(p: &Picture) -> Vec<&Polyline2D> {
        p.entities
            .iter()
            .filter_map(|e| match e {
                EntityType::Polyline2D(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn shapes_become_polylines_fitted_to_the_view() {
        let picture = read(SQUARE.as_bytes(), [10.0, 10.0]).unwrap();
        let lines = polylines(&picture);
        assert_eq!(lines.len(), 2);
        // 100 units across fit 10: a scale of 0.1; the rectangle is the
        // stroke's red, 4 vertices, closed, y running down.
        let rect = lines[0];
        assert_eq!(rect.common.color, Color::Rgb { r: 255, g: 0, b: 0 });
        assert_eq!(rect.vertices.len(), 4);
        assert!(rect.is_closed());
        assert!((rect.vertices[0].location.x - 1.0).abs() < 1e-6);
        assert!((rect.vertices[0].location.y + 1.0).abs() < 1e-6);
        assert!((rect.start_width - 0.2).abs() < 1e-6);
    }

    #[test]
    fn filled_curve_without_stroke_is_a_solid_hatch_in_the_fill_colour() {
        let picture = read(SQUARE.as_bytes(), [100.0, 100.0]).unwrap();
        let hatches: Vec<_> = picture
            .entities
            .iter()
            .filter_map(|e| match e {
                EntityType::Hatch(h) => Some(h),
                _ => None,
            })
            .collect();
        assert_eq!(hatches.len(), 1);
        let hatch = hatches[0];
        assert!(hatch.is_solid);
        assert_eq!(hatch.common.color, Color::Rgb { r: 0, g: 255, b: 0 });
        assert_eq!(hatch.paths.len(), 1);
        match &hatch.paths[0].edges[0] {
            BoundaryEdge::Polyline(edge) => {
                assert!(edge.vertices.len() > 6, "the curve is flattened");
                // The group's translation is applied.
                assert!(edge.vertices.iter().all(|v| v.x >= 50.0 - 1e-6));
            }
            _ => panic!("a polyline boundary"),
        }
    }

    #[test]
    fn rejects_non_svg_and_empty_documents() {
        assert_eq!(read(b"not svg", [1.0, 1.0]).err(), Some(Error::NotSvg));
        let empty = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>"#;
        assert_eq!(read(empty, [1.0, 1.0]).err(), Some(Error::Empty));
    }
}
