---
summary: Controls which events update the fields in the drawing.
type: Integer (bit-coded)
saved: Drawing
default: 31 (all events)
related: FIELDDISPLAY, FIELD, UPDATEFIELD, DXEVAL
---

## Values
Add the values of the events you want. The accepted range is 0 to 31.

| Value | Meaning |
|---|---|
| 0 | Fields update only when you update them yourself. |
| 1 | Update when the drawing opens. |
| 2 | Update when the drawing is saved. |
| 4 | Update when the drawing is plotted (plot date fields get the plot time). |
| 8 | Update when the drawing is archived. |
| 16 | Update when the drawing is regenerated with [`REGEN`](REGEN-Command). |

## Notes
- [`UPDATEFIELD`](UPDATEFIELD-Command) always updates the selected fields, whatever this value is.
- Packaging a drawing does not update fields yet, so bit 8 has no effect.
- Changing the value can be undone.
