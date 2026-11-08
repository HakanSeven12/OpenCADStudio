---
summary: Sets whether a right-click over an object runs its default command at once.
type: On/Off (0/1)
saved: Program settings
default: 0 (Off)
related: QUAD, QUADWIDTH, SHORTCUTMENUDURATION
---

## Values
| Value | Meaning |
|---|---|
| 0 | A right-click shows the Quad menu as usual. |
| 1 | A quick right-click over an object runs the default command for that object type, without a menu. |

## Notes
- The default command is the one you used last on that object type, or else the first command of its Quad menu.
- A right-click held longer than [`SHORTCUTMENUDURATION`](SHORTCUTMENUDURATION-System-Variable) opens the Quad menu for no selection instead.
- It works only when [`QUAD`](QUAD-System-Variable) is on and no command is running.
- Set it in [`OPTIONS`](OPTIONS-Command), in the Quad Cursor Menu section.
