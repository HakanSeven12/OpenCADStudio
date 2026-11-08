---
summary: Places a multiline text field that shows the current count and updates with the drawing.
related: COUNT, COUNTTABLE, UPDATEFIELD, FIELD
---

## Usage
1. Start count mode with [`COUNT`](COUNT-Command) or from the Count palette, and count something.
2. Run `COUNTFIELD`.
3. Specify the start point of the text, or type an option.

## Options
| Option | What it does |
|---|---|
| **Height** (`H`) | Sets the text height. |
| **Justify** (`J`) | Sets the justification: TL, TC, TR, ML, MC, MR, BL, BC or BR. The default is TL. |

## Notes
- The command only works during count mode. Otherwise it prints an error.
- The field counts the same thing as count mode: the block (with the Match Layer, Scale or Mirror State choices) or the selected objects. If a count area is set, the field counts inside it.
- The text uses the current text style and height.
- The **Insert Count Field** button on the count toolbar and in the Count palette does the same.
