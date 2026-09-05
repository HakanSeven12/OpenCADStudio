<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.el.md">Ελληνικά</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Λογότυπο του Open CAD Studio"></p>

<h1 align="center">Open CAD Studio</h1>

<p align="center">Εφαρμογή ανοιχτού κώδικα για δισδιάστατη σχεδίαση και τρισδιάστατη μοντελοποίηση, για υπολογιστές και τον ιστό, γραμμένη σε Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Τελευταία έκδοση" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Λήψεις εκδόσεων" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="Αστέρια στο GitHub" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="Άδεια GPL-3.0" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Άνοιγμα της διαδικτυακής εφαρμογής</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Λήψη της εφαρμογής για υπολογιστές</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Συμμετοχή στη συζήτηση</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Χώρος εργασίας του Open CAD Studio" width="100%"></p>

## Επισκόπηση

Το Open CAD Studio είναι μια εφαρμογή για τεχνική σχεδίαση, διάταξη φύλλων και μοντελοποίηση στερεών, η οποία λειτουργεί σε διαφορετικές πλατφόρμες. Διαβάζει και γράφει απευθείας σχέδια DWG και DXF, με κοινό πυρήνα επεξεργασίας στις εκδόσεις για υπολογιστές και προγράμματα περιήγησης.

Το έργο βρίσκεται υπό ενεργή ανάπτυξη. Διατηρείτε αντίγραφα ασφαλείας των σημαντικών σχεδίων σας και αναφέρετε προβλήματα που μπορούν να αναπαραχθούν μέσω των [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Κύρια χαρακτηριστικά

- **Άμεση επεξεργασία σχεδίων** — άνοιγμα, επεξεργασία, ανάκτηση και αποθήκευση αρχείων DWG και DXF χωρίς υπηρεσία μετατροπής.
- **Ακριβής δισδιάστατη σχεδίαση** — γραμμές, πολυγραμμές, καμπύλες, splines, διαγραμμίσεις, έλξεις αντικειμένων, παρακολούθηση, στρώσεις, μπλοκ και εξωτερικές αναφορές.
- **Εργαλεία τεκμηρίωσης** — κείμενο, διαστάσεις, γραμμές υπόδειξης, ανοχές, πίνακες, χώρος μοντέλου, χώρος χαρτιού, παράθυρα προβολής και στυλ εκτύπωσης.
- **Τρισδιάστατη μοντελοποίηση με γεωμετρικό πυρήνα** — βασικά στερεά, εξώθηση, περιστροφή, σάρωση, loft, πράξεις Boolean και ψηφίδωση οντοτήτων ACIS.
- **Απόδοση μέσω GPU** — επιταχυνόμενες δισδιάστατες και τρισδιάστατες προβολές μέσω του `wgpu`, με ορθογραφικές και προοπτικές κάμερες.
- **Επεκτάσιμες ροές εργασίας** — εγγενή πρόσθετα, δέσμες εντολών, μετατροπή χωρίς γραφικό περιβάλλον και API αυτοματισμού JSON με ένα αντικείμενο ανά γραμμή.

<p align="center"><img src="../../site/modeling.png" alt="Τρισδιάστατο μοντέλο στο Open CAD Studio" width="100%"></p>

## Μορφές αρχείων και εργασίες

| Μορφή ή εργασία | Υποστήριξη |
| --- | --- |
| DWG | Ανάγνωση και εγγραφή· αποθήκευση σε εκδόσεις από R14 έως 2018 |
| DXF | Ανάγνωση και εγγραφή· αποθήκευση σε εκδόσεις από R14 έως 2018 |
| BAK / SV$ | Άνοιγμα αντιγράφων ασφαλείας σχεδίων και αρχείων αυτόματης αποθήκευσης |
| OBJ | Εισαγωγή πολυγωνικών πλεγμάτων |
| LandXML | Εισαγωγή τοπογραφικών σημείων `CgPoint` |
| STL | Εξαγωγή δεδομένων τρισδιάστατου πλέγματος |
| STEP AP203 | Εξαγωγή δεδομένων τρισδιάστατου πλέγματος |
| PDF | Εκτύπωση διατάξεων και επιλεγμένης γεωμετρίας στην εφαρμογή για υπολογιστές |
| CSV | Εξαγωγή δεδομένων ιδιοτήτων οντοτήτων |
| CTB / STB | Φόρτωση και επεξεργασία πινάκων στυλ εκτύπωσης |

## Υπολογιστής ή ιστός

Χρησιμοποιήστε τη [διαδικτυακή εφαρμογή](https://www.opencadstudio.com) για άμεση πρόσβαση χωρίς εγκατάσταση. Τα σχέδια επιλέγονται μέσω του προγράμματος περιήγησης και αποθηκεύονται ως τοπικές λήψεις.

Χρησιμοποιήστε την εφαρμογή για υπολογιστές για συσχετίσεις αρχείων, μικρογραφίες στη διαχείριση αρχείων, εκτύπωση μέσω του συστήματος, εξαγωγή PDF, εξωτερικά πρόσθετα, δέσμες εντολών και αυτοματισμό χωρίς γραφικό περιβάλλον. Διατίθενται εκδόσεις για Windows, Linux και macOS με Apple Silicon.

## Εγκατάσταση

Κατεβάστε όλα τα τρέχοντα πακέτα από την [τελευταία έκδοση](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

Επιλέξτε ένα από τα υπογεγραμμένα πακέτα x86-64:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — το προτεινόμενο πρόγραμμα εγκατάστασης, με συντομεύσεις στο μενού Έναρξη, συσχετίσεις αρχείων DWG/DXF και μικρογραφίες σχεδίων.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — αυτόνομη εφαρμογή που δεν απαιτεί εγκατάσταση.

### Linux

Κατεβάστε το AppImage για x86-64, κάντε το εκτελέσιμο και εκκινήστε το:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

Το διαθέσιμο πακέτο για macOS υποστηρίζει Apple Silicon:

1. Κατεβάστε το `OpenCADStudio-*-macos-arm64.dmg`.
2. Ανοίξτε την εικόνα δίσκου και σύρετε το `OpenCADStudio.app` στον φάκελο **Εφαρμογές (Applications)**.
3. Αν το Gatekeeper εμποδίσει την πρώτη εκκίνηση, εγκρίνετε την εφαρμογή από τις **Ρυθμίσεις συστήματος → Απόρρητο και ασφάλεια (System Settings → Privacy & Security)**.

Η εφαρμογή διαθέτει υπογραφή ad-hoc, αλλά δεν έχει ακόμη επικυρωθεί μέσω της διαδικασίας notarization της Apple.

## Γλώσσες

Το Open CAD Studio μπορεί να ακολουθεί τη γλώσσα του συστήματος ή να χρησιμοποιεί μία από τις ακόλουθες 21 γλώσσες περιβάλλοντος:

> Αραβικά · Πορτογαλικά Βραζιλίας · Βουλγαρικά · Τσεχικά · Ολλανδικά · Αγγλικά · Φινλανδικά · Γαλλικά · Γερμανικά · Ελληνικά · Χίντι · Ουγγρικά · Ιταλικά · Ιαπωνικά · Κορεατικά · Πολωνικά · Ρωσικά · Απλοποιημένα κινεζικά · Ισπανικά · Παραδοσιακά κινεζικά · Τουρκικά

Αλλάξτε τη γλώσσα από τις ρυθμίσεις της εφαρμογής. Όταν είναι επιλεγμένη η **Γλώσσα συστήματος**, η διαδικτυακή έκδοση χρησιμοποιεί επίσης την προτιμώμενη γλώσσα του προγράμματος περιήγησης.

## Μεταγλώττιση από τον πηγαίο κώδικα

### Εφαρμογή για υπολογιστές

Απαιτήσεις:

- Git
- Τρέχουσα σταθερή έκδοση της εργαλειοθήκης Rust
- Βιβλιοθήκες ανάπτυξης γραφικών και γραμματοσειρών για την πλατφόρμα σας

Σε Ubuntu ή Debian, εγκαταστήστε τις εξαρτήσεις με:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Στη συνέχεια, μεταγλωττίστε την εφαρμογή:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

Το εκτελέσιμο αρχείο δημιουργείται στη θέση `target/release/OpenCADStudio` (`OpenCADStudio.exe` στα Windows).

### Διαδικτυακή εφαρμογή

Εγκαταστήστε μία φορά τον στόχο WebAssembly και τα εργαλεία μεταγλώττισης:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Εκκινήστε τον διακομιστή ανάπτυξης:

```bash
trunk serve
```

## Αυτοματισμός

Το εκτελέσιμο για υπολογιστές υποστηρίζει μεμονωμένες μετατροπές, έναν μόνιμο διακομιστή χωρίς γραφικό περιβάλλον και ένα σημείο σύνδεσης MCP για εφαρμογές τεχνητής νοημοσύνης, ανεξάρτητα από τον πελάτη:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
OpenCADStudio --mcp
```

Ο διακομιστής αυτοματισμού ανταλλάσσει ένα αντικείμενο JSON ανά γραμμή μέσω της τυπικής εισόδου/εξόδου ή μιας τοπικής υποδοχής TCP. Το αυτοτελές σημείο σύνδεσης MCP παρέχει πρόσβαση στον ενεργό επεξεργαστή της εφαρμογής μέσω των ίδιων εργαλείων σε κάθε συμβατό πελάτη. Για τη σύνδεση, ρυθμίστε τον πελάτη ώστε να εκκινεί το `OpenCADStudio --mcp`. Δείτε τον [οδηγό ελέγχου MCP](../automation/README.md).

## Πρόσθετα

Τα πρόσθετα για υπολογιστές εκτελούνται σε ξεχωριστές διεργασίες και επικοινωνούν με την κύρια εφαρμογή μέσω του API προσθέτων με καθορισμένη έκδοση. Η διαδικτυακή έκδοση δεν φορτώνει εγγενή πρόσθετα.

- [Αρχιτεκτονική προσθέτων](../plugin-architecture.md)
- [Πρότυπο προσθέτου](../plugin-template/README.md)
- [Μητρώο προσθέτων](../../plugins/README.md)

## Τεκμηρίωση του έργου

- [API αυτοματισμού](../automation/README.md)
- [Αρχιτεκτονική προσθέτων](../plugin-architecture.md)
- [Στάδια ψηφίδωσης](../tessellation.md)
- [Πολιτική ασφάλειας](../../SECURITY.md)

## Συνεισφορά

Είναι ευπρόσδεκτες οι αναφορές σφαλμάτων, τα pull requests με συγκεκριμένο αντικείμενο, οι μεταφράσεις, οι βελτιώσεις στην τεκμηρίωση και οι συνεισφορές προσθέτων.

- Αναζητήστε στα υπάρχοντα [issues](https://github.com/HakanSeven12/OpenCADStudio/issues) πριν υποβάλετε νέα αναφορά.
- Χρησιμοποιήστε τα [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) για ερωτήσεις και ιδέες.
- Αναφέρετε ιδιωτικά τυχόν ευπάθειες, ακολουθώντας την [πολιτική ασφάλειας](../../SECURITY.md).

## Ανάπτυξη του έργου

### Αστέρια

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Ιστορικό αστεριών του Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

### Λήψεις εκδόσεων

<a href="https://github.com/HakanSeven12/OpenCADStudio/releases">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/download-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/download-history-light.svg">
    <img alt="Ιστορικό λήψεων εκδόσεων του Open CAD Studio" src="https://www.opencadstudio.com/download-history-light.svg">
  </picture>
</a>

## Υποστήριξη του έργου

Αν το Open CAD Studio σας βοηθά στην εργασία σας, υποστηρίξτε τη συνεχή ανάπτυξή του μέσω του [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) ή του [Patreon](https://www.patreon.com/HakanSeven12).

## Άδεια χρήσης

Το Open CAD Studio διανέμεται υπό τη [Γενική Άδεια Δημόσιας Χρήσης GNU v3.0](../../LICENSE).
