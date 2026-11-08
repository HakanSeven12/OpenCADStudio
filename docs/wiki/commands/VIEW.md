---
summary: Saves the current view under a name, and restores, lists or deletes named views.
related: VPORTS, PLAN, RENAME, UCS, VISUALSTYLES
---
## Usage
1. At `VIEW [List / Save / Restore / Delete]:`, type an option.
2. For **Save**, **Restore** and **Delete**, type the view name.

You can also type everything on one line, for example `VIEW SAVE Detail1` or `VIEW Detail1` (which restores it).

## Options
| Option | What it does |
|---|---|
| **List** (`LIST`) | Lists the named views of the drawing. |
| **Save** (`SAVE`) | Saves the current view under the name you type. An existing view with that name is replaced. |
| **Restore** (`RESTORE`) | Restores the named view, including its visual style. |
| **Delete** (`DELETE`) | Deletes the named view. |

## Command-line form
`-VIEW` asks `Enter an option [?/Delete/Orthographic/Restore/Save/sEttings/Window]:` and suits scripts, since every answer is typed:

| Option | What it does |
|---|---|
| **?** | Lists views matching the names you type (wildcards allowed; Enter lists all), with M or P for model or paper space. |
| **Delete** (`D`) | Deletes the views matching the names you type. |
| **Orthographic** (`O`) | Sets a standard view: **Top**, **Bottom**, **Front**, **BAck**, **Left** or **Right**. |
| **Restore** (`R`) | Restores a named view. On a layout sheet, a model-space view asks you to select the viewport it goes into. |
| **Save** (`S`) | Saves the current view under a name. |
| **sEttings** (`E`) | Changes a saved view: **Background** (Color, Gradient, Image or None), **Categorize**, **Layer snapshot** (Save or Delete), **live Section**, **Ucs** (whether new named views save the current UCS) and **Visual style**. |
| **Window** (`W`) | Type a name, then pick two corners. The view of that window is saved. |

At the first `-VIEW` prompt you can also type a preset name directly: `TOP`, `BOTTOM`, `FRONT`, `BACK`, `LEFT`, `RIGHT`, `SWISO`, `SEISO`, `NEISO` or `NWISO`.

## Notes
- `VIEW TOP`, `VIEW FRONT` and the other preset names, as well as `VIEW ISO`, set the standard views too. These names win over a saved view with the same name.
- Named views are stored in the drawing. Use [`RENAME`](RENAME-Command) to rename one.
- With the orthographic UCS setting on, an orthographic preset also sets the matching UCS.
