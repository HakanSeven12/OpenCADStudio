use super::*;
use super::super::util::*;

pub(crate) fn plot_dialog_scale_factor(d: &crate::ui::window::plot::PlotDialogState) -> f64 {
    d.scales
        .iter()
        .find(|(name, _)| name == &d.scale)
        .map(|(_, factor)| *factor)
        .or_else(|| {
            let (paper, drawing) = parse_plot_scale(&d.scale);
            (paper > 0.0 && drawing > 0.0).then_some(paper / drawing)
        })
        .unwrap_or(1.0)
        .max(1e-9)
}

pub(crate) fn scale_name_for_factor(scales: &[(String, f64)], factor: f64) -> Option<String> {
    // Several names can share a ratio ("1:1" and 1'-0" = 1'-0"); the plain
    // ratio name reads best.
    let matching = || {
        scales
            .iter()
            .filter(|(_, candidate)| (*candidate - factor).abs() <= 1e-6 * factor.abs().max(1.0))
    };
    matching()
        .find(|(name, _)| !name.contains('='))
        .or_else(|| matching().next())
        .map(|(name, _)| name.clone())
}

pub(crate) fn plot_render_mode_override(
    d: &crate::ui::window::plot::PlotDialogState,
) -> Option<document::entities::ViewportRenderMode> {
    use document::entities::ViewportRenderMode as Mode;
    match d.shade.as_str() {
        "2D Wireframe" => Some(Mode::Wireframe2D),
        "3D Wireframe" => Some(Mode::Wireframe3D),
        "Hidden Line" => Some(Mode::HiddenLine),
        "Flat Shaded" => Some(Mode::FlatShaded),
        "Gouraud Shaded" => Some(Mode::GouraudShaded),
        "Flat Shaded + Edges" => Some(Mode::FlatShadedWithEdges),
        "Gouraud Shaded + Edges" => Some(Mode::GouraudShadedWithEdges),
        _ => None,
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(in crate::app) fn native_paths_match(left: &std::path::Path, right: &std::path::Path) -> bool {
    let resolve = |path: &std::path::Path| {
        std::fs::canonicalize(path).ok().or_else(|| {
            // Preserve directory-alias identity after the current drawing
            // disappears. Its opening fingerprint must still guard a save.
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            Some(std::fs::canonicalize(parent).ok()?.join(path.file_name()?))
        })
    };
    match (resolve(left), resolve(right)) {
        (Some(left), Some(right)) => left == right,
        _ => left == right,
    }
}

use crate::io::pdf_export::{PdfPageInput, PlotContent};

/// The dialog's sheet orientation as a catalogue value.
pub(crate) fn plot_dialog_orientation(
    d: &crate::ui::window::plot::PlotDialogState,
) -> crate::io::paper_catalog::Orientation {
    use crate::io::paper_catalog::Orientation;
    if d.orientation == "Portrait" {
        Orientation::Portrait
    } else {
        Orientation::Landscape
    }
}

/// The sheet the dialog names, or — when the name carries no size (an old
/// config, a bare driver name) — a custom sheet built from the dialog's stored
/// dimensions, so a drawing whose paper we cannot identify still plots at its
/// own size.
pub(crate) fn plot_dialog_paper(
    d: &crate::ui::window::plot::PlotDialogState,
) -> crate::io::paper_catalog::PaperSize {
    use crate::io::paper_catalog::{from_drawing, resolve, PaperSize, PaperUnits};
    if d.paper_width_mm > 0.0 && d.paper_height_mm > 0.0 {
        from_drawing(&d.paper, d.paper_width_mm, d.paper_height_mm)
    } else {
        resolve(&d.paper)
            .unwrap_or_else(|| PaperSize::custom(210.0, 297.0, PaperUnits::Millimeters))
    }
}

pub(crate) fn plot_dialog_sheet_mm(d: &crate::ui::window::plot::PlotDialogState) -> (f64, f64) {
    plot_dialog_paper(d).sheet_mm(plot_dialog_orientation(d))
}

/// Whether the dialog still names the sheet the stored settings carry.
pub(crate) fn paper_unchanged(
    d: &crate::ui::window::plot::PlotDialogState,
    stored: &document::objects::PlotSettings,
) -> bool {
    !stored.paper_size.is_empty()
        && crate::io::paper_catalog::from_drawing(
            &stored.paper_size,
            stored.paper_width,
            stored.paper_height,
        )
        .canonical
            == d.paper
}

/// The media name to store for the dialog's sheet. A drawing keeps its own
/// spelling of a sheet the user did not change — a system printer's `A4` or a
/// plotter's custom name must not turn into the catalogue's canonical name
/// just by passing through the dialog, or the source application would look the sheet up
/// under a name that driver never reported. Only a genuinely different
/// selection writes the canonical name.
pub(crate) fn paper_name_for_settings(
    d: &crate::ui::window::plot::PlotDialogState,
    stored: &document::objects::PlotSettings,
) -> String {
    if paper_unchanged(d, stored) {
        stored.paper_size.clone()
    } else {
        d.paper.clone()
    }
}

/// The stored rotation for the dialog's sheet on a medium that is itself
/// `natural`-oriented: a quarter turn when the requested orientation differs
/// from the medium's own, and "upside-down" adds a half turn on top. This is
/// how the commercial application writes it — a system printer's portrait
/// `A3` plotted landscape carries 90°, while its PDF driver's landscape
/// `ISO_A4_(297.00_x_210.00_MM)` medium plotted landscape carries 0°.
pub(crate) fn plot_dialog_rotation(
    d: &crate::ui::window::plot::PlotDialogState,
    natural: crate::io::paper_catalog::Orientation,
) -> document::objects::PlotRotation {
    use document::objects::PlotRotation;
    match (plot_dialog_orientation(d) != natural, d.upside_down) {
        (false, false) => PlotRotation::None,
        (true, false) => PlotRotation::Degrees90,
        (false, true) => PlotRotation::Degrees180,
        (true, true) => PlotRotation::Degrees270,
    }
}

/// Millimetres in the paper unit a page setup counts in.
pub(crate) fn plot_paper_unit_mm(units: document::objects::PlotPaperUnits) -> f64 {
    match units {
        document::objects::PlotPaperUnits::Inches => 25.4,
        _ => 1.0,
    }
}

/// The factor the commercial application stores next to the scale (DXF 147)
/// is the `paper : drawing` ratio times 25.4 on millimetre page setups and
/// the plain ratio on inch ones — observed in its files, whatever the
/// published description says. Both directions of that convention live here.
pub(crate) fn stored_scale_factor(ratio: f64, units: document::objects::PlotPaperUnits) -> f64 {
    match units {
        document::objects::PlotPaperUnits::Millimeters => ratio * 25.4,
        _ => ratio,
    }
}

pub(crate) fn ratio_from_stored_scale_factor(factor: f64, units: document::objects::PlotPaperUnits) -> f64 {
    match units {
        document::objects::PlotPaperUnits::Millimeters => factor / 25.4,
        _ => factor,
    }
}

/// The file format's standard scale for a `paper : drawing` ratio, when it
/// has one. Only the metric ratios exist as standard codes here; an
/// architectural scale is stored as a custom ratio with the same value.
pub(crate) fn standard_scale_for_ratio(paper: f64, drawing: f64) -> Option<document::objects::ScaledType> {
    use document::objects::ScaledType as S;
    const STANDARD: &[(f64, f64, S)] = &[
        (1.0, 1.0, S::OneToOne),
        (1.0, 2.0, S::OneToTwo),
        (1.0, 4.0, S::OneToFour),
        (1.0, 8.0, S::OneToEight),
        (1.0, 10.0, S::OneToTen),
        (1.0, 16.0, S::OneToSixteen),
        (1.0, 20.0, S::OneToTwenty),
        (1.0, 30.0, S::OneToThirty),
        (1.0, 40.0, S::OneToForty),
        (1.0, 50.0, S::OneToFifty),
        (1.0, 100.0, S::OneToHundred),
        (2.0, 1.0, S::TwoToOne),
        (4.0, 1.0, S::FourToOne),
        (8.0, 1.0, S::EightToOne),
        (10.0, 1.0, S::TenToOne),
        (100.0, 1.0, S::HundredToOne),
    ];
    let ratio = paper / drawing;
    STANDARD
        .iter()
        .find(|(p, d, _)| (p / d - ratio).abs() <= 1e-9 * ratio.abs().max(1.0))
        .map(|(_, _, scale)| *scale)
}

/// `12.5000` → `12.5`, `3.0000` → `3`: a fixed-precision number without its
/// trailing zeros, for fields the user reads back.
pub(crate) fn trim_decimals(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        text.to_string()
    }
}

/// A scale-list name for a plot scale the drawing has no name for, spelled as
/// the ratio it is (`1:250`, `25.4:1`) so the value survives in the picker
/// instead of collapsing to 1:1.
pub(crate) fn ratio_scale_name(factor: f64) -> String {
    if factor >= 1.0 {
        format!("{}:1", trim_decimals(&format!("{factor:.4}")))
    } else {
        format!("1:{}", trim_decimals(&format!("{:.4}", 1.0 / factor)))
    }
}

/// The plot device the dialog currently targets.
pub(crate) fn plot_dialog_device(
    d: &crate::ui::window::plot::PlotDialogState,
) -> crate::io::plot_device::PlotDevice {
    use crate::io::plot_device::PlotDevice;
    if d.to_file {
        PlotDevice::Pdf
    } else {
        match &d.printer {
            Some(name) => PlotDevice::Printer(name.clone()),
            None => PlotDevice::None,
        }
    }
}

/// The plotter name to store. Same rule as the sheet: a device name the source application
/// wrote (its PDF driver, a plotter `.pc3` we plot to PDF for it) stays as it
/// is while the dialog still points at the same kind of device; a changed
/// device, or one of our own legacy labels, is written in the source application's spelling.
pub(crate) fn device_name_for_settings(
    d: &crate::ui::window::plot::PlotDialogState,
    stored: &document::objects::PlotSettings,
) -> String {
    use crate::io::plot_device::PlotDevice;
    let device = plot_dialog_device(d);
    let (stored_device, keep_stored) = PlotDevice::from_stored_name(&stored.printer_name);
    if keep_stored && stored_device == device {
        stored.printer_name.clone()
    } else {
        device.canonical_name()
    }
}

/// The unprintable margins to store: the drawing's own while neither the
/// sheet nor the device changed (they came from the source application's driver and we have
/// no better source), otherwise what our device reports for the new sheet —
/// the equivalent of the source application's `PAPERUPDATE`.
pub(crate) fn margins_for_settings(
    d: &crate::ui::window::plot::PlotDialogState,
    stored: &document::objects::PlotSettings,
) -> document::objects::PaperMargin {
    use crate::io::plot_device::PlotDevice;
    let device = plot_dialog_device(d);
    let (stored_device, _) = PlotDevice::from_stored_name(&stored.printer_name);
    if paper_unchanged(d, stored) && stored_device == device {
        return stored.margins;
    }
    let margins = device.margins_mm(&plot_dialog_paper(d), &d.custom_papers);
    document::objects::PaperMargin::new(margins.left, margins.bottom, margins.right, margins.top)
}

pub(crate) fn plot_content_extents(content: &PlotContent) -> Option<(f64, f64, f64, f64)> {
    let PlotContent {
        wires,
        hatches,
        wipeouts,
        images,
        ..
    } = content;
    let mut bounds = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut include = |x: f64, y: f64| {
        if x.is_finite() && y.is_finite() {
            bounds.0 = bounds.0.min(x);
            bounds.1 = bounds.1.min(y);
            bounds.2 = bounds.2.max(x);
            bounds.3 = bounds.3.max(y);
        }
    };
    for wire in wires
        .iter()
        .filter(|wire| wire.name != "paper_printable_area")
    {
        let [x0, y0, x1, y1] = wire.aabb;
        include(x0 as f64, y0 as f64);
        include(x1 as f64, y1 as f64);
    }
    for hatch in wipeouts.iter().chain(hatches.iter()) {
        for &[x, y] in hatch.boundary.iter() {
            include(
                hatch.world_origin[0] + x as f64,
                hatch.world_origin[1] + y as f64,
            );
        }
    }
    for plot in images {
        let mut image_bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for vertex in &plot.image.verts {
            for axis in 0..2 {
                let value = vertex.pos[axis] as f64 + vertex.pos_low[axis] as f64;
                image_bounds[axis] = image_bounds[axis].min(value);
                image_bounds[axis + 2] = image_bounds[axis + 2].max(value);
            }
        }
        for clip in &plot.clips {
            for axis in 0..2 {
                let min = clip.iter().map(|p| p[axis]).fold(f64::INFINITY, f64::min);
                let max = clip
                    .iter()
                    .map(|p| p[axis])
                    .fold(f64::NEG_INFINITY, f64::max);
                image_bounds[axis] = image_bounds[axis].max(min);
                image_bounds[axis + 2] = image_bounds[axis + 2].min(max);
            }
        }
        if image_bounds[0] < image_bounds[2] && image_bounds[1] < image_bounds[3] {
            include(image_bounds[0], image_bounds[1]);
            include(image_bounds[2], image_bounds[3]);
        }
    }
    (bounds.0.is_finite()
        && bounds.1.is_finite()
        && bounds.2.is_finite()
        && bounds.3.is_finite()
        && bounds.2 > bounds.0
        && bounds.3 > bounds.1)
        .then_some(bounds)
}

/// Stamp the owning entity's effective color index onto a plot wire so CTB
/// plot-style lookups (`wire.aci > 0`) apply.
///
/// Direct entities are resolved here from their authored color/layer. Expanded
/// block wires are different: `BlockCache` has already resolved their effective
/// ACI through ByBlock and layer-0/ByLayer inheritance. Their wire name points
/// at the host INSERT rather than the child entity that produced the geometry,
/// so resolving that INSERT again would destroy the child's effective ACI.
///
/// In particular, a true-color INSERT on an indexed-color layer would overwrite
/// an inherited child ACI with 0 and bypass the CTB entirely.
pub(crate) fn plot_owner_aci(scene: &crate::scene::Scene, wire: &mut crate::scene::WireModel) {
    let Some(handle) = crate::scene::Scene::handle_from_wire_name(&wire.name) else {
        return;
    };
    let Some(entity) = scene.document.get_entity(handle) else {
        return;
    };

    // Expanded block geometry already carries the effective ACI calculated by
    // BlockCache. `wire.name` identifies the host INSERT for picking, not the
    // block child whose color inheritance produced `wire.aci`.
    if matches!(entity, document::EntityType::Insert(_)) {
        return;
    }

    let common = entity.common();
    wire.aci = match &common.color {
        document::types::Color::Index(index) => *index,
        document::types::Color::ByLayer => scene
            .document
            .layers
            .get(&common.layer)
            .map(|layer| match &layer.color {
                document::types::Color::Index(index) => *index,
                _ => 0,
            })
            .unwrap_or(7),
        _ => 0,
    };
}


pub(crate) fn plot_scene_content(
    scene: &crate::scene::Scene,
    paper_space_last: bool,
    render_mode_override: Option<document::entities::ViewportRenderMode>,
) -> PlotContent {
    let (mut paper_wires, mut model_wires) = scene.plot_wire_groups(render_mode_override);
    // A viewport border plots whenever its layer plots, as in the reference
    // application: the page setup's plot-viewport-borders flag (off in its
    // layouts) does not hide it.
    paper_wires.retain(|wire| wire.plot_visible);
    model_wires.retain(|wire| wire.plot_visible);
    // A text / leader background mask is filled with the canvas colour so it
    // hides what is behind it; on paper that colour is the sheet's, not the
    // dark screen's. Mark such fills so the export paints them paper-white
    // instead of plotting the canvas colour (or turning it black). (#1072)
    let canvas = scene.current_bg();
    let with_depth = |wires: Vec<crate::scene::WireModel>| {
        let depths = scene.plot_wire_depths(&wires);
        wires
            .into_iter()
            .zip(depths)
            .map(|(mut wire, draw_depth)| {
                if !wire.fill_tris.is_empty() && wire.color[..3] == canvas[..3] {
                    wire.bg_adapt =
                        Some(Box::new(crate::scene::model::wire_model::BgAdaptInputs {
                            raw_color: wire.color,
                            canvas_color: true,
                            ..Default::default()
                        }));
                }
                plot_owner_aci(scene, &mut wire);
                wire.resample_plot_curve();
                crate::io::pdf_export::PlotWire { wire, draw_depth }
            })
            .collect::<Vec<_>>()
    };
    let paper_wires = with_depth(paper_wires);
    let model_wires = with_depth(model_wires);
    let paper_hatches = scene.paper_plot_hatches().as_ref().clone();
    let paper_wipeouts = scene.paper_plot_wipeouts().as_ref().clone();
    let paper_images = scene.paper_plot_images();
    if scene.current_layout == "Model" {
        let splits = crate::io::pdf_export::PlotGroupSplits {
            wires: paper_wires.len(),
            hatches: paper_hatches.len(),
            wipeouts: paper_wipeouts.len(),
            images: paper_images.len(),
        };
        let entity_layers =
            plot_entity_layers(scene, &paper_wires, &paper_hatches, &paper_wipeouts);
        let entity_plot_styles =
            plot_entity_styles(scene, &paper_wires, &paper_hatches, &paper_wipeouts);
        return PlotContent {
            wires: std::sync::Arc::new(paper_wires),
            hatches: paper_hatches,
            wipeouts: paper_wipeouts,
            images: paper_images,
            group_splits: splits,
            entity_layers,
            entity_plot_styles,
        };
    }
    let (mut model_pattern_wires, model_hatches, model_wipeouts, model_images) =
        scene.viewport_plot_fills();
    model_pattern_wires.retain(|(wire, _)| wire.plot_visible);
    let model_pattern_wires = model_pattern_wires
        .into_iter()
        .map(|(mut wire, draw_depth)| {
            plot_owner_aci(scene, &mut wire);
            crate::io::pdf_export::PlotWire { wire, draw_depth }
        })
        .collect::<Vec<_>>();

    let (wires, hatches, wipeouts, images, splits) = if paper_space_last {
        let splits = crate::io::pdf_export::PlotGroupSplits {
            wires: model_wires.len() + model_pattern_wires.len(),
            hatches: model_hatches.len(),
            wipeouts: model_wipeouts.len(),
            images: model_images.len(),
        };
        let mut wires = model_wires;
        let mut hatches = model_hatches;
        let mut wipeouts = model_wipeouts;
        let mut images = model_images;
        wires.extend(model_pattern_wires);
        wires.extend(paper_wires);
        hatches.extend(paper_hatches);
        wipeouts.extend(paper_wipeouts);
        images.extend(paper_images);
        (wires, hatches, wipeouts, images, splits)
    } else {
        let splits = crate::io::pdf_export::PlotGroupSplits {
            wires: paper_wires.len(),
            hatches: paper_hatches.len(),
            wipeouts: paper_wipeouts.len(),
            images: paper_images.len(),
        };
        let mut wires = paper_wires;
        let mut hatches = paper_hatches;
        let mut wipeouts = paper_wipeouts;
        let mut images = paper_images;
        wires.extend(model_wires);
        wires.extend(model_pattern_wires);
        hatches.extend(model_hatches);
        wipeouts.extend(model_wipeouts);
        images.extend(model_images);
        (wires, hatches, wipeouts, images, splits)
    };
    let entity_layers = plot_entity_layers(scene, &wires, &hatches, &wipeouts);
    let entity_plot_styles = plot_entity_styles(scene, &wires, &hatches, &wipeouts);
    PlotContent {
        wires: std::sync::Arc::new(wires),
        hatches,
        wipeouts,
        images,
        group_splits: splits,
        entity_layers,
        entity_plot_styles,
    }
}

/// Resolve the object style and ByLayer or ByBlock inheritance for the
/// plotted occurrence. Block wires keep the member and the outer INSERT.
pub(crate) fn plot_entity_styles(
    scene: &crate::scene::Scene,
    wires: &[crate::io::pdf_export::PlotWire],
    hatches: &[crate::scene::model::hatch_model::HatchModel],
    wipeouts: &[crate::scene::model::hatch_model::HatchModel],
) -> std::collections::HashMap<(document::Handle, Option<document::Handle>), String> {
    let doc = &scene.document;
    if doc.header.plotstyle_mode {
        return Default::default();
    }
    let objects = wires
        .iter()
        .filter_map(|wire| {
            crate::scene::Scene::handle_from_wire_name(&wire.name)
                .map(|handle| (handle, wire.block_member, wire.effective_layer.as_deref()))
        })
        .chain(
            hatches
                .iter()
                .chain(wipeouts)
                .filter_map(|hatch| Some((hatch.entity?, None, hatch.effective_layer.as_deref()))),
        );
    let mut styles = std::collections::HashMap::new();
    for (handle, member, effective_layer) in objects {
        if styles.contains_key(&(handle, member)) {
            continue;
        }
        let Some(owner) = doc.get_entity(handle) else {
            continue;
        };
        let owner = owner.common();
        let child = member.and_then(|member| doc.get_entity(member));
        let style = match child.map(|child| child.common()) {
            Some(child) if child.plotstyle_flags != 1 => {
                plot_occurrence_style(doc, child, effective_layer)
            }
            _ => plot_occurrence_style(doc, owner, None),
        };
        styles.insert((handle, member), style);
    }
    styles
}

pub(crate) fn plot_occurrence_style(
    doc: &document::Document,
    common: &document::entities::EntityCommon,
    effective_layer: Option<&str>,
) -> String {
    let style_name = crate::app::properties::plot_style_name;
    match common.plotstyle_flags {
        0 => doc
            .layers
            .get(effective_layer.unwrap_or(&common.layer))
            .map(|layer| layer.plotstyle_handle)
            .filter(|h| !h.is_null())
            .map(|handle| style_name(doc, 3, Some(handle)))
            .filter(|name| name != "ByLayer")
            .unwrap_or_else(|| "Normal".to_string()),
        1 => "Normal".to_string(),
        flags => style_name(doc, flags, common.plotstyle_handle),
    }
}

/// Legacy handle-based layer fallback for direct and synthetic plot items.
/// Expanded block items carry their placement-specific `effective_layer`;
/// this map cannot represent one definition inserted on several layers.
pub(crate) fn plot_entity_layers(
    scene: &crate::scene::Scene,
    wires: &[crate::io::pdf_export::PlotWire],
    hatches: &[crate::scene::model::hatch_model::HatchModel],
    wipeouts: &[crate::scene::model::hatch_model::HatchModel],
) -> std::collections::HashMap<document::Handle, String> {
    let names = wires.iter().map(|wire| wire.name.as_str());
    let mut layers = std::collections::HashMap::new();
    for name in names {
        let Some(handle) = crate::scene::Scene::handle_from_wire_name(name) else {
            continue;
        };
        if layers.contains_key(&handle) {
            continue;
        }
        if let Some(entity) = scene.document.get_entity(handle) {
            layers.insert(handle, entity.common().layer.clone());
        }
    }
    // Hatch models carry the pattern name (not a handle) in `name`; their
    // drawing layer comes from the entity the model plots for.
    for hatch in hatches.iter().chain(wipeouts) {
        let Some(handle) = hatch.entity else {
            continue;
        };
        if layers.contains_key(&handle) {
            continue;
        }
        if let Some(entity) = scene.document.get_entity(handle) {
            layers.insert(handle, entity.common().layer.clone());
        }
    }
    layers
}


impl OpenCADStudio {
    /// The plot scale as `paper : drawing` in the dialog's page-setup unit,
    /// with the standard code it names when it names one. The dialog's
    /// factor is per the drawing's scale-family unit (inches for an imperial
    /// drawing, else millimetres) and already includes the drawing-unit
    /// factor; the page setup counts per its own unit. A named ratio is kept
    /// as written when no drawing-unit factor hides inside it — a metre
    /// drawing plotting "1:100" really plots 10 mm per unit, which is what
    /// the file must say — and its standard code is kept even across a
    /// paper-unit conversion, the way the commercial application writes
    /// "1:1" on an inch page setup over a millimetre paper space as
    /// `1 in = 25.4 units`.
    pub(in crate::app) fn dialog_plot_scale_ratio(&self) -> ((f64, f64), Option<document::objects::ScaledType>) {
        let d = &self.plot_dialog;
        let scene = &self.tabs[self.active_tab].scene;
        let factor = plot_dialog_scale_factor(d);
        let conversion = scene.scale_family_unit_mm() / d.paper_units.to_mm(1.0);
        let named = scene.scale_ratio(&d.scale).filter(|(paper, drawing)| {
            (paper / drawing - factor).abs() <= 1e-9 * factor.abs().max(1.0)
        });
        let standard = named.and_then(|(paper, drawing)| standard_scale_for_ratio(paper, drawing));
        let ratio = match named {
            Some((paper, drawing)) if (conversion - 1.0).abs() <= 1e-12 => (paper, drawing),
            _ => {
                let per_unit = (factor * conversion).max(1e-9);
                if per_unit >= 1.0 {
                    (per_unit, 1.0)
                } else {
                    (1.0, 1.0 / per_unit)
                }
            }
        };
        (ratio, standard)
    }

    /// `PAPERUPDATE`: when the printer that just answered does not list the
    /// dialog's sheet, either switch to the printer's default sheet (1) or
    /// keep the sheet and say so (0).
    pub(in crate::app) fn reconcile_sheet_with_printer(&mut self, printer: &str) {
        let d = &self.plot_dialog;
        let Some(caps) = d.printer_media.as_ref() else {
            return;
        };
        if caps.media.is_empty() {
            return;
        }
        let paper = plot_dialog_paper(d);
        if caps.margins_for(&paper).is_some() {
            return;
        }
        let sheet = paper.display();
        if d.paper_update == 1 {
            let Some(default) = caps
                .default_paper
                .clone()
                .or_else(|| caps.media.first().map(|media| media.paper.clone()))
            else {
                return;
            };
            let d = &mut self.plot_dialog;
            d.paper = default.canonical.to_string();
            (d.paper_width_mm, d.paper_height_mm) = default.sheet_mm(plot_dialog_orientation(d));
            let default = default.display();
            self.command_line.push_info(
                crate::tf!(
                    "{printer} does not support {sheet}; switched to its default sheet \
                     {default} (PAPERUPDATE = 1)."
                )
                .as_ref(),
            );
        } else {
            self.command_line.push_info(
                crate::tf!(
                    "{printer} does not support {sheet}; the sheet is kept (PAPERUPDATE = 0)."
                )
                .as_ref(),
            );
        }
    }

    /// Spell the picked scale in the custom fields.
    pub(in crate::app) fn refresh_custom_scale_fields(&mut self) {
        let ((paper, drawing), _) = self.dialog_plot_scale_ratio();
        let d = &mut self.plot_dialog;
        d.custom_scale_paper = trim_decimals(&format!("{paper:.6}"));
        d.custom_scale_drawing = trim_decimals(&format!("{drawing:.6}"));
    }

    /// Make the typed custom ratio the plot scale: the scale list's name for
    /// it when it has one, else the ratio itself.
    pub(in crate::app) fn adopt_custom_scale(&mut self) {
        let d = &self.plot_dialog;
        if d.area == "Layout" {
            return;
        }
        let (Ok(paper), Ok(drawing)) = (
            d.custom_scale_paper.trim().parse::<f64>(),
            d.custom_scale_drawing.trim().parse::<f64>(),
        ) else {
            return;
        };
        if !(paper > 0.0 && drawing > 0.0 && paper.is_finite() && drawing.is_finite()) {
            return;
        }
        let conversion =
            self.tabs[self.active_tab].scene.scale_family_unit_mm() / d.paper_units.to_mm(1.0);
        let factor = paper / drawing / conversion;
        let d = &mut self.plot_dialog;
        d.scale = scale_name_for_factor(&d.scales, factor).unwrap_or_else(|| {
            let name = ratio_scale_name(factor);
            if !d.scales.iter().any(|(n, _)| n == &name) {
                d.scales.push((name.clone(), factor));
                d.scales.sort_by(|a, b| a.1.total_cmp(&b.1));
            }
            name
        });
    }

    pub(in crate::app) fn dialog_plot_offsets_mm(&self) -> (f64, f64) {
        let d = &self.plot_dialog;
        let millimetres = |offset: &str| {
            d.paper_units
                .to_mm(offset.trim().parse::<f64>().unwrap_or(0.0))
        };
        (millimetres(&d.offset_x), millimetres(&d.offset_y))
    }

    /// The plot origin in millimetres from the sheet's corner: the dialog's
    /// offsets in the page-setup unit, counted from the printable area's
    /// corner unless `PLOTOFFSET` says from the paper edge.
    pub(in crate::app) fn dialog_plot_origin_mm(&self) -> (f64, f64) {
        let (x, y) = self.dialog_plot_offsets_mm();
        if self.plot_dialog.plot_offset_from_edge {
            return (x, y);
        }
        let ps = self.plot_settings_from_dialog(self.dialog_base_settings());
        let m = ps.margins;
        let (left, bottom, ..) = crate::scene::rotated_margins(
            (m.left, m.bottom, m.right, m.top),
            ps.rotation.to_code(),
        );
        (x + left, y + bottom)
    }

    /// The settings the dialog edits on top of: the named page setup it was
    /// loaded from, else the active layout's own, else a fresh metric sheet.
    /// Everything the dialog has no control for (viewport-border and
    /// paper-update flags, the plot view handle, the paper unit that fixes
    /// the layout's coordinate system) is carried over from here untouched.
    pub(in crate::app) fn dialog_base_settings(&self) -> document::objects::PlotSettings {
        use document::objects::{PlotPaperUnits, PlotSettings};
        let scene = &self.tabs[self.active_tab].scene;
        self.plot_setup_template
            .clone()
            .or_else(|| scene.plot_settings_for(&scene.current_layout))
            .unwrap_or_else(|| {
                let mut settings = PlotSettings::new("");
                settings.paper_units = PlotPaperUnits::Millimeters;
                settings
            })
    }

    /// The page setup the dialog describes, written the way the file format
    /// expects it: the portrait medium with the orientation in `rotation`,
    /// standard scales by their code, custom ones as a readable ratio in the
    /// layout's own paper unit. Named page setups and the layout's embedded
    /// settings both come from here so the two can never drift apart.
    pub(in crate::app) fn plot_settings_from_dialog(
        &self,
        mut ps: document::objects::PlotSettings,
    ) -> document::objects::PlotSettings {
        use crate::io::paper_catalog::Orientation;
        use document::objects::{
            PlotPaperUnits, PlotType, ScaledType, ShadePlotMode, ShadePlotResolutionLevel,
        };
        let d = &self.plot_dialog;
        // Names and margins are decided against the stored settings before
        // any field is overwritten.
        let paper_size = paper_name_for_settings(d, &ps);
        let printer_name = device_name_for_settings(d, &ps);
        let margins = margins_for_settings(d, &ps);
        let same_medium = paper_unchanged(d, &ps) && ps.paper_width > 0.0 && ps.paper_height > 0.0;
        ps.paper_size = paper_size;
        ps.printer_name = printer_name;
        ps.margins = margins;
        // An unchanged sheet keeps the medium exactly as the drawing stores
        // it — its driver's precise dimensions in the orientation the driver
        // defines it (a PDF driver lists landscape media of its own) — and
        // only the rotation says how the sheet is plotted on it. A new
        // selection is written as the catalogue's portrait medium.
        let natural = if same_medium {
            if ps.paper_width > ps.paper_height {
                Orientation::Landscape
            } else {
                Orientation::Portrait
            }
        } else {
            let (width, height) = plot_dialog_paper(d).portrait_mm();
            ps.paper_width = width.max(1.0);
            ps.paper_height = height.max(1.0);
            Orientation::Portrait
        };
        ps.rotation = plot_dialog_rotation(d, natural);
        let layout_area = d.area == "Layout";
        // Millimetres one paper-space unit spans in a stored layout plot: a
        // layout drawn in inches stays in inches when the sheet changes. Any
        // other plot's scale says nothing about paper space (millimetres).
        let layout_unit_mm = {
            let ratio = ps.scale_numerator / ps.scale_denominator;
            if ps.plot_type == PlotType::Layout
                && !ps.flags.use_standard_scale
                && ratio.is_finite()
                && ratio > 1e-12
            {
                ratio * plot_paper_unit_mm(ps.paper_units)
            } else {
                1.0
            }
        };
        ps.plot_type = match d.area.as_str() {
            "Window" => PlotType::Window,
            "Layout" => PlotType::Layout,
            "Display" => PlotType::LastScreenDisplay,
            "Limits" => PlotType::Limits,
            area if area.starts_with("View: ") => PlotType::View,
            _ => PlotType::Extents,
        };
        ps.plot_view_name = d.area.strip_prefix("View: ").unwrap_or("").to_string();
        if d.area == "Window" {
            if let Some((x0, y0, x1, y1)) = self.plot_window {
                ps.set_plot_window(x0, y0, x1, y1);
            }
        }
        ps.flags.plot_centered = d.center && !layout_area;
        // The file keeps millimetres. A Layout offset is relative to the
        // printable corner, like the saved CAD page setup it came from.
        let (origin_x, origin_y) = self.dialog_plot_offsets_mm();
        ps.origin_x = origin_x;
        ps.origin_y = origin_y;
        ps.paper_units = match d.paper_units {
            crate::io::paper_catalog::PaperUnits::Inches => PlotPaperUnits::Inches,
            crate::io::paper_catalog::PaperUnits::Millimeters => PlotPaperUnits::Millimeters,
        };
        if layout_area {
            // Paper layouts use physical paper-space coordinates, independently
            // of INSUNITS and the model's scale family, and keep the unit the
            // layout is drawn in: a metric layout counts 1 mm per unit (on an
            // inch plotter, 1 inch is 25.4 such units), an inch layout 1 inch
            // per unit. Using dialog_plot_scale_ratio() here incorrectly
            // brings the model unit conversion (1000 for metres) into the
            // paper sheet: A3 then appears as 0.420 x 0.297 instead of 420 x
            // 297. Resetting an inch layout to millimetres drew ARCH D 914
            // units wide (#3131).
            let page_unit_mm = plot_paper_unit_mm(ps.paper_units);
            let denominator = page_unit_mm / layout_unit_mm;
            ps.scale_type = ScaledType::OneToOne;
            ps.flags.use_standard_scale = false;
            ps.scale_numerator = 1.0;
            ps.scale_denominator = denominator;
            ps.standard_scale_factor = stored_scale_factor(1.0 / denominator, ps.paper_units);
        } else if d.fit_to_paper {
            ps.set_scale_to_fit();
            // The commercial application marks fitted plots with the
            // "standard scale" bit and leaves the ratio it computed.
            ps.flags.use_standard_scale = true;
        } else {
            let ((numerator, denominator), standard) = self.dialog_plot_scale_ratio();
            // The commercial application leaves the "standard scale" bit
            // clear even for a standard code and keeps the ratio in the
            // numerator/denominator pair; readers that trust the bit would
            // otherwise take the stored factor as the ratio itself.
            ps.scale_type = standard.unwrap_or(ScaledType::CustomScale);
            ps.flags.use_standard_scale = false;
            ps.scale_numerator = numerator;
            ps.scale_denominator = denominator;
            ps.standard_scale_factor = stored_scale_factor(numerator / denominator, ps.paper_units);
        }
        ps.current_style_sheet = d.style_name.clone();
        ps.flags.scale_lineweights = d.scale_lw;
        ps.flags.print_lineweights = d.lineweights;
        ps.flags.plot_plot_styles = d.apply_plot_styles;
        ps.flags.show_plot_styles = d.show_plot_styles;
        ps.flags.draw_viewports_first = d.paperspace_last;
        ps.flags.plot_hidden = d.hide_paperspace;
        ps.shade_plot_mode = match d.shade.as_str() {
            "2D Wireframe" | "3D Wireframe" => ShadePlotMode::Wireframe,
            "Hidden Line" => ShadePlotMode::Hidden,
            "As displayed" => ShadePlotMode::AsDisplayed,
            _ => ShadePlotMode::Rendered,
        };
        ps.shade_plot_resolution = match d.quality.as_str() {
            "Low" => ShadePlotResolutionLevel::Draft,
            "High" => ShadePlotResolutionLevel::Presentation,
            _ => ShadePlotResolutionLevel::Normal,
        };
        ps.shade_plot_dpi = 300;
        ps
    }

    /// Write the dialog's page setup into the active layout. No-op on the
    /// Model tab (which has no paper layout). Marks the tab dirty and
    /// re-tessellates the sheet. Called by the Plot dialog's Set current action.
    pub(in crate::app) fn apply_plot_page_settings(&mut self) {
        let i = self.active_tab;
        let layout_name = self.tabs[i].scene.current_layout.clone();
        let ps = self.plot_settings_from_dialog(self.dialog_base_settings());
        if layout_name == "Model" {
            // Model keeps its page setup too (#1620); it has no sheet limits.
            self.tabs[i]
                .scene
                .set_layout_plot_settings(&layout_name, &ps);
            self.tabs[i].dirty = true;
            return;
        }
        let (w, h) = plot_dialog_sheet_mm(&self.plot_dialog);
        let plot_area = self.plot_dialog.area.clone();
        let center = ps.flags.plot_centered;
        let rotation = ps.rotation.to_degrees() as i32;
        self.tabs[i]
            .scene
            .set_layout_plot_settings(&layout_name, &ps);
        // Layout limits are the sheet in paper-space units, which the
        // settings just written define, placed the way the commercial
        // application places it: the printable area's corner is the paper
        // space origin, so the sheet starts at minus its displayed margins.
        let units_per_mm = self.tabs[i].scene.paper_space_unit_factor().max(1e-9);
        let m = ps.margins;
        let (left, bottom, ..) = crate::scene::rotated_margins(
            (m.left, m.bottom, m.right, m.top),
            ps.rotation.to_code(),
        );
        let (x0, y0) = (-left * units_per_mm, -bottom * units_per_mm);
        let (lw, lh) = (w * units_per_mm, h * units_per_mm);
        for obj in self.tabs[i].scene.document.objects.values_mut() {
            if let document::objects::ObjectType::Layout(layout) = obj {
                if layout.name == layout_name {
                    // A centred window plot puts the sheet around the window.
                    let ((x0, y0), (x1, y1)) = if layout.plot_type == 4 && center {
                        crate::scene::Scene::layout_sheet(layout)
                            .unwrap_or(((x0, y0), (x0 + lw, y0 + lh)))
                    } else {
                        ((x0, y0), (x0 + lw, y0 + lh))
                    };
                    layout.min_limits = (x0, y0);
                    layout.max_limits = (x1, y1);
                    layout.min_extents = (x0, y0, 0.0);
                    layout.max_extents = (x1, y1, 0.0);
                    break;
                }
            }
        }

        self.tabs[i].dirty = true;
        self.tabs[i].scene.bump_geometry_no_blocks();
        self.command_line.push_info(
            crate::tf!(
                "Page setup: {w:.1}×{h:.1} mm  area={plot_area}  \
                 center={center}  rot={rotation}°"
            )
            .as_ref(),
        );
    }

    /// Refresh the paper/scale context from the active layout without
    /// disturbing the device the user plotted with. Shared by the dialog's
    /// export continuation and the direct `EXPORTPDF <path>` form.
    pub(in crate::app) fn refresh_current_plot_context(&mut self) {
        let i = self.active_tab;
        if self.tabs[i].scene.current_layout == "Model" {
            self.plot_dialog.paper_space = false;
            // Layout is a paper-only plot area. Its saved fixed scale can put
            // every model object outside the sheet, making direct PREVIEW and
            // SVG/PDF export look blank. Match the Plot dialog's existing
            // normalization instead of carrying that state into Model.
            if self.plot_dialog.area == "Layout" {
                self.plot_dialog.area = "Extents".into();
                self.normalize_common_plot_dialog();
            }
            return;
        }
        self.plot_dialog.paper_space = true;
        self.plot_dialog.scales = self.tabs[i]
            .scene
            .scale_list()
            .into_iter()
            .map(|(name, _, factor)| (name, factor))
            .collect();
        let kept_printer = self.plot_dialog.printer.clone();
        let kept_to_file = self.plot_dialog.to_file;
        if let Some(settings) = self.tabs[i].scene.effective_plot_settings() {
            self.load_plotsettings_into_dialog(&settings);
        }
        self.plot_dialog.printer = kept_printer;
        self.plot_dialog.to_file = kept_to_file;
    }

    /// Write `path` through the same page pipeline the plot dialog uses, with
    /// no dialog and no background task, so a script gets the file before the
    /// command returns. `all_layouts` exports every paper layout instead of
    /// only the current one.
    ///
    /// Returns the page count, byte size and the first page's sheet size in
    /// millimetres.
    pub(in crate::app) fn export_pdf_direct(
        &mut self,
        path: &std::path::Path,
        all_layouts: bool,
    ) -> Result<(usize, u64, (f64, f64)), String> {
        let pages = self.direct_export_pages(all_layouts)?;
        let style = self.dialog_plot_style(&self.plot_dialog);
        if !all_layouts {
            self.validate_geopdf_page(&pages[0])?;
        }
        crate::io::pdf_export::export_plot_pdf_pages(&pages, path, style.as_ref())?;
        let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        let sheet = pages
            .first()
            .map(|page| (page.paper_w, page.paper_h))
            .unwrap_or((0.0, 0.0));
        Ok((pages.len(), bytes, sheet))
    }

    /// [`Self::export_pdf_direct`] writing a DWF file: one sheet for each
    /// page, under its layout's name.
    ///
    /// Returns the sheet count, byte size and the first sheet's size in
    /// millimetres.
    pub(in crate::app) fn export_dwf_direct(
        &mut self,
        path: &std::path::Path,
        all_layouts: bool,
    ) -> Result<(usize, u64, (f64, f64)), String> {
        let scene = &self.tabs[self.active_tab].scene;
        let names: Vec<String> = if all_layouts {
            scene
                .layout_names()
                .into_iter()
                .filter(|name| name != "Model")
                .collect()
        } else {
            vec![scene.current_layout.clone()]
        };
        let pages = self.direct_export_pages(all_layouts)?;
        let style = self.dialog_plot_style(&self.plot_dialog);
        let sheets: Vec<crate::io::dwf_export::DwfSheet> = pages
            .iter()
            .zip(names.iter())
            .map(|(page, name)| crate::io::dwf_export::DwfSheet {
                name: name.as_str(),
                page,
            })
            .collect();
        crate::io::dwf_export::export_dwf_pages(&sheets, path, style.as_ref())?;
        let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        let sheet = pages
            .first()
            .map(|page| (page.paper_w, page.paper_h))
            .unwrap_or((0.0, 0.0));
        Ok((sheets.len(), bytes, sheet))
    }

    /// The pages a direct export writes: the current layout's, or one for
    /// every paper layout.
    fn direct_export_pages(&mut self, all_layouts: bool) -> Result<Vec<PdfPageInput>, String> {
        self.stamp_plot_fields();
        let pages = if all_layouts {
            let i = self.active_tab;
            let saved = std::mem::take(&mut self.print_all_layouts);
            self.print_all_layouts = self.tabs[i]
                .scene
                .layout_names()
                .into_iter()
                .filter(|name| name != "Model")
                .map(|name| (name, true))
                .collect();
            let result = self.print_all_pages();
            self.print_all_layouts = saved;
            let pages = result?;
            if pages.is_empty() {
                return Err(
                    crate::t!("Nothing to plot: the drawing has no paper layouts.").into_owned(),
                );
            }
            pages
        } else {
            self.refresh_current_plot_context();
            vec![self.direct_plot_page().ok_or_else(|| {
                crate::t!("Nothing to plot: model space contains no printable geometry.")
                    .into_owned()
            })?]
        };
        Ok(pages)
    }

    /// Publish named layouts into one PDF, each with its own page setup, and
    /// return each sheet's size so the caller can report per-sheet status.
    pub(in crate::app) fn export_pdf_layout_names(
        &mut self,
        path: &std::path::Path,
        names: &[String],
    ) -> Result<(usize, u64, Vec<(String, f64, f64)>), String> {
        if names.is_empty() {
            return Err(
                crate::t!("Nothing to publish: no paper layouts were selected.").into_owned(),
            );
        }
        self.stamp_plot_fields();
        let saved = std::mem::take(&mut self.print_all_layouts);
        self.print_all_layouts = names.iter().cloned().map(|name| (name, true)).collect();
        let result = self.print_all_pages();
        self.print_all_layouts = saved;
        let pages = result?;
        if pages.is_empty() {
            return Err(crate::t!("Nothing to publish: no page could be built.").into_owned());
        }
        let style = self.dialog_plot_style(&self.plot_dialog);
        crate::io::pdf_export::export_plot_pdf_pages(&pages, path, style.as_ref())?;
        let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        let sheets = names
            .iter()
            .zip(pages.iter())
            .map(|(name, page)| (name.clone(), page.paper_w, page.paper_h))
            .collect();
        Ok((pages.len(), bytes, sheets))
    }

    /// Write a vector preview of the current layout through the same page and
    /// plot-style pipeline PLOT uses. Returns page count, byte size, the first
    /// sheet's millimetres, the plot scale and the style name (if any).
    pub(in crate::app) fn export_preview_svg(
        &mut self,
        path: &std::path::Path,
    ) -> Result<(usize, u64, (f64, f64), f64, String), String> {
        self.stamp_plot_fields();
        self.refresh_current_plot_context();
        let page = self.direct_plot_page().ok_or_else(|| {
            crate::t!("Nothing to preview: the layout contains no printable geometry.").into_owned()
        })?;
        let style = self.dialog_plot_style(&self.plot_dialog);
        let style_name = if self.plot_dialog.style_name.is_empty() {
            "None".to_string()
        } else {
            self.plot_dialog.style_name.clone()
        };
        let (paper_w, paper_h, scale) = (page.paper_w, page.paper_h, page.scale as f64);
        crate::io::svg_export::export_svg_pages(&[page], path, style.as_ref())?;
        let bytes = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
        Ok((1, bytes, (paper_w, paper_h), scale, style_name))
    }

    pub(in crate::app) fn on_plot_export_path_some(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        let is_svg = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));
        let is_dwf = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dwf"));
        self.refresh_current_plot_context();
        let Some(page) = self.direct_plot_page() else {
            self.command_line.push_error(
                crate::t!("Nothing to plot: model space contains no printable geometry.").as_ref(),
            );
            return Task::none();
        };
        if !is_svg && !is_dwf {
            if let Err(error) = self.validate_geopdf_page(&page) {
                self.command_line.push_error(&error);
                return Task::none();
            }
        }
        let worker_path = path.clone();
        let open_when_done = self.plot_dialog.open_when_done;
        let sheet = self.tabs[self.active_tab].scene.current_layout.clone();
        let work = move || {
            // SVG and DWF share the plot page with PDF: pick the writer from
            // the chosen extension.
            let result = if is_svg {
                crate::io::svg_export::export_svg(&page, &worker_path)
            } else if is_dwf {
                crate::io::dwf_export::export_dwf(&page, &sheet, &worker_path)
            } else {
                crate::io::pdf_export::export_plot_pdf(&page, &worker_path)
            };
            if result.is_ok() && open_when_done {
                // A viewer that cannot start leaves the file written all the same.
                let _ = crate::io::print_to_printer::open_in_viewer(&worker_path);
            }
            result
                .map(|_| crate::tf!("Exported: {}", worker_path.display()).into_owned())
                .map_err(|e| crate::tf!("Export failed: {e}").into_owned())
        };
        self.run_plot_work(self.plot_dialog.background, false, work)
    }

    /// Export the pending Extents/Window/Display area using the same clipped
    /// render path in model space and paper space.
    pub(in crate::app) fn on_plot_window_export_path_some(
        &mut self,
        path: std::path::PathBuf,
    ) -> Task<Message> {
        let is_svg = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("svg"));
        let is_dwf = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dwf"));
        let job = match self.plot_dialog.area.as_str() {
            "Display" => self.display_plot_job(),
            "Extents" => self.extents_plot_job(),
            _ => self.window_plot_job(),
        };
        let Some(page) = job else {
            self.command_line
                .push_error(crate::t!("Plot area is empty. Pick a larger window.").as_ref());
            return Task::none();
        };
        if !is_svg && !is_dwf {
            if let Err(error) = self.validate_geopdf_page(&page) {
                self.command_line.push_error(&error);
                return Task::none();
            }
        }
        let worker_path = path.clone();
        self.close_active_modal();
        let open_when_done = self.plot_dialog.open_when_done;
        let sheet = self.tabs[self.active_tab].scene.current_layout.clone();
        let work = move || {
            let result = if is_svg {
                crate::io::svg_export::export_svg(&page, &worker_path)
            } else if is_dwf {
                crate::io::dwf_export::export_dwf(&page, &sheet, &worker_path)
            } else {
                crate::io::pdf_export::export_plot_pdf(&page, &worker_path)
            };
            if result.is_ok() && open_when_done {
                // A viewer that cannot start leaves the file written all the same.
                let _ = crate::io::print_to_printer::open_in_viewer(&worker_path);
            }
            result
                .map(|_| {
                    crate::tf!(
                        "Plotted window to {}",
                        worker_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    )
                    .into_owned()
                })
                .map_err(|e| crate::tf!("Plot failed: {e}").into_owned())
        };
        self.run_plot_work(self.plot_dialog.background, false, work)
    }

    pub(in crate::app) fn report_plot_result(&mut self, result: Result<String, String>, reopen_plot: bool) {
        match result {
            Ok(message) => self.command_line.push_info(&message),
            Err(error) => self.command_line.push_error(&error),
        }
        if reopen_plot {
            self.active_modal = Some(crate::app::ModalKind::Plot);
        }
    }

    pub(in crate::app) fn on_print_all_open(&mut self) -> Task<Message> {
        self.print_all_layouts = self.tabs[self.active_tab]
            .scene
            .layout_names()
            .into_iter()
            .filter(|name| name != "Model")
            .map(|name| (name, true))
            .collect();
        self.print_all_settings_override = false;
        self.active_modal = Some(crate::app::ModalKind::PrintAll);
        self.reset_modal_geometry();
        Task::none()
    }

    pub(in crate::app) fn on_print_all_options(&mut self) -> Task<Message> {
        let previous = self.plot_dialog.clone();
        let previous_style = self.active_plot_style.clone();
        let previous_window = self.plot_window;

        // Refresh runtime-only data such as printers, available media,
        // CTB names, scales and page-setup rows.
        //
        // `on_plot_dialog_open()` also seeds the editor from the active
        // layout. That is correct for normal PLOT, but Print All Options is a
        // shared batch override editor: its persisted user choices must win.
        let task = self.on_plot_dialog_open();

        // Restore only user-editable values. `copy_settings_from` deliberately
        // leaves the freshly refreshed runtime lists intact.
        //
        // Do this even on the first Print All Options opening after restarting
        // OCS. Previously this happened only when `print_all_settings_override`
        // was already true, so the first opening always fell back to the
        // active layout/default A4 and lost the persisted CTB selection.
        self.plot_dialog.copy_settings_from(&previous);

        // Preserve the parsed CTB when it matches the restored selection.
        // If it is absent/stale, dialog_plot_style() can resolve it by name.
        self.active_plot_style = previous_style.clone();
        self.plot_window = previous_window;

        self.print_all_options_prev = Some(previous);
        self.print_all_plot_style_prev = Some(previous_style);
        self.print_all_plot_window_prev = Some(previous_window);

        self.print_all_options = true;
        self.plot_dialog.paper_space = true;
        self.plot_dialog.area = "Layout".into();

        task
    }

    /// A plot gives its fields (PlotDate) the plot time, as the reference
    /// stores it; the hosts are redrawn before the plot reads the scene.
    pub(in crate::app) fn stamp_plot_fields(&mut self) {
        let i = self.active_tab;
        let hosts = crate::entities::field::stamp_plot_fields(&mut self.tabs[i].scene.document);
        if !hosts.is_empty() {
            self.tabs[i].scene.bump_text_hosts(&hosts);
            self.tabs[i].dirty = true;
        }
    }

    pub(in crate::app) fn print_all_pages(&mut self) -> Result<Vec<crate::io::pdf_export::PdfPageInput>, String> {
        self.stamp_plot_fields();
        let available = self.tabs[self.active_tab].scene.layout_names();
        let selected: Vec<String> = self
            .print_all_layouts
            .iter()
            .filter(|(name, checked)| *checked && available.contains(name))
            .map(|(name, _)| name.clone())
            .collect();
        if selected.is_empty() {
            return Err(crate::t!("Select at least one layout.").into_owned());
        }

        let i = self.active_tab;
        let original_layout = self.tabs[i].scene.current_layout.clone();
        let original_viewport = self.tabs[i].scene.active_viewport;
        let original_psltscale = self.tabs[i]
            .scene
            .document
            .header
            .paper_space_linetype_scaling;
        let original_plimcheck = self.tabs[i].scene.document.header.paper_space_limit_check;
        let original_dialog = self.plot_dialog.clone();
        let original_style = self.active_plot_style.clone();
        let original_window = self.plot_window;
        let original_setup = self.plot_setup_template.clone();
        let original_camera = self.tabs[i].scene.camera.borrow().clone();
        let original_camera_generation = self.tabs[i].scene.camera_generation;
        let override_dialog = self
            .print_all_settings_override
            .then(|| original_dialog.clone());
        let override_style = self
            .print_all_settings_override
            .then(|| original_style.clone());
        let result = (|| {
            let mut pages = Vec::with_capacity(selected.len());
            for name in selected {
                let page_setup = self.tabs[i]
                    .scene
                    .plot_settings_for(&name)
                    .ok_or_else(|| crate::tf!("Layout '{name}' has no page setup.").into_owned())?;
                {
                    let scene = &mut self.tabs[i].scene;
                    scene.current_layout = name.clone();
                    scene.active_viewport = None;
                    scene.load_current_layout_state();
                }
                if let Some(dialog) = &override_dialog {
                    self.plot_dialog = dialog.clone();
                    self.active_plot_style = override_style.clone().flatten();
                    self.plot_window = original_window;
                } else {
                    self.plot_dialog.paper_space = true;
                    self.plot_window = None;
                    self.plot_dialog.window = None;
                    self.load_plotsettings_into_dialog(&page_setup);
                }
                let dialog = self.plot_dialog.clone();
                if dialog.area == "Display" {
                    self.tabs[i].scene.restore_saved_camera();
                }
                if dialog.style_missing && dialog.apply_plot_styles {
                    return Err(crate::tf!(
                        "Layout '{name}' plot style table '{}' is not loaded.",
                        dialog.style_name
                    )
                    .into_owned());
                }
                let area_page = match dialog.area.as_str() {
                    "Display" => self.display_plot_job(),
                    "Extents" => self.extents_plot_job(),
                    "Limits" => self.limits_plot_job(),
                    "Window" => self.window_plot_job(),
                    area if area.starts_with("View: ") => {
                        self.named_view_plot_job(area.trim_start_matches("View: "))
                    }
                    _ => None,
                };
                let page = if dialog.area == "Layout" {
                    self.layout_plot_page_for("Layout")
                } else {
                    area_page.ok_or_else(|| {
                        crate::tf!("Layout '{name}' plot area is empty.").into_owned()
                    })?
                };
                pages.push(page);
            }
            Ok(pages)
        })();
        self.tabs[i].scene.current_layout = original_layout;
        self.tabs[i].scene.active_viewport = original_viewport;
        self.tabs[i]
            .scene
            .document
            .header
            .paper_space_linetype_scaling = original_psltscale;
        self.tabs[i].scene.document.header.paper_space_limit_check = original_plimcheck;
        self.plot_dialog = original_dialog;
        self.active_plot_style = original_style;
        self.plot_window = original_window;
        self.plot_setup_template = original_setup;
        *self.tabs[i].scene.camera.borrow_mut() = original_camera;
        self.tabs[i].scene.camera_generation = original_camera_generation;
        result
    }

    pub(in crate::app) fn on_print_all_pdf_path_some(&mut self, path: std::path::PathBuf) -> Task<Message> {
        let dialog = self.plot_dialog.clone();
        if self.print_all_settings_override && dialog.style_missing && dialog.apply_plot_styles {
            self.command_line.push_error(
                crate::tf!("Plot style table '{}' is not loaded.", dialog.style_name).as_ref(),
            );
            return Task::none();
        }
        let pages = match self.print_all_pages() {
            Ok(pages) => pages,
            Err(error) => {
                self.command_line.push_error(&error);
                return Task::none();
            }
        };
        let worker_path = path.clone();
        let fallback_style = self.dialog_plot_style(&dialog);
        self.save_config();
        self.close_active_modal();
        let work = move || {
            crate::io::pdf_export::export_plot_pdf_pages(
                &pages,
                &worker_path,
                fallback_style.as_ref(),
            )
            .map(|_| {
                crate::tf!(
                    "Exported {} layouts to {}",
                    pages.len(),
                    worker_path.display()
                )
                .into_owned()
            })
            .map_err(|error| crate::tf!("Export failed: {error}").into_owned())
        };
        self.run_print_all_work(dialog.background_publish, work)
    }

    pub(in crate::app) fn on_print_all_print(&mut self) -> Task<Message> {
        let dialog = self.plot_dialog.clone();
        if self.print_all_settings_override && dialog.style_missing && dialog.apply_plot_styles {
            self.command_line.push_error(
                crate::tf!("Plot style table '{}' is not loaded.", dialog.style_name).as_ref(),
            );
            return Task::none();
        }
        let pages = match self.print_all_pages() {
            Ok(pages) => pages,
            Err(error) => {
                self.command_line.push_error(&error);
                return Task::none();
            }
        };
        self.save_config();
        self.close_active_modal();
        // The web build has no system printers: render every layout into one
        // PDF and open it in a browser tab for the browser print dialog.
        #[cfg(target_arch = "wasm32")]
        {
            let stem = self.active_stem("_layouts");
            let work = move || {
                crate::io::print_to_printer::preview_pages_pdf(&stem, &pages)
                    .map(|_| crate::t!("Opened plot preview.").into_owned())
                    .map_err(|e| crate::tf!("Preview failed: {e}").into_owned())
            };
            self.run_print_all_work(true, work)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let options = self.plot_print_options(&dialog);
            let fallback_style = self.dialog_plot_style(&dialog);
            let temp_path = crate::io::print_to_printer::temp_pdf_path("print_all");
            self.command_line
                .push_info(crate::t!("Sending selected layouts to the system printer…").as_ref());
            let work = move || {
                crate::io::pdf_export::export_pdf_pages(&pages, &temp_path, fallback_style.as_ref())
                    .and_then(|_| {
                        crate::io::print_to_printer::print_existing_pdf(&temp_path, &options)
                    })
                    .map(|printer| {
                        crate::tf!("Sent {} layouts to printer: {printer}", pages.len())
                            .into_owned()
                    })
                    .map_err(|error| crate::tf!("Print failed: {error}").into_owned())
            };
            self.run_print_all_work(true, work)
        }
    }

    pub(in crate::app) fn run_print_all_work<F>(&mut self, background: bool, work: F) -> Task<Message>
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if background {
            background_task(crate::t!("Print all"), work, Message::PrintAllFinished)
        } else {
            Task::done(Message::PrintAllFinished(work()))
        }
    }

    pub(in crate::app) fn run_plot_work<F>(&mut self, background: bool, reopen_plot: bool, work: F) -> Task<Message>
    where
        F: FnOnce() -> Result<String, String> + Send + 'static,
    {
        if background {
            background_task(crate::t!("Plot"), work, move |result| {
                Message::BackgroundIoFinished(result, reopen_plot)
            })
        } else {
            let result = work();
            self.report_plot_result(result, reopen_plot);
            Task::none()
        }
    }

    /// PNGOUT / JPGOUT / BMPOUT / TIFOUT: `handles` drawn as they plot,
    /// fitted into an image the size of the drawing area, on white.
    pub(in crate::app) fn raster_out(
        &mut self,
        cmd: &str,
        handles: &[document::Handle],
    ) -> Task<Message> {
        let (ext, name, format) = match cmd {
            "JPGOUT" => ("jpg", "JPEG", image::ImageFormat::Jpeg),
            "BMPOUT" => ("bmp", "BMP", image::ImageFormat::Bmp),
            "TIFOUT" => ("tif", "TIFF", image::ImageFormat::Tiff),
            _ => ("png", "PNG", image::ImageFormat::Png),
        };
        let Some((page, width, height)) = self.raster_out_page(handles) else {
            self.command_line
                .push_error(crate::t!("None of the selected entities could be exported.").as_ref());
            return Task::none();
        };
        let start = move |path: std::path::PathBuf| {
            background_task(
                crate::t!("Export"),
                move || {
                    let bytes = raster_page(&page, width, height, format)?;
                    write_export(&path, &bytes)?;
                    Ok(crate::tf!("Exported: {}", path.display()).into_owned())
                },
                |result| Message::BackgroundIoFinished(result, false),
            )
        };
        let file_name = format!("{}.{ext}", self.active_stem(""));
        #[cfg(target_arch = "wasm32")]
        {
            let _ = name;
            start(std::path::PathBuf::from(file_name))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut start = Some(start);
            Task::perform(
                async move {
                    crate::sys::file_dialog()
                        .set_title(crate::t!("Export").as_ref())
                        .set_file_name(&file_name)
                        .add_filter(name, &[ext])
                        .add_filter(crate::t!("All Files").as_ref(), &["*"])
                        .save_file()
                        .await
                        .map(|h| crate::sys::handle_path(&h))
                },
                |path| path,
            )
            .then(move |path| match (path, start.take()) {
                (Some(path), Some(start)) => start(path),
                _ => Task::none(),
            })
        }
    }

    /// The plot page of `handles` alone, fitted with a margin into a sheet
    /// the size of the drawing area at 96 dpi, and that size in pixels.
    /// `None` when they draw nothing.
    pub(in crate::app) fn raster_out_page(
        &self,
        handles: &[document::Handle],
    ) -> Option<(PdfPageInput, u32, u32)> {
        let scene = &self.tabs[self.active_tab].scene;
        let wanted: rustc_hash::FxHashSet<document::Handle> = handles.iter().copied().collect();
        // Keep the selected items; `split` counts the kept ones of the
        // first group.
        fn keep<T>(items: &mut Vec<T>, split: &mut usize, mut keep: impl FnMut(&T) -> bool) {
            let (mut index, mut first) = (0, 0);
            items.retain(|item| {
                let kept = keep(item);
                if kept && index < *split {
                    first += 1;
                }
                index += 1;
                kept
            });
            *split = first;
        }
        let mut content = plot_scene_content(scene, false, None);
        let splits = &mut content.group_splits;
        keep(
            std::sync::Arc::make_mut(&mut content.wires),
            &mut splits.wires,
            |wire| {
                crate::scene::Scene::handle_from_wire_name(&wire.name)
                    .is_some_and(|h| wanted.contains(&h))
            },
        );
        keep(&mut content.hatches, &mut splits.hatches, |hatch| {
            hatch.entity.is_some_and(|h| wanted.contains(&h))
        });
        keep(&mut content.wipeouts, &mut splits.wipeouts, |hatch| {
            hatch.entity.is_some_and(|h| wanted.contains(&h))
        });
        // ponytail: image models carry no entity handle, so raster images
        // are left out; add one to ImageModel to export them too.
        content.images.clear();
        splits.images = 0;
        let (min_x, min_y, max_x, max_y) = plot_content_extents(&content)?;
        let (vp_w, vp_h) = scene.selection.borrow().view.vp_size;
        let (width, height) = if vp_w >= 1.0 && vp_h >= 1.0 {
            (vp_w.round() as u32, vp_h.round() as u32)
        } else {
            (1600, 1200)
        };
        // Paper at 96 dpi, so lineweights come out as on screen.
        const MM_PER_PX: f64 = 25.4 / 96.0;
        let (paper_w, paper_h) = (width as f64 * MM_PER_PX, height as f64 * MM_PER_PX);
        let (content_w, content_h) = ((max_x - min_x).max(1e-9), (max_y - min_y).max(1e-9));
        const MARGIN: f64 = 1.05;
        let scale = ((paper_w / MARGIN) / content_w)
            .min((paper_h / MARGIN) / content_h)
            .max(1e-9);
        let mut options = Self::pdf_plot_options(&self.plot_dialog);
        options.text_as_outlines = true;
        options.stamp = false;
        options.scale_lineweights = false;
        let page = PdfPageInput {
            content,
            paper_w,
            paper_h,
            offset_x: (paper_w - content_w * scale) * 0.5 / scale - min_x,
            offset_y: (paper_h - content_h * scale) * 0.5 / scale - min_y,
            rotation_deg: 0,
            content_rotation: 0.0,
            scale: scale as f32,
            clip: None,
            options,
            plot_style: None,
            geopdf: None,
        };
        Some((page, width, height))
    }

    /// Assemble a layout page for PDF export, preview, or printing.
    pub(in crate::app) fn layout_plot_page(&self) -> PdfPageInput {
        self.layout_plot_page_for(&self.plot_dialog.area)
    }

    pub(in crate::app) fn layout_plot_page_for(&self, plot_area: &str) -> PdfPageInput {
        let i = self.active_tab;
        let scene = &self.tabs[i].scene;
        let paper_space = scene.current_layout != "Model";
        let selected_sheet = plot_dialog_sheet_mm(&self.plot_dialog);
        let mut content = plot_scene_content(
            scene,
            self.plot_dialog.paperspace_last,
            plot_render_mode_override(&self.plot_dialog),
        );
        // The printable-area rectangle is an on-screen guide, not drawing
        // content. It used to leak into every paper-space PDF/preview/print.
        if paper_space {
            content.group_splits.wires = content.wires
                [..content.group_splits.wires.min(content.wires.len())]
                .iter()
                .filter(|wire| wire.name != "paper_printable_area")
                .count();
            std::sync::Arc::make_mut(&mut content.wires)
                .retain(|wire| wire.name != "paper_printable_area");
        }
        if let Some(((x0, y0), (x1, y1))) = scene.paper_limits() {
            // Paper geometry is stored in layout units; PDF pages use mm.
            // Scale both geometry and offsets by the same unit conversion.
            // The old path converted only page width/height, so inch/scaled
            // layouts placed tiny or 25.4×-shifted content on the page.
            let units_per_mm = scene.paper_space_unit_factor().max(1e-9);
            let mm_per_unit = 1.0 / units_per_mm;
            let paper_w = ((x1 - x0) * mm_per_unit).max(1.0);
            let paper_h = ((y1 - y0) * mm_per_unit).max(1.0);
            // Positioning choices are transient until explicitly saved. Layout
            // area keeps the sheet's current physical bounds; the other area
            // modes use the dialog paper size through `area_plot_job`.
            let dialog_rotation = if self.plot_dialog.upside_down { 180 } else { 0 };
            let plot_extents = plot_area == "Extents";
            let plot_layout = plot_area == "Layout";
            // Page orientation is already represented by the sheet bounds.
            // Only the explicit upside-down choice rotates layout content here.
            let rotation = dialog_rotation;
            let centered = self.plot_dialog.center;
            let (origin_x, origin_y) = if plot_layout {
                self.dialog_plot_offsets_mm()
            } else {
                self.dialog_plot_origin_mm()
            };

            let (scale, offset_x, offset_y) = if plot_extents {
                let (min_x, min_y, max_x, max_y) =
                    plot_content_extents(&content).unwrap_or((x0, y0, x1, y1));
                let content_w = (max_x - min_x).max(1e-9);
                let content_h = (max_y - min_y).max(1e-9);
                let scale = if self.plot_dialog.fit_to_paper {
                    const MARGIN: f64 = 1.05;
                    ((paper_w / MARGIN) / content_w)
                        .min((paper_h / MARGIN) / content_h)
                        .max(1e-9)
                } else {
                    (plot_dialog_scale_factor(&self.plot_dialog) * mm_per_unit).max(1e-9)
                };
                let target_x = if centered {
                    (paper_w - content_w * scale) * 0.5
                } else {
                    origin_x
                };
                let target_y = if centered {
                    (paper_h - content_h * scale) * 0.5
                } else {
                    origin_y
                };
                (scale, target_x / scale - min_x, target_y / scale - min_y)
            } else if plot_layout {
                // Map the complete paper-space sheet to the physical paper
                // selected in the dialog. Layout bounds can be stored in mm,
                // inches, or carry incomplete legacy metadata; deriving the
                // scale from the visible sheet avoids applying a stale unit
                // factor twice while preserving loaded layouts exactly when
                // their metadata is valid.
                let bounds_w = (x1 - x0).max(1e-9);
                let bounds_h = (y1 - y0).max(1e-9);
                let scale = (selected_sheet.0 / bounds_w)
                    .min(selected_sheet.1 / bounds_h)
                    .max(1e-9);
                let target_x = (selected_sheet.0 - bounds_w * scale) * 0.5;
                let target_y = (selected_sheet.1 - bounds_h * scale) * 0.5;
                (scale, target_x / scale - x0, target_y / scale - y0)
            } else {
                // Legacy direct callers still get dialog positioning. Normal
                // Display/Window paths use `area_plot_job` instead.
                let scale = mm_per_unit;
                let target_x = if centered {
                    (paper_w - (x1 - x0) * scale) * 0.5
                } else {
                    origin_x
                };
                let target_y = if centered {
                    (paper_h - (y1 - y0) * scale) * 0.5
                } else {
                    origin_y
                };
                (scale, target_x / scale - x0, target_y / scale - y0)
            };
            let (base_page_w, base_page_h) = if plot_layout {
                selected_sheet
            } else {
                (paper_w, paper_h)
            };
            let (page_w, page_h) = match rotation {
                90 | 270 => (base_page_h, base_page_w),
                _ => (base_page_w, base_page_h),
            };
            // Layout plots keep the full physical sheet as the PDF page, but
            // ink is restricted to the device's printable rectangle. Extents
            // has its own fitted area and must not inherit this paper clip.
            let clip = if plot_layout {
                scene
                    .printable_area_limits()
                    .map(|((px0, py0), (px1, py1))| {
                        (
                            (px0 + offset_x) as f32,
                            (py0 + offset_y) as f32,
                            (px1 - px0) as f32,
                            (py1 - py0) as f32,
                        )
                    })
            } else {
                None
            };
            // Move the ink relative to the printable corner. Keep the clip
            // anchored to the physical page.
            let (offset_x, offset_y) = if plot_layout {
                (offset_x + origin_x / scale, offset_y + origin_y / scale)
            } else {
                (offset_x, offset_y)
            };
            return PdfPageInput {
                content,
                paper_w: page_w,
                paper_h: page_h,
                offset_x,
                offset_y,
                rotation_deg: rotation,
                content_rotation: 0.0,
                scale: scale as f32,
                clip,
                options: Self::pdf_plot_options(&self.plot_dialog),
                plot_style: self.dialog_plot_style(&self.plot_dialog),
                geopdf: None,
            };
        }

        // Model space keeps its established extents + 5% margin behaviour.
        // A sheet edge below this floor (drawing with tiny extents, or the
        // 0×0 paper a fresh unsaved drawing carries) cannot be previewed,
        // printed, or measured meaningfully; the standard-sheet fallback
        // keeps the output a real page instead of a speck.
        const MIN_MODEL_SHEET_MM: f64 = 10.0;
        let standard_sheet = {
            let (w, h) = selected_sheet;
            (w.min(h) >= MIN_MODEL_SHEET_MM).then_some((w, h))
        };
        let (page_w, page_h, offset_x, offset_y) =
            if let Some((min, max)) = scene.model_space_extents() {
                let margin = 1.05_f64;
                let width = ((max.x - min.x) as f64 * margin).max(1.0);
                let height = ((max.y - min.y) as f64 * margin).max(1.0);
                if width < MIN_MODEL_SHEET_MM || height < MIN_MODEL_SHEET_MM {
                    // Per-axis: a thin-but-long profile keeps its long
                    // dimension instead of being clipped to the sheet, and
                    // the content is centered on the enlarged page.
                    let (w, h) = standard_sheet.unwrap_or((297.0, 210.0));
                    let page_w = width.max(w);
                    let page_h = height.max(h);
                    (
                        page_w,
                        page_h,
                        (page_w - width) * 0.5 - min.x as f64,
                        (page_h - height) * 0.5 - min.y as f64,
                    )
                } else {
                    let pad_x = (width - (max.x - min.x) as f64) * 0.5;
                    let pad_y = (height - (max.y - min.y) as f64) * 0.5;
                    (
                        width,
                        height,
                        -(min.x as f64) + pad_x,
                        -(min.y as f64) + pad_y,
                    )
                }
            } else {
                (297.0, 210.0, 0.0, 0.0)
            };
        PdfPageInput {
            content,
            paper_w: page_w,
            paper_h: page_h,
            offset_x,
            offset_y,
            rotation_deg: 0,
            content_rotation: 0.0,
            scale: 1.0,
            clip: None,
            options: Self::pdf_plot_options(&self.plot_dialog),
            plot_style: self.dialog_plot_style(&self.plot_dialog),
            geopdf: None,
        }
    }

    /// Build direct PDF/printer output using the physical layout sheet in
    /// paper space and the selected ISO sheet/scale around Model extents.
    pub(in crate::app) fn direct_plot_page(&self) -> Option<PdfPageInput> {
        if self.tabs[self.active_tab].scene.current_layout != "Model" {
            return Some(self.layout_plot_page_for("Layout"));
        }
        let page = self.extents_plot_job()?;
        let content = &page.content;
        if content.wires.is_empty()
            && content.hatches.is_empty()
            && content.wipeouts.is_empty()
            && content.images.is_empty()
        {
            return None;
        }
        Some(page)
    }

    pub(in crate::app) fn on_print_to_printer(&mut self) -> Task<Message> {
        let Some(page) = self.direct_plot_page() else {
            self.command_line.push_error(
                crate::t!("Nothing to plot: model space contains no printable geometry.").as_ref(),
            );
            return Task::none();
        };
        // The web build has no system printers: PRINT opens the rendered PDF
        // in a browser tab, where the browser's own print dialog takes over.
        #[cfg(target_arch = "wasm32")]
        {
            let stem = self.active_stem("");
            background_task(
                crate::t!("Plot preview"),
                move || {
                    crate::io::print_to_printer::preview_page_pdf(&stem, &page)
                        .map(|_| crate::t!("Opened plot preview.").into_owned())
                        .map_err(|e| crate::tf!("Preview failed: {e}").into_owned())
                },
                |result| Message::BackgroundIoFinished(result, false),
            )
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let options = self.plot_print_options(&self.plot_dialog);
            self.command_line
                .push_info(crate::t!("Sending to system printer…").as_ref());
            background_task(
                crate::t!("Print to printer"),
                move || {
                    iced::futures::executor::block_on(
                        crate::io::print_to_printer::print_wires_with(page, options),
                    )
                },
                Message::PrintResult,
            )
        }
    }

    /// QUICKPRINT / QP — use the current selection's bounding box as the plot
    /// window and export a PDF (drawing folder + name + timestamp) with the
    /// active page setup, no dialog. Model space only. (#325)
    pub(crate) fn on_quick_print_handles(
        &mut self,
        handles: Vec<document::Handle>,
    ) -> Task<Message> {
        let i = self.active_tab;
        if self.tabs[i].scene.current_layout != "Model" {
            self.command_line
                .push_error(crate::t!("Quick print works in model space.").as_ref());
            return Task::none();
        }
        let set: std::collections::HashSet<document::Handle> = handles.into_iter().collect();
        // Union the AABBs of the picked entities' wires (world XY), matched by
        // each wire's handle.
        let (x0, y0, x1, y1, any) = {
            let scene = &self.tabs[i].scene;
            let mut x0 = f32::INFINITY;
            let mut y0 = f32::INFINITY;
            let mut x1 = f32::NEG_INFINITY;
            let mut y1 = f32::NEG_INFINITY;
            let mut any = false;
            for w in scene.entity_wires().iter() {
                let picked = crate::scene::Scene::handle_from_wire_name(&w.name)
                    .is_some_and(|h| set.contains(&h));
                if !picked {
                    continue;
                }
                let [ax0, ay0, ax1, ay1] = w.aabb;
                if ax0.is_finite() && ay0.is_finite() && ax1.is_finite() && ay1.is_finite() {
                    x0 = x0.min(ax0);
                    y0 = y0.min(ay0);
                    x1 = x1.max(ax1);
                    y1 = y1.max(ay1);
                    any = true;
                }
            }
            (x0, y0, x1, y1, any)
        };
        if !any {
            self.command_line
                .push_error(crate::t!("Selection has no printable geometry.").as_ref());
            return Task::none();
        }
        if !(x1 > x0 && y1 > y0) {
            self.command_line
                .push_error(crate::t!("Selection has no printable area.").as_ref());
            return Task::none();
        }
        // Small margin so the outermost strokes aren't clipped flush to the edge.
        let mx = ((x1 - x0) * 0.02).max(0.0);
        let my = ((y1 - y0) * 0.02).max(0.0);
        // Window selection is needed to run this one job, but QuickPrint must
        // not change the user's saved Plot-area choice. The export helper has
        // built its page before returning, so restoring here is final.
        let previous_area = self.plot_dialog.area.clone();
        self.plot_dialog.area = "Window".into();
        self.plot_window = Some(self.view_frame_box((
            (x0 - mx) as f64,
            (y0 - my) as f64,
            (x1 + mx) as f64,
            (y1 + my) as f64,
        )));
        let path = self.quick_print_path();
        let task = self.on_plot_window_export_path_some(path);
        self.plot_dialog.area = previous_area;
        task
    }

    /// Auto output path for quick print: the drawing's folder + name + a
    /// timestamp, falling back to the temp dir / "drawing" when unsaved.
    pub(in crate::app) fn quick_print_path(&self) -> std::path::PathBuf {
        let i = self.active_tab;
        let cur = self.tabs[i].current_path.as_deref();
        let dir = cur
            .and_then(|p| p.parent())
            .map(|d| d.to_path_buf())
            .unwrap_or_else(std::env::temp_dir);
        let stem = cur
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "drawing".into());
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        dir.join(format!("{stem}_{ts}.pdf"))
    }

    /// File stem the plot dialog names its output by and, when it picks a
    /// folder (and the drawing has been saved), the finished path that
    /// skips the save dialog. The `PDF` subfolder is created on demand.
    pub(in crate::app) fn plot_output_target(&self) -> (String, Option<std::path::PathBuf>) {
        let tab = &self.tabs[self.active_tab];
        let drawing = self.active_stem("");
        let layout = &tab.scene.current_layout;
        let stem = match self.plot_dialog.output_name {
            1 => layout.clone(),
            2 => format!("{drawing}-{layout}"),
            _ => drawing,
        };
        let stem: String = stem
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
            .collect();
        let dir = tab.current_path.as_deref().and_then(|path| path.parent());
        let folder = match (self.plot_dialog.output_folder, dir) {
            _ if cfg!(target_arch = "wasm32") => None,
            (1, Some(dir)) => Some(dir.to_path_buf()),
            (2, Some(dir)) => Some(dir.join("PDF")),
            _ => None,
        };
        let path = folder
            .filter(|folder| std::fs::create_dir_all(folder).is_ok())
            .map(|folder| {
                let ext = if self.plot_dialog.file_svg {
                    "svg"
                } else {
                    "pdf"
                };
                folder.join(format!("{stem}.{ext}"))
            });
        (stem, path)
    }

    /// File stem of the active drawing for default output names, with an
    /// optional suffix (e.g. "_layouts" for print-all). Falls back to
    /// "drawing" when the tab has never been saved.
    pub(in crate::app) fn active_stem(&self, suffix: &str) -> String {
        self.tabs[self.active_tab]
            .current_path
            .as_deref()
            .and_then(|path| path.file_stem())
            .map(|stem| format!("{}{}", stem.to_string_lossy(), suffix))
            .unwrap_or_else(|| format!("drawing{suffix}"))
    }

    /// Open the full Plot / Print dialog, seeding its state from the active
    /// layout's plot settings and the printers found on the system.
    pub(in crate::app) fn on_plot_dialog_open(&mut self) -> Task<Message> {
        use crate::io::paper_catalog::Orientation;

        let cur = self.tabs[self.active_tab].scene.current_layout.clone();
        let same_context = self
            .plot_prev_context
            .as_ref()
            .is_some_and(|(tab, layout)| *tab == self.active_tab && layout == &cur);

        let previous = self.plot_dialog.clone();

        let scales: Vec<(String, f64)> = self.tabs[self.active_tab]
            .scene
            .scale_list()
            .into_iter()
            .map(|(name, _, factor)| (name, factor))
            .collect();
        let one_to_one = scale_name_for_factor(&scales, 1.0)
            .or_else(|| scales.first().map(|(name, _)| name.clone()))
            .unwrap_or_else(|| "1:1".into());
        let paper_space = self.tabs[self.active_tab].scene.current_layout != "Model";
        let plot_views = self.tabs[self.active_tab]
            .scene
            .document
            .views
            .iter()
            .filter(|view| view.paper_space == paper_space)
            .map(|view| view.name.clone())
            .collect();
        // Keep session-only choices while loading drawing fields from the layout.
        let d = &mut self.plot_dialog;
        match crate::io::print_to_printer::list_printers() {
            Ok(printers) => {
                d.printers = printers;
                d.printers_error = None;
            }
            Err(error) => {
                d.printers = Vec::new();
                d.printers_error = Some(error);
            }
        }
        d.default_printer = crate::io::plot_device::default_printer_name();
        d.plot_styles = crate::io::plot_style::available_table_names();
        d.scales = scales;
        d.plot_views = plot_views;
        if d.scale.eq_ignore_ascii_case("fit") {
            d.fit_to_paper = true;
            d.scale_lw = false;
            d.scale = one_to_one.clone();
        } else if !d.scales.iter().any(|(name, _)| name == &d.scale) {
            d.scale = one_to_one.clone();
        }
        d.paper_space = paper_space;
        d.paper = self.plot_paper.canonical.to_string();
        (d.paper_width_mm, d.paper_height_mm) = self.plot_paper.sheet_mm(self.plot_orientation);
        d.orientation = match self.plot_orientation {
            Orientation::Portrait => "Portrait",
            Orientation::Landscape => "Landscape",
        }
        .to_string();
        d.quality = match d.quality.as_str() {
            "Low" | "Draft" => "Low",
            "High" | "Presentation" | "Maximum" | "Custom" => "High",
            _ => "Normal",
        }
        .into();
        d.shade = match d.shade.as_str() {
            "Wireframe" => "2D Wireframe",
            value if value.starts_with("As displayed") => "As displayed",
            "2D Wireframe"
            | "3D Wireframe"
            | "Hidden Line"
            | "Flat Shaded"
            | "Gouraud Shaded"
            | "Flat Shaded + Edges"
            | "Gouraud Shaded + Edges" => d.shade.as_str(),
            _ => "As displayed",
        }
        .into();
        d.style_name = self
            .active_plot_style
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or_default();
        d.style_missing = false;
        self.plot_dialog.name_input = None;
        self.plot_dialog.name_rename = false;
        if self.plot_dialog.fit_to_paper {
            self.plot_dialog.scale_lw = false;
        }
        self.refresh_page_setups();
        if let Some(error) = self.plot_dialog.printers_error.clone() {
            self.command_line
                .push_warning(crate::tf!("Could not list printers: {error}").as_ref());
        }
        self.plot_prev = Some(previous.clone());
        let layout_entry = format!("*{cur}*");

        if same_context {
            // #1654: reopening Plot for the same drawing/layout must keep the
            // values the user just edited. The layout page setup is the initial
            // seed, not something that should overwrite paper, scale, CTB,
            // orientation, area and output choices on every Ctrl+P.
            //
            // `copy_settings_from` deliberately leaves refreshed runtime lists
            // (printers, media, scales, page setups, etc.) untouched.
            self.plot_dialog.copy_settings_from(&previous);
            self.plot_dialog.selected_setup = previous.selected_setup.clone();

            // A custom ratio such as 1:3 may not exist in the drawing's named
            // scale list. select_page_setup() normally adds it while loading a
            // PlotSettings, but same-context reopening intentionally skips that
            // reload. Keep the restored custom scale available to the picker so
            // the validation below does not collapse it to 1:1.
            if !self.plot_dialog.fit_to_paper
                && !self
                    .plot_dialog
                    .scales
                    .iter()
                    .any(|(name, _)| name == &self.plot_dialog.scale)
            {
                let factor = plot_dialog_scale_factor(&self.plot_dialog);

                if factor.is_finite() && factor > 0.0 {
                    let name = self.plot_dialog.scale.clone();
                    self.plot_dialog.scales.push((name, factor));
                    self.plot_dialog.scales.sort_by(|a, b| a.1.total_cmp(&b.1));
                }
            }
        } else {
            // First open for this tab/layout: seed the editor from its actual
            // page setup. An explicit setup selection later still uses
            // `select_page_setup` and therefore intentionally replaces fields.
            //
            // Keep the chosen output device when a drawing page setup does not
            // name one of its own.
            let kept_printer = self.plot_dialog.printer.clone();
            let kept_to_file = self.plot_dialog.to_file;

            if self.tabs[self.active_tab]
                .scene
                .plot_settings_for(&cur)
                .is_some()
            {
                self.select_page_setup(&layout_entry);
            } else {
                self.select_page_setup(crate::ui::window::plot::SETUP_PREV);
            }

            let setup_named_a_device =
                self.plot_dialog.printer.is_some() || self.plot_dialog.to_file;
            if !setup_named_a_device {
                self.plot_dialog.printer = kept_printer;
                self.plot_dialog.to_file = kept_to_file;
            }
        }

        self.plot_prev_context = Some((self.active_tab, cur.clone()));
        // A persisted plot window (saved with the dialog settings) restores
        // across sessions; a freshly loaded one wins so the two stay in sync.
        if self.plot_window.is_none() {
            self.plot_window = self.plot_dialog.window;
        } else {
            self.plot_dialog.window = self.plot_window;
        }
        if self.plot_dialog.scale.eq_ignore_ascii_case("fit") {
            self.plot_dialog.fit_to_paper = true;
            self.plot_dialog.scale = one_to_one.clone();
        } else if !self
            .plot_dialog
            .scales
            .iter()
            .any(|(name, _)| name == &self.plot_dialog.scale)
        {
            self.plot_dialog.scale = one_to_one;
        }
        if self.plot_dialog.fit_to_paper {
            self.plot_dialog.scale_lw = false;
        }
        // A model-space plot cannot use the paper-only Layout area.
        if cur == "Model" && self.plot_dialog.area == "Layout" {
            self.plot_dialog.area = "Window".into();
            self.normalize_common_plot_dialog();
        }
        self.active_modal = Some(crate::app::ModalKind::Plot);
        // Loading printer capabilities is informational when Plot merely
        // opens. It must not replace the sheet stored in the drawing.
        self.request_printer_media(false)
    }

    /// Keep the dialog's view of the selected printer's media current: use the
    /// session cache when the printer was asked before, otherwise ask it in
    /// the background and clear the stale answer meanwhile. PDF output and
    /// the default printer list the catalogue instead.
    pub(in crate::app) fn request_printer_media(&mut self, reconcile: bool) -> Task<Message> {
        let Some(printer) = (!self.plot_dialog.to_file)
            .then(|| self.plot_dialog.printer.clone())
            .flatten()
        else {
            self.plot_dialog.printer_media = None;
            self.plot_reconcile_printer_media = None;
            return Task::none();
        };

        // PAPERUPDATE is relevant after the user explicitly changes the
        // device/page setup. Merely reopening Plot must leave the drawing's
        // stored paper untouched.
        self.plot_reconcile_printer_media = reconcile.then(|| printer.clone());

        if let Some(caps) = crate::io::plot_device::cached_printer_capabilities(&printer) {
            self.plot_dialog.printer_media = Some(caps);

            if reconcile {
                self.reconcile_sheet_with_printer(&printer);
            }

            return Task::none();
        }

        self.plot_dialog.printer_media = None;
        let name = printer.clone();

        background_task(
            crate::t!("Printer capabilities"),
            move || crate::io::plot_device::printer_capabilities(&printer),
            move |caps| {
                Message::PlotDlg(crate::ui::window::plot::PlotDlgMsg::PrinterMedia(
                    name, caps,
                ))
            },
        )
    }

    /// The inline custom-sheet editor of the Plot dialog. A sheet is added
    /// to (or replaced in) the persisted list under its media name and
    /// selected at once; the list is saved immediately so it survives a
    /// crash the way a plotter configuration would.
    pub(in crate::app) fn on_custom_paper_msg(&mut self, msg: crate::ui::window::plot::CustomPaperMsg) {
        use crate::ui::window::plot::{CustomPaperDraft, CustomPaperMsg as C, MarginSide};
        let d = &mut self.plot_dialog;
        match msg {
            C::Open => {
                let paper = plot_dialog_paper(d);
                let existing = d
                    .custom_papers
                    .iter()
                    .find(|custom| custom.canonical() == paper.canonical);
                let margins = match existing {
                    Some(custom) => custom.margins_mm(),
                    None => plot_dialog_device(d).margins_mm(&paper, &d.custom_papers),
                };
                let mut draft = CustomPaperDraft::from_paper(&paper, margins);
                // A catalogue sheet is only the starting size; the new sheet
                // gets its own name (the source application's generic one when left blank).
                if existing.is_none() {
                    draft.name.clear();
                }
                d.custom_editor = Some(draft);
            }
            C::Cancel => d.custom_editor = None,
            C::Name(value) => Self::edit_custom_draft(d, |draft| draft.name = value),
            C::Width(value) => Self::edit_custom_draft(d, |draft| draft.width = value),
            C::Height(value) => Self::edit_custom_draft(d, |draft| draft.height = value),
            C::Units(value) => Self::edit_custom_draft(d, |draft| draft.inches = value == "Inches"),
            C::Margin(side, value) => Self::edit_custom_draft(d, |draft| {
                let index = match side {
                    MarginSide::Left => 0,
                    MarginSide::Bottom => 1,
                    MarginSide::Right => 2,
                    MarginSide::Top => 3,
                };
                draft.margins[index] = value;
            }),
            C::Add => {
                let Some(draft) = d.custom_editor.as_mut() else {
                    return;
                };
                match draft.build() {
                    Ok(custom) => {
                        let canonical = custom.canonical();
                        d.custom_papers
                            .retain(|existing| existing.canonical() != canonical);
                        d.custom_papers.push(custom);
                        d.custom_editor = None;
                        d.paper = canonical;
                        let (w, h) = plot_dialog_sheet_mm(d);
                        d.paper_width_mm = w;
                        d.paper_height_mm = h;
                        self.save_config();
                    }
                    Err(error) => draft.error = Some(error),
                }
            }
            C::Remove => {
                let selected = plot_dialog_paper(d).canonical.into_owned();
                let before = d.custom_papers.len();
                d.custom_papers
                    .retain(|custom| custom.canonical() != selected);
                if d.custom_papers.len() != before {
                    self.save_config();
                }
            }
        }
    }

    pub(in crate::app) fn edit_custom_draft(
        d: &mut crate::ui::window::plot::PlotDialogState,
        edit: impl FnOnce(&mut crate::ui::window::plot::CustomPaperDraft),
    ) {
        if let Some(draft) = d.custom_editor.as_mut() {
            edit(draft);
            draft.error = None;
        }
    }

    /// Properties… for the selected printer.
    ///
    /// On CUPS platforms (Linux, macOS) the driver's options are listed in an
    /// in-line editor and applied to every job for that printer through
    /// `lp -o`. On Windows the system opens the driver's own printing
    /// preferences in its own process, and the print job honours what the
    /// user saves there. Without a named printer, or where the options
    /// cannot be listed, the platform's printer settings open instead.
    pub(in crate::app) fn on_printer_properties(&mut self) -> Task<Message> {
        // "Default" is a real printer: its preferences sheet is the one to
        // open, not the system's printer list.
        let chosen = self
            .plot_dialog
            .printer
            .clone()
            .or_else(|| self.plot_dialog.default_printer.clone());
        let Some(printer) = chosen.filter(|_| !self.plot_dialog.to_file) else {
            self.open_printer_settings(None);
            return Task::none();
        };
        #[cfg(target_os = "windows")]
        {
            // The driver's sheet is modal: it pumps messages itself until the
            // user closes it. Hosted on our event-loop thread it starves the
            // loop it is sharing, so the window stops painting and a core
            // spins. The system's own preferences process pumps its own
            // messages instead, which leaves ours free.
            self.open_printer_settings(Some(&printer));
            return Task::none();
        }
        #[allow(unreachable_code)]
        {
            self.plot_dialog.printer_editor = Some(crate::ui::window::plot::PrinterOptionsDraft {
                printer: printer.clone(),
                options: None,
                choices: Default::default(),
                error: None,
            });
            let name = printer.clone();
            background_task(
                crate::t!("Printer options"),
                move || crate::io::print_to_printer::printer_options(&printer),
                move |result| {
                    Message::PlotDlg(crate::ui::window::plot::PlotDlgMsg::PrinterOptionsLoaded(
                        name, result,
                    ))
                },
            )
        }
    }

    /// The platform's printer settings surface: the driver's printing
    /// preferences for `printer`, or — without one — a settings panel on
    /// Linux, the printers control panel on Windows, System Settings on macOS.
    pub(in crate::app) fn open_printer_settings(&mut self, printer: Option<&str>) {
        match crate::io::print_to_printer::open_printer_properties(printer) {
            Ok(()) => self
                .command_line
                .push_info(crate::t!("Opened printer properties.").as_ref()),
            Err(error) => self.command_line.push_error(&error),
        }
    }

    pub(in crate::app) fn normalize_common_plot_dialog(&mut self) {
        self.plot_dialog.center = true;
        self.plot_dialog.offset_x = "0.0".into();
        self.plot_dialog.offset_y = "0.0".into();
        self.plot_dialog.fit_to_paper = true;
        self.plot_dialog.scale_lw = false;
    }

    /// Handle one edit / action from the Plot dialog.
    pub(in crate::app) fn on_plot_dlg(
        &mut self,
        msg: crate::ui::window::plot::PlotDlgMsg,
    ) -> Task<Message> {
        use crate::ui::window::plot::{
            PlotDlgMsg as M, PlotFlag, OUT_DEFAULT, OUT_PDF, OUT_SVG, STYLE_NONE,
        };
        match msg {
            M::Close => {
                // Keep the user's last Plot choices available for the next
                // invocation even when the dialog was simply closed.
                self.save_config();
                self.close_active_modal();
                Task::none()
            }
            M::Printer(s) => {
                // Driver choices belong to a printer; the editor closes when
                // the target changes.
                self.plot_dialog.printer_editor = None;
                if s == OUT_PDF {
                    self.plot_dialog.to_file = true;
                    self.plot_dialog.file_svg = false;
                } else if s == OUT_SVG {
                    self.plot_dialog.to_file = true;
                    self.plot_dialog.file_svg = true;
                } else if s == OUT_DEFAULT {
                    self.plot_dialog.to_file = false;
                    self.plot_dialog.file_svg = false;
                    self.plot_dialog.printer = None;
                } else {
                    self.plot_dialog.to_file = false;
                    self.plot_dialog.file_svg = false;
                    self.plot_dialog.printer = Some(s);
                }
                self.save_config();
                // This is an explicit device change, so PAPERUPDATE may
                // reconcile an unsupported sheet when its capabilities arrive.
                self.request_printer_media(true)
            }
            M::PrinterMedia(printer, caps) => {
                let still_current = !self.plot_dialog.to_file
                    && self.plot_dialog.printer.as_deref() == Some(printer.as_str());

                if still_current {
                    self.plot_dialog.printer_media = caps;

                    let reconcile = self
                        .plot_reconcile_printer_media
                        .as_deref()
                        .is_some_and(|pending| pending.eq_ignore_ascii_case(&printer));

                    if reconcile {
                        // Keep the explicit-device context alive while this
                        // printer remains selected. Capability updates for the
                        // same user-selected device must continue to obey
                        // PAPERUPDATE.
                        self.reconcile_sheet_with_printer(&printer);
                    }
                }

                Task::none()
            }
            M::CustomPaper(msg) => {
                self.on_custom_paper_msg(msg);
                Task::none()
            }
            M::PrinterProperties => self.on_printer_properties(),
            M::Import(msg) => self.on_page_setup_import(msg),
            M::PrinterOptionsLoaded(printer, result) => {
                if let Some(draft) = self.plot_dialog.printer_editor.as_mut() {
                    if draft.printer == printer {
                        match result {
                            Ok(options) => {
                                // Start from the remembered choices, then the
                                // driver's defaults.
                                let remembered = self
                                    .plot_dialog
                                    .driver_options
                                    .get(&printer)
                                    .cloned()
                                    .unwrap_or_default();
                                let draft = self.plot_dialog.printer_editor.as_mut().unwrap();
                                draft.choices = options
                                    .iter()
                                    .map(|option| {
                                        let choice = remembered
                                            .get(&option.key)
                                            .filter(|c| {
                                                option.choices.iter().any(|x| &x.keyword == *c)
                                            })
                                            .cloned()
                                            .unwrap_or_else(|| option.default.clone());
                                        (option.key.clone(), choice)
                                    })
                                    .collect();
                                draft.options = Some(options);
                            }
                            Err(error) => draft.error = Some(error),
                        }
                    }
                }
                Task::none()
            }
            M::PrinterOptionSet(key, choice) => {
                if let Some(draft) = self.plot_dialog.printer_editor.as_mut() {
                    draft.choices.insert(key, choice);
                }
                Task::none()
            }
            M::PrinterOptionsReset => {
                if let Some(draft) = self.plot_dialog.printer_editor.as_mut() {
                    if let Some(options) = &draft.options {
                        draft.choices = options
                            .iter()
                            .map(|option| (option.key.clone(), option.default.clone()))
                            .collect();
                    }
                }
                Task::none()
            }
            M::PrinterOptionsApply => {
                if let Some(draft) = self.plot_dialog.printer_editor.take() {
                    let overrides = draft.overrides();
                    if overrides.is_empty() {
                        self.plot_dialog.driver_options.remove(&draft.printer);
                    } else {
                        self.plot_dialog
                            .driver_options
                            .insert(draft.printer.clone(), overrides);
                    }
                    self.save_config();
                }
                Task::none()
            }
            M::PrinterOptionsCancel => {
                self.plot_dialog.printer_editor = None;
                Task::none()
            }
            M::Paper(s) => {
                self.plot_dialog.paper = s;
                let (w, h) = plot_dialog_sheet_mm(&self.plot_dialog);
                self.plot_dialog.paper_width_mm = w;
                self.plot_dialog.paper_height_mm = h;
                Task::none()
            }
            M::Orientation(s) => {
                self.plot_dialog.orientation = s;
                let (w, h) = plot_dialog_sheet_mm(&self.plot_dialog);
                self.plot_dialog.paper_width_mm = w;
                self.plot_dialog.paper_height_mm = h;
                Task::none()
            }
            M::Area(s) => {
                if self.print_all_options && s != "Layout" {
                    return Task::none();
                }
                // A paper Layout page setup commonly carries physical-sheet
                // origin and 1:1 scale. Those values are correct only for
                // Layout; carrying them into Window/Extents/Display moves a
                // large selected area off-page. Enter the common area modes
                // with centered, fit-to-paper defaults.
                if self.plot_dialog.area == "Layout" && s != "Layout" {
                    self.normalize_common_plot_dialog();
                }
                if s == "Layout" && self.plot_dialog.area != "Layout" {
                    self.plot_dialog.center = false;
                    self.plot_dialog.offset_x = "0.0".into();
                    self.plot_dialog.offset_y = "0.0".into();
                    self.plot_dialog.fit_to_paper = false;
                    self.plot_dialog.scale = scale_name_for_factor(&self.plot_dialog.scales, 1.0)
                        .unwrap_or_else(|| "1:1".into());
                }
                self.plot_dialog.area = s;
                Task::none()
            }
            M::Scale(s) => {
                if self.plot_dialog.area != "Layout" {
                    self.plot_dialog.scale = s;
                    self.refresh_custom_scale_fields();
                }
                Task::none()
            }
            M::PaperUnits(s) => {
                use crate::io::paper_catalog::PaperUnits;
                let units = if s == "Inches" {
                    PaperUnits::Inches
                } else {
                    PaperUnits::Millimeters
                };
                let d = &mut self.plot_dialog;
                if units != d.paper_units {
                    // The offsets are physical distances: re-spell them in the
                    // new unit. The scale converts with the unit on its own.
                    let convert = |text: &str| {
                        text.parse::<f64>()
                            .map(|value| units.from_mm(d.paper_units.to_mm(value)))
                            .map(|value| trim_decimals(&format!("{value:.6}")))
                            .unwrap_or_else(|_| text.to_string())
                    };
                    d.offset_x = convert(&d.offset_x);
                    d.offset_y = convert(&d.offset_y);
                    d.paper_units = units;
                    self.refresh_custom_scale_fields();
                }
                Task::none()
            }
            M::CustomScalePaper(s) => {
                self.plot_dialog.custom_scale_paper = s;
                self.adopt_custom_scale();
                Task::none()
            }
            M::CustomScaleDrawing(s) => {
                self.plot_dialog.custom_scale_drawing = s;
                self.adopt_custom_scale();
                Task::none()
            }
            M::Quality(s) => {
                self.plot_dialog.quality = s;
                Task::none()
            }
            M::OutputFolder(s) => {
                self.plot_dialog.output_folder = s.parse().unwrap_or(0);
                Task::none()
            }
            M::OutputName(s) => {
                self.plot_dialog.output_name = s.parse().unwrap_or(0);
                Task::none()
            }
            M::RasterDpi(s) => {
                self.plot_dialog.raster_dpi = s;
                Task::none()
            }
            M::VectorDpi(s) => {
                self.plot_dialog.vector_dpi = s;
                Task::none()
            }
            M::Shade(s) => {
                self.plot_dialog.shade = s;
                Task::none()
            }
            M::Copies(s) => {
                self.plot_dialog.copies = s;
                Task::none()
            }
            M::OffsetX(s) => {
                self.plot_dialog.offset_x = s;
                Task::none()
            }
            M::OffsetY(s) => {
                self.plot_dialog.offset_y = s;
                Task::none()
            }
            M::Flag(f) => {
                let d = &mut self.plot_dialog;
                match f {
                    PlotFlag::Background => d.background = !d.background,
                    PlotFlag::MergeLines => d.merge_lines = !d.merge_lines,
                    PlotFlag::FitToPaper if d.area != "Layout" => {
                        d.fit_to_paper = !d.fit_to_paper;
                        if d.fit_to_paper {
                            d.scale_lw = false;
                        }
                    }
                    PlotFlag::Center if d.area != "Layout" => d.center = !d.center,
                    PlotFlag::ScaleLw if !d.fit_to_paper => d.scale_lw = !d.scale_lw,
                    PlotFlag::PlotStyles if !d.style_name.is_empty() => {
                        d.apply_plot_styles = !d.apply_plot_styles
                    }
                    PlotFlag::DisplayStyles if d.paper_space && !d.style_name.is_empty() => {
                        d.show_plot_styles = !d.show_plot_styles
                    }
                    PlotFlag::UpsideDown => {
                        d.upside_down = !d.upside_down;
                    }
                    PlotFlag::Lineweights => d.lineweights = !d.lineweights,
                    PlotFlag::Transparency => d.transparency = !d.transparency,
                    PlotFlag::PaperspaceLast if d.paper_space => {
                        d.paperspace_last = !d.paperspace_last
                    }
                    PlotFlag::Stamp => d.stamp = !d.stamp,
                    // SVG-only preference; hidden and inert for printer/PDF
                    // output exactly like the SVG file-format gate checks.
                    PlotFlag::NonScalingStroke if d.to_file && d.file_svg => {
                        d.non_scaling_stroke = !d.non_scaling_stroke;
                    }
                    PlotFlag::HidePaperspace => d.hide_paperspace = !d.hide_paperspace,
                    PlotFlag::SaveToLayout => d.save_to_layout = !d.save_to_layout,
                    PlotFlag::OpenWhenDone => d.open_when_done = !d.open_when_done,
                    PlotFlag::TextAsOutlines if d.to_file && d.file_svg => {
                        d.text_as_outlines = !d.text_as_outlines;
                    }
                    PlotFlag::LayerAttributes if d.to_file && d.file_svg => {
                        d.layer_attributes = !d.layer_attributes;
                    }
                    PlotFlag::GeoPdf
                        if (d.to_file || cfg!(target_arch = "wasm32"))
                            && (!d.file_svg || cfg!(target_arch = "wasm32"))
                            && !d.paper_space
                            && d.area != "Layout" =>
                    {
                        d.geopdf = !d.geopdf;
                    }
                    _ => {}
                }
                Task::none()
            }
            M::LoadStyle => Task::done(Message::PlotStyleLoad),
            M::SaveStyle => Task::done(Message::PlotStylePanelSave),
            M::Style(name) => {
                if name == STYLE_NONE {
                    self.active_plot_style = None;
                    self.plot_dialog.style_name.clear();
                    self.plot_dialog.apply_plot_styles = false;
                    self.plot_dialog.show_plot_styles = false;
                    self.plot_dialog.style_missing = false;
                    self.plot_dialog.style_error = None;
                } else {
                    match self.load_layout_plot_style(&name) {
                        Ok(table) => {
                            self.plot_dialog.style_name = table.name.clone();
                            self.plot_dialog.apply_plot_styles = true;
                            self.plot_dialog.style_missing = false;
                            self.plot_dialog.style_error = None;
                            self.report_plot_style_warnings(&table);
                            self.active_plot_style = Some(table);
                        }
                        Err(error) => {
                            self.plot_dialog.style_name = name;
                            self.plot_dialog.style_missing = true;
                            self.command_line.push_error(&error);
                            self.plot_dialog.style_error = Some(error);
                        }
                    }
                }
                Task::none()
            }
            M::PickWindow => {
                if self.print_all_options {
                    return Task::none();
                }
                let i = self.active_tab;
                if self.tabs[i].scene.current_layout != "Model"
                    && self.tabs[i].scene.active_viewport.is_some()
                {
                    // A layout Window is selected in paper coordinates and may
                    // extend anywhere on the canvas, including outside the sheet.
                    // Leave MSPACE so the floating viewport cannot constrain the
                    // two picks to its model view.
                    self.tabs[i].scene.deselect_all();
                    self.tabs[i].scene.active_viewport = None;
                    self.adopt_view_display(i);
                    self.tabs[i].refresh_active_ucs();
                    self.refresh_properties();
                    self.sync_dyn_fields();
                }
                self.close_active_modal();
                Task::done(Message::Command("PLOTWINDOW".into()))
            }
            M::SelectSetup(name) => {
                // Leaving a row mid-rename commits it; otherwise the rename
                // field would follow the selection onto every clicked row.
                if self.plot_dialog.name_rename && self.plot_dialog.selected_setup != name {
                    let _ = self.on_plot_dlg(M::NameCommit);
                }
                // Print All Options edits the page setups represented by the
                // layout rows. With "Save changes to layout" enabled, preserve
                // the setup we are leaving before another one is loaded.
                if self.print_all_options
                    && self.plot_dialog.save_to_layout
                    && self.plot_dialog.selected_setup != name
                {
                    let previous = self.plot_dialog.selected_setup.clone();

                    if is_layout_entry(&previous) {
                        let layout = layout_entry_name(&previous).to_string();
                        self.apply_dialog_to_named_layout(&layout);
                    } else if previous == crate::ui::window::plot::SETUP_PREV {
                        // `<previous>` is a transient snapshot, not an actual
                        // page setup. `plot_prev_context` records which layout
                        // that snapshot came from.
                        //
                        // If the user returns to that same layout, commit the
                        // restored snapshot to it before reloading the row.
                        if let Some((tab, layout)) = self.plot_prev_context.clone() {
                            let target = format!("*{layout}*");

                            if tab == self.active_tab && name == target {
                                self.apply_dialog_to_named_layout(&layout);
                            }
                        }
                    }
                }

                self.select_page_setup(&name);

                if self.print_all_options {
                    self.plot_dialog.paper_space = true;
                    self.plot_dialog.area = "Layout".into();
                }

                // The setup may name a different printer.
                self.request_printer_media(true)
            }
            M::SetCurrent => {
                self.apply_dialog_to_layout();
                self.command_line
                    .push_info(crate::t!("Page setup applied to the layout.").as_ref());
                Task::none()
            }
            M::NewSetup => {
                if self.print_all_options {
                    return Task::none();
                }
                // Create a setup from the current editor values, then start an
                // inline rename so the user can name it.
                let name = self.next_page_setup_name("Setup");
                let ps = self.dialog_to_plotsettings();
                self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                self.tabs[self.active_tab].dirty = true;
                self.plot_dialog.selected_setup = name.clone();
                self.refresh_page_setups();
                self.plot_dialog.name_input = Some(name);
                self.plot_dialog.name_rename = true;
                iced::widget::operation::focus(crate::ui::window::plot::setup_rename_id())
            }
            M::CopySetup => {
                if self.print_all_options {
                    return Task::none();
                }
                // Duplicate the selected entry — a layout OR a named setup —
                // into a new standalone named page setup.
                let sel = self.plot_dialog.selected_setup.clone();
                let scene = &self.tabs[self.active_tab].scene;
                let (src_ps, base) = if is_layout_entry(&sel) {
                    let ln = layout_entry_name(&sel);
                    (scene.plot_settings_for(ln), format!("{ln} copy"))
                } else {
                    (scene.page_setup_get(&sel), format!("{sel} copy"))
                };
                if let Some(ps) = src_ps {
                    let name = self.next_page_setup_name(&base);
                    self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                    self.tabs[self.active_tab].dirty = true;
                    self.plot_dialog.selected_setup = name.clone();
                    self.refresh_page_setups();
                    self.plot_dialog.name_input = Some(name);
                    self.plot_dialog.name_rename = true;
                    return iced::widget::operation::focus(
                        crate::ui::window::plot::setup_rename_id(),
                    );
                }
                Task::none()
            }
            M::RenameStart(name) => {
                if self.print_all_options {
                    return Task::none();
                }
                // Only standalone named setups can be renamed.
                if is_layout_entry(&name) || is_special_entry(&name) {
                    return Task::none();
                }
                self.plot_dialog.selected_setup = name.clone();
                if let Some(ps) = self.tabs[self.active_tab].scene.page_setup_get(&name) {
                    self.load_plotsettings_into_dialog(&ps);
                }
                self.plot_dialog.name_input = Some(name);
                self.plot_dialog.name_rename = true;
                iced::widget::operation::focus(crate::ui::window::plot::setup_rename_id())
            }
            M::DeleteSetup => {
                if self.print_all_options {
                    return Task::none();
                }
                let sel = self.plot_dialog.selected_setup.clone();
                if !sel.is_empty() && !is_layout_entry(&sel) {
                    self.tabs[self.active_tab].scene.page_setup_delete(&sel);
                    self.tabs[self.active_tab].dirty = true;
                    self.plot_dialog.selected_setup.clear();
                    self.plot_dialog.name_input = None;
                    self.plot_dialog.name_rename = false;
                    self.refresh_page_setups();
                }
                Task::none()
            }
            M::NameInput(s) => {
                self.plot_dialog.name_input = Some(s);
                Task::none()
            }
            M::NameCommit => {
                if let Some(name) = self.plot_dialog.name_input.take() {
                    let name = name.trim().to_string();
                    if !name.is_empty() {
                        if self.plot_dialog.name_rename {
                            let old = self.plot_dialog.selected_setup.clone();
                            self.tabs[self.active_tab]
                                .scene
                                .page_setup_rename(&old, &name);
                        } else {
                            let ps = self.dialog_to_plotsettings();
                            self.tabs[self.active_tab].scene.page_setup_save(&name, ps);
                        }
                        self.tabs[self.active_tab].dirty = true;
                        self.plot_dialog.selected_setup = name;
                        self.refresh_page_setups();
                    }
                }
                self.plot_dialog.name_rename = false;
                Task::none()
            }
            M::NameCancel => {
                self.plot_dialog.name_input = None;
                self.plot_dialog.name_rename = false;
                Task::none()
            }
            M::Preview => self.on_plot_dlg_commit(true),
            M::Commit if self.print_all_options => {
                if self.plot_dialog.style_missing && self.plot_dialog.apply_plot_styles {
                    self.command_line.push_error(
                        crate::tf!(
                            "Plot style table '{}' is not loaded.",
                            self.plot_dialog.style_name
                        )
                        .as_ref(),
                    );
                    return Task::none();
                }
                self.plot_dialog.paper_space = true;
                self.plot_dialog.area = "Layout".into();
                self.sync_dialog_plot_runtime();
                self.save_config();
                self.print_all_settings_override = true;
                self.print_all_options = false;
                self.print_all_options_prev = None;
                self.print_all_plot_style_prev = None;
                self.print_all_plot_setup_prev = None;
                if let Some(previous) = self.print_all_plot_window_prev.take() {
                    self.plot_window = previous;
                }
                self.active_modal = Some(crate::app::ModalKind::PrintAll);
                self.reset_modal_geometry();
                Task::none()
            }
            M::Commit => self.on_plot_dlg_commit(false),
        }
    }

    pub(in crate::app) fn refresh_page_setups(&mut self) {
        use crate::ui::window::plot::{SETUP_NONE, SETUP_PREV};
        let scene = &self.tabs[self.active_tab].scene;
        // <none> / <previous>, then layouts (`*name*`), then named setups.
        let mut list = vec![SETUP_NONE.to_string(), SETUP_PREV.to_string()];
        list.extend(scene.layout_names().into_iter().map(|n| format!("*{n}*")));
        list.extend(scene.page_setup_names());
        self.plot_dialog.page_setups = list;
    }

    /// Apply a page-setup list selection to the editor. Handles the pseudo
    /// entries (`<none>` / `<previous>`), layout rows (`*name*`) and named
    /// setups.
    pub(in crate::app) fn select_page_setup(&mut self, name: &str) {
        use crate::ui::window::plot::{SETUP_NONE, SETUP_PREV};
        self.plot_dialog.selected_setup = name.to_string();
        if name == SETUP_NONE {
            self.plot_setup_template = None;
            // No page setup: default geometry + PDF output.
            let is_model = self.tabs[self.active_tab].scene.current_layout == "Model";
            let d = &mut self.plot_dialog;
            d.to_file = true;
            d.file_svg = false;
            let a4 = crate::io::paper_catalog::default_paper();
            d.paper = a4.canonical.to_string();
            d.orientation = "Landscape".into();
            (d.paper_width_mm, d.paper_height_mm) =
                a4.sheet_mm(crate::io::paper_catalog::Orientation::Landscape);
            d.area = if is_model {
                "Window".into()
            } else {
                "Layout".into()
            };
            d.center = true;
            d.offset_x = "0.0".into();
            d.offset_y = "0.0".into();
            d.paper_units = crate::io::paper_catalog::PaperUnits::Millimeters;
            d.upside_down = false;
            d.hide_paperspace = false;
            d.fit_to_paper = is_model;
            d.scale_lw = false;
            d.scale = scale_name_for_factor(&d.scales, 1.0)
                .or_else(|| d.scales.first().map(|(name, _)| name.clone()))
                .unwrap_or_else(|| "1:1".into());
            self.refresh_custom_scale_fields();
        } else if name == SETUP_PREV {
            if let Some(prev) = self.plot_prev.clone() {
                self.plot_dialog.copy_settings_from(&prev);
            }
        } else if is_layout_entry(name) {
            if let Some(ps) = self.tabs[self.active_tab]
                .scene
                .plot_settings_for(layout_entry_name(name))
            {
                self.load_plotsettings_into_dialog(&ps);
            }
        } else if let Some(ps) = self.tabs[self.active_tab].scene.page_setup_get(name) {
            self.load_plotsettings_into_dialog(&ps);
        }
    }

    /// A page-setup name based on `base` that isn't already taken (`base`,
    /// `base 2`, `base 3`, …).
    pub(in crate::app) fn next_page_setup_name(&self, base: &str) -> String {
        let existing = self.tabs[self.active_tab].scene.page_setup_names();
        if !existing.iter().any(|n| n == base) {
            return base.to_string();
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|c| !existing.iter().any(|n| n == c))
            .unwrap_or_else(|| base.to_string())
    }

    /// Build a `PlotSettings` from the current dialog fields (for saving a named
    /// page setup).
    /// The dialog's values as a named page setup.
    pub(in crate::app) fn dialog_to_plotsettings(&self) -> document::objects::PlotSettings {
        self.plot_settings_from_dialog(self.dialog_base_settings())
    }

    /// Load a `PlotSettings` into the dialog editor fields.
    pub(in crate::app) fn load_plotsettings_into_dialog(
        &mut self,
        ps: &document::objects::PlotSettings,
    ) {
        use crate::io::paper_catalog::PaperUnits;
        use document::objects::{PlotType, ShadePlotMode, ShadePlotResolutionLevel};
        self.plot_setup_template = Some(ps.clone());
        if matches!(ps.plot_type, PlotType::Window) && !ps.plot_window.is_empty() {
            self.plot_window = Some((
                ps.plot_window.lower_left_x,
                ps.plot_window.lower_left_y,
                ps.plot_window.upper_right_x,
                ps.plot_window.upper_right_y,
            ));
            self.plot_dialog.window = self.plot_window;
        }
        let mut style_error = None;
        if !ps.current_style_sheet.is_empty() {
            match self.load_layout_plot_style(&ps.current_style_sheet) {
                Ok(table) => {
                    self.report_plot_style_warnings(&table);
                    self.active_plot_style = Some(table);
                }
                Err(error) => {
                    self.active_plot_style = None;
                    style_error = Some(error);
                }
            }
        }
        let active_style_name = self
            .active_plot_style
            .as_ref()
            .map(|table| table.name.clone());
        let style_name = if ps.current_style_sheet.is_empty() {
            String::new()
        } else if active_style_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case(&ps.current_style_sheet))
        {
            active_style_name.clone().unwrap_or_default()
        } else {
            ps.current_style_sheet.clone()
        };
        let style_loaded = !style_name.is_empty()
            && active_style_name
                .as_deref()
                .is_some_and(|name| name.eq_ignore_ascii_case(&style_name));
        // The stored media name wins (it is what the source application wrote); a drawing
        // without a usable name is matched by size, and an unknown size keeps
        // its own dimensions as a custom sheet. The sheet's orientation is the
        // stored width/height order; the rotation below may flip it again.
        let paper =
            crate::io::paper_catalog::from_drawing(&ps.paper_size, ps.paper_width, ps.paper_height);
        let orient = if ps.paper_width >= ps.paper_height {
            "Landscape"
        } else {
            "Portrait"
        }
        .to_string();
        let family_unit_mm = self.tabs[self.active_tab].scene.scale_family_unit_mm();
        let d = &mut self.plot_dialog;
        d.paper = paper.canonical.to_string();
        d.paper_width_mm = ps.paper_width.max(1.0);
        d.paper_height_mm = ps.paper_height.max(1.0);
        d.orientation = orient;
        d.area = match ps.plot_type {
            PlotType::Window => "Window".to_string(),
            PlotType::Layout => "Layout".to_string(),
            PlotType::LastScreenDisplay => "Display".to_string(),
            PlotType::Limits => "Limits".to_string(),
            PlotType::View if !ps.plot_view_name.is_empty() => {
                d.plot_views.push(ps.plot_view_name.clone());
                d.plot_views.sort();
                d.plot_views.dedup();
                format!("View: {}", ps.plot_view_name)
            }
            _ => "Extents".to_string(),
        };
        if !d.paper_space && d.area == "Layout" {
            d.area = "Extents".into();
        }
        d.center = ps.flags.plot_centered;
        d.paper_units = match ps.paper_units {
            document::objects::PlotPaperUnits::Inches => PaperUnits::Inches,
            _ => PaperUnits::Millimeters,
        };
        let format_offset = |origin_mm| {
            let offset = d.paper_units.from_mm(origin_mm);
            if d.area == "Layout" {
                offset.to_string()
            } else {
                trim_decimals(&format!("{offset:.4}"))
            }
        };
        d.offset_x = format_offset(ps.origin_x);
        d.offset_y = format_offset(ps.origin_y);
        d.hide_paperspace = ps.flags.plot_hidden;
        let deg = ps.rotation.to_degrees() as i32;
        if matches!(deg, 90 | 270) {
            d.orientation = if d.orientation == "Portrait" {
                "Landscape"
            } else {
                "Portrait"
            }
            .into();
        }
        d.upside_down = matches!(deg, 180 | 270);
        d.fit_to_paper = d.area != "Layout" && ps.is_scale_to_fit();
        let ratio = ps.scale_numerator / ps.scale_denominator;
        let stored_factor = if ratio.is_finite() && ratio > 0.0 {
            ratio
        } else if ps.flags.use_standard_scale
            && ps.standard_scale_factor.is_finite()
            && ps.standard_scale_factor > 0.0
        {
            ratio_from_stored_scale_factor(ps.standard_scale_factor, ps.paper_units)
        } else {
            ps.scale_type.scale_factor()
        };
        // The file's factor is per the layout's paper unit; the picker's are
        // per the drawing's scale-family unit (see `plot_settings_from_dialog`).
        // A Layout plot is always 1:1 paper, whatever the family unit: a
        // metre drawing showed it as 1:1000 (#2219).
        let target_factor = if d.area == "Layout" {
            1.0
        } else if stored_factor.is_finite() && stored_factor > 0.0 {
            stored_factor * plot_paper_unit_mm(ps.paper_units) / family_unit_mm
        } else {
            1.0
        };
        if !d.fit_to_paper {
            // Layout plots are 1:1 in paper units irrespective of INSUNITS.
            // A legacy metre DWG can have a physical viewport "1:1" scale
            // (1000 mm/model unit) next to an imperial 1:1 scale object.
            // Prefer the explicit paper ratio label over an unrelated
            // architectural name that merely happens to have factor 1.
            d.scale = if d.area == "Layout" && d.scales.iter().any(|(name, _)| name == "1:1") {
                "1:1".to_string()
            } else {
                scale_name_for_factor(&d.scales, target_factor).unwrap_or_else(|| {
                    // A scale the drawing has no name for stays what it is.
                    let name = ratio_scale_name(target_factor);
                    d.scales.push((name.clone(), target_factor));
                    d.scales.sort_by(|a, b| a.1.total_cmp(&b.1));
                    name
                })
            };
        } else if !d.scales.iter().any(|(name, _)| name == &d.scale) {
            // A fitted plot's stored ratio is what the fit computed, not a
            // choice; the picker just needs a valid entry behind it.
            d.scale = scale_name_for_factor(&d.scales, 1.0)
                .or_else(|| d.scales.first().map(|(name, _)| name.clone()))
                .unwrap_or_else(|| "1:1".into());
        }
        d.scale_lw = ps.flags.scale_lineweights && !d.fit_to_paper;
        d.lineweights = ps.flags.print_lineweights;
        d.paperspace_last = ps.flags.draw_viewports_first;
        d.shade = if ps.flags.plot_hidden {
            "Hidden Line"
        } else {
            match ps.shade_plot_mode {
                ShadePlotMode::Wireframe => "2D Wireframe",
                ShadePlotMode::Hidden => "Hidden Line",
                ShadePlotMode::Rendered => "Gouraud Shaded",
                ShadePlotMode::AsDisplayed => "As displayed",
            }
        }
        .into();
        d.quality = match ps.shade_plot_resolution {
            ShadePlotResolutionLevel::Draft => "Low",
            ShadePlotResolutionLevel::Presentation
            | ShadePlotResolutionLevel::Maximum
            | ShadePlotResolutionLevel::Custom => "High",
            _ => "Normal",
        }
        .into();
        match crate::io::plot_device::PlotDevice::from_stored_name(&ps.printer_name).0 {
            crate::io::plot_device::PlotDevice::Pdf => {
                d.to_file = true;
                d.file_svg = false;
                d.printer = None;
            }
            crate::io::plot_device::PlotDevice::Printer(name) => {
                d.to_file = false;
                d.file_svg = false;
                d.printer = Some(name);
            }
            crate::io::plot_device::PlotDevice::None => {
                d.to_file = false;
                d.file_svg = false;
                d.printer = None;
            }
        }
        d.style_name = style_name;
        d.apply_plot_styles = ps.flags.plot_plot_styles;
        d.show_plot_styles = ps.flags.show_plot_styles;
        d.style_missing = !d.style_name.is_empty() && !style_loaded;
        d.style_error = if d.style_missing { style_error } else { None };
        self.refresh_custom_scale_fields();
    }

    /// Tell the user what a plot style table's loader had to tolerate.
    pub(in crate::app) fn report_plot_style_warnings(
        &mut self,
        table: &crate::io::plot_style::PlotStyleTable,
    ) {
        for warning in &table.load_warnings {
            let name = &table.name;
            self.command_line
                .push_warning(&format!("{name}: {warning}"));
        }
    }

    /// Copy transient paper/scale choices out of the dialog without changing
    /// the active layout.
    pub(in crate::app) fn sync_dialog_plot_runtime(&mut self) {
        let d = &self.plot_dialog;
        self.plot_paper = plot_dialog_paper(d);
        self.plot_orientation = plot_dialog_orientation(d);
    }

    pub(in crate::app) fn apply_dialog_to_layout(&mut self) {
        self.sync_dialog_plot_runtime();
        self.apply_plot_page_settings();
    }

    /// Persist the current Plot editor into a specific layout.
    ///
    /// This is needed by Print All Options because selecting `*Layout1*`,
    /// `*Layout2*`, etc. does not switch `scene.current_layout`.
    pub(in crate::app) fn apply_dialog_to_named_layout(&mut self, layout_name: &str) {
        use document::objects::{PlotPaperUnits, PlotSettings};

        let i = self.active_tab;

        let base = self.tabs[i]
            .scene
            .plot_settings_for(layout_name)
            .unwrap_or_else(|| {
                let mut settings = PlotSettings::new("");
                settings.paper_units = PlotPaperUnits::Millimeters;
                settings
            });

        let ps = self.plot_settings_from_dialog(base);

        if self.tabs[i]
            .scene
            .set_layout_plot_settings(layout_name, &ps)
        {
            self.tabs[i].dirty = true;

            if self.tabs[i].scene.current_layout == layout_name {
                self.tabs[i].scene.bump_geometry_no_blocks();
            }
        }
    }

    /// Open a preview PDF, export a PDF, or send the job to the chosen printer.
    pub(in crate::app) fn on_plot_dlg_commit(&mut self, preview: bool) -> Task<Message> {
        let d = self.plot_dialog.clone();
        // The web dialog only offers PDF output, but a page setup loaded from
        // the drawing can still seed `to_file = false` — always export there.
        #[cfg(target_arch = "wasm32")]
        let to_file = true;
        #[cfg(not(target_arch = "wasm32"))]
        let to_file = d.to_file;
        if d.style_missing && d.apply_plot_styles {
            let error = crate::tf!("Plot style table '{}' is not loaded.", d.style_name);
            self.command_line.push_error(error.as_ref());
            // The dialog may be in a window of its own, away from the command
            // line: say there why Export/Preview did nothing.
            self.plot_dialog
                .style_error
                .get_or_insert_with(|| error.into_owned());
            return Task::none();
        }
        if d.geopdf
            && !preview
            && to_file
            && (!d.file_svg || cfg!(target_arch = "wasm32"))
            && d.area != "Layout"
        {
            // Missing CRS or units means the ordinary plain-PDF fallback; only
            // malformed geospatial state or an unsupported frame fails here.
            if let Err(error) = self.geopdf_eligibility() {
                self.command_line.push_error(&error);
                return Task::none();
            }
        }
        // Remember the user's print preferences across sessions.
        self.save_config();
        // A preview never touches the layout; a sent plot writes the dialog
        // into it when "Save changes to layout" is on, so the page setup
        // remembers the last plot the way other applications expect. The
        // runtime paper/scale choices drive this one plot operation either way.
        if !preview && d.save_to_layout {
            self.apply_dialog_to_layout();
        } else {
            self.sync_dialog_plot_runtime();
        }
        self.active_modal = None;
        self.reset_modal_geometry();
        if !preview {
            self.stamp_plot_fields();
        }

        // Extents, Window and Display use one plot path in both spaces. Only
        // Paper-space Layout is special: it uses the physical sheet bounds.
        if d.area != "Layout" {
            let job = match d.area.as_str() {
                "Display" => self.display_plot_job(),
                "Extents" => self.extents_plot_job(),
                "Limits" => self.limits_plot_job(),
                "Window" => self.window_plot_job(),
                area if area.starts_with("View: ") => {
                    self.named_view_plot_job(area.trim_start_matches("View: "))
                }
                _ => None,
            };
            let Some(page) = job else {
                self.command_line
                    .push_error(crate::t!("Plot area is empty. Pick a larger window.").as_ref());
                self.active_modal = Some(crate::app::ModalKind::Plot);
                return Task::none();
            };
            if !preview && to_file && (!d.file_svg || cfg!(target_arch = "wasm32")) {
                if let Err(error) = self.validate_geopdf_page(&page) {
                    self.command_line.push_error(&error);
                    self.active_modal = Some(crate::app::ModalKind::Plot);
                    return Task::none();
                }
            }
            // Preview renders to a temp PDF and opens it — never a save dialog,
            // whatever the output target is.
            if preview {
                let stem = self.active_stem("");
                let work = move || {
                    crate::io::print_to_printer::preview_page_pdf(&stem, &page)
                        .map(|_| crate::t!("Opened plot preview.").into_owned())
                        .map_err(|e| crate::tf!("Preview failed: {e}").into_owned())
                };
                return self.run_plot_work(d.background, true, work);
            }
            if to_file {
                // Tested clipped export (opens a save dialog).
                return Task::done(Message::PlotWindowExport);
            }
            let tmp = crate::io::print_to_printer::temp_pdf_path("print");
            let opts = self.plot_print_options(&d);
            let work = move || {
                crate::io::pdf_export::export_pdf(&page, &tmp)
                    .and_then(|_| crate::io::print_to_printer::print_existing_pdf(&tmp, &opts))
                    .map(|printer| crate::tf!("Sent to printer: {printer}").into_owned())
                    .map_err(|e| crate::tf!("Print failed: {e}").into_owned())
            };
            return self.run_plot_work(true, false, work);
        }

        let page = self.layout_plot_page();

        if preview {
            let stem = self.active_stem("");
            let work = move || {
                crate::io::print_to_printer::preview_page_pdf(&stem, &page)
                    .map(|_| crate::t!("Opened plot preview.").into_owned())
                    .map_err(|e| crate::tf!("Preview failed: {e}").into_owned())
            };
            return self.run_plot_work(d.background, true, work);
        }

        if to_file {
            // Reuse the tested PDF export flow (it opens a save dialog).
            return Task::done(Message::PlotExport);
        }

        let opts = self.plot_print_options(&d);
        self.command_line
            .push_info(crate::t!("Sending to system printer…").as_ref());
        let work = move || {
            iced::futures::executor::block_on(crate::io::print_to_printer::print_wires_with(
                page, opts,
            ))
            .map(|printer| crate::tf!("Sent to printer: {printer}").into_owned())
            .map_err(|error| crate::tf!("Print failed: {error}").into_owned())
        };
        self.run_plot_work(true, false, work)
    }

    /// Build a [`PrintOptions`](crate::io::print_to_printer::PrintOptions) from
    /// the dialog state.
    pub(in crate::app) fn plot_print_options(
        &self,
        d: &crate::ui::window::plot::PlotDialogState,
    ) -> crate::io::print_to_printer::PrintOptions {
        crate::io::print_to_printer::PrintOptions {
            printer: d.printer.clone(),
            copies: d.copies.trim().parse::<u32>().unwrap_or(1).max(1),
            quality: Some(d.quality.clone()),
            driver_options: d
                .printer
                .as_ref()
                .and_then(|printer| d.driver_options.get(printer))
                .map(|options| {
                    options
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub(in crate::app) fn pdf_plot_options(
        d: &crate::ui::window::plot::PlotDialogState,
    ) -> crate::io::pdf_export::PdfPlotOptions {
        crate::io::pdf_export::PdfPlotOptions {
            object_lineweights: d.lineweights,
            scale_lineweights: d.scale_lw && !d.fit_to_paper,
            // PLOTTRANSPARENCYOVERRIDE: 0 never, 2 always, 1 as the dialog says.
            transparency: match d.transparency_override {
                0 => false,
                2 => true,
                _ => d.transparency,
            },
            stamp: d.stamp,
            merge_lines: d.merge_lines,
            quality: crate::io::pdf_export::PlotQuality::from_dialog(&d.quality),
            raster_dpi: crate::io::plot::parse_dpi(&d.raster_dpi),
            vector_dpi: crate::io::plot::parse_dpi(&d.vector_dpi),
            text_as_outlines: d.text_as_outlines,
            non_scaling_stroke: d.non_scaling_stroke,
            layer_attributes: d.layer_attributes,
        }
    }

    pub(in crate::app) fn dialog_plot_style(
        &self,
        d: &crate::ui::window::plot::PlotDialogState,
    ) -> Option<crate::io::plot_style::PlotStyleTable> {
        if d.style_name.is_empty() || d.style_missing || !d.apply_plot_styles {
            return None;
        }

        // Fast path: the selected table is already active.
        if let Some(table) = self
            .active_plot_style
            .as_ref()
            .filter(|table| table.name.eq_ignore_ascii_case(&d.style_name))
        {
            return Some(table.clone());
        }

        // The dialog selection is persisted separately from the parsed CTB
        // object. After reopening a drawing the name may be correct while the
        // in-memory table is still the previous/default one. Plotting must not
        // silently ignore the selected CTB in that state.
        let base = crate::io::drawing_dir(&self.tabs[self.active_tab].scene.document);
        crate::io::plot_style::PlotStyleTable::load_named_at(&d.style_name, base).ok()
    }

    pub(in crate::app) fn load_layout_plot_style(
        &self,
        name: &str,
    ) -> Result<crate::io::plot_style::PlotStyleTable, String> {
        use crate::io::plot_style::{PlotStyleTable, PlotStyleTableExt};

        let tab = &self.tabs[self.active_tab];
        let doc = &tab.scene.document;
        let base = crate::io::drawing_dir(doc);
        let source = tab
            .ifccad_session
            .as_ref()
            .and_then(|session| session.plot_style_source(doc, &tab.scene.current_layout, name));
        let hash = source.and_then(|source| source.sha256.as_deref());
        #[cfg(target_arch = "wasm32")]
        if let Some(hash) = hash {
            use crate::io::plot_style::{parse_external_table, session_table};
            use sha2::{Digest, Sha256};
            let bytes = session_table(&format!("{hash}/{name}"))
                .or_else(|| {
                    session_table(name)
                        .filter(|bytes| format!("{:x}", Sha256::digest(bytes.as_slice())) == hash)
                })
                .ok_or_else(|| {
                    format!(
                        "No supplied plot style table {name:?} matches the SHA-256 in the drawing."
                    )
                })?;
            return parse_external_table(name, !doc.header.plotstyle_mode, &bytes);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            use crate::io::plot_style;

            let picked = hash
                .and_then(|hash| plot_style::picked_table_path(name, hash).ok())
                .filter(|path| path.is_file());
            let local = plot_style::drawing_table_path(name, base);
            if source.is_some() || local.is_some() {
                let candidates: Vec<_> = picked
                    .into_iter()
                    .chain(local)
                    .chain(
                        plot_style::plot_styles_folder()
                            .into_iter()
                            .filter_map(|folder| std::fs::read_dir(folder).ok())
                            .flatten()
                            .filter_map(Result::ok)
                            .map(|entry| entry.path()),
                    )
                    .collect();
                return plot_style::resolve_external_table(
                    name,
                    !doc.header.plotstyle_mode,
                    hash,
                    &candidates,
                );
            }
        }
        PlotStyleTable::load_named_at(name, base)
    }

    pub(in crate::app) fn bind_picked_plot_style(
        &mut self,
        index: usize,
        name: &str,
        picked: &crate::io::plot_style::PickedPlotStyle,
    ) -> Result<(), String> {
        use document::objects::ObjectType;

        let tab = &mut self.tabs[index];
        let doc = &mut tab.scene.document;
        if picked.table.is_stb == doc.header.plotstyle_mode {
            return Err(format!(
                "Plot style table {:?} is incompatible with the drawing mode.",
                picked.source.file_name
            ));
        }
        let (handle, layout) = doc
            .objects
            .iter_mut()
            .find_map(|(handle, object)| {
                let ObjectType::Layout(layout) = object else {
                    return None;
                };
                (layout.name == name).then_some((*handle, layout))
            })
            .ok_or_else(|| format!("Layout {name:?} no longer exists."))?;
        picked.remember()?;
        layout.plot_style_sheet.clone_from(&picked.source.file_name);
        layout.plot_flags.plot_plot_styles = true;
        tab.ifccad_session
            .get_or_insert_with(Default::default)
            .bind_plot_style(handle, picked.source.clone());
        tab.dirty = true;
        tab.edit_revision = tab.edit_revision.wrapping_add(1);
        tab.scene.bump_geometry_no_blocks();
        Ok(())
    }

    pub(in crate::app) fn window_plot_job(&self) -> Option<PdfPageInput> {
        self.area_plot_job(self.plot_window?)
    }

    pub(in crate::app) fn display_plot_job(&self) -> Option<PdfPageInput> {
        self.area_plot_job(self.display_plot_window()?)
    }

    pub(in crate::app) fn limits_plot_job(&self) -> Option<PdfPageInput> {
        let (min, max) = self.tabs[self.active_tab].scene.current_drawing_limits()?;
        self.area_plot_job(self.view_frame_box((min.x, min.y, max.x, max.y)))
    }

    pub(in crate::app) fn named_view_plot_job(&self, name: &str) -> Option<PdfPageInput> {
        let view = self.tabs[self.active_tab]
            .scene
            .document
            .views
            .iter()
            .find(|view| {
                view.name.eq_ignore_ascii_case(name)
                    && view.paper_space
                        == (self.tabs[self.active_tab].scene.current_layout != "Model")
            })?;
        let half_w = view.width.abs() * 0.5;
        let half_h = view.height.abs() * 0.5;
        (half_w > 1e-9 && half_h > 1e-9).then_some(())?;
        let center = crate::scene::named_view_center(view);
        let (cx, cy) = (center.x, center.y);
        self.area_plot_job(self.view_frame_box((
            cx - half_w,
            cy - half_h,
            cx + half_w,
            cy + half_h,
        )))
    }

    pub(in crate::app) fn extents_plot_job(&self) -> Option<PdfPageInput> {
        let scene = &self.tabs[self.active_tab].scene;
        if scene.current_layout == "Model" {
            if let Some((min, max)) = scene.visible_block_bounds(scene.model_space_block_handle()) {
                return self.area_plot_job(self.view_frame_box((min.x, min.y, max.x, max.y)));
            }
            let (min, max) = scene.model_space_extents()?;
            return self.area_plot_job(self.view_frame_box((
                min.x as f64,
                min.y as f64,
                max.x as f64,
                max.y as f64,
            )));
        }
        let content = plot_scene_content(
            scene,
            self.plot_dialog.paperspace_last,
            plot_render_mode_override(&self.plot_dialog),
        );
        let extents = plot_content_extents(&content)?;
        self.area_plot_job(extents)
    }

    /// In-plane turn (radians) of a top-down orthographic model view. Model
    /// plot windows are framed in the view's own axes, so a twisted view
    /// plots the way it is seen.
    pub(in crate::app) fn model_plot_twist(&self) -> f64 {
        let scene = &self.tabs[self.active_tab].scene;
        if scene.current_layout != "Model" {
            return 0.0;
        }
        let camera = scene.camera.borrow();
        if camera.projection != crate::scene::view::camera::Projection::Orthographic {
            return 0.0;
        }
        let eye = camera.rotation * glam::Vec3::Z;
        if eye.x.abs() > 1e-5 || eye.y.abs() > 1e-5 || eye.z <= 0.0 {
            return 0.0;
        }
        let x = camera.rotation * glam::Vec3::X;
        let twist = (x.y as f64).atan2(x.x as f64);
        if twist.abs() < 1e-6 {
            0.0
        } else {
            twist
        }
    }

    /// A world-space box as a window in the view's own axes.
    pub(in crate::app) fn view_frame_box(
        &self,
        (x0, y0, x1, y1): (f64, f64, f64, f64),
    ) -> (f64, f64, f64, f64) {
        let [x0, y0, x1, y1] =
            crate::io::plot::turned_bounds(-self.model_plot_twist(), [x0, y0, x1, y1]);
        (x0, y0, x1, y1)
    }

    /// Current visible rectangle in the active space. The result is deliberately
    /// not clamped to the paper sheet: Display and Window may include the grey
    /// canvas outside the sheet, matching Model-space plotting.
    pub(in crate::app) fn display_plot_window(&self) -> Option<(f64, f64, f64, f64)> {
        let scene = &self.tabs[self.active_tab].scene;
        let (canvas_w, canvas_h) = scene.selection.borrow().view.vp_size;
        let viewport = if scene.current_layout == "Model" {
            scene.active_model_tile_bounds(canvas_w, canvas_h)
        } else {
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: canvas_w,
                height: canvas_h,
            }
        };
        if viewport.width < 1.0 || viewport.height < 1.0 {
            return None;
        }
        let bounds = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: viewport.width,
            height: viewport.height,
        };
        let (sin, cos) = (-self.model_plot_twist()).sin_cos();
        let camera = scene.camera.borrow();
        let mut x0 = f64::INFINITY;
        let mut y0 = f64::INFINITY;
        let mut x1 = f64::NEG_INFINITY;
        let mut y1 = f64::NEG_INFINITY;
        for point in [
            iced::Point::new(0.0, 0.0),
            iced::Point::new(bounds.width, 0.0),
            iced::Point::new(bounds.width, bounds.height),
            iced::Point::new(0.0, bounds.height),
        ] {
            let world = camera.unproject_on_plane(point, bounds, glam::Vec3::Z, glam::DVec3::ZERO);
            let world =
                glam::DVec2::new(cos * world.x - sin * world.y, sin * world.x + cos * world.y);
            x0 = x0.min(world.x);
            y0 = y0.min(world.y);
            x1 = x1.max(world.x);
            y1 = y1.max(world.y);
        }
        (x0.is_finite()
            && y0.is_finite()
            && x1.is_finite()
            && y1.is_finite()
            && x1 - x0 > 1e-6
            && y1 - y0 > 1e-6)
            .then_some((x0, y0, x1, y1))
    }

    /// `Ok(None)` means the drawing has no CRS, or a projected CRS without the
    /// drawing units needed to georeference it. Those drawings still export as
    /// plain PDF. Malformed GEODATA and unsupported plot frames remain errors.
    pub(in crate::app) fn geopdf_eligibility(&self) -> Result<Option<crate::io::crs::CrsDefinition>, String> {
        let scene = &self.tabs[self.active_tab].scene;
        if scene.current_layout != "Model"
            || scene.block_edit_block.is_some()
            || self.plot_dialog.area == "Layout"
        {
            return Err(
                "GeoPDF export requires a root model-space plot frame, not a layout or block editor"
                    .to_string(),
            );
        }
        let camera = scene.camera.borrow();
        if camera.projection != crate::scene::view::camera::Projection::Orthographic {
            return Err("GeoPDF export does not support perspective scenes".to_string());
        }
        let eye = (camera.rotation * glam::Vec3::Z).as_dvec3();
        if eye.x.abs() > 1e-6 || eye.y.abs() > 1e-6 || eye.z <= 0.0 {
            return Err("GeoPDF export requires a top-down orthographic XY scene".to_string());
        }
        drop(camera);
        let Some(crs) = crate::io::crs::document_crs(&scene.document)? else {
            return Ok(None);
        };
        if !crs.is_geographic()
            && crate::app::properties::insunits_to_mm(scene.document.header.insertion_units)
                .is_none()
        {
            return Ok(None);
        }
        Ok(Some(crs))
    }

    pub(in crate::app) fn geopdf_frame_for_window(
        &self,
        window: (f64, f64, f64, f64),
    ) -> Result<Option<crate::io::geopdf_export::GeoPdfFrame>, String> {
        if !self.plot_dialog.geopdf {
            return Ok(None);
        }
        let Some(crs) = self.geopdf_eligibility()? else {
            return Ok(None);
        };
        let units = crate::app::properties::insunits_to_mm(
            self.tabs[self.active_tab]
                .scene
                .document
                .header
                .insertion_units,
        );
        crate::io::geopdf_export::GeoPdfFrame::for_window(&crs, units, window).map(Some)
    }

    /// `area_plot_job` has an established Option-returning API, so its callers
    /// explicitly validate a missing opted-in frame. Missing CRS/units select
    /// the plain-PDF fallback; every other missing frame remains an error.
    pub(in crate::app) fn validate_geopdf_page(&self, page: &PdfPageInput) -> Result<(), String> {
        let scene = &self.tabs[self.active_tab].scene;
        if !self.plot_dialog.geopdf
            || self.plot_dialog.area == "Layout"
            || scene.current_layout != "Model"
            || page.geopdf.is_some()
        {
            return Ok(());
        }
        match self.geopdf_eligibility()? {
            Some(_) => {
                Err("The selected model window cannot be represented as a GeoPDF frame".to_string())
            }
            None => Ok(()),
        }
    }

    /// Render a selected rectangle through one shared Model/Paper path. The
    /// window may lie partly or wholly outside a paper sheet.
    pub(in crate::app) fn area_plot_job(&self, window: (f64, f64, f64, f64)) -> Option<PdfPageInput> {
        use crate::io::paper_catalog::{window_to_sheet, PlotScale};
        let i = self.active_tab;
        let (x0, y0, x1, y1) = window;
        if (x1 - x0) < 1e-6 || (y1 - y0) < 1e-6 {
            return None;
        }
        let (sheet_w, sheet_h) = plot_dialog_sheet_mm(&self.plot_dialog);
        let win_w = (x1 - x0).max(1e-9);
        let win_h = (y1 - y0).max(1e-9);
        let scale_sel = if self.plot_dialog.fit_to_paper {
            PlotScale::Fit
        } else {
            let factor = plot_dialog_scale_factor(&self.plot_dialog);
            if factor > 0.0 {
                let mm_per_unit = if self.tabs[i].scene.current_layout == "Model" {
                    1.0
                } else {
                    1.0 / self.tabs[i].scene.paper_space_unit_factor().max(1e-9)
                };
                PlotScale::Ratio(factor * mm_per_unit)
            } else {
                PlotScale::Fit
            }
        };
        let (scale, centered_x, centered_y) =
            window_to_sheet((win_w, win_h), (sheet_w, sheet_h), scale_sel);
        let (origin_x, origin_y) = self.dialog_plot_origin_mm();
        let target_x = if self.plot_dialog.center {
            centered_x
        } else {
            origin_x
        };
        let target_y = if self.plot_dialog.center {
            centered_y
        } else {
            origin_y
        };
        let scene = &self.tabs[i].scene;
        // The window is in the view's own axes; keep what its turned outline
        // touches in world space.
        let twist = self.model_plot_twist();
        let [wx0, wy0, wx1, wy1] =
            crate::io::plot::turned_bounds(twist, [x0, y0, x1, y1]).map(|v| v as f32);
        let mut content = plot_scene_content(
            scene,
            self.plot_dialog.paperspace_last,
            plot_render_mode_override(&self.plot_dialog),
        );
        let in_window = |wire: &crate::io::pdf_export::PlotWire| {
            wire.name != "paper_printable_area"
                && wire.aabb[0] <= wx1
                && wire.aabb[2] >= wx0
                && wire.aabb[1] <= wy1
                && wire.aabb[3] >= wy0
        };
        content.group_splits.wires = content.wires
            [..content.group_splits.wires.min(content.wires.len())]
            .iter()
            .filter(|wire| in_window(wire))
            .count();
        std::sync::Arc::make_mut(&mut content.wires).retain(in_window);
        let (sin, cos) = twist.sin_cos();
        let (frame_x, frame_y) = ((target_x / scale) - x0, (target_y / scale) - y0);
        let offset_x = cos * frame_x - sin * frame_y;
        let offset_y = sin * frame_x + cos * frame_y;
        let clip = Some((
            (target_x / scale) as f32,
            (target_y / scale) as f32,
            win_w as f32,
            win_h as f32,
        ));
        let rotation = if self.plot_dialog.upside_down { 180 } else { 0 };
        let (page_w, page_h) = match rotation {
            90 | 270 => (sheet_h, sheet_w),
            _ => (sheet_w, sheet_h),
        };
        Some(PdfPageInput {
            content,
            paper_w: page_w,
            paper_h: page_h,
            offset_x,
            offset_y,
            rotation_deg: rotation,
            content_rotation: -twist,
            scale: scale as f32,
            clip,
            options: Self::pdf_plot_options(&self.plot_dialog),
            plot_style: self.dialog_plot_style(&self.plot_dialog),
            geopdf: (twist == 0.0)
                .then(|| self.geopdf_frame_for_window(window).ok().flatten())
                .flatten(),
        })
    }

    pub(in crate::app) fn commit_plotstyle_panel_baseline(&mut self) {
        if self.plotstyle_panel_baseline.is_some() {
            self.plotstyle_panel_baseline = Some(self.active_plot_style.clone());
        }
    }

    pub(in crate::app) fn on_plot_style_panel_apply(&mut self) -> Task<Message> {
        let color_text = self.ps_color_buf.trim();
        let color = if color_text.is_empty() {
            None
        } else {
            let Some(hex) = color_text.strip_prefix('#').filter(|hex| hex.len() == 6) else {
                self.command_line.push_error(
                    crate::t!("Enter a plot color as #RRGGBB, or clear it to use object color.")
                        .as_ref(),
                );
                return Task::none();
            };
            let Ok(rgb) = u32::from_str_radix(hex, 16) else {
                self.command_line.push_error(
                    crate::t!("Enter a plot color as #RRGGBB, or clear it to use object color.")
                        .as_ref(),
                );
                return Task::none();
            };
            Some([
                ((rgb >> 16) & 0xFF) as u8,
                ((rgb >> 8) & 0xFF) as u8,
                (rgb & 0xFF) as u8,
            ])
        };

        let Ok(lineweight) = self.ps_lineweight_buf.trim().parse::<u8>() else {
            self.command_line
                .push_error(crate::t!("Choose a valid plot lineweight.").as_ref());
            return Task::none();
        };
        if lineweight != 255 && lineweight as usize >= crate::io::plot_style::LW_TABLE.len() {
            self.command_line
                .push_error(crate::t!("Choose a valid plot lineweight.").as_ref());
            return Task::none();
        }

        let Ok(screening) = self.ps_screening_buf.trim().parse::<u8>() else {
            self.command_line
                .push_error(crate::t!("Screening must be a whole number from 0 to 100.").as_ref());
            return Task::none();
        };
        if screening > 100 {
            self.command_line
                .push_error(crate::t!("Screening must be a whole number from 0 to 100.").as_ref());
            return Task::none();
        }

        let aci = self.plotstyle_panel_aci as usize;
        if aci == 0 {
            self.command_line
                .push_error(crate::t!("Select a plot style color index from 1 to 255.").as_ref());
            return Task::none();
        }

        let created = self.active_plot_style.is_none();
        if created {
            self.active_plot_style = Some(crate::io::plot_style::PlotStyleTable::identity(
                "Custom.ctb",
            ));
        }
        if let Some(entry) = self
            .active_plot_style
            .as_mut()
            .and_then(|table| table.aci_entries.get_mut(aci))
        {
            entry.color = color;
            entry.lineweight = lineweight;
            entry.screening = screening;
        }
        if created {
            self.command_line
                .push_output(crate::tf!("Created new CTB table, ACI {aci} updated.").as_ref());
        }
        Task::none()
    }

    pub(in crate::app) fn on_plot_style_panel_save(&mut self) -> Task<Message> {
        if self.active_plot_style.is_none() {
            self.command_line.push_error(
                crate::t!("No plot style table loaded. Load or create one first.").as_ref(),
            );
            return Task::none();
        }
        let default_name = self
            .active_plot_style
            .as_ref()
            .map(|t| t.name.clone())
            .unwrap_or("export.ctb".into());
        Task::perform(
            async move {
                let dialog = crate::sys::file_dialog()
                    .set_title(crate::t!("Save Plot Style Table").as_ref())
                    .set_file_name(&default_name)
                    .add_filter(crate::t!("Plot Style Files").as_ref(), &["ctb", "CTB"])
                    .add_filter(crate::t!("All Files").as_ref(), &["*"]);
                #[cfg(not(target_arch = "wasm32"))]
                let dialog = match crate::io::plot_style::ensure_plot_styles_dir() {
                    Ok(dir) => dialog.set_directory(dir),
                    Err(_) => dialog,
                };
                dialog
                    .save_file()
                    .await
                    .map(|h| crate::sys::handle_path(&h))
            },
            Message::PlotStylePanelSavePath,
        )
    }
}
