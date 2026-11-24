//! GEOMETRYCLEANUPOPEN, GEOMETRYCLEANUPCLOSE and the Geometry Cleanup
//! palette: the scan of the selected objects and the repair of one error.

use crate::app::{Message, OpenCADStudio};
use crate::modules::manage::geometry_cleanup as cleanup;
use crate::ui::dock::PanelId;
use crate::ui::window::geometry_cleanup::{rows, Finding, Fix, GeometryCleanupMsg, Scan};
use document::{EntityType, Handle};
use iced::Task;
use ocs_geom::geom2d::cleanup::{end_errors, proposed_tolerance};
use ocs_geom::geom2d::{Curve, Tolerance};

/// Ends nearer than this meet: they are not an error.
const WELD: f64 = 1e-6;

impl OpenCADStudio {
    pub(super) fn dispatch_geometry_cleanup(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        let cmd = cmd.trim();
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        match verb.to_ascii_uppercase().as_str() {
            "GEOMETRYCLEANUPOPEN" => {
                self.set_geometry_cleanup(true);
                // Objects selected before the command are scanned at once.
                let selected = self.tabs[i].scene.selected_handles_in_order();
                if !selected.is_empty() {
                    self.tabs[i].scene.deselect_all();
                    self.geometry_cleanup_scan(i, selected, None, true);
                }
                Some(Task::none())
            }
            "GEOMETRYCLEANUPCLOSE" => {
                self.set_geometry_cleanup(false);
                Some(Task::none())
            }
            "_GEOMETRYCLEANUPSCAN" => {
                let objects = rest
                    .split(',')
                    .filter_map(|text| u64::from_str_radix(text.trim(), 16).ok())
                    .map(Handle::new)
                    .collect();
                self.tabs[i].scene.deselect_all();
                self.geometry_cleanup_scan(i, objects, None, true);
                Some(Task::none())
            }
            _ => None,
        }
    }

    /// Open or close the Geometry Cleanup palette (docked, expanded).
    pub(in crate::app) fn set_geometry_cleanup(&mut self, open: bool) {
        let id = PanelId::GeometryCleanup;
        self.geometry_cleanup.show = open;
        if open {
            self.dock_open_at_default(id);
        } else {
            self.dock_unpeek(id);
        }
    }

    /// The curves of the objects that Geometry Cleanup may read, with
    /// their handles.
    fn geometry_cleanup_curves(&self, i: usize, objects: &[Handle]) -> Vec<(Handle, Curve)> {
        let document = &self.tabs[i].scene.document;
        objects
            .iter()
            .filter(|handle| cleanup::is_candidate(document, **handle))
            .filter_map(|handle| {
                Some((
                    *handle,
                    cleanup::cleanup_curve(document.get_entity(*handle)?)?,
                ))
            })
            .collect()
    }

    /// The errors among `objects` now, within `tolerance`.
    fn geometry_cleanup_findings(
        &self,
        i: usize,
        objects: &[Handle],
        tolerance: f64,
    ) -> Vec<Finding> {
        let kept = self.geometry_cleanup_curves(i, objects);
        let curves: Vec<Curve> = kept.iter().map(|(_, curve)| curve.clone()).collect();
        end_errors(&curves, tolerance, Tolerance::new(WELD))
            .into_iter()
            .map(|error| Finding {
                kind: error.kind,
                handle: kept[error.end.curve].0,
                at_start: error.end.at_start,
                location: error.location,
                target: error.target,
                other: error
                    .other
                    .map(|other| (kept[other.curve].0, other.at_start)),
                corner: error.corner,
                deviation: error.deviation,
            })
            .collect()
    }

