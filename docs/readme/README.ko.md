<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Open CAD Studio 로고"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Rust로 개발한 데스크톱 및 웹용 오픈 소스 2D 제도·3D 모델링 애플리케이션입니다.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="최신 릴리스" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="릴리스 다운로드" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="GitHub 스타" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="GPL-3.0 라이선스" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>웹 앱 실행</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>데스크톱 앱 다운로드</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>토론 참여</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Open CAD Studio 작업 공간" width="100%"></p>

## 개요

Open CAD Studio는 기술 도면 작성, 배치 작업, 솔리드 모델링을 위한 크로스 플랫폼 애플리케이션입니다. DWG 및 DXF 도면을 기본 형식으로 읽고 쓰며 데스크톱 버전과 브라우저 버전이 같은 편집 코어를 공유합니다.

이 프로젝트는 활발히 개발되고 있습니다. 중요한 실무 도면은 백업을 보관하고 재현 가능한 문제는 [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues)에 보고해 주세요.

## 주요 기능

- **네이티브 도면 작업 흐름** — 변환 서비스 없이 DWG 및 DXF 파일을 열고, 편집하고, 복구하고, 저장합니다.
- **정밀한 2D 제도** — 선, 폴리라인, 곡선, 스플라인, 해치, 객체 스냅, 추적, 레이어, 블록, 외부 참조를 지원합니다.
- **문서화 도구** — 문자, 치수, 지시선, 공차, 표, 모델 공간, 도면 공간, 뷰포트, 플롯 스타일을 제공합니다.
- **커널 기반 3D 모델링** — 솔리드 기본 형상, 돌출, 회전, 스윕, 로프트, 불리언 연산, ACIS 엔티티 테셀레이션을 지원합니다.
- **GPU 렌더링** — `wgpu`로 가속된 2D 및 3D 뷰포트와 직교·원근 카메라를 제공합니다.
- **확장 가능한 작업 흐름** — 네이티브 플러그인, 명령 스크립트, 헤드리스 변환, 줄 단위 JSON 자동화 API를 지원합니다.

<p align="center"><img src="../../site/modeling.png" alt="Open CAD Studio의 3D 모델" width="100%"></p>

## 파일 작업 흐름

| 형식 또는 작업 흐름 | 지원 내용 |
| --- | --- |
| DWG | 읽기 및 쓰기, R14부터 2018까지 버전을 지정해 저장 |
| DXF | 읽기 및 쓰기, R14부터 2018까지 버전을 지정해 저장 |
| BAK / SV$ | 도면 백업 및 자동 저장 파일 열기 |
| OBJ | 폴리곤 메시 가져오기 |
| LandXML | `CgPoint` 측량점 가져오기 |
| STL | 3D 메시 데이터 내보내기 |
| STEP AP203 | 3D 메시 데이터 내보내기 |
| PDF | 데스크톱에서 배치와 선택한 형상 플롯 |
| CSV | 엔티티 속성 데이터 추출 |
| CTB / STB | 플롯 스타일 테이블 불러오기 및 편집 |

## 데스크톱 또는 웹

