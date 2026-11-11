//! Writing kernel topology back into an ACIS document.
//!
//! Clean provenance keeps original records; dirty and synthesized nodes are
//! written from kernel geometry.

use document::entities::acis::types::{
    SatDocument, SatEdge, SatFace, SatPointer, SatRecord, SatToken, SatVertex,
};
use kernel::brep::{Body, Curve3, Provenance, Surface};

/// Why a body could not be written back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unwritable {
    /// A node was edited but names a source record that is no longer there.
    MissingSource(u32),
    /// A surface was edited and has no record form here.
    Surface,
    /// A curve was edited and has no record form here.
    Curve,
    /// The pointer walk landed on a record of the wrong kind, which would
    /// have been overwritten with geometry that parses and is not what was
    /// there.
    WrongRecord {
        /// What the record actually is.
        found: String,
        /// What was about to be written into it.
        expected: String,
    },
}

/// Writes a body back into the document it came from.
///
/// Records the body never touched are left alone. The document is modified in
/// place, so a caller holding one with several bodies lowers each in turn and
/// keeps everything else.
pub fn lower(body: &Body, document: &mut SatDocument) -> Result<usize, Unwritable> {
    let mut written = 0;

    for (_, face) in body.faces.iter() {
        let Some(source) = dirty_source(&face.provenance) else {
            continue;
        };
        let surface = body
            .surfaces
            .get(face.surface)
            .ok_or(Unwritable::Surface)?;
        let tokens = surface_tokens(surface).ok_or(Unwritable::Surface)?;
        // The surface lives in the record the face *points at*, not in the
        // face record, which holds nothing but pointers. Writing geometry
        // into the face itself replaces those and takes the topology with it.
        let at = geometry_slot(document, source, |record| {
            SatFace::from_record(record).map(|view| view.surface())
        })?;
        write_tokens(document, at, tokens, surface_type(surface))?;
        written += 1;
    }

    for (_, edge) in body.edges.iter() {
        let Some(source) = dirty_source(&edge.provenance) else {
            continue;
        };
        let curve = body.curves.get(edge.curve).ok_or(Unwritable::Curve)?;
        let tokens = curve_tokens(curve).ok_or(Unwritable::Curve)?;
        let at = geometry_slot(document, source, |record| {
            SatEdge::from_record(record).map(|view| view.curve())
        })?;
        write_tokens(document, at, tokens, curve_type(curve))?;
        written += 1;
    }

    for (_, vertex) in body.vertices.iter() {
        let Some(source) = dirty_source(&vertex.provenance) else {
            continue;
        };
        let at = geometry_slot(document, source, |record| {
            SatVertex::from_record(record).map(|view| view.point())
        })?;
        write_tokens(document, at, point_tokens(vertex.point), "point")?;
        written += 1;
    }

    Ok(written)
}

/// The record index a node must be written through, or `None` if it is clean.
///
/// A synthesized node has no record to write through at all. Adding one means
/// giving it a place in the pointer graph — which record names it, and which
/// it names — and that is the part a boolean's output needs and this does not
/// yet do. Saying so beats writing a geometry record nothing points at.
fn dirty_source(provenance: &Provenance) -> Option<u32> {
    match provenance {
        Provenance::Clean(_) => None,
        Provenance::Dirty(source) => Some(source.index()),
        Provenance::Synthesized => None,
    }
}

/// Where in the document's array the geometry record for a node sits.
///
/// Two lookups, because ACIS record *ids* and their positions in the array
/// are different numbers that happen to agree in a freshly written file and
/// stop agreeing the moment anything is inserted.
fn geometry_slot(
    document: &SatDocument,
    source: u32,
    pointer_of: impl Fn(&SatRecord) -> Option<SatPointer>,
) -> Result<usize, Unwritable> {
    let node = slot_of(document, source as i32).ok_or(Unwritable::MissingSource(source))?;
    let record = document
        .record(node)
        .ok_or(Unwritable::MissingSource(source))?;
    let pointer = pointer_of(record).ok_or(Unwritable::MissingSource(source))?;
    let id = i32::try_from(pointer.index().ok_or(Unwritable::MissingSource(source))?)
        .map_err(|_| Unwritable::MissingSource(source))?;
    slot_of(document, id).ok_or(Unwritable::MissingSource(source))
}

/// The array position of the record with this ACIS id.
fn slot_of(document: &SatDocument, id: i32) -> Option<usize> {
    (0..document.record_count()).find(|slot| {
        document
            .record(*slot)
            .is_some_and(|record| record.index == id)
    })
}

/// Replaces a geometry record's tokens, checking it is the kind expected.
///
/// The check is what keeps a mistake in the pointer walk from turning a
/// vertex record into a plane: it would still parse, and the failure would
/// surface as a solid with a wall in the wrong place.
fn write_tokens(
    document: &mut SatDocument,
    slot: usize,
    tokens: Vec<SatToken>,
    expected: &str,
) -> Result<(), Unwritable> {
    let record = document
        .record_mut(slot)
        .ok_or(Unwritable::MissingSource(slot as u32))?;
    if record.entity_type != expected {
        return Err(Unwritable::WrongRecord {
            found: record.entity_type.clone(),
            expected: expected.to_string(),
        });
    }
    record.tokens = tokens;
    Ok(())
}

/// How many of a body's nodes a save would rewrite.
///
/// The number a caller wants before committing: zero means the document comes
/// back byte for byte.
pub fn pending(body: &Body) -> usize {
    let dirty = |provenance: &Provenance| !provenance.is_reusable();
    body.faces.iter().filter(|(_, f)| dirty(&f.provenance)).count()
        + body.edges.iter().filter(|(_, e)| dirty(&e.provenance)).count()
        + body
            .vertices
            .iter()
            .filter(|(_, v)| dirty(&v.provenance))
            .count()
}

/// The record kind and tokens for a surface, from the one place that knows
/// the layouts.
///
/// Kept in `append` rather than copied here. The two had already drifted: a
/// cone's half-angle sits at token thirteen, after two continuation tokens,
/// and this file's own copy put it at eleven — which reads back as a
/// cylinder whatever the cone's real angle was.
fn surface_type(surface: &Surface) -> &'static str {
    super::append::surface_record(surface)
        .map(|(kind, _)| kind)
        .unwrap_or("plane-surface")
}

fn surface_tokens(surface: &Surface) -> Option<Vec<SatToken>> {
    let (_, tokens) = super::append::surface_record(surface)?;
    Some(lead(tokens))
}

fn curve_type(curve: &Curve3) -> &'static str {
    super::append::curve_record(curve)
        .map(|(kind, _)| kind)
        .unwrap_or("straight-curve")
}

fn curve_tokens(curve: &Curve3) -> Option<Vec<SatToken>> {
    let (_, tokens) = super::append::curve_record(curve)?;
    Some(lead(tokens))
}

fn point_tokens(point: [f64; 3]) -> Vec<SatToken> {
    lead(vec![super::append::position(point)])
}

/// Every record's first token is its attribute pointer; the geometry
/// accessors index from one.
fn lead(mut tokens: Vec<SatToken>) -> Vec<SatToken> {
    let mut all = vec![super::append::null()];
    all.append(&mut tokens);
    all
}
