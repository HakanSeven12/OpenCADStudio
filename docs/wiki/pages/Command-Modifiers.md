# Command Modifiers

Small prefixes and keywords change how a command or an input is read.

## Prefixes on a command name

| Prefix | Example | Effect |
|---|---|---|
| `'` | `'ZOOM`, `'PAN`, `'HELP` | Runs the command **transparently**, inside the command that is running, without ending it. Works for navigation (PAN, ZOOM), drafting aids (ORTHO, GRID, SNAP, POLAR, OSNAP, DSETTINGS), some palettes, HELP, and FILTER and QSELECT at a selection prompt. |
| `-` | `-LAYER`, `-INSERT`, `-PLOT` | Runs the **command-line form** of a dialog command: it asks at the command line instead of opening a window. Use it in scripts. |
| `_` | `_LINE` | Accepted and ignored: the English command name, as scripts written for other languages use it. |

Drafting-aid keys (F3, F8, F10 and the rest) never end the running command either.

## Typing a whole line with spaces

Space normally works like Enter: it submits what you typed. To type a command and its answers on one line (`UCS Z 90`, `WSCURRENT C`, a text with spaces), start the line with `>`: `>UCS Z 90`. The **>** button beside the command line does the same for every line until you turn it off.

## Answering a point prompt

| Type | Meaning |
|---|---|
| `x,y` / `x,y,z` | Absolute coordinates. |
| `@dx,dy` | Relative to the last point. |
| `@d<a` | Polar: distance and angle from the last point. |
| `#x,y` | Absolute even while dynamic input expects relative values. |
| a number alone | **Direct distance**: that far in the direction of the cursor. |
| `END`, `MID`, `CEN`, `INT`, `PER`, `TAN`, `NEA`, `QUA`, `NOD`, `INS`, `EXT`, `APP`, `PAR`, `GCEN` | **Object snap override** for this one pick. `NON` turns snapping off for it. |
| `FROM` | Pick a base point, then give an offset from it. |
| `M2P` / `MTP` | The midpoint between two points you pick. |

See [Precision Drawing](Precision-Drawing) for more.

## Answering a selection prompt

| Type | Selects |
|---|---|
| `ALL` | Everything |
| `L` | The last object drawn |
| `P` | The previous selection |
| `W` / `C` | Window / crossing |
| `WP` / `CP` | Window polygon / crossing polygon |
| `F` | Fence |
| `G` | A named group |
| `R` / `A` | Remove mode / add mode |
| `U` | Undo the last selection step |
| `'FILTER`, `'QSELECT` | Selection by properties, handed to the running command |

See [Selecting Objects](Selecting-Objects).

## Repeating and cancelling

- **Enter** or **Space** at an empty prompt repeats the last command.
- **Esc** cancels the running command.
- **Up** and **Down** recall earlier input.
