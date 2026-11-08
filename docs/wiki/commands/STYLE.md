---
summary: Opens the text style manager, where you create, edit, copy, import and delete text styles and set the current one.
related: TEXT, MTEXT, RENAME, PURGE
ribbon: Annotate tab › Text panel
---

## Usage
1. Type `STYLE`, or pick **Text Style...** from the menu, or use the Text panel's dialog launcher on the ribbon.
2. Pick a style in the list. **Used in drawing** filters the list to styles that are in use.
3. Use the list buttons to make a new style, copy, delete, import styles from another drawing, or set the selected style current.
4. Edit the selected style:
   - **Font**: font file and **Font style**; **TrueType fonts only** narrows the list; **Use SHX Big Font** with a **Big Font file**.
   - **Size**: **Annotative**, and the **Fixed height** (or **Paper text height** for annotative styles). A height of 0 means the height is asked for when the text is placed.
   - **Effects**: **Upside down**, **Backward**, **Vertical**, **Width factor**, **Oblique angle**.
5. Check the **Preview** and the list of **Texts using this style**, then click **Apply**.

## Command-line form
`-STYLE` asks for each setting at the command line, which suits scripts:
1. `Enter name of text style or [?]` (`?` lists the styles). A new name creates a new style.
2. `Specify full font name or font filename (TTF or SHX)`. A name without an extension is taken as an `.shx` font.
3. `Specify height of text`
4. `Specify width factor`
5. `Specify obliquing angle`
6. `Display text backwards? [Yes/No]`
7. `Display text upside-down? [Yes/No]`
8. `Vertically? [Yes/No]` (asked only for shape fonts)

The style then becomes the current text style.

## Notes
- Styles that come from an attached reference are read only.
- Before you apply, the manager shows how many properties and styles will change, and blocks Apply while a field has a bad value.
- In scripts, `STYLE` can also be followed by words: `STYLE LIST` (or `?`) lists the styles; `STYLE NEW <name>` creates one; `STYLE FONT <style> <font file>`, `STYLE WIDTH <style> <factor>` and `STYLE OBLIQUE <style> <degrees>` change one setting.
