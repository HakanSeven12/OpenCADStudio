//! OpenCADStudio native constraint graph sync benchmark suite.
//!
//! Every non-appearance edit used to rebuild the whole DWG-native associative
//! constraint graph through `Scene::sync_native_parametric_graph`.
//!
//! Measures the REAL per-edit production entry point:
//! `Scene::bump_entities_with_parametric_policy` — the function
//! `bump_entities` (src/app/command_driver paths) calls after a geometry
//! edit. On the baseline each such call ends in
//! `sync_native_parametric_graph`, which detaches and re-materializes the
//! associative graph for every scope (ModelSpace plus each block) on every
//! single edit. On the fixed revision the rebuild is gated on a fingerprint
//! of the constraint model: geometry-only edits skip it entirely, and any
//! model change still syncs on the same bump (plus the usual save/export
//! flushes).
//!
//! Fixture: a 60-line horizontal chain in ModelSpace (59 Coincident +
//! 60 Horizontal) plus a Parallel + a DistanceY driven by the named
//! parameter `gap`, and two block scopes each with one line and a DistanceX
//! driven by a literal — three scopes, one global network rebuild per sync.
//! Each iteration nudges one endpoint (rotating through chain ends, chain
//! middle, the gap line, and both block lines) and times the production
//! bump.
//!
//! Equivalence sink: after the measured edits, both revisions run the same
//! explicit `scene.sync_native_parametric_graph()` flush, then the bench
//! serializes the constraint-network object graph as a HANDLE-CANONICAL
//! digest:
//! 1. closure — from the named-objects `ACAD_ASSOCNETWORK` dictionary and
//!    every extension dictionary owned by a block record, follow every
//!    `Handle(N)` in each object's `Debug` text that resolves back into
//!    `document.objects` (the network tree: dictionaries, networks, groups,
//!    dependencies, variables), stopping at the named-objects root;
//! 2. canonical ids — sort the closure by a content key that starts from the
//!    object's `Debug` text with closure handles masked (references outside
//!    the closure — entities, block records — keep their stable raw values)
//!    and is refined three times by folding in the sorted keys of the
//!    objects each one references, so ids follow content and neighborhood,
//!    not allocation order, across runs;
//! 3. emit `#<canonical-id> <Debug text with closure handles rewritten to
//!    #<id>>`; entity handles stay raw (allocated at fixture build, stable
//!    across both runs).
//!
//! Output contract: lines prefixed `TIME ` (timings) and the single
//! `PRE-FLUSH ` line (which by design shows the fixed revision deferring the
//! rebuild) differ between baseline and fixed; every other line — above all
//! the block between `---GRAPH-DIGEST-START---` and
//! `---GRAPH-DIGEST-END---` plus `GRAPH-DIGEST-HASH` — must be identical.
//!
//! Deterministic: fixed geometry, fixed delta sequence, no randomness.
//! Run: `cargo bench --bench native_constraint_graph_sync`

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::time::Instant;

use document::entities::{EntityType, Line};
use document::objects::ObjectType;
use document::tables::BlockRecord;
use document::types::{Handle, Vector3};
use OpenCADStudio::scene::named_parameters::DrivingValue;
use OpenCADStudio::scene::parametric_constraints::{
    ConstraintKind, ParametricRef, ParametricScope,
};
use OpenCADStudio::scene::{ChangeKind, Scene};

const RUNS: usize = 5;
const ITERATIONS: usize = 30;
const WARMUP: usize = 3;
/// Horizontal segments of the ModelSpace chain.
const CHAIN_LINES: usize = 60;
const SEGMENT: f64 = 10.0;
/// Refinement rounds folded into the canonical sort key (round 0 = the
/// object's own masked text).
const KEY_ROUNDS: usize = 3;
/// Named-objects dictionary key an associative network is filed under
/// (`NETWORK_DICTIONARY_KEY` in `scene::dwg_native_constraints`).
const NETWORK_DICTIONARY_KEY: &str = "ACAD_ASSOCNETWORK";
const DIGEST_START: &str = "---GRAPH-DIGEST-START---";
const DIGEST_END: &str = "---GRAPH-DIGEST-END---";

