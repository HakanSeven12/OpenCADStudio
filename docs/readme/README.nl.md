<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Logo van Open CAD Studio"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Open-source 2D-tekenen en 3D-modelleren voor desktop en web, gebouwd met Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Nieuwste versie" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="GitHub-sterren" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="GPL-3.0-licentie" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Webapp starten</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Desktopapp downloaden</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Deelnemen aan discussies</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Werkruimte van Open CAD Studio" width="100%"></p>

## Overzicht

Open CAD Studio is een platformonafhankelijke toepassing voor technisch tekenen, lay-outwerk en solid modeling. DWG- en DXF-tekeningen worden rechtstreeks gelezen en geschreven; de desktop- en browserversie delen dezelfde bewerkingskern.

Het project wordt actief ontwikkeld. Bewaar reservekopieën van belangrijke productietekeningen en meld reproduceerbare problemen via [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Hoogtepunten

- **Native tekenworkflow** — DWG- en DXF-bestanden openen, bewerken, herstellen en opslaan zonder conversiedienst.
- **Nauwkeurig 2D-tekenen** — lijnen, polylijnen, krommen, splines, arceringen, object-snaps, tracking, lagen, blokken en externe referenties.
- **Documentatiegereedschap** — tekst, maatvoering, verwijslijnen, toleranties, tabellen, modelruimte, papierruimte, viewports en plotstijlen.
- **Kernelgestuurd 3D-modelleren** — solid-primitieven, extrusie, omwenteling, sweep, loft, Booleaanse bewerkingen en tessellatie van ACIS-entiteiten.
- **GPU-rendering** — versnelde 2D- en 3D-viewports via `wgpu`, met orthografische en perspectiefcamera's.
- **Uitbreidbare workflows** — native plug-ins, opdrachtscripts, headless conversie en een regelgebaseerde JSON-automatiserings-API.

<p align="center"><img src="../../site/modeling.png" alt="3D-model in Open CAD Studio" width="100%"></p>

## Bestandsworkflows

| Formaat of workflow | Ondersteuning |
| --- | --- |
| DWG | Lezen en schrijven; opslagdoelen van R14 tot en met 2018 |
| DXF | Lezen en schrijven; opslagdoelen van R14 tot en met 2018 |
| BAK / SV$ | Back-ups en automatisch opgeslagen tekeningen openen |
| OBJ | Polygonale meshes importeren |
| LandXML | `CgPoint`-meetpunten importeren |
| STL | 3D-meshgegevens exporteren |
| STEP AP203 | 3D-meshgegevens exporteren |
| PDF | Lay-outs en geselecteerde geometrie plotten op desktop |
| CSV | Eigenschapsgegevens van entiteiten extraheren |
| CTB / STB | Plotstijltabellen laden en bewerken |

## Desktop of web

Gebruik de [webapp](https://www.opencadstudio.com) voor directe toegang zonder installatie. Tekeningen worden via de browser geselecteerd en als lokale downloads opgeslagen.

Gebruik de desktoptoepassing voor native bestandskoppelingen, miniaturen in bestandsbeheer, systeemafdrukken, PDF-uitvoer, externe plug-ins, opdrachtscripts en headless automatisering. Uitgaven zijn beschikbaar voor Windows, Linux en Apple Silicon macOS.

## Installatie

Download alle actuele pakketten van de [nieuwste uitgave](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

Kies een van deze ondertekende x86-64-pakketten:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — aanbevolen installatieprogramma met snelkoppelingen in het menu Start, DWG/DXF-bestandskoppelingen en tekenminiaturen.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — zelfstandige toepassing; geen installatie nodig.

### Linux

Download de x86-64 AppImage, maak deze uitvoerbaar en start hem:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

Het gepubliceerde macOS-pakket ondersteunt Apple Silicon:

1. Download `OpenCADStudio-*-macos-arm64.dmg`.
2. Open de schijfkopie en sleep `OpenCADStudio.app` naar **Applications**.
3. Als Gatekeeper de eerste start blokkeert, keur de app dan goed via **System Settings → Privacy & Security**.

De toepassing is ad hoc ondertekend, maar momenteel niet door Apple genotariseerd.

## Talen

Open CAD Studio kan de systeemtaal volgen of een van deze 20 interfacetalen gebruiken:

> Arabisch · Braziliaans-Portugees · Bulgaars · Tsjechisch · Nederlands · Engels · Fins · Frans · Duits · Hindi · Hongaars · Italiaans · Japans · Koreaans · Pools · Russisch · Vereenvoudigd Chinees · Spaans · Traditioneel Chinees · Turks

Wijzig de taal in de toepassingsinstellingen. De browserversie gebruikt ook de voorkeurstaal van de browser wanneer **Systeem** is geselecteerd.

## Bouwen vanuit de broncode

### Desktop

Vereisten:

- Git
- Huidige stabiele Rust-toolchain
- Platformbibliotheken voor grafische en lettertypeontwikkeling

Installeer op Ubuntu of Debian de native afhankelijkheden:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Bouw daarna:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

Het binaire bestand wordt geschreven naar `target/release/OpenCADStudio` (`OpenCADStudio.exe` op Windows).

### Web

Installeer eenmalig het WebAssembly-doel en de buildgereedschappen:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Start de ontwikkelserver:

```bash
trunk serve
```

## Automatisering

Het desktopprogramma ondersteunt eenmalige conversie en een permanente headless server:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

De server wisselt per regel één JSON-object uit via standaardinvoer/-uitvoer of een lokale TCP-socket. Bekijk de [automatiseringshandleiding](../automation/README.md) en de meegeleverde [Python-client](../automation/ocs.py).

## Plug-ins

Desktopplug-ins draaien in afzonderlijke processen en communiceren via de geversioneerde plug-in-API met de host. De browserbuild laadt geen native plug-ins.

- [Plug-inarchitectuur](../plugin-architecture.md)
- [Plug-insjabloon](../plugin-template/README.md)
- [Plug-inregister](../../plugins/README.md)

## Projectdocumentatie

- [Automatiserings-API](../automation/README.md)
- [Plug-inarchitectuur](../plugin-architecture.md)
- [Tessellatiepijplijn](../tessellation.md)
- [Beveiligingsbeleid](../../SECURITY.md)

## Bijdragen

Bugmeldingen, gerichte pull requests, vertalingen, documentatieverbeteringen en plug-inbijdragen zijn welkom.

- Doorzoek bestaande [issues](https://github.com/HakanSeven12/OpenCADStudio/issues) voordat je een nieuwe melding opent.
- Gebruik [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) voor vragen en ideeën.
- Meld kwetsbaarheden privé volgens het [beveiligingsbeleid](../../SECURITY.md).

## Projectgroei

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Sterren en uitgavedownloads van Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## Steun het project

Als Open CAD Studio je werk helpt, steun dan de verdere ontwikkeling via [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) of [Patreon](https://www.patreon.com/HakanSeven12).

## Licentie

Open CAD Studio wordt verspreid onder de [GNU General Public License v3.0](../../LICENSE).
