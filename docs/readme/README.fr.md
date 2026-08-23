<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Logo Open CAD Studio"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Dessin 2D et modélisation 3D open source pour ordinateur et web, développés en Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Dernière version" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Téléchargements" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="Étoiles GitHub" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="Licence GPL-3.0" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Lancer l’application web</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Télécharger l’application de bureau</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Participer aux discussions</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Espace de travail Open CAD Studio" width="100%"></p>

## Présentation

Open CAD Studio est une application multiplateforme destinée au dessin technique, à la mise en page et à la modélisation de solides. Elle lit et écrit nativement les dessins DWG et DXF, avec un même cœur d’édition pour les versions bureau et navigateur.

Le projet est en développement actif. Conservez des sauvegardes des dessins de production importants et signalez les problèmes reproductibles dans les [issues GitHub](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Points forts

- **Flux de dessin natif** — ouvrez, modifiez, récupérez et enregistrez les fichiers DWG et DXF sans service de conversion.
- **Dessin 2D précis** — lignes, polylignes, courbes, splines, hachures, accrochages aux objets, repérage, calques, blocs et références externes.
- **Outils de documentation** — texte, cotations, repères, tolérances, tableaux, espace objet, espace papier, fenêtres et styles de tracé.
- **Modélisation 3D avec noyau géométrique** — primitives solides, extrusion, révolution, balayage, lissage, opérations booléennes et tessellation d’entités ACIS.
- **Rendu GPU** — vues 2D et 3D accélérées par `wgpu`, avec caméras orthographique et perspective.
- **Flux extensibles** — plugins natifs, scripts de commandes, conversion sans interface et API d’automatisation JSON ligne par ligne.

<p align="center"><img src="../../site/modeling.png" alt="Modèle 3D dans Open CAD Studio" width="100%"></p>

## Flux de fichiers

| Format ou flux | Prise en charge |
| --- | --- |
| DWG | Lecture et écriture ; cibles d’enregistrement versionnées de R14 à 2018 |
| DXF | Lecture et écriture ; cibles d’enregistrement versionnées de R14 à 2018 |
| BAK / SV$ | Ouverture des sauvegardes et fichiers d’enregistrement automatique |
| OBJ | Importation de maillages polygonaux |
| LandXML | Importation de points topographiques `CgPoint` |
| STL | Exportation de données de maillage 3D |
| STEP AP203 | Exportation de données de maillage 3D |
| PDF | Tracé des présentations et de la géométrie sélectionnée sur ordinateur |
| CSV | Extraction des propriétés des entités |
| CTB / STB | Chargement et modification des tables de styles de tracé |

## Bureau ou web

Utilisez l’[application web](https://www.opencadstudio.com) pour un accès immédiat sans installation. Les dessins sont sélectionnés dans le navigateur et enregistrés sous forme de téléchargements locaux.

Utilisez l’application de bureau pour les associations de fichiers natives, les miniatures du gestionnaire de fichiers, l’impression système, la sortie PDF, les plugins externes, les scripts de commandes et l’automatisation sans interface. Des versions sont proposées pour Windows, Linux et macOS Apple Silicon.

## Installation

Téléchargez tous les paquets actuels depuis la [dernière version](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

Choisissez l’un des paquets x86-64 signés :

- `OpenCADStudio-*-windows-x86_64-installer.msi` — programme d’installation recommandé avec raccourcis du menu Démarrer, associations DWG/DXF et miniatures des dessins.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — application autonome, sans installation.

### Linux

Téléchargez l’AppImage x86-64, rendez-la exécutable et lancez-la :

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

Le paquet macOS publié prend en charge Apple Silicon :

1. Téléchargez `OpenCADStudio-*-macos-arm64.dmg`.
2. Ouvrez l’image et faites glisser `OpenCADStudio.app` vers **Applications**.
3. Si Gatekeeper bloque le premier lancement, autorisez l’application dans **System Settings → Privacy & Security**.

L’application est signée de façon ad hoc, mais n’est actuellement pas notariée par Apple.

## Langues

Open CAD Studio peut suivre la langue du système ou utiliser l’une de ces 20 langues d’interface :

> Arabe · Portugais du Brésil · Bulgare · Tchèque · Néerlandais · Anglais · Finnois · Français · Allemand · Hindi · Hongrois · Italien · Japonais · Coréen · Polonais · Russe · Chinois simplifié · Espagnol · Chinois traditionnel · Turc

Changez la langue dans les paramètres de l’application. La version web utilise aussi la langue préférée du navigateur lorsque **Système** est sélectionné.

## Compilation depuis les sources

### Bureau

Prérequis :

- Git
- Chaîne d’outils Rust stable actuelle
- Bibliothèques de développement graphiques et de polices de la plateforme

Sous Ubuntu ou Debian, installez les dépendances natives :

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Compilez ensuite :

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

Le binaire obtenu est écrit dans `target/release/OpenCADStudio` (`OpenCADStudio.exe` sous Windows).

### Web

Installez une fois la cible WebAssembly et les outils de compilation :

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Lancez le serveur de développement :

```bash
trunk serve
```

## Automatisation

Le binaire de bureau prend en charge la conversion ponctuelle et un serveur persistant sans interface :

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

Le serveur échange un objet JSON par ligne via l’entrée/sortie standard ou un socket TCP local. Consultez le [guide d’automatisation](../automation/README.md) et le [client Python](../automation/ocs.py) fourni.

## Plugins

Les plugins de bureau s’exécutent dans des processus séparés et communiquent avec l’hôte via l’API de plugins versionnée. La version navigateur ne charge pas de plugins natifs.

- [Architecture des plugins](../plugin-architecture.md)
- [Modèle de plugin](../plugin-template/README.md)
- [Registre des plugins](../../plugins/README.md)

## Documentation du projet

- [API d’automatisation](../automation/README.md)
- [Architecture des plugins](../plugin-architecture.md)
- [Pipeline de tessellation](../tessellation.md)
- [Politique de sécurité](../../SECURITY.md)

## Contribuer

Les rapports de bogues, pull requests ciblées, traductions, améliorations de documentation et contributions de plugins sont les bienvenus.

- Recherchez dans les [issues](https://github.com/HakanSeven12/OpenCADStudio/issues) existantes avant d’ouvrir un nouveau rapport.
- Utilisez les [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) pour les questions et les idées.
- Signalez les vulnérabilités en privé en suivant la [politique de sécurité](../../SECURITY.md).

## Croissance du projet

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Étoiles et téléchargements d’Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## Soutenir le projet

Si Open CAD Studio vous aide dans votre travail, soutenez son développement via [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) ou [Patreon](https://www.patreon.com/HakanSeven12).

## Licence

Open CAD Studio est distribué sous la [Licence publique générale GNU v3.0](../../LICENSE).
