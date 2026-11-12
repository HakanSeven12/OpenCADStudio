//! Production text-shaping and tessellation benchmark. See benches/README.md.
use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use serde_json::json;
use sha2::{Digest, Sha256};
use OpenCADStudio::scene::text::{font_face::Face, lff, sysfont, ttf_glyph};

const TEXTS: usize = 256;
const DRAWING_TEXTS: usize = 10_000;
const UNIQUE_TEXTS: usize = 3_758;
type Geometry = (Vec<Vec<[f32; 2]>>, Vec<[f32; 2]>);

pub(crate) struct FixtureText {
    text: String,
    origin: [f32; 2],
    rotation: f32,
}

// Reproduce the source drawing's pattern and repetition histograms without
// storing its text or depending on the drawing at benchmark time.
pub(crate) fn fixture() -> Vec<FixtureText> {
    let mut values = Vec::with_capacity(DRAWING_TEXTS);
    for (value, count) in [("Ab", 2017), ("c1", 1981), ("XYZd", 1980)] {
        values.extend(std::iter::repeat_n(value.to_string(), count));
    }
    // (unique, twice, three times, decimal-part digits, integer start).
    for (unique, twice, three, decimals, start) in [
        (1623, 152, 12, 0, 1000),
        (1182, 40, 2, 2, 10_000),
        (567, 21, 0, 1, 1000),
        (164, 16, 2, 0, 100),
        (128, 1, 0, 2, 1000),
        (57, 3, 0, 1, 100),
        (14, 0, 0, 2, 100),
        (14, 2, 0, 0, 10),
        (3, 0, 0, 0, 1),
        (3, 0, 0, 1, 10),
    ] {
        let divisor = 10_usize.pow(decimals);
        for i in 0..unique {
            let n = start + i;
            let value = if decimals == 0 {
                n.to_string()
            } else {
                format!(
                    "{}/{:0width$}",
                    n / divisor,
                    n % divisor,
                    width = decimals as usize
                )
            };
            let count = if i < three {
                3
            } else if i < three + twice {
                2
            } else {
                1
            };
            values.extend(std::iter::repeat_n(value, count));
        }
    }
    assert_eq!(values.len(), DRAWING_TEXTS);
    assert_eq!(values.iter().collect::<HashSet<_>>().len(), UNIQUE_TEXTS);
    let mut frequencies = HashMap::new();
    let mut lengths = HashMap::new();
    let mut patterns = HashMap::new();
    for value in &values {
        *frequencies.entry(value).or_insert(0_usize) += 1;
        *lengths.entry(value.len()).or_insert(0_usize) += 1;
        let pattern: String = value
            .chars()
            .map(|ch| match ch {
                '0'..='9' => 'D',
                'A'..='Z' => 'U',
                'a'..='z' => 'L',
                _ => 'P',
            })
            .collect();
        *patterns.entry(pattern).or_insert(0_usize) += 1;
    }
    assert_eq!(
        lengths,
        HashMap::from([(1, 3), (2, 4014), (3, 187), (4, 3853), (5, 717), (6, 1226)])
    );
    let mut frequency_histogram = HashMap::new();
    for count in frequencies.values() {
        *frequency_histogram.entry(*count).or_insert(0_usize) += 1;
    }
    assert_eq!(
        frequency_histogram,
        HashMap::from([
            (1, 3504),
            (2, 235),
            (3, 16),
            (1980, 1),
            (1981, 1),
            (2017, 1)
        ])
    );
    assert_eq!(
        patterns,
        HashMap::from([
            ("UL".to_string(), 2017),
            ("LD".to_string(), 1981),
            ("UUUL".to_string(), 1980),
            ("DDDD".to_string(), 1799),
            ("DDDPDD".to_string(), 1226),
            ("DDDPD".to_string(), 588),
            ("DDD".to_string(), 184),
            ("DDPDD".to_string(), 129),
            ("DDPD".to_string(), 60),
            ("DD".to_string(), 16),
            ("DPDD".to_string(), 14),
            ("D".to_string(), 3),
            ("DPD".to_string(), 3),
        ])
    );
    // Fixed integer PRNG and shuffle keep the fixture identical across hosts.
    let mut state = 0x7c4a_91d3_u32;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state
    };
    for i in (1..values.len()).rev() {
        let j = next() as usize % (i + 1);
        values.swap(i, j);
    }
    values
        .into_iter()
        .map(|text| {
            let x = 6_577_000.0 + (next() % 1_191_938) as f64 / 1000.0;
            let y = 5_587_000.0 + (next() % 1_191_967) as f64 / 1000.0;
            let degrees = (2 + next() % 35_987) as f32 / 100.0;
            FixtureText {
                text,
                origin: [x as f32, y as f32],
                rotation: degrees.to_radians(),
            }
        })
        .collect()
}

