---
summary: Chooses how to start and finish an automatic selection area.
type: Integer (0–2)
saved: Program settings
default: 2
related: PICKADD, PICKAUTO, PICKBOX
---

## Values
| Value | Meaning |
|---|---|
| 0 | Pick two corners with separate clicks. |
| 1 | Press, drag and release to select an area. |
| 2 | Allow both gestures, according to PICKAUTO. |

With PICKAUTO bit 4 set, dragging from empty space draws a lasso. With PICKAUTO set to 1 and PICKDRAG set to 2, click both corners for classic Window or Crossing selection.
