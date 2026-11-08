---
summary: Opens the Find and Replace dialog to search for text in the drawing and replace it.
related: DDEDIT, TEXTEDIT
---

## Usage
1. Type `FIND`. The dialog opens.
2. Type the text in **Find:**.
3. Click **Find Next** to zoom to the next match and highlight it, or **Find All** to list every match.
4. To change text, type the new text in **Replace with:** and click **Replace** (current match) or **Replace All**.
5. Click **Close** when done.

## Notes
- The search is not case-sensitive.
- It looks in single-line text, multiline text, attribute definitions, and the attribute values of block references. Text inside blocks is found too.
- Matches in the current space come first.
- The result list shows each match's type, layer and text. Click a row to zoom to it.
- Text on locked layers is not replaced.
- Each replace can be undone with one undo.
