//! DWF and DGN underlays as the scene draws and snaps them: the display
//! wires of a sheet, and its vectors in the form PDF object snaps read. The
//! sheets themselves are read in `media::vector`.

pub use crate::media::vector::*;

mod wires;

pub(crate) use wires::display_wires;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use crate::media::vector::{layered_item, Key};
use document::entities::UnderlayType;

/// Forget what was read from `path` (its bytes changed), and the vectors
/// drawn from it.
pub fn forget(path: &str) {
    crate::media::vector::forget(path);
    vector_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|k, _| k.0 != path);
}

fn vector_cache() -> &'static Mutex<HashMap<Key, Option<Arc<crate::scene::model::pdf_vector::PageVectors>>>> {
    static CACHE: OnceLock<Mutex<HashMap<Key, Option<Arc<crate::scene::model::pdf_vector::PageVectors>>>>> =
        OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The sheet's geometry as page vectors for object snaps, in sheet units,
/// memoised.
pub fn page_vectors(
    kind: UnderlayType,
    path: &str,
    item: &str,
    hidden: &[String],
) -> Option<Arc<crate::scene::model::pdf_vector::PageVectors>> {
    let key = (path.to_string(), layered_item(item, hidden));
    if let Some(hit) = vector_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return hit.clone();
    }
    let value = sheet_without(kind, path, item, hidden).map(|s| Arc::new(to_page_vectors(&s)));
    vector_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, value.clone());
    value
}

fn to_page_vectors(sheet: &Sheet) -> crate::scene::model::pdf_vector::PageVectors {
    use crate::scene::model::pdf_vector as pv;
    let paths = sheet
        .paths
        .iter()
        .map(|p| pv::PdfPath {
            subpaths: p
                .subpaths
                .iter()
                .map(|sp| pv::SubPath {
                    segments: sp
                        .segments
                        .iter()
                        .map(|s| match s {
                            Segment::Line(a, b) => pv::Segment::Line(*a, *b),
                            Segment::Cubic(a, b, c, d) => pv::Segment::Cubic(*a, *b, *c, *d),
                        })
                        .collect(),
                    closed: sp.closed,
                })
                .collect(),
            stroke: p.stroke.map(|(c, w)| (c, w.max(0.0))),
            fill: p.fill,
            pen: None,
            closes: Vec::new(),
            clip: None,
        })
        .collect();
    pv::PageVectors {
        paths,
        ..Default::default()
    }
}
