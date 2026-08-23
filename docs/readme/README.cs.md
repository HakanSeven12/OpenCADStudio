<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Logo Open CAD Studio"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Open-source 2D kreslení a 3D modelování pro počítač i web, vytvořené v Rustu.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Nejnovější verze" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Stažení" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="Hvězdy GitHub" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="Licence GPL-3.0" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Spustit webovou aplikaci</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Stáhnout desktopovou aplikaci</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Zapojit se do diskuse</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Pracovní plocha Open CAD Studio" width="100%"></p>

## Přehled

Open CAD Studio je multiplatformní aplikace pro technické kreslení, práci s rozvržením a modelování těles. Nativně čte a zapisuje výkresy DWG a DXF; desktopová a prohlížečová verze používají společné editační jádro.

Projekt je aktivně vyvíjen. Důležité produkční výkresy zálohujte a reprodukovatelné problémy hlaste přes [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Hlavní funkce

- **Nativní práce s výkresy** — otevírání, úpravy, obnova a ukládání DWG a DXF bez konverzní služby.
- **Přesné 2D kreslení** — úsečky, křivky, spline, šrafy, uchopení objektů, trasování, hladiny, bloky a externí reference.
- **Dokumentační nástroje** — text, kóty, odkazové čáry, tolerance, tabulky, modelový prostor, výkresový prostor, výřezy a styly vykreslování.
- **3D modelování s geometrickým jádrem** — základní tělesa, vysunutí, rotace, tažení, loft, booleovské operace a teselace entit ACIS.
- **Vykreslování přes GPU** — akcelerované 2D a 3D pohledy pomocí `wgpu`, s ortografickou a perspektivní kamerou.
- **Rozšiřitelné postupy** — nativní pluginy, příkazové skripty, bezobslužná konverze a řádkové JSON automatizační API.

<p align="center"><img src="../../site/modeling.png" alt="3D model v Open CAD Studio" width="100%"></p>

## Práce se soubory

| Formát nebo postup | Podpora |
| --- | --- |
| DWG | Čtení a zápis; cílové verze ukládání R14 až 2018 |
| DXF | Čtení a zápis; cílové verze ukládání R14 až 2018 |
| BAK / SV$ | Otevírání záloh a automaticky uložených výkresů |
| OBJ | Import polygonových sítí |
| LandXML | Import zaměřených bodů `CgPoint` |
| STL | Export dat 3D sítí |
| STEP AP203 | Export dat 3D sítí |
| PDF | Vykreslení rozvržení a vybrané geometrie na počítači |
| CSV | Extrakce vlastností entit |
| CTB / STB | Načtení a úpravy tabulek stylů vykreslování |

## Počítač nebo web

Pro okamžitý přístup bez instalace použijte [webovou aplikaci](https://www.opencadstudio.com). Výkresy se vybírají v prohlížeči a ukládají jako místní soubory ke stažení.

Desktopovou aplikaci použijte pro nativní asociace souborů, náhledy ve správci souborů, systémový tisk, výstup PDF, externí pluginy, příkazové skripty a bezobslužnou automatizaci. Vydání jsou dostupná pro Windows, Linux a macOS s Apple Silicon.

## Instalace

Všechny aktuální balíčky stáhnete z [nejnovějšího vydání](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

Vyberte jeden z podepsaných balíčků x86-64:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — doporučený instalátor se zástupci v nabídce Start, asociacemi DWG/DXF a náhledy výkresů.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — samostatná aplikace bez instalace.

### Linux

Stáhněte x86-64 AppImage, nastavte jej jako spustitelný a spusťte:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

Publikovaný balíček pro macOS podporuje Apple Silicon:

1. Stáhněte `OpenCADStudio-*-macos-arm64.dmg`.
2. Otevřete obraz a přetáhněte `OpenCADStudio.app` do **Applications**.
3. Pokud Gatekeeper první spuštění zablokuje, povolte aplikaci v **System Settings → Privacy & Security**.

Aplikace je podepsána ad hoc, ale v současnosti není notářsky ověřena společností Apple.

## Jazyky

Open CAD Studio může používat jazyk systému nebo jeden z těchto 20 jazyků rozhraní:

> Arabština · Brazilská portugalština · Bulharština · Čeština · Nizozemština · Angličtina · Finština · Francouzština · Němčina · Hindština · Maďarština · Italština · Japonština · Korejština · Polština · Ruština · Zjednodušená čínština · Španělština · Tradiční čínština · Turečtina

Jazyk změníte v nastavení aplikace. Pokud je vybrána možnost **Systém**, webová verze používá také preferované národní prostředí prohlížeče.

## Sestavení ze zdrojového kódu

### Desktop

Požadavky:

- Git
- Aktuální stabilní nástroje Rust
- Vývojové knihovny grafiky a písem dané platformy

Na Ubuntu nebo Debianu nainstalujte nativní závislosti:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Poté sestavte:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

Výsledný program bude v `target/release/OpenCADStudio` (ve Windows `OpenCADStudio.exe`).

### Web

Jednorázově nainstalujte cíl WebAssembly a nástroje pro sestavení:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Spusťte vývojový server:

```bash
trunk serve
```

## Automatizace

Desktopový program podporuje jednorázovou konverzi a trvalý bezobslužný server:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

Server předává jeden objekt JSON na řádek přes standardní vstup/výstup nebo místní TCP socket. Viz [průvodce automatizací](../automation/README.md) a přiložený [klient Python](../automation/ocs.py).

## Pluginy

Desktopové pluginy běží v oddělených procesech a komunikují s hostitelem přes verzované API pluginů. Prohlížečová verze nativní pluginy nenačítá.

- [Architektura pluginů](../plugin-architecture.md)
- [Šablona pluginu](../plugin-template/README.md)
- [Registr pluginů](../../plugins/README.md)

## Dokumentace projektu

- [Automatizační API](../automation/README.md)
- [Architektura pluginů](../plugin-architecture.md)
- [Proces teselace](../tessellation.md)
- [Bezpečnostní zásady](../../SECURITY.md)

## Přispívání

Vítáme hlášení chyb, cílené pull requesty, překlady, vylepšení dokumentace i příspěvky k pluginům.

- Před otevřením nového hlášení prohledejte existující [issues](https://github.com/HakanSeven12/OpenCADStudio/issues).
- Pro otázky a nápady použijte [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions).
- Zranitelnosti hlaste soukromě podle [bezpečnostních zásad](../../SECURITY.md).

## Růst projektu

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Hvězdy a stažení Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## Podpořte projekt

Pokud vám Open CAD Studio pomáhá, podpořte další vývoj přes [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) nebo [Patreon](https://www.patreon.com/HakanSeven12).

## Licence

Open CAD Studio je šířeno pod [GNU General Public License v3.0](../../LICENSE).
