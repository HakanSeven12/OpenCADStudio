---
summary: Deletes a layer and erases every object on it.
related: LAYMRG, PURGE, LAYER
---
## Usage
1. At `LAYDEL  layer to delete:`, type the layer name.
2. The objects on the layer are erased, the layer is removed, and the command line reports how many objects were erased.

You can also type the name after the command, for example `LAYDEL Old-Notes`. Layer names are matched without regard to case.

## Notes
- Layer `0` cannot be deleted.
- The current layer cannot be deleted; make another layer current first.
- A name that is not a layer of the drawing is reported and nothing changes.
- The deletion can be undone.
