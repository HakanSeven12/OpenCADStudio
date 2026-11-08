---
summary: Opens the Field dialog and places the chosen field as multiline text that updates itself.
related: UPDATEFIELD, MTEXT, TEXT
ribbon: Insert tab › Data panel
---

## Usage
1. Type `FIELD`. The Field dialog opens.
2. Pick a **Category**, then a field name. Set its format and options. The **Preview** and the **Field expression** update as you go.
3. Click **Insert**.
4. At `Specify start point or [Height/Justify]:`, click where the text goes.

## Options
| Option | What it does |
|---|---|
| **Height** (`H`) | Type the text height, or pick two points. |
| **Justify** (`J`) | Choose the attachment point: `TL`, `TC`, `TR`, `ML`, `MC`, `MR`, `BL`, `BC` or `BR`. The default is `TL`. |

## Field categories
| Category | Fields |
|---|---|
| Date & Time | CreateDate, Date, PlotDate, SaveDate |
| Document | Author, Comments, Filename, Filesize, HyperlinkBase, Keywords, LastSavedBy, Subject, Title |
| Linked | Hyperlink |
| Objects | BlockPlaceholder, Count, CountInArea, Formula, NamedObject, Object |
| Other | DieselExpression, SystemVariable |
| Plot | DeviceName, Login, PageSetupName, PaperSize, PlotDate, PlotOrientation, PlotScale, PlotStyleTable, and more |

**All** lists every field.

## Notes
- For an **Object** field, click **Select object** in the dialog, pick the object in the drawing, then choose the property to show.
- For a **Formula** field, the **Average**, **Sum**, **Count** and **Cell** buttons let you pick a cell or a range of cells in a table.
- The text is placed in the current text style.
- Use [`UPDATEFIELD`](UPDATEFIELD-Command) to refresh field values.
