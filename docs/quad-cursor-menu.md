# Quad cursor menu

The Quad is a command menu that opens at the mouse cursor and offers only the
commands that make sense for what is on screen: the entity the cursor hovers, a
selection, or nothing at all. It is off by default and one app-wide setting.

Turning it on never changes how drawing works — it only takes over some
right-click and hover moments, and each of those moments can be switched back
off individually.

## Turning it on

| Where | Action |
| --- | --- |
| Keyboard | `SHIFT+F12` toggles the quad. |
| Status bar | Click the `QUAD` pill to toggle. |
| Options | Options → Quad Cursor Menu — master toggle, display flags, panel width, launch and dwell. |
| Command line | `SETVAR QUAD 1` (or `0`), also `QUADWIDTH` and `QUADCOMMANDLAUNCH`. |

The master switch only unlocks the feature. Each moment has its own flag, all
on by default, all reachable from the pill's caret menu, the Options
section, and the same settings:

| Flag | Moment |
| --- | --- |
| Offer the quad while hovering | The cursor rests on an entity. |
| Offer the quad for a selection | A right-click with a selection. |
| Right-click in the void opens it | A right-click with nothing under the cursor. |
| Suppress the hover quad while selected | Hovering inside an existing selection keeps it quiet. |

## Hovering an entity

Hover the line, hatch, insert, dimension, and the quad opens next to the cursor
after a short rest (300 ms by default; options cover 0–2000 ms). It starts as
one tile: the **default command** for that entity type — the command you most
recently used on that type, falling back to a built-in (for a line that is
`MOVE`). Move the cursor onto the tile and it expands into a grid —
`QUADWIDTH` columns wide (2–12, default 6) — of the most recently used
commands for the type, with the remaining applicable commands grouped into tabs
along the bottom. Picking a tile runs the command on the hovered entity; if the
entity is not in the selection first, it becomes the selection so every command
and undo step behaves exactly like its command-line twin.

The quad closes when you leave it, drift away, or open something else. Without
the hover flag (or while hushed over a selection), hovering stays highlighting
only, as before.

## Right-click

A right-click backs three moments, checked in this order:

| Moment | With the flags on | With the flags off |
| --- | --- | --- |
| On an entity, `QUADCOMMANDLAUNCH` set | A quick right-click launches the default command outright — even if a selection exists; a **held** right-click (from 250 ms) opens the No Selection quad instead. | The legacy right-click over the entity: menu or repeat-command per Right-Click mode. |
| Selection exists, quick right-click | The **Selection quad** — commands for the selection. | The legacy shortcut menu. |
| Nothing under the cursor, right-click | The **No Selection quad**. | The legacy shortcut menu. |

During an active command the quad never opens — right-clicks keep their in-command
behavior. `QUADCOMMANDLAUNCH` itself takes `0`/`1`; hold and mode combinations come
from the existing Right-Click options (Enter/Jump/Quick modes retain their meaning
around the quad).

## Keyboard

| Key | Effect while the quad is open |
| --- | --- |
| `1` … `9` | Run the matching tile of the primary panel. |
| `Enter` | Run the default tile. |
| `Tab` | Move to the next panel. |
| `Escape` | Close. |

Pending command-line text always wins: with something typed, digits and Enter
finish the command line and the quad steps aside.

## The No Selection quad

The No Selection quad is the starting place when nothing is hovered: a
**Recent** panel of your last commands, plus tabs for creating —
`Draw` (LINE, CIRCLE, ARC, ELLIPSE, RECT, PLINE, SPLINE, POINT, HATCH,
REVCLOUD), `Dimension` (DIMLINEAR, DIMALIGNED, DIMRADIUS, DIMDIAMETER,
DIMANGULAR, DIMJOGGED, DIMCENTER), `Annotate` (TEXT, MTEXT, LEADER, MLEADER,
TABLE, ATTDEF), and `Block` (INSERT, BEDIT, ATTEDIT, XATTACH). The Recent panel
fills from creation commands until you have launch history of your own.

## Plugins

Plugins join the quad by registering named quad panels bound to entity types —
the plugin [template](plugin-template-v2/DESIGN.md) hooks an "Insert Extras"
tab to `INSERT` entities with a custom icon, so hovering a block reference
offers it alongside the built-ins. A plugin's plain commands stay in the
command line and its auto-complete; only an explicit quad registration
becomes a tab. A callback tile hands the hit to the plugin with the
quad target's entities; a command tile runs the host command with the selection
aligned, as above. Registered panels obey the display flags like built-in ones.

## Settings summary

| Name | Meaning | Default |
| --- | --- | --- |
| `quad` | Master on/off. | off |
| `quad_on_hover` | Open while hovering. | on |
| `quad_on_select` | Open for a selection right-click. | on |
| `quad_on_right_click` | Open in the void right-click. | on |
| `quad_suppress_hover_when_selection` | No hover quad over a live selection. | on |
| `quad_width` | Grid columns, `QUADWIDTH` (2–12). | 6 |
| `quad_command_launch` | Quick right-click launches the default command (`QUADCOMMANDLAUNCH`). | off |
| `quad_hover_delay_ms` | Hover rest before opening (0–2000). | 300 |

The design, invariants, and plugin API surface are documented in
[quad-menu.md](quad-menu.md).
