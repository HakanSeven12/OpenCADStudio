//! GEOMETRYCLEANUPOPEN, GEOMETRYCLEANUPCLOSE and the Geometry Cleanup
//! palette: the scan of the selected objects and the repair of one error.

use crate::app::{Message, OpenCADStudio};
use crate::modules::manage::geometry_cleanup as cleanup;
use crate::modules::manage::geometry_cleanup_settings::{
    clean_tolerance, CleanupSettings, MIN_TOLERANCE,
};
use crate::ui::dock::PanelId;
use crate::ui::window::geometry_cleanup::{
    rows, AngleFinding, Finding, Fix, GeometryCleanupMsg, KeptError, Scan, ScanInput, ScanJob,
    ScanOutput, ScanTolerances, MARKER_REACH,
};
use crate::ui::window::geometry_cleanup_settings::SettingsDialog;
use document::{EntityType, Handle};
use iced::Task;
use ocs_geom::geom2d::cleanup::{
    angle_errors_until, end_errors_until, proposed_tolerance, AngleTolerances, Tolerances,
};
use ocs_geom::geom2d::{Curve, Tolerance};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Ends nearer than this meet: they are not an error.
const WELD: f64 = 1e-6;

/// The variables of Geometry Cleanup.
const SYSVARS: &[&str] = &["GEOMETRYCLEANUPSTATE", "GEOMETRYCLEANUPZOOMPAN"];

/// The colour index of the picked error's objects in review mode (red).
const REVIEW_COLOR: i16 = 1;

/// A scan of this many curves or more runs off the UI thread, and the
/// palette can be closed while it runs. A scan of fewer is done at once.
const BACKGROUND_FROM: usize = 10_000;

/// The name of the scan's background task, in its failure message.
pub(in crate::app) const SCAN_TASK: &str = "Geometry Cleanup scan";

