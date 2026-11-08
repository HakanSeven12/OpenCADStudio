---
summary: In the Block Editor, opens the property lookup table of a Lookup parameter for editing.
related: BPARAMETER, BACTION, BEDIT, BTABLE
---

## Usage
1. Run the command in the Block Editor. The **Property Lookup Table** dialog opens.
2. Choose the **Lookup parameter** whose table you want to edit.
3. Add columns with **Add Property**. Input columns take Linear or Polar parameter values; Lookup columns hold the values the Lookup parameter shows.
4. Add rows with **Add Row** and fill in the cells. Remove a row with **Delete**, and a column with **Remove**.
5. Click **Audit** to check the table.
6. Click **OK** to save the table, or **Cancel** to close without changes.

## Notes
- The command works only in the Block Editor, and the block must have a Lookup parameter. Place one with [`BPARAMETER`](BPARAMETER-Command) first.
- Input cells take numbers, formulas or ranges such as `[2,6]` and `(2,6]`. Formulas use input names; put names with spaces or punctuation in backquotes. Angles are in radians.
- Turn on **Reverse lookup** on a Lookup column to let users pick a row from the Lookup value, which then sets the inputs.
- The **<Unmatched>** row holds the value shown when no row matches the current inputs.
- When actions are shown in action bars ([`BACTIONBARMODE`](BACTIONBARMODE-System-Variable) is 1), the command is refused. Edit the table from the Lookup action's bar, or set [`BACTIONBARMODE`](BACTIONBARMODE-System-Variable) to 0.
- The Lookup action on the Actions tab of the Authoring Palettes opens the same table.