/// Distinct longer labels exercise glyphs at many different pen positions.
pub(crate) fn varied_fixture() -> Vec<FixtureText> {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut state = 0xa237_41d5_u32;
    let mut values = Vec::with_capacity(256);
    for i in 0..256 {
        let mut text = String::with_capacity(24 + i % 17);
        for _ in 0..(24 + i % 17) {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            text.push(ALPHABET[state as usize % ALPHABET.len()] as char);
        }
        values.push(FixtureText {
            text,
            origin: [0.0, 0.0],
            rotation: 0.23,
        });
    }
    assert_eq!(
        values
            .iter()
            .map(|item| &item.text)
            .collect::<HashSet<_>>()
            .len(),
        256
    );
    values
}

pub(crate) fn select_fixture(name: &str) -> Vec<FixtureText> {
    match name {
        "drawing" => fixture(),
        "varied" => varied_fixture(),
        _ => panic!("--fixture must be drawing or varied"),
    }
}

fn shape(font: &str, texts: &[String]) -> Vec<Arc<ttf_glyph::ShapedRun>> {
    texts
        .iter()
        .map(|s| ttf_glyph::shape_run(font, black_box(s)).expect("font must shape each text"))
        .collect()
}

fn tessellate(font: &str, texts: &[String]) -> Vec<Geometry> {
    texts
        .iter()
        .map(|s| lff::tessellate_text_ex([0.0, 0.0], 1.0, 0.23, 0.87, 0.0, font, black_box(s)))
        .collect()
}

pub(crate) fn tessellate_drawing(font: &str, texts: &[FixtureText]) -> Vec<Geometry> {
    texts
        .iter()
        .map(|item| {
            lff::tessellate_text_ex(
                item.origin,
                1.0,
                item.rotation,
                0.87,
                0.0,
                font,
                black_box(&item.text),
            )
        })
        .collect()
}

