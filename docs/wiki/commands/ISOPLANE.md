---
summary: Switches the isometric drafting plane between Left, Top and Right.
related: ISODRAFT, DSETTINGS, ORTHO
---
## Usage
- Type `ISOPLANE` to move to the next plane: Left, then Top, then Right. The command line shows the new plane.
- Type `ISOPLANE LEFT`, `ISOPLANE TOP` or `ISOPLANE RIGHT` (or `L`, `T`, `R`) to choose a plane directly.

## Options
| Option | What it does |
|---|---|
| **Left** (`L`) | Drafting axes at 90° and 150°. |
| **Top** (`T`) | Drafting axes at 30° and 150°. |
| **Right** (`R`) | Drafting axes at 30° and 90°. |

## Notes
- If isometric drafting is off, `ISOPLANE` first turns it on. Choosing a plane by name also turns it on.
- The crosshairs and Ortho follow the two axes of the chosen plane.
- The setting is saved with your preferences.
