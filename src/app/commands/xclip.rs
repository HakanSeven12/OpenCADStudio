//! XCLIP's edits on block references, as one undo step.

use super::*;
use crate::modules::insert::xclip::XclipAction;
use crate::scene::pick::xclip as clip;

impl OpenCADStudio {
    /// A block reference's extents in WCS: its block's objects through the
    /// insert transform — the base-point-aware one, because that is where the
    /// renderer puts the contents the user drew the boundary around.
    fn insert_extents(&self, i: usize, insert: document::Handle) -> ([f64; 2], [f64; 2]) {
        let document = &self.tabs[i].scene.document;
        let Some(document::EntityType::Insert(ins)) = document.get_entity(insert) else {
            return ([0.0; 2], [0.0; 2]);
        };
        let xform = crate::scene::render_graph::insert_transform(document, ins);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for e in document.entities_in_block(&ins.block_name) {
            let (a, b) = crate::scene::convert::tess::entity_bounds_in(document, e);
            for x in [a[0], b[0]] {
                for y in [a[1], b[1]] {
                    let w = xform.apply(document::types::Vector3::new(x, y, a[2]));
                    lo = [lo[0].min(w.x), lo[1].min(w.y)];
                    hi = [hi[0].max(w.x), hi[1].max(w.y)];
                }
            }
        }
        if lo[0] > hi[0] {
            let p = xform.apply(document::types::Vector3::new(0.0, 0.0, 0.0));
            return ([p.x, p.y], [p.x, p.y]);
        }
        (lo, hi)
    }

    pub(in crate::app) fn apply_xclip(
        &mut self,
        i: usize,
        inserts: Vec<document::Handle>,
        action: XclipAction,
    ) {
        if let XclipAction::Depth { rejected: true, .. } = action {
            self.command_line
                .push_error(crate::modules::insert::xclip::DEPTH_REJECTED);
        }
        self.push_undo_snapshot(i, "XCLIP");
        let extents: Vec<_> = inserts.iter().map(|h| self.insert_extents(i, *h)).collect();
        let current_layer = self.tabs[i]
            .scene
            .document
            .header
            .current_layer_name
            .clone();
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
                XclipAction::Depth { front, back, .. } => {
                    clip::set_insert_clip_depth(doc, *insert, *front, *back);
                }
                XclipAction::New { boundary, inverted } => {
                    clip::set_insert_clip(doc, *insert, boundary, *inverted, extents[n]);
                }
                XclipAction::Polyline => {
                    let Some(spatial) = clip::filter_handle(doc, *insert) else {
                        continue;
                    };
                    let (
                        Some(document::objects::ObjectType::SpatialFilter(filter)),
                        Some(document::EntityType::Insert(ins)),
                    ) = (doc.objects.get(&spatial), doc.get_entity(*insert))
                    else {
                        continue;
                    };
                    // The same base-point-aware transform the spatial filter
                    // was stored with, so the generated polyline lands on the
                    // boundary the user drew (the two cancel exactly).
                    let outline = clip::clip_outline_world(
                        doc,
                        filter,
                        &crate::scene::render_graph::insert_transform(doc, ins),
                    );
                    let mut pl = document::entities::LwPolyline::from_points(
                        outline
                            .iter()
                            .map(|p| document::types::Vector2::new(p[0], p[1]))
                            .collect(),
                    );
                    pl.is_closed = true;
                    if !current_layer.is_empty() {
                        pl.common.layer = current_layer.clone();
                    }
                    polylines.push(document::EntityType::LwPolyline(pl));
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
