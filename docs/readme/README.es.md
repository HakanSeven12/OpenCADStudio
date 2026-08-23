<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Logotipo de Open CAD Studio"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Dibujo 2D y modelado 3D de código abierto para escritorio y web, desarrollado con Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Última versión" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Descargas" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="Estrellas en GitHub" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="Licencia GPL-3.0" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Abrir la aplicación web</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Descargar la aplicación de escritorio</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Participar en la conversación</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Espacio de trabajo de Open CAD Studio" width="100%"></p>

## Descripción general

Open CAD Studio es una aplicación multiplataforma para dibujo técnico, trabajo con presentaciones y modelado de sólidos. Lee y escribe dibujos DWG y DXF de forma nativa, con un núcleo de edición compartido entre las versiones de escritorio y navegador.

El proyecto está en desarrollo activo. Conserva copias de seguridad de los dibujos de producción importantes e informa de problemas reproducibles mediante [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Características destacadas

- **Flujo de dibujo nativo** — abre, edita, recupera y guarda archivos DWG y DXF sin un servicio de conversión.
- **Dibujo 2D preciso** — líneas, polilíneas, curvas, splines, sombreados, referencias a objetos, rastreo, capas, bloques y referencias externas.
- **Herramientas de documentación** — texto, cotas, directrices, tolerancias, tablas, espacio modelo, espacio papel, ventanas gráficas y estilos de trazado.
- **Modelado 3D respaldado por kernel** — primitivas sólidas, extrusión, revolución, barrido, loft, operaciones booleanas y teselación de entidades ACIS.
- **Renderizado por GPU** — vistas 2D y 3D aceleradas mediante `wgpu`, con cámaras ortográfica y en perspectiva.
- **Flujos ampliables** — complementos nativos, scripts de comandos, conversión sin interfaz y una API de automatización JSON basada en líneas.

<p align="center"><img src="../../site/modeling.png" alt="Modelo 3D en Open CAD Studio" width="100%"></p>

## Flujos de archivos

| Formato o flujo | Compatibilidad |
| --- | --- |
| DWG | Lectura y escritura; destinos de guardado versionados de R14 a 2018 |
| DXF | Lectura y escritura; destinos de guardado versionados de R14 a 2018 |
| BAK / SV$ | Apertura de copias de seguridad y archivos de guardado automático |
| OBJ | Importación de mallas poligonales |
| LandXML | Importación de puntos topográficos `CgPoint` |
| STL | Exportación de datos de malla 3D |
| STEP AP203 | Exportación de datos de malla 3D |
| PDF | Trazado de presentaciones y geometría seleccionada en escritorio |
| CSV | Extracción de datos de propiedades de entidades |
| CTB / STB | Carga y edición de tablas de estilos de trazado |

## Escritorio o web

Usa la [aplicación web](https://www.opencadstudio.com) para acceder de inmediato sin instalar nada. Los dibujos se seleccionan mediante el navegador y se guardan como descargas locales.

Usa la aplicación de escritorio para asociaciones de archivos nativas, miniaturas del gestor de archivos, impresión del sistema, salida PDF, complementos externos, scripts de comandos y automatización sin interfaz. Hay versiones para Windows, Linux y macOS con Apple Silicon.

## Instalación

Descarga todos los paquetes actuales desde la [última versión](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

Elige uno de estos paquetes x86-64 firmados:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — instalador recomendado con accesos directos del menú Inicio, asociaciones DWG/DXF y miniaturas de dibujos.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — aplicación independiente; no requiere instalación.

### Linux

Descarga la AppImage x86-64, hazla ejecutable e iníciala:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

El paquete publicado para macOS es compatible con Apple Silicon:

1. Descarga `OpenCADStudio-*-macos-arm64.dmg`.
2. Abre la imagen y arrastra `OpenCADStudio.app` a **Applications**.
3. Si Gatekeeper bloquea el primer inicio, autoriza la aplicación en **System Settings → Privacy & Security**.

La aplicación tiene firma ad hoc, pero actualmente no está notarizada por Apple.

## Idiomas

Open CAD Studio puede seguir el idioma del sistema o usar cualquiera de estos 20 idiomas de interfaz:

> Árabe · Portugués de Brasil · Búlgaro · Checo · Neerlandés · Inglés · Finés · Francés · Alemán · Hindi · Húngaro · Italiano · Japonés · Coreano · Polaco · Ruso · Chino simplificado · Español · Chino tradicional · Turco

Cambia el idioma en los ajustes de la aplicación. La versión web también usa la configuración regional preferida del navegador cuando se selecciona **Sistema**.

## Compilar desde el código fuente

### Escritorio

Requisitos:

- Git
- Cadena de herramientas estable actual de Rust
- Bibliotecas de desarrollo de gráficos y fuentes de la plataforma

En Ubuntu o Debian, instala las dependencias nativas:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Después compila:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

El binario resultante se escribe en `target/release/OpenCADStudio` (`OpenCADStudio.exe` en Windows).

### Web

Instala una vez el destino WebAssembly y las herramientas de compilación:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Inicia el servidor de desarrollo:

```bash
trunk serve
```

## Automatización

El binario de escritorio admite conversión puntual y un servidor persistente sin interfaz:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

El servidor intercambia un objeto JSON por línea mediante entrada/salida estándar o un socket TCP local. Consulta la [guía de automatización](../automation/README.md) y el [cliente Python](../automation/ocs.py) incluido.

## Complementos

Los complementos de escritorio se ejecutan en procesos separados y se comunican con el anfitrión mediante la API de complementos versionada. La versión del navegador no carga complementos nativos.

- [Arquitectura de complementos](../plugin-architecture.md)
- [Plantilla de complemento](../plugin-template/README.md)
- [Registro de complementos](../../plugins/README.md)

## Documentación del proyecto

- [API de automatización](../automation/README.md)
- [Arquitectura de complementos](../plugin-architecture.md)
- [Canal de teselación](../tessellation.md)
- [Política de seguridad](../../SECURITY.md)

## Contribuir

Son bienvenidos los informes de errores, pull requests específicos, traducciones, mejoras de documentación y contribuciones de complementos.

- Busca en los [issues](https://github.com/HakanSeven12/OpenCADStudio/issues) existentes antes de abrir un informe nuevo.
- Usa [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) para preguntas e ideas.
- Informa de vulnerabilidades de forma privada siguiendo la [política de seguridad](../../SECURITY.md).

## Crecimiento del proyecto

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Estrellas y descargas de versiones de Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## Apoya el proyecto

Si Open CAD Studio te ayuda en tu trabajo, apoya su desarrollo mediante [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) o [Patreon](https://www.patreon.com/HakanSeven12).

## Licencia

Open CAD Studio se distribuye bajo la [Licencia Pública General de GNU v3.0](../../LICENSE).
