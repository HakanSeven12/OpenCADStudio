# Workspaces and Toolbars

## Workspaces

`WSCURRENT` switches between the two workspaces:

| Workspace | What you get |
|---|---|
| **Ribbon** | The ribbon with its tabs and panels. The default. |
| **Classic** | No ribbon; the menu bar and floating toolbars instead. The first time, the Draw, Modify, Layers and Properties toolbars are shown. |

Type `WSCURRENT` alone to see the current workspace. To switch, give the choice on the same line: `>WSCURRENT C` for Classic, `>WSCURRENT R` for Ribbon (the leading `>` keeps the space in the line; see [Command Modifiers](Command-Modifiers#typing-a-whole-line-with-spaces)).

## Toolbars

Every ribbon panel can also be shown as a floating toolbar.

| Command | What it does |
|---|---|
| `TOOLBAR` | Lists every toolbar by tab and panel name; a dot marks the ones shown. |
| `>TOOLBAR <name>` | Shows or hides that toolbar. |
| `>TOOLBAR <name> Show` / `Hide` | Shows or hides it explicitly. |

Drag a toolbar by its handle to move it.

## The menu bar

`MENUBAR` set to 1 shows the classic menu bar (File, Edit, View, Insert, Format, Tools, Draw, Dimension, Modify, Parametric, Window, Help) above the ribbon. It is always shown in the Classic workspace.

## Showing and hiding parts of the window

| Part | Show / hide |
|---|---|
| Ribbon | `RIBBON` / `RIBBONCLOSE` |
| Command line | `COMMANDLINE` / `COMMANDLINEHIDE`, **Ctrl+9** toggles |
| Everything except the drawing | `CLEANSCREEN`, **Ctrl+0** |
| Drawing tabs | `FILETAB` |
| Layout tabs | `LAYOUTTAB` |
| Status bar buttons | The menu at the right end of the status bar |
| Palettes | Their commands, or the title bar menu of each palette |

## The Quick Access Toolbar

The small toolbar beside the application menu holds the commands you use most, the current layer and window switching. The arrow at its end adds and removes commands.

## Your own ribbon panels

`CUI` lets you build your own panels, shown on a **My Tools** ribbon tab; see [Customization](Customization#the-ribbon-and-menus).
