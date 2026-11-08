---
summary: Lists the ribbon panels you can show as floating toolbars, and shows or hides one by name.
related: WSCURRENT, RIBBON, RIBBONCLOSE
---
## Usage
- Type `TOOLBAR` alone to list every toolbar. Each line reads `tab/panel`, and a `●` marks the ones shown now.
- Type `TOOLBAR <name>` to toggle one toolbar. A toolbar is a ribbon panel; give its panel name, or `tab/panel` when two tabs share a panel name (for example `TOOLBAR draw/Modify`).
- Add `Show` (`S`) or `Hide` (`H`) after the name to set it instead of toggling (for example `TOOLBAR Layers Show`).

## Options
| Option | What it does |
|---|---|
| **Show** (`S`) | Shows the panel as a floating toolbar over the drawing. |
| **Hide** (`H`) | Puts the panel back in its ribbon row. |

## Command-line form
`-TOOLBAR` works the same way as `TOOLBAR`. Put the name and the action on the same line, which suits scripts.

## Notes
- New toolbars line up along the top of the drawing area, left to right.
- Floating toolbars stay on screen while the ribbon is closed.
- Panel names match case-insensitively, in English or in the current interface language.
- An unknown name prints `no toolbar named "<name>"`.
- The shown toolbars are saved with your settings.
