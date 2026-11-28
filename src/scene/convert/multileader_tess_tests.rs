// Tests of how a multileader is drawn (`entities::multileader`).
use document::entities::multileader::TextAttachmentType;
use document::entities::{LeaderContentType, MultiLeader, MultiLeaderPathType};
use document::Document;

use crate::display::wire_model::WireModel;
use crate::entities::traits::PropertyEditable;
use crate::scene::convert::multileader_tess::MultiLeaderTess;

const H: f64 = 125.0;
const GAP: f64 = 25.0;
const DOGLEG: f64 = 18.0;

#[test]
fn mleader_arabic_text_tessellation() {
    let doc = Document::default();
    let mut ml = MultiLeader::default();
    ml.content_type = LeaderContentType::MText;
    ml.context.text_string = "عربي".to_string();
    ml.context.text_height = 10.0;
    ml.path_type = MultiLeaderPathType::StraightLineSegments;

    let wires = ml.tessellate(
        &doc,
        document::Handle::default(),
        false,
        [1.0, 1.0, 1.0, 1.0],
        1.0,
        1.0,
        None,
        None,
        None,
        None,
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );

    // Find the text wire model:
    // Arabic text requires shaping, so it should emit vector fill_tris
    // instead of unshaped SDF glyph quads (text_verts).
    let text_wire = wires
        .iter()
        .find(|w| !w.fill_tris.is_empty() || !w.text_verts.is_empty())
        .expect("Should produce a text wire for mleader");

    assert!(
        text_wire.text_verts.is_empty(),
        "Arabic text must not be emitted as unshaped SDF glyph quads"
    );
    assert!(
        !text_wire.fill_tris.is_empty(),
        "Arabic text must emit shaped vector fill triangles"
    );
}

#[test]
fn background_mask_switch_masks_with_background() {
    // Switching "Background mask" on in Properties must mask with the
    // drawing background, not fill with the inherited text colour and
    // hide the text (#2131).
    let doc = Document::default();
    let mut ml = MultiLeader::default();
    ml.content_type = LeaderContentType::MText;
    ml.context.text_string = "123".to_string();
    ml.context.text_height = 10.0;
    ml.path_type = MultiLeaderPathType::StraightLineSegments;
    PropertyEditable::apply_geom_prop(&mut ml, "background_mask_fill_on", "toggle");
    assert!(ml.context.background_mask_fill_on);
    assert!(ml.context.background_fill_enabled);

    let text_color = [1.0, 1.0, 1.0, 1.0];
    let bg_color = [0.1, 0.2, 0.3, 1.0];
    let wires = ml.tessellate(
        &doc,
        document::Handle::default(),
        false,
        text_color,
        1.0,
        1.0,
        None,
        None,
        None,
        None,
        None,
        bg_color,
        1.0,
    );
    let mask = wires
        .iter()
        .find(|w| w.fill_tris.len() == 6 && w.points.is_empty() && w.text_verts.is_empty())
        .expect("Should produce a background mask quad");
    assert_eq!(mask.color, bg_color);
    assert!(
        mask.bg_adapt
            .as_deref()
            .is_some_and(|adapt| adapt.canvas_color),
        "the drawing-background mask must keep its semantic canvas tag"
    );
}

