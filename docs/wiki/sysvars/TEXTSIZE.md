---
summary: Sets the default height of new text drawn with a text style that has no fixed height.
type: Real
saved: Drawing
default: 2.5
related: TEXT, MTEXT, TEXTSTYLE, SCALETEXT
---

## Values
Any number, in drawing units. With 0 or less, the last height used with the current text style is taken instead (or 2.5).

## Notes
- [`TEXT`](TEXT-Command) remembers the height you enter here.
- The text height box on the ribbon sets the same value, and [`SCALETEXT`](SCALETEXT-Command) offers it as its default height.
- When the drawing units change, this height is converted with them.
