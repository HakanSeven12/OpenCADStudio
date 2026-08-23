<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Logotipo do Open CAD Studio"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Desenho 2D e modelagem 3D de código aberto para desktop e web, desenvolvido em Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Versão mais recente" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Downloads das versões" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="Estrelas no GitHub" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="Licença GPL-3.0" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Abrir o aplicativo web</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Baixar o aplicativo desktop</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Participar da discussão</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Área de trabalho do Open CAD Studio" width="100%"></p>

## Visão geral

Open CAD Studio é um aplicativo multiplataforma para desenho técnico, trabalho com layouts e modelagem de sólidos. Ele lê e grava desenhos DWG e DXF nativamente, com um núcleo de edição compartilhado entre as versões desktop e web.

O projeto está em desenvolvimento ativo. Mantenha cópias de segurança de desenhos de produção importantes e relate problemas reproduzíveis pelo [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Destaques

- **Fluxo de desenho nativo** — abra, edite, recupere e salve arquivos DWG e DXF sem serviço de conversão.
- **Desenho 2D preciso** — linhas, polilinhas, curvas, splines, hachuras, snaps a objetos, rastreamento, camadas, blocos e referências externas.
- **Ferramentas de documentação** — texto, cotas, chamadas, tolerâncias, tabelas, espaço do modelo, espaço do papel, viewports e estilos de plotagem.
- **Modelagem 3D com kernel** — primitivas sólidas, extrusão, revolução, varredura, loft, operações booleanas e tesselação de entidades ACIS.
- **Renderização por GPU** — viewports 2D e 3D aceleradas por `wgpu`, com câmeras ortográfica e em perspectiva.
- **Fluxos extensíveis** — plugins nativos, scripts de comandos, conversão sem interface e API de automação JSON baseada em linhas.

<p align="center"><img src="../../site/modeling.png" alt="Modelo 3D no Open CAD Studio" width="100%"></p>

## Fluxos de arquivos

| Formato ou fluxo | Suporte |
| --- | --- |
| DWG | Leitura e gravação; destinos de salvamento versionados de R14 a 2018 |
| DXF | Leitura e gravação; destinos de salvamento versionados de R14 a 2018 |
| BAK / SV$ | Abertura de backups e arquivos de salvamento automático |
| OBJ | Importação de malhas poligonais |
| LandXML | Importação de pontos topográficos `CgPoint` |
| STL | Exportação de dados de malha 3D |
| STEP AP203 | Exportação de dados de malha 3D |
| PDF | Plotagem de layouts e geometria selecionada no desktop |
| CSV | Extração de dados de propriedades das entidades |
| CTB / STB | Carregamento e edição de tabelas de estilos de plotagem |

## Desktop ou web

Use o [aplicativo web](https://www.opencadstudio.com) para acesso imediato, sem instalação. Os desenhos são selecionados pelo navegador e salvos como downloads locais.

Use o aplicativo desktop para associações nativas de arquivos, miniaturas no gerenciador de arquivos, impressão do sistema, saída em PDF, plugins externos, scripts de comandos e automação sem interface. Há versões para Windows, Linux e macOS com Apple Silicon.

## Instalação

Baixe todos os pacotes atuais na [versão mais recente](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

Escolha um destes pacotes x86-64 assinados:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — instalador recomendado com atalhos no menu Iniciar, associações DWG/DXF e miniaturas de desenhos.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — aplicativo independente; não requer instalação.

### Linux

Baixe o AppImage x86-64, torne-o executável e execute:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

O pacote publicado para macOS é compatível com Apple Silicon:

1. Baixe `OpenCADStudio-*-macos-arm64.dmg`.
2. Abra a imagem e arraste `OpenCADStudio.app` para **Applications**.
3. Se o Gatekeeper bloquear a primeira abertura, autorize o aplicativo em **System Settings → Privacy & Security**.

O aplicativo possui assinatura ad hoc, mas atualmente não é notarizado pela Apple.

## Idiomas

Open CAD Studio pode seguir o idioma do sistema ou usar um destes 20 idiomas de interface:

> Árabe · Português do Brasil · Búlgaro · Tcheco · Holandês · Inglês · Finlandês · Francês · Alemão · Hindi · Húngaro · Italiano · Japonês · Coreano · Polonês · Russo · Chinês simplificado · Espanhol · Chinês tradicional · Turco

Altere o idioma nas configurações do aplicativo. A versão web também usa a localidade preferida do navegador quando **Sistema** está selecionado.

## Compilar a partir do código-fonte

### Desktop

Requisitos:

- Git
- Toolchain estável atual do Rust
- Bibliotecas de desenvolvimento gráfico e de fontes da plataforma

No Ubuntu ou Debian, instale as dependências nativas:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Depois compile:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

O binário resultante é gravado em `target/release/OpenCADStudio` (`OpenCADStudio.exe` no Windows).

### Web

Instale uma vez o alvo WebAssembly e as ferramentas de compilação:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Inicie o servidor de desenvolvimento:

```bash
trunk serve
```

## Automação

O binário desktop oferece conversão única e um servidor persistente sem interface:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

O servidor troca um objeto JSON por linha pela entrada/saída padrão ou por um socket TCP local. Consulte o [guia de automação](../automation/README.md) e o [cliente Python](../automation/ocs.py) incluído.

## Plugins

Plugins desktop são executados em processos separados e se comunicam com o host pela API de plugins versionada. A versão para navegador não carrega plugins nativos.

- [Arquitetura de plugins](../plugin-architecture.md)
- [Modelo de plugin](../plugin-template/README.md)
- [Registro de plugins](../../plugins/README.md)

## Documentação do projeto

- [API de automação](../automation/README.md)
- [Arquitetura de plugins](../plugin-architecture.md)
- [Pipeline de tesselação](../tessellation.md)
- [Política de segurança](../../SECURITY.md)

## Como contribuir

Relatos de erros, pull requests focados, traduções, melhorias na documentação e contribuições de plugins são bem-vindos.

- Pesquise as [issues](https://github.com/HakanSeven12/OpenCADStudio/issues) existentes antes de abrir um novo relato.
- Use [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) para perguntas e ideias.
- Relate vulnerabilidades em particular seguindo a [política de segurança](../../SECURITY.md).

## Crescimento do projeto

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Estrelas e downloads de versões do Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## Apoie o projeto

Se o Open CAD Studio ajuda no seu trabalho, apoie o desenvolvimento contínuo pelo [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) ou [Patreon](https://www.patreon.com/HakanSeven12).

## Licença

Open CAD Studio é distribuído sob a [Licença Pública Geral GNU v3.0](../../LICENSE).
