# Homebrew cask

[`open-cad-studio.rb`](open-cad-studio.rb) is a [Homebrew Cask](https://docs.brew.sh/Cask-Cookbook)
for installing the macOS (Apple Silicon) build.

## Install through a local tap

Current Homebrew installs casks only from a tap, not from a URL or a file
path, and it no longer has `--no-quarantine`. Until a public tap exists, put
the cask in a local one:

```bash
brew tap-new --no-git local/tap
mkdir -p "$(brew --repository local/tap)/Casks"
curl -fsSL -o "$(brew --repository local/tap)/Casks/open-cad-studio.rb" \
  https://raw.githubusercontent.com/HakanSeven12/OpenCADStudio/main/packaging/homebrew/open-cad-studio.rb
brew install --cask local/tap/open-cad-studio
```

The app is ad-hoc signed but **not** Apple-notarised (notarisation needs a paid
Apple Developer ID), so macOS blocks the first launch once: approve it in
System Settings > Privacy & Security > Open Anyway.

## Publishing as a proper tap

Create a separate GitHub repo named `homebrew-tap` under the same account, put a
copy of `open-cad-studio.rb` in its `Casks/` directory, then users can run:

```bash
brew install --cask hakanseven12/tap/open-cad-studio
```

`brew upgrade` then keeps the app current automatically.

## Updating for a new release

Each release run prints the new `version` + `sha256` in the GitHub Actions job
summary (the **Emit Homebrew cask sha256** step), once for Apple Silicon and once
for Intel. Paste the version and the two digests (`sha256 arm:` / `intel:`) into the
cask. To compute the digests manually:

```bash
shasum -a 256 OpenCADStudio-vX.Y.Z-macos-arm64.dmg OpenCADStudio-vX.Y.Z-macos-x86_64.dmg
```
