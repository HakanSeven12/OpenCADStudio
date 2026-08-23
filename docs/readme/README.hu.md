<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Open CAD Studio logó"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Nyílt forráskódú 2D rajzolás és 3D modellezés asztali gépre és webre, Rust nyelven.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Legújabb kiadás" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Letöltések" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="GitHub-csillagok" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="GPL-3.0 licenc" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Webalkalmazás indítása</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Asztali alkalmazás letöltése</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Csatlakozás a beszélgetéshez</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Open CAD Studio munkaterület" width="100%"></p>

## Áttekintés

Az Open CAD Studio többplatformos alkalmazás műszaki rajzoláshoz, elrendezések készítéséhez és testmodellezéshez. Natívan olvas és ír DWG- és DXF-rajzokat; az asztali és böngészős változat közös szerkesztőmagot használ.

A projekt aktív fejlesztés alatt áll. A fontos gyártási rajzokról tarts biztonsági másolatot, a reprodukálható hibákat pedig jelentsd a [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues) oldalon.

## Főbb jellemzők

- **Natív rajzi munkafolyamat** — DWG- és DXF-fájlok megnyitása, szerkesztése, helyreállítása és mentése átalakító szolgáltatás nélkül.
- **Pontos 2D rajzolás** — vonalak, vonalláncok, görbék, spline-ok, sraffozások, tárgyraszterek, követés, rétegek, blokkok és külső referenciák.
- **Dokumentációs eszközök** — szöveg, méretezés, mutatóvonalak, tűrések, táblázatok, modelltér, papírtér, nézetablakok és nyomtatási stílusok.
- **Kernelalapú 3D modellezés** — testprimitívek, kihúzás, forgatás, söprés, loft, logikai műveletek és ACIS-entitások tesszellációja.
- **GPU-megjelenítés** — `wgpu` által gyorsított 2D és 3D nézetek, ortografikus és perspektivikus kamerákkal.
- **Bővíthető munkafolyamatok** — natív bővítmények, parancsfájlok, felület nélküli konverzió és soralapú JSON automatizálási API.

<p align="center"><img src="../../site/modeling.png" alt="3D modell az Open CAD Studióban" width="100%"></p>

## Fájlmunkafolyamatok

| Formátum vagy munkafolyamat | Támogatás |
| --- | --- |
| DWG | Olvasás és írás; verziózott mentési célok R14-től 2018-ig |
| DXF | Olvasás és írás; verziózott mentési célok R14-től 2018-ig |
| BAK / SV$ | Rajzi biztonsági másolatok és automatikus mentések megnyitása |
| OBJ | Poligonhálók importálása |
| LandXML | `CgPoint` felmérési pontok importálása |
| STL | 3D hálóadatok exportálása |
| STEP AP203 | 3D hálóadatok exportálása |
| PDF | Elrendezések és kijelölt geometria nyomtatása asztali gépen |
| CSV | Entitástulajdonságok adatainak kinyerése |
| CTB / STB | Nyomtatásistílus-táblák betöltése és szerkesztése |

## Asztali vagy webes változat

