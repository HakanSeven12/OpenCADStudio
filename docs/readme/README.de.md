<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Open-CAD-Studio-Logo"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Quelloffenes 2D-Zeichnen und 3D-Modellieren für Desktop und Web, entwickelt mit Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Neueste Version" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="GitHub-Sterne" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="GPL-3.0-Lizenz" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Web-App starten</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Desktop-App herunterladen</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>An Diskussionen teilnehmen</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Arbeitsbereich von Open CAD Studio" width="100%"></p>

## Überblick

Open CAD Studio ist eine plattformübergreifende Anwendung für technische Zeichnungen, Layoutarbeit und Volumenmodellierung. DWG- und DXF-Zeichnungen werden nativ gelesen und geschrieben; Desktop- und Browserversion nutzen denselben Bearbeitungskern.

Das Projekt wird aktiv entwickelt. Bewahre Sicherungskopien wichtiger Produktionszeichnungen auf und melde reproduzierbare Probleme über [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Höhepunkte

- **Nativer Zeichnungsablauf** — DWG- und DXF-Dateien ohne Konvertierungsdienst öffnen, bearbeiten, wiederherstellen und speichern.
- **Präzises 2D-Zeichnen** — Linien, Polylinien, Kurven, Splines, Schraffuren, Objektfang, Spurverfolgung, Layer, Blöcke und externe Referenzen.
- **Dokumentationswerkzeuge** — Text, Bemaßungen, Führungslinien, Toleranzen, Tabellen, Modellbereich, Papierbereich, Ansichtsfenster und Plotstile.
- **Kernel-gestützte 3D-Modellierung** — Volumenprimitive, Extrusion, Rotation, Sweep, Loft, boolesche Operationen und Tessellierung von ACIS-Elementen.
- **GPU-Darstellung** — beschleunigte 2D- und 3D-Ansichten über `wgpu` mit orthografischen und perspektivischen Kameras.
- **Erweiterbare Abläufe** — native Plugins, Befehlsskripte, Konvertierung ohne Oberfläche und zeilenbasierte JSON-Automatisierungs-API.

<p align="center"><img src="../../site/modeling.png" alt="3D-Modell in Open CAD Studio" width="100%"></p>

## Dateiabläufe

| Format oder Ablauf | Unterstützung |
| --- | --- |
| DWG | Lesen und Schreiben; versionierte Speicherziele von R14 bis 2018 |
| DXF | Lesen und Schreiben; versionierte Speicherziele von R14 bis 2018 |
| BAK / SV$ | Zeichnungssicherungen und automatisch gespeicherte Dateien öffnen |
| OBJ | Polygonnetze importieren |
| LandXML | `CgPoint`-Vermessungspunkte importieren |
| STL | 3D-Netzdaten exportieren |
| STEP AP203 | 3D-Netzdaten exportieren |
| PDF | Layouts und ausgewählte Geometrie auf dem Desktop plotten |
| CSV | Eigenschaftsdaten von Elementen extrahieren |
| CTB / STB | Plotstiltabellen laden und bearbeiten |

## Desktop oder Web

Nutze die [Web-App](https://www.opencadstudio.com) für sofortigen Zugriff ohne Installation. Zeichnungen werden im Browser ausgewählt und als lokale Downloads gespeichert.

Nutze die Desktop-Anwendung für native Dateizuordnungen, Vorschaubilder im Dateimanager, Systemdruck, PDF-Ausgabe, externe Plugins, Befehlsskripte und Automatisierung ohne Oberfläche. Veröffentlichungen sind für Windows, Linux und Apple-Silicon-macOS verfügbar.

## Installation

Lade alle aktuellen Pakete von der [neuesten Version](https://github.com/HakanSeven12/OpenCADStudio/releases/latest) herunter.

### Windows

Wähle eines dieser signierten x86-64-Pakete:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — empfohlener Installer mit Startmenü-Verknüpfungen, DWG/DXF-Dateizuordnungen und Zeichnungsvorschauen.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — eigenständige Anwendung ohne Installation.

### Linux

Lade das x86-64-AppImage herunter, mache es ausführbar und starte es:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

Das veröffentlichte macOS-Paket unterstützt Apple Silicon:

1. `OpenCADStudio-*-macos-arm64.dmg` herunterladen.
2. Image öffnen und `OpenCADStudio.app` nach **Applications** ziehen.
3. Falls Gatekeeper den ersten Start blockiert, die App unter **System Settings → Privacy & Security** freigeben.

Die Anwendung ist ad hoc signiert, wird derzeit aber nicht von Apple notarisiert.

## Sprachen

Open CAD Studio kann der Systemsprache folgen oder eine dieser 20 Oberflächensprachen verwenden:

> Arabisch · Brasilianisches Portugiesisch · Bulgarisch · Tschechisch · Niederländisch · Englisch · Finnisch · Französisch · Deutsch · Hindi · Ungarisch · Italienisch · Japanisch · Koreanisch · Polnisch · Russisch · Vereinfachtes Chinesisch · Spanisch · Traditionelles Chinesisch · Türkisch

Die Sprache lässt sich in den Anwendungseinstellungen ändern. Die Browserversion verwendet bei Auswahl von **System** ebenfalls das bevorzugte Gebietsschema des Browsers.

## Aus dem Quellcode erstellen

### Desktop

Voraussetzungen:

- Git
- Aktuelle stabile Rust-Toolchain
- Plattformspezifische Entwicklungsbibliotheken für Grafik und Schriften

Unter Ubuntu oder Debian die nativen Abhängigkeiten installieren:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Danach erstellen:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

Die erzeugte Binärdatei liegt unter `target/release/OpenCADStudio` (unter Windows `OpenCADStudio.exe`).

### Web

WebAssembly-Ziel und Build-Werkzeuge einmalig installieren:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Entwicklungsserver starten:

```bash
trunk serve
```

## Automatisierung

Die Desktop-Binärdatei unterstützt einmalige Konvertierung und einen dauerhaften Server ohne Oberfläche:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

Der Server tauscht über Standardein-/ausgabe oder einen lokalen TCP-Socket je Zeile ein JSON-Objekt aus. Siehe [Automatisierungsanleitung](../automation/README.md) und den enthaltenen [Python-Client](../automation/ocs.py).

## Plugins

Desktop-Plugins laufen in getrennten Prozessen und kommunizieren über die versionierte Plugin-API mit dem Host. Die Browserversion lädt keine nativen Plugins.

- [Plugin-Architektur](../plugin-architecture.md)
- [Plugin-Vorlage](../plugin-template/README.md)
- [Plugin-Verzeichnis](../../plugins/README.md)

## Projektdokumentation

- [Automatisierungs-API](../automation/README.md)
- [Plugin-Architektur](../plugin-architecture.md)
- [Tessellierungs-Pipeline](../tessellation.md)
- [Sicherheitsrichtlinie](../../SECURITY.md)

## Mitwirken

Fehlerberichte, fokussierte Pull Requests, Übersetzungen, Dokumentationsverbesserungen und Plugin-Beiträge sind willkommen.

- Vor einem neuen Bericht bestehende [Issues](https://github.com/HakanSeven12/OpenCADStudio/issues) durchsuchen.
- [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) für Fragen und Ideen verwenden.
- Schwachstellen gemäß der [Sicherheitsrichtlinie](../../SECURITY.md) vertraulich melden.

## Projektwachstum

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Sterne und Versionsdownloads von Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## Projekt unterstützen

Wenn Open CAD Studio bei der Arbeit hilft, unterstütze die weitere Entwicklung über [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) oder [Patreon](https://www.patreon.com/HakanSeven12).

## Lizenz

Open CAD Studio wird unter der [GNU General Public License v3.0](../../LICENSE) bereitgestellt.
