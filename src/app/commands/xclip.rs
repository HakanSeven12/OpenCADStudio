//! XCLIP's edits on block references, as one undo step.

use super::*;
use crate::modules::insert::xclip::XclipAction;
use crate::scene::pick::xclip as clip;

impl OpenCADStudio {
    /// A block reference's extents in WCS: its block's objects through the
    /// insert transform.
    fn insert_extents(&self, i: usize, insert: codec::Handle) -> ([f64; 2], [f64; 2]) {
        let document = &self.tabs[i].scene.document;
        let Some(codec::EntityType::Insert(ins)) = document.get_entity(insert) else {
            return ([0.0; 2], [0.0; 2]);
        };
        let xform = ins.get_transform();
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for e in document.entities_in_block(&ins.block_name) {
            let (a, b) = crate::scene::convert::tess::entity_bounds_in(document, e);
            for x in [a[0], b[0]] {
                for y in [a[1], b[1]] {
                    let w = xform.apply(codec::types::Vector3::new(x, y, a[2]));
                    lo = [lo[0].min(w.x), lo[1].min(w.y)];
                    hi = [hi[0].max(w.x), hi[1].max(w.y)];
                }
            }
        }
        if lo[0] > hi[0] {
            let p = xform.apply(codec::types::Vector3::new(0.0, 0.0, 0.0));
            return ([p.x, p.y], [p.x, p.y]);
        }
        (lo, hi)
    }

    pub(in crate::app) fn apply_xclip(
        &mut self,
        i: usize,
        inserts: Vec<codec::Handle>,
        action: XclipAction,
    ) {
        self.push_undo_snapshot(i, "XCLIP");
        let extents: Vec<_> = inserts.iter().map(|h| self.insert_extents(i, *h)).collect();
        let current_layer = self.tabs[i].scene.document.header.current_layer_name.clone();
        let scene = &mut self.tabs[i].scene;
        let mut polylines = Vec::new();
        for (n, insert) in inserts.iter().enumerate() {
            let doc = &mut scene.document;
            match &action {
                XclipAction::Enable(on) => {
                    clip::set_insert_clip_enabled(doc, *insert, *on);
                }
                XclipAction::Delete => {
                    clip::remove_insert_clip(doc, *insert);
                }
                XclipAction::Depth { front, back } => {
                    clip::set_insert_clip_depth(doc, *insert, *front, *back);
                }
                XclipAction::New { boundary, inverted } => {
                    clip::set_insert_clip(doc, *insert, boundary, *inverted, extents[n]);
                }
                XclipAction::Polyline => {
                    let Some(spatial) = clip::filter_handle(doc, *insert) else {
                        continue;
                    };
                    let (Some(codec::objects::ObjectType::SpatialFilter(filter)), Some(codec::EntityType::Insert(ins))) =
                        (doc.objects.get(&spatial), doc.get_entity(*insert))
                    else {
                        continue;
                    };
                    let outline = clip::clip_outline_world(doc, filter, &ins.get_transform());
                    let mut pl = codec::entities::LwPolyline::from_points(
                        outline
                            .iter()
                            .map(|p| codec::types::Vector2::new(p[0], p[1]))
                            .collect(),
                    );
                    pl.is_closed = true;
                    if !current_layer.is_empty() {
                        pl.common.layer = current_layer.clone();
                    }
                    polylines.push(codec::EntityType::LwPolyline(pl));
                }
            }
            scene.reseed_derived_caches(*insert);
        }
        for pl in polylines {
            scene.add_entity(pl);
        }
        let changes: Vec<_> = inserts
            .iter()
            .map(|h| (*h, crate::scene::ChangeKind::Modified))
            .collect();
        scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_properties();
    }
}
