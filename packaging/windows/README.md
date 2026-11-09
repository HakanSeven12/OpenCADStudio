# Windows Packaging & WinGet

This directory contains the WiX toolset configuration for building the signed Windows MSI installers, as well as documentation for the official WinGet package.

## Windows Package Manager (WinGet)

Open CAD Studio is published to the official Microsoft community repository (`microsoft/winget-pkgs`) under the package identifier:

* **Package Identifier:** `HakanSeven.OpenCADStudio`

Users can install or upgrade Open CAD Studio on Windows via Terminal or PowerShell:

```powershell
winget install HakanSeven.OpenCADStudio
```

### Version Scheme

Open CAD Studio uses calendar-based weekly tags (e.g. `v2026.45`). Because Windows Installer MSI `ProductVersion` restricts the major version component to $\le 255$, `scripts/release.py` maps the year from `2026` to `26` for MSI packages:

* Git release tag: `v2026.45`
* MSI / WinGet package version: `26.45.0`

### Automated Release Workflow

The [`.github/workflows/winget.yml`](../../.github/workflows/winget.yml) workflow automates submitting manifest pull requests to `microsoft/winget-pkgs`:

1. It runs when the Release workflow has finished uploading the installers, or by hand via `workflow_dispatch` (optionally with a tag).
2. It calls `scripts/release.py versions --tag <tag>` to resolve the MSI version (`26.45.0`).
3. It uses `michidk/run-komac@v2` ([Komac](https://github.com/russellbanks/Komac)) to:
   - Point the manifest at the release's x64 and arm64 MSI installers.
   - Update and submit the package manifest to `microsoft/winget-pkgs`.

#### Required Repository Secret

To enable automated WinGet submissions:

1. Create a Classic GitHub Personal Access Token (PAT) with `public_repo` scope.
2. Ensure a fork of [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) exists under the repository owner account.
3. Add the token to repository secrets under the name `WINGET_TOKEN`.

If `WINGET_TOKEN` is not configured, the workflow safely logs an informational step summary and exits cleanly.

### Manual Update with Komac

To manually submit or update a release using the [Komac CLI](https://github.com/russellbanks/Komac):

```powershell
komac update HakanSeven.OpenCADStudio `
  --version 26.45.0 `
  --urls "https://github.com/HakanSeven12/OpenCADStudio/releases/download/v2026.45/OpenCADStudio-v2026.45-windows-x86_64-installer.msi" `
         "https://github.com/HakanSeven12/OpenCADStudio/releases/download/v2026.45/OpenCADStudio-v2026.45-windows-arm64-installer.msi" `
  --submit
```
