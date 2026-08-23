<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Open CAD Studio ロゴ"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Rust で開発された、デスクトップと Web 向けのオープンソース 2D 製図・3D モデリングアプリケーション。</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="最新リリース" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="ダウンロード数" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="GitHub スター" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="GPL-3.0 ライセンス" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Web アプリを起動</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>デスクトップアプリをダウンロード</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>ディスカッションに参加</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Open CAD Studio ワークスペース" width="100%"></p>

## 概要

Open CAD Studio は、技術製図、レイアウト作業、ソリッドモデリングのためのクロスプラットフォームアプリケーションです。DWG および DXF 図面をネイティブに読み書きし、デスクトップ版とブラウザ版で共通の編集コアを使用します。

本プロジェクトは活発に開発されています。重要な実務図面はバックアップを保存し、再現可能な問題は [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues) で報告してください。

## 主な特長

- **ネイティブ図面ワークフロー** — 変換サービスを使わず、DWG/DXF ファイルを開く、編集する、復旧する、保存することができます。
- **高精度な 2D 製図** — 線分、ポリライン、曲線、スプライン、ハッチング、オブジェクトスナップ、トラッキング、レイヤー、ブロック、外部参照。
- **ドキュメント作成ツール** — テキスト、寸法、引出線、公差、表、モデル空間、ペーパー空間、ビューポート、印刷スタイル。
- **カーネルベースの 3D モデリング** — ソリッドプリミティブ、押し出し、回転、スイープ、ロフト、ブーリアン演算、ACIS エンティティのテッセレーション。
- **GPU レンダリング** — `wgpu` による高速な 2D/3D ビューポートと、正投影・透視投影カメラ。
- **拡張可能なワークフロー** — ネイティブプラグイン、コマンドスクリプト、ヘッドレス変換、行単位の JSON 自動化 API。

<p align="center"><img src="../../site/modeling.png" alt="Open CAD Studio の 3D モデル" width="100%"></p>

## ファイルワークフロー

| 形式またはワークフロー | 対応内容 |
| --- | --- |
| DWG | 読み書き。R14 から 2018 までのバージョンを指定して保存 |
| DXF | 読み書き。R14 から 2018 までのバージョンを指定して保存 |
| BAK / SV$ | 図面バックアップと自動保存ファイルを開く |
| OBJ | ポリゴンメッシュをインポート |
| LandXML | `CgPoint` 測量点をインポート |
| STL | 3D メッシュデータをエクスポート |
| STEP AP203 | 3D メッシュデータをエクスポート |
| PDF | デスクトップ版でレイアウトと選択ジオメトリを出力 |
| CSV | エンティティのプロパティデータを抽出 |
| CTB / STB | 印刷スタイルテーブルを読み込み、編集 |

## デスクトップ版と Web 版

インストールせずにすぐ使うには [Web アプリ](https://www.opencadstudio.com) を利用してください。図面はブラウザで選択し、ローカルダウンロードとして保存されます。

ネイティブなファイル関連付け、ファイルマネージャーのサムネイル、システム印刷、PDF 出力、外部プラグイン、コマンドスクリプト、ヘッドレス自動化にはデスクトップ版を使用してください。Windows、Linux、Apple Silicon macOS 向けのリリースがあります。

## インストール

現在のすべてのパッケージは [最新リリース](https://github.com/HakanSeven12/OpenCADStudio/releases/latest) からダウンロードできます。

### Windows

署名済み x86-64 パッケージのいずれかを選択します。

- `OpenCADStudio-*-windows-x86_64-installer.msi` — スタートメニューのショートカット、DWG/DXF 関連付け、図面サムネイルを含む推奨インストーラー。
- `OpenCADStudio-*-windows-x86_64-portable.exe` — インストール不要のスタンドアロンアプリケーション。

### Linux

x86-64 AppImage をダウンロードし、実行可能にして起動します。

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

公開されている macOS パッケージは Apple Silicon に対応しています。

1. `OpenCADStudio-*-macos-arm64.dmg` をダウンロードします。
2. イメージを開き、`OpenCADStudio.app` を **Applications** にドラッグします。
3. Gatekeeper が初回起動をブロックした場合は、**System Settings → Privacy & Security** でアプリを許可します。

アプリケーションは ad hoc 署名されていますが、現在 Apple の公証は受けていません。

## 言語

Open CAD Studio はシステム言語に従うか、次の 20 のインターフェース言語を使用できます。

> アラビア語 · ブラジルポルトガル語 · ブルガリア語 · チェコ語 · オランダ語 · 英語 · フィンランド語 · フランス語 · ドイツ語 · ヒンディー語 · ハンガリー語 · イタリア語 · 日本語 · 韓国語 · ポーランド語 · ロシア語 · 簡体字中国語 · スペイン語 · 繁体字中国語 · トルコ語

言語はアプリケーション設定から変更できます。**システム** を選ぶと、ブラウザ版もブラウザの優先ロケールを使用します。

## ソースからビルド

### デスクトップ

必要なもの:

- Git
- 現在の安定版 Rust ツールチェーン
- 各プラットフォームのグラフィックスおよびフォント開発ライブラリ

Ubuntu または Debian では、ネイティブ依存関係をインストールします。

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

続いてビルドします。

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

生成されたバイナリは `target/release/OpenCADStudio` に保存されます（Windows では `OpenCADStudio.exe`）。

### Web

WebAssembly ターゲットとビルドツールを一度インストールします。

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

開発サーバーを起動します。

```bash
trunk serve
```

## 自動化

デスクトップバイナリは、1 回限りの変換と常駐ヘッドレスサーバーに対応しています。

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

サーバーは標準入出力またはローカル TCP ソケットを通じ、1 行につき 1 個の JSON オブジェクトを交換します。[自動化ガイド](../automation/README.md) と付属の [Python クライアント](../automation/ocs.py) を参照してください。

## プラグイン

デスクトッププラグインは別プロセスで動作し、バージョン管理されたプラグイン API を介してホストと通信します。ブラウザ版はネイティブプラグインを読み込みません。

- [プラグインアーキテクチャ](../plugin-architecture.md)
- [プラグインテンプレート](../plugin-template/README.md)
- [プラグインレジストリ](../../plugins/README.md)

## プロジェクト文書

- [自動化 API](../automation/README.md)
- [プラグインアーキテクチャ](../plugin-architecture.md)
- [テッセレーションパイプライン](../tessellation.md)
- [セキュリティポリシー](../../SECURITY.md)

## コントリビューション

バグ報告、目的を絞った pull request、翻訳、文書の改善、プラグインへの貢献を歓迎します。

- 新しい報告を作成する前に既存の [issues](https://github.com/HakanSeven12/OpenCADStudio/issues) を検索してください。
- 質問やアイデアには [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) を利用してください。
- 脆弱性は [セキュリティポリシー](../../SECURITY.md) に従って非公開で報告してください。

## プロジェクトの成長

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Open CAD Studio のスター数とリリースダウンロード数" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## プロジェクトを支援

Open CAD Studio が作業に役立つ場合は、[GitHub Sponsors](https://github.com/sponsors/HakanSeven12) または [Patreon](https://www.patreon.com/HakanSeven12) で継続的な開発をご支援ください。

## ライセンス

Open CAD Studio は [GNU General Public License v3.0](../../LICENSE) の下で配布されています。
