---
summary: Sets whether the view turns to the plan view of the UCS when the UCS changes.
type: On/Off (0/1)
saved: Drawing
default: 0 (Off)
related: UCS, PLAN
---

## Values
| Value | Meaning |
|---|---|
| 0 | The view stays as it is |
| 1 | The view follows the UCS and shows its plan view |

## Notes
- The value belongs to the current viewport: the Model view, or the active viewport of a layout.
- In a paper layout, activate a viewport first. Paper space itself cannot follow the UCS.
- Setting 1 also turns the view to the current UCS at once.
