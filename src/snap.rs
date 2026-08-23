//! OpenCADStudio-style object snap (OSNAP) engine.
//!
//! Implemented modes:
//!   Endpoint, Midpoint, Center, Node, Quadrant, Intersection,
//!   Extension, Insertion, Perpendicular, Nearest, ApparentIntersection, Grid, Tangent

use glam::{DVec3, Mat4, Vec3};
use iced::time::Instant;
use iced::{Point, Rectangle};

use crate::command::TangentObject;
use crate::scene::model::wire_model::{SnapHint, TangentGeom, WireModel};
use crate::scene::pick::interaction_index::WireSource;
const DEFAULT_OSNAP_RADIUS_PX: f32 = 15.0;

// ── Snap type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrackingKind {
    Generic,
    Extension,
    Perpendicular,
}
/// Every OSNAP mode — mirrors the OpenCADStudio list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapType {
    Endpoint,
    Midpoint,
    Center,
    Node,
    Quadrant,
    Intersection,
    Extension,
    Insertion,
    Perpendicular,
    Tangent,
    Nearest,
    ApparentIntersection,
    Parallel,
    Grid,
    /// Object acquisition (domain-object pick, e.g. network structure) — orange marker.
    ObjectPick,
}

/// Ordered list used by the popup and snap engine.
pub const ALL_SNAP_MODES: &[(SnapType, &str, &str)] = &[
    (SnapType::Endpoint, "◻", "Endpoint"),
    (SnapType::Midpoint, "△", "Midpoint"),
    (SnapType::Center, "◯", "Center"),
    (SnapType::Node, "◆", "Node"),
    (SnapType::Quadrant, "◇", "Quadrant"),
    (SnapType::Intersection, "✕", "Intersection"),
    (SnapType::Extension, "—", "Extension"),
    (SnapType::Insertion, "⊾", "Insertion"),
    (SnapType::Perpendicular, "⊥", "Perpendicular"),
    (SnapType::Tangent, "⌒", "Tangent"),
    (SnapType::Nearest, "✧", "Nearest"),
    (SnapType::ApparentIntersection, "✗", "Apparent Intersection"),
    (SnapType::Parallel, "∥", "Parallel"),
    // NOTE: Grid is intentionally NOT an object-snap mode. Grid snap is a
    // separate system (`Snapper::grid_snap_on`) so object snap never catches a
    // grid point; it is toggled on its own and handled directly in `snap()`.
];

// ── Snap result ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct SnapResult {
    pub world: glam::DVec3,
    pub screen: Point,
    pub snap_type: SnapType,
    /// Set when `snap_type == Tangent`; provides entity geometry for TTR/TTT.
    pub tangent_obj: Option<TangentObject>,
    /// Screen position of the endpoint an Extension snap extends from, so the
    /// overlay can draw the dashed extension guide line back to it. `None` for
    /// every other snap type. (#238)
    pub extension_base: Option<Point>,
    /// Second extension-guide base, set only for an extended intersection
    /// (`snap_type == Intersection` where two extension lines cross): the
    /// overlay draws a dashed guide from each base to the crossing so both
    /// contributing extensions stay visible. `None` otherwise. (#247, #259)
    pub extension_base2: Option<Point>,
    /// Acquired endpoint used by the Extension guide and typed distance.
    pub extension_origin: Option<glam::DVec3>,
    /// Source direction paired with `extension_origin`.
    pub extension_dir: Option<glam::DVec3>,
}

/// Object-snap-tracking alignment: the cursor projected onto a ray from an
/// acquired tracking point.
#[derive(Debug, Clone, Copy)]
pub struct OtrackHit {
    /// Cursor projected onto the tracking ray.
    pub aligned: DVec3,
    /// Unit ray direction toward the cursor side (for typed-distance entry).
    pub dir: DVec3,
    /// The tracking point the ray emanates from.
    pub base: DVec3,

    pub kind: TrackingKind,
}

// ── Snapper ───────────────────────────────────────────────────────────────

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

pub struct Snapper {
    /// Global snap on/off toggle.  When false, all snapping is bypassed
    /// but the `enabled` set is preserved so it can be restored.
    pub snap_enabled: bool,
    /// Which snap modes are configured (used when `snap_enabled` is true).
    pub enabled: HashSet<SnapType>,
    /// Grid snap on/off — a system fully separate from object snap. When on,
    /// `snap()` can pick the nearest grid corner; object snap never does.
    pub grid_snap_on: bool,
    /// World-space grid spacing.
    pub grid_spacing: f32,
    /// Pixel-radius snap aperture, shared by OSNAP, tracking, polar and
    /// extension so the catch distance is the same everywhere.
    pub osnap_radius_px: f32,
    /// Object Snap Tracking on/off (F11).
    pub otrack_enabled: bool,
    /// Acquired OST points, world space. Tracking is planar: `tracking_dirs_at`
    /// builds every direction from X/Y with Z zeroed, and the rays follow.
    pub tracking_points: Vec<DVec3>,
    /// Edge directions at each acquired point (parallel to `tracking_points`):
    /// the line direction of every wire segment meeting at that corner, so
    /// OTRACK can offer an alignment ray along a segment's extension, not only
    /// the ortho/polar axes. Pulling the cursor along an acquired corner's edge
    /// then locks to that line (#219). Empty for a point that is not a segment
    /// endpoint (e.g. a midpoint or centre acquisition).
    pub tracking_dirs: Vec<Vec<DVec3>>,
    /// Perpendicular directions through each acquired tracking point.
    ///
    /// Kept separate from `tracking_dirs` because those directions are also
    /// consumed by Extension snap and extended-intersection logic. These rays
    /// belong only to OTRACK when Perpendicular OSNAP is enabled. (#695)
    pub tracking_perp_dirs: Vec<Vec<DVec3>>,
    /// Last snap world position (for dwell detection).
    pub last_snap_world: Option<DVec3>,
    /// When the cursor first rested near `last_snap_world`.
    pub dwell_since: Option<Instant>,
    /// Whether the current dwell already acquired/removed a point (fire once).
    pub dwell_acquired: bool,
    /// The point the in-progress command is drawing *from* (the rubber-band
    /// origin), if any. Perpendicular snap drops its foot from here so the new
    /// segment is genuinely perpendicular to the target — without it, perp
    /// would just give the nearest point on the line. Set before each `snap`.
    pub from_point: Option<Vec3>,
    /// Parallel snap: the acquired reference as (unit direction, a point on the
    /// line). When the cursor's direction from the command's start point runs
    /// parallel to this, the point locks onto that parallel line; the point half
    /// marks the reference on screen. Works with only the Parallel object snap
    /// on — independent of OTRACK (#277).
    pub parallel_ref: Option<(Vec3, Vec3)>,
    /// Dwell state for acquiring/removing `parallel_ref`: the candidate line
    /// direction + point under the cursor, when it was first hovered, and
    /// whether this dwell has already fired (so it acquires/toggles once).
    parallel_dwell: Option<(Vec3, Vec3, Instant, bool)>,
    /// One-shot snap override (Shift+RMB menu): the (enabled set, snap on)
    /// pair saved when the override engaged, restored when it is consumed by
    /// the next point pick or cancelled. While `Some`, `enabled` holds only
    /// the override mode and `snap_enabled` is forced on — the override works
    /// even with running osnap off (#337).
    override_saved: Option<(HashSet<SnapType>, bool)>,
}

impl Default for Snapper {
    fn default() -> Self {
        let mut enabled = HashSet::default();
        enabled.insert(SnapType::Endpoint);
        enabled.insert(SnapType::Midpoint);
        enabled.insert(SnapType::Center);
        enabled.insert(SnapType::Node);
        enabled.insert(SnapType::Quadrant);
        enabled.insert(SnapType::Intersection);
        enabled.insert(SnapType::Nearest);
        Self {
            snap_enabled: false,
            enabled,
            grid_snap_on: false,
            grid_spacing: 1.0,
            osnap_radius_px: DEFAULT_OSNAP_RADIUS_PX,
            otrack_enabled: false,
            tracking_points: Vec::new(),
            tracking_dirs: Vec::new(),
            tracking_perp_dirs: Vec::new(),
            last_snap_world: None,
            dwell_since: None,
            dwell_acquired: false,
            from_point: None,
            parallel_ref: None,
            parallel_dwell: None,
            override_saved: None,
        }
    }
}

impl Snapper {
    /// True when snap is globally on AND at least one mode is configured.
    pub fn is_active(&self) -> bool {
        self.snap_enabled && !self.enabled.is_empty()
    }

    pub fn is_on(&self, t: SnapType) -> bool {
        self.enabled.contains(&t)
    }

    /// Engage a one-shot snap override (Shift+RMB menu): only `t` snaps, even
    /// with running osnap off, until `clear_override` restores the saved
    /// configuration. Re-picking while active replaces the mode but keeps the
    /// original saved state.
    pub fn set_override(&mut self, t: SnapType) {
        if self.override_saved.is_none() {
            self.override_saved = Some((self.enabled.clone(), self.snap_enabled));
        }
        self.enabled = std::iter::once(t).collect();
        self.snap_enabled = true;
    }

    /// Restore the pre-override snap configuration. No-op when inactive.
    pub fn clear_override(&mut self) {
        if let Some((enabled, on)) = self.override_saved.take() {
            self.enabled = enabled;
            self.snap_enabled = on;
        }
    }

    /// Whether temporary tracking points are being acquired and drawn: OTRACK
    /// on, or the Extension object snap on. Extension tracks a segment's line
    /// only from an acquired endpoint, and works independently of OTRACK's
    /// on/off state (#262).
    pub fn tracking_active(&self) -> bool {
        self.otrack_enabled || (self.snap_enabled && self.is_on(SnapType::Extension))
    }

    /// Whether an alignment guide may be on screen — tracking (above) OR a
    /// Parallel lock. Used to gate the guide-line projection; Parallel draws its
    /// alignment guide without acquiring tracking points. (#277)
    pub fn alignment_active(&self) -> bool {
        self.tracking_active() || (self.snap_enabled && self.is_on(SnapType::Parallel))
    }

    pub fn toggle_global(&mut self) {
        self.snap_enabled = !self.snap_enabled;
    }

    /// Grid snap on/off — independent of the object-snap master and mode set.
    pub fn grid_snap(&self) -> bool {
        self.grid_snap_on
    }

    pub fn toggle_grid_snap(&mut self) {
        self.grid_snap_on = !self.grid_snap_on;
    }

    pub fn toggle(&mut self, t: SnapType) {
        if !self.enabled.remove(&t) {
            self.enabled.insert(t);
        }
    }

    pub fn all_on(&self) -> bool {
        ALL_SNAP_MODES
            .iter()
            .all(|(t, _, _)| self.enabled.contains(t))
    }
    pub fn none_on(&self) -> bool {
        self.enabled.is_empty()
    }

    pub fn enable_all(&mut self) {
        for &(t, _, _) in ALL_SNAP_MODES {
            self.enabled.insert(t);
        }
    }
    pub fn disable_all(&mut self) {
        self.enabled.clear();
    }