#[test]
fn background_fill_follows_codes_292_and_291() {
    // Code 292 switches the text background fill on and code 291 picks
    // the drawing background as its colour. A label saved with 291 on and
    // 292 off has no fill: it used to be filled with its stored grey.
    let mask_color = |color_on: bool, fill_on: bool| {
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        ml.context.text_string = "LABEL".to_string();
        ml.context.text_height = 10.0;
        ml.path_type = MultiLeaderPathType::Invisible;
        ml.context.background_fill_color = document::types::Color::from_rgb(200, 200, 200);
        ml.context.background_scale_factor = 1.5;
        ml.context.background_fill_enabled = color_on;
        ml.context.background_mask_fill_on = fill_on;
        let wires = ml.tessellate(
            &Document::default(),
            document::Handle::default(),
            false,
            [1.0, 1.0, 0.0, 1.0],
            1.0,
            1.0,
            None,
            None,
            None,
            None,
            None,
            [0.1, 0.2, 0.3, 1.0],
            1.0,
        );
        wires
            .iter()
            .find(|w| w.fill_tris.len() == 6 && w.points.is_empty() && w.text_verts.is_empty())
            .map(|w| w.color)
    };
    let grey = 200.0 / 255.0;

    assert_eq!(mask_color(true, false), None, "291 alone draws no fill");
    let stored = mask_color(false, true).expect("292 fills");
    assert!(
        stored[..3].iter().all(|c| (c - grey).abs() < 1e-3),
        "{stored:?}"
    );
    assert_eq!(mask_color(true, true), Some([0.1, 0.2, 0.3, 1.0]));
    assert_eq!(mask_color(false, false), None);
}

/// Vertical extent of everything a tessellated MLEADER draws.
fn drawn_height(wires: &[crate::display::wire_model::WireModel]) -> f32 {
    let ys = wires.iter().flat_map(|w| {
        w.points
            .iter()
            .chain(w.fill_tris.iter())
            .map(|p| p[1])
            .chain(w.text_verts.iter().map(|v| v.pos[1]))
            .filter(|y| y.is_finite())
            .collect::<Vec<_>>()
    });
    let (lo, hi) = ys.fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
    hi - lo
}

#[test]
fn annotative_mleader_ignores_overall_scale() {
    // AutoCAD greys out the overall scale of an annotative MLEADER and
    // draws it at its context height. A file with overall scale 50 at a
    // 1:50 annotation scale rendered its text 50× too large.
    let doc = Document::default();
    let make = |overall: f64| {
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        ml.context.text_string = "22x2.0".to_string();
        ml.enable_annotation_scale = true;
        ml.scale_factor = overall;
        ml.context.scale_factor = 50.0;
        ml.context.text_height = 250.0;
        ml
    };
    let height = |ml: &MultiLeader| {
        drawn_height(&ml.tessellate(
            &doc,
            document::Handle::default(),
            false,
            [1.0, 1.0, 1.0, 1.0],
            1.0,
            50.0,
            None,
            None,
            None,
            None,
            None,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
        ))
    };

    let unit = height(&make(1.0));
    let scaled = height(&make(50.0));
    assert!(unit > 0.0, "the text must be drawn");
    assert!(
        (scaled - unit).abs() <= unit * 1.0e-3,
        "overall scale must not resize an annotative MLEADER: {unit} vs {scaled}"
    );
}

#[test]
fn mleader_right_underline_attachment_meets_landing() {
    // Text left of its leader attaches with its RIGHT attachment
    // ("underline bottom line" here, while the left one is "underline top
    // line"): the underline must lie on the landing and run on to it, and
    // extending the leader to the text must not climb to the text corner.
    use document::types::Vector3;
    let doc = Document::default();
    let mut ml = MultiLeader::default();
    ml.content_type = LeaderContentType::MText;
    ml.path_type = MultiLeaderPathType::StraightLineSegments;
    ml.enable_landing = true;
    ml.enable_dogleg = true;
    ml.extend_leader_to_text = true;
    let ctx = &mut ml.context;
    ctx.text_string = "MULTILEADER\\PTEST".to_string();
    ctx.text_height = 1.0;
    ctx.landing_gap = 0.5;
    ctx.text_location = Vector3::new(7.5, 5.0, 0.0);
    ctx.text_attachment_point = document::entities::multileader::TextAttachmentPointType::TopRight;
    ctx.text_left_attachment = TextAttachmentType::BottomOfTopLineUnderlineBottomLine;
    ctx.text_right_attachment = TextAttachmentType::BottomLine;
    let root = ctx.add_leader_root();
    root.connection_point = Vector3::new(10.0, 0.0, 0.0);
    root.direction = Vector3::new(-1.0, 0.0, 0.0);
    root.landing_distance = 2.0;
    root.create_line(vec![
        Vector3::new(15.0, -5.0, 0.0),
        Vector3::new(10.0, 0.0, 0.0),
    ]);

    let wires = ml.tessellate(
        &doc,
        document::Handle::default(),
        false,
        [1.0, 1.0, 1.0, 1.0],
        1.0,
        1.0,
        None,
        None,
        None,
        None,
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );
    let leader_top = wires[0]
        .points
        .iter()
        .filter(|p| p[0].is_finite())
        .fold(f32::MIN, |m, p| m.max(p[1]));
    assert!(
        leader_top <= 1.0e-4,
        "leader rises above the landing: {leader_top}"
    );
    let underline = wires
        .iter()
        .find(|w| w.points.len() == 2 && w.text_verts.is_empty())
        .expect("an underline is drawn");
    for p in &underline.points {
        assert!(p[1].abs() < 1.0e-3, "underline off the landing: {p:?}");
    }
    let right = underline.points[0][0].max(underline.points[1][0]);
    assert!(
        (right - 8.0).abs() < 1.0e-3,
        "underline stops short of the landing: {right}"
    );
}