fn line(scene: &mut Scene, start: (f64, f64), end: (f64, f64)) -> Handle {
    scene.add_entity(EntityType::Line(Line::from_points(
        Vector3::new(start.0, start.1, 0.0),
        Vector3::new(end.0, end.1, 0.0),
    )))
}

/// ModelSpace: a `CHAIN_LINES` chain (Coincident-joined, Horizontal) plus a
/// Parallel + DistanceY pair driven by the named parameter `gap`. Two block
/// scopes: one DistanceX each. Returns the scene and the handles the edit
/// loop nudges.
fn build_fixture() -> (Scene, Vec<Handle>) {
    let mut scene = Scene::new();
    let mut chain = Vec::with_capacity(CHAIN_LINES);
    for i in 0..CHAIN_LINES {
        chain.push(line(
            &mut scene,
            (i as f64 * SEGMENT, 0.0),
            ((i + 1) as f64 * SEGMENT, 0.0),
        ));
    }
    let gap_line = line(&mut scene, (0.0, 5.0), (10.0, 5.0));
    scene.named_parameters_mut().set("gap", "5").unwrap();

    let set = scene.parametric_constraint_set_mut(ParametricScope::ModelSpace);
    for pair in chain.windows(2) {
        set.add(
            ConstraintKind::Coincident,
            vec![
                ParametricRef::point(pair[0], 1),
                ParametricRef::point(pair[1], 0),
            ],
            None,
        );
    }
    for &handle in &chain {
        set.add(
            ConstraintKind::Horizontal,
            vec![ParametricRef::whole(handle)],
            None,
        );
    }
    set.add(
        ConstraintKind::Parallel,
        vec![ParametricRef::whole(chain[0]), ParametricRef::whole(gap_line)],
        None,
    );
    set.add(
        ConstraintKind::DistanceY,
        vec![
            ParametricRef::point(chain[0], 0),
            ParametricRef::point(gap_line, 0),
        ],
        Some(DrivingValue::Named("gap".to_string())),
    );

    let mut targets = vec![chain[0], chain[CHAIN_LINES / 2], chain[CHAIN_LINES - 1], gap_line];
    for (index, length) in [7.0f64, 13.0].into_iter().enumerate() {
        let mut block = BlockRecord::new(format!("scope_block_{index}"));
        block.handle = scene.document.allocate_handle();
        let block_handle = block.handle;
        scene.document.block_records.add(block).unwrap();

        let mut block_line = Line::from_points(
            Vector3::new(0.0, index as f64 * 2.0, 0.0),
            Vector3::new(length, index as f64 * 2.0, 0.0),
        );
        block_line.common.owner_handle = block_handle;
        let line_handle = scene
            .document
            .add_entity(EntityType::Line(block_line))
            .unwrap();
        let set = scene.parametric_constraint_set_mut(ParametricScope::Block(block_handle));
        set.add(
            ConstraintKind::DistanceX,
            vec![
                ParametricRef::point(line_handle, 0),
                ParametricRef::point(line_handle, 1),
            ],
            Some(DrivingValue::Literal(length)),
        );
        targets.push(line_handle);
    }
    (scene, targets)
}

/// One production geometry edit plus the timed per-edit bump. Returns the
/// timed `bump_entities_with_parametric_policy` duration in milliseconds.
fn edit(scene: &mut Scene, targets: &[Handle], counter: &mut u64) -> f64 {
    let slot = (*counter as usize) % targets.len();
    *counter += 1;
    let step = ((*counter % 7) as f64 + 1.0) * 0.01;
    let handle = targets[slot];
    if let Some(EntityType::Line(line)) = scene.document.get_entity_mut(handle) {
        match *counter % 4 {
            0 => line.start.y += step,
            1 => line.end.y -= step,
            2 => line.start.x += step,
            _ => line.end.x -= step,
        }
    }
    let changes = [(handle, ChangeKind::Modified)];
    let start = Instant::now();
    scene.bump_entities_with_parametric_policy(black_box(&changes), black_box(&[]), false);
    start.elapsed().as_secs_f64() * 1000.0
}

