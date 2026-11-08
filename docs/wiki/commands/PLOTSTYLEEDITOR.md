---
summary: Opens the plot style editor, where you change the plot colour, lineweight and screening of each colour in a CTB table.
related: PLOTSTYLE, PLOTSTYLEPANEL, PLOT, PAGESETUP
---

## Usage
1. Start `PLOTSTYLEEDITOR`. The editor opens with the plot style table chosen in the Plot dialog.
2. Pick a colour index (ACI) from the list.
3. Set its **Plot color** (with **Choose color…**, or **Reset** to use the object colour), **Lineweight** (or **Use object lineweight**) and **Screening (0-100)**. Changes apply to the entry as you make them.
4. Click **Save** to write over the table, or **Save As…** to save a copy under a new name.

## Notes
- The editor lists the layers in the current drawing that use the selected colour.
- **Load CTB/STB** loads another table, and **Clear Table** removes the active one.
- When you open it from the Plot dialog, the editor appears above the dialog.
- [`PLOTSTYLEPANEL`](PLOTSTYLEPANEL-Command) and [`STYLESMANAGER`](STYLESMANAGER-Command) open the same editor.
