# Module layers

The crate's top-level modules are arranged in layers. Code may use modules in its own layer or in lower layers. A reference from a lower layer to a higher one is an *upward dependency*: it ties code that many things rely on to code that changes often, and makes both harder to change or test on their own.

Where the layers are going (crates, and the order of the work) is in [`TARGET.md`](TARGET.md).

`scripts/arch_check.py` measures upward dependencies. CI runs it on every pull request and every push to main and reports what the change adds or removes. **A change that adds an upward dependency fails the `architecture` job.** The ones that exist today are not counted against a change; they are removed over time, and a change that removes one says so in the report.

## The layers

| Layer | Modules | What lives here |
| --- | --- | --- |
| 0 foundations | `par`, `perf`, `sys`, `sysinfo`, `i18n`, `config`, `network`, `schema_validation`, `xml_depth`, `wildcard`, `paths`, `icon_assets` | Helpers with no knowledge of CAD or of the app |
| 1 drawing model | `entities`, `display`, `text`, `parametric`, `filter`, `calc`, `catalog`, `media` | Entities and their geometry behaviour |
| 2 files and rendering | `io`, `scene`, `snap`, `gpu_backend`, `ifc_plan` | Reading and writing files; turning the model into pictures |
| 3 tool contract | `command`, `plugin_catalog` | What a command or an add-on package is |
| 4 tools | `modules`, `entity_creation` | The commands and ribbon tabs themselves |
| 5 presentation | `ui`, `input` | Windows, widgets, input handling |
| 6 app shell | `app`, `plugin`, `mcp`, `mcp_intent`, `mcp_ops`, `rest`, `cli`, `discussions`, `patreon`, `videos`, `locale_catalog` | The application and its front doors |

The rule of thumb: code that many things depend on sits low and changes rarely; code that changes often sits high, and nothing depends on it.

## Exceptions and choices

- **`ui` may use `app::Message`.** iced views return the application's message type, so every view names it. Anything else `ui` takes from `app` still counts.
- **`scene` must not use `io`**, although both are layer 2. Rendering should not depend on file formats; the two would otherwise grow into a cycle. The libraries both of them read (linetypes, patterns, plot styles, paper sizes, visual styles, coordinate systems) are in `catalog` (layer 1), which both of them use.
- **`network`, `schema_validation`, `xml_depth` and `gpu_backend` sit low** because they depend on almost nothing: `network` is a shared HTTP agent, `schema_validation` a small validator, `xml_depth` the nesting check the XML readers in `io`, `scene` and `ui` run before parsing, and `gpu_backend` depends only on `sys` and `config`.
- **`plugin` is part of the app shell.** The plugin host drives the application (`app::plugin_host`) and merges plugin tabs with the core ones, so it sits with `app`; what a plugin may call is `ocs_plugin_api`.
- **`entity_creation` sits with the tools.** It builds entities from the creation contracts MCP and the automation API send, with the commands' own builders; only the front doors use it.
- **`snap` sits with `scene`.** Object snap runs on the wires the scene tessellates and on screen positions; nothing in the drawing model uses it.
- **`sys` stays a foundation module** even though it holds platform glue for the web build. Its calls into `io` and `scene` are reported as upward dependencies until they are restructured.

## What counts

- A file that refers to a higher module counts once for that pair, however many references it has.
- Test code is skipped: files named `tests.rs`, `test_support.rs` or `*_tests.rs`, files under a `tests/` or `*_tests/` folder, `#[cfg(test)] mod` blocks, and files declared with `#[cfg(test)] mod name;`.
- Comments and the contents of strings are ignored.
- Only `crate::` paths are read. A `super::` path that climbs out of a top-level module is not seen.
- `readiness` (below) reads more: items in the crate root (`src/lib.rs`), `super::` paths that climb out of a top-level module, and the macros `src/` exports. It counts each use that keeps a planned crate from building alone, test code apart, and makes no exceptions such as `app::Message`. It never fails a change.

## Running it

```sh
python3 scripts/arch_check.py report            # counts for this checkout
python3 scripts/arch_check.py diff ../other     # added and removed, compared with another checkout
python3 scripts/arch_check.py readiness         # what blocks each crate planned in TARGET.md
python3 scripts/test_arch_check.py              # the tool's own tests
```

Add `--markdown` to any command for a table or list ready to paste.

## When a pull request adds one

The job fails and lists each new reference with its file and line. Some ways to resolve it, roughly in order of preference:

1. **Move the item down.** If a lower module needs a type or function that does not depend on anything high, move it to the lower module and re-export it from its old home, so existing callers do not change.
2. **Pass it in.** If lower code needs a value the app owns (a setting, a callback), let the caller supply it instead of reaching up for it.
3. **Change the layer.** If a module is in the wrong layer, change `LAYERS` in `scripts/arch_check.py` in the same pull request and say why in the description.
4. **Allow it.** If the dependency is by design, add the items to `ALLOWED` in `scripts/arch_check.py` in the same pull request and say why in the description.

New top-level modules need a layer. The report lists any module that has none, and the `architecture` job fails until it has one.
