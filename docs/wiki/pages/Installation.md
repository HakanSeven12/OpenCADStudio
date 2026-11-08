# Installation

Open CAD Studio comes as a desktop program for Windows, Linux and macOS, and as a web app that runs in the browser. Both open the same drawings with the same editing tools. The desktop program adds file associations, printing to system printers, PDF output, plugins, command scripts and command-line automation.

All downloads are on the [download page](https://www.opencadstudio.com/download/), which always points at the latest release. Every release and its files are also listed on the [releases page](https://github.com/HakanSeven12/OpenCADStudio/releases).

## Web app

Open [opencadstudio.com](https://www.opencadstudio.com) in a current browser. Nothing is installed. Drawings are picked from your computer or phone and saved back as downloads. Native plugins and command-line automation are not available in the browser.

## Windows

Pick the package that matches your processor: `x86_64` for most PCs, `arm64` for ARM processors.

| Package | Use it when |
|---|---|
| `…-windows-x86_64-installer.msi` | You want Start menu shortcuts, DWG/DXF file associations and drawing thumbnails in the file manager. Recommended. |
| `…-windows-x86_64-portable.exe` | You want to run the program without installing it, for example from a USB drive. |

## Linux

**AppImage.** Download the AppImage for your processor (`linux-x86_64` or `linux-aarch64`), make it executable and run it:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

**Flatpak.** The program is also published as a Flatpak through FlatPark, which follows new releases:

```bash
flatpak remote-add --if-not-exists flatpark https://dl.flatpark.org/flatpark.flatpakrepo
flatpak install flatpark io.github.HakanSeven12.OpenCadStudio
```

The Flatpak opens and saves drawings through the system file chooser. External references and underlays that point to other files need access to your home folder:

```bash
flatpak override --user --filesystem=home io.github.HakanSeven12.OpenCadStudio
```

**Snap.** Each release also has a snap package. It is not in the store yet, so it installs only with `--dangerous` and does not update itself.

## macOS

1. Download the `.dmg` for your Mac: `macos-arm64` for M-series Macs, `macos-x86_64` for older Macs.
2. Open it and drag **OpenCADStudio.app** into **Applications**.
3. If the first start is blocked, allow the app under **System Settings › Privacy & Security**.

## Updating

The desktop program checks for a newer release when it starts and tells you when one is available. Install the new package over the old one; your settings, aliases and shortcuts are kept.

## Where your settings live

Settings, the alias file, shortcuts and your own patterns and linetypes are kept in one folder:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\OpenCADStudio` |
| Linux | `~/.config/OpenCADStudio` (or `$XDG_CONFIG_HOME/OpenCADStudio`) |
| macOS | `~/Library/Application Support/OpenCADStudio` |

## Language

The interface is available in 23 languages. By default it follows the system language. Change it under **Options › General**.

## Building from source

The program is written in Rust. With a current stable Rust toolchain installed:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

Linux needs the usual graphics and font development packages; the [README](https://github.com/HakanSeven12/OpenCADStudio#build-from-source) lists them, together with the steps for the web build.
