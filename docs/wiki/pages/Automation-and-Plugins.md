# Automation and Plugins

Repetitive work can be handed to the program: command scripts for sequences of commands, command-line conversion for batches of files, an automation API for other programs and AI assistants, and plugins that add new commands and panels.

## Command scripts

A script (`.scr`) is a plain text file of what you would type at the command line. Each line is one command line: a command, optionally followed by its answers separated by spaces. An empty line presses Enter. Lines starting with `;` or `#` are comments.

```
; draw a frame and save
LINE 0,0 5000,0 5000,4000 0,4000 C
ZOOM E
QSAVE
```

Run it with `SCRIPT` (`SCR`), or from the command line:

| Command line | What it does |
|---|---|
| `OpenCADStudio --script setup.scr drawing.dwg` | Opens the drawing and runs the script in the editor. |
| `OpenCADStudio --batch job.scr` | Runs the script with no window and exits; the exit code is 1 if a line failed. |
| `OpenCADStudio --each job.scr a.dwg b.dwg c.dwg` | Runs the script on each drawing in turn, with no window. What the script does not save is not saved. |

Use the `-` versions of commands (`-LAYER`, `-INSERT`, `-PLOT`) in scripts: they ask at the command line instead of opening a dialog.

## Command-line conversion

The desktop program converts drawings without showing a window:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --export input-r12.dxf output.dwg --target-version R14
```

The output type follows the file extension; `--target-version` sets the DWG or DXF version.

## The automation API

The same editing core can be driven by other programs:

| Start with | What you get |
|---|---|
| `OpenCADStudio --serve` | A headless automation server that takes one JSON request per line, over standard input and output or a local TCP port (`--port 4242`). |
| `OpenCADStudio --http <port>` | The same API as a REST server on `http://127.0.0.1:<port>`, for ordinary HTTP clients. |
| `OpenCADStudio --mcp` | A Model Context Protocol server for AI assistants, connected to the live editor. |

Through these you can open and save drawings, read every object and property, create and change objects as one undoable step, run commands, measure, select, and take pictures of the drawing. `audit` and `save_verified` check that a save kept everything before you rely on it.

The full reference is in the repository: [automation guide](https://github.com/HakanSeven12/OpenCADStudio/blob/main/docs/automation/README.md) and [API specification](https://github.com/HakanSeven12/OpenCADStudio/blob/main/docs/automation/API-SPEC.md).

## Plugins

Plugins add commands, ribbon tabs, palettes, grips, object types and file formats. Each plugin runs in its own process, so a faulty plugin cannot take the program down with it. Plugins are available in the desktop program, not in the web app.

<img src="images/icons/ui/gear.svg" width="20" alt=""> `PLUGINMANAGER` (or `PLUGINS`) opens the Plugin Manager:

- **Installed** lists your plugins; **Enable**, **Disable** or **Uninstall** them.
- **Available plugins** lists the plugins published by the community. **Install** downloads the build for your system and installs it.

The marketplace list is curated in [`plugins/registry.json`](https://github.com/HakanSeven12/OpenCADStudio/blob/main/plugins/registry.json). Listing a plugin there is not a security review: a plugin runs its own native code, so install only plugins you trust.

### Python

The bundled Python plugin runs Python code against the open drawing through an `ocs` module: read and change objects, layers and blocks, all through the same checked interface as other plugins. `PY_EVAL` runs a line of Python and `PY_RUN` a script file.

`SCRIPTCOMMANDS` controls whether scripts may run program commands.

### Writing a plugin

Plugins are written in Rust against the versioned plugin API (`ocs_plugin_api`). Start from the [plugin template](https://github.com/HakanSeven12/OpenCADStudio/tree/main/docs/plugin-template) and the [example plugin](https://github.com/HakanSeven12/opencad-example-plugin), and read the [plugin architecture](https://github.com/HakanSeven12/OpenCADStudio/blob/main/docs/plugin-architecture.md). To list your plugin in the marketplace, open a pull request that adds it to `plugins/registry.json`.

## Node graph

<img src="images/icons/ui/node_graph.svg" width="20" alt=""> The node graph builds geometry and edits from connected nodes instead of commands: nodes for points, curves, transformations and drawing objects, wired together, with the result updating as inputs change. Open it from its button next to the drawing.