Használd a [webalkalmazást](https://www.opencadstudio.com) azonnali, telepítés nélküli hozzáféréshez. A rajzok a böngészőben választhatók ki és helyi letöltésként menthetők.

Az asztali alkalmazást válaszd natív fájltársításokhoz, fájlkezelői bélyegképekhez, rendszernyomtatáshoz, PDF-kimenethez, külső bővítményekhez, parancsfájlokhoz és felület nélküli automatizáláshoz. Windows, Linux és Apple Silicon macOS rendszerhez érhetők el kiadások.

## Telepítés

Minden aktuális csomag a [legújabb kiadásból](https://github.com/HakanSeven12/OpenCADStudio/releases/latest) tölthető le.

### Windows

Válassz az aláírt x86-64 csomagok közül:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — ajánlott telepítő Start menü-parancsikonokkal, DWG/DXF-fájltársításokkal és rajzi bélyegképekkel.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — önálló alkalmazás, telepítés nélkül.

### Linux

Töltsd le az x86-64 AppImage fájlt, tedd futtathatóvá, majd indítsd el:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

A kiadott macOS-csomag az Apple Silicon rendszereket támogatja:

1. Töltsd le az `OpenCADStudio-*-macos-arm64.dmg` fájlt.
2. Nyisd meg a lemezképet, és húzd az `OpenCADStudio.app` alkalmazást az **Applications** mappába.
3. Ha a Gatekeeper blokkolja az első indítást, engedélyezd az alkalmazást a **System Settings → Privacy & Security** alatt.

Az alkalmazás ad hoc aláírással rendelkezik, de az Apple jelenleg nem hitelesítette közjegyzői eljárással.

## Nyelvek

Az Open CAD Studio követheti a rendszer nyelvét, vagy használhatja az alábbi 20 felületi nyelv egyikét:

> Arab · Brazil portugál · Bolgár · Cseh · Holland · Angol · Finn · Francia · Német · Hindi · Magyar · Olasz · Japán · Koreai · Lengyel · Orosz · Egyszerűsített kínai · Spanyol · Hagyományos kínai · Török

A nyelv az alkalmazás beállításaiban módosítható. **Rendszer** választásakor a böngészős változat is a böngésző előnyben részesített területi beállítását használja.

## Fordítás forráskódból

### Asztali alkalmazás

Követelmények:

- Git
- Aktuális stabil Rust-eszközlánc
- A platform grafikai és betűkészlet-fejlesztő könyvtárai

Ubuntu vagy Debian alatt telepítsd a natív függőségeket:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Ezután fordítsd le:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

A létrejövő bináris a `target/release/OpenCADStudio` helyre kerül (Windows alatt `OpenCADStudio.exe`).

### Web

Telepítsd egyszer a WebAssembly célt és a fordítóeszközöket:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Indítsd el a fejlesztői kiszolgálót:

```bash
trunk serve
```

## Automatizálás

Az asztali bináris egyszeri konverziót és tartós, felület nélküli kiszolgálót támogat:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

A kiszolgáló soronként egy JSON-objektumot cserél a szabványos bemeneten/kimeneten vagy helyi TCP-foglalaton. Lásd az [automatizálási útmutatót](../automation/README.md) és a mellékelt [Python-klienst](../automation/ocs.py).

## Bővítmények

Az asztali bővítmények külön folyamatokban futnak, és a verziózott bővítmény-API-n keresztül kommunikálnak a gazdával. A böngészős változat nem tölt be natív bővítményeket.

- [Bővítményarchitektúra](../plugin-architecture.md)
- [Bővítménysablon](../plugin-template/README.md)
- [Bővítményjegyzék](../../plugins/README.md)

## Projektdokumentáció

- [Automatizálási API](../automation/README.md)
- [Bővítményarchitektúra](../plugin-architecture.md)
- [Tesszellációs folyamat](../tessellation.md)
- [Biztonsági szabályzat](../../SECURITY.md)

## Közreműködés

Hibajelentéseket, célzott pull requesteket, fordításokat, dokumentációfejlesztést és bővítmény-hozzájárulásokat egyaránt várunk.

- Új jelentés előtt keress a meglévő [issue-k](https://github.com/HakanSeven12/OpenCADStudio/issues) között.
- Kérdésekhez és ötletekhez használd a [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) oldalt.
- A sérülékenységeket a [biztonsági szabályzat](../../SECURITY.md) szerint, bizalmasan jelentsd.

## A projekt növekedése

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Open CAD Studio csillagok és kiadásletöltések" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## A projekt támogatása

Ha az Open CAD Studio segíti a munkádat, támogasd a további fejlesztést a [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) vagy a [Patreon](https://www.patreon.com/HakanSeven12) oldalán.

## Licenc

Az Open CAD Studio a [GNU General Public License v3.0](../../LICENSE) alatt kerül terjesztésre.
