//! STEP reader probe for Solidcore's STEP bake-off (`bench/step/run.py` in
//! github.com/NVIDIA/solidcore, Apache-2.0): reads a file through the app's
//! STEP import and writes what it holds in the bake-off's `eval` form.
//!
//! ```sh
//! cargo build --release --example step_eval
//! bench/step/run.py --out /tmp/bake --reader ocs=target/release/examples/step_eval
//! ```
//!
//! `step_eval eval FILE --json-out OUT` writes `{"bodies": [{"volume_mm3",
//! "faces", "closed"}]}`, one entry per body occurrence: every product
//! counted as often as the assembly places it.

use OpenCADStudio::io::step_import::{read, StepBody, StepModel};

fn body_json(body: &StepBody, mm: f64) -> serde_json::Value {
    let solid = &body.body;
    let closed = solid.edges.iter().all(|(_, edge)| edge.coedges.len() == 2);
    let volume = kernel::brep::mass_properties(solid).map_or(0.0, |m| m.volume);
    serde_json::json!({
        "volume_mm3": volume * mm.powi(3),
        "faces": solid.faces.len(),
        "closed": closed && volume > 0.0,
    })
}

fn occurrences(model: &StepModel, product: usize, mm: f64, depth: usize, out: &mut Vec<serde_json::Value>) {
    let Some(item) = model.products.get(product) else { return };
    if depth > 64 {
        return;
    }
    let mm = mm * item.scale;
    out.extend(item.bodies.iter().map(|body| body_json(body, mm)));
    for (child, _) in &item.children {
        occurrences(model, *child, mm, depth + 1, out);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some("eval"), Some(file)) = (args.get(1).map(String::as_str), args.get(2)) else {
        eprintln!("usage: step_eval eval FILE --json-out OUT");
        std::process::exit(2);
    };
    let out = args.iter().position(|a| a == "--json-out").and_then(|i| args.get(i + 1));
    let bytes = std::fs::read(file).unwrap_or_else(|e| {
        eprintln!("{file}: {e}");
        std::process::exit(1);
    });
    let Some(model) = read(&bytes) else {
        eprintln!("{file}: not a STEP file this reader takes");
        std::process::exit(1);
    };
    let mm = model.unit_mm.unwrap_or(1.0);
    let mut bodies: Vec<_> = model.direct.iter().map(|body| body_json(body, mm)).collect();
    for root in &model.roots {
        occurrences(&model, *root, mm, 0, &mut bodies);
    }
    let text = serde_json::json!({ "bodies": bodies }).to_string();
    match out {
        Some(path) => std::fs::write(path, text).unwrap_or_else(|e| {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }),
        None => println!("{text}"),
    }
}
