---
summary: Imports the elements of a DGN file as drawing objects.
related: DGNATTACH, IMPORT, PDFIMPORT
ribbon: Insert tab › Import panel
---

## Usage
1. Choose a DGN file in the file dialog.
2. In the DGN Import dialog, set the options:
   - **Model**: the design model to import. Sheet models are not listed.
   - **Target**: import into a **New Drawing** or the **Current Drawing**. For the current drawing, choose **Add prefix to dependent definitions** or **Ignore duplicate names** for names that already exist.
   - **Units**: use the file's **Master unit** or **Sub unit**.
   - **Explode text nodes to text elements**: imports each line of a text node as its own text object.
   - **External DGN references**: bind them as blocks or references, ignore them, or attach them as DGN underlays. You also choose what happens when a translated drawing already exists: prompt, overwrite, or keep it.
   - **Property mapping**: shows how the standard mapping setup turns DGN colors, levels, line styles and line weights into drawing colors, layers, linetypes and lineweights.
3. Click **Import**.

## Command-line form
`-DGNIMPORT` asks on the command line, which suits scripts:
1. `Enter filename for DGN import:` Type the file path.
2. `Enter name of model or [?]:` Type a model name, press Enter for the first one, or type `?` to list the models.
3. `Specify conversion units [Master/Sub] <Master>:` Type `M` or `S`.
4. `Specify mapping setup or [?] <Standard>:` Type a mapping setup name, press Enter for the standard one, or type `?` to list them.
   - A setup name may carry the V7 text code page as a suffix: `Standard;codepage=1250` reads the file's text as Windows-1250 (Czech/Slovak MicroStation writes), and the picker's other pages (`1251`, `1253`-`1258`, `874`, `932`, `936`, `949`, `950`, `65001` for UTF-8) work the same. `Standard;codepage=1252` selects the default explicitly.

The interactive dialog exposes the same choice for V7 files as the **V7 text code page** card: the Windows pages the decoder supports - Windows-1252 the default, plus 1250, 1251, 1253-1258, Windows-874, Shift-JIS (932), GBK (936), Korean Wansung (949), Big5 (950) and UTF-8 (65001). It shows the stored setting and its choice becomes the stored one when the import runs; the model preview and mapping rows re-decode right away.

## Notes
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, `DGNIMPORT` uses the command-line form.
- [`DGNIMPORTMODE`](DGNIMPORTMODE-System-Variable) sets the target the dialog starts with.
- V7 files name no text code page. The default decoding is Windows-1252; Czech and Slovak Czech files (and other Western/Central European pages) need the explicit code page above.
- You can also open DGN files through [`IMPORT`](IMPORT-Command).
- Invalid or unsupported DGN files are rejected with a message.
