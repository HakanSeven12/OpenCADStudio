---
summary: Changes fade, contrast, visibility, monochrome and clipping of selected PDF, DWF and DGN underlays.
related: PDFATTACH, DWFATTACH, DGNATTACH, ADJUST, PDFADJUST, CLIP
---
## Usage
1. Select the underlays and press Enter. You can also select them before you start.
2. At `UNDERLAY [Fade / Contrast / On / Off / Mono / Clip]:`, type an option.
3. Answer the option's prompt, if it has one.

The command line reports how many underlays were updated.

## Options
| Option | What it does |
|---|---|
| **Fade** (`FADE`) | Type a fade value from 0 to 100. Higher values make the underlay lighter. |
| **Contrast** (`CONTRAST`) | Type a contrast value from 0 to 100. |
| **On** (`ON`) | Shows the underlays. |
| **Off** (`OFF`) | Hides the underlays. |
| **Mono** (`MONO`) | Type `On` to show the underlays in one color, or `Off` to show their own colors. |
| **Clip** (`CLIP`) | Type `On` to turn clipping on, or `Off` to remove the clip boundary. |

## Notes
- You can type the option and value on one line, for example `UNDERLAY FADE 50`, which suits scripts.
- Other objects in the selection, and underlays on locked layers, are not changed.
- With nothing selected, `UNDERLAY <option>` reports `select underlay entities first`.
- You can undo the change.
