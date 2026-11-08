---
summary: Lists, adds or removes extended data (XDATA) on the selected objects.
related: LIST, PROPERTIES
---

## Usage
1. Select objects, or run the command and select them at the prompt.
2. At `XDATA  [List / Clear]  (SET <app> <value> by typing):`, choose **List** or **Clear**.

To add data, select the objects first and enter `XDATA SET <app> <value>` as one line (start the line with `>` so that Space does not end the entry, or use a script).

## Options
| Option | What it does |
|---|---|
| **List** | Prints every extended data record of each selected object: its application name, how many values it holds, and the values. |
| **Clear** | Removes all extended data from the selected objects. |
| `SET <app> <value>` | Adds a record for application `<app>` that holds the text `<value>`. Without an application name, `OpenCADStudio` is used. |
| `CLEAR <app>` | Removes only the records of application `<app>`. |

## Notes
- The one-line forms (`XDATA LIST`, `XDATA SET ...`, `XDATA CLEAR ...`) work on the current selection; with nothing selected you see `XDATA: select entities first.`
- Objects on locked layers are not changed.
- **SET** and **Clear** can be undone.
