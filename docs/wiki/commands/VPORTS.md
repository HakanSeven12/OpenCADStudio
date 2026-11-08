---
summary: Splits model space into tiled viewports, or creates, saves and restores viewport layouts on a paper layout.
related: VPJOIN, MVIEW, HORIZONTAL, VERTICAL, CASCADE, SYNCPVIEWPORTS
ribbon: View tab › Model Viewports panel
---

## Usage
In model space:
1. Run `VPORTS`.
2. At `VPORTS  Configuration [SIngle/2H/2V/4]:`, type a configuration and press Enter. Enter alone gives a single viewport.

On a paper layout:
1. Run `VPORTS` to list the viewports in the layout with their size, position, scale and state.
2. Type `VPORTS` followed by a configuration on the same line (for example `VPORTS 4`) to replace the layout's viewports with a preset arrangement that fills the printable area.

## Options
| Option | What it does |
|---|---|
| **SIngle** (`1`) | One viewport. In model space `SI` also works. |
| **2H** / **2V** | Two viewports, split horizontally or vertically. In model space `2` gives `2H`. |
| **3R**, **3L**, **3A**, **3B** (`3` = `3R`) | Three viewports, with the large one on the right, left, above or below. Model space only. |
| **3V** / **3H** | Three equal viewports side by side or stacked. Model space only. |
| **4** | Four equal viewports. |
| **SAVE** `<name>` | Saves the current layout's viewport rectangles under a name, stored in the drawing. Paper layouts only. |
| **RESTORE** `<name>` | Re-creates a saved configuration on the current layout. Paper layouts only. |
| **LIST** | Lists the saved configurations and how many viewports each holds. |
| **DELETE** `<name>` | Deletes a saved configuration. |

## Notes
- The ribbon's **Viewport Configuration** drop-down offers the model space layouts by picture (Single, Two: Vertical, Two: Horizontal, Three: Right/Left/Above/Below/Vertical/Horizontal, Four: Equal).
- On a paper layout, the preset arrangements and **RESTORE** erase the layout's existing viewports first. If any of them is on a locked layer, nothing changes.
- `-VPORTS` does the same and suits scripts.
- The command is not available inside the block editor.
- To type an option and its name on one line, start the line with `>` so that Space does not end the entry, for example `>VPORTS SAVE Plan`.
