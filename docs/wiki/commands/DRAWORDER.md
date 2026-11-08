---
summary: Changes which overlapping objects are drawn on top: bring them to the front, send them to the back, or put them above or under other objects.
related: HATCHTOBACK, TEXTTOFRONT
ribbon: Draw tab › Modify panel
---

## Usage
1. Select objects, or run `DRAWORDER` first and select them, then press Enter.
2. Choose an option, or press Enter for **Back**.
3. For **Above** or **Under**, select the reference objects, then press Enter.

## Options
| Option | What it does |
|---|---|
| **Above** (`A`) | Puts the objects just above the reference objects you select next. |
| **Under** (`U`) | Puts the objects just under the reference objects you select next. |
| **Front** (`F`) | Draws the objects on top of everything else in the same space. |
| **Back** (`B`) | Draws the objects behind everything else in the same space. This is the default. |

## Notes
- The order is saved with the drawing.
- Objects on locked layers are not reordered.
- The **Draw Order** button in the slide-out of the Modify panel has **Bring to Front**, **Send to Back**, **Bring Above Objects** and **Send Under Objects**. They use the current selection, or ask you to select objects first.
- The right-click menu for selected objects has a **Draw Order** submenu with the same four choices.