설치 없이 바로 사용하려면 [웹 앱](https://www.opencadstudio.com)을 이용하세요. 도면은 브라우저에서 선택하고 로컬 다운로드로 저장합니다.

네이티브 파일 연결, 파일 관리자 미리보기, 시스템 인쇄, PDF 출력, 외부 플러그인, 명령 스크립트, 헤드리스 자동화에는 데스크톱 애플리케이션을 사용하세요. Windows, Linux, Apple Silicon macOS용 릴리스가 제공됩니다.

## 설치

현재 제공되는 모든 패키지는 [최신 릴리스](https://github.com/HakanSeven12/OpenCADStudio/releases/latest)에서 다운로드할 수 있습니다.

### Windows

서명된 x86-64 패키지 중 하나를 선택하세요.

- `OpenCADStudio-*-windows-x86_64-installer.msi` — 시작 메뉴 바로 가기, DWG/DXF 파일 연결, 도면 미리보기가 포함된 권장 설치 프로그램입니다.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — 설치가 필요 없는 독립 실행형 애플리케이션입니다.

### Linux

x86-64 AppImage를 다운로드하고 실행 권한을 부여한 뒤 실행하세요.

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

배포되는 macOS 패키지는 Apple Silicon을 지원합니다.

1. `OpenCADStudio-*-macos-arm64.dmg`를 다운로드합니다.
2. 이미지를 열고 `OpenCADStudio.app`을 **Applications**로 드래그합니다.
3. Gatekeeper가 첫 실행을 차단하면 **System Settings → Privacy & Security**에서 앱을 허용합니다.

애플리케이션은 임시 서명되어 있지만 현재 Apple 공증을 받지 않았습니다.

## 언어

Open CAD Studio는 시스템 언어를 따르거나 다음 19개 인터페이스 언어 중 하나를 사용할 수 있습니다.

> 아랍어 · 브라질 포르투갈어 · 불가리아어 · 체코어 · 네덜란드어 · 영어 · 핀란드어 · 프랑스어 · 독일어 · 힌디어 · 헝가리어 · 이탈리아어 · 일본어 · 한국어 · 폴란드어 · 러시아어 · 중국어 간체 · 스페인어 · 중국어 번체 · 터키어

애플리케이션 설정에서 언어를 변경할 수 있습니다. **시스템**을 선택하면 브라우저 버전도 브라우저의 기본 로캘을 사용합니다.

## 소스에서 빌드

### 데스크톱

요구 사항:

- Git
- 최신 안정 Rust 툴체인
- 플랫폼별 그래픽 및 글꼴 개발 라이브러리

Ubuntu 또는 Debian에서는 네이티브 종속성을 설치하세요.

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

그런 다음 빌드합니다.

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

생성된 바이너리는 `target/release/OpenCADStudio`에 저장됩니다(Windows에서는 `OpenCADStudio.exe`).

### 웹

WebAssembly 대상과 빌드 도구를 한 번 설치하세요.

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

개발 서버를 시작합니다.

```bash
trunk serve
```

## 자동화

데스크톱 바이너리는 일회성 변환과 지속형 헤드리스 서버를 지원합니다.

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

서버는 표준 입출력 또는 로컬 TCP 소켓을 통해 한 줄에 하나의 JSON 객체를 교환합니다. [자동화 안내서](../automation/README.md)와 포함된 [Python 클라이언트](../automation/ocs.py)를 참고하세요.

## 플러그인

데스크톱 플러그인은 별도 프로세스에서 실행되며 버전이 지정된 플러그인 API를 통해 호스트와 통신합니다. 브라우저 빌드는 네이티브 플러그인을 불러오지 않습니다.

- [플러그인 아키텍처](../plugin-architecture.md)
- [플러그인 템플릿](../plugin-template/README.md)
- [플러그인 레지스트리](../../plugins/README.md)

## 프로젝트 문서

- [자동화 API](../automation/README.md)
- [플러그인 아키텍처](../plugin-architecture.md)
- [테셀레이션 파이프라인](../tessellation.md)
- [보안 정책](../../SECURITY.md)

## 기여하기

버그 보고, 목적이 분명한 pull request, 번역, 문서 개선, 플러그인 기여를 환영합니다.

- 새 보고서를 열기 전에 기존 [issues](https://github.com/HakanSeven12/OpenCADStudio/issues)를 검색하세요.
- 질문과 아이디어는 [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions)를 이용하세요.
- 취약점은 [보안 정책](../../SECURITY.md)에 따라 비공개로 보고하세요.

## 프로젝트 성장

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Open CAD Studio 스타 및 릴리스 다운로드" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## 프로젝트 후원

Open CAD Studio가 업무에 도움이 된다면 [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) 또는 [Patreon](https://www.patreon.com/HakanSeven12)을 통해 지속적인 개발을 후원해 주세요.

## 라이선스

Open CAD Studio는 [GNU General Public License v3.0](../../LICENSE)에 따라 배포됩니다.
