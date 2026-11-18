use super::*;
use crate::scene::model::pdf_vector::{PdfImage, PdfText};
use std::f64::consts::{FRAC_PI_2, FRAC_PI_8, PI, TAU};

/// A quarter circle as one Bézier: its control points this share of
/// the radius along the tangents (4/3 (√2 − 1)).
const K: f64 = 0.552_284_749_830_793_4;

fn line(a: [f64; 2], b: [f64; 2]) -> Segment {
    Segment::Line(a, b)
}

/// The Bézier quarter circle round `c` from the angle `from`,
/// counter-clockwise.
fn quarter(c: [f64; 2], r: f64, from: f64) -> Segment {
    let (s0, c0) = from.sin_cos();
    let (s1, c1) = (from + FRAC_PI_2).sin_cos();
    let p0 = [c[0] + r * c0, c[1] + r * s0];
    let p3 = [c[0] + r * c1, c[1] + r * s1];
    Segment::Cubic(
        p0,
        [p0[0] - K * r * s0, p0[1] + K * r * c0],
        [p3[0] + K * r * s1, p3[1] - K * r * c1],
        p3,
    )
}

/// The four Béziers of a circle, as PDF writers draw one.
fn circle(c: [f64; 2], r: f64) -> Vec<Segment> {
    (0..4)
        .map(|k| quarter(c, r, k as f64 * FRAC_PI_2))
        .collect()
}

/// Straight segments through `points`.
fn lines(points: &[[f64; 2]]) -> Vec<Segment> {
    points.windows(2).map(|w| line(w[0], w[1])).collect()
}

/// A path stroked in `rgb`, `width` points wide.
fn stroked(segments: Vec<Segment>, closed: bool, rgb: [u8; 3], width: f64) -> PdfPath {
    PdfPath {
        subpaths: vec![SubPath { segments, closed }],
        stroke: Some((rgb, width)),
        fill: None,
        pen: Some((rgb, width)),
        closes: Vec::new(),
        clip: None,
    }
}

/// A red line, one point wide, through `points`.
fn red(points: &[[f64; 2]]) -> PdfPath {
    stroked(lines(points), false, [255, 0, 0], 1.0)
}

/// Each ring filled in `rgb`, closed by `h` (which draws the line back
/// to its start), painted with a black one-point pen.
fn filled(rings: &[&[[f64; 2]]], rgb: [u8; 3]) -> PdfPath {
    let subpaths: Vec<SubPath> = rings
        .iter()
        .map(|ring| {
            let mut points = ring.to_vec();
            points.push(ring[0]);
            SubPath {
                segments: lines(&points),
                closed: true,
            }
        })
        .collect();
    PdfPath {
        closes: vec![
            SubClose {
                op: true,
                line: true
            };
            subpaths.len()
        ],
        subpaths,
        stroke: None,
        fill: Some(rgb),
        pen: Some(([0, 0, 0], 1.0)),
        clip: None,
    }
}

fn page(paths: Vec<PdfPath>) -> PageVectors {
    PageVectors {
        paths,
        ..PageVectors::default()
    }
}

/// A PDF underlay inserted at `at`, scaled by `scale` and turned by
/// `rotation`.
fn underlay(at: [f64; 2], scale: f64, rotation: f64) -> Underlay {
    let mut u = Underlay::new(UnderlayType::Pdf);
    u.insertion_point = Vector3::new(at[0], at[1], 0.0);
    (u.x_scale, u.y_scale, u.rotation) = (scale, scale, rotation);
    u
}

fn naming(settings: PdfImportSettings) -> LayerNaming {
    LayerNaming {
        settings,
        prefix: "PDF_".to_string(),
        current: "0".to_string(),
        styles: Vec::new(),
    }
}

/// The page imported whole, in the default settings, onto an underlay
/// at the origin at full size.
fn import(content: PageVectors) -> ImportResult {
    import_with(
        content,
        PdfImportSettings::default(),
        &underlay([0.0, 0.0], 1.0, 0.0),
    )
}

