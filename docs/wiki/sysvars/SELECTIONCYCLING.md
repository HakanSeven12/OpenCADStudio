---
summary: Controls whether clicking where objects overlap opens a list to pick from.
type: Integer
saved: Program settings
default: 0 (Off)
related: DSETTINGS
---

## Values
| Value | Meaning |
|---|---|
| 0 | Off. A click picks one object. |
| 1 | On. A click where objects overlap opens a list of the objects under the cursor. |
| 2 | Same as 1. |

## Notes
- 1 and 2 behave the same. When cycling is on, the variable reads back as 2.
- You can also turn it on in the Selection Cycling tab of [`DSETTINGS`](DSETTINGS-Command), in Options › Selection, or from the status bar.
