# Leaders and Tables

## Multileaders

A multileader is an arrow and a line pointing at something, with text or a block at the other end. One multileader can have several leader lines.

| | Tool | Command | What it does |
|---|---|---|---|
| <img src="images/icons/mleader.svg" width="24" alt=""> | Multileader | `MLEADER` (`MLD`) | Draws a multileader. Place the arrowhead first, the landing first or the content first. |
| <img src="images/icons/mleader_add.svg" width="24" alt=""> | Add Leader | `MLEADEREDIT` (`MLE`) | Adds leader lines to a multileader. |
| <img src="images/icons/mleader_remove.svg" width="24" alt=""> | Remove Leader | `AIMLEADEREDITREMOVE` | Removes leader lines from a multileader. |
| <img src="images/icons/mleader_align.svg" width="24" alt=""> | Align | `MLEADERALIGN` (`MLA`) | Lines up several multileaders. |
| <img src="images/icons/mleader_collect.svg" width="24" alt=""> | Collect | `MLEADERCOLLECT` (`MLC`) | Gathers multileaders with block content into one. |
| <img src="images/icons/leader.svg" width="24" alt=""> | Leader | `LEADER` (`LEAD`), `QLEADER` (`LE`) | The older leader object, still used in many drawings. |

`MLEADERSTYLE` (`MLS`) manages multileader styles: the arrowhead and its size, the leader line (straight or spline), the landing, and the content (multiline text, a block such as a balloon, or none), with text style and attachment.

## Tables

<img src="images/icons/table.svg" width="24" alt=""> `TABLE` (`TB`) inserts a table: give the number of columns and rows, then place it. You can start an empty table, or fill it from a data link or from data extracted from the drawing.

### Editing cells

Click a cell to select it; Shift+click or drag selects a range. While cells are selected, the **Table Cell** tab appears on the ribbon:

| | Tool |
|---|---|
| <img src="images/icons/table_row_above.svg" width="20" alt=""> <img src="images/icons/table_row_below.svg" width="20" alt=""> | Insert a row above or below |
| <img src="images/icons/table_row_delete.svg" width="20" alt=""> | Delete rows |
| <img src="images/icons/table_col_left.svg" width="20" alt=""> <img src="images/icons/table_col_right.svg" width="20" alt=""> | Insert a column to the left or right |
| <img src="images/icons/table_col_delete.svg" width="20" alt=""> | Delete columns |
| <img src="images/icons/table_merge.svg" width="20" alt=""> <img src="images/icons/table_unmerge.svg" width="20" alt=""> | Merge or unmerge cells |
| <img src="images/icons/table_cell_borders.svg" width="20" alt=""> | Cell borders |
| <img src="images/icons/table_align_mc.svg" width="20" alt=""> | Cell alignment (nine positions) |
| <img src="images/icons/table_cell_lock.svg" width="20" alt=""> | Lock content or format |
| <img src="images/icons/table_cell_format.svg" width="20" alt=""> | Data format: number, currency, date, percentage and more |
| <img src="images/icons/table_cell_contents.svg" width="20" alt=""> | Insert a block or a field |
| <img src="images/icons/table_cell_formula.svg" width="20" alt=""> | Insert a formula |

Double-click a cell to type in it (`TABLEDIT`).

**Grips.** The grips of a selected range change the width of its columns and the height of its rows; the top and left grips move the edge shared with the neighbouring row or column. The diamond grip in the corner of a range fills the cells you drag it over: numbers continue as a series, other contents are copied.

**Formulas.** A cell can hold a formula that starts with `=`, for example `=A1*B1` or `=SUM(B2:B10)`. Supported functions: `SUM`, `AVERAGE`, `COUNT`, `MIN` and `MAX`. The result updates when the cells it uses change.

**Breaking a table.** A long table can be split into several parts side by side or one below the other, with the header repeated; drag the break grip at the bottom of the table.

### Table styles and data

- `TABLESTYLE` manages table styles: title, header and data cell styles with their text, borders, margins and fill.
- `DATALINK` (`DL`) links a table to an external spreadsheet or CSV file; `DATALINKUPDATE` (`DLU`) refreshes it.
- `DATAEXTRACTION` (`DX`) collects properties of objects and block attributes in the drawing into a table or a CSV file.
- `COUNT` counts block references and can put the result in a table.

## Geometric tolerance

<img src="images/icons/tolerance.svg" width="24" alt=""> `TOLERANCE` (`TOL`) creates a feature control frame: the tolerance symbol, the tolerance values with diameter and material conditions, and datum references.