/// A text-right MLEADER whose landing runs from (0,0) to (DOGLEG,0).
fn landed_leader(text: &str, attach: TextAttachmentType) -> MultiLeader {
    use document::entities::multileader::{LeaderLine, LeaderRoot, TextAttachmentPointType};
    use document::types::Vector3;

    let mut ml = MultiLeader::default();
    ml.content_type = LeaderContentType::MText;
    ml.path_type = MultiLeaderPathType::StraightLineSegments;
    ml.context.text_string = text.to_string();
    ml.context.text_height = H;
    ml.context.landing_gap = GAP;
    ml.context.text_attachment_point = TextAttachmentPointType::TopLeft;
    ml.context.text_left_attachment = attach;
    ml.context.text_location = Vector3::new(DOGLEG + GAP, 1.2 * H, 0.0);
    let mut line = LeaderLine::default();
    line.points = vec![Vector3::new(-341.0, -387.0, 0.0)];
    let mut root = LeaderRoot::default();
    root.connection_point = Vector3::new(0.0, 0.0, 0.0);
    root.direction = Vector3::new(1.0, 0.0, 0.0);
    root.landing_distance = DOGLEG;
    root.lines = vec![line];
    ml.context.leader_roots = vec![root];
    ml
}

fn tess(ml: &MultiLeader) -> Vec<WireModel> {
    ml.tessellate(
        &Document::default(),
        document::Handle::default(),
        false,
        [1.0, 1.0, 1.0, 1.0],
        1.0,
        1.0,
        None,
        None,
        None,
        None,
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    )
}

/// Every drawn position of an MLEADER (leader, mask, glyphs, searchable
/// text), read back in f64 and moved by `-offset`.
fn drawn_positions(wires: &[WireModel], offset: [f64; 2]) -> Vec<[f64; 2]> {
    let ds = |hi: &[[f32; 3]], lo: &[[f32; 3]]| -> Vec<[f64; 2]> {
        hi.iter()
            .enumerate()
            .filter(|(_, p)| p[0].is_finite())
            .map(|(i, p)| {
                let l = lo.get(i).copied().unwrap_or([0.0; 3]);
                [
                    p[0] as f64 + l[0] as f64 - offset[0],
                    p[1] as f64 + l[1] as f64 - offset[1],
                ]
            })
            .collect()
    };
    let mut out = Vec::new();
    for w in wires {
        out.extend(ds(&w.points, &w.points_low.iter().collect::<Vec<_>>()));
        out.extend(ds(&w.fill_tris, &w.fill_tris_low));
        let (pos, low): (Vec<_>, Vec<_>) = w.text_verts.iter().map(|v| (v.pos, v.pos_low)).unzip();
        out.extend(ds(&pos, &low));
        out.extend(
            w.searchable_text
                .iter()
                .map(|run| [run.origin[0] - offset[0], run.origin[1] - offset[1]]),
        );
    }
    out
}