    /// Scan `objects` and show what was found. A new selection gets the
    /// tolerance proposed for the drawing and a report on the command line;
    /// a rescan keeps the tolerance, and the picked error if it is still
    /// there.
    fn geometry_cleanup_scan(
        &mut self,
        i: usize,
        objects: Vec<Handle>,
        tolerance: Option<f64>,
        report: bool,
    ) {
        let rescan = tolerance.is_some();
        let tolerance = tolerance.unwrap_or_else(|| {
            let document = &self.tabs[i].scene.document;
            let drawing: Vec<Curve> = document
                .entities()
                .filter(|entity| cleanup::is_candidate(document, entity.common().handle))
                .filter_map(cleanup::cleanup_curve)
                .collect();
            proposed_tolerance(&drawing)
        });
        let findings = self.geometry_cleanup_findings(i, &objects, tolerance);
        if report {
            self.command_line.push_output(&crate::tf!(
                "Geometry Cleanup: {} error(s) found.",
                findings.len()
            ));
        }
        let picked = self
            .geometry_cleanup_scan_here(i)
            .zip(self.geometry_cleanup.picked)
            .and_then(|(scan, index)| scan.findings.get(index).copied())
            .filter(|_| rescan)
            .and_then(|old| findings.iter().position(|f| f.same_error(&old)));
        self.geometry_cleanup.scan = Some(Scan {
            tab: self.tabs[i].id,
            epoch: self.tabs[i].scene.geometry_epoch,
            objects,
            tolerance,
            findings,
        });
        self.geometry_cleanup.picked = picked;
    }

    /// After the drawing changes, the open palette scans its objects again:
    /// an edit, an undo or a repair keeps the list true. Not while a command
    /// or a grip edit runs, which change the drawing on each move; the scan
    /// comes when they end.
    pub(in crate::app) fn refresh_geometry_cleanup_if_stale(&mut self) {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if !self.geometry_cleanup.show || tab.active_cmd.is_some() || tab.active_grip.is_some() {
            return;
        }
        let Some(scan) = self.geometry_cleanup_scan_here(i) else {
            return;
        };
        if scan.epoch == self.tabs[i].scene.geometry_epoch {
            return;
        }
        let (objects, tolerance) = (scan.objects.clone(), scan.tolerance);
        self.geometry_cleanup_scan(i, objects, Some(tolerance), false);
    }

    /// The scan of the active tab, if the palette shows one.
    fn geometry_cleanup_scan_here(&self, i: usize) -> Option<&Scan> {
        self.geometry_cleanup
            .scan
            .as_ref()
            .filter(|scan| scan.tab == self.tabs[i].id)
    }

    pub(in crate::app) fn on_geometry_cleanup(&mut self, m: GeometryCleanupMsg) -> Task<Message> {
        let i = self.active_tab;
        match m {
            GeometryCleanupMsg::SelectObjects => {
                self.tabs[i].scene.deselect_all();
                let command = cleanup::SelectObjects;
                self.reset_command_start_state(i);
                self.command_line
                    .push_info(&crate::command::CadCommand::prompt(&command));
                self.tabs[i].active_cmd = Some(Box::new(command));
                self.push_ucs_to_cmd(i);
                self.sync_dyn_fields();
                return self.focus_cmd_input();
            }
            GeometryCleanupMsg::Rescan => {
                if let Some(scan) = self.geometry_cleanup_scan_here(i) {
                    let (objects, tolerance) = (scan.objects.clone(), scan.tolerance);
                    self.geometry_cleanup_scan(i, objects, Some(tolerance), true);
                }
            }
            GeometryCleanupMsg::Pick(index) => {
                let Some(finding) = self.geometry_cleanup_scan_here(i).and_then(|scan| {
                    scan.findings
                        .get(index)
                        .copied()
                        .map(|f| (f, scan.tolerance))
                }) else {
                    return Task::none();
                };
                self.geometry_cleanup.picked = Some(index);
                self.geometry_cleanup_show(i, finding.0, finding.1);
            }
            GeometryCleanupMsg::Fix(index, fix) => {
                let Some(scan) = self.geometry_cleanup_scan_here(i) else {
                    return Task::none();
                };
                let Some(picked) = scan.findings.get(index).copied() else {
                    return Task::none();
                };
                // The rows below the fixed one, then those above it nearest
                // first: where the pick goes after the repair.
                let rows = rows(&self.geometry_cleanup, scan);
                let at = rows.iter().position(|&row| row == index).unwrap_or(0);
                let after: Vec<Finding> = rows[at + 1..]
                    .iter()
                    .chain(rows[..at].iter().rev())
                    .map(|&row| scan.findings[row])
                    .collect();
                let (objects, tolerance) = (scan.objects.clone(), scan.tolerance);
                if !self.geometry_cleanup_fix(i, picked, fix, &objects, tolerance) {
                    return Task::none();
                }
                self.geometry_cleanup_scan(i, objects, Some(tolerance), false);
                self.geometry_cleanup_pick_next(i, &after);
            }
            GeometryCleanupMsg::Fold(kind) => {
                let folded = &mut self.geometry_cleanup.folded;
                if let Some(at) = folded.iter().position(|k| *k == kind) {
                    folded.remove(at);
                } else {
                    folded.push(kind);
                }
            }
        }
        Task::none()
    }

