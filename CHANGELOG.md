# Changelog

Notable changes on `feature/parametric-constraint-system` since each push to
GitHub. Newest entries first. "Unreleased" covers local changes not yet
pushed.

## Unreleased (since `3d0a41b6`)

- **Feature:** Properties panel gains a "Parameters" section (shown with no
  selection) for inline named-parameter name/formula editing, add, and
  delete, and a "Constraints" section (shown for a single selected entity)
  listing every constraint touching it as a clickable link that selects
  the linked entities in the viewport — danger-tinted when the constraint
  conflicts. A newly-added parameter's formula now defaults to `1`, not
  `0`, since a 0-length seed value is a genuine solver singularity for a
  Distance constraint driven by that parameter (no defined direction to
  grow back out of).
  ([command_driver.rs](src/app/command_driver.rs),
  [properties.rs](src/app/properties.rs),
  [ui/properties.rs](src/ui/properties.rs))

- **Feature:** new Options > General toggle, "Show values and parameter
  names on constraint markers" (on by default, persisted). Off, every
  constraint pill in the viewport shows just its bare glyph instead of
  glyph-plus-driven-value/parameter-name — the glyph alone is enough to
  see a constraint is present, without the value text covering nearby
  geometry on a dense sketch.
  ([options.rs](src/ui/window/options.rs),
  [view/mod.rs](src/app/view/mod.rs))

- **Feature:** all 18 constraint-ribbon icons (including the plain "F",
  "L", "M", "S" letter glyphs) replaced with proper SVGs under
  `assets/icons/constrain/`, matching the Modify section's two-color style
  (`#B4B6B9` white / `#6DB7ED` blue, consistent stroke width). Tangent
  reuses the Draw section's white-arc-plus-blue-dots motif.
  ([assets/icons/constrain](assets/icons/constrain),
  [tools.rs](src/modules/draw/constrain/tools.rs))

- **Fix:** a Concentric or CenterPoint constraint that moved an ellipse's
  center visibly rotated and reshaped the ellipse, even though nothing
  constrained its shape. Root cause: `ocs_gcs::geo::Ellipse` stores
  `focus1` as an absolute point rather than an offset from `center`, so
  when a constraint pulled only on `center`, `focus1`'s absolute
  coordinates stayed put while `center` moved — changing the derived
  `focus1 - center` vector that `major_axis`/`minor_axis_ratio` are
  computed from on write-back. The solver now adds two constraints per
  registered ellipse pinning that offset to its pre-solve value, so
  `focus1` translates rigidly with `center` unless something actually
  constrains it.
  ([sketch_solve.rs](src/scene/sketch_solve.rs))

