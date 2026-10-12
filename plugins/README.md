# Plugin registry

`registry.json` is the curated list of third-party plugins Open CAD Studio
offers in its **Plugin Manager → marketplace**. The app fetches it from this
repo's `main` branch at runtime, so adding an entry makes a plugin discoverable
to every user without an app update.

## Add your plugin

Open a pull request adding one object to the array in
[`registry.json`](registry.json):

```json
{
  "repo": "your-account/your-plugin-repo",
  "name": "Human-readable name",
  "description": "One line describing what it does."
}
```

Requirements for the linked repo:

- Builds a `cdylib` against [`ocs_plugin_api`](../crates/ocs_plugin_api) and
  exports the host symbols via `ocs_plugin_api::export_plugin!`.
- Publishes per-platform binaries plus `plugin.toml` as **GitHub Release**
  assets (see [opencad-example-plugin](https://github.com/HakanSeven12/opencad-example-plugin)
  and its release workflow for a template). Asset names carry the platform, e.g.
  `your.plugin-linux-x86_64.so`, `…-windows-x86_64.dll`, `…-macos-aarch64.dylib`.
- `plugin.toml` declares an `api_version` compatible with the host (3 or
  newer). From API 4 on it must also carry the `rustc_version` and
  `acadrust_source` the library was built with; the host loads it only when
  both match its own build.

The host reads the release matching the user's platform, checks the API
version, and installs it into the user's plugins folder.

> Listing is curation, not endorsement or a security review. Installing a
> plugin runs its native code; users install at their own risk.
