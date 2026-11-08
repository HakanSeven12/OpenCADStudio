---
summary: Edits a block reference or an external reference in place, in the current drawing.
related: REFSET, REFCLOSE, BEDIT, XREF, EXTERNALREFERENCES
ribbon: Insert tab › Reference panel
---

## Usage
1. Select the reference. If one reference is already selected, it is used.
2. The **Reference Edit** dialog opens. Under **Reference to edit**, the chain of nested references at the picked point is shown. Click a level to choose which one to edit.
3. Under **Working set**, choose **All objects**, or **Pick in the drawing** to choose the nested objects yourself.
4. Set the session options and click **Start Editing**.
5. If you chose **Pick in the drawing**, select the nested objects and press Enter.
6. Edit the objects with the usual commands.
7. End the session with [`REFCLOSE`](REFCLOSE-Command), or with the save and discard buttons on the toolbar at the right edge of the drawing.

## Options
| Option | What it does |
|---|---|
| **Also edit attribute definitions** | Includes the block's attribute definitions in the working set. Available when the block has them. |
| **Lock objects not in the working set** | Objects outside the working set act like objects on a locked layer during the session: you can see them, but not select or change them. |
| **Give clashing layer, style and block names unique names** | For an external reference, its layers, styles and blocks get their own names of the form `$0$NAME`. When off, they share the drawing's layer, style or block of the same name. |

## Command-line form
`-REFEDIT` asks everything at the command line, which suits scripts:
1. Select reference.
2. `Select nesting level [Ok/Next] <Next>:` **Next** (`N`) or Enter steps outward through the nested references and wraps back to the innermost one. **Ok** (`O`) takes the current level.
3. `Enter object selection method [All/Nested] <All>:` **All** (`A`) takes every object; **Nested** (`N`) asks you to select nested objects. Click objects or drag a window, then press Enter.
4. `Display attribute definitions [Yes/No] <No>:` decides whether attribute definitions are included.

The lock and unique-name settings come from the last use of the dialog.

## Notes
- The objects of the chosen definition are placed in the drawing where the reference shows them. Everything else is faded by `XFADECTL`.
- Objects you create during the session join the working set. Use [`REFSET`](REFSET-Command) to move other objects in or out.
- Saving writes the working set back into the block definition and updates every reference to it. For an external reference, the referenced file is written and reloaded.
- Only one reference can be edited at a time. Starting another reports that one is already checked out.
- The dialog settings are remembered.
- Undo of REFCLOSE opens the session again.
