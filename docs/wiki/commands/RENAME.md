---
summary: Renames layers, blocks, text styles, dimension styles, linetypes, UCSs and views.
related: LAYER, BLOCK, DIMSTYLE, PURGE
---

## Usage
1. Run the command. The **Rename** dialog opens.
2. Under **Named Objects**, choose the kind of name.
3. Under **Items**, click the name to change. It appears in **Old Name**.
4. Type the new name in **Rename To** and click **Rename To**, or click **OK** to rename and close.

## Command-line form
`-RENAME` asks at the command line, which suits scripts:
1. `Select the object type to rename:` type **Layer**, **Block**, **Text style** (`STYLE`), **Dim style** (`DIMSTYLE`), **Linetype**, **UCS** or **View**.
2. Enter the current name.
3. Enter the new name.

You can also give everything on one line: `RENAME <type> <old> <new>`, for example `RENAME LAYER Old New`.

## Notes
- The dialog does not list names that cannot be renamed: layer `0` and `Defpoints`, `ByLayer`, `ByBlock` and `Continuous`, layout and anonymous blocks, and names from external references.
- If the rename is refused, the dialog stays open.
- The rename can be undone.
