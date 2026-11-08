---
summary: Turns the view about its line of sight by a twist angle.
related: VIEW, 3DORBIT, PLAN
---

## Usage
1. Run `DVIEW`. It asks **Enter option [TWist/Undo]**.
2. Type `TW` and enter the twist angle. The prompt shows the current twist as the default.
3. The view turns, and the options are asked again. Press Enter to finish.

## Options
| Option | What it does |
|---|---|
| **TWist** (`TW`) | Asks for a view twist angle and turns the view to it. Press Enter at the angle prompt to keep the current twist. |
| **Undo** (`U`) | Goes back to the twist before the last change in this command. |

## Notes
- The command only sets the twist. It does not move the camera or target, and it has no clipping or perspective options.
- In a layout with an active viewport, the viewport's view is turned.
