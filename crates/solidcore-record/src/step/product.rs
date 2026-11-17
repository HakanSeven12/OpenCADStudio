//! The product structure of a STEP file (ISO 10303-41 product definitions, ISO 10303-43
//! representations, ISO 10303-44 assembly structure, ISO 10303-46 presentation) as plain data:
//! the products and their definitions, the shape representations with their items and contexts,
//! the relationships between representations (with or without a transformation), the assembly
//! occurrences, and the colours the presentation styles give items. Read from a parsed
//! [`Exchange`] by [`read`].
//!
//! Nothing is composed: an occurrence's placement is the two placements of its
//! `ITEM_DEFINED_TRANSFORMATION` (or the operator of its relationship) as the file gives them,
//! and a mapped item keeps its map's origin and its own target. Forming a placement's matrix,
//! inverting it and multiplying placements down the tree are the kernel's, sequenced by ops
//! (#2578, slice 6). Lengths are in millimetres (each times its representation context's length
//! unit, the only arithmetic here); directions keep the ratios the file writes; colours keep the
//! numbers the file writes (not clamped, not converted between linear and sRGB), and a
//! pre-defined colour's RGB comes from a fixed table of ISO 10303-46's names.
//!
//! Cross references are indices into the vectors of [`ProductStructure`], each vector in entity
//! id order. Bodies and the items a style colours are referred to by their entity ids: their
//! topology is read elsewhere (#2578, slice 2d).
//!
//! Input is hostile: every reference the structure follows must exist and be of the expected
//! kind, an assembly or a chain of mapped items may nest at most [`MAX_ASSEMBLY_DEPTH`] deep and
//! may not cycle, and expanding the assembly tree ([`ProductStructure::instances`]) stops with an
//! error past [`MAX_VISITS`] instances. Reading is linear in the file: a sub-assembly shared at
//! every level is read once, and only its expansion grows with its uses. Nothing is
//! pre-allocated from a count in the file, and every error is a message, never a panic.
//!
//! Origin: the data walk of SolidCraft's `crates/kernel/src/step_in/mod.rs` at `a5b71ca2` (MIT
//! OR Apache-2.0): `representation`, `Reader::index`, `find_colour`, `find_transparency`,
//! `placement_matrix` (its reading), `mapped_targets`, `occurrence_name`, `product_name`,
//! `occurrence_matrix` (its reading), `node` (its walk), `part_name`, and the root rule of
//! `StepReading::with_workers`.
//!
//! Deviations from SolidCraft, none with a geometric computation in its place:
//! - Placements are data ([`Frame`]), not matrices: `frame_matrix`, `mat_mul`, `linear_det`,
//!   `placement_inverse` and the operator's determinant check are the kernel's (slice 6); an
//!   operator's scale is refused here only when it is zero or not a number (was: also a singular
//!   operator).
//! - A cycle in the assembly or in mapped items, nesting past [`MAX_ASSEMBLY_DEPTH`] (the same
//!   bound: 33 levels, the root at depth 0) and a reference the structure follows that is missing
//!   or of the wrong kind are refused with a message (were: a warning, the rest left out). An
//!   occurrence of a definition in itself is a cycle (was: skipped).
//! - [`MAX_VISITS`] bounds [`ProductStructure::instances`] (was: the occurrences and mapped
//!   representations the import walked, the rest left out with a warning). Reading itself
//!   visits each definition and representation once. `MAX_BODIES`, `MAX_WINDOW` and
//!   `MAX_CLOSURE_WORK` bound the bodies built and their conversion, which are ops' (slice 6).
//! - Colours are kept per style ([`StyledItem`]): every surface style with its side, fill
//!   colour, rendering colour and transparency, and every curve style's colour (was: the first
//!   colour a bounded search of up to 64 entities reached from the style list, and the first
//!   transparency, as `f32`, clamped to 0..1, opacity `1 - t`). An unknown pre-defined colour
//!   keeps its name and no RGB (was: white). Choosing one colour per body or face is ops'.
//! - Names are kept as the file gives them, escapes decoded by [`super::p21`] (were: trimmed,
//!   with fallbacks: the product's id for an empty name, "Import" for none, an occurrence's id).
//! - A relationship between representations that is a complex instance without a
//!   transformation is kept as a plain link (was: ignored).
//! - A `PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS` is a definition like a
//!   `PRODUCT_DEFINITION`, and a root when no occurrence uses it (was: never a root, so its
//!   bodies came in outside the tree). A `DRAUGHTING_MODEL` a relationship names is a
//!   representation (was: a link to nothing). A `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` of
//!   anything but an occurrence places nothing (as before; its relationship is still a link).
//! - Every map is a `BTreeMap` or a `Vec` in entity id order (were `HashMap`s); doc comments on
//!   every public item. Recursion: the nesting check and the expansion carry their depth and stop
//!   past [`MAX_ASSEMBLY_DEPTH`]; a presentation style assignment is followed one level (its
//!   styles are not assignments again).

use std::collections::{BTreeMap, BTreeSet};

use super::geom::{P3, Placement};
use super::p21::{Entity, Exchange, Param};
use super::read::{MAX_COORD, R, Reader};
use super::units::{self, Context, Units};

/// Deepest assembly nesting, and deepest chain of mapped items, read: the root is at depth 0,
/// so at most 33 levels.
pub const MAX_ASSEMBLY_DEPTH: usize = 32;
/// Most instances [`ProductStructure::instances`] expands (a sub-assembly used twice at every
/// level would otherwise grow exponentially).
pub const MAX_VISITS: usize = 100_000;

