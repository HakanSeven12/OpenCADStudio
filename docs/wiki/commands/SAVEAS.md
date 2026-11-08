---
summary: Saves the current drawing under a new file name, location, format or version.
related: SAVE, QSAVE, SAVEALL, EXPORT
web: In the web app the dialog saves through the browser; the command-line form opens the dialog instead.
---

## Usage
1. Type `SAVEAS`. The Save As dialog opens.
2. Choose the file format and version, and the file name.
3. Confirm. The drawing now uses the new file.

## Command-line form
When file dialogs are turned off ([`FILEDIA`](FILEDIA-System-Variable) is 0) or a script is running, `SAVEAS` asks at the command line instead:
1. `Enter file format [R14/2000/2004/2007/2010/2013/2018/DXF]`: press Enter to keep the current format.
2. `Save drawing as`: type the path, or press Enter to keep the name shown.

You can also type it in one line, for example `SAVEAS 2013 C:\drawings\plan.dwg` or `SAVEAS DXF plan`. This suits scripts.

## Notes
- The dialog starts with the format and version of the file you opened. A new drawing starts with the default format from Options.
- The dialog lists DWG and DXF versions from R14 to 2018, DWT 2018, and IFCCAD (`.ifcx`).
- When the path has no extension, `.dwg` or `.dxf` is added to match the format.
- Saving is refused in a read-only session and in a Test Block window.
