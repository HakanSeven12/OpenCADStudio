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

## Notes
- When [`FILEDIA`](FILEDIA-System-Variable) is 0, `DGNIMPORT` uses the command-line form.
- [`DGNIMPORTMODE`](DGNIMPORTMODE-System-Variable) sets the target the dialog starts with.
- You can also open DGN files through [`IMPORT`](IMPORT-Command).
- Invalid or unsupported DGN files are rejected with a message.
