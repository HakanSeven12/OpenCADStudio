---
summary: Combines selected single-line and multiline text into one multiline text object, or turns each text into its own multiline text.
related: MTEXT, TEXT, TXTEXP, EXPLODE
ribbon: Insert tab › Import panel (Combine Text)
---
## Usage
1. At `Select objects or [SEttings]:`, select the text objects and press Enter. If you selected text before you started, the command runs at once.
2. The command line reports how many text objects were removed and how many multiline text objects were added.

## Options
| Option | What it does |
|---|---|
| **SEttings** (`SE`) | Opens the Combine Text — Settings dialog. |

## Settings
| Setting | What it does |
|---|---|
| **Combine into a single MText object** | On (default): everything becomes one multiline text. Off: each text, and each line of a multiline text, becomes its own multiline text. |
| **Line order** | **Top-down** (default) sorts the lines by position; **Selection order** keeps the order in which you selected them. Inactive when combining is off. |
| **Word-wrap** | On (default): the lines join with spaces in a box as wide as the selected text. Off: each object starts a new paragraph. |
| **Force uniform line spacing** | Uses one line spacing for all lines. Always on while word-wrap is on. |

## Notes
- The new text sits at the top-left corner of the first object and takes its layer, text style, height and rotation.
- Objects on locked layers are left out.
- The settings are saved with your preferences.
- You can undo the change.
