---
summary: Opens the Filter dialog to select objects that match a set of conditions, combined with all/any logic and groups.
related: QSELECT, SELECT, FI
web: Exporting and importing saved filters to a file is not available in the web app.
---

## Usage
1. Type `FILTER`. The Filter dialog opens.
2. Choose whether objects must match **all of these** or **any of these** conditions.
3. Click **Condition** to add a row. Pick an object type, a property, an operator and a value. Tick **NOT** to turn a row around.
4. Click **Group** to add a group of rows with its own all/any setting.
5. Under **Search in**, choose the current space, the current **Selection**, or click **Window…** to pick the objects to search.
6. Choose **New selection**, **Add** or **Remove** for how the result changes the selection.
7. Click the select button. The matching objects are selected.

## In the dialog
- **Reads:** shows the filter written out as a sentence.
- **Live result** counts the matches by type and by layer while you edit.
- **From object** hides the dialog so you can pick an object; its properties become rows you can tick and add.
- **Select the objects that do not match** inverts the result.
- **Save**, **Save As** and **Delete** keep named filters in the drawing. **Export** and **Import** write them to a file or read them from one.

## Notes
- You can run it transparently as `'FILTER` while another command asks you to select objects. The result is handed back to that command.
- Quick Select's **Advanced…** button opens this dialog with its conditions already filled in.
