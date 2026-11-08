---
summary: Inserts a table, either empty, from a table style's starting table, from a data link, or from a data extraction.
related: TABLESTYLE, TABLEDIT, DATALINK, DATAEXTRACTION
ribbon: Annotate tab › Tables panel
---

## Usage
1. Type `TABLE`. The Insert Table dialog opens.
2. Pick a **Table style**. **Table Style…** opens the table style manager.
3. Under **Insert options**, choose how to start:
   - **Start from empty table**
   - **Start from table style** (uses the style's starting table; pick what to keep and add extra rows or columns)
   - **Start from data link** (pick a link; **Manage…** opens the data link manager)
   - **Start from data extraction**
4. Under **Insertion behavior**, choose **Specify insertion point** or **Specify window**.
5. Under **Column & row settings**, set **Columns**, **Column width**, **Data rows** and **Row height** (in lines).
6. Under **Set cell styles**, choose the cell style for the **First row**, **Second row** and **All other rows**.
7. Click OK, then pick the insertion point, or the two corners of the window.

## Command-line form
`-TABLE` asks everything at the command line, which suits scripts:
1. It shows the current table style, cell width and cell height.
2. `Enter number of columns or [Auto/from Style/data Link]`
3. `Enter number of rows or [Auto]`
4. `Specify insertion point or [Style/Width/Height]` (or, with Auto, `Specify first corner` and `Specify second corner`).

| Option | What it does |
|---|---|
| **Auto** | Columns or rows are worked out from a window you draw. |
| **from Style** | Starts from the starting table of a table style. You can then choose **Style**, **Rows** (additional rows), **Columns** (additional columns) and **Options** (keep **Label text**, **Data text**, **Formulas**, **fIelds**, **data liNks**, **Blocks**, **cell style Overrides**, each Yes/No). |
| **data Link** | Enter the name of a data link (or `?` to list them) to fill the table from it. |
| **Style** | Enter the table style name. |
| **Width** | Enter the cell width. |
| **Height** | Enter the minimum cell height, in lines. |

## Notes
- The dialog remembers the column, row, width and height values for the next table.
- **from Style** needs a table style that holds a starting table; otherwise you are told to add one with [`TABLESTYLE`](TABLESTYLE-Command).
- **data Link** needs a data link in the drawing; create one with [`DATALINK`](DATALINK-Command).
