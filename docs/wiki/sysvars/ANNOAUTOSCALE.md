---
summary: Adds a newly set annotation scale to existing annotative objects, and selects which objects get it.
type: Integer
saved: Program settings
default: -4 (Off; mode 4 is remembered)
related: CANNOSCALE, ANNOALLVISIBLE, ANNOSCALE
---

## Values
| Value | Meaning |
|---|---|
| 1 | Add the scale, but skip objects on layers that are off, frozen or locked |
| 2 | Add the scale, but skip objects on layers that are off or frozen |
| 3 | Add the scale, but skip objects on locked layers |
| 4 | Add the scale to all annotative objects |
| 0 or a negative value | Off. A negative value keeps its mode for when you turn it on again |

The accepted range is -4 to 4.

## Notes
- The status bar button turns it on and off by changing the sign.
- Options › User Preferences sets the mode.