    /// Update dwell tracking from the current snap result.
    pub fn update_otrack_dwell<W: WireSource + ?Sized>(
        &mut self,
        snap: Option<SnapResult>,
        wires: &W,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        now: Instant,
    ) {
        // Extension uses acquired points even when OTRACK is off. (#262)
        if !self.tracking_active() {
            self.last_snap_world = None;
            self.dwell_since = None;
            self.dwell_acquired = false;
            return;
        }
        // Acquire only stable geometric snap points for OTRACK. (#716)
        let snap_world = snap.and_then(|hit| {
            matches!(
                hit.snap_type,
                SnapType::Endpoint
                    | SnapType::Midpoint
                    | SnapType::Center
                    | SnapType::Node
                    | SnapType::Quadrant
                    | SnapType::Intersection
                    | SnapType::Insertion
                    | SnapType::ApparentIntersection
            )
            .then_some(hit.world)
        });
        // With OTRACK off, acquisition is Extension-driven, and Extension tracks
        // a line only from a real segment endpoint — so acquire endpoints only.
        // This stops a paused cursor on an extension foot (or a midpoint/centre)
        // from being acquired and evicting, through the 4-point cap, the very
        // endpoint the user acquired — the reason the marker vanished after a
        // few pauses (#262). OTRACK keeps acquiring any snap point.
        let endpoints_only = !self.otrack_enabled;
        // The cursor must rest near a snap point for this long before it is
        // acquired, so that brushing past snap points while moving the mouse
        // does not create accidental tracking points.
        const DWELL_MS: u128 = 250;
        const DWELL_PX: f32 = 8.0;

        match snap_world {
            None => {
                // Leaving all geometry: capture the point we were dwelling on if
                // it qualified, before the reset loses it.
                self.acquire_on_leave(now, DWELL_MS, wires, endpoints_only);
                self.last_snap_world = None;
                self.dwell_since = None;
                self.dwell_acquired = false;
            }
            Some(p) => {
                // Convert to screen to measure pixel distance.
                let is_same = if let Some(prev) = self.last_snap_world {
                    let dp = world_to_screen(p, view_rot, eye, bounds);
                    let dp2 = world_to_screen(prev, view_rot, eye, bounds);
                    let dx = dp.x - dp2.x;
                    let dy = dp.y - dp2.y;
                    (dx * dx + dy * dy).sqrt() < DWELL_PX
                } else {
                    false
                };
                if is_same {
                    let elapsed = self
                        .dwell_since
                        .map_or(0, |t| now.duration_since(t).as_millis());
                    if !self.dwell_acquired && elapsed >= DWELL_MS {
                        self.dwell_acquired = true;
                        // Dwelling over an already-acquired point removes it;
                        // otherwise acquire it.
                        let existing = self.tracking_points.iter().position(|t| {
                            let d = (*t - p).length();
                            d < self.grid_spacing as f64 * 0.1
                        });
                        match existing {
                            Some(idx) => {
                                self.tracking_points.remove(idx);

                                if idx < self.tracking_dirs.len() {
                                    self.tracking_dirs.remove(idx);
                                }

                                if idx < self.tracking_perp_dirs.len() {
                                    self.tracking_perp_dirs.remove(idx);
                                }
                            }
                            None => self.acquire_tracking_point(p, wires, endpoints_only),
                        }
                    }
                } else {
                    // Moved to a different snap point: capture the previous one
                    // first if it was dwelt on long enough, so a pause-then-drag
                    // gesture reliably acquires it even without in-place events.
                    self.acquire_on_leave(now, DWELL_MS, wires, endpoints_only);
                    self.last_snap_world = Some(p);
                    self.dwell_since = Some(now);
                    self.dwell_acquired = false;
                }
            }
        }
    }
    /// Immediately acquire an explicit grip origin as a tracking point.
    ///
    /// Unlike normal cursor acquisition, a grip was deliberately selected by the
    /// user, so it should not require OTRACK dwell before Extension/OTRACK can use it.
    pub fn acquire_grip_tracking_point<W: WireSource + ?Sized>(
        &mut self,
        p: DVec3,
        wires: &W,
    ) {
        if !self.tracking_active() {
            return;
        }

        // With OTRACK disabled, Extension acquisition must remain endpoint-only.
        let endpoints_only = !self.otrack_enabled;

        self.acquire_tracking_point(p, wires, endpoints_only);
    }
    /// Add `p` as a tracking point (capturing its corner edge directions) unless
    /// it is already tracked; drops the oldest when the 4-point cap is reached.
    /// Edge directions are scanned once here, at acquisition, so OTRACK can align
    /// to a segment's extension without rescanning geometry per move (#219).
    fn acquire_tracking_point<W: WireSource + ?Sized>(
        &mut self,
        p: DVec3,
        wires: &W,
        endpoints_only: bool,
    ) {
        if self
            .tracking_points
            .iter()
            .any(|t| (*t - p).length() < self.grid_spacing as f64 * 0.1)
        {
            return;
        }
        // Edge directions double as an endpoint test: a point with no incident
        // segment — a midpoint, centre, intersection or extension foot — has
        // none. Extension-driven acquisition (#262) keeps only endpoints.
        let (dirs, perp_dirs) = tracking_dirs_at(p, wires);
        if endpoints_only && dirs.is_empty() {
            return;
        }
        if self.tracking_points.len() >= 4 {
            self.tracking_points.remove(0);

            if !self.tracking_dirs.is_empty() {
                self.tracking_dirs.remove(0);
            }

            if !self.tracking_perp_dirs.is_empty() {
                self.tracking_perp_dirs.remove(0);
            }
        }
        self.tracking_points.push(p);
        self.tracking_dirs.push(dirs);
        self.tracking_perp_dirs.push(perp_dirs);
    }

    /// If the cursor dwelt on a snap point long enough but the in-place check
    /// never fired (a perfectly still cursor emits no move events, so the timer
    /// is only re-examined once the cursor moves off), acquire it now as the
    /// cursor leaves. This makes "pause on a corner, then drag along its edge"
    /// reliably capture the corner (#219).
    fn acquire_on_leave<W: WireSource + ?Sized>(
        &mut self,
        now: Instant,
        dwell_ms: u128,
        wires: &W,
        endpoints_only: bool,
    ) {
        if self.dwell_acquired {
            return; // already handled by the in-place branch
        }
        let Some(prev) = self.last_snap_world else {
            return;
        };
        let elapsed = self
            .dwell_since
            .map_or(0, |t| now.duration_since(t).as_millis());
        if elapsed >= dwell_ms {
            self.acquire_tracking_point(prev, wires, endpoints_only);
        }
    }

    /// Project the cursor onto acquired tracking rays and eligible crossings.
    pub fn otrack_snap(
        &self,
        cursor_world: DVec3,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        polar_step_deg: Option<f32>,
        last_point: Option<DVec3>,
        required_crossing_ray: Option<(DVec3, DVec3)>,
        // Ortho on: the axis from `last_point` is a hard lock. Only crossings of
        // an acquired ray with that axis lock; single tracking rays are
        // suppressed so the cursor can't leave the ortho axis. (#218)
        ortho: bool,
        // UCS axes in world space: tracking rays follow the active UCS while
        // their world geometry stays in f64.
        ucs_x: DVec3,
        ucs_y: DVec3,
    ) -> Option<OtrackHit> {
        if !self.otrack_enabled || self.tracking_points.is_empty() {
            return None;
        }

        let cursor_screen = world_to_screen(cursor_world, view_rot, eye, bounds);
        // Use the same aperture as OSNAP so the catch distance is uniform.
        let r = self.osnap_radius_px;
        let screen_dist = |w: DVec3| {
            let s = world_to_screen(w, view_rot, eye, bounds);
            ((s.x - cursor_screen.x).powi(2) + (s.y - cursor_screen.y).powi(2)).sqrt()
        };

        // Only rays near the cursor direction can fall inside the aperture.
        // Quantize that direction and keep its two neighbours; enumerating all
        // 180 rays at a 1° polar increment multiplied every tracking-point and
        // ray-intersection check for no observable benefit.
        let ray_angles = |origin: DVec3| -> Vec<f64> {
            let Some(step) = polar_step_deg.filter(|s| *s > 1e-3) else {
                return vec![0.0, 90.0];
            };
            let d = cursor_world - origin;
            let ux = d.dot(ucs_x);
            let uy = d.dot(ucs_y);
            if ux * ux + uy * uy < 1e-24 {
                return vec![0.0];
            }
            let step = step as f64;
            let angle = uy.atan2(ux).to_degrees().rem_euclid(180.0);
            let nearest = (angle / step).round() * step;
            let mut out = vec![
                (nearest - step).rem_euclid(180.0),
                nearest.rem_euclid(180.0),
                (nearest + step).rem_euclid(180.0),
            ];
            out.sort_by(f64::total_cmp);
            out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            out
        };

        // Build candidate rays tagged by origin group so two rays sharing an
        // origin (a parallel pencil that only meets at that origin) are never
        // intersected with each other.
        struct Ray {
            origin: DVec3,
            dir: DVec3,
            group: usize,
            kind: TrackingKind,
        }
        let mut rays: Vec<Ray> = Vec::new();
        for (gi, &tp) in self.tracking_points.iter().enumerate() {
            for adeg in ray_angles(tp) {
                let ar = adeg.to_radians();
                rays.push(Ray {
                    origin: tp,
                    dir: ucs_x * ar.cos() + ucs_y * ar.sin(),
                    group: gi,
                    kind: TrackingKind::Generic,
                });
            }
            // Extension rays along the corner's own edges (world-space geometry
            // directions — already oriented, no UCS rotation). Included in the
            // single-ray set so pulling the cursor along a segment's extension
            // locks to it. (#219)
            if let Some(edirs) = self.tracking_dirs.get(gi) {
                for &d in edirs {
                    rays.push(Ray {
                        origin: tp,
                        dir: d,
                        group: gi,
                        kind: TrackingKind::Extension,
                    });
                }
            }
            // Perpendicular tracking ray through the acquired point.
            //
            // This is intentionally OTRACK-only. `tracking_dirs` continues to contain
            // only real edge directions so Extension and extended intersections are not
            // polluted by synthetic perpendicular rays. (#695)
            if self.is_on(SnapType::Perpendicular) {
                if let Some(pdirs) = self.tracking_perp_dirs.get(gi) {
                    for &d in pdirs {
                        rays.push(Ray {
                            origin: tp,
                            dir: d,
                            group: gi,
                            kind: TrackingKind::Perpendicular,
                        });
                    }
                }
            }
            // Ray from the command's base point toward this acquired corner:
            // once the first point is placed, the cursor can lock onto the line
            // joining that base point and an existing corner — the direction
            // *between* them, not only the ortho/polar axes or the corner's own
            // edges. Grouped with the corner so it never self-intersects with
            // that corner's other rays (they meet only at the corner).
            if let Some(bp) = last_point {
                let d = tp - bp;
                let len2 = d.x * d.x + d.y * d.y;
                if len2 > 1e-24 {
                    let inv = 1.0 / len2.sqrt();
                    rays.push(Ray {
                        origin: bp,
                        dir: DVec3::new(d.x * inv, d.y * inv, 0.0),
                        group: gi,
                        kind: TrackingKind::Generic,
                    });
                }
            }
        }
        // OTRACK rays come first; the auxiliary rays appended below (polar from
        // last_point, ortho axis from last_point) only participate in
        // intersection locking, never in single-ray fallback.
        let otrack_ray_count = rays.len();
        const POLAR_GROUP: usize = usize::MAX;
        const ORTHO_GROUP: usize = usize::MAX - 1;
        if let (Some(_), Some(lp)) = (polar_step_deg.filter(|s| *s > 1e-3), last_point) {
            for angle in ray_angles(lp) {
                let ar = angle.to_radians();
                rays.push(Ray {
                    origin: lp,
                    dir: ucs_x * ar.cos() + ucs_y * ar.sin(),
                    group: POLAR_GROUP,
                    kind: TrackingKind::Generic,
                });
            }
        }
        // Ortho axis rays from `last_point`, so a tracking ray crossing the
        // ortho axis locks on-axis (the useful corner-finding case). (#218)
        let ortho_lock = ortho && last_point.is_some();
        if let (true, Some(lp)) = (ortho_lock, last_point) {
            for &adeg in &[0.0_f64, 90.0] {
                let ar = adeg.to_radians();
                rays.push(Ray {
                    origin: lp,
                    dir: ucs_x * ar.cos() + ucs_y * ar.sin(),
                    group: ORTHO_GROUP,
                    kind: TrackingKind::Generic,
                });
            }
        }

        // ── Intersection lock — crossing of two vectors from distinct origins.
        let mut best_x: Option<(f32, OtrackHit)> = None;
        for i in 0..rays.len() {
            for j in (i + 1)..rays.len() {
                if rays[i].group == rays[j].group {
                    continue;
                }
                if required_crossing_ray.is_some_and(|(origin, dir)| {
                    !((rays[i].origin == origin && rays[i].dir == dir)
                        || (rays[j].origin == origin && rays[j].dir == dir))
                }) {
                    continue;
                }
                // Under an ortho lock only crossings that involve the ortho axis
                // are valid — every other crossing lies off it. (#218)
                if ortho_lock && rays[i].group != ORTHO_GROUP && rays[j].group != ORTHO_GROUP {
                    continue;
                }
                let Some(x) =
                    line_intersect_xy(rays[i].origin, rays[i].dir, rays[j].origin, rays[j].dir)
                else {
                    continue;
                };
                if required_crossing_ray.is_some_and(|(origin, dir)| {
                    let len2 = dir.x * dir.x + dir.y * dir.y;
                    let t = ((x.x - origin.x) * dir.x + (x.y - origin.y) * dir.y) / len2;
                    t < 0.05
                }) {
                    continue;
                }
                let sd = screen_dist(x);
                if sd < r && best_x.as_ref().map_or(true, |(bd, _)| sd < *bd) {
                    // Report an acquired tracking ray (not an auxiliary
                    // last_point ray) as base/dir for typed-distance entry.
                    let ot = if rays[i].group != POLAR_GROUP && rays[i].group != ORTHO_GROUP {
                        &rays[i]
                    } else {
                        &rays[j]
                    };
                    let t = (x.x - ot.origin.x) * ot.dir.x + (x.y - ot.origin.y) * ot.dir.y;
                    let dir_out = if t >= 0.0 { ot.dir } else { -ot.dir };
                    best_x = Some((
                        sd,
                        OtrackHit {
                            aligned: x,
                            dir: dir_out,
                            base: ot.origin,
                            kind: ot.kind,
                        }
                    ));
                }
            }
        }
        if let Some((_, h)) = best_x {
            return Some(h);
        }
        if required_crossing_ray.is_some() {
            return None;
        }

        // With Ortho on and a base point, the axis is a hard lock: no single
        // tracking ray may pull the cursor off it. Only crossings with the
        // ortho axis (handled above) lock; otherwise defer to the caller's
        // ortho constraint. (#218)
        if ortho_lock {
            return None;
        }

        // ── Single-ray alignment (OTRACK rays only) ──
        let mut best: Option<(f32, OtrackHit)> = None;
        for ray in rays.iter().take(otrack_ray_count) {
            let t = (cursor_world.x - ray.origin.x) * ray.dir.x
                + (cursor_world.y - ray.origin.y) * ray.dir.y;
            let aligned = DVec3::new(
                ray.origin.x + ray.dir.x * t,
                ray.origin.y + ray.dir.y * t,
                ray.origin.z,
            );
            let sd = screen_dist(aligned);
            if sd < r && best.as_ref().map_or(true, |(bd, _)| sd < *bd) {
                let dir_out = if t >= 0.0 { ray.dir } else { -ray.dir };
                best = Some((
                    sd,
                    OtrackHit {
                        aligned,
                        dir: dir_out,
                        base: ray.origin,
                        kind: ray.kind,
                    },
                ));
            }
        }
        best.map(|(_, h)| h)
    }

