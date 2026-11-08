---
summary: Loads, clears or reports the plot style table (CTB) used for plotting.
related: PLOTSTYLEEDITOR, PLOT, PAGESETUP
web: In the web app, a table you load is kept for the current session only.
---

## Usage
1. Type `PLOTSTYLE`. The command line shows the active table, and a file dialog opens.
2. Choose a CTB file.

The loaded table becomes the plot style table of the Plot dialog. The command line reports how many colour entries it has.

You can also type a word after the command:

## Options
| Option | What it does |
|---|---|
| **LOAD** | Opens the file dialog to load a table. Same as `PLOTSTYLE` alone. |
| **CLEAR** (or **NONE**) | Removes the active plot style table. |
| **STATUS** (or **?**) | Shows the name of the active table and how many colour overrides it has. |

For example, type `PLOTSTYLE CLEAR`.

## Notes
- Named plot style tables (STB) cannot be used for plotting; loading one shows an error.
- Use [`PLOTSTYLEEDITOR`](PLOTSTYLEEDITOR-Command) to change the entries of a table.