#[cfg(test)]
thread_local! {
    /// The scans of the drawing on this thread, for the tests.
    pub(super) static SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

impl OpenCADStudio {
    pub(super) fn dispatch_geometry_cleanup(
        &mut self,
        cmd: &str,
        i: usize,
    ) -> Option<Task<Message>> {
        let cmd = cmd.trim();
        let (verb, rest) = cmd.split_once(char::is_whitespace).unwrap_or((cmd, ""));
        let verb = verb.to_ascii_uppercase();
        let rest = rest.trim();
        if verb == "SETVAR" {
            let (name, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            let name = name.to_ascii_uppercase();
            if !SYSVARS.contains(&name.as_str()) {
                return None;
            }
            self.geometry_cleanup_sysvar(&name, Some(value.trim()).filter(|v| !v.is_empty()));
            return Some(Task::none());
        }
        if SYSVARS.contains(&verb.as_str()) {
            self.geometry_cleanup_sysvar(&verb, Some(rest).filter(|v| !v.is_empty()));
            return Some(Task::none());
        }
        match verb.as_str() {
            "GEOMETRYERRORREVIEWCLOSE" => {
                self.geometry_cleanup_end_review(i);
                Some(Task::none())
            }
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

    /// GEOMETRYCLEANUPSTATE is read-only; GEOMETRYCLEANUPZOOMPAN takes 0
    /// or 1 (asked for when none is given).
    fn geometry_cleanup_sysvar(&mut self, name: &str, value: Option<&str>) {
        if name == "GEOMETRYCLEANUPSTATE" {
            let v = usize::from(self.geometry_cleanup.show);
            self.command_line
                .push_output(&crate::tf!("{name} = {v} (read only)"));
            return;
        }
        let current = i32::from(!self.geometry_cleanup.auto_zoom_off);
        let ask = |app: &mut Self| {
            app.command_line
                .push_output(&crate::tf!("Enter new value for {name} <{current}>:"));
            app.pending_setvar = Some(name.to_string());
        };
        match value.map(str::parse::<i32>) {
            None => ask(self),
            Some(Ok(n @ (0 | 1))) => {
                self.geometry_cleanup.auto_zoom_off = n == 0;
                self.persist_settings_if_changed();
            }
            Some(_) => {
                self.command_line
                    .push_rejection(&crate::t!("Requires 0 or 1 only."));
                ask(self);
            }
        }
    }

    /// End review mode: the drawing is drawn as it is again.
    pub(in crate::app) fn geometry_cleanup_end_review(&mut self, i: usize) {
        self.geometry_cleanup.review = false;
        self.geometry_cleanup_review_display(i);
    }

    /// Show or end the review display of tab `i`: in review mode, the
    /// picked error's objects in the review colour and the rest faded (the
    /// display of count mode). Not on a tab in count mode, which owns it.
    pub(in crate::app) fn geometry_cleanup_review_display(&mut self, i: usize) {
        let picked = self
            .geometry_cleanup_scan_here(i)
            .zip(self.geometry_cleanup.picked)
            .and_then(|(scan, index)| scan.findings.get(index).copied())
            .filter(|_| self.geometry_cleanup.review && self.geometry_cleanup.show);
        if self.tabs[i].count.is_some() {
            self.geometry_cleanup.review_shown = false;
            return;
        }
        let display = picked.map(|finding| crate::scene::CountDisplay {
            counted: std::iter::once(finding.handle)
                .chain(finding.other.map(|o| o.0))
                .collect(),
            errors: Default::default(),
            color: crate::ui::window::count_palette::aci_rgba(REVIEW_COLOR),
            error_color: crate::ui::window::count_palette::aci_rgba(REVIEW_COLOR),
        });
        if display.is_none() && !self.geometry_cleanup.review_shown {
            return;
        }
        self.geometry_cleanup.review_shown = display.is_some();
        // The colours change, not the geometry: the scan is still true.
        let fresh = self.geometry_cleanup_scan_current(i);
        self.tabs[i].scene.set_count_display(display);
        let epoch = self.tabs[i].scene.geometry_epoch;
        if let Some(scan) = self.geometry_cleanup.scan.as_mut().filter(|_| fresh) {
            scan.epoch = epoch;
        }
    }

    /// Open or close the Geometry Cleanup palette (docked, expanded).
    pub(in crate::app) fn set_geometry_cleanup(&mut self, open: bool) {
        let id = PanelId::GeometryCleanup;
        if !open {
            self.geometry_cleanup_close_menu(self.active_tab);
            self.geometry_cleanup.review = false;
            self.geometry_cleanup_stop_scan();
        }
        self.geometry_cleanup.show = open;
        if open {
            self.dock_open_at_default(id);
        } else {
            self.dock_unpeek(id);
            self.geometry_cleanup_review_display(self.active_tab);
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

    /// The direction of the UCS X axis in tab `i`, in radians from the WCS
    /// X axis, when the UCS is turned only about Z; otherwise 0.
    fn geometry_cleanup_x_axis(&self, i: usize) -> f64 {
        let (_, x, _, z) = self.tabs[i].ucs_xform().axes();
        if z.x.abs() <= 1e-9 && z.y.abs() <= 1e-9 && z.z > 0.0 {
            x.y.atan2(x.x)
        } else {
            0.0
        }
    }

    /// The errors among `objects` now, each within the tolerance of its
    /// type.
    fn geometry_cleanup_findings(
        &self,
        i: usize,
        objects: &[Handle],
        tolerances: Tolerances,
        angles: &AngleTolerances,
    ) -> Vec<Finding> {
        let tolerances = ScanTolerances::Given(tolerances, angles.clone());
        let input = self.geometry_cleanup_scan_input(i, objects, tolerances);
        scan_of(&input, &AtomicBool::new(false))
            .map(|output| output.found)
            .unwrap_or_default()
    }

    /// What a scan of `objects` in tab `i` reads. To propose the tolerances
    /// it copies the curves of the whole drawing once, and takes those of
    /// `objects` from them.
    fn geometry_cleanup_scan_input(
        &self,
        i: usize,
        objects: &[Handle],
        tolerances: ScanTolerances,
    ) -> ScanInput {
        let x_axis = self.geometry_cleanup_x_axis(i);
        let ScanTolerances::Proposed(_) = tolerances else {
            return ScanInput {
                curves: self.geometry_cleanup_curves(i, objects),
                tolerances,
                x_axis,
            };
        };
        let drawing = self.geometry_cleanup_curves(i, &self.geometry_cleanup_model_space(i));
        let at: std::collections::HashMap<Handle, usize> = drawing
            .iter()
            .enumerate()
            .map(|(n, (handle, _))| (*handle, n))
            .collect();
        let curves = objects
            .iter()
            .filter_map(|handle| at.get(handle).map(|&n| drawing[n].clone()))
            .collect();
        ScanInput {
            curves,
            tolerances: ScanTolerances::Proposed(
                drawing.into_iter().map(|(_, curve)| curve).collect(),
            ),
            x_axis,
        }
    }

    /// The objects in model space that Geometry Cleanup may read.
    pub(in crate::app) fn geometry_cleanup_model_space(&self, i: usize) -> Vec<Handle> {
        let document = &self.tabs[i].scene.document;
        document
            .entities()
            .map(|entity| entity.common().handle)
            .filter(|handle| cleanup::is_candidate(document, *handle))
            .collect()
    }

    /// The tolerance proposed for the drawing of tab `i`.
    pub(in crate::app) fn geometry_cleanup_proposed(&self, i: usize) -> f64 {
        let objects = self.geometry_cleanup_model_space(i);
        let drawing: Vec<Curve> = self
            .geometry_cleanup_curves(i, &objects)
            .into_iter()
            .map(|(_, curve)| curve)
            .collect();
        proposed(&drawing)
    }

    /// Scan `objects` and show what was found. A new scan (`tolerances` is
    /// `None`) uses the saved settings, or the proposed settings for the
    /// drawing when there are none; a rescan keeps the tolerances of its
    /// scan, and the picked error if it is still there.
    pub(in crate::app) fn geometry_cleanup_scan(
        &mut self,
        i: usize,
        objects: Vec<Handle>,
        tolerances: Option<(Tolerances, AngleTolerances)>,
        report: bool,
    ) {
        let rescan = tolerances.is_some();
        // The rows change: a menu or a preview of the old list is wrong.
        self.geometry_cleanup_close_menu(i);
        // A newer scan replaces one that runs.
        self.geometry_cleanup_cancel_job();
        let tolerances = match (tolerances, &self.geometry_cleanup.settings) {
            (Some((tolerances, angles)), _) => ScanTolerances::Given(tolerances, angles),
            (None, Some(settings)) => {
                ScanTolerances::Given(settings.tolerances(), settings.angle_tolerances())
            }
            (None, None) => ScanTolerances::Proposed(Vec::new()),
        };
        if !rescan {
            // A new selection ends review mode.
            self.geometry_cleanup.review = false;
        }
        let input = self.geometry_cleanup_scan_input(i, &objects, tolerances);
        self.geometry_cleanup.jobs += 1;
        let job = ScanJob {
            id: self.geometry_cleanup.jobs,
            tab: self.tabs[i].id,
            epoch: self.tabs[i].scene.geometry_epoch,
            objects,
            input: Arc::new(input),
            stop: Arc::new(AtomicBool::new(false)),
            rescan,
            report,
            started: false,
            after: None,
        };
        if self.geometry_cleanup_in_background(job.input.curves.len()) {
            // It starts when this message has been handled.
            self.geometry_cleanup.job = Some(job);
            self.geometry_cleanup_review_display(i);
            return;
        }
        let Some(output) = scan_of(&job.input, &job.stop) else {
            return;
        };
        self.geometry_cleanup_finish(i, job, output);
    }

    /// Whether a scan of `curves` curves runs off the UI thread.
    fn geometry_cleanup_in_background(&self, curves: usize) -> bool {
        #[cfg(test)]
        if self.geometry_cleanup.background_always {
            return true;
        }
        curves >= BACKGROUND_FROM
    }

    /// Start the scan job, off the UI thread, once the message that made it
    /// has been handled.
    pub(in crate::app) fn geometry_cleanup_start_job(&mut self) -> Task<Message> {
        let Some(job) = self
            .geometry_cleanup
            .job
            .as_mut()
            .filter(|job| !job.started)
        else {
            return Task::none();
        };
        job.started = true;
        let (id, input, stop) = (job.id, Arc::clone(&job.input), Arc::clone(&job.stop));
        crate::app::update::background_task(
            SCAN_TASK,
            move || scan_of(&input, &stop),
            move |found| Message::GeometryCleanup(GeometryCleanupMsg::Scanned(id, Arc::new(found))),
        )
    }

    /// Stop the running scan job and forget it: its result is not used.
    /// Whether there was one.
    fn geometry_cleanup_cancel_job(&mut self) -> bool {
        let Some(job) = self.geometry_cleanup.job.take() else {
            return false;
        };
        job.stop.store(true, Ordering::Relaxed);
        true
    }

    /// Stop the running scan and clear the list: closing the palette, its
    /// Cancel, or a failed scan. The drawing does not change.
    pub(in crate::app) fn geometry_cleanup_stop_scan(&mut self) {
        if !self.geometry_cleanup_cancel_job() {
            return;
        }
        self.geometry_cleanup.scan = None;
        self.geometry_cleanup.picked = None;
        self.geometry_cleanup.review = false;
        self.geometry_cleanup_review_display(self.active_tab);
    }

    /// The scan job `id` ended with `found`. The result of a job that was
    /// stopped or replaced, or of a drawing that is closed, is dropped.
    fn geometry_cleanup_scanned(&mut self, id: u64, found: &Option<ScanOutput>) {
        let Some(job) = self.geometry_cleanup.job.take_if(|job| job.id == id) else {
            return;
        };
        let Some(i) = self.tabs.iter().position(|tab| tab.id == job.tab) else {
            return;
        };
        let Some(output) = found.clone() else {
            return;
        };
        // The scan is of the drawing at the job's epoch: when objects of it
        // changed meanwhile, the palette scans again.
        self.geometry_cleanup_finish(i, job, output);
    }

    /// Show what `job` found in tab `i`.
    fn geometry_cleanup_finish(&mut self, i: usize, job: ScanJob, output: ScanOutput) {
        let rescan = job.rescan;
        let marks = self.geometry_cleanup_kept_marks(i);
        let mut scan = Scan {
            tab: job.tab,
            epoch: job.epoch,
            object_set: job.objects.iter().copied().collect(),
            objects: job.objects,
            tolerances: output.tolerances,
            angles: output.angles,
            found: output.found,
            findings: Vec::new(),
            kept: Vec::new(),
        };
        scan.split(&marks);
        if job.report {
            self.command_line.push_output(&crate::tf!(
                "Geometry Cleanup: {} error(s) found.",
                scan.findings.len()
            ));
        }
        let picked = self
            .geometry_cleanup_picked()
            .filter(|_| rescan)
            .and_then(|old| scan.findings.iter().position(|f| f.same_error(&old)));
        self.geometry_cleanup.scan = Some(scan);
        self.geometry_cleanup.picked = picked;
        self.geometry_cleanup_review_display(i);
        if let Some(after) = job.after {
            self.geometry_cleanup_pick_next(i, &after);
        }
    }

    /// After a repair or a Keep: pick the next of `after` now, or when the
    /// scan that runs ends.
    fn geometry_cleanup_then_pick(&mut self, i: usize, after: Vec<Finding>) {
        match self.geometry_cleanup.job.as_mut() {
            Some(job) => job.after = Some(after),
            None => self.geometry_cleanup_pick_next(i, &after),
        }
    }

    /// The picked error of the palette's scan.
    fn geometry_cleanup_picked(&self) -> Option<Finding> {
        let scan = self.geometry_cleanup.scan.as_ref()?;
        scan.findings.get(self.geometry_cleanup.picked?).copied()
    }

    /// Whether the palette's scan of tab `i` is still true: no object that
    /// it read has changed since. A change that the scene cannot name (an
    /// undo, a file open) counts as a change.
    fn geometry_cleanup_scan_current(&self, i: usize) -> bool {
        let Some(scan) = self.geometry_cleanup_scan_here(i) else {
            return false;
        };
        let scene = &self.tabs[i].scene;
        scan.epoch == scene.geometry_epoch
            || scene.replay_since(scan.epoch).is_some_and(|changes| {
                changes
                    .iter()
                    .all(|(handle, _)| !scan.object_set.contains(handle))
            })
    }

    /// The errors kept as designed in the drawing of tab `i`. Marks of
    /// objects that are no longer in the drawing are dropped first.
    fn geometry_cleanup_kept_marks(&mut self, i: usize) -> HashSet<KeptError> {
        let document = &self.tabs[i].scene.document;
        let id = &document.header.fingerprint_guid;
        if id.is_empty() {
            return HashSet::new();
        }
        let Some(marks) = self.geometry_cleanup.kept.get_mut(id) else {
            return HashSet::new();
        };
        marks.retain(|mark| document.get_entity(Handle::new(mark.handle)).is_some());
        if marks.is_empty() {
            self.geometry_cleanup.kept.remove(id);
            return HashSet::new();
        }
        marks.iter().copied().collect()
    }

    /// The FINGERPRINTGUID of the drawing of tab `i`, which names its kept
    /// errors. A drawing that has none gets one now and saves it with its
    /// next save, as the node graph does.
    fn geometry_cleanup_drawing_id(&mut self, i: usize) -> String {
        let tab = &mut self.tabs[i];
        let header = &mut tab.scene.document.header;
        if header.fingerprint_guid.is_empty() {
            header.fingerprint_guid =
                format!("{{{}}}", crate::app::control::new_guid_v4().to_uppercase());
            tab.dirty = true;
        }
        header.fingerprint_guid.clone()
    }

    /// The markers of the errors in the drawing of tab `i`: each error's
    /// index in the scan and its place on the canvas, for the errors in
    /// view. None while the palette is closed, in a layout, or while a
    /// command runs.
    pub(in crate::app) fn geometry_cleanup_markers(&self, i: usize) -> Vec<(usize, iced::Point)> {
        let tab = &self.tabs[i];
        if !self.geometry_cleanup.show
            || self.geometry_cleanup.job.is_some()
            || tab.is_start
            || tab.active_cmd.is_some()
            || tab.scene.current_layout != "Model"
        {
            return Vec::new();
        }
        let Some(scan) = self.geometry_cleanup_scan_here(i) else {
            return Vec::new();
        };
        let (vw, vh) = tab.scene.selection.borrow().view.vp_size;
        let tile = tab.scene.active_model_tile_bounds(vw, vh);
        let project = tab.scene.camera.borrow().projector(tile);
        scan.findings
            .iter()
            .enumerate()
            .filter_map(|(index, finding)| {
                let [x, y] = finding.location;
                let at = project(glam::DVec3::new(x, y, 0.0))?;
                let at = iced::Point::new(tile.x + at.x, tile.y + at.y);
                let inside = at.x >= tile.x
                    && at.y >= tile.y
                    && at.x <= tile.x + tile.width
                    && at.y <= tile.y + tile.height;
                inside.then_some((index, at))
            })
            .collect()
    }

    /// The open marker menu and the place of its marker on the canvas.
    pub(in crate::app) fn geometry_cleanup_marker_menu(
        &self,
        i: usize,
    ) -> Option<(iced::Point, iced::Element<'_, Message>)> {
        let index = self.geometry_cleanup.menu?;
        let (_, at) = self
            .geometry_cleanup_markers(i)
            .into_iter()
            .find(|(marker, _)| *marker == index)?;
        let finding = self.geometry_cleanup_scan_here(i)?.findings.get(index)?;
        Some((
            at,
            crate::ui::window::geometry_cleanup::marker_menu(index, finding),
        ))
    }

    /// The pointer moved on the canvas to `p`: on a marker, open its menu;
    /// away from the marker and its menu, close the menu.
    pub(in crate::app) fn geometry_cleanup_pointer(&mut self, i: usize, p: iced::Point) {
        let markers = self.geometry_cleanup_markers(i);
        let on = markers
            .iter()
            .filter(|(_, at)| (at.x - p.x).hypot(at.y - p.y) <= MARKER_REACH)
            .min_by(|a, b| {
                let d = |at: iced::Point| (at.x - p.x).hypot(at.y - p.y);
                d(a.1).total_cmp(&d(b.1))
            })
            .map(|(index, _)| *index);
        match on {
            Some(index) if self.geometry_cleanup.menu != Some(index) => {
                self.geometry_cleanup_close_menu(i);
                self.geometry_cleanup.menu = Some(index);
            }
            Some(_) => {}
            None if self.geometry_cleanup.menu.is_some() && !self.geometry_cleanup.menu_hovered => {
                self.geometry_cleanup_close_menu(i);
            }
            None => {}
        }
    }

    /// Close the marker menu and end the preview of a repair.
    fn geometry_cleanup_close_menu(&mut self, i: usize) {
        self.geometry_cleanup.menu = None;
        self.geometry_cleanup.menu_hovered = false;
        self.geometry_cleanup_preview(i, None);
    }

    /// Preview repair `target` in the drawing of tab `i`: the objects that
    /// it changes are hidden, and drawn as the repair leaves them. `None`
    /// ends the preview.
    fn geometry_cleanup_preview(&mut self, i: usize, target: Option<(usize, Fix)>) {
        if self.geometry_cleanup.preview == target {
            return;
        }
        let had = self.geometry_cleanup.preview.is_some();
        self.geometry_cleanup.preview = target;
        // A command owns the preview while it runs.
        if self.tabs[i].active_cmd.is_some() {
            return;
        }
        let edits = target.and_then(|(index, fix)| {
            let finding = *self.geometry_cleanup_scan_here(i)?.findings.get(index)?;
            finding
                .fixes()
                .contains(&fix)
                .then(|| self.geometry_cleanup_edits(i, &finding, fix))?
        });
        let fresh = self.geometry_cleanup_scan_current(i);
        let scene = &mut self.tabs[i].scene;
        match edits.filter(|edits| !edits.is_empty()) {
            Some(edits) => {
                let (handles, entities): (Vec<Handle>, Vec<EntityType>) = edits.into_iter().unzip();
                let wires = scene.wires_for_entities(&entities);
                scene.set_command_preview_hidden(&handles);
                scene.set_preview_wires(wires);
            }
            None if had => scene.clear_preview_wire(),
            None => {}
        }
        // Hiding the objects moves the geometry epoch, but changes no
        // geometry: the scan is still true, and a rescan would end the
        // preview.
        let epoch = self.tabs[i].scene.geometry_epoch;
        if let Some(scan) = self.geometry_cleanup.scan.as_mut().filter(|_| fresh) {
            scan.epoch = epoch;
        }
    }

    /// List the errors again after a Keep or a Restore: the kept errors
    /// changed, not the drawing. A scan that is still true is split again
    /// with no scan; one that is not is scanned again.
    fn geometry_cleanup_relist(&mut self, i: usize) {
        // The rows change: a menu or a preview of the old list is wrong.
        self.geometry_cleanup_close_menu(i);
        if !self.geometry_cleanup_scan_current(i) {
            if let Some(scan) = self.geometry_cleanup_scan_here(i) {
                let (objects, tolerances) = (scan.objects.clone(), scan_tolerances(scan));
                self.geometry_cleanup_scan(i, objects, Some(tolerances), false);
            }
            return;
        }
        let marks = self.geometry_cleanup_kept_marks(i);
        let old = self.geometry_cleanup_picked();
        let epoch = self.tabs[i].scene.geometry_epoch;
        let Some(scan) = self.geometry_cleanup.scan.as_mut() else {
            return;
        };
        scan.epoch = epoch;
        scan.split(&marks);
        self.geometry_cleanup.picked =
            old.and_then(|old| scan.findings.iter().position(|f| f.same_error(&old)));
        self.geometry_cleanup_review_display(i);
    }

    /// After the drawing changes, the open palette scans its objects again:
    /// an edit, an undo or a repair keeps the list true. Not while a command
    /// or a grip edit runs, which change the drawing on each move; the scan
    /// comes when they end.
    pub(in crate::app) fn refresh_geometry_cleanup_if_stale(&mut self) {
        let i = self.active_tab;
        let tab = &self.tabs[i];
        if !self.geometry_cleanup.show
            || tab.active_cmd.is_some()
            || tab.active_grip.is_some()
            || self.geometry_cleanup.job.is_some()
        {
            return;
        }
        let Some(scan) = self.geometry_cleanup_scan_here(i) else {
            return;
        };
        let epoch = self.tabs[i].scene.geometry_epoch;
        if scan.epoch == epoch {
            return;
        }
        if self.geometry_cleanup_scan_current(i) {
            // Only objects that the scan did not read changed.
            if let Some(scan) = self.geometry_cleanup.scan.as_mut() {
                scan.epoch = epoch;
            }
            return;
        }
        let (objects, tolerances) = (scan.objects.clone(), scan_tolerances(scan));
        self.geometry_cleanup_scan(i, objects, Some(tolerances), false);
    }

    /// The scan of the active tab, if the palette shows one.
    pub(in crate::app) fn geometry_cleanup_scan_here(&self, i: usize) -> Option<&Scan> {
        self.geometry_cleanup
            .scan
            .as_ref()
            .filter(|scan| scan.tab == self.tabs[i].id)
    }

    pub(in crate::app) fn on_geometry_cleanup(&mut self, m: GeometryCleanupMsg) -> Task<Message> {
        let i = self.active_tab;
        // While a scan runs, the list of the last one may be wrong.
        if self.geometry_cleanup.job.is_some()
            && matches!(
                m,
                GeometryCleanupMsg::Pick(_)
                    | GeometryCleanupMsg::Fix(..)
                    | GeometryCleanupMsg::Keep(_)
                    | GeometryCleanupMsg::Restore(_)
                    | GeometryCleanupMsg::RestoreAll
                    | GeometryCleanupMsg::Preview(Some(_))
            )
        {
            return Task::none();
        }
        if matches!(
            m,
            GeometryCleanupMsg::Pick(_) | GeometryCleanupMsg::Fix(..) | GeometryCleanupMsg::Keep(_)
        ) {
            // The drawing shows the objects as they are before a change.
            self.geometry_cleanup_close_menu(i);
        }
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
                    let (objects, tolerances) = (scan.objects.clone(), scan_tolerances(scan));
                    self.geometry_cleanup_scan(i, objects, Some(tolerances), true);
                }
            }
            GeometryCleanupMsg::Keep(index) => {
                let Some(scan) = self.geometry_cleanup_scan_here(i) else {
                    return Task::none();
                };
                let Some(picked) = scan.findings.get(index).copied() else {
                    return Task::none();
                };
                let after = rows_after(&self.geometry_cleanup, scan, index);
                let id = self.geometry_cleanup_drawing_id(i);
                let marks = self.geometry_cleanup.kept.entry(id).or_default();
                if !marks.contains(&picked.identity()) {
                    marks.push(picked.identity());
                }
                self.geometry_cleanup_relist(i);
                self.geometry_cleanup_then_pick(i, after);
            }
            GeometryCleanupMsg::Restore(index) => {
                let Some(restored) = self
                    .geometry_cleanup_scan_here(i)
                    .and_then(|scan| scan.kept.get(index))
                    .map(Finding::identity)
                else {
                    return Task::none();
                };
                let id = &self.tabs[i].scene.document.header.fingerprint_guid;
                if let Some(marks) = self.geometry_cleanup.kept.get_mut(id) {
                    marks.retain(|mark| *mark != restored);
                    if marks.is_empty() {
                        self.geometry_cleanup.kept.remove(id);
                    }
                }
                self.geometry_cleanup_relist(i);
            }
            GeometryCleanupMsg::RestoreAll => {
                let id = &self.tabs[i].scene.document.header.fingerprint_guid;
                self.geometry_cleanup.kept.remove(id);
                self.geometry_cleanup_relist(i);
            }
            GeometryCleanupMsg::Scanned(id, found) => self.geometry_cleanup_scanned(id, &found),
            GeometryCleanupMsg::CancelScan => self.geometry_cleanup_stop_scan(),
            GeometryCleanupMsg::FoldKept => {
                self.geometry_cleanup.kept_open = !self.geometry_cleanup.kept_open;
            }
            GeometryCleanupMsg::Preview(target) => self.geometry_cleanup_preview(i, target),
            GeometryCleanupMsg::MenuHover(on) => self.geometry_cleanup.menu_hovered = on,
            GeometryCleanupMsg::CloseReview => self.geometry_cleanup_end_review(i),
            GeometryCleanupMsg::ToggleAutoZoom => {
                self.geometry_cleanup.auto_zoom_off = !self.geometry_cleanup.auto_zoom_off;
                self.persist_settings_if_changed();
            }
            GeometryCleanupMsg::Pick(index) => {
                let Some(finding) = self.geometry_cleanup_scan_here(i).and_then(|scan| {
                    scan.findings
                        .get(index)
                        .copied()
                        .map(|f| (f, scan.tolerances))
                }) else {
                    return Task::none();
                };
                self.geometry_cleanup.picked = Some(index);
                self.geometry_cleanup.review = true;
                self.geometry_cleanup_show(i, finding.0, finding.1);
                self.geometry_cleanup_review_display(i);
            }
            GeometryCleanupMsg::Fix(index, fix) => {
                let Some(scan) = self.geometry_cleanup_scan_here(i) else {
                    return Task::none();
                };
                let Some(picked) = scan.findings.get(index).copied() else {
                    return Task::none();
                };
                let after = rows_after(&self.geometry_cleanup, scan, index);
                let (objects, tolerances) = (scan.objects.clone(), scan_tolerances(scan));
                if !self.geometry_cleanup_fix(i, picked, fix, &objects, &tolerances) {
                    return Task::none();
                }
                self.geometry_cleanup_scan(i, objects, Some(tolerances), false);
                self.geometry_cleanup_then_pick(i, after);
            }
            GeometryCleanupMsg::Fold(kind) => {
                let folded = &mut self.geometry_cleanup.folded;
                if let Some(at) = folded.iter().position(|k| *k == kind) {
                    folded.remove(at);
                } else {
                    folded.push(kind);
                }
            }
            m => return self.on_geometry_cleanup_settings(m),
        }
        Task::none()
    }

    /// The messages of the Settings dialog.
    fn on_geometry_cleanup_settings(&mut self, m: GeometryCleanupMsg) -> Task<Message> {
        let i = self.active_tab;
        match m {
            GeometryCleanupMsg::OpenSettings => {
                let proposed = self.geometry_cleanup_proposed(i);
                let settings = self
                    .geometry_cleanup
                    .settings
                    .clone()
                    .unwrap_or_else(|| CleanupSettings::proposed(proposed));
                self.geometry_cleanup.dialog = Some(SettingsDialog::new(
                    &settings,
                    proposed,
                    self.geometry_cleanup.settings_file.clone(),
                ));
                self.active_modal = Some(crate::app::ModalKind::GeometryCleanupSettings);
            }
            GeometryCleanupMsg::SettingsTolerance(kind, value) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.set_text(kind, value);
                }
            }
            GeometryCleanupMsg::SettingsUseProposed(kind) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.use_proposed(kind);
                }
            }
            GeometryCleanupMsg::SettingsRemove(kind) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.remove(kind);
                }
            }
            GeometryCleanupMsg::SettingsAdd(kind) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.add(kind);
                }
            }
            GeometryCleanupMsg::SettingsAngle(index, value) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.set_angle(index, value);
                }
            }
            GeometryCleanupMsg::SettingsAngleTolerance(index, value) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.set_angle_tolerance(index, value);
                }
            }
            GeometryCleanupMsg::SettingsRemoveAngle(index) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.remove_angle(index);
                }
            }
            GeometryCleanupMsg::SettingsAddAngle(kind) => {
                if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                    dialog.add_angle(kind);
                }
            }
            GeometryCleanupMsg::SettingsImport => {
                return Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title(crate::t!("Import Geometry Cleanup Settings").as_ref())
                            .add_filter(
                                crate::t!("Geometry Cleanup Settings").as_ref(),
                                &["gcs", "GCS"],
                            )
                            .pick_file()
                            .await
                            .map(|handle| crate::sys::handle_path(&handle))
                    },
                    |picked| match picked {
                        Some(path) => {
                            Message::GeometryCleanup(GeometryCleanupMsg::SettingsImported(path))
                        }
                        None => Message::Noop,
                    },
                );
            }
            GeometryCleanupMsg::SettingsImported(path) => {
                let read = std::fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|text| CleanupSettings::parse_gcs(&text));
                match read {
                    Ok(settings) => {
                        if let Some(dialog) = self.geometry_cleanup.dialog.as_mut() {
                            *dialog = SettingsDialog::new(
                                &settings,
                                dialog.proposed,
                                Some(path.display().to_string()),
                            );
                        }
                    }
                    Err(error) => self
                        .command_line
                        .push_error(&crate::tf!("The settings file cannot be read: {}", error)),
                }
            }
            GeometryCleanupMsg::SettingsExport => {
                return Task::perform(
                    async {
                        crate::sys::file_dialog()
                            .set_title(crate::t!("Export Geometry Cleanup Settings").as_ref())
                            .add_filter(
                                crate::t!("Geometry Cleanup Settings").as_ref(),
                                &["gcs", "GCS"],
                            )
                            .set_file_name("GeometryCleanup.gcs")
                            .save_file()
                            .await
                            .map(|handle| crate::sys::handle_path(&handle))
                    },
                    |picked| match picked {
                        Some(path) => {
                            Message::GeometryCleanup(GeometryCleanupMsg::SettingsExportTo(path))
                        }
                        None => Message::Noop,
                    },
                );
            }
            GeometryCleanupMsg::SettingsExportTo(path) => {
                let Some(settings) = self
                    .geometry_cleanup
                    .dialog
                    .as_ref()
                    .and_then(|d| d.parsed())
                else {
                    return Task::none();
                };
                if let Err(error) = std::fs::write(&path, settings.to_gcs()) {
                    self.command_line.push_error(&crate::tf!(
                        "The settings file cannot be written: {}",
                        error
                    ));
                }
            }
            GeometryCleanupMsg::SettingsCancel => {
                self.geometry_cleanup.dialog = None;
                self.close_active_modal();
            }
            GeometryCleanupMsg::SettingsScan => {
                let Some(dialog) = self.geometry_cleanup.dialog.as_ref() else {
                    return Task::none();
                };
                let Some(settings) = dialog.parsed() else {
                    return Task::none();
                };
                let file = dialog.file.clone();
                let tolerances = (settings.tolerances(), settings.angle_tolerances());
                self.geometry_cleanup.settings = Some(settings);
                self.geometry_cleanup.settings_file = file;
                self.geometry_cleanup.dialog = None;
                self.close_active_modal();
                // Scan the objects of the palette's scan again, or all of
                // model space when the palette has no scan of this drawing.
                let objects = match self.geometry_cleanup_scan_here(i) {
                    Some(scan) => scan.objects.clone(),
                    None => self.geometry_cleanup_model_space(i),
                };
                self.set_geometry_cleanup(true);
                self.geometry_cleanup_scan(i, objects, Some(tolerances), true);
            }
            _ => {}
        }
        Task::none()
    }

    /// After a repair: pick the first of `after` that is still an error,
    /// and zoom to it. Nothing is picked when none is left.
    fn geometry_cleanup_pick_next(&mut self, i: usize, after: &[Finding]) {
        let Some(scan) = self.geometry_cleanup_scan_here(i) else {
            return;
        };
        let tolerances = scan.tolerances;
        let next = after.iter().find_map(|old| {
            let index = scan.findings.iter().position(|f| f.same_error(old))?;
            Some((index, scan.findings[index]))
        });
        self.geometry_cleanup.picked = next.map(|(index, _)| index);
        if let Some((_, finding)) = next {
            self.geometry_cleanup_show(i, finding, tolerances);
        }
        self.geometry_cleanup_review_display(i);
    }

    /// Zoom to an error and select its objects: round a length error by
    /// its tolerance, round the line or lines of an angle error.
    fn geometry_cleanup_show(&mut self, i: usize, finding: Finding, tolerances: Tolerances) {
        let (low, high) = match finding.kind.end_kind() {
            Some(kind) => {
                let tolerance = tolerances.get(kind).unwrap_or(finding.deviation);
                let reach = (tolerance * 25.0).max(finding.deviation * 5.0);
                let [x, y] = finding.location;
                ([x - reach, y - reach], [x + reach, y + reach])
            }
            None => {
                let document = &self.tabs[i].scene.document;
                let mut low = finding.location;
                let mut high = finding.location;
                let segments = finding.angle.map(|a| (a.segment, a.other_segment));
                let (segment, other_segment) = segments.unwrap_or_default();
                let parts = std::iter::once((finding.handle, segment))
                    .chain(finding.other.map(|o| (o.0, other_segment)));
                for p in parts
                    .filter_map(|(h, segment)| {
                        cleanup::straight_part(document.get_entity(h)?, segment)
                    })
                    .flatten()
                {
                    low = [low[0].min(p[0]), low[1].min(p[1])];
                    high = [high[0].max(p[0]), high[1].max(p[1])];
                }
                let margin = 0.1 * (high[0] - low[0]).max(high[1] - low[1]);
                (
                    [low[0] - margin, low[1] - margin],
                    [high[0] + margin, high[1] + margin],
                )
            }
        };
        let (low, high) = (
            glam::DVec3::new(low[0], low[1], 0.0),
            glam::DVec3::new(high[0], high[1], 0.0),
        );
        // GEOMETRYCLEANUPZOOMPAN 0: only an error that is not in view.
        if !self.geometry_cleanup.auto_zoom_off || !self.geometry_cleanup_in_view(i, low, high) {
            self.handle_zoom_to_window(low, high);
        }
        let scene = &mut self.tabs[i].scene;
        scene.select_entity(finding.handle, true);
        if let Some((other, _)) = finding.other {
            scene.select_entity(other, false);
        }
        self.refresh_selected_grips();
    }

    /// Whether the window from `low` to `high` is all in the view of tab
    /// `i`.
    fn geometry_cleanup_in_view(&self, i: usize, low: glam::DVec3, high: glam::DVec3) -> bool {
        let scene = &self.tabs[i].scene;
        let (vw, vh) = scene.selection.borrow().view.vp_size;
        let tile = scene.active_model_tile_bounds(vw, vh);
        let project = scene.camera.borrow().projector(tile);
        [
            low,
            high,
            glam::DVec3::new(low.x, high.y, 0.0),
            glam::DVec3::new(high.x, low.y, 0.0),
        ]
        .into_iter()
        .all(|corner| {
            project(corner).is_some_and(|at| {
                (0.0..=tile.width).contains(&at.x) && (0.0..=tile.height).contains(&at.y)
            })
        })
    }

    /// Repair one error as one undo step; `false` when nothing changed. The
    /// drawing can have changed since the scan (an undo, an edit), so the
    /// error is looked up again first when an object of the scan changed;
    /// an error that is gone changes nothing.
    fn geometry_cleanup_fix(
        &mut self,
        i: usize,
        picked: Finding,
        fix: Fix,
        objects: &[Handle],
        (tolerances, angles): &(Tolerances, AngleTolerances),
    ) -> bool {
        let finding = if self.geometry_cleanup_scan_current(i) {
            Some(picked)
        } else {
            self.geometry_cleanup_findings(i, objects, *tolerances, angles)
                .into_iter()
                .find(|f| f.same_error(&picked))
        };
        let Some(finding) = finding else {
            self.command_line
                .push_error(&crate::t!("The error is no longer in the drawing."));
            return false;
        };
        let edits = self.geometry_cleanup_edits(i, &finding, fix);
        let Some(edits) = edits.filter(|_| finding.fixes().contains(&fix)) else {
            self.command_line
                .push_error(&crate::t!("This error cannot be fixed that way."));
            return false;
        };
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

    /// The objects that repair `fix` of `finding` changes, each with what
    /// it becomes; `None` when the repair cannot be made. The repair and
    /// its preview both use it.
    fn geometry_cleanup_edits(
        &self,
        i: usize,
        finding: &Finding,
        fix: Fix,
    ) -> Option<Vec<(Handle, EntityType)>> {
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
                Fix::RotateAtStart | Fix::RotateAtEnd => {
                    let angle = finding.angle?;
                    let ends = cleanup::straight_part(own, angle.segment)?;
                    let pivot = ends[usize::from(fix == Fix::RotateAtEnd)];
                    edits.push((
                        finding.handle,
                        cleanup::rotated_part(own, angle.segment, pivot, angle.turn)?,
                    ));
                }
                Fix::RotateShorter | Fix::RotateLonger => {
                    let (other, _) = finding.other?;
                    let angle = finding.angle?;
                    let length = |handle, segment| {
                        let [a, b] = cleanup::straight_part(entity(handle)?, segment)?;
                        Some((b[0] - a[0]).hypot(b[1] - a[1]))
                    };
                    // At equal lengths, the first line is the shorter.
                    let first_shorter = length(finding.handle, angle.segment)?
                        <= length(other, angle.other_segment)?;
                    let (handle, segment, turn) = if first_shorter == (fix == Fix::RotateShorter) {
                        (finding.handle, angle.segment, angle.turn)
                    } else {
                        (other, angle.other_segment, -angle.turn)
                    };
                    edits.push((
                        handle,
                        cleanup::rotated_part(entity(handle)?, segment, finding.location, turn)?,
                    ));
                }
            }
            Some(())
        })();
        done.map(|()| edits)
    }
}