    /// Clear all acquired tracking points (e.g. when command ends).
    pub fn clear_tracking(&mut self) {
        self.tracking_points.clear();
        self.tracking_dirs.clear();
        self.tracking_perp_dirs.clear();
        self.parallel_ref = None;
        self.parallel_dwell = None;
        self.last_snap_world = None;
        self.dwell_since = None;
        self.dwell_acquired = false;
    }

    /// Parallel snap acquisition. When the Parallel object snap is on, hovering
    /// a line (or polyline segment) for a short dwell acquires it as the parallel
    /// reference (its direction + a point on it, which marks it on screen); the
    /// reference persists once the cursor moves off so the user can then draw
    /// parallel to it. Dwelling on the SAME reference line again removes it
    /// (toggle). Curves (circle/arc/ellipse) are ignored — "parallel to a curve"
    /// is undefined. Call on every viewport move. (#277)
    pub fn update_parallel<W: WireSource + ?Sized>(
        &mut self,
        cursor_world: Vec3,
        wires: &W,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
        now: Instant,
    ) {
        if !(self.snap_enabled && self.is_on(SnapType::Parallel)) {
            self.parallel_ref = None;
            self.parallel_dwell = None;
            return;
        }
        const PAR_DWELL_MS: u128 = 150;
        let parallel = |a: Vec3, b: Vec3| (a.x * b.x + a.y * b.y).abs() > 0.9998;
        let Some((dir, pt)) = nearest_segment(
            cursor_world,
            wires,
            view_rot,
            eye,
            bounds,
            self.osnap_radius_px,
        ) else {
            // Off all lines: drop the in-progress candidate, keep the reference.
            self.parallel_dwell = None;
            return;
        };
        // Restart the dwell when the hovered line changes (different direction,
        // or a parallel line far from the candidate's point on screen).
        let same_candidate = self.parallel_dwell.map_or(false, |(cd, cp, _, _)| {
            parallel(cd, dir)
                && screen_perp_dist(pt, cp, cd, view_rot, eye, bounds) < self.osnap_radius_px
        });
        match self.parallel_dwell {
            Some((cd, cp, since, fired)) if same_candidate => {
                if !fired && now.duration_since(since).as_millis() >= PAR_DWELL_MS {
                    // Dwelt long enough: acquire this line, or remove it if it is
                    // already the reference (hovering it a second time toggles).
                    let is_ref = self.parallel_ref.map_or(false, |(rd, rp)| {
                        parallel(rd, dir)
                            && screen_perp_dist(pt, rp, rd, view_rot, eye, bounds)
                                < self.osnap_radius_px
                    });
                    self.parallel_ref = if is_ref { None } else { Some((dir, pt)) };
                    self.parallel_dwell = Some((cd, cp, since, true));
                }
            }
            _ => self.parallel_dwell = Some((dir, pt, now, false)),
        }
    }

    /// Parallel lock: if the cursor's direction from `base` runs parallel to the
    /// acquired reference, snap the point onto the line through `base` parallel
    /// to the reference (locking when the cursor sits within the snap aperture
    /// of that line). Independent of OTRACK. (#277)
    pub fn parallel_snap(
        &self,
        cursor_world: Vec3,
        base: Option<Vec3>,
        view_rot: glam::Mat4,
        eye: glam::DVec3,
        bounds: iced::Rectangle,
    ) -> Option<SnapResult> {
        if !(self.snap_enabled && self.is_on(SnapType::Parallel)) {
            return None;
        }
        let (dir, _) = self.parallel_ref?;
        let base = base?;
        let d = cursor_world - base;
        // Need a bit of travel from the base, and the cursor must be pulling
        // roughly along the reference (not backward-only noise near the base).
        if (d.x * d.x + d.y * d.y).sqrt() < self.grid_spacing * 0.01 {
            return None;
        }
        let t = d.x * dir.x + d.y * dir.y;
        let locked = base + dir * t;
        let sl = world_to_screen(locked.as_dvec3(), view_rot, eye, bounds);
        let sc = world_to_screen(cursor_world.as_dvec3(), view_rot, eye, bounds);
        if dist2(sl, sc) > self.osnap_radius_px * self.osnap_radius_px {
            return None; // cursor not near the parallel line — don't lock
        }
        Some(SnapResult {
            world: locked.as_dvec3(),
            screen: sl,
            snap_type: SnapType::Parallel,
            tangent_obj: None,
            extension_base: None,
            extension_base2: None,
            extension_origin: None,
            extension_dir: None,
        })
    }

    /// Only runs Tangent snap — used when a command needs object picks via tangent.
    pub fn snap_tangent_only<W: WireSource + ?Sized>(
        &self,
        cursor_world: Vec3,
        cursor_screen: Point,
        wires: &W,
        view_rot: Mat4,
        eye: glam::DVec3,
        bounds: Rectangle,
    ) -> Option<SnapResult> {
        let tmp = Snapper {
            snap_enabled: true,
            enabled: {
                let mut s = HashSet::default();
                s.insert(SnapType::Tangent);
                s
            },
            grid_snap_on: false,
            grid_spacing: self.grid_spacing,
            osnap_radius_px: self.osnap_radius_px,
            otrack_enabled: false,
            tracking_points: Vec::new(),
            tracking_dirs: Vec::new(),
            tracking_perp_dirs: Vec::new(),
            last_snap_world: None,
            dwell_since: None,
            dwell_acquired: false,
            from_point: None,
            parallel_ref: None,
            parallel_dwell: None,
            override_saved: None,
        };
        // Tangent-only: Grid is disabled here, so the grid basis is irrelevant.
        tmp.snap(
            cursor_world.as_dvec3(),
            cursor_screen,
            wires,
            view_rot,
            eye,
            bounds,
            Vec3::ZERO,
            (Vec3::X, Vec3::Y, Vec3::Z),
            None,
        )
    }

