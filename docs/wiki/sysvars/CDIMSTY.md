---
summary: Sets the current dimension style, used by new dimensions.
type: String
saved: Drawing
default: Standard
related: DIMSTYLE
---

## Values
The name of a dimension style that exists in the drawing.

## Notes
- Type `CDIMSTY` and then the style name, or `CDIMSTY name` in one line. `DIMCURRENT` does the same.
- A name that does not exist gives an error and keeps the current style.
- The dimension style list on the ribbon also sets it. [`DIMSTYLE`](DIMSTYLE-Command) manages the styles.