/// Every `Handle(N)` value mentioned by a `Debug` rendering.
fn handle_values(text: &str) -> Vec<u64> {
    let bytes = text.as_bytes();
    let mut values = Vec::new();
    let mut search = 0;
    while let Some(rel) = text[search..].find("Handle(") {
        let found = search + rel;
        let digits = found + "Handle(".len();
        let mut end = digits;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if end > digits && end < bytes.len() && bytes[end] == b')' {
            if let Ok(value) = text[digits..end].parse::<u64>() {
                values.push(value);
            }
            search = end + 1;
        } else {
            search = digits;
        }
    }
    values
}

/// Rewrites every `Handle(N)` token in `text` through `replace`; unmapped
/// tokens stay verbatim.
fn map_handle_text(text: &str, replace: &dyn Fn(u64) -> Option<String>) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut search = 0;
    while let Some(rel) = text[search..].find("Handle(") {
        let found = search + rel;
        let digits = found + "Handle(".len();
        let mut end = digits;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        out.push_str(&text[search..found]);
        if end > digits && end < bytes.len() && bytes[end] == b')' {
            let mapped = text[digits..end].parse::<u64>().ok().and_then(&replace);
            match mapped {
                Some(replacement) => out.push_str(&replacement),
                None => out.push_str(&text[found..=end]),
            }
            search = end + 1;
        } else {
            out.push_str(&text[found..digits]);
            search = digits;
        }
    }
    out.push_str(&text[search..]);
    out
}

/// Handles of every object in the stored constraint-network tree: roots are
/// the named-objects `ACAD_ASSOCNETWORK` dictionary plus every extension
/// dictionary a block record owns; edges follow handle mentions between
/// objects, stopping at the named-objects root itself.
fn graph_closure(scene: &Scene) -> Vec<Handle> {
    let document = &scene.document;
    let named_objects = document.header.named_objects_dict_handle;
    let mut stack: Vec<Handle> = Vec::new();
    if let Some(ObjectType::Dictionary(dictionary)) = document.objects.get(&named_objects) {
        if let Some(handle) = dictionary.get(NETWORK_DICTIONARY_KEY) {
            stack.push(handle);
        }
    }
    let mut owners: HashSet<u64> = document
        .block_records
        .iter()
        .map(|record| record.handle.value())
        .collect();
    owners.insert(document.header.model_space_block_handle.value());
    for (handle, object) in &document.objects {
        if let ObjectType::Dictionary(dictionary) = object {
            if owners.contains(&dictionary.owner.value()) {
                stack.push(*handle);
            }
        }
    }

    let mut seen: HashSet<Handle> = HashSet::new();
    while let Some(handle) = stack.pop() {
        if !seen.insert(handle) || handle == named_objects {
            continue;
        }
        let Some(object) = document.objects.get(&handle) else {
            continue;
        };
        let text = format!("{:?}", (handle, object));
        for value in handle_values(&text) {
            let referenced = Handle::new(value);
            if referenced != named_objects && document.objects.contains_key(&referenced) {
                stack.push(referenced);
            }
        }
    }
    let mut closure: Vec<Handle> = seen.into_iter().collect();
    closure.sort_unstable();
    closure
}