/// The rows below row `index`, then those above it nearest first: where the
/// pick goes after a repair or a Keep.
fn rows_after(
    palette: &crate::ui::window::geometry_cleanup::GeometryCleanupPalette,
    scan: &Scan,
    index: usize,
) -> Vec<Finding> {
    let rows = rows(palette, scan);
    let at = rows.iter().position(|&row| row == index).unwrap_or(0);
    rows[at + 1..]
        .iter()
        .chain(rows[..at].iter().rev())
        .map(|&row| scan.findings[row])
        .collect()
}

/// The tolerances of a scan, for a rescan.
fn scan_tolerances(scan: &Scan) -> (Tolerances, AngleTolerances) {
    (scan.tolerances, scan.angles.clone())
}

/// The tolerance proposed for a drawing of `drawing` curves, as the
/// settings keep it, so that the palette and the dialog show the same value.
fn proposed(drawing: &[Curve]) -> f64 {
    clean_tolerance(proposed_tolerance(drawing)).unwrap_or(MIN_TOLERANCE)
}

/// What a scan finds: the errors, each within the tolerance of its type,
/// and the tolerances used; `None` when `stop` was set before the scan
/// ended. It reads only `input`, so it can run off the UI thread.
pub(super) fn scan_of(input: &ScanInput, stop: &AtomicBool) -> Option<ScanOutput> {
    #[cfg(test)]
    SCANS.with(|scans| scans.set(scans.get() + 1));
    let (tolerances, angles) = match &input.tolerances {
        ScanTolerances::Given(tolerances, angles) => (*tolerances, angles.clone()),
        ScanTolerances::Proposed(drawing) => {
            let settings = CleanupSettings::proposed(proposed(drawing));
            (settings.tolerances(), settings.angle_tolerances())
        }
    };
    let kept = &input.curves;
    let curves: Vec<Curve> = kept.iter().map(|(_, curve)| curve.clone()).collect();
    let ends = end_errors_until(&curves, tolerances, Tolerance::new(WELD), stop)?
        .into_iter()
        .map(|error| Finding {
            kind: error.kind.into(),
            handle: kept[error.end.curve].0,
            at_start: error.end.at_start,
            location: error.location,
            target: error.target,
            other: error
                .other
                .map(|other| (kept[other.curve].0, other.at_start)),
            corner: error.corner,
            deviation: error.deviation,
            angle: None,
            stretch: !matches!(kept[error.end.curve].1, Curve::Ellipse(_)),
        });
    let weld = Tolerance::new(WELD);
    let lines = angle_errors_until(&curves, &angles, input.x_axis, weld, stop)?
        .into_iter()
        .map(|error| Finding {
            kind: error.kind.into(),
            handle: kept[error.curve].0,
            at_start: false,
            location: error.location,
            target: error.location,
            other: error.other.map(|other| (kept[other].0, false)),
            corner: None,
            deviation: error.deviation,
            angle: Some(AngleFinding {
                angle: error.angle,
                turn: error.turn,
                meets: error.meets,
                segment: error.segment,
                other_segment: error.other_segment,
            }),
            stretch: false,
        });
    let found = ends.chain(lines).collect();
    Some(ScanOutput {
        tolerances,
        angles,
        found,
    })
}