/// The product structure of a file. Every vector is in entity id order; cross references are
/// indices into them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProductStructure {
    /// Every `PRODUCT`.
    pub products: Vec<Product>,
    /// Every `PRODUCT_DEFINITION` (and `PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS`).
    pub definitions: Vec<Definition>,
    /// The representations the structure reaches: those a definition's shape uses, those a
    /// relationship or a representation map names, and every representation that lists a body.
    pub representations: Vec<Representation>,
    /// Every `REPRESENTATION_MAP` a mapped item uses.
    pub maps: Vec<RepresentationMap>,
    /// Every relationship between two representations.
    pub links: Vec<Link>,
    /// Every assembly occurrence.
    pub occurrences: Vec<Occurrence>,
    /// The definitions no occurrence uses as its component, in id order.
    pub roots: Vec<usize>,
    /// Every styled item.
    pub styles: Vec<StyledItem>,
}

/// `PRODUCT`.
#[derive(Clone, Debug, PartialEq)]
pub struct Product {
    /// The entity id.
    pub entity: u64,
    /// Its `id` attribute.
    pub id: String,
    /// Its `name`.
    pub name: String,
    /// Its `description` (empty when `$`).
    pub description: String,
}

/// `PRODUCT_DEFINITION` (or `PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS`): a version of a
/// product, with its shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    /// The entity id.
    pub entity: u64,
    /// Its `id` attribute.
    pub id: String,
    /// Its `description` (empty when `$`).
    pub description: String,
    /// The `PRODUCT_DEFINITION_FORMATION` it defines (entity id).
    pub formation: u64,
    /// The product of that formation (index into [`ProductStructure::products`]).
    pub product: usize,
    /// The representations a `SHAPE_DEFINITION_REPRESENTATION` gives its shape (indices into
    /// [`ProductStructure::representations`]), in the order of those entities.
    pub shapes: Vec<usize>,
}

/// A representation (`SHAPE_REPRESENTATION`, `ADVANCED_BREP_SHAPE_REPRESENTATION`, …).
#[derive(Clone, Debug, PartialEq)]
pub struct Representation {
    /// The entity id.
    pub entity: u64,
    /// The entity type (the first record's, for a complex instance).
    pub kind: String,
    /// Its `name`.
    pub name: String,
    /// Its items, in the file's order.
    pub items: Vec<Item>,
    /// Its representation context (entity id), if the file gives one.
    pub context: Option<u64>,
    /// That context's units and uncertainty (millimetres and radians when it names none).
    pub units: Context,
}

/// An item of a representation.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The entity id.
    pub entity: u64,
    /// Its `name` (empty when it has none).
    pub name: String,
    /// What it is.
    pub kind: ItemKind,
}

/// What a representation item is.
#[derive(Clone, Debug, PartialEq)]
pub enum ItemKind {
    /// A body, read by the topology reader from its entity id.
    Body(BodyKind),
    /// `MAPPED_ITEM`: the map's representation, placed.
    Mapped {
        /// The map (index into [`ProductStructure::maps`]).
        map: usize,
        /// Its `mapping_target`, in this representation's units: where the map's origin goes.
        target: Frame,
    },
    /// Anything else (a placement, a curve set, …): its entity type.
    Other(String),
}

/// The kinds of body a representation lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// `MANIFOLD_SOLID_BREP`.
    ManifoldSolidBrep,
    /// `BREP_WITH_VOIDS`.
    BrepWithVoids,
    /// `FACETED_BREP`.
    FacetedBrep,
    /// `SHELL_BASED_SURFACE_MODEL`.
    ShellBasedSurfaceModel,
}

/// `REPRESENTATION_MAP`: a representation that mapped items place.
#[derive(Clone, Debug, PartialEq)]
pub struct RepresentationMap {
    /// The entity id.
    pub entity: u64,
    /// Its `mapping_origin`, in the mapped representation's units.
    pub origin: Frame,
    /// The mapped representation (index into [`ProductStructure::representations`]).
    pub representation: usize,
}

/// A placement as the file gives it.
///
/// ISO 10303-43 places a mapped item (and an `ITEM_DEFINED_TRANSFORMATION`) by the map that
/// takes the origin frame onto the target frame. Forming it is the kernel's (slice 6).
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    /// `AXIS2_PLACEMENT_3D`, or an `AXIS2_PLACEMENT_2D` (the placement in the XY plane it is).
    Placement(Placement),
    /// `CARTESIAN_TRANSFORMATION_OPERATOR_3D` or its `_NON_UNIFORM` subtype.
    Operator(Operator),
}

/// `CARTESIAN_TRANSFORMATION_OPERATOR_3D` (and `_NON_UNIFORM`), as the file writes it.
///
/// ISO 10303-42 makes the operator's axes from the three directions (an absent one from the
/// others and the global axes) and scales them by `scale` (absent: 1), `scale2` and `scale3`
/// (absent: `scale`). That construction is the kernel's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Operator {
    /// `axis1` as written (not normalised).
    pub axis1: Option<P3>,
    /// `axis2` as written.
    pub axis2: Option<P3>,
    /// `axis3` as written.
    pub axis3: Option<P3>,
    /// `local_origin`, in millimetres.
    pub origin: P3,
    /// `scale` as written (never zero).
    pub scale: Option<f64>,
    /// `scale2` of a non-uniform operator.
    pub scale2: Option<f64>,
    /// `scale3` of a non-uniform operator.
    pub scale3: Option<f64>,
}