fn add_usize(h: &mut Sha256, n: usize) {
    h.update((n as u64).to_le_bytes());
}
fn points(h: &mut Sha256, p: &[[f32; 2]]) {
    add_usize(h, p.len());
    for v in p {
        for x in v {
            h.update(x.to_bits().to_le_bytes());
        }
    }
}
fn shape_digest(runs: &[Arc<ttf_glyph::ShapedRun>]) -> String {
    let mut h = Sha256::new();
    for run in runs {
        h.update(run.advance.to_bits().to_le_bytes());
        add_usize(&mut h, run.cells.len());
        for &(a, b, x, y) in &run.cells {
            add_usize(&mut h, a);
            add_usize(&mut h, b);
            h.update(x.to_bits().to_le_bytes());
            h.update(y.to_bits().to_le_bytes());
        }
        add_usize(&mut h, run.glyphs.len());
        for g in &run.glyphs {
            add_usize(&mut h, g.strokes.len());
            for p in &g.strokes {
                points(&mut h, p);
            }
            points(&mut h, &g.fill_tris);
        }
    }
    format!("{:x}", h.finalize())
}
pub(crate) fn geometry_digest(geometry: &[Geometry]) -> String {
    let mut h = Sha256::new();
    for (strokes, fill) in geometry {
        add_usize(&mut h, strokes.len());
        for p in strokes {
            points(&mut h, p);
        }
        points(&mut h, fill);
    }
    format!("{:x}", h.finalize())
}
pub(crate) fn fixture_digest(texts: &[FixtureText]) -> String {
    let mut h = Sha256::new();
    for text in texts {
        add_usize(&mut h, text.text.len());
        h.update(text.text.as_bytes());
        for x in text.origin {
            h.update(x.to_bits().to_le_bytes());
        }
        h.update(text.rotation.to_bits().to_le_bytes());
    }
    format!("{:x}", h.finalize())
}
fn elapsed<T>(f: impl FnOnce() -> T) -> (f64, T) {
    let start = Instant::now();
    let value = black_box(f());
    (start.elapsed().as_secs_f64() * 1000.0, value)
}
fn main() {
    let mut font = None;
    let mut fixture_name = String::from("drawing");
    let mut output: Option<PathBuf> = None;
    let mut samples = 7_usize;
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--font") => {
                font = Some(
                    args.next()
                        .expect("--font requires a family")
                        .into_string()
                        .expect("UTF-8 font"),
                )
            }
            Some("--fixture") => fixture_name = args.next().expect("--fixture requires a name").into_string().expect("UTF-8 fixture name"),
            Some("--output") => {
                output = Some(args.next().expect("--output requires a path").into())
            }
            Some("--samples") => {
                samples = args
                    .next()
                    .expect("--samples requires a count")
                    .to_string_lossy()
                    .parse()
                    .expect("valid sample count")
            }
            _ => panic!(
                "usage: text_shaping_geometry [--font FAMILY] [--fixture drawing|varied] [--samples N] [--output NEW_FILE]"
            ),
        }
    }
    assert!(samples > 0, "samples must be nonzero");
    let families = sysfont::families();
    let font = match font {
        Some(name) => families
            .into_iter()
            .find(|f| f.eq_ignore_ascii_case(&name))
            .expect("requested font family is not installed"),
        None => match families
            .into_iter()
            .find(|f| matches!(Face::resolve(f), Face::Ttf { .. }))
        {
            Some(f) => f,
            None => {
                eprintln!("SKIP: no installed TrueType font family");
                return;
            }
        },
    };
    assert!(
        matches!(Face::resolve(&font), Face::Ttf { .. }),
        "font must resolve to the TrueType path"
    );
    let (font_hash, face_index) = sysfont::with_face_data(&font, |bytes, index| {
        (format!("{:x}", Sha256::digest(bytes)), index)
    })
    .expect("font data must be available");
    let drawing = select_fixture(&fixture_name);
    let micro_text_count = if fixture_name == "varied" { 16 } else { TEXTS };
    let mut seen = HashSet::new();
    let texts: Vec<String> = drawing
        .iter()
        .filter(|item| seen.insert(&item.text))
        .take(micro_text_count)
        .map(|item| item.text.clone())
        .collect();
    assert_eq!(texts.len(), micro_text_count);
    // Initialize font discovery and shaping outside the measured samples.
    assert!(ttf_glyph::shape_run(&font, "warmup").is_some());
    let mut cold = Vec::new();
    let mut hot = Vec::new();
    let mut geometry = Vec::new();
    let mut drawing_times = Vec::new();
    let mut expected_shape = None;
    let mut expected_geometry = None;
    let mut glyph_count = 0;
    let mut geometry_points = 0;
    let mut drawing_points = 0;
    let mut expected_drawing = None;
    for _ in 0..samples {
        ttf_glyph::clear_fallback_cache();
        let (ms, shaped) = elapsed(|| shape(&font, &texts));
        cold.push(ms);
        glyph_count = shaped.iter().map(|run| run.glyphs.len()).sum();
        assert!(glyph_count > 0, "fixture must produce visible glyphs");
        let hash = shape_digest(&shaped);
        if let Some(expected) = &expected_shape {
            assert_eq!(&hash, expected, "cold shape changed");
        } else {
            expected_shape = Some(hash.clone());
        }
        let (ms, cached) = elapsed(|| shape(&font, &texts));
        hot.push(ms);
        assert!(
            shaped.iter().zip(&cached).all(|(a, b)| Arc::ptr_eq(a, b)),
            "warm shape workload did not remain in cache"
        );
        assert_eq!(shape_digest(&cached), hash, "warm shape changed");
        drop((shaped, cached));
        let expected = geometry_digest(&tessellate(&font, &texts));
        let (ms, out) = elapsed(|| tessellate(&font, &texts));
        geometry.push(ms);
        geometry_points = out
            .iter()
            .map(|(strokes, fill)| strokes.iter().map(Vec::len).sum::<usize>() + fill.len())
            .sum();
        assert!(geometry_points > 0, "fixture must produce geometry");
        let actual = geometry_digest(&out);
        assert_eq!(actual, expected, "warm geometry changed");
        if let Some(previous) = &expected_geometry {
            assert_eq!(&actual, previous, "geometry changed between samples");
        } else {
            expected_geometry = Some(actual);
        }
        drop(out);
        ttf_glyph::clear_fallback_cache();
        let (ms, drawing_out) = elapsed(|| tessellate_drawing(&font, &drawing));
        drawing_times.push(ms);
        drawing_points = drawing_out
            .iter()
            .map(|(strokes, fill)| strokes.iter().map(Vec::len).sum::<usize>() + fill.len())
            .sum();
        assert!(drawing_points > 0, "drawing fixture must produce geometry");
        let digest = geometry_digest(&drawing_out);
        if let Some(expected) = &expected_drawing {
            assert_eq!(
                &digest, expected,
                "drawing geometry changed between samples"
            );
        } else {
            expected_drawing = Some(digest);
        }
    }
    let unique_count = drawing
        .iter()
        .map(|item| &item.text)
        .collect::<HashSet<_>>()
        .len();
    let report = json!({"benchmark":"text_shaping_geometry", "schema":2, "revision":option_env!("OCS_GIT_REV"), "os":std::env::consts::OS, "arch":std::env::consts::ARCH, "profile": if cfg!(debug_assertions) { "debug" } else { "release" }, "font":font, "font_sha256":font_hash, "face_index":face_index, "fixture":fixture_name, "fixture_sha256":fixture_digest(&drawing), "drawing_text_count":drawing.len(), "drawing_unique_text_count":unique_count, "micro_text_count":texts.len(), "glyph_count":glyph_count, "geometry_points":geometry_points, "drawing_points":drawing_points, "samples":samples, "cold_shape_ms":cold, "warm_shape_ms":hot, "warm_tessellation_ms":geometry, "drawing_tessellation_ms":drawing_times, "shape_sha256":expected_shape, "geometry_sha256":expected_geometry, "drawing_geometry_sha256":expected_drawing, "scope":"cold shape includes shaping, outlines and triangulation; warm tessellation includes tokenization, cache lookup, transforms and output allocation; drawing tessellation starts with an empty shape cache; hashing excluded"});
    let rendered = serde_json::to_string_pretty(&report).unwrap();
    if let Some(path) = output {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .expect("create new report");
        writeln!(file, "{rendered}").unwrap();
    }
    println!("{rendered}");
}
