---
summary: Controls which OLE objects are shown.
type: Integer
saved: Program settings
default: 0 (all shown)
related: INSERTOBJ, OLEFRAME, OLEQUALITY
---

## Values
| Value | Meaning |
|---|---|
| 0 | All OLE objects are shown. |
| 1 | Only OLE objects in paper space are shown. |
| 2 | Only OLE objects in model space are shown. |
| 3 | No OLE objects are shown. |

## Notes
- Applies to every open drawing. A value outside 0 to 3 is refused.
