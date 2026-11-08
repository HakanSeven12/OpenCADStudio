---
summary: Finds a layer by typing part of its name, then makes it current or moves the selected objects to it.
related: LAYER, LAYCUR, LAYMCUR
---

## Usage
1. Optionally select objects first.
2. Start typing a layer name. A list of up to eight matching layers appears, with each layer's color and on, freeze and lock state.
3. Pick a layer from the list, or type its full name, and press Enter.

With no selection, the layer becomes the current layer. With a selection, the selected objects move to that layer.

## Notes
- Matching ignores case. Exact names come first, then names that start with the text, then names with a word that starts with it, then names that contain it. Letters typed in order also match, so `cte` finds `Corte`.
- Layer names can contain spaces.
- If no layer has the name you enter, the command reports it and nothing changes.
- Moving objects to a layer can be undone.
