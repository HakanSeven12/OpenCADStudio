# Mérés eszköztár – fejlesztési csomag

Kiindulás: a feltöltött `measurement-development-full-006be890.zip`, Open CAD Studio `006be890`. A csomag kizárólag az ehhez képest új vagy módosított fájlokat tartalmazza, projektgyökérhez viszonyított útvonalakkal. Távoli tároló módosítása és Pull Request nem történt.

## Változások

- A meglévő `MEASURE_MENU` / `LargeDropdown` sorrendje: **Distance (DIST), Radius (MEASURERADIUS), Angle (MEASUREANGLE), Area (AREA)**. A komponens felépítése változatlan. A Distance és Area eredeti SVG-je változatlan.
- A `MEASURERADIUS` és `MEASUREANGLE` a meglévő `MeasureGeomCommand` külön kezdőállapotai. Közös kijelölési és számítási kódot használnak; a parancsregisztráció, menüindítás és parancssori indítás össze van kötve. A `MEASUREGEOM` és `MEA` továbbra is elérhető négy mérési móddal.
- Sugármérés: kör és körív, sugár és átmérő; más típus és hibás sugár esetén hibaüzenet, a parancs továbbra is vár érvényes kijelölést. Az objektum feletti kurzormozgás előzetes értéket mutat.
- Szögmérés: csúcs, első szár pontja, második szár pontja. Az első lépésben `Lines` / `L` két LINE objektum mérésére vált; onnan az első kijelölés előtt `Points` / `P` visszavált. Azonos objektum, nulla hosszúságú szár, nem LINE objektum és nem véges koordináta kezelve van.
- A szögszámítás `f64` vektorokat használ, skálázott normalizálással és `atan2`-vel. A végeredmény fokban jelenik meg, az `AUPREC` pontosságával. A hossz- és területadatok a meglévő formázókat használják.
- Közös, dokumentumfülenkénti, átmeneti mérési felirat. A parancsok csak az élő eredményszöveget szolgáltatják; a pozicionálás, végleges eredmény megőrzése és eltüntetése közös. A végleges kiírás a befejezés helyén marad, egérmozgatásra nem követi a kurzort.
- A rajzterületi bal, jobb vagy középső gomb lenyomása eltünteti a végleges feliratot, majd a normál eseménykezelés folytatódik. A befejező kattintás felengedése nem törli az éppen elkészült eredményt. ESC és új parancs indítása is töröl.
- A felirat nem vesz át egéreseményt, a rajzterület széléhez és a parancssorhoz igazodik, és a téma színeit használja. Az élő megjelenítés a DYN kapcsolótól független, alaphelyzetben a kurzor fölött van. A meglévő geometriai előnézet és tárgyraszter megmarad.
- Az AREA objektumkijelölési előnézete kijelölésváltozáskor számol; az összetett görbék területét nem integrálja újra minden képkockában. Add/Subtract közben az aktuális terület és az előzetes összeg látható, az összeg csak a mérés elfogadásakor változik.
- A felirat nem rajzi entitás, nem módosítja a DWG tartalmát vagy a dokumentum módosításjelzőjét. A meglévő parancssori eredmény megmarad.
- Új sugár- és szög-SVG, magyar és angol szövegek. A többi 20 nyelvi katalógus új kulcsai angol szöveget kaptak, hogy a katalógusok kulcskészlete egyezzen.
- Nincs új külső függőség; a `Cargo.toml` és `Cargo.lock` változatlan.

## Beillesztés PowerShellből

Zárd be a futó CAD-alkalmazást. A következő parancsok tiszta Git-munkafát és pontosan a kiinduló commitot követelik meg; eltérés esetén megállnak. Saját módosításaidat előbb mentsd el vagy commitold. A ZIP-et nem egy új almappába, hanem közvetlenül a projekt gyökerébe kell kicsomagolni.

```powershell
$ErrorActionPreference = 'Stop'
$project = 'C:\CAD-Development\OpenCADStudio-Dev'
$patch = Join-Path $env:USERPROFILE 'Downloads\measurement-tools-patch.zip'
Set-Location -LiteralPath $project
if (-not (Test-Path -LiteralPath $patch -PathType Leaf)) { throw 'A ZIP nem található.' }
$status = git status --porcelain
if ($LASTEXITCODE -ne 0) { throw 'Nem elérhető Git-munkafa.' }
if ($status) { throw 'A munkafa nem tiszta. Előbb mentsd a saját módosításaidat.' }
$baseCommit = git rev-parse '006be890^{commit}'
if ($LASTEXITCODE -ne 0) { throw 'A kiinduló commit nem található.' }
$currentCommit = git rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $currentCommit -ne $baseCommit) { throw 'Nem a 006be890 commiton állsz.' }
$currentBranch = git branch --show-current
if ($currentBranch -ne 'feature/measurement-tools') {
    git switch -c feature/measurement-tools
    if ($LASTEXITCODE -ne 0) { throw 'Az ág nem hozható létre. Ellenőrizd a már létező ágakat.' }
}
Expand-Archive -LiteralPath $patch -DestinationPath $project -Force
git status --short
git diff --stat
git diff --check
if ($LASTEXITCODE -ne 0) { throw 'Whitespace-ellenőrzési hiba.' }
```