fn import_with(content: PageVectors, settings: PdfImportSettings, u: &Underlay) -> ImportResult {
    convert(&[(None, content)], u, &ImportArea::All, &naming(settings))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

/// Whether two angles are the same direction (0 and a whole turn are).
fn same_angle(a: f64, b: f64) -> bool {
    let d = (a - b).rem_euclid(TAU);
    d < 1e-9 || TAU - d < 1e-9
}

fn assert_at(actual: [f64; 2], expected: [f64; 2], what: &str) {
    assert!(
        near(actual[0], expected[0]) && near(actual[1], expected[1]),
        "{what}: {actual:?}, expected {expected:?}"
    );
}

fn polylines(result: &ImportResult) -> Vec<&LwPolyline> {
    result
        .entities
        .iter()
        .filter_map(|e| match e {
            EntityType::LwPolyline(p) => Some(p),
            _ => None,
        })
        .collect()
}

fn vertices(pl: &LwPolyline) -> Vec<[f64; 2]> {
    pl.vertices
        .iter()
        .map(|v| [v.location.x, v.location.y])
        .collect()
}

fn xy(v: Vector3) -> [f64; 2] {
    [v.x, v.y]
}

#[test]
fn settings_read_and_write_their_system_variables() {
    let default = PdfImportSettings::default();
    // PDFIMPORTMODE 6 (lineweights, join), PDFIMPORTFILTER 8 (no
    // raster images), PDFIMPORTLAYERS 0 (the PDF's layers).
    assert_eq!(
        (default.mode(), default.filter(), default.layers_value()),
        (6, 8, 0)
    );
    for value in 0..32 {
        let mut s = default;
        s.set_mode(value);
        assert_eq!(s.mode(), value);
    }
    for value in 0..16 {
        let mut s = default;
        s.set_filter(value);
        assert_eq!(s.filter(), value);
    }
    let layers = |layers| PdfImportSettings { layers, ..default }.layers_value();
    assert_eq!(
        [
            ImportLayers::Pdf,
            ImportLayers::Object,
            ImportLayers::Current
        ]
        .map(layers),
        [0, 1, 2]
    );
}

#[test]
fn colours_are_index_colours_when_basic_and_widths_standard_lineweights() {
    let basics = [
        [255, 0, 0],
        [255, 255, 0],
        [0, 255, 0],
        [0, 255, 255],
        [0, 0, 255],
        [255, 0, 255],
    ];
    for (k, rgb) in basics.into_iter().enumerate() {
        assert_eq!(color_of(rgb), Color::Index(k as u8 + 1));
    }
    assert_eq!(color_of([0, 0, 0]), Color::Index(7));
    assert_eq!(
        color_of([255, 255, 255]),
        Color::Rgb {
            r: 255,
            g: 255,
            b: 255
        }
    );
    assert_eq!(color_of([1, 2, 3]), Color::Rgb { r: 1, g: 2, b: 3 });
    // Points to the nearest standard weight in hundredths of a
    // millimetre: 1 pt is 0.353 mm.
    let weights = [0.0, 0.25, 1.0, 2.0, 100.0].map(lineweight_of);
    assert_eq!(
        weights.map(|w| match w {
            LineWeight::Value(v) => v,
            _ => -99,
        }),
        [0, 9, 35, 70, 211]
    );
}

#[test]
fn strokes_that_meet_become_one_polyline_placed_by_the_underlay() {
    let content = || {
        page(vec![
            red(&[[1.0, 1.0], [2.0, 1.0], [2.0, 2.0]]),
            red(&[[2.0, 2.0], [3.0, 2.0]]),
        ])
    };
    // At (100, 200), twice the size, a quarter turn: (x, y) lands at
    // (100 − 2y, 200 + 2x).
    let u = underlay([100.0, 200.0], 2.0, FRAC_PI_2);
    let result = import_with(content(), PdfImportSettings::default(), &u);
    let pls = polylines(&result);
    assert_eq!(pls.len(), 1);
    let pl = pls[0];
    let expected = [[98.0, 202.0], [98.0, 204.0], [96.0, 204.0], [96.0, 206.0]];
    assert_eq!(pl.vertices.len(), 4);
    for (v, e) in vertices(pl).into_iter().zip(expected) {
        assert_at(v, e, "vertex");
    }
    assert!(!pl.is_closed);
    assert!(pl.vertices.iter().all(|v| v.bulge == 0.0));
    assert_eq!(pl.common.layer, "PDF_Geometry");
    // The layer takes the colour; the object follows it.
    assert_eq!(
        result.layers,
        [("PDF_Geometry".to_string(), Color::Index(1))]
    );
    assert_eq!(pl.common.color, Color::ByLayer);
    assert_eq!(pl.common.line_weight, LineWeight::Value(35));

    // Straight strokes join with "Join" off too; lineweights off leave
    // the weight to the layer.
    let settings = PdfImportSettings {
        join: false,
        lineweights: false,
        ..PdfImportSettings::default()
    };
    let result = import_with(content(), settings, &u);
    assert_eq!(polylines(&result).len(), 1);
    assert_eq!(
        polylines(&result)[0].common.line_weight,
        LineWeight::ByLayer
    );

    // A stroke drawn the other way round is turned to meet.
    let result = import(page(vec![
        red(&[[0.0, 5.0], [1.0, 5.0]]),
        red(&[[2.0, 5.0], [1.0, 5.0]]),
    ]));
    let pls = polylines(&result);
    assert_eq!(pls.len(), 1);
    assert_eq!(vertices(pls[0]), [[0.0, 5.0], [1.0, 5.0], [2.0, 5.0]]);

    // Four sides drawn apart close into one polyline.
    let square = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]];
    let sides = square.windows(2).map(red).collect();
    let result = import(page(sides));
    let pls = polylines(&result);
    assert_eq!(pls.len(), 1);
    assert!(pls[0].is_closed);
    assert_eq!(vertices(pls[0]), &square[..4]);
}

