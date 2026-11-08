---
summary: Gathers several block multileaders into one group with a single leader, in a row, a column or wrapped rows.
related: MLEADER, MLEADERALIGN, MLEADEREDIT
ribbon: Annotate tab › Leaders panel
---

## Usage
1. Select multileaders with block content, then press Enter.
2. Specify the location of the collected multileader, or type an option to change the layout.
3. The blocks are placed side by side from that point. The last multileader you selected keeps its leader; the others lose theirs.

## Options
| Option | What it does |
|---|---|
| **Vertical** (`V`) | Stacks the blocks in a column. |
| **Horizontal** (`H`) | Places the blocks in a row. This is the default the first time you run the command. |
| **Wrap** (`W`) | Starts a new row when the row would exceed the wrap width you enter. Type **Number** (`N`) to set how many blocks go in each row instead. |

## Notes
- Only multileaders whose content is a block can be selected.
- The layout carries over to the next run.
- Multileaders on locked layers are skipped.
