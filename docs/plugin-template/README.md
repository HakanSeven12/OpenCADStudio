# Open CAD Studio plugin template

A complete scaffold for an **external** Open CAD Studio add-on. A plugin is its
own repository that builds a `cdylib`; the host loads it at runtime. Copy this
folder into a new repo and rename the placeholders.

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | `cdylib` crate depending on `ocs_plugin_api` (`host` feature) |
| `src/lib.rs` | manifest + `CadModule` ribbon + `BuiltinPlugin` + `export_plugin!` |
| `plugin.toml` | metadata read by the host (mirrors the manifest) |
| `.github/workflows/release.yml` | cross-builds the cdylib and publishes a release |
| `PLUGIN.md` | your command reference + XDATA schemas |

## Quick start

1. Copy this folder into a new repository.
2. Rename `my-plugin` / `My Plugin` / `opencad.my_plugin` / `my_plugin` / `MP_`
   throughout (`Cargo.toml`, `src/lib.rs`, `plugin.toml`, the workflow `asset:`
   names), keeping `plugin.toml` and the `MANIFEST` in sync.
3. `cargo build` to check it compiles.

## Test locally

```sh
cargo build --release
mkdir -p "<config>/OpenCADStudio/plugins/opencad.my_plugin"
cp target/release/*my_plugin*.so "<config>/OpenCADStudio/plugins/opencad.my_plugin/"
sed "s|__RUSTC_VERSION__|$(rustc --version)|" plugin.toml > \
  "<config>/OpenCADStudio/plugins/opencad.my_plugin/plugin.toml"
```

Restart Open CAD Studio: the ribbon tab appears and `MP_` commands route to your
plugin. (`<config>` = `%APPDATA%` / `~/Library/Application Support` /
`$XDG_CONFIG_HOME`.)

## Publish

Push a `v*` tag — the workflow builds the cdylib on Linux/Windows/macOS and
uploads each binary plus `plugin.toml` to a GitHub Release. Users install it from
the **Plugin Manager** by linking your `owner/repo`, or — once your repo is added
to [`plugins/registry.json`](https://github.com/HakanSeven12/OpenCADStudio/blob/main/plugins/registry.json)
via PR — straight from *Available plugins*.

> The binary must be built with the same toolchain and `ocs_plugin_api` version
> as the host (approach B). See `docs/plugin-architecture.md`.
