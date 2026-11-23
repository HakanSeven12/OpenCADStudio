//! Model files IMPORT reads: ACIS text (SAT), STEP and IGES, as the drawing
//! objects they become.

use document::entities::{Entity as _, Insert};
use document::types::{Color, Matrix4, Transform, Vector3};
use document::EntityType;

use crate::scene::convert::acis_import::{painted_body_entity, sat_entities};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelFormat {
    Sat,
    Step,
    Iges,
}

/// The objects a file brings in.
#[derive(Debug, Clone)]
pub struct Imported {
    /// Block definitions, each before any block that inserts it.
    pub blocks: Vec<(String, Vec<EntityType>)>,
    /// The objects for the current space.
    pub entities: Vec<EntityType>,
}

/// Why a file did not become drawing objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFailure {
    /// Nothing readable at the path.
    Missing,
    /// The file is not of the format, or holds nothing the importer reads.
    Unreadable,
}

/// File units over drawing units; one where the drawing is unitless.
fn ratio(file_mm: f64, drawing_mm: Option<f64>) -> f64 {
    drawing_mm
        .filter(|mm| *mm > 0.0)
        .map_or(1.0, |mm| file_mm / mm)
}

/// A block name from a product's, with the characters names cannot hold
/// replaced.
fn block_name(name: &str) -> String {
    crate::scene::sanitized_table_name(name, "Part")
}

/// A STEP presentation colour as the drawing colour it imports as: one of
/// the standard index colours red, yellow, green, cyan, blue, magenta,
/// white or the dark grey 250 (which black also becomes) when it is close
/// to it, otherwise the true colour. Measured on the 8-bit channels:
/// white is an intensity (channel mean) of at least 0.93 with an HSV
/// saturation of at most 0.2; grey 250 an intensity from 0.16 to 0.39 with
/// a saturation below 0.2; the six hues a value of at least 0.82, a
/// saturation of at least 0.75 and a hue within 7.5° of theirs (the hue
/// angle of the colour's projection across the grey axis).
pub fn step_colour(rgb: crate::io::step_import::Rgb) -> Color {
    let [r, g, b] = rgb.map(|c| (c.clamp(0.0, 1.0) * 255.0).round());
    let true_colour = Color::Rgb {
        r: r as u8,
        g: g as u8,
        b: b as u8,
    };
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max == 0.0 {
        return Color::Index(250);
    }
    let saturation = (max - min) / max;
    let intensity = (r + g + b) / 3.0 / 255.0;
    if saturation <= 0.2 && intensity >= 0.93 {
        return Color::Index(7);
    }
    if saturation < 0.2 && (0.16..=0.39).contains(&intensity) {
        return Color::Index(250);
    }
    if saturation < 0.75 || max / 255.0 < 0.82 {
        return true_colour;
    }
    let hue = (3f64.sqrt() / 2.0 * (g - b))
        .atan2(r - (g + b) / 2.0)
        .to_degrees();
    let sector = (hue / 60.0).round();
    if (hue - 60.0 * sector).abs() > 7.5 {
        return true_colour;
    }
    // Red, yellow, green, cyan, blue, magenta.
    Color::Index([1, 2, 3, 4, 5, 6][sector.rem_euclid(6.0) as usize])
}

/// An insert of `block` placed by `placement` (columns: axes, origin).
fn placed_insert(block: &str, placement: &[[f64; 4]; 3]) -> EntityType {
    let mut insert = Insert::new(block, Vector3::ZERO);
    let mut m = Matrix4::identity();
    for r in 0..3 {
        m.m[r] = placement[r];
    }
    insert.apply_transform(&Transform::from_matrix(m));
    EntityType::Insert(insert)
}

