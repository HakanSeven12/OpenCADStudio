---
summary: Opens the Parameters Manager, where you create and edit named parameters and their formulas.
related: PARAMETERSCLOSE, DCLINEAR, DELCONSTRAINT
ribbon: Parametric tab › Manage panel; Block Editor tab › Action Parameters panel
---

## Usage
1. Run the command. The Named Parameters dialog opens.
2. Click **+ Add parameter** to add a row. Type a name and a formula, for example `hole_dia = 12` or `hole_spacing = 2 * hole_dia + 1.5`.
3. The **Value** column shows the result and **Used by** shows which constraints use the parameter.
4. Click **Apply** to save the changes and solve the constraints again.

## Command-line form
`-PARAMETERS` works on the command line, so it suits scripts. One run does one operation.
1. Enter a parameter option [New/Edit/Rename/Delete/?].
2. **New** (`N`): Enter name for new user parameter, then Enter expression.
3. **Edit** (`E`): Enter parameter name. The old expression and value are shown; then Enter expression.
4. **Rename** (`R`): Enter old parameter name, then Enter new parameter name.
5. **Delete** (`D`): Enter parameter name to delete.
6. **?**: lists every parameter with its expression and value.

## Notes
- Closing the dialog discards edits that were not applied. Formula errors, circular references and duplicate names are flagged.
- In the Block Editor, PARAMETERS opens or closes the docked Parameters Manager for the block's parameters instead of the dialog.
