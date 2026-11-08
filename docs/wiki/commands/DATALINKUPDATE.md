---
summary: Updates linked tables from their source files, or writes linked tables back to their files.
related: DATALINK, DATAEXTRACTION
ribbon: Insert tab › Linking & Extraction panel
---

## Usage
1. Select an option [Update data link/Write data link] <Update data link>. Press Enter to update.
2. For **Update data link**: select the linked tables and press Enter, or type an option to update by link name or to update all links.
3. For **Write data link**: select the linked tables and press Enter.

The command line reports how many objects were found and how many were updated or written.

## Options
| Option | What it does |
|---|---|
| **Update data link** (`U`) | Reads the source files and refreshes the tables. On the ribbon this is **Download from Source**. |
| **Write data link** (`W`) | Writes the table contents back to the source files. On the ribbon this is **Upload to Source**. |
| **Data link** (`D`) | Asks for a data link name and updates the tables that use it. Type `?` to list the drawing's data links. |
| **all data linKs** (`K`) | Updates every linked table in the drawing. |

## Notes
- Only tables that have a data link can be selected.
- Writing works only for CSV and text sources, and only when the link allows writing to the source file (set in [`DATALINK`](DATALINK-Command)).
- A table made by [`DATAEXTRACTION`](DATAEXTRACTION-Command) is refreshed by running its saved extraction again.
- If the drawing has no data links, the command says so and suggests [`DATALINK`](DATALINK-Command).
