---
summary: Moves, justifies, resets or rotates the text of a dimension.
related: DIMEDIT, DIMSTYLE, DIMOVERRIDE
ribbon: Annotate tab › Dimensions panel
---

## Usage
1. Select a dimension.
2. Move the cursor and click to place the text there, or type an option.

The command ends after one change.

## Options
| Option | What it does |
|---|---|
| **Left** (`L`) | Puts the text at the left end of the dimension line. |
| **Right** (`R`) | Puts the text at the right end of the dimension line. |
| **Center** (`C`) | Puts the text in the middle of the dimension line. |
| **Home** (`H`) | Returns the text to the place the dimension style gives it. |
| **Angle** (`A`) | Asks for a text angle in degrees and rotates the text. |

## Notes
- **Left**, **Right** and **Center** work on linear and aligned dimensions.
- The slide-out of the Dimensions panel has **Text Angle**, **Left Justify**, **Center Justify** and **Right Justify** buttons. Each runs `DIMTEDIT` with that option already chosen; you only pick the dimension.
