//! GEOMETRYCLEANUPOPEN, GEOMETRYCLEANUPCLOSE and the Geometry Cleanup
//! palette: the scan of the selected objects and the repair of one error.

use crate::app::{Message, OpenCADStudio};
use crate::modules::manage::geometry_cleanup as cleanup;
use crate::modules::manage::geometry_cleanup_settings::{
    clean_tolerance, CleanupSettings, MIN_TOLERANCE,
};
use crate::ui::dock::PanelId;
use crate::ui::window::geometry_cleanup::{
    rows, AngleFinding, Finding, Fix, GeometryCleanupMsg, Scan,
};
use crate::ui::window::geometry_cleanup_settings::SettingsDialog;
use document::{EntityType, Handle};
use iced::Task;
use ocs_geom::geom2d::cleanup::{
    angle_errors, end_errors, proposed_tolerance, AngleTolerances, Tolerances,
};
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
        let kept = self.geometry_cleanup_curves(i, objects);
        let curves: Vec<Curve> = kept.iter().map(|(_, curve)| curve.clone()).collect();
        let ends = end_errors(&curves, tolerances, Tolerance::new(WELD))
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
        let x_axis = self.geometry_cleanup_x_axis(i);
        let lines = angle_errors(&curves, angles, x_axis, Tolerance::new(WELD))
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
        ends.chain(lines).collect()
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
        // As the settings keep it, so that the palette and the dialog show
        // the same value.
        clean_tolerance(proposed_tolerance(&drawing)).unwrap_or(MIN_TOLERANCE)
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
        let (tolerances, angles) = tolerances.unwrap_or_else(|| {
            let settings = match &self.geometry_cleanup.settings {
                Some(settings) => settings.clone(),
                None => CleanupSettings::proposed(self.geometry_cleanup_proposed(i)),
            };
            (settings.tolerances(), settings.angle_tolerances())
        });
        let findings = self.geometry_cleanup_findings(i, &objects, tolerances, &angles);
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
            tolerances,
            angles,
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
                let (objects, tolerances) = (scan.objects.clone(), scan_tolerances(scan));
                if !self.geometry_cleanup_fix(i, picked, fix, &objects, &tolerances) {
                    return Task::none();
                }
                self.geometry_cleanup_scan(i, objects, Some(tolerances), false);
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
        self.handle_zoom_to_window(
            glam::DVec3::new(low[0], low[1], 0.0),
            glam::DVec3::new(high[0], high[1], 0.0),
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
        (tolerances, angles): &(Tolerances, AngleTolerances),
    ) -> bool {
        let Some(finding) = self
            .geometry_cleanup_findings(i, objects, *tolerances, angles)
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

/// The tolerances of a scan, for a rescan.
fn scan_tolerances(scan: &Scan) -> (Tolerances, AngleTolerances) {
    (scan.tolerances, scan.angles.clone())
}
