---
summary: Sets the light used to shade point clouds.
type: On/Off (0/1)
saved: Drawing
default: 0 (Key and fill light)
related: POINTCLOUDLIGHTING
---

## Values
| Value | Meaning |
|---|---|
| 0 | A key light from the upper left and a fill light from the lower right. |
| 1 | One light from above. |

## Notes
- The lights follow the view.
- It has an effect only when [`POINTCLOUDLIGHTING`](POINTCLOUDLIGHTING-System-Variable) is 1 or 2.
