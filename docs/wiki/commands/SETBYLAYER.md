---
summary: Resets the color, linetype, lineweight, transparency, material and plot style of selected objects to ByLayer.
related: SETBYLAYERMODE, MATCHPROP, CHPROP, PROPERTIES
ribbon: Draw tab › Modify panel (slide-out)
---

## Usage
1. Select objects, or type `S` to choose which properties are reset. If you selected objects before you start the command, this step is skipped.
2. Press Enter to finish the selection.
3. Answer **Change ByBlock to ByLayer? [Yes/No]**. With **No**, properties set to ByBlock are kept.
4. Answer **Include blocks? [Yes/No]**. With **Yes**, the objects inside selected block references are reset too.
5. The command line reports how many objects were changed.

## Options
| Option | What it does |
|---|---|
| **Settings** (`S`) | Shows the active properties and lets you turn them on or off. Press Enter to go back to the selection. |
| **Color** (`C`) | Turns resetting the color on or off. |
| **LType** (`LT`) | Turns resetting the linetype on or off. |
| **LWeight** (`LW`) | Turns resetting the lineweight on or off. |
| **Transparency** (`T`) | Turns resetting the transparency on or off. |
| **Material** (`M`) | Turns resetting the material on or off. |
| **Yes** (`Y`) / **No** (`N`) | Answers the ByBlock and block questions. Enter keeps the default shown in angle brackets. |

## Command-line form
`-SETBYLAYER` runs the same prompts. Everything is asked at the command line, so both forms suit scripts.

## Notes
- Your settings and the last Yes/No answers are kept for the next run. They are stored in `SETBYLAYERMODE`; you can also type a number from 0 to 255 there.
- Plot style is reset as well, unless `SETBYLAYERMODE` turns it off.
- Objects on locked layers are skipped.
- If nothing needs to change, the command says so and makes no change. Otherwise the change can be undone in one step.
