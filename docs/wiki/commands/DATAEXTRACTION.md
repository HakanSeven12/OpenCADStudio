---
summary: Extracts object properties and block attribute values into a table in the drawing, an external file, or both.
related: DATALINK, DATALINKUPDATE, TABLE, EATTEXT
ribbon: Insert tab › Linking & Extraction panel
web: In the web app, extraction settings files are downloaded instead of saved next to the drawing.
---

## Usage
`DATAEXTRACTION` opens the **Data Extraction** wizard. Move through the pages with **Next** and **Back**.
1. **Begin**: choose **Create a new data extraction**, **Use a previous extraction as a template (.dxex)**, or **Edit an existing data extraction**, and give the settings file.
2. **Define data source**: use the current drawing, the selected objects in the current drawing, or a list of drawings and folders (with **Include current drawing** and **Include subfolders**).
3. **Select objects**: tick the object types and blocks to read. Filters let you show blocks only, non-blocks only, blocks with attributes only, or objects in use only.
4. **Properties**: tick the properties to extract. Use the category filter to shorten the list.
5. **Refine data**: choose **Combine identical rows**, **Show count column** and **Show name column**.
6. **Choose output**: **Insert data extraction table into drawing**, **Output data to external file (.xlsx .csv .txt)**, or both.
7. **Table style**: pick the table style, a title, and the cell styles for the title, header and data rows.
8. **Finish**: click **Finish**. The external file is written, and if you chose a table, specify its insertion point.

## Command-line form
`-DATAEXTRACTION` runs a saved extraction without the wizard. It asks:
1. Enter the template file path for the extraction (a `.dxex` file). A relative path is looked up next to the drawing first.
2. If the extraction makes a table, specify its insertion point.

This form suits scripts.

## Notes
- The drawing must be saved first. If it is not, a notice offers to save it before the wizard opens.
- The settings are saved in a `.dxex` file, so the same extraction can be run again or used as a template.
- An extraction table stays linked to its settings. [`DATALINKUPDATE`](DATALINKUPDATE-Command) runs the extraction again and refreshes the table.