fn fnv1a(text: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The equivalence sink: the constraint-network graph, canonicalized so the
/// same graph prints identically regardless of which raw handles each
/// revision happened to allocate.
fn graph_digest(scene: &Scene) -> String {
    let document = &scene.document;
    let closure = graph_closure(scene);
    let closure_values: HashSet<u64> = closure.iter().map(|handle| handle.value()).collect();

    // Raw Debug text per closure object, in closure (raw-handle) order.
    let raws: Vec<String> = closure
        .iter()
        .map(|&handle| {
            let object = document
                .objects
                .get(&handle)
                .expect("closure handle must resolve to an object");
            format!("{:?}", (handle, object))
        })
        .collect();
    // References to other closure objects, as positions into this list.
    let positions: HashMap<u64, usize> = closure
        .iter()
        .enumerate()
        .map(|(index, handle)| (handle.value(), index))
        .collect();
    let neighbors: Vec<Vec<usize>> = raws
        .iter()
        .map(|raw| {
            let mut own: Vec<usize> = handle_values(raw)
                .into_iter()
                .filter_map(|value| positions.get(&value).copied())
                .collect();
            own.sort_unstable();
            own.dedup();
            own
        })
        .collect();

    // Canonical sort key: own masked text (closure handles masked, external
    // references keep their stable raw values), refined by folding in the
    // sorted keys of the referenced closure objects.
    let mask = |text: &str| {
        map_handle_text(text, &|value| {
            closure_values
                .contains(&value)
                .then(|| "H".to_string())
        })
    };
    let mut keys: Vec<String> = raws.iter().map(|raw| mask(raw)).collect();
    for _ in 0..KEY_ROUNDS {
        let previous = keys;
        keys = neighbors
            .iter()
            .enumerate()
            .map(|(index, own)| {
                let mut linked: Vec<&str> =
                    own.iter().map(|&j| previous[j].as_str()).collect();
                linked.sort_unstable();
                format!("{}|{}", previous[index], linked.join(","))
            })
            .collect();
    }

    let mut order: Vec<usize> = (0..closure.len()).collect();
    order.sort_by(|&left, &right| {
        keys[left]
            .cmp(&keys[right])
            .then_with(|| closure[left].value().cmp(&closure[right].value()))
    });
    let ties = order
        .windows(2)
        .filter(|pair| keys[pair[0]] == keys[pair[1]])
        .count();

    let canonical: HashMap<u64, usize> = order
        .iter()
        .enumerate()
        .map(|(rank, &index)| (closure[index].value(), rank))
        .collect();
    let mut text = String::new();
    text.push_str(&format!("TOTAL-OBJECTS {}\n", document.objects.len()));
    text.push_str(&format!("CLOSURE {}\n", closure.len()));
    text.push_str(&format!("KEY-TIES {ties}\n"));
    for (rank, &index) in order.iter().enumerate() {
        let line = map_handle_text(&raws[index], &|value| {
            canonical.get(&value).map(|id| format!("#{id}"))
        });
        text.push_str(&format!("#{rank} {line}\n"));
    }
    text
}

fn main() {
    let (mut scene, targets) = build_fixture();
    let model_constraints = scene
        .parametric_constraint_set(ParametricScope::ModelSpace)
        .map(|set| set.constraints.len())
        .unwrap_or(0);
    println!(
        "native graph sync per edit: ModelSpace {model_constraints} constraints ({CHAIN_LINES}-line chain + gap pair) + 2 block scopes, named parameter `gap`"
    );
    println!(
        "  measured: Scene::bump_entities_with_parametric_policy (per-edit point; the baseline's tail runs sync_native_parametric_graph for all scopes; {ITERATIONS} edits/sample, {RUNS} samples)"
    );

    let mut counter = 0u64;
    let mut sink = 0u64;
    for _ in 0..WARMUP {
        let ms = edit(&mut scene, &targets, &mut counter);
        sink = sink.wrapping_add(ms.to_bits());
    }
    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let mut acc_ms = 0.0;
        for _ in 0..ITERATIONS {
            let ms = edit(&mut scene, &targets, &mut counter);
            sink = sink.wrapping_add(ms.to_bits());
            acc_ms += ms;
        }
        samples.push(acc_ms / ITERATIONS as f64);
    }
    samples.sort_by(f64::total_cmp);
    let median = samples[RUNS / 2];
    println!(
        "TIME bump_entities_with_parametric_policy median {median:.4} ms/edit  samples {samples:?}  sink {sink}"
    );
    println!(
        "  one call = one geometry edit's full production bump (solve + refresh + graph sync on the baseline)"
    );

    println!(
        "PRE-FLUSH objects={} closure={}",
        scene.document.objects.len(),
        graph_closure(&scene).len()
    );

    // The flush point every save/export path already drives: with the fix,
    // this is the ONLY place the graph is rebuilt during the run.
    scene.sync_native_parametric_graph();

    let digest = graph_digest(&scene);
    println!("{DIGEST_START}");
    print!("{digest}");
    println!("{DIGEST_END}");
    println!("GRAPH-DIGEST-HASH {:016x}", fnv1a(&digest));
}