- **Fix:** a sketch's constraint set (or the named-parameter table) silently
  lost *all* of its entries on the next DWG open once its serialized size
  passed 255 bytes — roughly 5-6 constraints, easily reached by a real
  sketch. Root cause: the vendored DWG writer packs an `XRecord` binary
  `Chunk` entry's length as a single `u8` and truncates anything longer
  (`cadcodec`'s `encode_xrecord_entries`), so the truncated bytes then
  failed to `bincode::deserialize` on load and the loader silently dropped
  the whole scope rather than erroring. DXF was unaffected (its writer has
  no such cap), which is what made this findable — the exact same document
  round-tripped constraint-for-constraint through DXF and came back empty
  through DWG. Fixed entirely on the OCS side, no dependency patch needed:
  the blob is now split across as many same-code `310` entries as it takes
  (below 255 bytes each) on save, and every entry for that key is
  concatenated back together on load, instead of just the first one.
  ([sketch_persist.rs](src/scene/sketch_persist.rs),
  [named_parameters_persist.rs](src/scene/named_parameters_persist.rs))

- **Fix:** none of the 17 sketch-constraint commands (`HCONSTRAINT`,
  `VCONSTRAINT`, `PCONSTRAINT`, `QCONSTRAINT`, `ECONSTRAINT`, `TCONSTRAINT`,
  `NCONSTRAINT`, `NRCONSTRAINT`, `LCONSTRAINT`, `FXCONSTRAINT`,
  `SYCONSTRAINT`, `CCONSTRAINT`, `EDCONSTRAINT`, `CPCONSTRAINT`,
  `MPCONSTRAINT`, `OCCONSTRAINT`, `DCONSTRAINT`, `ACONSTRAINT`) carried an
  `inventory::submit!` registration — a pre-existing gap the
  `docs/command_reference.md` generator had already flagged, which meant
  every constraint command (including this session's new `NRCONSTRAINT`)
  was invisible to command-line autocomplete and the MCP server's
  `commands` listing, despite working fine when typed exactly. Each
  constraint module now registers itself, with a regression test pinning
  it so the gap can't silently reappear.
  - The same missing registration also broke the **keyboard-shortcut
    editor** (`ALIASEDIT`): it validates a custom shortcut's target
    against this same registry and silently rejected anything not in it,
    so a user could not assign a custom shortcut to *any* constraint
    command — not just the new ones, the original `HCONSTRAINT`/
    `CCONSTRAINT`/etc. family too. Fixed by the same change.
  ([tools.rs](src/modules/draw/constrain/tools.rs),
  [coincident.rs](src/modules/draw/constrain/coincident.rs),
  [equal_distance.rs](src/modules/draw/constrain/equal_distance.rs),
  [point_on_entity.rs](src/modules/draw/constrain/point_on_entity.rs),
  [value.rs](src/modules/draw/constrain/value.rs),
  [command.rs](src/command.rs))

## Pushed 2026-09-10 (`eb96e8b6` → `3d0a41b6`)

- **Fix:** a dimensional constraint's value prompt (e.g. `DCONSTRAINT`'s
  "Specify distance:") rejected a named parameter typed in its own
  lowercase name — the command line uppercases typed text before it
  reaches the parser, so `r` became `R` and no longer matched the
  parameter table's case-sensitive lookup. `parse_driving_value` now
  matches parameter names case-insensitively and resolves to the
  parameter's canonical name.
  ([value.rs](src/modules/draw/constrain/value.rs))
- **Feature:** Named Parameters panel gains a "Used by" column showing
  which constraints (kind + entity handles) currently reference each
  parameter, with a hover tooltip listing the full per-constraint entity
  breakdown when more than a couple of constraints use it. Backed by a new
  `Scene::parameter_usage(name)` query that searches every sketch scope
  (model space and blocks) and deduplicates an entity referenced by more
  than one of a constraint's point markers.
  ([named_parameters.rs](src/ui/window/named_parameters.rs),
  [sketch_constraints.rs](src/scene/sketch_constraints.rs))
- Named Parameters modal widened (620 → 820) to fit the new column.
  ([modal.rs](src/app/view/modal.rs))
- **Feature:** constraint-solver support extended well beyond Line/Circle,
  closing most of the gap with FreeCAD's Sketcher (grounded against real
  ObjectARX headers so DWG-native persistence stays honest about what
  AutoCAD itself can represent):
  - Arcs are now full constraint participants: center/radius (Concentric,
    Tangent, Equal, CenterPoint, Radius, Diameter, Fixed, Normal) via the
    same solver representation a Circle uses, *and* actual endpoints
    (Coincident/PointOnCurve/Midpoint/EqualDistance/Symmetric on an arc's
    real start/end point, marker 0/1) — kept numerically consistent with
    center/radius/angle by "arc rules" constraints added for every
    registered arc, reusing an already-ported-but-unused `ocs_gcs`
    primitive (`CurveValue`) rather than new low-level math.
  - New dimensional kinds: `Diameter`, `DistanceX`, `DistanceY`,
    `ArcLength` — reusing existing DWG classes (Diameter/DistanceX/Y ride
    the same `ACRADIUSDIAMETERCONSTRAINT`/`ACDISTANCECONSTRAINT` objects
    Radius/Distance already use, with a different mode/direction byte,
    matching how AutoCAD itself represents them). `ArcLength` has no DWG
    equivalent at all (confirmed against ObjectARX's own headers) and is
    XRecord-only by design, not omission.
  - New geometric kind: `Normal` (a line perpendicular to a circle/arc's
    tangent), matching AutoCAD's real `kNormal` constraint type.
  - New entity: `Ellipse` (center/Concentric/CenterPoint/Fixed only —
    major/minor axis dimensional constraints and `PointOnEllipse`-based
    Coincident/Tangent remain a follow-up). DWG-native persistence for
    ellipse-referencing constraints is deliberately not implemented yet —
    `acadrust`'s `Ellipse`/`BoundedEllipse` node variants have a different
    field shape than the geometry-dependency role Circle/Arc play, and
    guessing the semantics risked writing a plausible-but-wrong object
    graph, so it degrades to XRecord-only instead.
  ([sketch_solve.rs](src/scene/sketch_solve.rs),
  [sketch_constraints.rs](src/scene/sketch_constraints.rs),
  [dwg_native_constraints.rs](src/scene/dwg_native_constraints.rs),
  [value.rs](src/modules/draw/constrain/value.rs))