#[test]
fn far_from_origin_label_draws_as_near_the_origin() {
    // Survey drawings sit where f32 steps are tens of units: the leader,
    // mask and text must keep the shape they have near the origin, not
    // collapse to a few rounded points.
    let mut near = landed_leader(
        "DREN 1\\PPROFUNDIDAD=1.80m",
        TextAttachmentType::MiddleOfText,
    );
    near.context.background_fill_enabled = true;
    near.context.background_mask_fill_on = true;
    near.context.background_scale_factor = 1.5;
    let offset = [334_050_780.44, 6_218_261_561.70];
    let mut far = near.clone();
    far.translate(document::types::Vector3::new(offset[0], offset[1], 0.0));

    let near_pts = drawn_positions(&tess(&near), [0.0, 0.0]);
    let far_pts = drawn_positions(&tess(&far), offset);
    assert!(near_pts.len() > 10);
    assert_eq!(near_pts.len(), far_pts.len());
    for (a, b) in near_pts.iter().zip(&far_pts) {
        assert!(
            (a[0] - b[0]).abs() < 1.0e-3 && (a[1] - b[1]).abs() < 1.0e-3,
            "{a:?} is drawn at {b:?} far from the origin"
        );
    }
}

fn text_bottom(wires: &[WireModel]) -> f32 {
    wires
        .iter()
        .flat_map(|w| w.text_verts.iter().map(|v| v.pos[1]))
        .fold(f32::MAX, f32::min)
}

/// The landing's underline: the only bare two-point rule.
fn underline(wires: &[WireModel]) -> Option<[[f32; 3]; 2]> {
    wires
        .iter()
        .find(|w| w.points.len() == 2 && w.text_verts.is_empty() && w.fill_tris.is_empty())
        .map(|w| [w.points[0], w.points[1]])
}

#[test]
fn underline_attachment_runs_from_landing_end() {
    // Measured on AutoCAD 2025 by exploding an MLEADER with "underline"
    // attachment: the underline is the landing continued under the text,
    // from the dogleg's end to the far end of the text, and the text's
    // baseline sits 0.2 × height above it.
    let (h, gap, dogleg) = (H, GAP, DOGLEG);
    let ml = landed_leader(
        "fsfsdfs",
        TextAttachmentType::BottomOfTopLineUnderlineBottomLine,
    );
    let wires = tess(&ml);

    // Single-line text: "bottom of bottom line" puts the same baseline on
    // the landing without an underline, so the lift is the difference.
    let plain = landed_leader("fsfsdfs", TextAttachmentType::BottomOfBottomLine);
    let lift = text_bottom(&wires) - text_bottom(&tess(&plain));
    assert!(
        (lift - 0.2 * h as f32).abs() < 1.0e-2,
        "underlined text sits 0.2 × height above the landing, got {lift}"
    );

    let [a, b] = underline(&wires).expect("an underline rule");
    assert!(
        (a[0] - dogleg as f32).abs() < 1.0e-3,
        "underline starts at the landing end, got x = {}",
        a[0]
    );
    assert!(
        a[1].abs() < 1.0e-3 && b[1].abs() < 1.0e-3,
        "underline runs at the landing level, got y = {} / {}",
        a[1],
        b[1]
    );
    assert!(b[0] > (dogleg + gap) as f32, "underline spans the text");
}

