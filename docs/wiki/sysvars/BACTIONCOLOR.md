---
summary: Sets the color of actions in the Block Editor.
type: String (color)
saved: Program settings
default: 7
related: BPARAMETERCOLOR, BGRIPOBJCOLOR, BESETTINGS, BEDIT
---

## Values
| Value | Meaning |
|---|---|
| BYLAYER | The color of the current layer |
| BYBLOCK | The drawing's foreground color |
| 1 to 255 | A color index; 7 is the foreground color |
| RGB:r,g,b | A true color, for example RGB:255,128,0 |

## Notes
- The [`BESETTINGS`](BESETTINGS-Command) dialog sets this value too.
