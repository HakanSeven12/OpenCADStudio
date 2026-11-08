---
summary: Creates a named group from the selected objects, or lists, renames and edits groups.
related: UNGROUP
ribbon: Draw tab › Groups panel
---

## Usage
1. Select objects, then press Enter. If objects are already selected, this step is skipped.
2. Enter group name: type a name, or press Enter to use the suggested name shown in brackets.

The command line confirms that the group was created.

## Command-line form
Type `GROUP` followed by a keyword to manage existing groups. These forms suit scripts.

| Form | What it does |
|---|---|
| `GROUP LIST` (or `GROUP ?`) | Lists every group with its number of objects. |
| `GROUP RENAME <old name> <new name>` | Renames a group. The new name must not already be used. |
| `GROUP ADD <name> [handle ...]` | Adds objects to a group, by handle, or the selected objects if no handles are given. |
| `GROUP REMOVE <name> [handle ...]` | Removes objects from a group, by handle, or the selected objects if no handles are given. |

## Notes
- Objects on locked layers are left out of a new group.
- [`PICKSTYLE`](PICKSTYLE-System-Variable) controls whether picking one member selects the whole group, and [`GROUPDISPLAYMODE`](GROUPDISPLAYMODE-System-Variable) controls how a picked group is shown.
