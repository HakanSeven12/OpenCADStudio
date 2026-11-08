---
summary: Controls whether new 3D solids keep a history of the shapes they were built from.
type: On/Off (0/1)
saved: Drawing
default: 0 (Off)
related: SHOWHIST
---

## Values
| Value | Meaning |
|---|---|
| 0 | No history is recorded. |
| 1 | Solids made by solid operations, such as union, subtract, intersect and slice, keep the original shapes in their history. |

## Notes
- Whether that history is shown is set by [`SHOWHIST`](SHOWHIST-System-Variable).
- Changing the value can be undone. It is also in Options › 3D Modeling.