/// STEP: a block per product — its solids, and inserts of its components —
/// with each top product inserted at the origin, scaled from the file's
/// units to the drawing's. The solids take their colour from the block.
fn step_blocks(model: crate::io::step_import::StepModel, scale: f64, stem: &str) -> Imported {
    // Unique names, in product order.
    let mut names: Vec<String> = Vec::new();
    for product in &model.products {
        let base = match product.numbered {
            Some(id) => block_name(&format!("{stem}-{id}")),
            None => block_name(&product.name),
        };
        let mut name = base.clone();
        let mut n = 1;
        while names.iter().any(|taken| taken.eq_ignore_ascii_case(&name)) {
            name = format!("{base}_{n}");
            n += 1;
        }
        names.push(name);
    }
    // Children before parents. A link that closes a loop in the file's
    // assembly (a product that would contain itself) is dropped, so no
    // block ends up inserting itself.
    let mut order = Vec::new();
    let mut dropped: std::collections::HashSet<(usize, usize)> = Default::default();
    let mut state = vec![0u8; model.products.len()];
    for &root in &model.roots {
        if state[root] != 0 {
            continue;
        }
        state[root] = 1;
        // Each product being walked and the next of its children to visit.
        let mut stack = vec![(root, 0usize)];
        while let Some((k, next)) = stack.last_mut() {
            let k = *k;
            match model.products[k].children.get(*next) {
                Some(&(child, _)) => {
                    let link = (k, *next);
                    *next += 1;
                    match state[child] {
                        0 => {
                            state[child] = 1;
                            stack.push((child, 0));
                        }
                        1 => {
                            dropped.insert(link);
                        }
                        _ => {}
                    }
                }
                None => {
                    state[k] = 2;
                    order.push(k);
                    stack.pop();
                }
            }
        }
    }
    let blocks = order
        .iter()
        .map(|&k| {
            let product = &model.products[k];
            let mut entities: Vec<EntityType> = product
                .bodies
                .iter()
                .filter_map(|body| {
                    let faces: Vec<Option<Color>> =
                        body.faces.iter().map(|c| c.map(step_colour)).collect();
                    let colour = body.colour.map(step_colour);
                    let mut entity = painted_body_entity(&body.body, colour, &faces, false)?;
                    if product.scale != 1.0 {
                        entity.apply_transform(&Transform::from_scale(product.scale));
                    }
                    Some(entity)
                })
                .collect();
            entities.extend(
                product
                    .children
                    .iter()
                    .enumerate()
                    .filter(|(n, _)| !dropped.contains(&(k, *n)))
                    .map(|(_, (child, placement))| {
                        let mut insert = placed_insert(&names[*child], placement);
                        insert.common_mut().color = Color::ByBlock;
                        insert
                    }),
            );
            (names[k].clone(), entities)
        })
        .collect();
    let identity = [
        [scale, 0.0, 0.0, 0.0],
        [0.0, scale, 0.0, 0.0],
        [0.0, 0.0, scale, 0.0],
    ];
    // Solids placed straight into the drawing keep the colour by block
    // they have in a block.
    let mut direct: Vec<EntityType> = model
        .direct
        .iter()
        .filter_map(|body| {
            let faces: Vec<Option<Color>> = body.faces.iter().map(|c| c.map(step_colour)).collect();
            let colour = body.colour.map(step_colour);
            let mut entity = painted_body_entity(&body.body, colour, &faces, false)?;
            if scale != 1.0 {
                entity.apply_transform(&Transform::from_scale(scale));
            }
            Some(entity)
        })
        .collect();
    direct.extend(
        model
            .roots
            .iter()
            .map(|&k| placed_insert(&names[k], &identity)),
    );
    Imported {
        blocks,
        entities: direct,
    }
}

/// `None` when the file is not of `format` or holds nothing it can read.
/// `stem` names the blocks of a STEP file without products.
pub fn read(
    format: ModelFormat,
    bytes: &[u8],
    drawing_mm: Option<f64>,
    stem: &str,
) -> Option<Imported> {
    let imported = match format {
        ModelFormat::Sat => Imported {
            blocks: Vec::new(),
            entities: sat_entities(&String::from_utf8_lossy(bytes))?,
        },
        ModelFormat::Step => {
            let model = crate::io::step_import::read(bytes)?;
            // A file that names no length unit is read in drawing units.
            let scale = model.unit_mm.map_or(1.0, |mm| ratio(mm, drawing_mm));
            step_blocks(model, scale, stem)
        }
        ModelFormat::Iges => {
            // Solids, then the surfaces as surfaces however flat, then the
            // free curves as splines; faces and edges carry the object's
            // colour, every object its level's layer.
            let model = crate::io::iges_import::read(bytes, drawing_mm)?;
            let solids = model.bodies.iter().map(|body| (body, false));
            let sheets = model.sheets.iter().map(|body| (body, true));
            let mut entities: Vec<EntityType> = solids
                .chain(sheets)
                .filter_map(|((body, color, layer), sheet)| {
                    let mut entity = painted_body_entity(body, Some(*color), &[], sheet)?;
                    entity.common_mut().layer = layer.clone();
                    Some(entity)
                })
                .collect();
            for (curve, color, layer) in &model.curves {
                let mut spline = document::entities::Spline::new();
                crate::entities::spline::replace_with_nurbs(&mut spline, curve);
                spline.common.color = *color;
                spline.common.layer = layer.clone();
                entities.push(EntityType::Spline(spline));
            }
            Imported {
                blocks: Vec::new(),
                entities,
            }
        }
    };
    let solids = imported
        .blocks
        .iter()
        .flat_map(|(_, entities)| entities)
        .chain(&imported.entities)
        .any(|entity| !matches!(entity, EntityType::Insert(_)));
    solids.then_some(imported)
}

