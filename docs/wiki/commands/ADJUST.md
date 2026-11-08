---
summary: Changes the brightness, contrast or fade of raster images, or the fade, contrast or monochrome display of PDF, DWF and DGN underlays.
related: IMAGEADJUST, PDFADJUST, IMAGEATTACH, PDFATTACH
ribbon: Insert tab › Reference panel
---

## Usage
1. Select the images or underlays, or start the command and select them.
2. Choose an option. Press Enter for the default shown in brackets.
3. Enter a value from 0 to 100, or answer Yes or No for Monochrome. Press Enter to keep the value shown.

## Options
For raster images:

| Option | What it does |
|---|---|
| **Contrast** (`C`) | Sets the image contrast (0 to 100). |
| **Fade** (`F`) | Sets how faded the image looks (0 to 100). |
| **Brightness** (`B`) | Sets the image brightness (0 to 100). This is the default. |

For underlays:

| Option | What it does |
|---|---|
| **Fade** (`F`) | Sets how faded the underlay looks (0 to 100). |
| **Contrast** (`C`) | Sets the underlay contrast (0 to 100). |
| **Monochrome** (`M`) | Shows the underlay in one color (`Y`) or in full color (`N`). |

## Notes
- All selected objects must be of one kind: images only, or underlays of one file type. A mixed set is rejected and you are asked to select again.
- Objects on locked layers are skipped.
- For underlays, the option and the fade and contrast values you used last become the defaults for the next run.
- The change applies to every selected object and can be undone in one step.
