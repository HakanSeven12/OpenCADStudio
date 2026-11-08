---
summary: Changes the fade, contrast and monochrome display of DGN underlays.
related: ADJUST, DGNATTACH, DGNCLIP, PDFADJUST, DWFADJUST
---

## Usage
1. Select one or more DGN underlays and press Enter. Underlays you selected before running the command are used directly.
2. Choose the setting to change: type an option, or press Enter for the one shown in angle brackets.
3. Enter the new value, or press Enter to keep the value shown.

## Options
| Option | What it does |
|---|---|
| **Fade** (`F`) | Asks for a fade value from 0 to 100. |
| **Contrast** (`C`) | Asks for a contrast value from 0 to 100. |
| **Monochrome** (`M`) | Asks **Yes** (`Y`) or **No** (`N`) to show the underlay in one color. |

## Notes
- The values must be whole numbers between 0 and 100.
- The default option is the one you used last in the session.
- All selected underlays must be DGN underlays; a mix of underlay types is rejected.
