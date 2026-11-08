---
summary: Manages links between tables and external spreadsheet or CSV files, and inserts linked tables.
related: DATALINKUPDATE, DATAEXTRACTION, TABLE
ribbon: Insert tab › Linking & Extraction panel
web: In the web app, a typed file path is not used; you pick the file instead.
---

## Usage
1. Start the command. The **Data Link Manager** opens with the drawing's data links on the left.
2. Click **Create a new data link**, or select a link and click **Edit**.
3. Enter the **Data link name**, choose the **File** (CSV, TXT, XLS, XLSX, XLSB or ODS), and choose the **Path type** (**Full path**, **Relative path** or **No path**).
4. For spreadsheet files, choose what to link: **Entire sheet**, a **Named range**, or a **Cell range**.
5. Click **OK** to save the link. The **Preview** shows the linked data.
6. To place the data, select the link and click **Insert Table**, then specify the insertion point.

Other buttons: **Delete** removes the selected link, **Close** closes the manager.

## Command-line form
Type `DATALINK` followed by the path of a CSV file (for example `DATALINK C:\data\parts.csv`) to skip the manager. The file is read and you specify the insertion point of the new linked table.

## Notes
- **More options** in the editor holds **Allow writing to source file**, **Use source formatting** and **Keep table updated to source formatting**.
- If the file cannot be found, the details show "Status: Source not found".
- Use [`DATALINKUPDATE`](DATALINKUPDATE-Command) to refresh a linked table after the file changes, or to write the table back to the file.