#[test]
fn bezier_circles_and_arcs_become_circles_arcs_and_bulges() {
    // Twice the size: a circle of radius 1 round (5, 5) is one of 2
    // round (10, 10).
    let u = underlay([0.0, 0.0], 2.0, 0.0);
    let blue = |segments, closed| stroked(segments, closed, [0, 0, 255], 1.0);
    let result = import_with(
        page(vec![blue(circle([5.0, 5.0], 1.0), true)]),
        PdfImportSettings::default(),
        &u,
    );
    let [EntityType::Circle(c)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert_at(xy(c.center), [10.0, 10.0], "circle centre");
    assert!(near(c.radius, 2.0), "{}", c.radius);

    // A lone quarter circle is an ARC, counter-clockwise from 0 to 90°.
    let result = import_with(
        page(vec![blue(vec![quarter([0.0, 4.0], 1.0, 0.0)], false)]),
        PdfImportSettings::default(),
        &u,
    );
    let [EntityType::Arc(a)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert_at(xy(a.center), [0.0, 8.0], "arc centre");
    assert!(near(a.radius, 2.0));
    assert!(
        same_angle(a.start_angle, 0.0) && same_angle(a.end_angle, FRAC_PI_2),
        "{} {}",
        a.start_angle,
        a.end_angle
    );

    // A line running into a quarter circle: one polyline whose second
    // vertex bulges by tan(90° / 4).
    let bend = || {
        page(vec![blue(
            vec![
                line([2.0, 0.0], [3.0, 0.0]),
                quarter([3.0, 1.0], 1.0, -FRAC_PI_2),
            ],
            false,
        )])
    };
    let result = import_with(bend(), PdfImportSettings::default(), &u);
    let pls = polylines(&result);
    assert_eq!(pls.len(), 1, "{:?}", result.entities);
    assert_eq!(vertices(pls[0]).len(), 3);
    assert_at(vertices(pls[0])[2], [8.0, 2.0], "arc end");
    let bulges: Vec<f64> = pls[0].vertices.iter().map(|v| v.bulge).collect();
    assert!(
        near(bulges[0], 0.0) && (bulges[1] - FRAC_PI_8.tan()).abs() < 1e-6 && near(bulges[2], 0.0),
        "{bulges:?}"
    );
    // "Join" off: the line, and the arc apart.
    let off = PdfImportSettings {
        join: false,
        ..PdfImportSettings::default()
    };
    let result = import_with(bend(), off, &u);
    // (in either order: both pieces have the subpath's place)
    assert!(
        matches!(
            result.entities.as_slice(),
            [EntityType::LwPolyline(_), EntityType::Arc(_)]
                | [EntityType::Arc(_), EntityType::LwPolyline(_)]
        ),
        "{:?}",
        result.entities
    );

    // Mirrored, the arc turns the other way.
    let mut mirrored = u.clone();
    mirrored.x_scale = -2.0;
    let result = import_with(bend(), PdfImportSettings::default(), &mirrored);
    let bulge = polylines(&result)[0].vertices[1].bulge;
    assert!((bulge + FRAC_PI_8.tan()).abs() < 1e-6, "{bulge}");

    // A Bézier that is no arc: a one-span cubic SPLINE on its control
    // points.
    let curve = Segment::Cubic([0.0, 0.0], [0.0, 1.0], [3.0, 1.0], [3.0, 0.0]);
    let result = import_with(
        page(vec![blue(vec![curve], false)]),
        PdfImportSettings::default(),
        &u,
    );
    let [EntityType::Spline(sp)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert_eq!(sp.degree, 3);
    assert_eq!(
        sp.control_points.iter().map(|p| xy(*p)).collect::<Vec<_>>(),
        [[0.0, 0.0], [0.0, 2.0], [6.0, 2.0], [6.0, 0.0]]
    );
    assert_eq!(sp.knots, [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0]);
    assert!(sp.flags.planar && !sp.flags.linear && !sp.flags.closed);
}

/// `n` points round the ellipse with semi-axes `a` (along x) and `b`,
/// from the angle `from` to `to`.
fn on_ellipse(a: f64, b: f64, n: usize, from: f64, to: f64) -> Vec<[f64; 2]> {
    (0..n)
        .map(|k| {
            let t = from + (to - from) * k as f64 / (n - 1) as f64;
            [a * t.cos(), b * t.sin()]
        })
        .collect()
}

#[test]
fn straight_polylines_on_a_circle_or_an_ellipse_become_one() {
    let closed_ring = |points: Vec<[f64; 2]>| {
        let mut segments = lines(&points);
        segments.push(line(points[points.len() - 1], points[0]));
        stroked(segments, true, [0, 0, 0], 1.0)
    };
    // 24 sides on a circle of radius 2.
    let ring = on_ellipse(2.0, 2.0, 25, 0.0, TAU)[..24].to_vec();
    let result = import(page(vec![closed_ring(ring)]));
    let [EntityType::Circle(c)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert_at(xy(c.center), [0.0, 0.0], "circle centre");
    assert!(near(c.radius, 2.0));

    // 24 sides on an ellipse 4 × 2.
    let ring = on_ellipse(2.0, 1.0, 25, 0.0, TAU)[..24].to_vec();
    let result = import(page(vec![closed_ring(ring)]));
    let [EntityType::Ellipse(e)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert_at(xy(e.center), [0.0, 0.0], "ellipse centre");
    assert!(
        near(e.major_axis.x.abs(), 2.0) && near(e.major_axis.y, 0.0),
        "{:?}",
        e.major_axis
    );
    assert!(near(e.minor_axis_ratio, 0.5));
    assert!(near(e.end_parameter - e.start_parameter, TAU));

    // Twelve segments on a quarter of a circle of radius 3: an ARC.
    let arc = on_ellipse(3.0, 3.0, 13, 0.0, FRAC_PI_2);
    let result = import(page(vec![stroked(lines(&arc), false, [0, 0, 0], 1.0)]));
    let [EntityType::Arc(a)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert!(
        near(a.radius, 3.0) && same_angle(a.start_angle, 0.0) && same_angle(a.end_angle, FRAC_PI_2)
    );

    // Eight segments are too few to read as a curve.
    let short = on_ellipse(3.0, 3.0, 9, 0.0, FRAC_PI_2);
    let result = import(page(vec![stroked(lines(&short), false, [0, 0, 0], 1.0)]));
    assert!(
        matches!(result.entities.as_slice(), [EntityType::LwPolyline(p)] if !p.is_closed && p.vertices.len() == 9),
        "{:?}",
        result.entities
    );
}

fn corners(s: &Solid) -> [[f64; 2]; 4] {
    [
        s.first_corner,
        s.second_corner,
        s.third_corner,
        s.fourth_corner,
    ]
    .map(xy)
}

#[test]
fn fills_become_solids_hatches_or_lines() {
    let triangle: &[[f64; 2]] = &[[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]];
    let square: &[[f64; 2]] = &[[2.0, 0.0], [3.0, 0.0], [3.0, 1.0], [2.0, 1.0]];
    let pentagon: &[[f64; 2]] = &[[4.0, 0.0], [5.0, 0.0], [5.5, 0.5], [4.5, 1.0], [4.0, 0.5]];
    let blue = [0, 0, 255];
    let content = || {
        page(vec![
            filled(&[triangle], blue),
            filled(&[square], blue),
            filled(&[pentagon], blue),
        ])
    };
    let result = import(content());
    let [EntityType::Solid(t), EntityType::Solid(q), EntityType::Hatch(h)] =
        result.entities.as_slice()
    else {
        panic!("{:?}", result.entities);
    };
    // Three corners: the third twice. Four: in the order 1-2-4-3.
    assert_eq!(
        corners(t),
        [triangle[0], triangle[1], triangle[2], triangle[2]]
    );
    assert_eq!(corners(q), [square[0], square[1], square[3], square[2]]);
    // Half transparent; a four-cornered SOLID at the pen's width, a
    // triangle closed by its operator not.
    let half = document::types::Transparency::from_alpha_value(0x0200_007F);
    for common in [&t.common, &q.common, &h.common] {
        assert_eq!(common.layer, "PDF_Solid Fills");
        assert_eq!(common.transparency, half);
        assert_eq!(common.color, Color::ByLayer);
    }
    assert_eq!(t.common.line_weight, LineWeight::ByLayer);
    assert_eq!(q.common.line_weight, LineWeight::Value(35));
    // Five corners: a solid hatch of one boundary.
    assert!(h.is_solid);
    assert_eq!(h.paths.len(), 1);
    let BoundaryEdge::Polyline(edge) = &h.paths[0].edges[0] else {
        panic!("{:?}", h.paths[0].edges);
    };
    assert_eq!(
        edge.vertices.iter().map(|v| [v.x, v.y]).collect::<Vec<_>>(),
        pentagon
    );
    // The fill layer, in the fill colour. (The pen's layer is listed
    // too, though no line is on it.)
    assert_eq!(
        result.layers[0],
        ("PDF_Solid Fills".to_string(), Color::Index(5))
    );

    // "Fills as hatches": the four-cornered one too.
    let hatches = PdfImportSettings {
        hatches: true,
        ..PdfImportSettings::default()
    };
    let result = import_with(content(), hatches, &underlay([0.0, 0.0], 1.0, 0.0));
    assert!(
        matches!(
            result.entities.as_slice(),
            [
                EntityType::Solid(_),
                EntityType::Hatch(_),
                EntityType::Hatch(_)
            ]
        ),
        "{:?}",
        result.entities
    );

    // Fills off: nothing.
    let no_fills = PdfImportSettings {
        fills: false,
        ..PdfImportSettings::default()
    };
    assert!(
        import_with(content(), no_fills, &underlay([0.0, 0.0], 1.0, 0.0))
            .entities
            .is_empty()
    );

    // A fill on one line bounds no area: a closed line in the pen.
    let result = import(page(vec![filled(
        &[&[[0.0, 0.0], [1.0, 0.0], [2.0, 0.0]]],
        blue,
    )]));
    let pls = polylines(&result);
    assert_eq!(result.entities.len(), 1);
    assert!(pls[0].is_closed);
    assert_eq!(pls[0].common.layer, "PDF_Geometry");
    assert!(result
        .layers
        .contains(&("PDF_Geometry".to_string(), Color::Index(7))));
}

/// A text run of a font no system has.
fn text(value: &str, origin: [f64; 2], rotation: f64) -> PdfText {
    PdfText {
        text: value.to_string(),
        font: "NoSuchFont".to_string(),
        origin,
        cap_height: 0.0,
        width: 1.0,
        rotation,
        color: [255, 0, 0],
        em: 0.25,
    }
}

#[test]
fn text_becomes_mtext_in_a_style_named_after_its_font() {
    let u = underlay([10.0, 20.0], 2.0, 0.5);
    let content = PageVectors {
        texts: vec![text("Hello  ", [1.0, 3.0], 0.3)],
        ..PageVectors::default()
    };
    let result = import_with(content, PdfImportSettings::default(), &u);
    let [EntityType::MText(m)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert_eq!(m.value, "Hello");
    assert_eq!(m.style, "PDF NoSuchFont");
    // A font no installed family answers to gets Arial's file.
    assert_eq!(
        result.text_styles,
        [("PDF NoSuchFont".to_string(), "arial.ttf".to_string())]
    );
    assert_eq!(m.attachment_point, AttachmentPoint::MiddleLeft);
    assert!(near(m.rotation, 0.8));
    assert_eq!(m.rectangle_width, 0.0);
    // The height is a capital height: about 0.72 of the em (the exact
    // share is the installed Arial's, where there is one), scaled.
    let cap = m.height / 2.0;
    assert!((0.6..0.8).contains(&(cap / 0.25)), "{}", cap / 0.25);
    // Middle left: half that height up from the baseline's start, along
    // the run's upright.
    let mid = [
        1.0 - 0.3f64.sin() * cap / 2.0,
        3.0 + 0.3f64.cos() * cap / 2.0,
    ];
    let w = crate::entities::underlay::local_to_world(&u, mid);
    assert_at(xy(m.insertion_point), [w[0], w[1]], "insertion");
    assert_eq!(m.common.layer, "PDF_Text");
    assert_eq!(result.layers, [("PDF_Text".to_string(), Color::Index(1))]);

    // Text off: none.
    let off = PdfImportSettings {
        text: false,
        ..PdfImportSettings::default()
    };
    let content = PageVectors {
        texts: vec![text("Hello", [1.0, 3.0], 0.0)],
        ..PageVectors::default()
    };
    assert!(import_with(content, off, &u).entities.is_empty());
}

fn picture(origin: [f64; 2]) -> PdfImage {
    PdfImage {
        rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        width: 2,
        height: 1,
        origin,
        u: [0.5, 0.0],
        v: [0.0, 0.5],
    }
}

#[test]
fn images_are_placed_with_their_pixel_steps_when_asked_for() {
    let content = || PageVectors {
        images: vec![picture([1.0, 1.0])],
        ..PageVectors::default()
    };
    let u = underlay([100.0, 200.0], 2.0, FRAC_PI_2);
    // Off by default.
    assert!(import_with(content(), PdfImportSettings::default(), &u)
        .images
        .is_empty());
    let raster = PdfImportSettings {
        raster: true,
        ..PdfImportSettings::default()
    };
    let result = import_with(content(), raster, &u);
    let [image] = result.images.as_slice() else {
        panic!("{} images", result.images.len());
    };
    assert_eq!((image.width, image.height), (2, 1));
    assert_eq!(image.rgba, [255, 0, 0, 255, 0, 0, 255, 255]);
    assert_at(xy(image.insertion), [98.0, 202.0], "insertion");
    // Half an inch per pixel, twice the size, a quarter turn.
    assert_at(xy(image.u), [0.0, 1.0], "pixel along");
    assert_at(xy(image.v), [-1.0, 0.0], "pixel up");
    assert_eq!(image.layer, "PDF_Images");
    assert_eq!(result.layers, [("PDF_Images".to_string(), Color::Index(7))]);
}

#[test]
fn layers_follow_the_layer_setting() {
    let content = || {
        vec![(
            Some("Walls".to_string()),
            PageVectors {
                paths: vec![
                    red(&[[0.0, 0.0], [1.0, 0.0]]),
                    filled(&[&[[2.0, 0.0], [3.0, 0.0], [2.5, 1.0]]], [0, 0, 255]),
                ],
                texts: vec![text("Note", [5.0, 5.0], 0.0)],
                ..PageVectors::default()
            },
        )]
    };
    let layers = |layers, prefix: &str| {
        let naming = LayerNaming {
            prefix: prefix.to_string(),
            ..naming(PdfImportSettings {
                layers,
                ..PdfImportSettings::default()
            })
        };
        let result = convert(
            &content(),
            &underlay([0.0, 0.0], 1.0, 0.0),
            &ImportArea::All,
            &naming,
        );
        let mut names: Vec<String> = result
            .entities
            .iter()
            .map(|e| e.common().layer.clone())
            .collect();
        names.sort();
        names
    };
    // The PDF's layer for geometry and fills; text to the text layer.
    assert_eq!(
        layers(ImportLayers::Pdf, "PDF_"),
        ["PDF_Text", "PDF_Walls", "PDF_Walls"]
    );
    assert_eq!(
        layers(ImportLayers::Pdf, "PDF2_"),
        ["PDF2_Text", "PDF2_Walls", "PDF2_Walls"]
    );
    assert_eq!(
        layers(ImportLayers::Object, "PDF_"),
        ["PDF_Geometry", "PDF_Solid Fills", "PDF_Text"]
    );
    assert_eq!(layers(ImportLayers::Current, "PDF_"), ["0", "0", "0"]);
}

#[test]
fn the_import_area_keeps_what_reaches_into_it() {
    // The area (0, 0)–(2, 2) in page inches, given in the drawing: the
    // underlay is at (10, 10), twice the size.
    let u = underlay([10.0, 10.0], 2.0, 0.0);
    let area = ImportArea::Polygon(
        [[10.0, 10.0], [14.0, 10.0], [14.0, 14.0], [10.0, 14.0]]
            .map(|[x, y]| DVec3::new(x, y, 0.0))
            .to_vec(),
    );
    let content = PageVectors {
        paths: vec![
            red(&[[0.5, 0.5], [1.5, 0.5]]),
            red(&[[3.0, 3.0], [4.0, 3.0]]),
            red(&[[1.0, 1.5], [5.0, 1.5]]),
            // Beside the area: at its height, past its right side.
            red(&[[2.5, 1.0], [3.5, 1.0]]),
        ],
        texts: vec![text("In", [0.5, 1.0], 0.0), text("Out", [5.0, 5.0], 0.0)],
        images: vec![picture([1.0, 0.25]), picture([4.0, 4.0])],
        ..PageVectors::default()
    };
    let settings = PdfImportSettings {
        raster: true,
        ..PdfImportSettings::default()
    };
    let result = convert(&[(None, content)], &u, &area, &naming(settings));
    // The line inside and the one reaching in (kept whole).
    let pls = polylines(&result);
    assert_eq!(pls.len(), 2);
    assert_at(vertices(pls[1])[1], [20.0, 13.0], "reaching line end");
    let texts: Vec<&str> = result
        .entities
        .iter()
        .filter_map(|e| match e {
            EntityType::MText(m) => Some(m.value.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["In"]);
    assert_eq!(result.images.len(), 1);
    assert_at(xy(result.images[0].insertion), [12.0, 10.5], "image inside");
}

#[test]
fn a_layer_takes_the_colour_most_of_its_objects_have() {
    let blue = |points: &[[f64; 2]]| stroked(lines(points), false, [0, 0, 255], 1.0);
    let result = import(page(vec![
        blue(&[[0.0, 0.0], [1.0, 0.0]]),
        red(&[[0.0, 1.0], [1.0, 1.0]]),
        red(&[[0.0, 2.0], [1.0, 2.0]]),
    ]));
    assert_eq!(
        result.layers,
        [("PDF_Geometry".to_string(), Color::Index(1))]
    );
    let colors: Vec<Color> = polylines(&result).iter().map(|p| p.common.color).collect();
    assert_eq!(colors, [Color::Index(5), Color::ByLayer, Color::ByLayer]);
    // A tie goes to the first colour.
    let result = import(page(vec![
        blue(&[[0.0, 0.0], [1.0, 0.0]]),
        red(&[[0.0, 1.0], [1.0, 1.0]]),
    ]));
    assert_eq!(
        result.layers,
        [("PDF_Geometry".to_string(), Color::Index(5))]
    );
}

#[test]
fn equal_dashes_in_a_row_become_one_dashed_line() {
    let dashes = |gaps: &[f64]| {
        let mut x = 0.0;
        let mut paths = vec![red(&[[x, 0.0], [x + 0.5, 0.0]])];
        for gap in gaps {
            x += 0.5 + gap;
            paths.push(red(&[[x, 0.0], [x + 0.5, 0.0]]));
        }
        page(paths)
    };
    let linetypes = PdfImportSettings {
        linetypes: true,
        ..PdfImportSettings::default()
    };
    let u = underlay([0.0, 0.0], 2.0, 0.0);
    let result = import_with(dashes(&[0.5, 0.5, 0.5]), linetypes, &u);
    let pls = polylines(&result);
    assert_eq!(pls.len(), 1);
    assert_eq!(vertices(pls[0]), [[0.0, 0.0], [7.0, 0.0]]);
    assert_eq!(pls[0].common.linetype, DASH_LINETYPE);
    // The dash length, scaled.
    assert!(near(pls[0].common.linetype_scale, 1.0));
    assert!(result.dashed);
    // Two dashes, uneven gaps, or "Infer linetypes" off: lines apart.
    assert_eq!(
        polylines(&import_with(dashes(&[0.5]), linetypes, &u)).len(),
        2
    );
    assert_eq!(
        polylines(&import_with(dashes(&[0.5, 0.25]), linetypes, &u)).len(),
        3
    );
    let off = import_with(dashes(&[0.5, 0.5, 0.5]), PdfImportSettings::default(), &u);
    assert_eq!(polylines(&off).len(), 4);
    assert!(!off.dashed);
}

#[test]
fn a_clip_cuts_straight_lines_and_fills() {
    let clip = vec![[1.0, -1.0], [2.0, -1.0], [2.0, 2.0], [1.0, 2.0]];
    let mut line_out = red(&[[0.0, 0.0], [3.0, 0.0]]);
    line_out.clip = Some(clip.clone());
    let mut fill_out = filled(
        &[&[[0.0, 0.5], [3.0, 0.5], [3.0, 1.5], [0.0, 1.5]]],
        [0, 0, 255],
    );
    fill_out.clip = Some(clip);
    let result = import(page(vec![line_out, fill_out]));
    let [EntityType::Hatch(h), EntityType::LwPolyline(pl)] = result.entities.as_slice() else {
        panic!("{:?}", result.entities);
    };
    assert_eq!(vertices(pl), [[1.0, 0.0], [2.0, 0.0]]);
    let BoundaryEdge::Polyline(edge) = &h.paths[0].edges[0] else {
        panic!("{:?}", h.paths[0].edges);
    };
    assert!(
        edge.vertices.iter().all(|v| (1.0..=2.0).contains(&v.x)),
        "{:?}",
        edge.vertices
    );
    assert_eq!(edge.vertices.len(), 4);
}

#[test]
fn strokes_far_out_on_the_page_still_join() {
    // Points beyond the joining grid's range (about 9.2e11 inches)
    // fall in its last cell, either way.
    for far in [1.0e12, -1.0e12] {
        let result = import(page(vec![
            red(&[[far, 0.0], [far, 1.0]]),
            red(&[[far, 1.0], [far, 2.0]]),
        ]));
        let pls = polylines(&result);
        assert_eq!(pls.len(), 1, "{far}");
        assert_eq!(vertices(pls[0]), [[far, 0.0], [far, 1.0], [far, 2.0]]);
    }
}

/// Content no well-formed page gives, in every setting that changes
/// what is made: no panic.
#[test]
fn odd_values_never_panic() {
    let (nan, inf) = (f64::NAN, f64::INFINITY);
    let odd_paths = vec![
        red(&[[nan, 0.0], [1.0, nan]]),
        red(&[[inf, 0.0], [-inf, 1.0]]),
        red(&[[0.0, 0.0], [0.0, 0.0]]),
        red(&[[1.0, 1.0], [1.0, 1.0], [1.0, 1.0]]),
        stroked(lines(&[[0.0, 0.0], [1.0, 0.0]]), false, [0, 0, 0], nan),
        stroked(lines(&[[0.0, 0.0], [1.0, 0.0]]), false, [0, 0, 0], -1.0),
        stroked(lines(&[[0.0, 0.0], [1.0, 0.0]]), false, [0, 0, 0], inf),
        stroked(circle([0.0, 0.0], 0.0), true, [0, 0, 0], 1.0),
        stroked(circle([nan, 0.0], 1.0), true, [0, 0, 0], 1.0),
        stroked(
            vec![Segment::Cubic(
                [0.0, 0.0],
                [0.0, 0.0],
                [0.0, 0.0],
                [0.0, 0.0],
            )],
            false,
            [0, 0, 0],
            1.0,
        ),
        filled(&[&[[0.0, 0.0], [1.0, 0.0]]], [0, 0, 0]),
        filled(&[&[[0.0, 0.0], [0.0, 0.0], [0.0, 0.0]]], [0, 0, 0]),
        filled(&[&[[nan, 0.0], [1.0, 0.0], [0.0, inf]]], [0, 0, 0]),
        filled(
            &[
                &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
                &[[5.0, 5.0], [6.0, 5.0]],
            ],
            [0, 0, 0],
        ),
        {
            let mut p = red(&[[0.0, 0.0], [3.0, 0.0]]);
            p.clip = Some(vec![[nan, 0.0], [1.0, 1.0], [0.0, 1.0]]);
            p
        },
        {
            let mut p = filled(&[&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]], [0, 0, 0]);
            p.closes = vec![
                SubClose {
                    op: true,
                    line: true
                };
                5
            ];
            p
        },
        {
            let mut p = filled(&[&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]], [0, 0, 0]);
            p.closes.clear();
            p.pen = None;
            p
        },
    ];
    let odd_texts = vec![
        text("", [0.0, 0.0], 0.0),
        text("   ", [0.0, 0.0], 0.0),
        text("x", [nan, inf], nan),
        PdfText {
            em: 0.0,
            width: nan,
            ..text("y", [0.0, 0.0], 0.0)
        },
    ];
    let odd_images = vec![
        PdfImage {
            rgba: Vec::new(),
            width: 0,
            height: 0,
            ..picture([0.0, 0.0])
        },
        PdfImage {
            u: [nan, 0.0],
            ..picture([inf, 0.0])
        },
    ];
    let content = PageVectors {
        paths: odd_paths,
        texts: odd_texts,
        images: odd_images,
        ..PageVectors::default()
    };
    let mut runs = 0;
    for mode in 0..32 {
        for layers in [
            ImportLayers::Pdf,
            ImportLayers::Object,
            ImportLayers::Current,
        ] {
            let mut settings = PdfImportSettings {
                layers,
                raster: true,
                ..PdfImportSettings::default()
            };
            settings.set_mode(mode);
            for u in [
                underlay([0.0, 0.0], 1.0, 0.0),
                underlay([1.0, 2.0], -3.0, PI),
                underlay([0.0, 0.0], 0.0, 0.0),
            ] {
                for area in [
                    ImportArea::All,
                    ImportArea::Polygon(vec![
                        DVec3::new(0.0, 0.0, 0.0),
                        DVec3::new(nan, 1.0, 0.0),
                        DVec3::new(1.0, 1.0, 0.0),
                    ]),
                ] {
                    let content = [(
                        Some("L".to_string()),
                        PageVectors {
                            paths: content.paths.clone(),
                            texts: content.texts.clone(),
                            images: content.images.clone(),
                            ..PageVectors::default()
                        },
                    )];
                    let _ = convert(&content, &u, &area, &naming(settings));
                    runs += 1;
                }
            }
        }
    }
    assert_eq!(runs, 32 * 3 * 3 * 2);
}

// ── The PDFIMPORT command's prompts ─────────────────────────────────────────

fn fixture(path: &str) -> String {
    format!("{}/tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"))
}

/// Types `text` at the command's prompt.
fn typed(command: &mut PdfImportCommand, text: &str) -> CmdResult {
    command
        .on_text_input(text)
        .unwrap_or_else(|| panic!("{text:?} not taken at {:?}", command.prompt()))
}

fn error(result: CmdResult) -> String {
    match result {
        CmdResult::ReportError(message) => message,
        _ => panic!("not an error"),
    }
}

fn measurement(result: CmdResult) -> String {
    match result {
        CmdResult::ReportMeasurement(message) => message,
        _ => panic!("not a report"),
    }
}

fn needs_point(result: CmdResult) -> bool {
    matches!(result, CmdResult::NeedPoint)
}

/// -PDFIMPORT at the File option, the Inkscape fixture read and its page
/// asked for.
fn at_page() -> PdfImportCommand {
    let mut command = PdfImportCommand::command_line();
    assert_eq!(command.prompt(), "Select PDF underlay or [File] <File>:");
    assert!(needs_point(command.on_enter()));
    assert_eq!(command.prompt(), "Enter filename for PDF import:");
    assert!(needs_point(typed(
        &mut command,
        &fixture("pdf/inkscape.pdf")
    )));
    assert_eq!(command.prompt(), "Enter page number or [?] <1>:");
    command
}

#[test]
fn the_file_option_asks_for_the_page_insertion_scale_and_rotation() {
    let mut command = at_page();
    // `?` lists the pages, or finds one.
    assert!(needs_point(typed(&mut command, "?")));
    assert_eq!(command.prompt(), "Enter page to list <*>:");
    assert_eq!(measurement(typed(&mut command, "*")), "1");
    assert!(needs_point(typed(&mut command, "?")));
    assert_eq!(measurement(typed(&mut command, "1")), "Found page 1.");
    assert!(needs_point(typed(&mut command, "?")));
    assert_eq!(
        measurement(typed(&mut command, "7")),
        "No matching page found."
    );
    // Enter takes page 1.
    assert!(needs_point(command.on_enter()));
    assert_eq!(command.prompt(), "Specify insertion point <0,0>:");
    assert!(needs_point(command.on_point(DVec3::new(10.0, 20.0, 0.0))));
    assert_eq!(command.prompt(), "Specify scale factor <1.0>:");
    assert!(needs_point(typed(&mut command, "2")));
    assert_eq!(command.prompt(), "Specify rotation <0>:");
    let CmdResult::PdfImportFile(import) = typed(&mut command, "90") else {
        panic!("no import");
    };
    assert_eq!(import.path, fixture("pdf/inkscape.pdf"));
    assert_eq!(import.page, "1");
    assert_eq!(import.insertion, DVec3::new(10.0, 20.0, 0.0));
    assert_eq!(import.scale, 2.0);
    assert!((import.rotation - FRAC_PI_2).abs() < 1e-12);

    // A name without its extension is a PDF file; a second point gives the
    // scale, then the angle, from the insertion point.
    let mut command = PdfImportCommand::command_line();
    let _ = command.on_enter();
    let stem = fixture("pdf/inkscape");
    assert!(needs_point(typed(&mut command, &stem)));
    assert!(needs_point(typed(&mut command, "1")));
    let at = DVec3::new(1.0, 1.0, 0.0);
    let _ = command.on_point(at);
    assert!(needs_point(
        command.on_point(at + DVec3::new(3.0, 4.0, 0.0))
    ));
    let CmdResult::PdfImportFile(import) = command.on_point(at + DVec3::new(0.0, 2.0, 0.0)) else {
        panic!("no import");
    };
    assert_eq!(import.path, format!("{stem}.pdf"));
    assert_eq!(import.scale, 5.0);
    assert!((import.rotation - FRAC_PI_2).abs() < 1e-12);

    // Enter at each step takes the defaults: page 1, the origin, 1 and 0.
    let mut command = at_page();
    for _ in 0..4 {
        if let CmdResult::PdfImportFile(import) = command.on_enter() {
            assert_eq!(
                (import.insertion, import.scale, import.rotation),
                (DVec3::ZERO, 1.0, 0.0)
            );
            return;
        }
    }
    panic!("no import after the defaults");
}

#[test]
fn the_file_option_refuses_what_it_cannot_use() {
    let mut command = PdfImportCommand::command_line();
    let _ = command.on_enter();
    assert_eq!(error(command.on_enter()), "Filename cannot be blank");
    let missing = fixture("pdf/missing.pdf");
    assert!(error(typed(&mut command, &missing)).ends_with("missing.pdf does not exist."));
    // A file that is no PDF (it has no trailer) cannot be opened.
    let not_pdf = error(typed(&mut command, &fixture("pdf/README.md")));
    assert!(
        not_pdf.starts_with("This file cannot be opened."),
        "{not_pdf}"
    );
    // Still at the file name.
    assert_eq!(command.prompt(), "Enter filename for PDF import:");
    // A PDF with its objects but no cross-reference table or trailer, as
    // one cut off before its end: refused, though the interpreter could
    // rebuild it.
    let cut_off = "%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                   2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
                   3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>\nendobj\n";
    let path = std::env::temp_dir().join(format!("pdfimport-test-cut-{}.pdf", std::process::id()));
    std::fs::write(&path, cut_off).unwrap();
    let message = error(typed(&mut command, path.to_str().unwrap()));
    let _ = std::fs::remove_file(&path);
    assert!(
        message.starts_with("This file cannot be opened."),
        "{message}"
    );

    let mut command = at_page();
    assert_eq!(
        error(typed(&mut command, "x")),
        "Please enter page number or '?'."
    );
    assert_eq!(
        error(typed(&mut command, "-1")),
        "Please enter page number or '?'."
    );
    // Zero is asked again without a message.
    assert!(needs_point(typed(&mut command, "0")));
    assert_eq!(command.prompt(), "Enter page number or [?] <1>:");
    assert!(error(typed(&mut command, "2")).starts_with("There is no 2 in "));
    assert!(needs_point(typed(&mut command, "?")));
    assert_eq!(
        error(typed(&mut command, "x")),
        "Please enter page number or *."
    );

    let mut command = at_page();
    let _ = command.on_enter();
    let _ = command.on_enter();
    assert_eq!(
        error(typed(&mut command, "0")),
        "Value must be positive and nonzero."
    );
    assert_eq!(
        error(typed(&mut command, "big")),
        "Requires numeric distance or second point."
    );
    // A second point on the insertion point is a scale of 0.
    assert_eq!(
        error(command.on_point(DVec3::ZERO)),
        "Value must be positive and nonzero."
    );
    let _ = command.on_enter();
    assert_eq!(
        error(typed(&mut command, "steep")),
        "Requires valid numeric angle or second point."
    );
}

/// A PDF underlay, as the command is handed it when one is picked.
fn pdf_underlay() -> EntityType {
    EntityType::Underlay(Underlay::pdf())
}

#[test]
fn an_underlay_is_imported_by_its_area_and_what_becomes_of_it() {
    let handle = Handle::new(42);
    // Only a PDF underlay can be picked.
    let mut command = PdfImportCommand::new();
    assert!(command.needs_entity_pick());
    command.inject_picked_entity(EntityType::Line(document::entities::Line::new()));
    assert_eq!(
        error(command.on_entity_pick(handle, DVec3::ZERO)),
        "Object selected was not a PDF underlay."
    );
    command.inject_picked_entity(pdf_underlay());
    assert!(needs_point(command.on_entity_pick(handle, DVec3::ZERO)));
    assert_eq!(
        command.prompt(),
        "Specify first corner of area to import or [Polygonal/All/Settings] <All>:"
    );

    // Two corners give a rectangle; Keep.
    let a = DVec3::new(1.0, 2.0, 0.0);
    let b = DVec3::new(5.0, 7.0, 0.0);
    let _ = command.on_point(a);
    assert_eq!(command.prompt(), "Specify opposite corner:");
    let _ = command.on_point(b);
    assert_eq!(
        command.prompt(),
        "Keep, Detach or Unload PDF underlay? [Keep/Detach/Unload] <Unload>:"
    );
    assert_eq!(error(typed(&mut command, "X")), "Invalid option keyword.");
    let CmdResult::PdfImport(request) = typed(&mut command, "K") else {
        panic!("no import");
    };
    assert_eq!(request.underlay, handle);
    assert_eq!(request.mode, UnderlayMode::Keep);
    assert_eq!(
        request.area,
        ImportArea::Polygon(vec![
            a,
            DVec3::new(5.0, 2.0, 0.0),
            b,
            DVec3::new(1.0, 7.0, 0.0)
        ])
    );

    // A polygon, its last point taken back; All; the other modes.
    let mut command = PdfImportCommand::for_underlay(handle);
    assert_eq!(error(typed(&mut command, "Z")), "Invalid option keyword.");
    assert!(matches!(
        typed(&mut command, "S"),
        CmdResult::OpenPdfImportSettings
    ));
    assert!(needs_point(typed(&mut command, "P")));
    assert_eq!(command.prompt(), "Specify first point:");
    let corners = [
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(4.0, 0.0, 0.0),
        DVec3::new(9.0, 9.0, 0.0),
        DVec3::new(0.0, 4.0, 0.0),
    ];
    for p in &corners[..3] {
        let _ = command.on_point(*p);
    }
    assert_eq!(command.prompt(), "Specify next point or [Undo]:");
    let _ = typed(&mut command, "U");
    let _ = command.on_point(corners[3]);
    let _ = command.on_enter();
    let CmdResult::PdfImport(request) = typed(&mut command, "D") else {
        panic!("no import");
    };
    assert_eq!(
        request.area,
        ImportArea::Polygon(vec![corners[0], corners[1], corners[3]])
    );
    assert_eq!(request.mode, UnderlayMode::Detach);
    // Fewer than three points is no polygon yet.
    let mut command = PdfImportCommand::for_underlay(handle);
    let _ = typed(&mut command, "P");
    let _ = command.on_point(corners[0]);
    let _ = command.on_point(corners[1]);
    let _ = command.on_enter();
    assert_eq!(command.prompt(), "Specify next point or [Undo]:");

    let mut command = PdfImportCommand::for_underlay(handle);
    let _ = command.on_enter();
    let CmdResult::PdfImport(request) = command.on_enter() else {
        panic!("no import");
    };
    assert_eq!(
        (request.area, request.mode),
        (ImportArea::All, UnderlayMode::Unload)
    );
}

#[test]
fn a_file_import_is_placed_at_its_point_scale_and_rotation() {
    let import = PdfFileImport {
        path: fixture("pdf/inkscape.pdf"),
        page: "1".to_string(),
        scale: 2.5,
        rotation: 0.5,
        insertion: DVec3::new(3.0, 4.0, 5.0),
    };
    crate::scene::model::pdf_raster::register_source(
        &import.path,
        std::sync::Arc::new(std::fs::read(&import.path).unwrap()),
    );
    let u = import.placement();
    assert_eq!(u.underlay_type, UnderlayType::Pdf);
    assert_eq!(
        (
            u.insertion_point.x,
            u.insertion_point.y,
            u.insertion_point.z
        ),
        (3.0, 4.0, 5.0)
    );
    // A page without a viewport measure: one drawing unit per page inch.
    assert_eq!((u.x_scale, u.y_scale), (2.5, 2.5));
    assert_eq!(u.rotation, 0.5);
}
