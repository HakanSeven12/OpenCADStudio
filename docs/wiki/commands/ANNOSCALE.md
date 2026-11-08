---
summary: Sets the current annotation scale, which controls the size of annotative objects in model space.
related: SCALELISTEDIT, OBJECTSCALE, ANNOUPDATE, ANNORESET
---

## Usage
1. Type the new annotation scale, for example `1:50`, `2:1`, or a plain factor.
2. Press Enter.

You can also type the value on the same line, for example `ANNOSCALE 1:50`.

## Notes
- `CANNOSCALE` runs the same command.
- The scale is stored in the drawing, so it is kept when you save.
- If you type the command with an empty value, the command line shows the current annotation scale.
- If the value is not a valid scale, an error with the expected format is shown.
- When [`ANNOAUTOSCALE`](ANNOAUTOSCALE-System-Variable) is greater than 0, changing the scale also adds the new scale to annotative objects that supported the previous scale.