/// A relationship between two representations: `SHAPE_REPRESENTATION_RELATIONSHIP` (or
/// `REPRESENTATION_RELATIONSHIP`), with or without `REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION`.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    /// The entity id.
    pub entity: u64,
    /// `rep_1` (index into [`ProductStructure::representations`]).
    pub rep_1: usize,
    /// `rep_2`.
    pub rep_2: usize,
    /// The transformation, when the relationship has one.
    pub transformation: Option<Transformation>,
}

/// The `transformation_operator` of a relationship with transformation.
#[derive(Clone, Debug, PartialEq)]
pub enum Transformation {
    /// `ITEM_DEFINED_TRANSFORMATION`: `item_1` (in `rep_1`'s units) is placed onto `item_2` (in
    /// `rep_2`'s units).
    ///
    /// `item_1` is always the item of `rep_1` and `item_2` that of `rep_2`. A file that lists
    /// `transform_item_1` in `rep_2`'s items and `transform_item_2` in `rep_1`'s (a
    /// non-conformant writing some exporters use) has its two items swapped on reading.
    Items {
        /// The entity id.
        entity: u64,
        /// The item that lives in `rep_1`, in `rep_1`'s units.
        item_1: Frame,
        /// The item that lives in `rep_2`, in `rep_2`'s units.
        item_2: Frame,
    },
    /// A placement or an operator, in `rep_2`'s units.
    Frame(Frame),
}

/// The kinds of assembly occurrence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OccurrenceKind {
    /// `NEXT_ASSEMBLY_USAGE_OCCURRENCE`.
    NextAssemblyUsage,
    /// `ASSEMBLY_COMPONENT_USAGE`.
    AssemblyComponentUsage,
    /// `SPECIFIED_HIGHER_USAGE_OCCURRENCE`.
    SpecifiedHigherUsage,
}

/// An assembly occurrence: one use of a definition (the child) in another (the parent).
#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    /// The entity id.
    pub entity: u64,
    /// The entity type.
    pub kind: OccurrenceKind,
    /// Its `id` attribute.
    pub id: String,
    /// Its `name`.
    pub name: String,
    /// Its `description` (empty when `$`).
    pub description: String,
    /// `relating_product_definition` (index into [`ProductStructure::definitions`]).
    pub parent: usize,
    /// `related_product_definition`.
    pub child: usize,
    /// Its `reference_designator`, when it has one.
    pub reference_designator: Option<String>,
    /// The relationships a `CONTEXT_DEPENDENT_SHAPE_REPRESENTATION` gives it (indices into
    /// [`ProductStructure::links`]), in the order of those entities.
    pub placements: Vec<usize>,
}

/// One node of the expanded assembly tree ([`ProductStructure::instances`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instance {
    /// The definition placed (index into [`ProductStructure::definitions`]).
    pub definition: usize,
    /// The occurrence that places it (`None` for a root).
    pub occurrence: Option<usize>,
    /// The instance of its parent (index into the expansion; `None` for a root).
    pub parent: Option<usize>,
}

/// `STYLED_ITEM` (or `OVER_RIDING_STYLED_ITEM`): the styles of one item.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledItem {
    /// The entity id.
    pub entity: u64,
    /// Its `name`.
    pub name: String,
    /// The item it styles (entity id: a body, a face, a mapped item, …).
    pub item: u64,
    /// The styled item it overrides (entity id), for an `OVER_RIDING_STYLED_ITEM`.
    pub overrides: Option<u64>,
    /// Its surface styles (`SURFACE_STYLE_USAGE`), in the file's order.
    pub surface: Vec<SurfaceStyle>,
    /// The colours of its curve styles (`CURVE_STYLE`), in the file's order.
    pub curve: Vec<Colour>,
}

/// Which side of a surface a style is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// `.POSITIVE.`
    Positive,
    /// `.NEGATIVE.`
    Negative,
    /// `.BOTH.` (or anything else).
    Both,
}

/// `SURFACE_STYLE_USAGE` and what its `SURFACE_SIDE_STYLE` says about colour.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceStyle {
    /// The side.
    pub side: Side,
    /// The first `FILL_AREA_STYLE_COLOUR` of its `SURFACE_STYLE_FILL_AREA`.
    pub fill: Option<Colour>,
    /// The colour of its first `SURFACE_STYLE_RENDERING` (or `_WITH_PROPERTIES`).
    pub rendering: Option<Colour>,
    /// The first `SURFACE_STYLE_TRANSPARENT` of that rendering, as written (0 opaque, 1
    /// invisible).
    pub transparency: Option<f64>,
}

/// A colour as the file gives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Colour {
    /// `COLOUR_RGB`: the three numbers as written (ISO 10303-46: each in 0..1; not clamped here).
    Rgb {
        /// Its `name`.
        name: String,
        /// Red, green and blue.
        rgb: [f64; 3],
    },
    /// `DRAUGHTING_PRE_DEFINED_COLOUR`: its name, and the RGB ISO 10303-46 gives that name
    /// (`None` for a name it does not define).
    PreDefined {
        /// Its `name`.
        name: String,
        /// Red, green and blue of the name.
        rgb: Option<[f64; 3]>,
    },
}

/// The RGB of a pre-defined colour name (ISO 10303-46, `pre_defined_colour`).
pub fn pre_defined_rgb(name: &str) -> Option<[f64; 3]> {
    Some(match name {
        "red" => [1.0, 0.0, 0.0],
        "green" => [0.0, 1.0, 0.0],
        "blue" => [0.0, 0.0, 1.0],
        "yellow" => [1.0, 1.0, 0.0],
        "magenta" => [1.0, 0.0, 1.0],
        "cyan" => [0.0, 1.0, 1.0],
        "black" => [0.0, 0.0, 0.0],
        "white" => [1.0, 1.0, 1.0],
        _ => return None,
    })
}