    /// Find the best snap candidate near the cursor.
    pub fn snap<W: WireSource + ?Sized>(
        &self,
        cursor_world: glam::DVec3,
        cursor_screen: Point,
        wires: &W,
        view_rot: Mat4,
        eye: glam::DVec3,
        bounds: Rectangle,
        // Grid origin and live drafting axes, so grid snap lands on the same
        // rotated or isometric grid the user sees.
        grid_origin: Vec3,
        grid_axes: (Vec3, Vec3, Vec3),
        construction_ray: Option<(glam::DVec3, glam::DVec3)>,
    ) -> Option<SnapResult> {
        // Object-snap selection is priority-then-distance, NOT nearest-wins.
        // "Continuous" snaps (Nearest, Perpendicular, …) sit on the geometry
        // and are therefore almost always closer to the cursor than a discrete
        // Endpoint/Midpoint/Center, so a pure-distance pick would let them mask
        // every other enabled snap. Instead a higher-priority snap inside the
        // snap circle wins even when a lower-priority one is closer; distance
        // only breaks ties within the same priority. See #118.
        let radius2 = self.osnap_radius_px * self.osnap_radius_px;
        let mut best: Option<SnapResult> = None;
        let mut best_rank = u8::MAX;
        let mut best_sub = u8::MAX;
        let mut best_d2 = f32::MAX;

        // Reject candidates projecting outside the pane rectangle. The GPU
        // scissors viewport content to exactly `bounds`, but the hit-test wire
        // set reaches past it (the cull keeps a margin and lines run beyond the
        // rect), so without this a snap could land on geometry clipped out of
        // the viewport. `bounds` is the full canvas in model space, so this is a
        // no-op there.
        let in_bounds = |s: Point| -> bool {
            s.x >= 0.0 && s.x <= bounds.width && s.y >= 0.0 && s.y <= bounds.height
        };

        // ── Grid snap — a SEPARATE system from object snap ───────────────────
        // Grid snap has its own toggle (`grid_snap_on`) and is independent of
        // the object-snap master (`snap_enabled`) and the object-snap mode set.
        // Object snaps therefore NEVER catch grid points; only when grid snap is
        // on can a grid corner be picked. It is evaluated first and at the
        // lowest priority, so any object snap inside the aperture overrides it.
        if self.grid_snap_on {
            let s = self.grid_spacing as f64;
            if s.abs() > 1e-9 {
                // Round in the UCS grid frame, then map back to world.
                let (ax, ay, az) = grid_axes;
                let ax = ax.normalize_or(Vec3::X).as_dvec3();
                let ay = ay.normalize_or(Vec3::Y).as_dvec3();
                let az = az.normalize_or(Vec3::Z).as_dvec3();
                let origin = grid_origin.as_dvec3();
                let rel = cursor_world - origin;
                // The isometric pairs are oblique, so dot products alone are
                // not coordinates. Invert their 2×2 Gram matrix before rounding.
                let aa = ax.dot(ax);
                let ab = ax.dot(ay);
                let bb = ay.dot(ay);
                let det = aa * bb - ab * ab;
                let (ux, uy) = if det.abs() > 1e-9 {
                    let ra = rel.dot(ax);
                    let rb = rel.dot(ay);
                    ((ra * bb - rb * ab) / det, (rb * aa - ra * ab) / det)
                } else {
                    (rel.dot(ax), rel.dot(ay))
                };
                let ux = (ux / s).round() * s;
                let uy = (uy / s).round() * s;
                let uz = (rel.dot(az) / s).round() * s;
                let gp = origin + ax * ux + ay * uy + az * uz;
                let screen = world_to_screen(gp, view_rot, eye, bounds);
                let d2 = dist2(screen, cursor_screen);
                if d2 < radius2 && in_bounds(screen) {
                    best = Some(SnapResult {
                        world: gp,
                        screen,
                        snap_type: SnapType::Grid,
                        tangent_obj: None,
                        extension_base: None,
                        extension_base2: None,
                        extension_origin: None,
                        extension_dir: None,
                    });
                    best_rank = snap_tier(SnapType::Grid);
                    best_sub = snap_priority(SnapType::Grid);
                    best_d2 = d2;
                }
            }
        }

        // Object snaps are gated by the object-snap master toggle. With it off
        // only the grid result (if any) stands.
        if !self.snap_enabled {
            return best;
        }

        // World-space snap radius — derived from the view scale so wires whose
        // entire extent is clearly outside the snap circle can be skipped cheaply
        // before projecting any of their vertices to screen space.
        // view_proj col-0 x = 2*zoom / viewport_width for an orthographic camera,
        // so scale_x * (width/2) = pixels per world unit.
        let world_snap_r = {
            let s = view_rot.col(0).x.abs() * bounds.width * 0.5;
            if s > 1e-6 {
                self.osnap_radius_px / s
            } else {
                f32::MAX
            }
        };

        // Returns false when the wire's AABB does not overlap the snap circle —
        // safe to skip all vertex work for this wire.
        // UNBOUNDED_AABB (±infinity) passes through automatically without a
        // special-case branch because the arithmetic is exact for infinities.
        let wire_in_range = |wire: &WireModel| -> bool {
            // The AABB is stored in f32, so at UTM-scale coordinates each bound
            // is quantized by up to ~1 ulp (≈ coord × 2⁻²³ ≈ 0.7 m at 5.7e6).
            // When zoomed in hard the snap radius shrinks below that, so the
            // raw f32 bound can wrongly exclude a wire the cursor is on. Pad the
            // test by the bound's own quantization so the cull never rejects a
            // genuinely in-range wire (it only ever over-includes, which the
            // per-vertex screen test below then rejects precisely).
            let mag = wire.aabb.iter().fold(0.0f32, |m, c| m.max(c.abs()));
            let pad = (mag * f32::EPSILON * 2.0) as f64;
            let r = world_snap_r as f64 + pad;
            cursor_world.x + r >= wire.aabb[0] as f64
                && cursor_world.x - r <= wire.aabb[2] as f64
                && cursor_world.y + r >= wire.aabb[1] as f64
                && cursor_world.y - r <= wire.aabb[3] as f64
        };

        // The per-wire snaps below skip out-of-aperture wires via `wire_in_range`,
        // so they stay O(n) and snap at any zoom. Intersection / ApparentIntersection
        // instead compare pairs of in-range segments — O(k²) — which hangs when the
        // aperture spans the whole drawing. Count the in-range wires once and gate
        // only those pairwise passes; the single-wire snaps always run.
        // Intersection / ApparentIntersection compare every in-range segment
        // against every other — O(total in-range segments²). Gate on the
        // in-range *point* (≈ segment) count, not the wire count: a handful of
        // curved wires, each hundreds of tessellated points, blows the
        // quadratic up even though the wire count looks modest. (A dense survey
        // cursor cell held ~1k wires / ~240k points → 15 s pre-gate.)
        const MAX_PAIRWISE_POINTS: usize = 3_000;
        let mut in_range_pts = 0usize;
        if wires.segments().is_none() {
            for w in wires.iter().filter(|w| wire_in_range(w)) {
                in_range_pts += w.points.len();
                if in_range_pts > MAX_PAIRWISE_POINTS {
                    break;
                }
            }
        }
        let allow_unindexed_pairwise = in_range_pts <= MAX_PAIRWISE_POINTS;
        let local_segments = indexed_segments(wires);

        let mut try_pt = |world: glam::DVec3, snap_type: SnapType| {
            let screen = world_to_screen(world, view_rot, eye, bounds);
            if !in_bounds(screen) {
                return;
            }
            let d2 = dist2(screen, cursor_screen);
            // `!(d2 < radius2)` (not `d2 >= radius2`) so a NaN distance from
            // degenerate geometry is rejected: with priority selection a NaN
            // would otherwise pass the gate and be chosen on rank alone,
            // feeding a NaN snap point to the renderer. (#118)
            if !(d2 < radius2) {
                return;
            }
            let (tier, sub) = (snap_tier(snap_type), snap_priority(snap_type));
            if snap_better(tier, d2, sub, (best_rank, best_d2, best_sub)) {
                best_rank = tier;
                best_sub = sub;
                best_d2 = d2;
                best = Some(SnapResult {
                    world,
                    screen,
                    snap_type,
                    tangent_obj: None,
                    extension_base: None,
                    extension_base2: None,
                    extension_origin: None,
                    extension_dir: None,
                });
            }
        };

        // ── Pre-baked snap points (Center, Node, Quadrant, Insertion) ──────
        let mut try_snap_hint = |world: DVec3, hint: SnapHint| {
            let snap_type = match hint {
                SnapHint::Center => SnapType::Center,
                SnapHint::Node => SnapType::Node,
                SnapHint::Quadrant => SnapType::Quadrant,
                SnapHint::Insertion => SnapType::Insertion,
                SnapHint::Midpoint => SnapType::Midpoint,
                SnapHint::Endpoint => SnapType::Endpoint,
            };
            if self.is_on(snap_type) {
                try_pt(world, snap_type);
            }
        };
        if let Some(points) = wires.snap_points() {
            for point_ref in points {
                let Some(&(world, hint)) = wires
                    .source_wire(point_ref.wire)
                    .and_then(|wire| wire.snap_pts.get(point_ref.index as usize))
                else {
                    continue;
                };
                try_snap_hint(world, hint);
            }
        } else {
            for wire in wires.iter() {
                for &(world, hint) in &wire.snap_pts {
                    try_snap_hint(world, hint);
                }
            }
        }
        drop(try_snap_hint);

        // ── Endpoint ───────────────────────────────────────────────────────
        if self.is_on(SnapType::Endpoint) {
            if let Some(vertices) = wires.key_vertices() {
                for vertex_ref in vertices {
                    let Some(&point) = wires
                        .source_wire(vertex_ref.wire)
                        .and_then(|wire| wire.key_vertices.get(vertex_ref.index as usize))
                    else {
                        continue;
                    };
                    try_pt(DVec3::from_array(point), SnapType::Endpoint);
                }
                // Tessellated open curves have no key-vertex set. Their only
                // endpoints are first/last, so testing candidate wires remains
                // O(local wires), independent of tessellation density.
                for wire in wires.iter().filter(|wire| wire.key_vertices.is_empty()) {
                    let closed = wire
                        .snap_pts
                        .iter()
                        .any(|(_, hint)| matches!(hint, SnapHint::Quadrant));
                    if closed {
                        continue;
                    }
                    if !wire.points.is_empty() {
                        try_pt(wp_f64(wire, 0), SnapType::Endpoint);
                    }
                    if wire.points.len() > 1 {
                        try_pt(wp_f64(wire, wire.points.len() - 1), SnapType::Endpoint);
                    }
                }
            } else {
                for wire in wires.iter() {
                    if !wire_in_range(wire) {
                        continue;
                    }
                    if !wire.key_vertices.is_empty() {
                        for &point in &wire.key_vertices {
                            try_pt(DVec3::from_array(point), SnapType::Endpoint);
                        }
                    } else {
                        let closed = wire
                            .snap_pts
                            .iter()
                            .any(|(_, hint)| matches!(hint, SnapHint::Quadrant));
                        if !closed {
                            if !wire.points.is_empty() {
                                try_pt(wp_f64(wire, 0), SnapType::Endpoint);
                            }
                            if wire.points.len() > 1 {
                                try_pt(wp_f64(wire, wire.points.len() - 1), SnapType::Endpoint);
                            }
                        }
                    }
                }
            }
        }

        // ── Midpoint ───────────────────────────────────────────────────────
        // Tessellated curves contribute their pre-baked Midpoint above.
        if self.is_on(SnapType::Midpoint) {
            if let Some(segments) = wires.key_segments() {
                for segment_ref in segments {
                    let Some(wire) = wires.source_wire(segment_ref.wire) else {
                        continue;
                    };
                    let start = segment_ref.start as usize;
                    let Some((&a, &b)) = wire
                        .key_vertices
                        .get(start)
                        .zip(wire.key_vertices.get(start + 1))
                    else {
                        continue;
                    };
                    let a = DVec3::from_array(a);
                    let b = DVec3::from_array(b);
                    if a.distance_squared(b) > 1e-12 {
                        try_pt((a + b) * 0.5, SnapType::Midpoint);
                    }
                }
            } else {
                for wire in wires.iter().filter(|wire| wire_in_range(wire)) {
                    for segment in wire.key_vertices.windows(2) {
                        let a = DVec3::from_array(segment[0]);
                        let b = DVec3::from_array(segment[1]);
                        if a.distance_squared(b) > 1e-12 {
                            try_pt((a + b) * 0.5, SnapType::Midpoint);
                        }
                    }
                }
            }
        }

        // ── Nearest — closest point on any segment (clamped) ──────────────
        if self.is_on(SnapType::Nearest) {
            if let Some(segments) = &local_segments {
                for seg in segments {
                    try_pt(
                        nearest_on_segment(cursor_world, seg.a, seg.b),
                        SnapType::Nearest,
                    );
                }
            } else {
                for wire in wires.iter() {
                    if !wire_in_range(wire) {
                        continue;
                    }
                    for i in 0..wire.points.len().saturating_sub(1) {
                        let p =
                            nearest_on_segment(cursor_world, wp_f64(wire, i), wp_f64(wire, i + 1));
                        try_pt(p, SnapType::Nearest);
                    }
                }
            }
        }

        // ── Perpendicular — foot of perpendicular from the drawing base ──
        // Perpendicular requires a drawing base; cursor fallback acts like Nearest. (#716)
        if self.is_on(SnapType::Perpendicular) {
            if let Some(q) = self.from_point.map(|v| v.as_dvec3()) {
                if let Some(segments) = &local_segments {
                    for seg in segments {
                        if let Some(foot) = perp_foot(q, seg.a, seg.b) {
                            try_pt(foot, SnapType::Perpendicular);
                        }
                    }
                } else {
                    for wire in wires.iter() {
                        if !wire_in_range(wire) {
                            continue;
                        }
                        for i in 0..wire.points.len().saturating_sub(1) {
                            if let Some(foot) = perp_foot(q, wp_f64(wire, i), wp_f64(wire, i + 1)) {
                                try_pt(foot, SnapType::Perpendicular);
                            }
                        }
                    }
                }
            }
        }

        let mut try_ray_intersections = |origin: glam::DVec3, through: glam::DVec3| {
            if (through - origin).length_squared() <= 1e-18 {
                return;
            }
            if let Some(segments) = &local_segments {
                for segment in segments {
                    if let Some(point) =
                        ray_segment_intersect_3d(origin, through, segment.a, segment.b)
                    {
                        if (point - origin).length_squared() > 1e-18 {
                            try_pt(point, SnapType::Intersection);
                        }
                    }
                }
            } else {
                for wire in wires.iter().filter(|wire| wire_in_range(wire)) {
                    for index in 0..wire.points.len().saturating_sub(1) {
                        if let Some(point) = ray_segment_intersect_3d(
                            origin,
                            through,
                            wp_f64(wire, index),
                            wp_f64(wire, index + 1),
                        ) {
                            if (point - origin).length_squared() > 1e-18 {
                                try_pt(point, SnapType::Intersection);
                            }
                        }
                    }
                }
            }
        };
        // Check engaged extension tracking rays against nearby real geometry.
        // OTRACK can align the cursor to an acquired segment extension even when
        // Extension OSNAP itself is not the current snap result. Allow Intersection
        // to stop that active extension ray where it crosses drawing geometry.
        if self.is_on(SnapType::Intersection) && self.otrack_enabled {
            for (&origin, dirs) in self.tracking_points.iter().zip(&self.tracking_dirs) {
                for &dir in dirs {
                    // Only test the extension ray while the cursor is actually
                    // engaged with it, so Intersection does not become Nearest.
                    if extension_snap(
                        cursor_world,
                        origin,
                        dir,
                        view_rot,
                        eye,
                        bounds,
                        self.osnap_radius_px,
                    )
                    .is_some()
                    {
                        try_ray_intersections(origin, origin + dir);
                    }
                }
            }
        }
        if self.is_on(SnapType::Intersection) {
            if let Some((origin, through)) = construction_ray {
                try_ray_intersections(origin, through);
            }
        }

        // Check engaged perpendicular tracking rays against nearby geometry.
        if self.is_on(SnapType::Intersection)
            && self.otrack_enabled
            && self.is_on(SnapType::Perpendicular)
        {
            for (gi, &origin) in self.tracking_points.iter().enumerate() {
                let Some(perp_dirs) = self.tracking_perp_dirs.get(gi) else {
                    continue;
                };

                for &dir in perp_dirs {
                    let rel = cursor_world - origin;
                    let t = rel.x * dir.x + rel.y * dir.y;
                    let aligned = glam::DVec3::new(
                        origin.x + dir.x * t,
                        origin.y + dir.y * t,
                        origin.z,
                    );
                    let aligned_screen = world_to_screen(aligned, view_rot, eye, bounds);
                    if dist2(aligned_screen, cursor_screen) > radius2 {
                        continue;
                    }
                    let dir_to_cursor = if t >= 0.0 { dir } else { -dir };
                    try_ray_intersections(origin, origin + dir_to_cursor);
                }
            }
        }
        drop(try_ray_intersections);

        // ── Intersection — segment-segment intersections (pairwise, gated) ──
        if self.is_on(SnapType::Intersection)
            && (local_segments.is_some() || allow_unindexed_pairwise)
        {
            if let Some(segments) = &local_segments {
                // Exact cursor-local sweep: never discard a valid intersection
                // in dense geometry. Min-X ordering plus Y overlap avoids
                // comparing segment pairs whose bounds cannot meet.
                let mut local: Vec<&IndexedSegment> = segments.iter().collect();
                local.sort_by(|a, b| a.min_x().total_cmp(&b.min_x()));
                if indexed_sweep_within_budget(&local) {
                    'intersection_sweep: for i in 0..local.len() {
                        let a = local[i];
                        for &b in local.iter().skip(i + 1) {
                            if b.min_x() > a.max_x() {
                                break;
                            }
                            if a.wire == b.wire || a.max_y() < b.min_y() || a.min_y() > b.max_y() {
                                continue;
                            }
                            if let Some(pt) = seg_intersect_3d(a.a, a.b, b.a, b.b) {
                                let exact_cursor = dist2(
                                    world_to_screen(pt, view_rot, eye, bounds),
                                    cursor_screen,
                                ) <= f32::EPSILON;
                                try_pt(pt, SnapType::Intersection);
                                if exact_cursor {
                                    break 'intersection_sweep;
                                }
                            }
                        }
                    }
                }
            } else {
                for i in 0..wires.len() {
                    let Some(wire_i) = wires.get(i) else {
                        continue;
                    };
                    if !wire_in_range(wire_i) {
                        continue;
                    }
                    for j in (i + 1)..wires.len() {
                        let Some(wire_j) = wires.get(j) else {
                            continue;
                        };
                        if !wire_in_range(wire_j) {
                            continue;
                        }
                        for ai in 0..wire_i.points.len().saturating_sub(1) {
                            // S: pre-convert outside inner loop
                            let a0 = wp_f64(wire_i, ai);
                            let a1 = wp_f64(wire_i, ai + 1);
                            let a_min_x = a0.x.min(a1.x);
                            let a_max_x = a0.x.max(a1.x);
                            let a_min_y = a0.y.min(a1.y);
                            let a_max_y = a0.y.max(a1.y);
                            for bi in 0..wire_j.points.len().saturating_sub(1) {
                                let b0 = wp_f64(wire_j, bi);
                                let b1 = wp_f64(wire_j, bi + 1);
                                // O: tight per-segment AABB overlap cull
                                if a_max_x < b0.x.min(b1.x)
                                    || a_min_x > b0.x.max(b1.x)
                                    || a_max_y < b0.y.min(b1.y)
                                    || a_min_y > b0.y.max(b1.y)
                                {
                                    continue;
                                }
                                if let Some(pt) = seg_intersect_3d(a0, a1, b0, b1) {
                                    try_pt(pt, SnapType::Intersection);
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Extension — persistent rays captured at endpoint acquisition ──
        // The source geometry may be far outside the cursor's spatial query by
        // the time its extension is used. Keep the outward incident directions
        // captured with each tracking point instead of rescanning either the
        // whole drawing or only the now-local candidates.
        if self.is_on(SnapType::Extension) && !self.tracking_points.is_empty() {
            for (&origin, dirs) in self.tracking_points.iter().zip(&self.tracking_dirs) {
                for &dir in dirs {
                    if let Some(ext) = extension_snap(
                        cursor_world,
                        origin,
                        dir,
                        view_rot,
                        eye,
                        bounds,
                        self.osnap_radius_px,
                    ) {
                        try_pt(ext, SnapType::Extension);
                    }
                }
            }

            // Extended intersection: intersect only outward rays whose infinite
            // lines pass near the cursor. At most four acquired points × six
            // directions participate, independent of drawing density.
            let filter2 = (self.osnap_radius_px * 2.0).powi(2);
            let mut cand: Vec<(glam::DVec3, glam::DVec3)> = Vec::new();
            for (&origin, dirs) in self.tracking_points.iter().zip(&self.tracking_dirs) {
                for &dir in dirs {
                    let s0 = world_to_screen(origin, view_rot, eye, bounds);
                    let s1 = world_to_screen(origin + dir, view_rot, eye, bounds);
                    let ex = s1.x - s0.x;
                    let ey = s1.y - s0.y;
                    let l2 = ex * ex + ey * ey;
                    if l2 < 1e-6 {
                        continue;
                    }
                    // Perpendicular screen distance² from the cursor to the line.
                    let cross = ex * (cursor_screen.y - s0.y) - ey * (cursor_screen.x - s0.x);
                    if cross * cross / l2 <= filter2 {
                        cand.push((origin, dir));
                    }
                }
            }
            for i in 0..cand.len() {
                let (a0, d1) = cand[i];
                for &(b0, d2) in cand.iter().skip(i + 1) {
                    if (a0 - b0).length_squared() < 1e-18 {
                        continue;
                    }
                    let denom = d1.x * d2.y - d1.y * d2.x;
                    if denom.abs() < 1e-12 {
                        continue; // parallel
                    }
                    let t1 = ((b0.x - a0.x) * d2.y - (b0.y - a0.y) * d2.x) / denom;
                    let t2 = ((b0.x - a0.x) * d1.y - (b0.y - a0.y) * d1.x) / denom;
                    if t1 < 0.05 || t2 < 0.05 {
                        continue;
                    }
                    // Emit as an Intersection, not an Extension: the crossing is
                    // a distinct point and must outrank the per-segment extension
                    // feet (which sit closer to the cursor on their own lines),
                    // or the cursor would snap to a line instead of the crossing.
                    let pt = glam::DVec3::new(a0.x + t1 * d1.x, a0.y + t1 * d1.y, a0.z);
                    try_pt(pt, SnapType::Intersection);
                }
            }
        }

        // ── Apparent Intersection — screen-space intersections (pairwise, gated) ──
        // L: pre-project each in-range wire's points to screen once, not once per segment pair.
        if self.is_on(SnapType::ApparentIntersection)
            && (local_segments.is_some() || allow_unindexed_pairwise)
        {
            if let Some(segments) = &local_segments {
                // Project once, then run an exact screen-AABB sweep. Unlike the
                // previous 512-item cap this cannot drop the true apparent
                // intersection in a dense aperture.
                let mut projected: Vec<(&IndexedSegment, Point, Point, [f32; 4])> = segments
                    .iter()
                    .map(|seg| {
                        let a = world_to_screen(seg.a, view_rot, eye, bounds);
                        let b = world_to_screen(seg.b, view_rot, eye, bounds);
                        (
                            seg,
                            a,
                            b,
                            [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)],
                        )
                    })
                    .collect();
                projected.sort_by(|a, b| a.3[0].total_cmp(&b.3[0]));
                if projected_sweep_within_budget(&projected) {
                    'apparent_sweep: for i in 0..projected.len() {
                        let (segment_a, screen_a0, screen_a1, aabb_a) = projected[i];
                        for &(segment_b, screen_b0, screen_b1, aabb_b) in
                            projected.iter().skip(i + 1)
                        {
                            if aabb_b[0] > aabb_a[2] {
                                break;
                            }
                            if segment_a.wire == segment_b.wire
                                || aabb_a[3] < aabb_b[1]
                                || aabb_a[1] > aabb_b[3]
                            {
                                continue;
                            }
                            if let Some((ta, _)) =
                                seg_intersect_2d(screen_a0, screen_a1, screen_b0, screen_b1)
                            {
                                let apparent_screen = Point::new(
                                    screen_a0.x + ta * (screen_a1.x - screen_a0.x),
                                    screen_a0.y + ta * (screen_a1.y - screen_a0.y),
                                );
                                try_pt(
                                    segment_a.a + ta as f64 * (segment_a.b - segment_a.a),
                                    SnapType::ApparentIntersection,
                                );
                                if dist2(apparent_screen, cursor_screen) <= f32::EPSILON {
                                    break 'apparent_sweep;
                                }
                            }
                        }
                    }
                }
            } else {
                let screen_pts: Vec<Option<Vec<Point>>> = wires
                    .iter()
                    .map(|w| {
                        if !wire_in_range(w) {
                            return None;
                        }
                        Some(
                            (0..w.points.len())
                                .map(|i| world_to_screen(wp_f64(w, i), view_rot, eye, bounds))
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();

                for i in 0..wires.len() {
                    let Some(ref si) = screen_pts[i] else {
                        continue;
                    };
                    let Some(wire_i) = wires.get(i) else {
                        continue;
                    };
                    for j in (i + 1)..wires.len() {
                        let Some(ref sj) = screen_pts[j] else {
                            continue;
                        };
                        let Some(wire_j) = wires.get(j) else {
                            continue;
                        };
                        for ai in 0..wire_i.points.len().saturating_sub(1) {
                            let sa0 = si[ai];
                            let sa1 = si[ai + 1];
                            for bi in 0..wire_j.points.len().saturating_sub(1) {
                                let sb0 = sj[bi];
                                let sb1 = sj[bi + 1];
                                if let Some((ta, _)) = seg_intersect_2d(sa0, sa1, sb0, sb1) {
                                    let wa0 = wp_f64(wire_i, ai);
                                    let wa1 = wp_f64(wire_i, ai + 1);
                                    try_pt(
                                        wa0 + ta as f64 * (wa1 - wa0),
                                        SnapType::ApparentIntersection,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // ── Tangent ────────────────────────────────────────────────────────
        // Operates directly on tangent_geoms geometry — independent of the
        // wire.points rendering structure so polyline segments work correctly.
        if self.is_on(SnapType::Tangent) {
            for wire in wires.iter() {
                for tg in &wire.tangent_geoms {
                    let (world_pt, d2) = match tg {
                        TangentGeom::Line { p1, p2 } => {
                            let sp0 = world_to_screen(
                                glam::DVec3::new(p1[0] as f64, p1[1] as f64, p1[2] as f64),
                                view_rot,
                                eye,
                                bounds,
                            );
                            let sp1 = world_to_screen(
                                glam::DVec3::new(p2[0] as f64, p2[1] as f64, p2[2] as f64),
                                view_rot,
                                eye,
                                bounds,
                            );
                            let d2 = dist2_to_segment(cursor_screen, sp0, sp1);
                            let t = t_on_segment(cursor_screen, sp0, sp1);
                            let w = Vec3::from(*p1) + t * (Vec3::from(*p2) - Vec3::from(*p1));
                            (w.as_dvec3(), d2)
                        }
                        TangentGeom::Circle { center, radius } => {
                            let cv = Vec3::from(*center);
                            let r = *radius;
                            let sc = world_to_screen(cv.as_dvec3(), view_rot, eye, bounds);
                            let rim = world_to_screen(
                                glam::DVec3::new((cv.x + r) as f64, cv.y as f64, cv.z as f64),
                                view_rot,
                                eye,
                                bounds,
                            );
                            let sr = dist2(sc, rim).sqrt();
                            let dc = dist2(cursor_screen, sc).sqrt();
                            let edge_d = (dc - sr).abs();
                            let w = if let Some(from) = self.from_point {
                                let Some((t0, t1)) = circle_tangent_points(from, cv, r) else {
                                    continue;
                                };
                                let s0 = world_to_screen(t0.as_dvec3(), view_rot, eye, bounds);
                                let s1 = world_to_screen(t1.as_dvec3(), view_rot, eye, bounds);
                                if dist2(s0, cursor_screen) <= dist2(s1, cursor_screen) {
                                    t0
                                } else {
                                    t1
                                }
                            } else {
                                let dx = cursor_screen.x - sc.x;
                                let dy = cursor_screen.y - sc.y;
                                let dl = (dx * dx + dy * dy).sqrt();
                                let (nx, ny) = if dl > 1e-6 {
                                    (dx / dl, -dy / dl)
                                } else {
                                    (1.0, 0.0)
                                };
                                Vec3::new(cv.x + r * nx, cv.y + r * ny, cv.z)
                            };
                            (w.as_dvec3(), edge_d * edge_d)
                        }
                        TangentGeom::Arc {
                            center,
                            axis_x,
                            axis_y,
                            radius,
                            start_angle,
                            end_angle,
                        } => {
                            let Some(from) = self.from_point else {
                                continue;
                            };
                            let Some(world) = arc_tangent_points(
                                from.as_dvec3(),
                                *center,
                                *axis_x,
                                *axis_y,
                                *radius,
                                *start_angle,
                                *end_angle,
                            )
                            .into_iter()
                            .min_by(|a, b| {
                                let sa = world_to_screen(*a, view_rot, eye, bounds);
                                let sb = world_to_screen(*b, view_rot, eye, bounds);
                                dist2(sa, cursor_screen).total_cmp(&dist2(sb, cursor_screen))
                            }) else {
                                continue;
                            };
                            let edge_d2 = wire
                                .points
                                .windows(2)
                                .enumerate()
                                .filter_map(|(index, _)| {
                                    let a = wp_f64(wire, index);
                                    let b = wp_f64(wire, index + 1);
                                    (a.is_finite() && b.is_finite()).then(|| {
                                        dist2_to_segment(
                                            cursor_screen,
                                            world_to_screen(a, view_rot, eye, bounds),
                                            world_to_screen(b, view_rot, eye, bounds),
                                        )
                                    })
                                })
                                .fold(f32::INFINITY, f32::min);
                            (world, edge_d2)
                        }
                        TangentGeom::PlanarCircle {
                            center,
                            axis_x,
                            axis_y,
                            radius,
                        } => {
                            let cv = DVec3::from_array(*center);
                            let candidate = self.from_point.and_then(|from| {
                                planar_circle_tangent_points(
                                    from.as_dvec3(),
                                    *center,
                                    *axis_x,
                                    *axis_y,
                                    *radius,
                                )
                                .into_iter()
                                .min_by(|a, b| {
                                    let sa = world_to_screen(*a, view_rot, eye, bounds);
                                    let sb = world_to_screen(*b, view_rot, eye, bounds);
                                    dist2(sa, cursor_screen).total_cmp(&dist2(sb, cursor_screen))
                                })
                            });
                            if self.from_point.is_some() && candidate.is_none() {
                                continue;
                            }
                            let fallback = || {
                                (0..wire.points.len())
                                    .map(|index| wp_f64(wire, index))
                                    .filter(|point| point.is_finite())
                                    .min_by(|a, b| {
                                        let sa = world_to_screen(*a, view_rot, eye, bounds);
                                        let sb = world_to_screen(*b, view_rot, eye, bounds);
                                        dist2(sa, cursor_screen)
                                            .total_cmp(&dist2(sb, cursor_screen))
                                    })
                                    .unwrap_or(cv)
                            };
                            let world = candidate.unwrap_or_else(fallback);
                            let edge_d2 = wire
                                .points
                                .windows(2)
                                .enumerate()
                                .filter_map(|(index, _)| {
                                    let a = wp_f64(wire, index);
                                    let b = wp_f64(wire, index + 1);
                                    (a.is_finite() && b.is_finite()).then(|| {
                                        dist2_to_segment(
                                            cursor_screen,
                                            world_to_screen(a, view_rot, eye, bounds),
                                            world_to_screen(b, view_rot, eye, bounds),
                                        )
                                    })
                                })
                                .fold(f32::INFINITY, f32::min);
                            (world, edge_d2)
                        }
                    };
                    let (tier, sub) = (
                        snap_tier(SnapType::Tangent),
                        snap_priority(SnapType::Tangent),
                    );
                    let screen_pt = world_to_screen(world_pt, view_rot, eye, bounds);
                    if d2 < radius2
                        && in_bounds(screen_pt)
                        && snap_better(tier, d2, sub, (best_rank, best_d2, best_sub))
                    {
                        best_rank = tier;
                        best_sub = sub;
                        best_d2 = d2;
                        let tangent_obj = match tg {
                            TangentGeom::Line { p1, p2 } => TangentObject::Line {
                                p1: glam::DVec3::new(p1[0] as f64, p1[1] as f64, p1[2] as f64),
                                p2: glam::DVec3::new(p2[0] as f64, p2[1] as f64, p2[2] as f64),
                            },
                            TangentGeom::Circle { center, radius } => TangentObject::Circle {
                                center: glam::DVec3::new(
                                    center[0] as f64,
                                    center[1] as f64,
                                    center[2] as f64,
                                ),
                                radius: *radius as f64,
                            },
                            TangentGeom::Arc { center, radius, .. } => TangentObject::Circle {
                                center: glam::DVec3::from_array(*center),
                                radius: *radius,
                            },
                            TangentGeom::PlanarCircle { center, radius, .. } => {
                                TangentObject::Circle {
                                    center: glam::DVec3::from_array(*center),
                                    radius: *radius,
                                }
                            }
                        };
                        best = Some(SnapResult {
                            world: world_pt,
                            screen: screen_pt,
                            snap_type: SnapType::Tangent,
                            tangent_obj: Some(tangent_obj),
                            extension_base: None,
                            extension_base2: None,
                            extension_origin: None,
                            extension_dir: None,
                        });
                    }
                }
            }
        }

        // ── Center via curve proximity ─────────────────────────────────────
        // A circle/arc/ellipse's centre is offset from its curve — for an arc
        // it usually sits in empty space well off the geometry — so gating the
        // Center snap purely on the cursor's distance to the centre *point*
        // (the pre-baked pass above) means hovering the curve, the natural
        // gesture, never offers it. Mirror running-osnap behaviour: when the
        // cursor is near such a curve, offer its centre, ranked by how close
        // the cursor is to the curve. Runs here, after `try_pt`'s borrow ends,
        // so it can update the candidate state directly. (#152)
        if self.is_on(SnapType::Center) {
            let mut offer = |wire: &WireModel, curve_d2: f32| {
                let Some(center) = wire
                    .snap_pts
                    .iter()
                    .find(|(_, hint)| matches!(hint, SnapHint::Center))
                    .map(|&(c, _)| c)
                else {
                    return;
                };
                let screen = world_to_screen(center, view_rot, eye, bounds);
                // Rim-hover Center: the cursor is on the CURVE, not near the
                // centre, so its distance metric (curve_d2 ≈ 0 anywhere on the
                // rim) must not compete inside the discrete tier — it would
                // mask the circle's own Quadrants at every rim position
                // (#420). Tier 1 sits below every discrete point snap and
                // above the continuous ones; the pre-baked Center point (true
                // cursor-to-centre distance) stays in tier 0.
                let (tier, sub) = (1u8, snap_priority(SnapType::Center));
                if curve_d2 < radius2
                    && in_bounds(screen)
                    && snap_better(tier, curve_d2, sub, (best_rank, best_d2, best_sub))
                {
                    best_rank = tier;
                    best_sub = sub;
                    best_d2 = curve_d2;
                    best = Some(SnapResult {
                        world: center,
                        screen,
                        snap_type: SnapType::Center,
                        tangent_obj: None,
                        extension_base: None,
                        extension_base2: None,
                        extension_origin: None,
                        extension_dir: None,
                    });
                }
            };
            if let Some(segments) = &local_segments {
                let mut distances: HashMap<u32, f32> = HashMap::default();
                for segment in segments {
                    let Some(wire) = wires.source_wire(segment.wire) else {
                        continue;
                    };
                    if !wire
                        .snap_pts
                        .iter()
                        .any(|(_, hint)| matches!(hint, SnapHint::Center))
                    {
                        continue;
                    }
                    let nearest = nearest_on_segment(cursor_world, segment.a, segment.b);
                    let distance = dist2(
                        world_to_screen(nearest, view_rot, eye, bounds),
                        cursor_screen,
                    );
                    distances
                        .entry(segment.wire)
                        .and_modify(|best| *best = best.min(distance))
                        .or_insert(distance);
                }
                for (wire_idx, distance) in distances {
                    if let Some(wire) = wires.source_wire(wire_idx) {
                        offer(wire, distance);
                    }
                }
            } else {
                for wire in wires.iter().filter(|wire| wire_in_range(wire)) {
                    let mut curve_d2 = f32::INFINITY;
                    for index in 0..wire.points.len().saturating_sub(1) {
                        let nearest = nearest_on_segment(
                            cursor_world,
                            wp_f64(wire, index),
                            wp_f64(wire, index + 1),
                        );
                        curve_d2 = curve_d2.min(dist2(
                            world_to_screen(nearest, view_rot, eye, bounds),
                            cursor_screen,
                        ));
                    }
                    offer(wire, curve_d2);
                }
            }
        }

        // If an Extension snap or an extended intersection won, re-find the
        // endpoint(s) whose ray(s) it lies on so the overlay can draw the dashed
        // guide line(s) back to them. An Extension yields one base; an extended
        // intersection yields both crossing extensions. A genuine on-segment
        // intersection yields none, so its guides simply don't draw. (#238, #247, #259)
        if let Some(b) = best.as_mut() {
            if matches!(b.snap_type, SnapType::Extension | SnapType::Intersection) {
                let (b1, b2, ray) = extension_bases_screen(
                    b.world,
                    &self.tracking_points,
                    &self.tracking_dirs,
                    view_rot,
                    eye,
                    bounds,
                );
                b.extension_base = b1;
                b.extension_base2 = b2;
                b.extension_origin = ray.map(|(origin, _)| origin);
                b.extension_dir = ray.map(|(_, dir)| dir);
            }
        }

        best
    }
}

// ── Object-snap priority ───────────────────────────────────────────────────

/// Selection priority for an object snap — lower wins. Discrete snaps that
/// land on a specific feature (Endpoint, Midpoint, Center, …) outrank the
/// "continuous" snaps (Perpendicular, Tangent, Nearest) that can sit anywhere
/// along the geometry, so enabling a continuous snap can't suppress the
/// discrete ones the user also turned on. Mirrors the usual CAD running-osnap
/// precedence. See #118.
/// Priority TIER for candidate selection. All discrete point snaps
/// (Endpoint … Insertion) share one tier so the cursor's nearest feature wins
/// among them — a circle's Center must not mask its Quadrants just by rank
/// (#420). Continuous snaps keep their individual lower tiers, preserving the
/// #118 guarantee that they never suppress a discrete snap in the aperture.
fn snap_tier(t: SnapType) -> u8 {
    match t {
        SnapType::Endpoint
        | SnapType::Intersection
        | SnapType::ApparentIntersection
        | SnapType::Midpoint
        | SnapType::Center
        | SnapType::Node
        | SnapType::Quadrant
        | SnapType::Insertion => 0,
        other => snap_priority(other),
    }
}

/// Candidate ordering: tier first, then cursor distance, and only when two
/// candidates are effectively equidistant (coincident features) the classic
/// sub-priority — so an Endpoint still beats an Intersection sitting on the
/// exact same point. Distances are screen-px²; 4.0 ≈ a 2 px coincidence band.
fn snap_better(tier: u8, d2: f32, sub: u8, best: (u8, f32, u8)) -> bool {
    let (bt, bd2, bsub) = best;
    if tier != bt {
        return tier < bt;
    }
    if (d2 - bd2).abs() > 4.0 {
        return d2 < bd2;
    }
    sub < bsub
}

fn snap_priority(t: SnapType) -> u8 {
    match t {
        SnapType::Endpoint => 0,
        SnapType::Intersection => 1,
        SnapType::ApparentIntersection => 2,
        SnapType::Midpoint => 3,
        SnapType::Center => 4,
        SnapType::Node => 5,
        SnapType::Quadrant => 6,
        SnapType::Insertion => 7,
        SnapType::ObjectPick => 8,
        SnapType::Perpendicular => 9,
        SnapType::Tangent => 10,
        SnapType::Parallel => 11,
        SnapType::Extension => 12,
        SnapType::Nearest => 13,
        SnapType::Grid => 14,
    }
}

// ── Geometric helpers ─────────────────────────────────────────────────────

/// Tracking directions through `p`, in one pass: the alignment ray along each
/// segment that ENDS at `p`, and the perpendicular of each segment that PASSES
/// THROUGH it.
///
/// The two ask different questions of the same segments, and the scan is the
/// expensive part — asking separately would walk the whole drawing twice for
/// every acquisition. Both are deduped by near-parallelism and capped.
///
/// Edge rays let the cursor track a segment's extension rather than only the
/// ortho/polar axes (#219); their sign is kept, because Extension needs to know
/// which way the segment left the corner, so two opposite collinear edges stay
/// distinct. Perpendicular rays are OTRACK's alone and describe an infinite
/// line, so one direction covers both sides (#695).
///
/// Scanned once, at acquisition — not per move. The edge list is empty when `p`
/// is not a segment endpoint (midpoint / centre / node), which is what makes it
/// double as the endpoint test.
fn tracking_dirs_at<W: WireSource + ?Sized>(
    p: DVec3,
    wires: &W,
) -> (Vec<DVec3>, Vec<DVec3>) {
    // Acquired points and reconstructed wire vertices are both f64. Keep a
    // small scale-aware window for double-single reconstruction residuals
    // without allowing unrelated UTM-scale vertices to match.
    let tol = 1e-8_f64.max(2e-12 * p.x.abs().max(p.y.abs()));
    let tol2 = tol * tol;
    let mut edges: Vec<DVec3> = Vec::new();
    let mut perps: Vec<DVec3> = Vec::new();
    let mut consider = |a: DVec3, b: DVec3| {
        if !a.x.is_finite() || !b.x.is_finite() {
            return false;
        }
        let seg_len = {
            let seg = b - a;
            (seg.x * seg.x + seg.y * seg.y).sqrt()
        };
        if seg_len < 1e-9 {
            return false;
        }
        let at_a = (a - p).length_squared() < tol2;
        let at_b = (b - p).length_squared() < tol2;

        if at_a || at_b {
            // Store the outward ray from the acquired endpoint.
            let seg = if at_a { a - b } else { b - a };
            let d = DVec3::new(seg.x / seg_len, seg.y / seg_len, 0.0);
            if !edges.iter().any(|e| e.x * d.x + e.y * d.y > 0.99996) {
                edges.push(d);
            }
        }

        // A perpendicular needs only that `p` lie on the segment — the whole
        // point of #695 is tracking away from a point partway along a line, not
        // just from its ends.
        if at_a || at_b || (nearest_on_segment(p, a, b) - p).length_squared() <= tol2 {
            let seg = b - a;
            let perp = DVec3::new(-seg.y / seg_len, seg.x / seg_len, 0.0);
            // Opposite perpendiculars describe the same infinite tracking line.
            let known = |list: &[DVec3]| {
                list.iter()
                    .any(|d| (d.x * perp.x + d.y * perp.y).abs() > 0.99996)
            };
            // Also skip a perpendicular that lands on a direction the edge rays
            // already cover: at a square corner one segment's perpendicular is
            // the other's own ray, and two rays down the same line are two
            // things for the cursor to choose between that look identical.
            if !known(&perps) && !known(&edges) {
                perps.push(perp);
            }
        }

        edges.len() >= 6 && perps.len() >= 6
    };
    let is_round = |wire: &WireModel| {
        wire.snap_pts
            .iter()
            .any(|(_, hint)| matches!(hint, SnapHint::Quadrant | SnapHint::Center))
    };
    if let Some(segments) = indexed_segments(wires) {
        for segment in segments {
            if wires.source_wire(segment.wire).is_some_and(&is_round) {
                continue;
            }
            if consider(segment.a, segment.b) {
                break;
            }
        }
        return (edges, perps);
    }
    'outer: for wire in wires.iter() {
        if is_round(wire) {
            continue;
        }
        let n = wire.points.len();
        if n < 2 {
            continue;
        }
        for i in 0..n - 1 {
            let a = wp_f64(wire, i);
            let b = wp_f64(wire, i + 1);
            if consider(a, b) {
                break 'outer;
            }
        }
    }
    (edges, perps)
}

#[derive(Clone, Copy)]
struct IndexedSegment {
    wire: u32,
    a: DVec3,
    b: DVec3,
}

impl IndexedSegment {
    fn min_x(self) -> f64 {
        self.a.x.min(self.b.x)
    }

    fn max_x(self) -> f64 {
        self.a.x.max(self.b.x)
    }

    fn min_y(self) -> f64 {
        self.a.y.min(self.b.y)
    }

    fn max_y(self) -> f64 {
        self.a.y.max(self.b.y)
    }
}

const MAX_INDEXED_PAIRWISE_WORK: usize = 500_000;

fn indexed_sweep_within_budget(segments: &[&IndexedSegment]) -> bool {
    let mut work = 0usize;
    for (index, &a) in segments.iter().enumerate() {
        for &b in segments.iter().skip(index + 1) {
            if b.min_x() > a.max_x() {
                break;
            }
            work += 1;
            if work > MAX_INDEXED_PAIRWISE_WORK {
                return false;
            }
            if a.wire == b.wire || a.max_y() < b.min_y() || a.min_y() > b.max_y() {
                continue;
            }
        }
    }
    true
}

fn projected_sweep_within_budget(segments: &[(&IndexedSegment, Point, Point, [f32; 4])]) -> bool {
    let mut work = 0usize;
    for (index, &(a, _, _, aabb_a)) in segments.iter().enumerate() {
        for &(b, _, _, aabb_b) in segments.iter().skip(index + 1) {
            if aabb_b[0] > aabb_a[2] {
                break;
            }
            work += 1;
            if work > MAX_INDEXED_PAIRWISE_WORK {
                return false;
            }
            if a.wire == b.wire || aabb_a[3] < aabb_b[1] || aabb_a[1] > aabb_b[3] {
                continue;
            }
        }
    }
    true
}

fn indexed_segments<W: WireSource + ?Sized>(wires: &W) -> Option<Vec<IndexedSegment>> {
    let refs = wires.segments()?;
    Some(
        refs.iter()
            .filter_map(|seg| {
                let wire = wires.source_wire(seg.wire)?;
                let start = seg.start as usize;
                if start + 1 >= wire.points.len() {
                    return None;
                }
                let a = wp_f64(wire, start);
                let b = wp_f64(wire, start + 1);
                (a.x.is_finite() && a.y.is_finite() && b.x.is_finite() && b.y.is_finite())
                    .then_some(IndexedSegment {
                        wire: seg.wire,
                        a,
                        b,
                    })
            })
            .collect(),
    )
}

/// Reconstruct the absolute f64 position of wire vertex `i` from its
/// double-single high/low pair. At UTM-scale coordinates the `points` (high)
/// f32 alone is ~0.5 m off; adding the low residual restores f64 precision so
/// computed snaps (nearest/perp/intersection/extension) land on the geometry.
#[inline]
fn wp_f64(wire: &WireModel, i: usize) -> glam::DVec3 {
    let h = wire.points[i];
    let l = wire.points_low.get(i).copied().unwrap_or([0.0; 3]);
    glam::DVec3::new(
        h[0] as f64 + l[0] as f64,
        h[1] as f64 + l[1] as f64,
        h[2] as f64 + l[2] as f64,
    )
}

/// Closest point on segment [p0, p1] to `query`.
fn nearest_on_segment(query: glam::DVec3, p0: glam::DVec3, p1: glam::DVec3) -> glam::DVec3 {
    let d = p1 - p0;
    let len2 = d.x * d.x + d.y * d.y;
    if len2 < 1e-12 {
        return p0;
    }
    let t = ((query.x - p0.x) * d.x + (query.y - p0.y) * d.y) / len2;
    let t = t.clamp(0.0, 1.0);
    glam::DVec3::new(p0.x + t * d.x, p0.y + t * d.y, p0.z + t * d.z)
}

/// Foot of perpendicular from `query` to the line through [p0, p1] (XY plane, unclamped).
/// Returns `None` if the segment is degenerate.
fn perp_foot(query: glam::DVec3, p0: glam::DVec3, p1: glam::DVec3) -> Option<glam::DVec3> {
    let d = p1 - p0;
    let len2 = d.x * d.x + d.y * d.y;
    if len2 < 1e-12 {
        return None;
    }
    let t = ((query.x - p0.x) * d.x + (query.y - p0.y) * d.y) / len2;
    // Reject if the foot is far outside the segment (more than 2× segment length).
    if t < -1.0 || t > 2.0 {
        return None;
    }
    Some(glam::DVec3::new(
        p0.x + t * d.x,
        p0.y + t * d.y,
        p0.z + t * d.z,
    ))
}

/// True 3D intersection of a forward ray and a finite segment.
fn ray_segment_intersect_3d(
    ray_origin: glam::DVec3,
    ray_through: glam::DVec3,
    b0: glam::DVec3,
    b1: glam::DVec3,
) -> Option<glam::DVec3> {
    let d1x = ray_through.x - ray_origin.x;
    let d1y = ray_through.y - ray_origin.y;
    let d2x = b1.x - b0.x;
    let d2y = b1.y - b0.y;

    let cross = d1x * d2y - d1y * d2x;
    if cross.abs() < 1e-9 {
        return None;
    }

    let ex = b0.x - ray_origin.x;
    let ey = b0.y - ray_origin.y;

    let t = (ex * d2y - ey * d2x) / cross;
    let s = (ex * d1y - ey * d1x) / cross;

    if t < 0.0 || s < 0.0 || s > 1.0 {
        return None;
    }

    let za = ray_origin.z + t * (ray_through.z - ray_origin.z);
    let zb = b0.z + s * (b1.z - b0.z);
    let tol = 1e-6_f64.max(1e-9 * za.abs().max(zb.abs()));

    if (za - zb).abs() > tol {
        return None;
    }

    Some(glam::DVec3::new(
        ray_origin.x + t * d1x,
        ray_origin.y + t * d1y,
        0.5 * (za + zb),
    ))
}

/// XY-plane segment-segment intersection.  Returns `None` if parallel or outside.
/// True 3D intersection of two segments: the point where their plan (XY)
/// projections cross **and** both segments are at the same height there. Returns
/// `None` when parallel in plan, out of range, or the segments only *appear* to
/// cross in plan because they sit at different Z — a real Intersection requires
/// the objects to actually meet, so that case is left to Apparent Intersection
/// (the view-space crossing). (#335)
fn seg_intersect_3d(
    a0: glam::DVec3,
    a1: glam::DVec3,
    b0: glam::DVec3,
    b1: glam::DVec3,
) -> Option<glam::DVec3> {
    let d1x = a1.x - a0.x;
    let d1y = a1.y - a0.y;
    let d2x = b1.x - b0.x;
    let d2y = b1.y - b0.y;
    let cross = d1x * d2y - d1y * d2x;
    if cross.abs() < 1e-9 {
        return None;
    } // parallel in plan
    let ex = b0.x - a0.x;
    let ey = b0.y - a0.y;
    let t = (ex * d2y - ey * d2x) / cross;
    let s = (ex * d1y - ey * d1x) / cross;
    if t < 0.0 || t > 1.0 || s < 0.0 || s > 1.0 {
        return None;
    }
    // The plans cross; the segments truly meet only if they are at the same
    // height at that crossing. Different Z ⇒ they merely overlap in plan, which
    // is an apparent (view-dependent) intersection, not a real one.
    let za = a0.z + t * (a1.z - a0.z);
    let zb = b0.z + s * (b1.z - b0.z);
    let tol = 1e-6_f64.max(1e-9 * za.abs().max(zb.abs()));
    if (za - zb).abs() > tol {
        return None;
    }
    Some(glam::DVec3::new(
        a0.x + t * d1x,
        a0.y + t * d1y,
        0.5 * (za + zb),
    ))
}

/// Intersection of two infinite lines in the XY plane, each given by an origin
/// and a direction. Returns `None` when the lines are parallel.
fn line_intersect_xy(o1: DVec3, d1: DVec3, o2: DVec3, d2: DVec3) -> Option<DVec3> {
    let cross = d1.x * d2.y - d1.y * d2.x;
    if cross.abs() < 1e-15 {
        return None;
    }
    let ex = o2.x - o1.x;
    let ey = o2.y - o1.y;
    let t = (ex * d2.y - ey * d2.x) / cross;
    Some(DVec3::new(o1.x + d1.x * t, o1.y + d1.y * t, o1.z))
}

/// Screen-space 2D segment intersection.  Returns `(t, s)` parameters if found.
fn seg_intersect_2d(a0: Point, a1: Point, b0: Point, b1: Point) -> Option<(f32, f32)> {
    let d1x = a1.x - a0.x;
    let d1y = a1.y - a0.y;
    let d2x = b1.x - b0.x;
    let d2y = b1.y - b0.y;
    let cross = d1x * d2y - d1y * d2x;
    if cross.abs() < 1e-6 {
        return None;
    }
    let ex = b0.x - a0.x;
    let ey = b0.y - a0.y;
    let t = (ex * d2y - ey * d2x) / cross;
    let s = (ex * d1y - ey * d1x) / cross;
    if t < 0.0 || t > 1.0 || s < 0.0 || s > 1.0 {
        return None;
    }
    Some((t, s))
}

/// Returns the two external tangent points on an XY circle.
fn circle_tangent_points(p: Vec3, center: Vec3, radius: f32) -> Option<(Vec3, Vec3)> {
    let curve = cadkernel::geom2d::Curve::Circle(cadkernel::geom2d::Circle {
        centre: [center.x as f64, center.y as f64],
        radius: radius as f64,
    });
    let points = cadkernel::geom2d::tangent_from(&curve, [p.x as f64, p.y as f64]);
    let [first, second] = points.as_slice() else {
        return None;
    };
    Some((
        Vec3::new(first.point[0] as f32, first.point[1] as f32, center.z),
        Vec3::new(second.point[0] as f32, second.point[1] as f32, center.z),
    ))
}

fn planar_circle_tangent_points(
    from: DVec3,
    center: [f64; 3],
    axis_x: [f64; 3],
    axis_y: [f64; 3],
    radius: f64,
) -> Vec<DVec3> {
    let plane = cadkernel::space::Plane::from_axes(center, axis_x, axis_y);
    let Some(from) = plane.project(from.to_array()) else {
        return Vec::new();
    };
    let curve = cadkernel::geom2d::Curve::Circle(cadkernel::geom2d::Circle {
        centre: [0.0, 0.0],
        radius,
    });
    cadkernel::geom2d::tangent_from(&curve, from)
        .into_iter()
        .map(|point| DVec3::from_array(plane.point_at(point.point)))
        .collect()
}

fn arc_tangent_points(
    from: DVec3,
    center: [f64; 3],
    axis_x: [f64; 3],
    axis_y: [f64; 3],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> Vec<DVec3> {
    let plane = cadkernel::space::Plane::from_axes(center, axis_x, axis_y);
    let Some(from) = plane.project(from.to_array()) else {
        return Vec::new();
    };
    let curve = cadkernel::geom2d::Curve::Arc(cadkernel::geom2d::Arc {
        centre: [0.0, 0.0],
        radius,
        start_angle,
        end_angle,
    });
    cadkernel::geom2d::tangent_from(&curve, from)
        .into_iter()
        .map(|point| DVec3::from_array(plane.point_at(point.point)))
        .collect()
}

/// Snap to the extension of a ray beyond `origin` in `dir` direction.
/// Returns `None` if the cursor is not near the extension line.
fn extension_snap(
    cursor_world: glam::DVec3,
    origin: glam::DVec3,
    dir: glam::DVec3,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    radius_px: f32,
) -> Option<glam::DVec3> {
    let len2 = dir.x * dir.x + dir.y * dir.y;
    if len2 < 1e-12 {
        return None;
    }
    let t = ((cursor_world.x - origin.x) * dir.x + (cursor_world.y - origin.y) * dir.y) / len2;
    if t < 0.05 {
        return None;
    } // only beyond the endpoint
    let world_pt = glam::DVec3::new(origin.x + t * dir.x, origin.y + t * dir.y, origin.z);
    let screen_pt = world_to_screen(world_pt, view_rot, eye, bounds);
    let cursor_screen = world_to_screen(cursor_world, view_rot, eye, bounds);
    if dist2(screen_pt, cursor_screen) > radius_px * radius_px {
        return None;
    }
    Some(world_pt)
}

/// Find acquired endpoint(s) whose outward ray contains `snapped`, returning
/// their screen positions for Extension guides. The captured ray set persists
/// after its source geometry leaves the cursor-local spatial query.
fn extension_bases_screen(
    snapped: glam::DVec3,
    tracking_points: &[glam::DVec3],
    tracking_dirs: &[Vec<glam::DVec3>],
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> (
    Option<Point>,
    Option<Point>,
    Option<(glam::DVec3, glam::DVec3)>,
) {
    let snapped_screen = world_to_screen(snapped, view_rot, eye, bounds);
    // Collect qualifying endpoints, off-ray distance measured in screen space so
    // the tolerance stays scale-independent at UTM coordinates (a world² test
    // would reject the crossing base once coordinates reach ~1e7).
    let mut found: Vec<(f32, glam::DVec3, glam::DVec3, Point)> = Vec::new();
    for (&origin, dirs) in tracking_points.iter().zip(tracking_dirs) {
        for &dir in dirs {
            let len2 = dir.x * dir.x + dir.y * dir.y;
            if len2 < 1e-12 {
                continue;
            }
            let t = ((snapped.x - origin.x) * dir.x + (snapped.y - origin.y) * dir.y) / len2;
            if t < 0.05 {
                continue;
            }
            let on = glam::DVec3::new(origin.x + t * dir.x, origin.y + t * dir.y, origin.z);
            let off = dist2(world_to_screen(on, view_rot, eye, bounds), snapped_screen);
            if off <= 4.0 {
                let base = world_to_screen(origin, view_rot, eye, bounds);
                found.push((off, origin, dir, base));
            }
        }
    }
    // Nearest-fit first, then keep up to two with distinct origins (collinear
    // segments sharing an endpoint must not draw the same guide twice).
    found.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut bases: [Option<Point>; 2] = [None, None];
    let mut origins: Vec<glam::DVec3> = Vec::new();
    let mut directions: Vec<glam::DVec3> = Vec::new();
    for (_, origin, dir, base) in found {
        if origins
            .iter()
            .any(|o| (*o - origin).length_squared() < 1e-12)
        {
            continue;
        }
        origins.push(origin);
        directions.push(dir);
        if bases[0].is_none() {
            bases[0] = Some(base);
        } else {
            bases[1] = Some(base);
            break;
        }
    }
    // `origins` is sorted nearest-fit first, so its head is the endpoint the
    // primary guide is drawn from — the one a typed distance measures along.
    (
        bases[0],
        bases[1],
        origins.first().copied().zip(directions.first().copied()),
    )
}

// ── Projection helpers ────────────────────────────────────────────────────

/// Project a world point to screen relative-to-eye: subtract the f64 eye first
/// so the result is precise at UTM-scale absolute coordinates (a full
/// view-projection with a ~1e7 translation cancels catastrophically in f32).
/// `view_rot` is the rotation-only view-projection (Camera::view_proj_rte).
fn world_to_screen(
    world: glam::DVec3,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> Point {
    let rel = (world - eye).as_vec3();
    let ndc = view_rot.project_point3(rel);
    Point::new(
        (ndc.x + 1.0) * 0.5 * bounds.width,
        (1.0 - ndc.y) * 0.5 * bounds.height,
    )
}

#[inline]
fn dist2(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}

/// The nearest line / polyline segment under the cursor as (unit direction,
/// world point on it), within `aperture_px` in screen space, or None.
/// Tessellated curves (circle / arc / ellipse) are skipped — they carry a
/// Center snap hint and "parallel to a curve" is meaningless. Used to acquire
/// the Parallel-snap reference. (#277)
fn nearest_segment<W: WireSource + ?Sized>(
    cursor_world: Vec3,
    wires: &W,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
    aperture_px: f32,
) -> Option<(Vec3, Vec3)> {
    let cs = world_to_screen(cursor_world.as_dvec3(), view_rot, eye, bounds);
    let mut best_d2 = aperture_px * aperture_px;
    let mut best: Option<(Vec3, Vec3)> = None;
    if let Some(segments) = indexed_segments(wires) {
        for segment in segments {
            let Some(wire) = wires.source_wire(segment.wire) else {
                continue;
            };
            if wire
                .snap_pts
                .iter()
                .any(|(_, h)| matches!(h, SnapHint::Center))
            {
                continue;
            }
            let sa = world_to_screen(segment.a, view_rot, eye, bounds);
            let sb = world_to_screen(segment.b, view_rot, eye, bounds);
            let d2 = dist2_to_segment(cs, sa, sb);
            if d2 < best_d2 {
                let dx = segment.b.x - segment.a.x;
                let dy = segment.b.y - segment.a.y;
                let l = (dx * dx + dy * dy).sqrt();
                if l > 1e-9 {
                    best_d2 = d2;
                    let dir = Vec3::new((dx / l) as f32, (dy / l) as f32, 0.0);
                    let np =
                        nearest_on_segment(cursor_world.as_dvec3(), segment.a, segment.b).as_vec3();
                    best = Some((dir, np));
                }
            }
        }
        return best;
    }
    for wire in wires.iter() {
        if wire
            .snap_pts
            .iter()
            .any(|(_, h)| matches!(h, SnapHint::Center))
        {
            continue; // circle / arc / ellipse — no meaningful parallel
        }
        for i in 0..wire.points.len().saturating_sub(1) {
            let a = wp_f64(wire, i);
            let b = wp_f64(wire, i + 1);
            if !a.x.is_finite() || !b.x.is_finite() {
                continue;
            }
            let sa = world_to_screen(a, view_rot, eye, bounds);
            let sb = world_to_screen(b, view_rot, eye, bounds);
            let d2 = dist2_to_segment(cs, sa, sb);
            if d2 < best_d2 {
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let l = (dx * dx + dy * dy).sqrt();
                if l > 1e-9 {
                    best_d2 = d2;
                    let dir = Vec3::new((dx / l) as f32, (dy / l) as f32, 0.0);
                    let np = nearest_on_segment(cursor_world.as_dvec3(), a, b).as_vec3();
                    best = Some((dir, np));
                }
            }
        }
    }
    best
}

/// Perpendicular screen-space distance (px) from world point `q` to the infinite
/// line through `line_pt` along `line_dir`. Used to tell whether a hovered line
/// is the acquired parallel reference (same line) regardless of zoom. (#277)
fn screen_perp_dist(
    q: Vec3,
    line_pt: Vec3,
    line_dir: Vec3,
    view_rot: Mat4,
    eye: glam::DVec3,
    bounds: Rectangle,
) -> f32 {
    let sq = world_to_screen(q.as_dvec3(), view_rot, eye, bounds);
    let s0 = world_to_screen(line_pt.as_dvec3(), view_rot, eye, bounds);
    let s1 = world_to_screen((line_pt + line_dir).as_dvec3(), view_rot, eye, bounds);
    let ex = s1.x - s0.x;
    let ey = s1.y - s0.y;
    let l = (ex * ex + ey * ey).sqrt();
    if l < 1e-6 {
        return dist2(sq, s0).sqrt();
    }
    (ex * (sq.y - s0.y) - ey * (sq.x - s0.x)).abs() / l
}

/// Squared distance from point p to line segment [a, b] in screen space.
fn dist2_to_segment(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-6 {
        let ex = p.x - a.x;
        let ey = p.y - a.y;
        return ex * ex + ey * ey;
    }
    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
    let t = t.clamp(0.0, 1.0);
    let nx = a.x + t * dx - p.x;
    let ny = a.y + t * dy - p.y;
    nx * nx + ny * ny
}

/// Parameter t ∈ [0,1] of the closest point on segment [a,b] to p.
fn t_on_segment(p: Point, a: Point, b: Point) -> f32 {
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-6 {
        return 0.0;
    }
    (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
}

#[cfg(test)]
mod ext_tests {
    use super::*;

    #[test]
    fn tracking_active_covers_otrack_and_extension() {
        let mut s = Snapper::default();
        // OTRACK off, Extension not enabled → no acquisition.
        s.snap_enabled = true;
        s.otrack_enabled = false;
        assert!(!s.tracking_active());
        // Extension on with the snap master on → acquire, independent of OTRACK.
        s.enabled.insert(SnapType::Extension);
        assert!(s.tracking_active());
        // Extension is gated by the snap master.
        s.snap_enabled = false;
        assert!(!s.tracking_active());
        // OTRACK acquires regardless of the object-snap master.
        s.enabled.remove(&SnapType::Extension);
        s.otrack_enabled = true;
        assert!(s.tracking_active());
    }

    #[test]
    fn extension_acquisition_keeps_only_endpoints() {
        let mut s = Snapper::default();
        // A single line segment (0,0)-(10,0): its endpoints are vertices, its
        // midpoint and any extension foot are not.
        let wire = WireModel {
            text_verts: Vec::new(),
            points: vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            ..Default::default()
        };
        let wires = [wire];

        // Extension-driven acquisition (endpoints_only): a midpoint — like an
        // extension foot the cursor paused on — is ignored, so it can't fill the
        // buffer and evict the real endpoint (#262).
        s.acquire_tracking_point(DVec3::new(5.0, 0.0, 0.0), &wires, true);
        assert!(s.tracking_points.is_empty());
        // The genuine endpoint is acquired.
        s.acquire_tracking_point(DVec3::new(10.0, 0.0, 0.0), &wires, true);
        assert_eq!(s.tracking_points.len(), 1);
        // OTRACK (endpoints_only = false) still acquires any snap point.
        s.acquire_tracking_point(DVec3::new(5.0, 0.0, 0.0), &wires, false);
        assert_eq!(s.tracking_points.len(), 2);
    }

    #[test]
    fn base_to_corner_direction_is_tracked() {
        let mut s = Snapper::default();
        s.otrack_enabled = true;
        s.osnap_radius_px = 10.0;
        // An acquired corner of an existing line, and the base point (first
        // point) of the line currently being drawn.
        let corner = DVec3::new(8_000.0, 6_000.0, 0.0);
        s.tracking_points.push(corner);
        s.tracking_dirs.push(Vec::new());
        let base = DVec3::new(0.0, 0.0, 0.0);

        // A plain orthographic camera: world * 0.0001 → NDC.
        let view_rot = Mat4::from_scale(Vec3::splat(0.0001));
        let eye = glam::DVec3::ZERO;
        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 1000.0,
        };

        // Cursor 5000 units along the base→corner ray (3-4-5 direction), nudged
        // a hair off it. The OTRACK result must retain that exact distance.
        let dir = (corner - base).normalize();
        let perp = DVec3::new(-dir.y, dir.x, 0.0);
        let cursor = base + dir * 5_000.0 + perp * 0.05;

        let hit = s
            .otrack_snap(
                cursor,
                view_rot,
                eye,
                bounds,
                None,
                Some(base),
                None,
                false,
                DVec3::X,
                DVec3::Y,
            )
            .expect("base→corner alignment should lock");
        // The aligned point lies on the base→corner line, and the reported base
        // is the command's base point (so typed-distance runs from there).
        let off = hit.aligned - base;
        let cross = off.x * dir.y - off.y * dir.x;
        assert!((off.length() - 5_000.0).abs() < 1e-10);
        assert!(
            cross.abs() < 1e-12,
            "aligned point off the base→corner line: {:?}",
            hit.aligned
        );
        assert!(
            (hit.base - base).length() < 1e-12,
            "ray base is not the command base"
        );

        // Without a base point the alignment does not exist (nothing to join).
        let none = s.otrack_snap(
            cursor,
            view_rot,
            eye,
            bounds,
            None,
            None,
            None,
            false,
            DVec3::X,
            DVec3::Y,
        );
        assert!(none.is_none(), "no base point → no base→corner alignment");
    }

    #[test]
    fn tangent_points_are_perpendicular_to_the_radius() {
        let c = Vec3::new(0.0, 0.0, 0.0);
        let p = Vec3::new(10.0, 0.0, 0.0);
        let (t0, t1) = circle_tangent_points(p, c, 5.0).expect("external tangents exist");
        for t in [t0, t1] {
            // On the circle...
            assert!(((t - c).length() - 5.0).abs() < 1e-3, "{t:?} off circle");
            // ...and the radius C→T is perpendicular to the line P→T — the
            // defining property of a tangent (not just the nearest point).
            let ct = t - c;
            let pt = t - p;
            assert!(
                (ct.x * pt.x + ct.y * pt.y).abs() < 1e-3,
                "radius not perpendicular to the line at {t:?}"
            );
        }
        // Known geometry: acos(5/10) = 60°, so the tangents are at (2.5, ±4.330).
        assert!((t0.x - 2.5).abs() < 1e-2 && (t0.y.abs() - 4.330).abs() < 1e-2);
        // A point inside the circle has no external tangent.
        assert!(circle_tangent_points(Vec3::new(1.0, 0.0, 0.0), c, 5.0).is_none());
    }

    #[test]
    fn intersection_requires_matching_z_not_just_plan_crossing() {
        use glam::DVec3;
        // Two segments whose plans cross at the origin, both at z = 0: a real
        // 3D intersection.
        let hit = seg_intersect_3d(
            DVec3::new(-1.0, 0.0, 0.0),
            DVec3::new(1.0, 0.0, 0.0),
            DVec3::new(0.0, -1.0, 0.0),
            DVec3::new(0.0, 1.0, 0.0),
        )
        .expect("coplanar crossing is a real intersection");
        assert!(hit.x.abs() < 1e-9 && hit.y.abs() < 1e-9 && hit.z.abs() < 1e-9);

        // Same plan crossing, but the second segment sits at z = 5: the lines
        // only *appear* to cross in top view — not a real intersection (#335).
        assert!(
            seg_intersect_3d(
                DVec3::new(-1.0, 0.0, 0.0),
                DVec3::new(1.0, 0.0, 0.0),
                DVec3::new(0.0, -1.0, 5.0),
                DVec3::new(0.0, 1.0, 5.0),
            )
            .is_none(),
            "different-Z plan crossing must not be a real Intersection"
        );

        // A real 3D crossing above the ground plane: both segments pass through
        // (0,0,5), so it snaps there at the true height.
        let hi = seg_intersect_3d(
            DVec3::new(-1.0, 0.0, 5.0),
            DVec3::new(1.0, 0.0, 5.0),
            DVec3::new(0.0, -1.0, 5.0),
            DVec3::new(0.0, 1.0, 5.0),
        )
        .expect("coplanar crossing at z=5");
        assert!(
            (hi.z - 5.0).abs() < 1e-9,
            "z should be the true height, got {}",
            hi.z
        );
    }
}
