# Customization

## Options

<img src="images/icons/options_tool.svg" width="24" alt=""> `OPTIONS` (`OP`, or **Cmd+,** on macOS) opens the program settings:

![Options](images/screens/options.png)


| Tab | What you set there |
|---|---|
| **General** | Interface language; shortcuts to plugins, keyboard shortcuts, command aliases and plot settings; the default save format. |
| **Files** | Where the program keeps its files: configuration, plot styles, plugins, autosave files, fonts, colour books, the support files folder and the default template for new drawings. |
| **Open and Save** | File associations for DWG and DXF, backups and automatic save. |
| **Display** | Colour theme, drawing area colours, cursor, grip colours. |
| **Drafting** | Drafting rotation, polar tracking increment, snap marker size, and the Drafting Settings. |
| **Modeling** | 3D display tools: the ViewCube and the UCS icon. |
| **Selection** | Pickbox and aperture size, selection modes (Shift to add, the picker for overlapping objects), visual effects. |
| **User Preferences** | Navigation style, mouse wheel and touchpad zoom, text and dimension behaviour, the 3D mouse. |
| **Graphics** | The graphics backend and display quality. |

Settings are stored in the settings folder; see [Installation](Installation#where-your-settings-live).

## Command aliases

Aliases are the short names you can type instead of a command: `L` for `LINE`, `CO` for `COPY`. They are kept in a plain text file, `ocad.pgp`, in your settings folder. Each line is one alias:

```
L,      *LINE
CO,     *COPY
WALL,   *MLINE
```

<img src="images/icons/edit_aliases.svg" width="24" alt=""> `ALIASEDIT` edits the aliases in a table: add, change, remove, and **Import PGP…** to bring in an alias file from another CAD program. Imported aliases that point to commands that exist here replace the matching ones; the rest are kept. **Apply** saves and activates the changes.

## Keyboard shortcuts

`CUI` › **Keyboard Shortcuts** assigns key combinations to commands: pick a command, press the keys, and save. The default shortcuts are listed in [Keyboard Shortcuts](Keyboard-Shortcuts).

## The ribbon and menus

<img src="images/icons/user_interface.svg" width="24" alt=""> `CUI` opens **Customize User Interface**, where you build **panels** of your own, shown on a **My Tools** ribbon tab. Give a panel a title and add commands or aliases to it; changes show at once. Its **Keyboard Shortcuts…** button opens the shortcut editor.

Elsewhere:

- **Quick Access Toolbar**: the arrow at its end adds or removes commands.
- **Right-click menus**: **Customize…** at the bottom of a right-click menu hides rows you do not use and adds your own commands under **My commands**.

<img src="images/icons/cui_export.svg" width="24" alt=""> `CUIEXPORT` saves your keyboard shortcuts to a file and <img src="images/icons/cui_import.svg" width="24" alt=""> `CUIIMPORT` loads them, for example on another computer.

## Support files

The folder set under **Options › Files › Support files** is searched for:

- hatch patterns (`.pat`) — see [Hatches and Fills](Hatches-and-Fills#your-own-patterns);
- linetypes (`.lin`) — see [Layers and Object Properties](Layers-and-Properties#linetypes);
- fonts and shape files (`.shx`, `.ttf`).

Files there are used next to the built-in ones. New patterns and linetypes load the next time the program starts.

Your settings folder also has `patterns` and `linetypes` folders that work the same way.

## The status bar

The menu at the right end of the status bar chooses which buttons it shows. Right-click a button for its settings.

## System variables

Many settings are system variables you can type at the command line: `PICKBOX 5`, `ZOOMFACTOR 60`, `OSMODE 4133`. Some are stored in the drawing, others in your settings. See [System Variables](System-Variables).

## Colour books

Colour books add named colours to the colour picker. One colour book is included; put more `.acb` files in the colour book folder (**Options › Files**).
