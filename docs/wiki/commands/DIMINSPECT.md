---
summary: Adds or removes an inspection frame, with a label and an inspection rate, on dimensions.
related: DIMEDIT, DIMSTYLE
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select the dimensions first, or select them from the dialog with **Select dimensions**.
2. In the Inspection Dimension dialog, choose the frame shape: **Round**, **Angular** or **None**.
3. Turn on **Label** and **Inspection rate** as needed and type their text.
4. Click **OK** to add the inspection data to the selected dimensions. Click **Remove inspection** to take it off.

## Command-line form
`-DIMINSPECT` asks on the command line, which suits scripts:
1. `Add inspection data or [Remove] <Add>:` Press Enter or type `A` to add, or type `R` to remove.
2. Select the dimensions and press Enter. With Remove, the inspection data is removed now.
3. `Enter shape option [Round/Angular/None] <Round>:` Type `R`, `A` or `N`.
4. `Enter label data or <None>:` Type a label, or press Enter for none.
5. `Enter inspection rate <100%>:` Type the rate, or press Enter for 100%.

## Notes
- If a selected dimension already has inspection data, the dialog opens with its values. Otherwise it shows the values you used last.
- Dimensions on locked layers are skipped.
