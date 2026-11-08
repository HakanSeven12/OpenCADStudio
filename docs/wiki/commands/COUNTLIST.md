---
summary: Opens the Count palette, which lists every block in the drawing with its count.
related: COUNT, COUNTLISTCLOSE, COUNTAREA, COUNTTABLE, COUNTFIELD
ribbon: View tab › Palettes panel
---

## Usage
1. Run `COUNTLIST`. The Count palette opens.
2. Click a block in the list to count it. The drawing enters count mode.

## Palette
| Control | What it does |
|---|---|
| **Search** | Filters the list by name. |
| **Name** / **Count** headers | Sort the list by name or by count. |
| Row menu | **Review Count Details**, **Insert Count Field**, or expand the row by **Layer**, **Scale** or **Mirror State**. |
| **Count in a specified area** | Runs [`COUNTAREA`](COUNTAREA-Command). |
| **Create Table** | Check the blocks to include, then click **Insert** and pick a point to place a count table. |

While a block is counted, the palette shows its count, the **Match Layer**, **Match Scale** and **Match Mirror State** check boxes, and a **Count Error Report** that lists overlapping objects. Click one to show it. **Back to List** returns to the list.

## Notes
- In the web app the palette floats in the middle of the screen instead of opening a separate window.