fn entity(ex: &Exchange, id: u64) -> R<&Entity> {
    ex.get(id).ok_or_else(|| format!("missing entity #{id}"))
}

fn reference(p: Option<&Param>, what: &str) -> R<u64> {
    p.and_then(Param::as_ref_id)
        .ok_or_else(|| format!("{what}: expected a reference"))
}

/// A string attribute; `$`, or anything that is not a string, is empty.
fn text(p: Option<&Param>) -> String {
    p.and_then(Param::as_str).unwrap_or("").to_string()
}

fn colour_number(p: Option<&Param>, what: &str) -> R<f64> {
    p.and_then(Param::as_f64)
        .filter(|x| x.abs() < MAX_COORD)
        .ok_or_else(|| format!("{what}: expected a number"))
}

/// The name, items and context of a representation entity: a simple `…REPRESENTATION` or
/// `DRAUGHTING_MODEL` (a representation subtype a relationship may name), or a complex
/// instance's `REPRESENTATION` record, else its first record named `…REPRESENTATION` with three
/// parameters.
fn representation_params(e: &Entity) -> Option<&[Param]> {
    if e.is_complex() {
        e.record("REPRESENTATION").or_else(|| {
            e.records
                .iter()
                .find(|r| r.name.ends_with("REPRESENTATION") && r.params.len() >= 3)
                .map(|r| r.params.as_slice())
        })
    } else if e.name().ends_with("REPRESENTATION") || e.name() == "DRAUGHTING_MODEL" {
        Some(e.params())
    } else {
        None
    }
}

fn body_kind(name: &str) -> Option<BodyKind> {
    match name {
        "MANIFOLD_SOLID_BREP" => Some(BodyKind::ManifoldSolidBrep),
        "BREP_WITH_VOIDS" => Some(BodyKind::BrepWithVoids),
        "FACETED_BREP" => Some(BodyKind::FacetedBrep),
        "SHELL_BASED_SURFACE_MODEL" => Some(BodyKind::ShellBasedSurfaceModel),
        _ => None,
    }
}

fn occurrence_kind(name: &str) -> Option<OccurrenceKind> {
    match name {
        "NEXT_ASSEMBLY_USAGE_OCCURRENCE" => Some(OccurrenceKind::NextAssemblyUsage),
        "ASSEMBLY_COMPONENT_USAGE" => Some(OccurrenceKind::AssemblyComponentUsage),
        "SPECIFIED_HIGHER_USAGE_OCCURRENCE" => Some(OccurrenceKind::SpecifiedHigherUsage),
        _ => None,
    }
}

/// The `rep_1`, `rep_2` and transformation operator (entity ids) of a relationship between
/// representations, or `None` when `e` is not one.
fn link_params(e: &Entity) -> Option<(Option<&Param>, Option<&Param>, Option<&Param>)> {
    if e.is_complex() {
        let rr = e.record("REPRESENTATION_RELATIONSHIP")?;
        let op = e
            .record("REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION")
            .and_then(<[Param]>::first);
        return Some((rr.get(2), rr.get(3), op));
    }
    let p = e.params();
    match e.name() {
        "SHAPE_REPRESENTATION_RELATIONSHIP" | "REPRESENTATION_RELATIONSHIP" => {
            Some((p.get(2), p.get(3), None))
        }
        "REPRESENTATION_RELATIONSHIP_WITH_TRANSFORMATION" => Some((p.get(2), p.get(3), p.get(4))),
        _ => None,
    }
}

/// An optional direction of an operator: `$` or `*` is none.
fn optional_direction(reader: &Reader<'_>, p: Option<&Param>, what: &str) -> R<Option<P3>> {
    match p {
        None | Some(Param::Null | Param::Derived) => Ok(None),
        Some(Param::Ref(id)) => Ok(Some(reader.direction(*id)?)),
        Some(_) => Err(format!("{what}: expected a direction")),
    }
}

/// An optional scale of an operator: `$` or `*` is none; zero is refused.
fn optional_scale(p: Option<&Param>, what: &str) -> R<Option<f64>> {
    match p {
        None | Some(Param::Null | Param::Derived) => Ok(None),
        Some(v) => v
            .as_f64()
            .filter(|x| *x != 0.0 && x.abs() < MAX_COORD)
            .map(Some)
            .ok_or_else(|| format!("{what}: expected a number other than zero")),
    }
}

/// A placement or a transformation operator, in `units`.
fn frame(ex: &Exchange, id: u64, units: Units) -> R<Frame> {
    let e = entity(ex, id)?;
    let reader = Reader::new(ex, units);
    match e.name() {
        "CARTESIAN_TRANSFORMATION_OPERATOR_3D"
        | "CARTESIAN_TRANSFORMATION_OPERATOR_3D_NON_UNIFORM" => {
            let p = e.params();
            let non_uniform = e.name().ends_with("NON_UNIFORM");
            Ok(Frame::Operator(Operator {
                axis1: optional_direction(&reader, p.get(2), "operator axis1")?,
                axis2: optional_direction(&reader, p.get(3), "operator axis2")?,
                origin: reader.point(reference(p.get(4), "operator origin")?)?,
                scale: optional_scale(p.get(5), "operator scale")?,
                axis3: optional_direction(&reader, p.get(6), "operator axis3")?,
                scale2: if non_uniform {
                    optional_scale(p.get(7), "operator scale2")?
                } else {
                    None
                },
                scale3: if non_uniform {
                    optional_scale(p.get(8), "operator scale3")?
                } else {
                    None
                },
            }))
        }
        "AXIS2_PLACEMENT_3D" | "AXIS2_PLACEMENT_2D" => Ok(Frame::Placement(
            reader.frame(id).map_err(|e| format!("#{id}: {e}"))?,
        )),
        n => Err(format!("#{id}: expected a placement, found {n}")),
    }
}

