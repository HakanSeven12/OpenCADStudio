# Backlog — deferred items, known gaps, and scope-downs

A consolidated list of things noticed but deliberately not fixed/built, so
they aren't lost between sessions. Pulled from `docs/
parametric_system_design.md`'s own "open questions" log, the `ocs_gcs` port
plan, and things found while doing unrelated work this session. Not a bug
tracker for the whole app — scoped to what surfaced during the parametric
constraint system project and its surrounding investigation.

## Parametric constraint system — deliberate scope-downs (documented, not accidental)

1. **Stage 9 undo is two presses, not one.** Adding a persistent constraint
   and the geometry move it triggers are two separate undo/redo entries
   (`HistorySnapshot::SketchConstraints` pushed adjacent to, not merged into,
   the geometry `Delta`). The design doc's recommended §5.1(b) "live
   XRecord" approach would fix this but depends on stage 4 (save/load),
   which itself was scoped down (next item) — revisit both together if this
   is ever worth fixing.
2. **Stage 4 does not resolve on load.** Opening a file with saved
   constraints restores them but does not immediately re-solve — the DOF
   badge and any conflict state only appear after the next edit that touches
   an affected scope. Deliberate: resolving on load risked desyncing from
   the `DerivedCaches` bundle `on_file_opened` installs right after, without
   budget this session to verify that's safe.
3. **Stage 11's conflict resolver is a one-at-a-time pill, not the full
   SketchXpert-style panel.** Clicking "⚠ n conflicting" removes one flagged
   constraint and re-solves; there's no named-candidate list or cyclable live
   preview before committing (design doc §6.4 Phase C). This is the single
   largest UX gap remaining in the constraint system relative to the
   original plan.

## `ocs_gcs` solver — not ported

- `SlopeAtBSplineKnot` (tangent continuity across a spline knot) — needs
  knot-multiplicity reconstruction for a narrower use case than
  `PointOnBSpline`, which is ported. Deferred, not silently dropped.
- `InternalAlignmentPoint2Ellipse` / `InternalAlignmentPoint2Hyperbola` —
  FreeCAD-Sketcher-internal bookkeeping for *creating* conic geometry
  interactively, not for constraining existing geometry. Lower priority.
- `EllipticalArcRangeToEndPoints` — dead code upstream (a `ConstraintType`
  enum value with no implementing class anywhere in current planegcs
  source), not a gap in this port.
- **Golden-fixture testing against the locally-installed FreeCAD.app**
  (testing-strategy "Oracle A") was never wired up. Everything in `ocs_gcs`
  is verified only against Oracle B (closed-form + finite-difference). Still
  a real asset sitting unused if higher-confidence validation is ever
  wanted.
- **Performance at scale is untested.** The "only touched scopes rebuild"
  locality argument (design doc §4.2) is architecturally sound but has no
  profiling behind it on a large, heavily-constrained drawing.

## Constraint types not exposed in the app's UI (even though some primitives exist)

`ConstraintKind` (`src/scene/sketch_constraints.rs`) currently has:
Coincident, Horizontal, Vertical, Parallel, Perpendicular, Equal, Distance,
Angle, Radius, Tangent. AutoCAD's own Geometric Constraints panel additionally
has **Fix**, **Concentric**, **Collinear**, **Symmetric**, **Smooth** — none
of these have a `ConstraintKind` variant here. Some are buildable from
already-ported `ocs_gcs` primitives (e.g. Concentric ≈ `C2CDistance` at zero
distance) without new solver math; `Fix` would need a different mechanism
entirely (pinning a point's params non-free rather than adding a constraint
row) and doesn't fit the current `ConstraintKind` model at all.

## Newly discovered this session (not previously tracked anywhere)

- **`CCONSTRAINT`, `TCONSTRAINT`, and their four siblings
  (`HCONSTRAINT`/`VCONSTRAINT`/`PCONSTRAINT`/`QCONSTRAINT`/`ECONSTRAINT`) have
  no `inventory::submit!(CommandRegistration)` registration.** Found while
  generating `docs/command_reference.md`. All seven work correctly when
  invoked (ribbon button or exact typed name) but don't appear in the
  command-line's autocomplete or in the MCP server's `commands` listing,
  unlike almost every other command in the app. Pre-existing across the
  whole constraint-command family, not something this session's Tangent/
  Coincident additions broke uniquely — just never noticed before. Cheap fix
  (one `inventory::submit!` block) whenever someone wants to spend five
  minutes on it.
- **No live link from a sketch to 3D solid features.**
  `SolidHistorySweep`/`Revolve`/etc. (`src/scene/model/solid_history.rs`)
  hold `sweep_entity: Option<EmbeddedEntity>` — a geometry copy snapshotted
  at creation time, not a `Handle` back into the sketch. Dragging or
  re-solving a sketch that was previously extruded/revolved/swept does not
  update the resulting solid. This is the real "feature-history tree" gap
  discussed when scoping FreeCAD/AutoCAD parity — a substantially bigger
  project than the named-parameters work (see `docs/
  named_parameters_design.md` §3), not something to fold into it by
  accident.
- **No named-parameter/expression sheet** (AutoCAD's Parameters Manager /
  FreeCAD's Spreadsheet equivalent). This is the next feature queued up —
  see `docs/named_parameters_design.md` for the handoff context.
- **No end-user manual exists anywhere in the repo.** Checked `docs/`
  thoroughly — everything there is developer-facing (plugin architecture,
  this parametric design doc, tessellation notes, native-vs-web diffing,
  release notes, automation scripts) plus localized top-level README
  translations. No in-app Help link points anywhere either. Not started;
  would be a real, separate writing effort if wanted.
- **No theme install/import/export mechanism.** Theming is entirely in-app:
  pick one of iced's built-in themes, or hand-tune one "Custom" slot via six
  hex-color inputs (`src/ui/window/options.rs`, `UiThemePalette` in
  `src/app/config.rs`). No theme file format, no way to author and share a
  theme, only one custom palette remembered at a time.
- **No touch/multi-touch input support**, relevant to the wasm build running
  on a tablet (e.g. iPad). Grepped the whole codebase for touch/gesture
  handling — none exists; all input assumes mouse+keyboard, relying on the
  browser's synthetic mouse-event emulation for taps. No pinch-zoom, no
  two-finger pan, and OSNAP/grip-drag precision (already tight even with a
  mouse — see the Coincident epsilon note below) would be materially worse
  with a fingertip.

## Testing/tooling notes for future live-verification sessions

- **Coincident's success path could not be reliably live-verified via
  synthetic mouse clicks** — its resolver epsilon (`COINCIDENT_EPSILON_SQ =
  1.0e-12`, ~1e-6 world units, reused from stage 8's `infer_coincident_refs`)
  is tighter than this session's `left_click`/OSNAP automation could
  reliably hit, even with Endpoint snap enabled. Concluded to be an
  environment limitation, not a code defect — automated tests cover the
  resolver logic directly — but worth knowing before assuming a future
  automated click-through will succeed where this session's didn't.
- **`open_application`/`request_access` always resolve to the
  `/Applications`-installed app bundle**, regardless of which binary was
  most recently built. To live-test a code change, build a release binary
  and swap it into a **scratch copy** of the installed bundle (e.g.
  `/tmp/ocs_livetest/OpenCADStudio.app`), keeping the original small
  launcher (`CFBundleExecutable = "OpenCADStudio"`) intact, then
  `codesign --force --deep -s -` to re-sign ad-hoc before `open`-ing it.
  This reuses the real bundle identity (`io.github.HakanSeven12.OpenCadStudio`)
  so access requests correctly target it. A standalone bundle with a fresh
  bundle id gets silently `user_denied` — not a viable shortcut. The debug
  binary run bare via `nohup` never presented a controllable window either
  — use a release build.
- **`target/debug` can balloon fast from repeated live-testing rebuilds** —
  hit 35GB and caused a real "No space left on device" build failure this
  session. `cargo clean` reclaimed 64GB. Worth a proactive `cargo clean` (or
  checking `df`) before a long live-testing stretch, not just after it
  breaks.
- Synthetic input reliability, generally: Escape frequently failed to cancel
  an active command (use Return instead); LINE clicks sometimes produced
  stray zero-length duplicate entities; selection clicks often missed thin
  line bodies or landed on a midpoint-edit-box zone (use a crossing-window
  drag-select instead of precise body clicks).

## Legal / process

- **LGPL-2.1-or-later review for `ocs_gcs` is still open** (original plan
  §6). Non-blocking for continued development, but must happen before any
  public release containing it — separate-relinkable-module posture vs.
  broader relicensing, notice/attribution obligations under GPLv3
  distribution.
