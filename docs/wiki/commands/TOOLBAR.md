---
summary: Lists the ribbon panels you can show as toolbars, and shows, docks or hides one by name.
related: WSCURRENT, RIBBON, RIBBONCLOSE
---
## Usage
- Type `TOOLBAR` alone to list every toolbar. Each line reads `tab/panel`, and a `●` marks the ones shown now.
- Type `TOOLBAR <name>` to toggle one toolbar. A toolbar is a ribbon panel; give its panel name, or `tab/panel` when two tabs share a panel name (for example `TOOLBAR draw/Modify`).
- Add `Show` (`S`) or `Hide` (`H`) after the name to set it instead of toggling (for example `TOOLBAR Layers Show`).
- Add `Left` (`L`) or `Right` (`R`) to dock it as a one-column toolbar at that side of the drawing (for example `TOOLBAR Draw Left`), and `Float` (`F`) to float a docked toolbar again.

## Options
| Option | What it does |
|---|---|
| **Show** (`S`) | Shows the panel as a floating toolbar over the drawing. |
| **Hide** (`H`) | Puts the panel back in its ribbon row. |
| **Left** (`L`) | Docks the panel as a one-column toolbar at the left side of the drawing. |
| **Right** (`R`) | Docks the panel as a one-column toolbar at the right side of the drawing. |
| **Float** (`F`) | Floats a docked toolbar over the drawing again. |

## Command-line form
`-TOOLBAR` works the same way as `TOOLBAR`. Put the name and the action on the same line, which suits scripts.

## Notes
- New toolbars line up along the top of the drawing area, left to right.
- Floating and docked toolbars stay on screen while the ribbon is closed.
- Dragging a floating toolbar to the left or right side of the window docks it there. A click on a docked toolbar's grip (the bar at its top) floats it again.
- Docked toolbars stack one under the other and take their room from the drawing area. Combo boxes (layer, color, styles) are left out of a docked toolbar; a drop-down shows as its current tool.
- Panel names match case-insensitively, in English or in the current interface language.
- An unknown name prints `no toolbar named "<name>"`.
- The shown toolbars are saved with your settings.