Az új fájlokat a `git status --short` mutatja; a `git diff --stat` csak a már követett fájlokat sorolja fel. A fenti művelet nem commitol és nem pushol.

## Fordítás és célzott tesztek PowerShellből

A projekt meglévő Windows/Rust/MSVC előfeltételei és a függőségek letöltéséhez internetelérés szükséges.

```powershell
Set-Location 'C:\CAD-Development\OpenCADStudio-Dev'
cargo check --locked
if ($LASTEXITCODE -ne 0) { throw 'A cargo check sikertelen.' }
cargo test --locked --lib measurement -- --test-threads=1
if ($LASTEXITCODE -ne 0) { throw 'A mérési tesztek sikertelenek.' }
cargo test --locked --lib modules::draw::inquiry -- --test-threads=1
if ($LASTEXITCODE -ne 0) { throw 'Az inquiry tesztek sikertelenek.' }
cargo fmt --check
# A kiinduló projektben is vannak formázási eltérések; lásd az eredményeket.
cargo run --locked
```

A `--test-threads=1` a formázási beállításokat módosító tesztek és az alkalmazásállapot-tesztek kiszámítható futását segíti. Memóriahiány esetén a tesztek előtt megadható `$env:CARGO_BUILD_JOBS = '3'` és `$env:CARGO_PROFILE_TEST_DEBUG = '0'`. Ez újraépítést okozhat; a változókat a vizsgálat után `Remove-Item Env:CARGO_BUILD_JOBS, Env:CARGO_PROFILE_TEST_DEBUG -ErrorAction SilentlyContinue` paranccsal törölheted.

## Kézi ellenőrzés Windows alatt

1. A Mérés menüben ellenőrizd a négy elemet és azok sorrendjét; indítsd őket menüből és begépelt paranccsal is. Ellenőrizd külön a `MEASUREGEOM` négy opcióját és a `MEA` aliast.
2. DIST: mérj a `(0,0,0)` és `(3,4,0)` pontok között: 5 egység. Az első pont után mozgasd a kurzort; a felirat és a geometriai előnézet frissüljön. Próbáld ki tárgyraszterrel, DYN be- és kikapcsolva, más hosszformátummal/pontossággal, valamint nagy koordináták közelében.
3. MEASURERADIUS: mérj 10 sugarú körön és köríven: sugár 10, átmérő 20. Előbb csak vidd föléjük a kurzort, aztán kattints. Egyenes kijelölésekor hibaüzenetet várj, utána körrel lehessen befejezni. Üres terület fölött ne maradjon az előző objektum élő értéke.
4. MEASUREANGLE: csúcs `(0,0,0)`, első szár `(1,0,0)`, második `(0,1,0)` → 90°; második `(-1,1,0)` → 135°. A csúcs ismételt kiválasztása érvénytelen szár, és nem fejezheti be a mérést. Próbáld ki 3D pontokkal és eltérő AUPREC értékekkel is.
5. MEASUREANGLE → Lines: két 45°-ban álló LINE → 45°, merőlegesek → 90°, párhuzamosak → 0°. Fordított végpontsorrenddel ugyanaz az eredmény várható. Ugyanazon egyenes kétszeri, kör vagy nulla hosszúságú egyenes kijelölése ne adjon hamis eredményt.
6. AREA: `(0,0,0)`, `(4,0,0)`, `(0,3,0)`, Enter → terület 6, kerület 12. A harmadik pont mozgatásakor legyen élő érték. Ellenőrizd Objects módban a kört, zárt polylinet, és Add/Subtract módban több egymás utáni részterület összegzését.
7. Mindegyik befejezett mérés után mozgasd az egeret: a végleges felirat maradjon a helyén. A következő rajzterületi kattintás törölje, és egyben kezdje meg a normál kijelölést / folytassa az AREA következő részterületét. Próbálj közvetlenül a felirat szövegére is kattintani. A befejező kattintás felengedése ne törölje rögtön a feliratot.
8. ESC, új mérés, jobb és középső rajzterületi kattintás is törölje a végleges feliratot. Válts világos/sötét témát; teszteld a sarkokat és a nyitott parancssort. Válts dokumentumfület: másik rajz eredménye ne jelenjen meg az aktuális rajzon.
9. Csak mérés után ne nőjön az entitásszám és ne váljon módosítottá a tiszta rajz. Mentés és újranyitás után ne maradjon mérési felirat a DWG-ben. A parancssori előzményben továbbra is legyen olvasható az eredmény.