/// Reads and parses `path` — the work an import does off the UI thread.
/// `drawing_mm` is the drawing's unit; blocks are named after the file stem.
pub fn load(
    format: ModelFormat,
    path: &str,
    drawing_mm: Option<f64>,
) -> Result<Imported, ImportFailure> {
    let bytes =
        crate::scene::model::pdf_raster::source_bytes(path).ok_or(ImportFailure::Missing)?;
    let stem = std::path::Path::new(&path.replace('\\', "/"))
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    read(format, &bytes, drawing_mm, &stem).ok_or(ImportFailure::Unreadable)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `load` tells a file it cannot open from one it cannot read, so the
    /// command line can say which.
    #[test]
    fn load_reports_a_missing_file_and_an_unreadable_one() {
        assert!(matches!(
            load(ModelFormat::Step, "definitely-not-here.step", None),
            Err(ImportFailure::Missing)
        ));
        let path =
            std::env::temp_dir().join(format!("ocs-model-load-test-{}.step", std::process::id()));
        std::fs::write(&path, b"not a model").expect("write the scratch file");
        let result = load(
            ModelFormat::Step,
            path.to_str().expect("the scratch path is UTF-8"),
            None,
        );
        let _ = std::fs::remove_file(&path);
        assert!(matches!(result, Err(ImportFailure::Unreadable)));
    }

    /// Band edges measured on imported STEP colours (8-bit channels).
    #[test]
    fn step_colour_bands() {
        let index = |c: [u8; 3]| match step_colour(c.map(|v| f64::from(v) / 255.0)) {
            Color::Index(i) => i,
            _ => 0,
        };
        let cases: [([u8; 3], u8); 14] = [
            ([255, 36, 0], 1),
            ([255, 37, 0], 0),
            ([210, 0, 0], 1),
            ([209, 0, 0], 0),
            ([255, 63, 63], 1),
            ([255, 64, 64], 0),
            ([0, 0, 0], 250),
            ([1, 1, 1], 0),
            ([102, 99, 96], 250),
            ([42, 40, 40], 0),
            ([90, 90, 72], 0),
            ([255, 255, 204], 7),
            ([240, 240, 233], 7),
            ([237, 237, 237], 0),
        ];
        for (c, want) in cases {
            assert_eq!(index(c), want, "{c:?}");
        }
    }

    /// SAT text of several bodies, written by the app's own writer.
    fn sat_fixtures() -> Vec<String> {
        use ocs_solid::model::{
            boolean, make_box, make_cone, make_cylinder, make_sphere, make_torus,
        };
        let cut = boolean(
            &make_box([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]).unwrap(),
            &make_cylinder([5.0, 5.0, -1.0], 2.0, 12.0).unwrap(),
            ocs_solid::model::BooleanOp::Subtract,
        )
        .unwrap()
        .expect("a box with a hole");
        [
            make_box([0.0, 0.0, 0.0], [10.0, 20.0, 30.0]).unwrap(),
            make_cylinder([0.0, 0.0, 0.0], 5.0, 10.0).unwrap(),
            make_sphere([0.0, 0.0, 0.0], 5.0).unwrap(),
            make_cone([0.0, 0.0, 0.0], 5.0, 2.0, 10.0).unwrap(),
            make_torus([0.0, 0.0, 0.0], 10.0, 2.0).unwrap(),
            cut,
        ]
        .iter()
        .map(|body| {
            crate::scene::convert::acis_export::solid_to_sat(body)
                .expect("the body writes")
                .to_sat_string()
        })
        .collect()
    }

    /// Draws `entities` in a new drawing, as the app does after an import.
    fn draw(entities: Vec<EntityType>) {
        let mut scene = crate::scene::Scene::new();
        for entity in entities {
            scene.add_entity(entity);
        }
        let camera = crate::scene::view::camera::Camera::default();
        let _ = scene.model_tile_wires_arc(0, &camera, 1.0, 1.0);
        scene.fit_all();
    }

    /// Runs `work` on its own thread and fails if it does not end in
    /// `seconds`: a loop that never ends fails the test and does not hang
    /// the test run.
    fn ends_in(seconds: u64, what: &str, work: impl FnOnce() + Send + 'static) {
        let (done, wait) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            work();
            let _ = done.send(());
        });
        match wait.recv_timeout(std::time::Duration::from_secs(seconds)) {
            Ok(()) => worker.join().expect("the work ended"),
            // The work panicked: show its panic.
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                if let Err(panic) = worker.join() {
                    std::panic::resume_unwind(panic);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("{what} did not end in {seconds} s")
            }
        }
    }

    /// A SAT file can read correctly and still stop the app later: the
    /// solid kernel reads the records again when the body is drawn. An edge
    /// on a circle with a stored parameter of 1e308 made that step add
    /// turns to the angle without end.
    #[test]
    fn a_sat_edge_with_a_huge_parameter_does_not_hang_the_draw() {
        let cylinder = sat_fixtures().swap_remove(1);
        let edge = "edge $-1 -1 $-1 $3 0 $3 6.283185307179586 ";
        assert!(cylinder.contains(edge), "the rim edge of the cylinder");
        for (start, end) in [("1e308", "1e308"), ("-1e308", "-1e308"), ("inf", "inf")] {
            let damaged = cylinder.replace(edge, &format!("edge $-1 -1 $-1 $3 {start} $3 {end} "));
            // The kernel says that it lost the curve of that edge, and keeps
            // the face that does not use it.
            let parsed = document::entities::acis::SatDocument::parse(&damaged)
                .expect("the damaged file is still SAT text");
            let (bodies, loss) = ocs_solid::lift(&parsed);
            assert_eq!(loss.curves.len(), 1, "the curve of the edge: {loss:?}");
            assert!(loss.refusals.is_empty(), "no later refusal: {loss:?}");
            assert_eq!(bodies.len(), 1);
            assert_eq!(bodies[0].face_records.len(), 1, "one face of the three");
            ends_in(20, "the draw of the damaged cylinder", move || {
                if let Some(imported) = read(ModelFormat::Sat, damaged.as_bytes(), None, "x") {
                    draw(imported.entities);
                }
            });
        }
    }

    /// Damaged copies of each fixture: cut at each line and at each seventh
    /// byte, each line left out, and each number and record pointer
    /// replaced by a value that a damaged file can hold. None can panic
    /// when it is read, and each eighth one that still reads is drawn.
    #[test]
    fn damaged_sat_files_never_panic() {
        const VALUES: [&str; 9] = [
            "0", "-1", "1e308", "-1e308", "1e-308", "99999999", "nan", "$-1", "$99999",
        ];
        ends_in(120, "the read and the draw of the damaged files", || {
            let mut cases = 0;
            let mut drawn = 0;
            let mut value = 0;
            for text in sat_fixtures() {
                let lines: Vec<&str> = text.lines().collect();
                let mut check = |damaged: String| {
                    cases += 1;
                    let imported = read(ModelFormat::Sat, damaged.as_bytes(), None, "x");
                    if let Some(imported) = imported.filter(|_| cases % 8 == 0) {
                        drawn += 1;
                        draw(imported.entities);
                    }
                };
                for n in 0..lines.len() {
                    check(lines[..n].join("\n"));
                    check([&lines[..n], &lines[n + 1..]].concat().join("\n"));
                }
                for cut in (0..text.len()).step_by(7) {
                    check(text[..cut].to_string());
                }
                for (n, line) in lines.iter().enumerate() {
                    let tokens: Vec<&str> = line.split(' ').collect();
                    for k in 0..tokens.len() {
                        if !tokens[k].starts_with('$') && tokens[k].parse::<f64>().is_err() {
                            continue;
                        }
                        for _ in 0..2 {
                            let mut replaced = tokens.clone();
                            replaced[k] = VALUES[value % VALUES.len()];
                            value += 1;
                            let replaced = replaced.join(" ");
                            let mut damaged = lines.clone();
                            damaged[n] = &replaced;
                            check(damaged.join("\n"));
                        }
                    }
                }
            }
            assert!(cases > 7000, "{cases} damaged files");
            assert!(drawn > 500, "{drawn} damaged files drawn");
        });
    }

    /// SAT text of bodies with spline curves and surfaces: two lofts
    /// between circles and an extruded closed spline.
    fn spline_sat_fixtures() -> Vec<String> {
        use solidcore_ops::sweep::Curve;
        let circle = |z: f64, radius: f64, x: f64| Curve::Arc {
            center: [x, 0.0, z],
            axis: [0.0, 0.0, 1.0],
            reference: [1.0, 0.0, 0.0],
            radius,
            start: 0.0,
            end: std::f64::consts::TAU,
        };
        let points = [
            [0.0, 0.0],
            [6.0, -1.0],
            [8.0, 4.0],
            [3.0, 7.0],
            [-2.0, 4.0],
            [0.0, 0.0],
        ]
        .iter()
        .map(|p| [p[0], p[1], 0.0, 1.0])
        .collect();
        let spline = Curve::Nurbs {
            degree: 3,
            points,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0, 3.0],
        };
        [
            ocs_solid::model::loft(
                &[vec![circle(0.0, 5.0, 0.0)], vec![circle(8.0, 3.0, 2.0)]],
                3,
                true,
            ),
            ocs_solid::model::loft(
                &[
                    vec![circle(0.0, 5.0, 0.0)],
                    vec![circle(5.0, 2.0, 1.0)],
                    vec![circle(9.0, 4.0, -1.0)],
                ],
                3,
                true,
            ),
            ocs_solid::model::extrude(&[spline], [0.0, 0.0, 1.0], 6.0, 0.0, true),
        ]
        .into_iter()
        .map(|body| {
            crate::scene::convert::acis_export::solid_to_sat(&body.expect("the body is made"))
                .expect("the body writes")
                .to_sat_string()
        })
        .collect()
    }

    /// Damaged copies of `text`: cut at each line and at each seventh byte,
    /// each line left out, and each number and record pointer replaced by a
    /// value that a damaged file can hold.
    fn damaged_copies(text: &str) -> Vec<String> {
        const VALUES: [&str; 9] = [
            "0", "-1", "1e308", "-1e308", "1e-308", "99999999", "nan", "$-1", "$99999",
        ];
        let lines: Vec<&str> = text.lines().collect();
        let mut damaged = Vec::new();
        for n in 0..lines.len() {
            damaged.push(lines[..n].join("\n"));
            damaged.push([&lines[..n], &lines[n + 1..]].concat().join("\n"));
        }
        for cut in (0..text.len()).step_by(7) {
            damaged.push(text[..cut].to_string());
        }
        let mut value = 0;
        for (n, line) in lines.iter().enumerate() {
            let tokens: Vec<&str> = line.split(' ').collect();
            for k in 0..tokens.len() {
                if !tokens[k].starts_with('$') && tokens[k].parse::<f64>().is_err() {
                    continue;
                }
                for _ in 0..2 {
                    let mut replaced = tokens.clone();
                    replaced[k] = VALUES[value % VALUES.len()];
                    value += 1;
                    let replaced = replaced.join(" ");
                    let mut copy = lines.clone();
                    copy[n] = &replaced;
                    damaged.push(copy.join("\n"));
                }
            }
        }
        damaged
    }

    /// The spline fixtures are read whole: the limits on a spline's degree
    /// and knot counts refuse nothing that the app's own writer makes.
    #[test]
    fn spline_bodies_from_the_writer_are_read_whole() {
        for text in spline_sat_fixtures() {
            assert!(text.contains("spline-surface") && text.contains("intcurve-curve"));
            let parsed = document::entities::acis::SatDocument::parse(&text).expect("SAT text");
            let (bodies, loss) = ocs_solid::lift(&parsed);
            assert_eq!(bodies.len(), 1);
            assert!(loss.is_empty(), "nothing is lost: {loss:?}");
        }
    }

    /// A spline record names how many times each knot repeats, and its
    /// degree. The reader made one knot for each repeat, so a count from a
    /// damaged file became memory without a limit (32 GB for a count of
    /// 4,000,000,000), and a degree of -1 overflowed. The parser now refuses
    /// such a record: the body is read without that curve or surface, and
    /// the loss names it.
    #[test]
    fn a_spline_with_a_count_that_is_not_possible_is_lost_alone() {
        let text = spline_sat_fixtures().swap_remove(0);
        let curve = text
            .lines()
            .find(|line| line.starts_with("intcurve-curve"))
            .expect("the loft has a spline curve");
        let surface = text
            .lines()
            .find(|line| line.starts_with("spline-surface"))
            .expect("the loft has a spline surface");
        // (the record, the token after `nubs` or `nurbs` to replace, the value)
        let cases = [
            ("curve multiplicity", curve, 5, "50000000"),
            ("curve degree", curve, 1, "-1"),
            ("curve knot count", curve, 3, "50000000"),
            ("surface multiplicity", surface, 11, "50000000"),
            ("surface degree", surface, 1, "-1"),
            ("surface knot count", surface, 8, "50000000"),
            // A count that is much larger than the record.
            ("curve knot count, very large", curve, 3, "9000000000000000"),
        ];
        let parsed = document::entities::acis::SatDocument::parse(&text).expect("the loft is SAT");
        let index = |kind: &str| {
            let record = parsed.records.iter().find(|r| r.entity_type == kind);
            record.map(|r| r.index as usize).expect(kind)
        };
        let (curve_record, surface_record) = (index("intcurve-curve"), index("spline-surface"));
        for (name, line, offset, value) in cases {
            let (curves, surfaces) = if line == curve {
                (vec![curve_record], vec![])
            } else {
                (vec![], vec![surface_record])
            };
            let mut tokens: Vec<&str> = line.split(' ').collect();
            let spline = tokens
                .iter()
                .rposition(|token| *token == "nubs" || *token == "nurbs")
                .expect("the record has a spline");
            assert!(
                tokens[spline + offset].parse::<i64>().is_ok(),
                "{name}: the token is a count: {}",
                tokens[spline + offset]
            );
            tokens[spline + offset] = value;
            let damaged = text.replace(line, &tokens.join(" "));
            ends_in(20, "the read and the draw of the damaged loft", move || {
                let parsed = document::entities::acis::SatDocument::parse(&damaged)
                    .expect("the damaged file is still SAT text");
                let (bodies, loss) = ocs_solid::lift(&parsed);
                assert_eq!(bodies.len(), 1, "{name}: the body is read");
                assert_eq!(
                    (&loss.curves, &loss.surfaces, loss.broken.len()),
                    (&curves, &surfaces, 0),
                    "{name}: only the damaged record is lost: {loss:?}"
                );
                if let Some(imported) = read(ModelFormat::Sat, damaged.as_bytes(), None, "x") {
                    draw(imported.entities);
                }
            });
        }
    }

    /// The damaged copies of the spline bodies: each is read, and each
    /// eighth one that still reads is drawn.
    #[test]
    fn damaged_spline_sat_files_never_panic() {
        ends_in(120, "the read and the draw of the damaged files", || {
            let (mut cases, mut drawn) = (0, 0);
            for text in spline_sat_fixtures() {
                for damaged in damaged_copies(&text) {
                    cases += 1;
                    let imported = read(ModelFormat::Sat, damaged.as_bytes(), None, "x");
                    if let Some(imported) = imported.filter(|_| cases % 8 == 0) {
                        drawn += 1;
                        draw(imported.entities);
                    }
                }
            }
            assert!(cases > 4000, "{cases} damaged files");
            assert!(drawn > 300, "{drawn} damaged files drawn");
        });
    }

    /// The steps after the draw, on each sixteenth damaged copy that still
    /// reads: the mass properties, and a save to DXF and to DWG with a
    /// reload and a draw of what comes back. None can panic.
    #[test]
    fn damaged_sat_bodies_survive_mass_properties_and_a_save() {
        use crate::entities::traits::EntityTypeOps;
        ends_in(120, "the steps after the draw of the damaged files", || {
            let (mut cases, mut saved) = (0, 0);
            for text in sat_fixtures().into_iter().chain(spline_sat_fixtures()) {
                for damaged in damaged_copies(&text) {
                    cases += 1;
                    if cases % 16 != 0 {
                        continue;
                    }
                    let Some(imported) = read(ModelFormat::Sat, damaged.as_bytes(), None, "x")
                    else {
                        continue;
                    };
                    let mut scene = crate::scene::Scene::new();
                    for entity in imported.entities {
                        let _ = entity.mass_props();
                        scene.add_entity(entity);
                    }
                    for ext in ["dxf", "dwg"] {
                        let version = scene.document.version;
                        let Ok(bytes) = crate::io::save_to_bytes(&scene.document, ext, version)
                        else {
                            continue;
                        };
                        if let Ok(document) = crate::io::load_bytes(&format!("x.{ext}"), bytes) {
                            saved += 1;
                            draw(document.entities().cloned().collect());
                        }
                    }
                }
            }
            assert!(
                saved > 500,
                "{saved} saves of a damaged body were read back"
            );
        });
    }
}
