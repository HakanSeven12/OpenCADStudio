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
