<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Open CAD Studio 标志"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">使用 Rust 构建、面向桌面端和网页端的开源二维绘图与三维建模应用。</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="版本下载量" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="GitHub 星标" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="GPL-3.0 许可证" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>启动网页应用</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>下载桌面应用</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>参与讨论</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Open CAD Studio 工作区" width="100%"></p>

## 概览

Open CAD Studio 是一款用于技术绘图、布局设计和实体建模的跨平台应用。它可原生读取和写入 DWG 与 DXF 图纸，桌面版和浏览器版共享同一套编辑核心。

本项目正在积极开发中。请保留重要生产图纸的备份，并通过 [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues) 报告可重现的问题。

## 主要功能

- **原生图纸工作流** — 无需转换服务即可打开、编辑、恢复和保存 DWG/DXF 文件。
- **精确二维绘图** — 支持直线、多段线、曲线、样条曲线、填充、对象捕捉、追踪、图层、块和外部参照。
- **文档工具** — 支持文字、标注、引线、公差、表格、模型空间、图纸空间、视口和打印样式。
- **几何内核支持的三维建模** — 支持实体基本体、拉伸、旋转、扫掠、放样、布尔运算以及 ACIS 实体细分。
- **GPU 渲染** — 通过 `wgpu` 加速二维和三维视口，并支持正交与透视相机。
- **可扩展工作流** — 支持原生插件、命令脚本、无界面转换和逐行 JSON 自动化 API。

<p align="center"><img src="../../site/modeling.png" alt="Open CAD Studio 中的三维模型" width="100%"></p>

## 文件工作流

| 格式或工作流 | 支持情况 |
| --- | --- |
| DWG | 读取和写入；可选择 R14 至 2018 的保存版本 |
| DXF | 读取和写入；可选择 R14 至 2018 的保存版本 |
| BAK / SV$ | 打开图纸备份和自动保存文件 |
| OBJ | 导入多边形网格 |
| LandXML | 导入 `CgPoint` 测量点 |
| STL | 导出三维网格数据 |
| STEP AP203 | 导出三维网格数据 |
| PDF | 在桌面版中输出布局和选定几何图形 |
| CSV | 提取实体属性数据 |
| CTB / STB | 加载和编辑打印样式表 |

## 桌面版或网页版

使用[网页应用](https://www.opencadstudio.com)可无需安装立即开始。图纸通过浏览器选择，并以本地下载文件保存。

需要原生文件关联、文件管理器缩略图、系统打印、PDF 输出、外部插件、命令脚本和无界面自动化时，请使用桌面应用。项目提供 Windows、Linux 和 Apple Silicon macOS 版本。

## 安装

请从[最新版本](https://github.com/HakanSeven12/OpenCADStudio/releases/latest)下载所有当前软件包。

### Windows

请选择一个已签名的 x86-64 软件包：

- `OpenCADStudio-*-windows-x86_64-installer.msi` — 推荐的安装程序，包含开始菜单快捷方式、DWG/DXF 文件关联和图纸缩略图。
- `OpenCADStudio-*-windows-x86_64-portable.exe` — 无需安装的独立应用程序。

### Linux

下载 x86-64 AppImage，添加执行权限并运行：

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

发布的 macOS 软件包支持 Apple Silicon：

1. 下载 `OpenCADStudio-*-macos-arm64.dmg`。
2. 打开镜像，将 `OpenCADStudio.app` 拖入 **Applications**。
3. 如果 Gatekeeper 阻止首次启动，请在 **System Settings → Privacy & Security** 中允许该应用。

应用采用临时签名，但目前尚未通过 Apple 公证。

## 语言

Open CAD Studio 可以跟随系统语言，也可以使用以下 20 种界面语言：

> 阿拉伯语 · 巴西葡萄牙语 · 保加利亚语 · 捷克语 · 荷兰语 · 英语 · 芬兰语 · 法语 · 德语 · 印地语 · 匈牙利语 · 意大利语 · 日语 · 韩语 · 波兰语 · 俄语 · 简体中文 · 西班牙语 · 繁体中文 · 土耳其语

可在应用设置中更改语言。选择**系统**时，浏览器版也会使用浏览器的首选区域设置。

## 从源代码构建

### 桌面版

要求：

- Git
- 当前稳定版 Rust 工具链
- 对应平台的图形和字体开发库

在 Ubuntu 或 Debian 上安装原生依赖：

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

然后构建：

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

生成的二进制文件位于 `target/release/OpenCADStudio`（Windows 上为 `OpenCADStudio.exe`）。

### 网页版

一次性安装 WebAssembly 目标和构建工具：

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

启动开发服务器：

```bash
trunk serve
```

## 自动化

桌面二进制程序支持一次性转换和持续运行的无界面服务器：

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

服务器通过标准输入/输出或本地 TCP 套接字逐行交换 JSON 对象。请参阅[自动化指南](../automation/README.md)和随附的 [Python 客户端](../automation/ocs.py)。

## 插件

桌面插件运行于独立进程中，并通过有版本管理的插件 API 与主程序通信。浏览器版本不会加载原生插件。

- [插件架构](../plugin-architecture.md)
- [插件模板](../plugin-template/README.md)
- [插件注册表](../../plugins/README.md)

## 项目文档

- [自动化 API](../automation/README.md)
- [插件架构](../plugin-architecture.md)
- [细分处理流程](../tessellation.md)
- [安全策略](../../SECURITY.md)

## 参与贡献

欢迎提交错误报告、目标明确的 pull request、翻译、文档改进和插件贡献。

- 提交新报告前请搜索现有 [issues](https://github.com/HakanSeven12/OpenCADStudio/issues)。
- 问题和想法请使用 [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions)。
- 请按照[安全策略](../../SECURITY.md)私下报告漏洞。

## 项目成长

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Open CAD Studio 星标和版本下载量" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## 支持项目

如果 Open CAD Studio 对您的工作有所帮助，请通过 [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) 或 [Patreon](https://www.patreon.com/HakanSeven12) 支持持续开发。

## 许可证

Open CAD Studio 根据 [GNU General Public License v3.0](../../LICENSE) 分发。
