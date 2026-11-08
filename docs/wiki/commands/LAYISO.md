---
summary: Isolates the layers of the selected objects by turning off, or locking, every other layer.
related: LAYUNISO, LAYOFF, LAYFRZ, LAYWALK, ISOLATEOBJECTS
ribbon: Draw tab › Layers panel
---

## Usage
1. Select objects. If objects are already selected, their layers are isolated at once.
2. Otherwise, at the `Select objects or [Settings]:` prompt, pick the objects and press Enter, or type an option.
3. All other layers are turned off, or locked, depending on the setting.

## Options
| Option | What it does |
|---|---|
| **Settings** (`SE`) | Asks how other layers are treated: **Off** turns them off; **Lock and Fade** locks them, so they stay visible but faded and cannot be edited. Then you select the objects. |

## Notes
- The current layer and layer `0` are never turned off or locked by this command.
- The setting stays for the drawing until you change it. The default is **Off**.
- To return the layers to how they were, use [`LAYUNISO`](LAYUNISO-Command).
- The change can be undone.
