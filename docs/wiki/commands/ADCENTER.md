---
summary: Browses the blocks, layers, linetypes and styles of an open or saved drawing and adds the picked ones to the current drawing.
related: CONTENTBROWSER, LAYER, BLOCKSPALETTE
ribbon: Insert tab › Content panel
---

## Usage
1. Start the command. The Design Center window opens.
2. Under **Drawings**, pick one of the open drawings, or click **Browse...** to read a DWG or DXF file from disk.
3. Under **Named Objects**, pick Blocks, Dimension styles, Layers, Linetypes, Multileader styles or Text styles.
4. Click items to pick them (click again to unpick), or click **Select All**.
5. Click **Add to Drawing**.

## Notes
- What an item needs comes with it: a layer's linetype, a dimension style's text style and arrow blocks, the blocks nested in a block.
- An item whose name the current drawing already has is skipped; its definition there is kept. The command line reports how many items were added and skipped.
- Drawings picked with Browse... stay listed the next time the window opens.
- One Undo removes everything an Add brought in.
