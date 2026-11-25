A parametric block with visibility states, made in BricsCAD V26 (Linux, German
user interface) and saved there as DWG 2018 (AC1032).

The block `ocs-bricscad-visibility-block` holds a 10 × 10 frame, a circle with a
cross through it and the text `LABEL`. It has two visibility parameters, set up
with `VISIBILITYSTATES` on the drawing that became the block:

| Parameter | States | Objects the second state hides |
|---|---|---|
| `LAMP` | `On`, `Off` | the circle and the two cross lines |
| `LABEL` | `Shown`, `Hidden` | the text |

`visibility-states.dwg` inserts the block three times along the x axis:

| Handle | At | `LAMP` | `LABEL` | Block |
|---|---|---|---|---|
| `EE` | 0, 0 | `On` | `Shown` | the definition itself |
| `146` | 20, 0 | `Off` | `Shown` | `*U2` |
| `19E` | 40, 0 | `Off` | `Hidden` | `*U3` |

What the file shows about how BricsCAD stores such a block:

- A reference in a state other than the definition's shows an anonymous copy
  (`*U2`, `*U3`) with every member of the definition in the same order. The
  members a state hides sit on the layer `*SUPPRESS`, which is frozen and off.
- The parameters live in objects of BricsCAD's own classes (`BDMDB…`,
  `BdmDb…`, `BDMOBJECT`) under `BDM_DATABASE` in the extension dictionary of
  the definition, of each copy and of model space. Each block also has an
  `ACAD_ASSOCNETWORK` whose variables (`LAMP`, `look_1`, `Unterdrücken_1`, …)
  those objects read and drive. The variables list them as reactors and the
  objects reach the variables by handle, so the variables must keep their
  handles when the drawing is saved.

## Authoring: `authoring-*.dwg`

Saved by BricsCAD V26 (same setup) along one run of `VISIBILITYSTATES` on a
drawing of five lines on layer `0`, ten long and five apart along the x axis:
L1 to L5, handles `92` to `96`.

`authoring-start.dwg` is the drawing before the run. `authoring-select.dwg` is
the drawing after these steps (parameter, state, action, lines):

| # | Parameter | State | Action | Lines |
|---|---|---|---|---|
| 1 | `P1` | `S1` | Add | L1, L2 |
| 2 | `P1` | `S2` | Invisible | L1 |
| 3 | `P1` | `S3` | Invisible | L2 |
| 4 | `P1` | `S2` | Visible | L1 |
| 5 | `P1` | `S1` | Add | L3 |
| 6 | `P1` | `S1` | Remove | L3 |
| 7 | `P2` | `Sichtbar` | Add | L4 |
| 8 | `P2` | `Unsichtbar` | Invisible | L4 |
| 9 | `P2` | `Sichtbar` | Add | L2 |
| 10 | `P1` | `S4` | Visible | L1 |
| 11 | `P1` | `S2` | Select state | |
| 12 | `P1` | `S3` | Select state | |

`authoring-edit.dwg` is the drawing after steps 1 to 10 and then:

| # | Parameter | State | Action | Lines |
|---|---|---|---|---|
| 11 | `P1` | `S1` | Invisible | L5 |
| 12 | `P1` | `S2` | Visible | L4 |
| 13 | `P1` | `S1` | Remove | L1 |
| 14 | `P1` | `S3` | Add | L2 |
| 15 | `P1` | `S2` | Add | L3 |

What the run shows about how BricsCAD keeps the states it authors:

- A parameter is a configuration feature with a table: a row per state, a
  column (`look_n`, the lowest free number) per group of lines that show and
  hide together, 1 where the group shows. A new state starts with every group
  hidden.
- Each group is a suppression feature (`Unterdrücken_n` in the German
  interface) whose variable is `1 - ` the product of the group's columns, one
  per parameter that rules the group, in parameter order (step 9 makes
  `1 - look_1 * look_4`).
- Remove keeps the line's `BDMOBJECT` but empties its dependencies; adding the
  line again reuses it (step 15).
- Every read of a variable (an `ACDBASSOCVALUEDEPENDENCY`) holds the
  variable's value, and a variable's reads carry their position as flags.
