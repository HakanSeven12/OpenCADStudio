---
summary: Opens the table style manager, where you create, edit, rename and delete table styles.
related: TABLE, CTABLESTYLE, STYLE
ribbon: Annotate tab › Tables panel
---

## Usage
1. Type `TABLESTYLE`, or use the Table Style button on the ribbon.
2. Pick a style in the list. **Used in drawing** filters the list to styles that tables use.
3. Use **New from this**, **Rename** or **Delete** to manage styles.
4. Edit the selected style on four pages:
   - **General**: **Description**, **Annotative**, the **Starting table** (**Select table…** picks a table in the drawing that new tables start from; **Remove** clears it), **Table direction**, and whether to suppress the title or header row.
   - **Cell Properties**: for the chosen cell style (**Title**, **Header**, **Data**), the fill color, alignment, format, data type, margins and **Merge cells on row/column creation**.
   - **Cell Text**: text style, text height, text color and text angle.
   - **Cell Borders**: lineweight, linetype, color, and double lines with spacing, for each edge.
5. A preview shows the result.

## Notes
- `TABLESTYLE` can also take words after it in scripts:
  - `TABLESTYLE LIST` (or `?`) lists the table styles and their margins.
  - `TABLESTYLE NEW <name>` creates a style.
  - `TABLESTYLE SET <name>` makes a style current.