## Ismert korlátok

- A két egyenes mód az irányok kisebb, 0–90° közötti szögét adja, a végpontsorrendtől függetlenül. Térben kitérő egyeneseken is az irányszög mérhető; nincs metszéspont- vagy szögtartomány-kiválasztás. A hárompontos mód 0–180°-ot ad, nem irányított/reflex szöget.
- Az AREA és a MEASUREGEOM Area meglévő számítási jelentése megmarad: előbbi a meglévő térbeli/görbealapú mérés, utóbbi a munkasíkra vetített sokszög. Nem síkbeli pontsorokon ezért eltérhetnek.
- A végleges felirat képernyőpozícióhoz kötött átmeneti UI, nem modellhez kötött méretfelirat. Nem nyomtatható és nem szerkeszthető rajzi objektumként.
- A további 20 nyelv új szövegeinek anyanyelvi fordítása külön feladat.
- A Windows alatti interaktív kézi ellenőrzést a fenti útmutató alapján a helyi gépen kell elvégezni; itt nem történt Windows GUI-próba.

## Automatikus tesztlefedettség

22 új Rust-teszt került a forrásba: kör és körív sugara, érvénytelen/stale kijelölés, ismert és szélsőséges skálájú szögek, degenerált szárak, két egyenes irányszöge, előnézet, f64/pontosság/hosszformátum, AREA élő érték és Add/Subtract, objektumkijelölési előnézet, regisztráció és menüindítás, dokumentum változatlansága, felirat élettartama, valódi viewport-kattintások továbbfeldolgozása, valamint az Iced felirat egéreseményei és szélekhez igazítása. A tesztkód a következő öt fájlban van: `measuregeom.rs`, `dist.rs`, `area.rs`, `src/app/measurement.rs`, `src/app/view/overlay.rs`.

## Automatikus ellenőrzési eredmények

Ellenőrzési környezet: Linux x86_64, Rust/Cargo 1.98.0, 2026.10.14. A fordításnál és tesztelésnél a memóriaigény csökkentésére 3 build szálat, kikapcsolt debug információt és a függőségeknél 0 optimalizációs szintet használtam. A projekt profilfájljai változatlanok.

| Ellenőrzés | Tényleges eredmény |
|---|---|
| `cargo check --locked` | Sikeres, kilépési kód 0. |
| `cargo check --locked --tests` | Sikeres, kilépési kód 0. A tesztkód is lefordítható. |
| `cargo test --locked --lib measurement -- --test-threads=1` | 26 sikeres, 0 hibás, 1 kihagyott. Mind a 22 új mérési teszt sikeres. |
| `cargo test --locked --lib modules::draw::inquiry -- --test-threads=1` | 18 sikeres, 0 hibás, 0 kihagyott. |
| `cargo test --locked --lib i18n::tests::every_catalog_covers_and_formats_the_source_catalog -- --exact --test-threads=1` | 1 sikeres, 0 hibás. A katalógusok kulcskészlete és formázása ellenőrizve. |
| `cargo fmt --check` | Sikertelen a kiinduló és a módosított forráson is: ugyanaz a 72 fájl tartalmaz örökölt formázási eltéréseket. A feladattól független fájlokat nem formáztam át. |
| `git diff --check` | Sikeres, nincs whitespace-hiba a patchben. |
| `python scripts/test_locales.py` | A kiinduló projektben 135, a módosított projektben 134 hiányzó régi literál-leképezés. Új hiányzó leképezés nincs. Ez az eredeti ellenőrzőszkript mindkét állapoton hibát jelez. |
| SVG-k | XML-ként érvényesek; az új ikonok világos és sötét háttéren megtekintve. Az eredeti Distance/Area SVG és a két Cargo-fájl byte szerint változatlan. |
| Windows GUI / DWG-mentés kézi próbája | Ebben a környezetben nem futott; a fenti kézi ellenőrzési listával végezhető el. |

A fenti két tesztcsoport részben átfed, a darabszámokat nem szabad összeadni egyedi tesztszámként. A kihagyott teszt meglévő `viewport_dimension_external_fixture_keeps_unchanged_measurements`: az `OPENCAD_VIEWPORT_REGRESSION_DXF` változóban megadott külső DXF-et igényli. Az új tesztek egyike sincs kihagyva.

