---
summary: Changes the fade, contrast and monochrome display of DWF underlays.
related: ADJUST, DWFATTACH, DWFCLIP, PDFADJUST
---

## Usage
1. Select the DWF underlays, or start the command and select them, then press Enter.
2. Choose an option. Press Enter for the default shown in brackets.
3. Enter a value from 0 to 100, or answer Yes or No for **Monochrome**. Press Enter to keep the value shown.

## Options
| Option | What it does |
|---|---|
| **Fade** (`F`) | Sets how faded the underlay looks (0 to 100). |
| **Contrast** (`C`) | Sets the underlay contrast (0 to 100). |
| **Monochrome** (`M`) | Shows the underlay in one color (`Y`) or in full color (`N`). |

## Notes
- Only DWF underlays are taken from the selection. [`ADJUST`](ADJUST-Command) works on images and on PDF and DGN underlays too.
- Underlays on locked layers are skipped.
- The option and the values you used last become the defaults for the next run.
