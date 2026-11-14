# Module layers

The crate's top-level modules are arranged in layers. Code may use modules in its own layer or in lower layers. A reference from a lower layer to a higher one is an *upward dependency*: it ties code that many things rely on to code that changes often, and makes both harder to change or test on their own.

`scripts/arch_check.py` measures upward dependencies. CI runs it on every pull request and reports what the pull request adds or removes. **It only reports. It does not block merging.**

## The layers

| Layer | Modules | What lives here |
| --- | --- | --- |
| 0 foundations | `par`, `perf`, `sys`, `sysinfo`, `i18n`, `config`, `network`, `schema_validation` | Helpers with no knowledge of CAD or of the app |
| 1 drawing model | `entities`, `snap`, `parametric`, `entity_creation`, `filter` | Entities and their geometry behaviour |
| 2 files and rendering | `io`, `scene`, `gpu_backend` | Reading and writing files; turning the model into pictures |
| 3 tool contract | `command`, `plugin` | What a command or plugin is, and how it runs |
| 4 tools | `modules` | The commands and ribbon tabs themselves |
| 5 presentation | `ui`, `input` | Windows, widgets, input handling |
| 6 app shell | `app`, `mcp`, `mcp_intent`, `mcp_ops`, `rest`, `cli`, `discussions`, `patreon`, `videos`, `locale_catalog` | The application and its front doors |

The rule of thumb: code that many things depend on sits low and changes rarely; code that changes often sits high, and nothing depends on it.

## Exceptions and choices

- **`ui` may use `app::Message`.** iced views return the application's message type, so every view names it. Anything else `ui` takes from `app` still counts.
- **`scene` must not use `io`**, although both are layer 2. Rendering should not depend on file formats; the two would otherwise grow into a cycle.
- **`network`, `schema_validation` and `gpu_backend` sit low** because they depend on almost nothing: `network` is a shared HTTP agent, `schema_validation` a small validator, and `gpu_backend` depends only on `sys` and `config`.
- **`sys` stays a foundation module** even though it holds platform glue for the web build. Its calls into `io` and `scene` are reported as upward dependencies until they are restructured.

## What counts

- A file that refers to a higher module counts once for that pair, however many references it has.
- Test code is skipped: files named `tests.rs`, `test_support.rs` or `*_tests.rs`, files under a `tests/` or `*_tests/` folder, `#[cfg(test)] mod` blocks, and files declared with `#[cfg(test)] mod name;`.
- Comments and the contents of strings are ignored.
- Only `crate::` paths are read. A `super::` path that climbs out of a top-level module is not seen.

## Running it

```sh
python3 scripts/arch_check.py report            # counts for this checkout
python3 scripts/arch_check.py diff ../other     # added and removed, compared with another checkout
python3 scripts/test_arch_check.py              # the tool's own tests
```

Add `--markdown` to either command for a table or list ready to paste.

## When a pull request adds one

The report is information, not a verdict. Some ways to resolve an upward dependency, roughly in order of preference:

1. **Move the item down.** If a lower module needs a type or function that does not depend on anything high, move it to the lower module and re-export it from its old home, so existing callers do not change.
2. **Pass it in.** If lower code needs a value the app owns (a setting, a callback), let the caller supply it instead of reaching up for it.
3. **Change the layer.** If a module is in the wrong layer, change `LAYERS` in `scripts/arch_check.py` in the same pull request and say why in the description.
4. **Leave it.** Sometimes the dependency is the least bad option for now. Say so in the description, so the choice is visible.

New top-level modules need a layer. The report lists any module that has none.