A Linux alatt használt pontos parancsok (a `+1.98.0` a telepített toolchaint választja):

```sh
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 cargo +1.98.0 check --locked --config 'profile.dev.package."*".opt-level=0'
CARGO_BUILD_JOBS=3 CARGO_PROFILE_DEV_DEBUG=0 cargo +1.98.0 check --locked --tests --config 'profile.dev.package."*".opt-level=0'
CARGO_BUILD_JOBS=3 CARGO_PROFILE_TEST_DEBUG=0 cargo +1.98.0 test --locked --config 'profile.test.package."*".opt-level=0' --lib measurement -- --test-threads=1
CARGO_BUILD_JOBS=3 CARGO_PROFILE_TEST_DEBUG=0 cargo +1.98.0 test --locked --config 'profile.test.package."*".opt-level=0' --lib modules::draw::inquiry -- --test-threads=1
CARGO_BUILD_JOBS=3 CARGO_PROFILE_TEST_DEBUG=0 cargo +1.98.0 test --locked --config 'profile.test.package."*".opt-level=0' --lib i18n::tests::every_catalog_covers_and_formats_the_source_catalog -- --exact --test-threads=1
cargo +1.98.0 fmt --check
```

A környezetben frissen telepített Rust 1.99 már `rustc -vV` alatt SIGBUS hibával leállt; ezen a toolchainen a cargo check/test nem érte el a projekt fordítását. A fenti sikeres eredmények Rust 1.98 alatt születtek. Ebből nem következik, hogy a helyi Windows 1.99 toolchain hibás lenne. A teljes projekt összes tesztjét és release buildjét nem futtattam.

## A csomag összes fájlja

42 fájl: 4 új, 38 módosított.

| Állapot | Fájl |
|---|---|
| Új | `assets/icons/measure-angle.svg` |
| Új | `assets/icons/measure-radius.svg` |
| Új | `docs/measurement-tools.md` |
| Módosított | `locales/ar-SA/opencadstudio.ftl` |
| Módosított | `locales/bg-BG/opencadstudio.ftl` |
| Módosított | `locales/cs-CZ/opencadstudio.ftl` |
| Módosított | `locales/de-DE/opencadstudio.ftl` |
| Módosított | `locales/el-GR/opencadstudio.ftl` |
| Módosított | `locales/en-US/opencadstudio.ftl` |
| Módosított | `locales/es-ES/opencadstudio.ftl` |
| Módosított | `locales/fi-FI/opencadstudio.ftl` |
| Módosított | `locales/fr-FR/opencadstudio.ftl` |
| Módosított | `locales/hi-IN/opencadstudio.ftl` |
| Módosított | `locales/hu-HU/opencadstudio.ftl` |
| Módosított | `locales/it-IT/opencadstudio.ftl` |
| Módosított | `locales/ja-JP/opencadstudio.ftl` |
| Módosított | `locales/ko-KR/opencadstudio.ftl` |
| Módosított | `locales/lo-LA/opencadstudio.ftl` |
| Módosított | `locales/nl-NL/opencadstudio.ftl` |
| Módosított | `locales/pl-PL/opencadstudio.ftl` |
| Módosított | `locales/pt-BR/opencadstudio.ftl` |
| Módosított | `locales/ru-RU/opencadstudio.ftl` |
| Módosított | `locales/tr-TR/opencadstudio.ftl` |
| Módosított | `locales/zh-CN/opencadstudio.ftl` |
| Módosított | `locales/zh-TW/opencadstudio.ftl` |
| Módosított | `src/app/command_driver/mod.rs` |
| Módosított | `src/app/commands/draw.rs` |
| Módosított | `src/app/commands/mod.rs` |
| Módosított | `src/app/control/mod.rs` |
| Módosított | `src/app/document.rs` |
| Új | `src/app/measurement.rs` |
| Módosított | `src/app/mod.rs` |
| Módosított | `src/app/update/command.rs` |
| Módosított | `src/app/update/mod.rs` |
| Módosított | `src/app/update/viewport.rs` |
| Módosított | `src/app/view/mod.rs` |
| Módosított | `src/app/view/overlay.rs` |
| Módosított | `src/command.rs` |
| Módosított | `src/modules/draw/inquiry/area.rs` |
| Módosított | `src/modules/draw/inquiry/dist.rs` |
| Módosított | `src/modules/draw/inquiry/measuregeom.rs` |
| Módosított | `src/modules/draw/mod.rs` |