#[test]
fn underline_attachment_values_match_autocad() {
    // AutoCAD 2025, three-line text, one MLEADER per attachment value
    // (see `underlined_line`): 5 underlines the bottom line, 6 and 8 the
    // top line, 7 draws no underline. The underlined line meets the
    // landing, so the text sits above it (5) or hangs below it (6, 8).
    let text = "rand unu\\Prand doi\\Prand trei";
    let cases = [
        (TextAttachmentType::BottomLine, true, true),
        (
            TextAttachmentType::BottomOfTopLineUnderlineBottomLine,
            true,
            false,
        ),
        (
            TextAttachmentType::BottomOfTopLineUnderlineTopLine,
            false,
            false,
        ),
        (TextAttachmentType::BottomOfTopLineUnderlineAll, true, false),
    ];
    for (attach, underlined, text_above) in cases {
        let wires = tess(&landed_leader(text, attach));
        match underline(&wires) {
            Some([a, b]) => {
                assert!(underlined, "{attach:?} must not draw an underline");
                assert!(
                    (a[0] - DOGLEG as f32).abs() < 1.0e-3 && a[1].abs() < 1.0e-3,
                    "{attach:?}: underline starts at the landing end, got {a:?}"
                );
                assert!(b[1].abs() < 1.0e-3, "{attach:?}: underline stays level");
            }
            None => assert!(!underlined, "{attach:?} must draw an underline"),
        }
        let bottom = text_bottom(&wires);
        if text_above {
            assert!(
                bottom > 0.0,
                "{attach:?}: text sits above the landing, got {bottom}"
            );
        } else {
            assert!(
                bottom < -H as f32,
                "{attach:?}: text hangs below the landing, got {bottom}"
            );
        }
    }
}

/// An angled label written with its anchor free of the connection — a
/// survey export leaves `text_location` a block height plus a gap away
/// from `connection_point` — must still have its attachment line ride the
/// leader's connection, exactly like an unrotated label. The ink must not
/// keep the stored, off-leader placement.
#[test]
fn rotated_text_attaches_to_its_connection() {
    use document::entities::multileader::TextAttachmentPointType;
    use document::types::Vector3;
    let doc = Document::default();
    // 55° text direction; the leader runs against the reading direction.
    let th = 1.0_f64;
    let (tx, ty) = ((55.0f64).to_radians().cos(), (55.0f64).to_radians().sin());
    let (up_x, up_y) = (-ty, tx);
    let build = |anchor_perp: f64| {
        let mut ml = MultiLeader::default();
        ml.content_type = LeaderContentType::MText;
        ml.path_type = MultiLeaderPathType::StraightLineSegments;
        ml.enable_landing = true;
        ml.enable_dogleg = true;
        let ctx = &mut ml.context;
        ctx.text_string = "R724\\P SR502-pilíř".to_string();
        ctx.text_height = th;
        ctx.landing_gap = 0.5;
        ctx.text_rotation = 0.0; // direction vector governs below
        ctx.text_direction = Vector3::new(tx, ty, 0.0);
        ctx.text_attachment_point = TextAttachmentPointType::TopCenter;
        ctx.text_left_attachment = TextAttachmentType::BottomLine;
        ctx.text_right_attachment = TextAttachmentType::BottomLine;
        // The connection the leader lands on; the writer left the anchor
        // `anchor_perp` up the text's up axis from it.
        ctx.text_location = Vector3::new(10.0 + up_x * anchor_perp, 5.0 + up_y * anchor_perp, 0.0);
        let root = ctx.add_leader_root();
        root.connection_point = Vector3::new(10.0, 5.0, 0.0);
        root.direction = Vector3::new(-tx, -ty, 0.0);
        root.landing_distance = 0.0;
        let mut line = document::entities::multileader::LeaderLine::default();
        line.points = vec![Vector3::new(10.0 + tx * 2.5, 5.0 + ty * 2.5, 0.0)];
        root.lines = vec![line];
        ml
    };
    let tess = |ml: &MultiLeader| {
        ml.tessellate(
            &doc,
            document::Handle::default(),
            false,
            [1.0, 1.0, 1.0, 1.0],
            1.0,
            1.0,
            None,
            None,
            None,
            None,
            None,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
        )
    };
    // Ink perp-distance above the connection: smallest projection of any
    // stroke point onto the text's up axis, measured from (10, 5).
    let ink_perp = |ml: &MultiLeader| {
        let wires = tess(ml);
        wires
            .iter()
            .skip(1) // wire 0 is the leader itself
            .flat_map(|w| w.points.iter().chain(w.fill_tris.iter()))
            .filter(|p| p[0].is_finite())
            .map(|p| (f64::from(p[0]) - 10.0) * up_x + (f64::from(p[1]) - 5.0) * up_y)
            .fold(f64::INFINITY, f64::min)
    };
    // A writer that parked the anchor a block height plus a gap above the
    // connection: before the fix the ink rode the raw anchor (~4·h away).
    let anchor_free = build(4.0 * th);
    let attached = ink_perp(&anchor_free);
    assert!(
        attached.abs() < 1.2 * th,
        "rotated ink stays {attached}·h above the connection"
    );
    let row = underline(&tess(&anchor_free)).expect("the bottom-line rule draws");
    let mut connection_distance = f64::INFINITY;
    for p in row {
        let perp = (f64::from(p[0]) - 10.0) * up_x + (f64::from(p[1]) - 5.0) * up_y;
        assert!(
            perp.abs() < 1.0e-4,
            "the rotated underline rides {perp} units off its connection"
        );
        connection_distance = connection_distance
            .min(((f64::from(p[0]) - 10.0).powi(2) + (f64::from(p[1]) - 5.0).powi(2)).sqrt());
    }
    assert!(
        connection_distance < 1.0e-4,
        "the rotated underline stops {connection_distance} units short of the connection"
    );
    // The approach edge must also sit one gap past the connection along
    // the reading direction — a writer that parked it half a block down
    // the row used to let the label pile onto its neighbour.
    let along_edge = |ml: &MultiLeader, sign: f64| {
        let wires = tess(ml);
        let (ux, uy) = (tx, ty);
        wires
            .iter()
            .skip(1)
            .flat_map(|w| w.searchable_text.iter())
            .filter(|r| r.origin[0].is_finite())
            .map(|r| (f64::from(r.origin[0]) - 10.0) * ux + (f64::from(r.origin[1]) - 5.0) * uy)
            .fold(sign * f64::INFINITY, |a, v| {
                if sign * v < a {
                    a
                } else {
                    sign * v.max(a)
                }
            })
    };
    let free_edge = along_edge(&anchor_free, -1.0);
    // Word origins sit upstream of the ink edge; what matters is that the
    // ink returned to the leader's approach side at all (before the fix
    // they hung opposite the connection), not floating blocks away.
    assert!(
        (free_edge < -0.2 * th) && (free_edge > -12.0 * th),
        "rotated ink's nearest word origin sits {free_edge}·h from the connection"
    );
    // An anchor already placed on the connection (this app's own command
    // writes one) must not move the block past the underline gap.
    let anchored = build(0.0);
    let riding = ink_perp(&anchored);
    assert!(
        riding.abs() < 1.2 * th,
        "an on-connection anchor is displaced to {riding}·h"
    );
}

