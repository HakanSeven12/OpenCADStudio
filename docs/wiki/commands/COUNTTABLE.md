---
summary: Inserts a table that lists blocks and their counts, with each count kept up to date by a field.
related: COUNT, COUNTLIST, COUNTFIELD, UPDATEFIELD, TABLE
---

## Usage
1. Enter the block names to include, separated by commas. Press Enter to include all blocks.
2. Specify the insertion point of the table.

## Options
| Option | What it does |
|---|---|
| **?** | Asks which blocks to list (wildcards allowed, `*` by default), lists the matching blocks page by page, shows totals of user blocks, external references, dependent blocks and unnamed blocks, and ends the command. |

## Notes
- The table has two columns, **Item** and **Count**, with one row per block, sorted by name.
- With all blocks, the table lists every block that has references. If a count area is set, the counts are taken inside it.
- A name that does not match a block ends the command with an error.
- The counts are fields, so they update with the drawing.
