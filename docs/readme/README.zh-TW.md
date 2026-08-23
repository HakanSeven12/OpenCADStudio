<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Open CAD Studio 標誌"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">使用 Rust 建構、適用於桌面與網頁的開放原始碼二維繪圖和三維建模應用程式。</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="最新版本" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="版本下載量" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="GitHub 星號" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="GPL-3.0 授權" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>啟動網頁應用程式</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>下載桌面應用程式</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>參與討論</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Open CAD Studio 工作區" width="100%"></p>

## 概覽

Open CAD Studio 是用於技術繪圖、配置作業和實體建模的跨平台應用程式。它能原生讀取與寫入 DWG 和 DXF 圖面，桌面版與瀏覽器版共用相同的編輯核心。

本專案正積極開發中。請保留重要生產圖面的備份，並透過 [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues) 回報可重現的問題。

## 主要功能

- **原生圖面工作流程** — 不需轉換服務即可開啟、編輯、復原及儲存 DWG/DXF 檔案。
- **精確二維繪圖** — 支援直線、聚合線、曲線、雲形線、填充線、物件鎖點、追蹤、圖層、圖塊和外部參考。
- **文件工具** — 支援文字、標註、引線、公差、表格、模型空間、圖紙空間、視埠和出圖型式。
- **幾何核心支援的三維建模** — 支援實體基本形、擠出、旋轉、掃掠、疊層、布林運算和 ACIS 圖元細分。
- **GPU 轉譯** — 透過 `wgpu` 加速二維和三維視埠，並支援正投影與透視相機。
- **可擴充工作流程** — 支援原生外掛程式、指令碼、無介面轉換和逐行 JSON 自動化 API。

<p align="center"><img src="../../site/modeling.png" alt="Open CAD Studio 中的三維模型" width="100%"></p>

## 檔案工作流程

| 格式或工作流程 | 支援內容 |
| --- | --- |
| DWG | 讀取與寫入；可選擇 R14 至 2018 的儲存版本 |
| DXF | 讀取與寫入；可選擇 R14 至 2018 的儲存版本 |
| BAK / SV$ | 開啟圖面備份與自動儲存檔案 |
| OBJ | 匯入多邊形網格 |
| LandXML | 匯入 `CgPoint` 測量點 |
| STL | 匯出三維網格資料 |
| STEP AP203 | 匯出三維網格資料 |
| PDF | 在桌面版中出圖配置和選取的幾何圖形 |
| CSV | 擷取圖元屬性資料 |
| CTB / STB | 載入與編輯出圖型式表 |

## 桌面版或網頁版

使用[網頁應用程式](https://www.opencadstudio.com)即可不需安裝立即開始。圖面透過瀏覽器選取，並儲存為本機下載檔案。

需要原生檔案關聯、檔案管理員縮圖、系統列印、PDF 輸出、外部外掛程式、指令碼和無介面自動化時，請使用桌面應用程式。專案提供 Windows、Linux 和 Apple Silicon macOS 版本。

## 安裝

請從[最新版本](https://github.com/HakanSeven12/OpenCADStudio/releases/latest)下載所有目前的套件。

### Windows

請選擇一個已簽署的 x86-64 套件：

- `OpenCADStudio-*-windows-x86_64-installer.msi` — 建議的安裝程式，包含開始功能表捷徑、DWG/DXF 檔案關聯和圖面縮圖。
- `OpenCADStudio-*-windows-x86_64-portable.exe` — 不需安裝的獨立應用程式。

### Linux

下載 x86-64 AppImage，加入執行權限並執行：

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

發佈的 macOS 套件支援 Apple Silicon：

1. 下載 `OpenCADStudio-*-macos-arm64.dmg`。
2. 開啟映像檔，將 `OpenCADStudio.app` 拖入 **Applications**。
3. 如果 Gatekeeper 阻止首次啟動，請在 **System Settings → Privacy & Security** 中允許此應用程式。

應用程式採用臨時簽章，但目前尚未通過 Apple 公證。

## 語言

Open CAD Studio 可以跟隨系統語言，也可以使用以下 20 種介面語言：

> 阿拉伯文 · 巴西葡萄牙文 · 保加利亞文 · 捷克文 · 荷蘭文 · 英文 · 芬蘭文 · 法文 · 德文 · 北印度文 · 匈牙利文 · 義大利文 · 日文 · 韓文 · 波蘭文 · 俄文 · 簡體中文 · 西班牙文 · 繁體中文 · 土耳其文

可在應用程式設定中變更語言。選擇**系統**時，瀏覽器版也會使用瀏覽器的偏好地區設定。

## 從原始碼建構

### 桌面版

需求：

- Git
- 目前穩定版 Rust 工具鏈
- 對應平台的圖形與字型開發程式庫

在 Ubuntu 或 Debian 上安裝原生相依套件：

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

接著建構：

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

產生的二進位檔位於 `target/release/OpenCADStudio`（Windows 上為 `OpenCADStudio.exe`）。

### 網頁版

一次安裝 WebAssembly 目標和建構工具：

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

啟動開發伺服器：

```bash
trunk serve
```

## 自動化

桌面二進位程式支援單次轉換和持續執行的無介面伺服器：

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

伺服器透過標準輸入/輸出或本機 TCP 通訊端逐行交換 JSON 物件。請參閱[自動化指南](../automation/README.md)和隨附的 [Python 用戶端](../automation/ocs.py)。

## 外掛程式

桌面外掛程式在獨立處理程序中執行，並透過有版本管理的外掛程式 API 與主程式通訊。瀏覽器版本不會載入原生外掛程式。

- [外掛程式架構](../plugin-architecture.md)
- [外掛程式範本](../plugin-template/README.md)
- [外掛程式登錄檔](../../plugins/README.md)

## 專案文件

- [自動化 API](../automation/README.md)
- [外掛程式架構](../plugin-architecture.md)
- [細分處理流程](../tessellation.md)
- [安全性政策](../../SECURITY.md)

## 參與貢獻

歡迎提交錯誤報告、目標明確的 pull request、翻譯、文件改進和外掛程式貢獻。

- 建立新報告前請搜尋現有 [issues](https://github.com/HakanSeven12/OpenCADStudio/issues)。
- 問題和想法請使用 [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions)。
- 請依照[安全性政策](../../SECURITY.md)私下回報弱點。

## 專案成長

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Open CAD Studio 星號與版本下載量" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## 支持專案

如果 Open CAD Studio 對您的工作有幫助，請透過 [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) 或 [Patreon](https://www.patreon.com/HakanSeven12) 支持持續開發。

## 授權

Open CAD Studio 依 [GNU General Public License v3.0](../../LICENSE) 散佈。