/// The row of lines under a "line" (bottom line) attachment visually
/// continues the leader. A writer that stores no landing distance still
/// expects that row to touch the leader's connection — a gap between the
/// landing end (here: the connection itself) and the underline left the
/// text's lines dangling in space.
#[test]
fn underline_reaches_the_connection_without_a_landing() {
    use document::entities::multileader::TextAttachmentPointType;
    use document::types::Vector3;
    let doc = Document::default();
    let mut ml = MultiLeader::default();
    ml.content_type = LeaderContentType::MText;
    ml.path_type = MultiLeaderPathType::StraightLineSegments;
    ml.enable_landing = true;
    ml.enable_dogleg = true;
    let ctx = &mut ml.context;
    ctx.text_string = "SS100-pilíř\\P (štxhv 400x240x510)".to_string();
    ctx.text_height = 1.0;
    ctx.landing_gap = 0.5;
    ctx.text_attachment_point = TextAttachmentPointType::TopCenter;
    ctx.text_left_attachment = TextAttachmentType::BottomLine;
    ctx.text_right_attachment = TextAttachmentType::BottomLine;
    ctx.text_direction = Vector3::new(1.0, 0.0, 0.0);
    // Landing distance 0: the row's attachment line is the connection.
    // The stale anchor sits half a block too far down the row; the snap
    // brings the approach edge back to one gap past the connection.
    ctx.text_location = Vector3::new(3.0, 1.2, 0.0);
    let root = ctx.add_leader_root();
    root.connection_point = Vector3::new(0.0, 0.0, 0.0);
    root.direction = Vector3::new(1.0, 0.0, 0.0);
    root.landing_distance = 0.0;
    let mut line = document::entities::multileader::LeaderLine::default();
    line.points = vec![Vector3::new(-3.0, 0.0, 0.0)];
    root.lines = vec![line];
    ml.extend_leader_to_text = false;

    let wires = ml.tessellate(
        &doc,
        document::Handle::default(),
        false,
        [1.0, 1.0, 1.0, 1.0],
        1.0,
        1.0,
        None,
        None,
        None,
        None,
        None,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );
    let row = wires
        .iter()
        .find(|w| w.points.len() == 2 && w.text_verts.is_empty() && w.fill_tris.is_empty())
        .expect("the attachment row draws");
    assert!(
        (row.points[0][1] - row.points[1][1]).abs() < 1e-3,
        "the row is level: {:?}",
        row.points
    );
    for point in &row.points {
        assert!(
            point[1].abs() < 1e-3,
            "the bottom line's descenders must not lift its underline off the connection: {point:?}"
        );
    }
    // The leader advances with the reading direction: the row's left end
    // must touch the connection at the origin, past the landing gap.
    let left = row.points[0][0].min(row.points[1][0]);
    assert!(
        left.abs() < 1e-3,
        "the row stops short of the connection: left={left}"
    );

    // At survey coordinates, rebuilding the connection from an f32 text
    // insertion used to leave a visible residual even when the local math
    // was right. The joined endpoint must come from the leader itself.
    let mut far = ml.clone();
    far.translate(Vector3::new(-547_468.676429, -1_043_053.245158, 0.0));
    let far_row = underline(&tess(&far)).expect("the far-from-origin rule draws");
    let cp = far.context.leader_roots[0].connection_point;
    let rendered_cp = [cp.x as f32, cp.y as f32];
    let distance = far_row
        .iter()
        .map(|p| {
            ((f64::from(p[0]) - f64::from(rendered_cp[0])).powi(2)
                + (f64::from(p[1]) - f64::from(rendered_cp[1])).powi(2))
            .sqrt()
        })
        .fold(f64::INFINITY, f64::min);
    assert!(
        distance < 2.0e-5,
        "the survey-coordinate underline misses the leader by {distance}"
    );
    assert!(
        (far_row[0][1] - far_row[1][1]).abs() < 1.0e-5,
        "the survey-coordinate underline must stay level: {far_row:?}"
    );
}

