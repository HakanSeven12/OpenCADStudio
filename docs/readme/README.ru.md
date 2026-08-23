<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="Логотип Open CAD Studio"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center">Открытое приложение для 2D-черчения и 3D-моделирования на компьютере и в браузере, созданное на Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="Последняя версия" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="Загрузки" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="Звёзды GitHub" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="Лицензия GPL-3.0" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center">
  <a href="https://www.opencadstudio.com"><strong>Запустить веб-приложение</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>Скачать приложение для компьютера</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>Присоединиться к обсуждению</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="Рабочее пространство Open CAD Studio" width="100%"></p>

## Обзор

Open CAD Studio — кроссплатформенное приложение для технического черчения, работы с листами и моделирования тел. Оно напрямую читает и записывает чертежи DWG и DXF; версии для компьютера и браузера используют общее ядро редактирования.

Проект активно развивается. Храните резервные копии важных рабочих чертежей и сообщайте о воспроизводимых проблемах через [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## Основные возможности

- **Прямая работа с чертежами** — открытие, редактирование, восстановление и сохранение DWG и DXF без службы преобразования.
- **Точное 2D-черчение** — линии, полилинии, кривые, сплайны, штриховки, объектные привязки, отслеживание, слои, блоки и внешние ссылки.
- **Средства оформления** — текст, размеры, выноски, допуски, таблицы, пространство модели, пространство листа, видовые экраны и стили печати.
- **3D-моделирование на геометрическом ядре** — примитивы тел, выдавливание, вращение, заметание, лофт, булевы операции и тесселяция объектов ACIS.
- **Отрисовка на GPU** — ускоренные 2D- и 3D-виды через `wgpu`, ортографическая и перспективная камеры.
- **Расширяемые процессы** — нативные плагины, сценарии команд, преобразование без интерфейса и построчный JSON API автоматизации.

<p align="center"><img src="../../site/modeling.png" alt="3D-модель в Open CAD Studio" width="100%"></p>

## Работа с файлами

| Формат или процесс | Поддержка |
| --- | --- |
| DWG | Чтение и запись; сохранение в версиях от R14 до 2018 |
| DXF | Чтение и запись; сохранение в версиях от R14 до 2018 |
| BAK / SV$ | Открытие резервных копий и файлов автосохранения |
| OBJ | Импорт полигональных сеток |
| LandXML | Импорт точек съёмки `CgPoint` |
| STL | Экспорт данных 3D-сетки |
| STEP AP203 | Экспорт данных 3D-сетки |
| PDF | Печать листов и выбранной геометрии в версии для компьютера |
| CSV | Извлечение данных свойств объектов |
| CTB / STB | Загрузка и редактирование таблиц стилей печати |

## Компьютер или браузер

Используйте [веб-приложение](https://www.opencadstudio.com), чтобы начать без установки. Чертежи выбираются в браузере и сохраняются как локальные загрузки.

Используйте приложение для компьютера, если нужны системные ассоциации файлов, миниатюры в файловом менеджере, системная печать, вывод PDF, внешние плагины, сценарии команд и автоматизация без интерфейса. Выпуски доступны для Windows, Linux и macOS на Apple Silicon.

## Установка

Все актуальные пакеты доступны в [последнем выпуске](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

Выберите один из подписанных пакетов x86-64:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — рекомендуемый установщик с ярлыками в меню «Пуск», ассоциациями DWG/DXF и миниатюрами чертежей.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — автономное приложение, не требующее установки.

### Linux

Скачайте x86-64 AppImage, сделайте его исполняемым и запустите:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

Опубликованный пакет macOS поддерживает Apple Silicon:

1. Скачайте `OpenCADStudio-*-macos-arm64.dmg`.
2. Откройте образ и перетащите `OpenCADStudio.app` в **Applications**.
3. Если Gatekeeper блокирует первый запуск, разрешите приложение в **System Settings → Privacy & Security**.

Приложение имеет специальную подпись, но в настоящее время не нотарифицировано Apple.

## Языки

Open CAD Studio может следовать языку системы или использовать один из 20 языков интерфейса:

> Арабский · Бразильский португальский · Болгарский · Чешский · Нидерландский · Английский · Финский · Французский · Немецкий · Хинди · Венгерский · Итальянский · Японский · Корейский · Польский · Русский · Упрощённый китайский · Испанский · Традиционный китайский · Турецкий

Язык меняется в настройках приложения. При выборе **Система** версия для браузера также использует предпочтительную локаль браузера.

## Сборка из исходного кода

### Приложение для компьютера

Требования:

- Git
- Текущая стабильная цепочка инструментов Rust
- Библиотеки разработки графики и шрифтов для платформы

В Ubuntu или Debian установите нативные зависимости:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

Затем выполните сборку:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

Готовый файл находится в `target/release/OpenCADStudio` (`OpenCADStudio.exe` в Windows).

### Web

Один раз установите цель WebAssembly и инструменты сборки:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

Запустите сервер разработки:

```bash
trunk serve
```

## Автоматизация

Программа для компьютера поддерживает однократное преобразование и постоянный сервер без интерфейса:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

Сервер обменивается одним объектом JSON в строке через стандартный ввод/вывод или локальный TCP-сокет. См. [руководство по автоматизации](../automation/README.md) и включённый [клиент Python](../automation/ocs.py).

## Плагины

Плагины для компьютера работают в отдельных процессах и взаимодействуют с основной программой через версионированный API. Браузерная сборка не загружает нативные плагины.

- [Архитектура плагинов](../plugin-architecture.md)
- [Шаблон плагина](../plugin-template/README.md)
- [Реестр плагинов](../../plugins/README.md)

## Документация проекта

- [API автоматизации](../automation/README.md)
- [Архитектура плагинов](../plugin-architecture.md)
- [Конвейер тесселяции](../tessellation.md)
- [Политика безопасности](../../SECURITY.md)

## Участие в проекте

Приветствуются сообщения об ошибках, целевые pull request, переводы, улучшения документации и вклад в плагины.

- Перед созданием нового сообщения выполните поиск по существующим [issues](https://github.com/HakanSeven12/OpenCADStudio/issues).
- Для вопросов и идей используйте [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions).
- Сообщайте об уязвимостях конфиденциально в соответствии с [политикой безопасности](../../SECURITY.md).

## Рост проекта

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="Звёзды и загрузки выпусков Open CAD Studio" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## Поддержать проект

Если Open CAD Studio помогает в вашей работе, поддержите дальнейшую разработку через [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) или [Patreon](https://www.patreon.com/HakanSeven12).

## Лицензия

Open CAD Studio распространяется по [GNU General Public License v3.0](../../LICENSE).
