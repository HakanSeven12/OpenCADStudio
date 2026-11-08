---
summary: Shows again every object hidden by ISOLATEOBJECTS or HIDEOBJECTS, including objects the drawing was saved with as hidden.
related: ISOLATEOBJECTS, HIDEOBJECTS, LAYUNISO
---
## Usage
Run `UNISOLATEOBJECTS`. All hidden objects are shown again and the command line reports `Isolation ended — all objects shown.` If nothing was hidden, it reports `No hidden objects.`

## Notes
- It ends the current isolation or hide, and also shows objects that were already hidden when the drawing was opened (for example, hidden in another program). Showing those changes the drawing.
- Objects inside block definitions are not changed.
- You can also end isolation with **End Isolation** in the status-bar isolation menu, or **End Object Isolation** in the shortcut menu.
- You can undo the command.