#[test]
fn leader_type_none_draws_no_arrowhead_or_landing() {
    // A leader of type None (path type Invisible) shows only its content.
    // The arrowhead used to be drawn at the hidden leader's tip and the
    // landing beside the text, so a site plan's street labels picked up
    // stray dots and dashes.
    let tip = [-341.0, -387.0];
    let near = |p: &[f64; 2], q: [f64; 2], r: f64| (p[0] - q[0]).hypot(p[1] - q[1]) < r;
    let on_landing = |p: &[f64; 2]| p[1].abs() < 1.0 && p[0] > -1.0 && p[0] < DOGLEG;

    let mut shown = landed_leader("LABEL", TextAttachmentType::MiddleOfText);
    shown.context.arrowhead_size = 40.0;
    let shown_pts = drawn_positions(&tess(&shown), [0.0, 0.0]);
    assert!(shown_pts.iter().any(|p| near(p, tip, 50.0)));
    assert!(shown_pts.iter().any(on_landing));

    let mut hidden = shown.clone();
    hidden.path_type = MultiLeaderPathType::Invisible;
    let hidden_pts = drawn_positions(&tess(&hidden), [0.0, 0.0]);
    assert!(!hidden_pts.is_empty(), "the text still draws");
    assert!(
        !hidden_pts.iter().any(|p| near(p, tip, 50.0)),
        "no arrowhead at the hidden leader's tip"
    );
    assert!(!hidden_pts.iter().any(on_landing), "no landing");
}
