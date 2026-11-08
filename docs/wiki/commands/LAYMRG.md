---
summary: Moves every object on the layers you choose onto a target layer, then deletes the emptied layers.
related: LAYTRANS, LAYDEL, LAYMATCH, LAYERS, PURGE
---

## Usage
1. At `Select object on layer to merge or [Name]:`, pick objects on the layers to merge, or type `N` and enter a layer name. If objects are already selected, their layers are used and this step is skipped.
2. Press Enter when the list of selected layers is complete.
3. At `Select object on target layer or [Name]:`, pick an object on the target layer, or type `N` and enter its name.
4. The command line warns how many layers will be merged. At `Do you wish to continue? [Yes/No] <No>:`, type `Y` to merge.

## Options
| Option | What it does |
|---|---|
| **Name** (`N`) | Type a layer name instead of picking an object. |
| **Yes** (`Y`) | Merges the layers. |
| **No** (`N`) | Cancels without changing anything. This is the default. |

## Notes
- Layer `0` and the current layer cannot be merged away. The target cannot be one of the source layers.
- Objects inside block definitions and block attributes move to the target layer too.
- For scripts, type `LAYMRG <source layer> <target layer>`. This merges one layer at once, without the confirmation.
- The merge can be undone in one step.
