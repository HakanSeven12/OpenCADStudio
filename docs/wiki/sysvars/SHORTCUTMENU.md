---
summary: Controls what a right-click in the drawing area does.
type: Integer (bit code)
saved: Program settings
default: 11 (shortcut menu)
related: SHORTCUTMENUDURATION
---

## Values
| Value | Meaning |
|---|---|
| 0 | While a command runs, the first right-click is Enter and a second right-click opens the shortcut menu. When no command runs, a right-click opens the menu. |
| 1–15 | A right-click always opens the shortcut menu. Reads back as 11. |
| 16–31 (bit 16 set) | Time-sensitive right-click: a quick click is Enter (or repeats the last command when idle); holding the button opens the shortcut menu. Reads back as 27. |

Accepted range: 0 to 31.

## Notes
- How long you must hold the button for the time-sensitive menu is set by [`SHORTCUTMENUDURATION`](SHORTCUTMENUDURATION-System-Variable).
- The same choice is in the Options dialog.
