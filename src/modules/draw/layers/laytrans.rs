// Layer translation — map this drawing's layers onto a set taken from another
// drawing, then move everything across.
//
// Drawings arrive from outside with someone else's layer names. Translating
// them is three separate jobs that the command, the dialog and the saved
// mappings all share: reading a target set out of another file, deciding which
// source layer becomes which target, and performing the move. Only the third
// touches the drawing, and it is the same move LAYMRG makes for a single pair,
// so both go through `merge_layer`. (#624)

use std::collections::BTreeMap;
use std::path::Path;

use acadrust::tables::Layer;

use crate::scene::Scene;

/// A target layer read out of another drawing, with the properties the
/// translated layer should end up carrying.
#[derive(Clone, Debug)]
pub struct TargetLayer {
    pub name: String,
    pub layer: Layer,
}

/// One source layer and the target it becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub from: String,
    pub to: String,
}

/// What a translation should do beyond moving the objects.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Reset each moved object's own colour and linetype to ByLayer, so the
    /// target layer's properties are what actually shows. Objects that override
    /// their layer would otherwise survive the translation looking unchanged.
    pub force_bylayer: bool,
}

/// What a translation did, for the command line and the log.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Source layer, target layer, objects moved.
    pub translated: Vec<(String, String, usize)>,
    /// Mappings that could not run, with the reason.
    pub skipped: Vec<(String, String)>,
}

impl Report {
    pub fn objects(&self) -> usize {
        self.translated.iter().map(|(_, _, n)| n).sum()
    }

    /// The transaction log AutoCAD writes beside the drawing, as text.
    pub fn to_log(&self) -> String {
        let mut out = String::new();
        for (from, to, moved) in &self.translated {
            out.push_str(&format!("{from} -> {to}  ({moved} object(s))\n"));
        }
        for (layer, reason) in &self.skipped {
            out.push_str(&format!("{layer}: skipped — {reason}\n"));
        }
        out
    }
}

/// The layers another drawing defines, as translation targets.
///
/// Any drawing serves: a standards file, a template, or an already-correct
/// drawing. They are all the same format, so nothing here cares which.
pub fn load_targets(path: &Path) -> Result<Vec<TargetLayer>, String> {
    let document = crate::io::load_file(path)?;
    let mut targets: Vec<TargetLayer> = document
        .layers
        .iter()
        // A layer the source drawing itself borrowed from a reference is not
        // part of the standard it is offering.
        .filter(|layer| !layer.name.contains('|') && !layer.flags.xref_dependent)
        .map(|layer| TargetLayer {
            name: layer.name.clone(),
            layer: layer.clone(),
        })
        .collect();
    targets.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    if targets.is_empty() {
        return Err("no layers found in that file".into());
    }
    Ok(targets)
}

/// Layers of `scene` that a translation could move, current layer excluded —
/// the drawing has to keep drawing somewhere.
pub fn source_layers(scene: &Scene, current: &str) -> Vec<String> {
    let mut names: Vec<String> = scene
        .document
        .layers
        .names()
        .filter(|name| *name != "0" && !name.eq_ignore_ascii_case(current))
        .map(|name| name.to_string())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names
}

/// Pair up every source layer that a target names as well.
///
/// This is the mapping most translations want and all of them start from: the
/// two drawings already agree on those names, so the only thing to settle is
/// the properties.
pub fn map_same(sources: &[String], targets: &[TargetLayer]) -> Vec<Mapping> {
    let by_name: BTreeMap<String, &TargetLayer> = targets
        .iter()
        .map(|target| (target.name.to_lowercase(), target))
        .collect();
    sources
        .iter()
        .filter_map(|source| {
            by_name.get(&source.to_lowercase()).map(|target| Mapping {
                from: source.clone(),
                to: target.name.clone(),
            })
        })
        .collect()
}

/// Move everything on each mapped layer onto its target and drop the layer it
/// left. The target is created, with the properties the target drawing gave it,
/// when this drawing has no layer by that name.
pub fn translate(
    scene: &mut Scene,
    mappings: &[Mapping],
    targets: &[TargetLayer],
    current_layer: &str,
    options: Options,
) -> Report {
    let mut report = Report::default();
    for mapping in mappings {
        if mapping.from.eq_ignore_ascii_case(&mapping.to) {
            continue;
        }
        if mapping.from == "0" {
            report
                .skipped
                .push((mapping.from.clone(), "layer \"0\" cannot be translated".into()));
            continue;
        }
        if mapping.from.eq_ignore_ascii_case(current_layer) {
            report.skipped.push((
                mapping.from.clone(),
                "it is the current layer".into(),
            ));
            continue;
        }
        if !scene.document.layers.contains(&mapping.from) {
            report
                .skipped
                .push((mapping.from.clone(), "no such layer in this drawing".into()));
            continue;
        }
        // Bring the target in with the standard's own properties. An existing
        // layer of that name is left as it is: the drawing already has an
        // opinion about it, and silently restyling it would reach past what the
        // translation was asked to do.
        if !scene.document.layers.contains(&mapping.to) {
            let Some(target) = targets
                .iter()
                .find(|target| target.name.eq_ignore_ascii_case(&mapping.to))
            else {
                report
                    .skipped
                    .push((mapping.from.clone(), "target layer is not in the loaded set".into()));
                continue;
            };
            let mut layer = target.layer.clone();
            layer.name = target.name.clone();
            layer.handle = scene.document.allocate_handle();
            // Whatever reference the target drawing held it through does not
            // come with it.
            layer.flags.xref_dependent = false;
            layer.xref_block_record_handle = acadrust::Handle::NULL;
            let _ = scene.document.layers.add(layer);
        }
        let moved = merge_layer(scene, &mapping.from, &mapping.to, options.force_bylayer);
        report
            .translated
            .push((mapping.from.clone(), mapping.to.clone(), moved));
    }
    if !report.translated.is_empty() {
        let touched: Vec<String> = report
            .translated
            .iter()
            .map(|(_, to, _)| to.clone())
            .collect();
        scene.invalidate_dependency_index();
        scene.invalidate_layer_dependencies(&touched);
    }
    report
}

/// Move every object off `from` onto `to` and drop `from`. Returns how many
/// objects moved.
///
/// The entity list is flat and holds block definitions too, so objects inside
/// blocks travel with the rest rather than being left behind on a layer that no
/// longer exists.
pub fn merge_layer(scene: &mut Scene, from: &str, to: &str, force_bylayer: bool) -> usize {
    let mut moved = 0usize;
    for entity in scene.document.entities_mut() {
        if entity.common().layer != from {
            continue;
        }
        let common = entity.common_mut();
        common.layer = to.to_string();
        if force_bylayer {
            common.color = acadrust::Color::ByLayer;
            common.color_name = None;
            common.color_book_handle = None;
            common.linetype = String::new();
            common.linetype_handle = None;
            common.line_weight = acadrust::LineWeight::ByLayer;
        }
        moved += 1;
    }
    scene.document.layers.remove(from);
    moved
}