/// A colour entity: `COLOUR_RGB` or `DRAUGHTING_PRE_DEFINED_COLOUR`; any other kind is none.
fn colour(ex: &Exchange, id: u64) -> R<Option<Colour>> {
    let e = entity(ex, id)?;
    let p = e.params();
    match e.name() {
        "COLOUR_RGB" => Ok(Some(Colour::Rgb {
            name: text(p.first()),
            rgb: [
                colour_number(p.get(1), "red")?,
                colour_number(p.get(2), "green")?,
                colour_number(p.get(3), "blue")?,
            ],
        })),
        "DRAUGHTING_PRE_DEFINED_COLOUR" => {
            let name = text(p.first());
            let rgb = pre_defined_rgb(&name);
            Ok(Some(Colour::PreDefined { name, rgb }))
        }
        _ => Ok(None),
    }
}

/// The references of a list parameter.
fn refs<'a>(p: Option<&'a Param>, what: &str) -> R<impl Iterator<Item = u64> + 'a> {
    let list = p
        .and_then(Param::as_list)
        .ok_or_else(|| format!("{what}: expected a list"))?;
    Ok(list.iter().filter_map(Param::as_ref_id))
}

/// A `SURFACE_STYLE_USAGE`'s side and its side style's colours.
fn surface_style(ex: &Exchange, usage: &Entity) -> R<SurfaceStyle> {
    let p = usage.params();
    let side = match p.first().and_then(Param::as_enum) {
        Some("POSITIVE") => Side::Positive,
        Some("NEGATIVE") => Side::Negative,
        _ => Side::Both,
    };
    let mut out = SurfaceStyle {
        side,
        fill: None,
        rendering: None,
        transparency: None,
    };
    let side_style = entity(ex, reference(p.get(1), "surface style usage")?)?;
    if side_style.name() != "SURFACE_SIDE_STYLE" {
        return Ok(out);
    }
    for id in refs(side_style.params().get(1), "surface side style")? {
        let element = entity(ex, id)?;
        let ep = element.params();
        match element.name() {
            "SURFACE_STYLE_FILL_AREA" if out.fill.is_none() => {
                let fill = entity(ex, reference(ep.first(), "surface style fill area")?)?;
                for c in refs(fill.params().get(1), "fill area style")? {
                    let fc = entity(ex, c)?;
                    if fc.name() == "FILL_AREA_STYLE_COLOUR" {
                        out.fill = colour(ex, reference(fc.params().get(1), "fill colour")?)?;
                        if out.fill.is_some() {
                            break;
                        }
                    }
                }
            }
            "SURFACE_STYLE_RENDERING" | "SURFACE_STYLE_RENDERING_WITH_PROPERTIES"
                if out.rendering.is_none() =>
            {
                out.rendering = colour(ex, reference(ep.get(1), "surface rendering colour")?)?;
                if element.name() == "SURFACE_STYLE_RENDERING_WITH_PROPERTIES" {
                    for prop in refs(ep.get(2), "rendering properties")? {
                        let pe = entity(ex, prop)?;
                        if pe.name() == "SURFACE_STYLE_TRANSPARENT" {
                            out.transparency =
                                Some(colour_number(pe.params().first(), "transparency")?);
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

/// The styles a `PRESENTATION_STYLE_ASSIGNMENT` (or a style the styled item lists directly)
/// gives, added to `out`.
fn add_styles(ex: &Exchange, id: u64, out: &mut StyledItem, nested: bool) -> R<()> {
    let e = entity(ex, id)?;
    let p = e.params();
    match e.name() {
        "PRESENTATION_STYLE_ASSIGNMENT" | "PRESENTATION_STYLE_BY_CONTEXT" if !nested => {
            for s in refs(p.first(), "presentation style assignment")? {
                add_styles(ex, s, out, true)?;
            }
        }
        "SURFACE_STYLE_USAGE" => out.surface.push(surface_style(ex, e)?),
        "CURVE_STYLE" => {
            if let Some(c) = p.get(3).and_then(Param::as_ref_id)
                && let Some(c) = colour(ex, c)?
            {
                out.curve.push(c);
            }
        }
        _ => {}
    }
    Ok(())
}

fn styled_item(ex: &Exchange, id: u64, e: &Entity) -> R<Option<StyledItem>> {
    let (p, overrides) = match e.name() {
        "STYLED_ITEM" if !e.is_complex() => (e.params(), None),
        "OVER_RIDING_STYLED_ITEM" | "CONTEXT_DEPENDENT_OVER_RIDING_STYLED_ITEM" => {
            let p = e.params();
            (p, p.get(3).and_then(Param::as_ref_id))
        }
        _ => return Ok(None),
    };
    let item = reference(p.get(2), "styled item")?;
    entity(ex, item)?;
    if let Some(o) = overrides {
        entity(ex, o)?;
    }
    let mut out = StyledItem {
        entity: id,
        name: text(p.first()),
        item,
        overrides,
        surface: Vec::new(),
        curve: Vec::new(),
    };
    for s in refs(p.get(1), "styled item styles")? {
        add_styles(ex, s, &mut out, false)?;
    }
    Ok(Some(out))
}

/// Index of `id` in an id → index map, or an error naming what was expected.
fn index_of(map: &BTreeMap<u64, usize>, id: u64, what: &str) -> R<usize> {
    map.get(&id)
        .copied()
        .ok_or_else(|| format!("#{id}: expected {what}"))
}

/// The height of node `n` of the graph `children` (the longest path below it), checking that no
/// path from a root is longer than [`MAX_ASSEMBLY_DEPTH`] and that the graph has no cycle.
/// Recursion is bounded: `depth` grows by one per level and is refused past the bound. Each node's
/// height is computed once, so the walk is linear in the graph.
fn height(
    children: &[Vec<usize>],
    n: usize,
    depth: usize,
    memo: &mut [Option<usize>],
    on_path: &mut [bool],
    what: &str,
) -> R<usize> {
    if depth > MAX_ASSEMBLY_DEPTH {
        return Err(format!("{what} nested too deeply"));
    }
    if on_path.get(n).copied().unwrap_or(false) {
        return Err(format!("{what} cycle"));
    }
    if let Some(Some(h)) = memo.get(n).copied() {
        if depth + h > MAX_ASSEMBLY_DEPTH {
            return Err(format!("{what} nested too deeply"));
        }
        return Ok(h);
    }
    if let Some(f) = on_path.get_mut(n) {
        *f = true;
    }
    let mut h = 0;
    for &c in children.get(n).map_or(&[][..], Vec::as_slice) {
        h = h.max(height(children, c, depth + 1, memo, on_path, what)? + 1);
    }
    if let Some(f) = on_path.get_mut(n) {
        *f = false;
    }
    if let Some(m) = memo.get_mut(n) {
        *m = Some(h);
    }
    Ok(h)
}

/// Checks the graph `children` (node → nodes below it) for cycles and depth from every node.
fn check_nesting(children: &[Vec<usize>], what: &str) -> R<()> {
    let mut memo = vec![None; children.len()];
    let mut on_path = vec![false; children.len()];
    for n in 0..children.len() {
        height(children, n, 0, &mut memo, &mut on_path, what)?;
    }
    Ok(())
}

/// Reads the product structure of `ex`.
pub fn read(ex: &Exchange) -> R<ProductStructure> {
    let mut out = ProductStructure::default();
    // Shape aspects of definitions: PRODUCT_DEFINITION_SHAPE / PROPERTY_DEFINITION → what it
    // defines.
    let mut pds: BTreeMap<u64, u64> = BTreeMap::new();
    let mut sdrs: Vec<(u64, u64, u64)> = Vec::new(); // (SDR, PDS, rep)
    let mut cdsrs: Vec<(u64, u64, u64)> = Vec::new(); // (CDSR, relationship, PDS)
    let mut occurrence_ids: Vec<u64> = Vec::new();
    let mut definition_ids: Vec<u64> = Vec::new();
    let mut link_ids: Vec<u64> = Vec::new();
    // Representations to read: those listing a body here, more as they are reached.
    let mut pending: Vec<u64> = Vec::new();
    let mut products: BTreeMap<u64, usize> = BTreeMap::new();
    for (&id, e) in &ex.entities {
        let p = e.params();
        match e.name() {
            "PRODUCT" => {
                products.insert(id, out.products.len());
                out.products.push(Product {
                    entity: id,
                    id: text(p.first()),
                    name: text(p.get(1)),
                    description: text(p.get(2)),
                });
            }
            "PRODUCT_DEFINITION" | "PRODUCT_DEFINITION_WITH_ASSOCIATED_DOCUMENTS" => {
                definition_ids.push(id);
            }
            "PRODUCT_DEFINITION_SHAPE" | "PROPERTY_DEFINITION" => {
                pds.insert(id, reference(p.get(2), "property definition")?);
            }
            "SHAPE_DEFINITION_REPRESENTATION" => sdrs.push((
                id,
                reference(p.first(), "shape definition representation")?,
                reference(p.get(1), "shape definition representation")?,
            )),
            "CONTEXT_DEPENDENT_SHAPE_REPRESENTATION" => cdsrs.push((
                id,
                reference(p.first(), "context dependent shape representation")?,
                reference(p.get(1), "context dependent shape representation")?,
            )),
            n if occurrence_kind(n).is_some() => occurrence_ids.push(id),
            _ => {}
        }
        if let Some((r1, r2, _)) = link_params(e) {
            link_ids.push(id);
            pending.push(reference(r1, "relationship rep_1")?);
            pending.push(reference(r2, "relationship rep_2")?);
        }
        if let Some(rp) = representation_params(e)
            && let Some(items) = rp.get(1).and_then(Param::as_list)
            && items.iter().any(|i| {
                i.as_ref_id()
                    .and_then(|i| ex.get(i))
                    .is_some_and(|i| body_kind(i.name()).is_some())
            })
        {
            pending.push(id);
        }
    }
    // Definitions and their products.
    let mut definitions: BTreeMap<u64, usize> = BTreeMap::new();
    for id in definition_ids {
        let p = entity(ex, id)?.params();
        let formation = reference(p.get(2), "product definition formation")?;
        let of_product = reference(
            entity(ex, formation)?.params().get(2),
            "product definition formation product",
        )?;
        definitions.insert(id, out.definitions.len());
        out.definitions.push(Definition {
            entity: id,
            id: text(p.first()),
            description: text(p.get(1)),
            formation,
            product: index_of(&products, of_product, "a PRODUCT")?,
            shapes: Vec::new(),
        });
    }
    // Shapes of definitions: SHAPE_DEFINITION_REPRESENTATION of a PRODUCT_DEFINITION_SHAPE of a
    // PRODUCT_DEFINITION (one of anything else, a shape aspect's, is not the product's shape).
    let mut shapes: Vec<(usize, u64)> = Vec::new();
    for &(sdr, d, rep) in &sdrs {
        let of = *pds
            .get(&d)
            .ok_or_else(|| format!("#{sdr}: #{d} is not a property definition"))?;
        if let Some(&def) = definitions.get(&of) {
            shapes.push((def, rep));
            pending.push(rep);
        }
    }
    // Every representation reached, and the maps its mapped items use.
    let mut reps: BTreeSet<u64> = BTreeSet::new();
    let mut maps: BTreeSet<u64> = BTreeSet::new();
    while let Some(r) = pending.pop() {
        if !reps.insert(r) {
            continue;
        }
        let e = entity(ex, r)?;
        let rp = representation_params(e)
            .ok_or_else(|| format!("#{r}: expected a representation, found {}", e.name()))?;
        for item in refs(rp.get(1), "representation items")? {
            let ie = entity(ex, item)?;
            if ie.name() == "MAPPED_ITEM" {
                let map = reference(ie.params().get(1), "mapped item source")?;
                let me = entity(ex, map)?;
                if me.name() != "REPRESENTATION_MAP" {
                    return Err(format!(
                        "#{item}: mapping source #{map} is a {}, not a REPRESENTATION_MAP",
                        me.name()
                    ));
                }
                maps.insert(map);
                pending.push(reference(me.params().get(1), "mapped representation")?);
            }
        }
    }
    let rep_index: BTreeMap<u64, usize> = reps.iter().enumerate().map(|(i, &r)| (r, i)).collect();
    let map_index: BTreeMap<u64, usize> = maps.iter().enumerate().map(|(i, &m)| (m, i)).collect();
    let mut contexts: BTreeMap<u64, Context> = BTreeMap::new();
    let mut context_of = |ctx: Option<u64>| -> Context {
        ctx.map_or_else(Context::default, |c| {
            *contexts.entry(c).or_insert_with(|| units::context(ex, c))
        })
    };
    // Representations, then maps (whose origins are in their representation's units).
    for &r in &reps {
        let e = entity(ex, r)?;
        let rp =
            representation_params(e).ok_or_else(|| format!("#{r}: expected a representation"))?;
        let context = rp.get(2).and_then(Param::as_ref_id);
        let ctx = context_of(context);
        let mut items = Vec::new();
        for item in refs(rp.get(1), "representation items")? {
            let ie = entity(ex, item)?;
            let kind = if let Some(b) = body_kind(ie.name()) {
                ItemKind::Body(b)
            } else if ie.name() == "MAPPED_ITEM" {
                let ip = ie.params();
                let map = reference(ip.get(1), "mapped item source")?;
                ItemKind::Mapped {
                    map: index_of(&map_index, map, "a REPRESENTATION_MAP")?,
                    target: frame(ex, reference(ip.get(2), "mapped item target")?, ctx.units)
                        .map_err(|e| format!("mapped item #{item}: {e}"))?,
                }
            } else {
                ItemKind::Other(ie.name().to_string())
            };
            items.push(Item {
                entity: item,
                name: text(ie.params().first()),
                kind,
            });
        }
        out.representations.push(Representation {
            entity: r,
            kind: e.name().to_string(),
            name: text(rp.first()),
            items,
            context,
            units: ctx,
        });
    }
    for &m in &maps {
        let p = entity(ex, m)?.params();
        let rep = reference(p.get(1), "mapped representation")?;
        let representation = index_of(&rep_index, rep, "a representation")?;
        let units = out
            .representations
            .get(representation)
            .map_or_else(Units::default, |r| r.units.units);
        out.maps.push(RepresentationMap {
            entity: m,
            origin: frame(ex, reference(p.first(), "mapping origin")?, units)
                .map_err(|e| format!("representation map #{m}: {e}"))?,
            representation,
        });
    }
    for &(def, rep) in &shapes {
        let r = index_of(&rep_index, rep, "a representation")?;
        if let Some(d) = out.definitions.get_mut(def) {
            d.shapes.push(r);
        }
    }
    // Relationships between representations.
    let units_of = |out: &ProductStructure, r: usize| {
        out.representations
            .get(r)
            .map_or_else(Units::default, |r| r.units.units)
    };
    let mut links: BTreeMap<u64, usize> = BTreeMap::new();
    for id in link_ids {
        let e = entity(ex, id)?;
        let Some((r1, r2, op)) = link_params(e) else {
            continue;
        };
        let rep_1 = index_of(
            &rep_index,
            reference(r1, "relationship rep_1")?,
            "a representation",
        )?;
        let rep_2 = index_of(
            &rep_index,
            reference(r2, "relationship rep_2")?,
            "a representation",
        )?;
        let transformation = (|| -> R<Option<Transformation>> {
            let Some(op) = op else { return Ok(None) };
            let op = reference(Some(op), "relationship transformation")?;
            let oe = entity(ex, op)?;
            if oe.name() != "ITEM_DEFINED_TRANSFORMATION" {
                return Ok(Some(Transformation::Frame(frame(
                    ex,
                    op,
                    units_of(&out, rep_2),
                )?)));
            }
            let p = oe.params();
            let written_1 = reference(p.get(2), "transform item 1")?;
            let written_2 = reference(p.get(3), "transform item 2")?;
            // Some exporters write the items the other way round from the representations
            // (`transform_item_1` listed in `rep_2`); where the items live decides.
            let holds = |r: usize, item: u64| {
                out.representations
                    .get(r)
                    .is_some_and(|r| r.items.iter().any(|i| i.entity == item))
            };
            let swapped = (holds(rep_2, written_1) && !holds(rep_1, written_1))
                || (holds(rep_1, written_2) && !holds(rep_2, written_2));
            let (item_1, item_2) = if swapped {
                (written_2, written_1)
            } else {
                (written_1, written_2)
            };
            Ok(Some(Transformation::Items {
                entity: op,
                item_1: frame(ex, item_1, units_of(&out, rep_1))?,
                item_2: frame(ex, item_2, units_of(&out, rep_2))?,
            }))
        })()
        .map_err(|e| format!("relationship #{id}: {e}"))?;
        links.insert(id, out.links.len());
        out.links.push(Link {
            entity: id,
            rep_1,
            rep_2,
            transformation,
        });
    }
    // Assembly occurrences and their placements.
    let mut occurrences: BTreeMap<u64, usize> = BTreeMap::new();
    for id in occurrence_ids {
        let e = entity(ex, id)?;
        let p = e.params();
        let kind = occurrence_kind(e.name()).unwrap_or(OccurrenceKind::NextAssemblyUsage);
        let parent = reference(p.get(3), "occurrence relating product definition")?;
        let child = reference(p.get(4), "occurrence related product definition")?;
        occurrences.insert(id, out.occurrences.len());
        out.occurrences.push(Occurrence {
            entity: id,
            kind,
            id: text(p.first()),
            name: text(p.get(1)),
            description: text(p.get(2)),
            parent: index_of(&definitions, parent, "a PRODUCT_DEFINITION")
                .map_err(|e| format!("occurrence #{id}: {e}"))?,
            child: index_of(&definitions, child, "a PRODUCT_DEFINITION")
                .map_err(|e| format!("occurrence #{id}: {e}"))?,
            reference_designator: p.get(5).and_then(Param::as_str).map(str::to_string),
            placements: Vec::new(),
        });
    }
    for &(cdsr, rel, d) in &cdsrs {
        let link = index_of(&links, rel, "a representation relationship")
            .map_err(|e| format!("#{cdsr}: {e}"))?;
        let of = *pds
            .get(&d)
            .ok_or_else(|| format!("#{cdsr}: #{d} is not a property definition"))?;
        // One for anything but an occurrence (a file may give a part's own shape this way)
        // places nothing; its relationship is a link like any other.
        if let Some(o) = occurrences
            .get(&of)
            .and_then(|&o| out.occurrences.get_mut(o))
        {
            o.placements.push(link);
        }
    }
    // Nesting: the assembly, and chains of mapped items.
    let mut below = vec![Vec::new(); out.definitions.len()];
    let mut used = vec![false; out.definitions.len()];
    for o in &out.occurrences {
        if let Some(b) = below.get_mut(o.parent) {
            b.push(o.child);
        }
        if let Some(u) = used.get_mut(o.child) {
            *u = true;
        }
    }
    check_nesting(&below, "assembly")?;
    let mut mapped = vec![Vec::new(); out.representations.len()];
    for (r, rep) in out.representations.iter().enumerate() {
        for item in &rep.items {
            if let ItemKind::Mapped { map, .. } = item.kind
                && let (Some(m), Some(b)) = (out.maps.get(map), mapped.get_mut(r))
            {
                b.push(m.representation);
            }
        }
    }
    check_nesting(&mapped, "mapped item")?;
    out.roots = used
        .iter()
        .enumerate()
        .filter(|(_, u)| !**u)
        .map(|(i, _)| i)
        .collect();
    // Styles.
    for (&id, e) in &ex.entities {
        if let Some(s) = styled_item(ex, id, e).map_err(|e| format!("styled item #{id}: {e}"))? {
            out.styles.push(s);
        }
    }
    Ok(out)
}

impl ProductStructure {
    /// The assembly tree expanded: every root, then depth first each occurrence of the
    /// definition just placed, in occurrence order. A definition used `n` times is `n` instances.
    /// Refused past [`MAX_VISITS`] instances.
    pub fn instances(&self) -> R<Vec<Instance>> {
        let mut below: Vec<Vec<usize>> = vec![Vec::new(); self.definitions.len()];
        for (i, o) in self.occurrences.iter().enumerate() {
            if let Some(b) = below.get_mut(o.parent) {
                b.push(i);
            }
        }
        let mut out = Vec::new();
        for &root in &self.roots {
            self.expand(&below, root, None, None, 0, &mut out)?;
        }
        Ok(out)
    }

    /// Adds the instance of `definition` and, below it, its occurrences' instances. Recursion is
    /// bounded by `depth` (refused past [`MAX_ASSEMBLY_DEPTH`], which [`read`] has checked).
    fn expand(
        &self,
        below: &[Vec<usize>],
        definition: usize,
        occurrence: Option<usize>,
        parent: Option<usize>,
        depth: usize,
        out: &mut Vec<Instance>,
    ) -> R<()> {
        if depth > MAX_ASSEMBLY_DEPTH {
            return Err("assembly nested too deeply".into());
        }
        if out.len() >= MAX_VISITS {
            return Err(format!("more than {MAX_VISITS} assembly instances"));
        }
        let me = out.len();
        out.push(Instance {
            definition,
            occurrence,
            parent,
        });
        for &o in below.get(definition).map_or(&[][..], Vec::as_slice) {
            if let Some(occ) = self.occurrences.get(o) {
                self.expand(below, occ.child, Some(o), Some(me), depth + 1, out)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
