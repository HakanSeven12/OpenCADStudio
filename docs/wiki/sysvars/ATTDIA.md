---
summary: Sets whether inserting a block asks for attribute values in a dialog or on the command line.
type: On/Off (0/1)
saved: Drawing
default: 1 (Dialog)
related: ATTREQ, INSERT, ATTDEF
---

## Values
| Value | Meaning |
|---|---|
| 0 | Attribute values are asked on the command line |
| 1 | Attribute values are asked in a dialog |

## Notes
- Only attributes that are not Constant and not Preset are asked.
- When [`ATTREQ`](ATTREQ-System-Variable) is 0, no values are asked at all.
