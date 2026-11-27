# Where the structure is going

The root package holds almost all of the code (about 1.16 million lines of Rust, 93 % of the workspace). It builds as one crate, so every change recompiles all of it, all tests link into one binary, and the central files are where parallel pull requests collide. The goal is a set of crates that follow the layers in [`RULES.md`](RULES.md), each one buildable and testable without the ones above it.

This page is the plan. [`RULES.md`](RULES.md) is the rule that holds today; CI enforces it (the `architecture` job).

## The crates

| Crate | Layer | What it holds |
| --- | --- | --- |
| `ocs_base` | 0 | `par`, `perf`, `i18n`, `config`, `network`, `wildcard`, `xml_depth`, `schema_validation` |
| `ocs_geom`, `ocs_solid`, `ocs_constraints` | 0–1 | as today |
| `ocs_model` | 1 | `entities`, `parametric`, `entity_creation`, `filter`, `calc`: the drawing model and its behaviour, with no GUI or GPU types |
| `ocs_io` | 2 | file formats; depends on the model, not on rendering |
| `ocs_session` | 2 | one open drawing being edited: the document, selection, undo, visibility and isolation, and the document operations (copy and paste, block definitions) that `Scene` and `app` hold today |
| `ocs_render` | 2 | tessellation, the render caches and the GPU pipeline; reads a session, never edits it |
| `ocs_commands` | 3–4 | the command contract and the commands, working on a session through an edit context rather than on the app, so the GUI, MCP, REST and batch runs share one path |
| `ocs_ui` | 5 | widgets, ribbon, windows |
| `opencadstudio` | 6 | the application shell, the plugin host and the front doors (`mcp`, `rest`, `cli`) |

`CRATES` in `scripts/arch_check.py` holds this table as data. A module the table does not name is in the crate of its layer. Layer 2 has three crates: `io` and `ifc_plan` are in `ocs_io`, `scene`, `gpu_backend` and `snap` in `ocs_render`, and `catalog` is in none yet. `python3 scripts/arch_check.py readiness` lists what keeps each crate from building alone, and the [Code Health](https://github.com/HakanSeven12/OpenCADStudio/wiki/Code-Health) page shows it for each release.

## The order

Each step builds, passes the tests and lands on `main` on its own. A step that moves code changes no behaviour, keeps history (`git mv`) and re-exports the moved items from their old path so callers need not change in the same commit.

1. **No new upward dependency.** Done: the `architecture` job fails a change that adds one, or that adds a module without a layer.
2. **Remove the upward dependencies**, smallest pairs first; `entities -> scene` is the one that gates the first crate split. The types the model needs from `scene` (properties and grips in `scene::model::object`, transforms in `scene::view::transform`, the wire model, text shaping, the tessellation helpers) move down into the model layer.
3. **Process-wide state into the session.** About 180 mutable statics (atomics, mutexes, `OnceLock`) and 66 `thread_local!` blocks hold per-session state today, such as a command's last-used values. A switch that is process-wide forces tests that touch it to run serially. They move into the session or the app state they belong to.
4. **System variables from one table**: name, type, default, where the value is kept (drawing header or user profile), how it is read and set. `SETVAR`, the name lists, the settings file and the command reference are then derived from it instead of being edited in several files.
5. **Split the crates bottom up**: `ocs_base`, `ocs_model`, `ocs_io`.
6. **Split `Scene`** into session, view state (camera, panes), render caches and previews; then `ocs_session` and `ocs_render`.
7. **The command layer:**
   - a registry in place of the name match;
   - a small `CadCommand` core with optional capability traits in place of one trait with 168 methods;
   - `CmdResult` split into document edits and requests to the GUI.
8. **Split `app` and `Message` by feature**: each feature owns its state, its messages (`Message::Layers(layers::Msg)`) and its view. Then `ocs_ui`.

Large files are split by the step that touches them, not in a separate pass.

## For contributors

- Small upward-dependency removals (one pair, one file) are welcome as pull requests. Say in the description which pair it removes.
- Moving whole folders or splitting crates is done in a quiet window and announced first, because it touches every open pull request.
- The file formats (DWG/DXF structure, the codec) and the plugin API's wire format are not changed by any of these steps.