    /// After a repair: pick the first of `after` that is still an error,
    /// and zoom to it. Nothing is picked when none is left.
    fn geometry_cleanup_pick_next(&mut self, i: usize, after: &[Finding]) {
        let Some(scan) = self.geometry_cleanup_scan_here(i) else {
            return;
        };
        let tolerance = scan.tolerance;
        let next = after.iter().find_map(|old| {
            let index = scan.findings.iter().position(|f| f.same_error(old))?;
            Some((index, scan.findings[index]))
        });
        self.geometry_cleanup.picked = next.map(|(index, _)| index);
        if let Some((_, finding)) = next {
            self.geometry_cleanup_show(i, finding, tolerance);
        }
    }

    /// Zoom to an error and select its objects.
    fn geometry_cleanup_show(&mut self, i: usize, finding: Finding, tolerance: f64) {
        let reach = (tolerance * 25.0).max(finding.deviation * 5.0);
        let [x, y] = finding.location;
        self.handle_zoom_to_window(
            glam::DVec3::new(x - reach, y - reach, 0.0),
            glam::DVec3::new(x + reach, y + reach, 0.0),
        );
        let scene = &mut self.tabs[i].scene;
        scene.select_entity(finding.handle, true);
        if let Some((other, _)) = finding.other {
            scene.select_entity(other, false);
        }
        self.refresh_selected_grips();
    }

    /// Repair one error as one undo step; `false` when nothing changed. The
    /// drawing can have changed since the scan (an undo, an edit), so the
    /// error is looked up again first; an error that is gone changes
    /// nothing.
    fn geometry_cleanup_fix(
        &mut self,
        i: usize,
        picked: Finding,
        fix: Fix,
        objects: &[Handle],
        tolerance: f64,
    ) -> bool {
        let Some(finding) = self
            .geometry_cleanup_findings(i, objects, tolerance)
            .into_iter()
            .find(|f| f.same_error(&picked))
        else {
            self.command_line
                .push_error(&crate::t!("The error is no longer in the drawing."));
            return false;
        };
        let document = &self.tabs[i].scene.document;
        let entity = |handle: Handle| document.get_entity(handle);
        let mut edits: Vec<(Handle, EntityType)> = Vec::new();
        let done = (|| -> Option<()> {
            let own = entity(finding.handle)?;
            match fix {
                Fix::Trim | Fix::Extend => {
                    edits.push((
                        finding.handle,
                        cleanup::moved_along(own, finding.at_start, finding.target)?,
                    ));
                }
                Fix::Stretch => {
                    edits.push((
                        finding.handle,
                        cleanup::stretched(own, finding.at_start, finding.target)?,
                    ));
                }
                Fix::Move => {
                    let by = [
                        finding.target[0] - finding.location[0],
                        finding.target[1] - finding.location[1],
                    ];
                    edits.push((finding.handle, cleanup::moved(own, by)));
                }
                Fix::ExtendBoth => {
                    let corner = finding.corner?;
                    let (other, other_at_start) = finding.other?;
                    edits.push((
                        finding.handle,
                        cleanup::moved_along(own, finding.at_start, corner)?,
                    ));
                    edits.push((
                        other,
                        cleanup::moved_along(entity(other)?, other_at_start, corner)?,
                    ));
                }
            }
            Some(())
        })();
        if done.is_none() || !finding.fixes().contains(&fix) {
            self.command_line
                .push_error(&crate::t!("This error cannot be fixed that way."));
            return false;
        }
        self.push_undo_snapshot(i, "GEOMETRYCLEANUP");
        let mut changes = Vec::new();
        for (handle, replacement) in edits {
            if let Some(target) = self.tabs[i].scene.document.get_entity_mut(handle) {
                *target = replacement;
                changes.push((handle, crate::scene::ChangeKind::Modified));
            }
        }
        self.tabs[i].scene.bump_entities(&changes);
        self.tabs[i].dirty = true;
        self.refresh_selected_grips();
        self.refresh_properties();
        true
    }
}
