---
summary: Colors objects in the Block Editor by how far they are constrained.
type: On/Off (0/1)
saved: Not saved
default: 0 (Off)
related: BEDIT, BESETTINGS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Objects keep their normal colors |
| 1 | Objects are colored by constraint status: unconstrained, partially constrained, fully constrained, or over-constrained |

You can also type TOGGLE to switch the value.

## Notes
- It works only while the Block Editor is open, and is turned off again each time the Block Editor opens a block.
- The four status colors are set in the [`BESETTINGS`](BESETTINGS-Command) dialog.
