# Web App and Desktop Program

Open CAD Studio runs as a desktop program (Windows, Linux, macOS) and as a web app at [opencadstudio.com](https://www.opencadstudio.com). Both are built from the same code: the drawing tools, the 2D and 3D editing, DWG and DXF reading and writing are the same. The differences come from what a browser allows.

| Area | Desktop | Web app |
|---|---|---|
| Installing | Installer or portable package | Nothing to install |
| Opening drawings | File dialog, drag and drop, double-click in the file manager, recent files | The browser's file picker or drag and drop |
| Saving | Saves to the file on disk; backups and autosave next to it | Saves as a download |
| Windows | Every dialog and palette can be a window of its own | One page; dialogs and palettes float inside it |
| Fonts | Stroke fonts, your system's TrueType fonts and font folders | Stroke fonts plus web fonts loaded on demand; a drawing's desktop font names are mapped to similar web fonts |
| 3D modelling | Yes | Yes, the same |
| Hatches, images, PDF and point-cloud underlays | Yes | Yes |
| Printing | System printers and PDF | PDF |
| Plugins | Yes, through the Plugin Manager | No |
| Command scripts, command-line conversion, automation API | Yes | No |
| File associations and thumbnails | Yes (installer) | No |
| Update check | At start-up | Always the latest version |
| Graphics | Vulkan, DirectX 12, Metal or OpenGL | WebGL2 |

Big drawings open faster on the desktop, which uses all processor cores; the web app parses drawings in a background worker.

Settings you change in the web app are kept by the browser for that site.
