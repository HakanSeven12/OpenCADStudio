---
summary: Controls which grips a selected revision cloud shows.
type: On/Off (0/1)
saved: Program settings
default: 1 (On)
related: REVCLOUD, REVCLOUDCREATEMODE
---

## Values
| Value | Meaning |
|---|---|
| 0 (OFF) | One grip on every vertex of the cloud. |
| 1 (ON) | Grips on the corners of the cloud's path only (or the center and quadrants of a circular cloud). |

## Notes
- You can type 0/1 or OFF/ON.
- Clouds that are already selected change their grips at once.
- If the path of a cloud cannot be read back, it shows a grip on every vertex even when this is on.
