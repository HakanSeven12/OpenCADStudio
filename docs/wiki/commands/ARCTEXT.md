---
summary: Places text along a selected arc, one single-line text object per character, each turned to follow the arc.
related: TEXT, MTEXT, ARC
---

## Usage
1. Select an arc, or select it at the prompt and press Enter.
2. Type the text to place along the arc and press Enter.

The characters are spread evenly from the start of the arc to its end.

You can also select the arc first and type the text on the same line, for example `ARCTEXT HELLO`.

## Notes
- If the selection holds no arc, an error asks you to select an arc first. Only the first selected arc is used.
- Each character becomes its own text object. The text height is 12% of the arc radius.
- Control characters are dropped. If nothing printable is left, nothing is placed.
- You can undo the result in one step.
