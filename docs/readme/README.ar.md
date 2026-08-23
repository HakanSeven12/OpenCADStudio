<p align="center">
  <a href="../../README.md">English</a> · <a href="README.bg.md">Български</a> · <a href="README.pt-BR.md">Português (Brasil)</a> · <a href="README.cs.md">Čeština</a> · <a href="README.nl.md">Nederlands</a> · <a href="README.fr.md">Français</a> · <a href="README.fi.md">Suomi</a> · <a href="README.de.md">Deutsch</a> · <a href="README.hu.md">Magyar</a> · <a href="README.it.md">Italiano</a> · <a href="README.ja.md">日本語</a> · <a href="README.ko.md">한국어</a> · <a href="README.pl.md">Polski</a> · <a href="README.ru.md">Русский</a> · <a href="README.zh-CN.md">简体中文</a> · <a href="README.es.md">Español</a> · <a href="README.zh-TW.md">繁體中文</a> · <a href="README.tr.md">Türkçe</a> · <a href="README.hi.md">हिन्दी</a> · <a href="README.ar.md">العربية</a>
</p>

<p align="center"><img src="../../assets/logo.svg" width="112" alt="شعار Open CAD Studio"></p>
<h1 align="center">Open CAD Studio</h1>
<p align="center" dir="rtl">تطبيق مفتوح المصدر للرسم ثنائي الأبعاد والنمذجة ثلاثية الأبعاد على سطح المكتب والويب، مبني بلغة Rust.</p>

<p align="center">
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><img alt="أحدث إصدار" src="https://img.shields.io/github/v/release/HakanSeven12/OpenCADStudio"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases"><img alt="تنزيلات الإصدارات" src="https://img.shields.io/github/downloads/HakanSeven12/OpenCADStudio/total"></a>
  <a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers"><img alt="نجوم GitHub" src="https://img.shields.io/github/stars/HakanSeven12/OpenCADStudio"></a>
  <a href="../../LICENSE"><img alt="ترخيص GPL-3.0" src="https://img.shields.io/github/license/HakanSeven12/OpenCADStudio"></a>
</p>

<p align="center" dir="rtl">
  <a href="https://www.opencadstudio.com"><strong>تشغيل تطبيق الويب</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/releases/latest"><strong>تنزيل تطبيق سطح المكتب</strong></a> ·
  <a href="https://github.com/HakanSeven12/OpenCADStudio/discussions"><strong>الانضمام إلى النقاش</strong></a>
</p>

<p align="center"><img src="../../site/workspace.png" alt="مساحة عمل Open CAD Studio" width="100%"></p>

## نظرة عامة

Open CAD Studio تطبيق متعدد المنصات للرسم التقني والعمل على التخطيطات ونمذجة المجسمات. يقرأ رسومات DWG وDXF ويكتبها مباشرة، ويشترك إصدار سطح المكتب وإصدار المتصفح في نواة التحرير نفسها.

المشروع قيد التطوير النشط. احتفظ بنسخ احتياطية من رسومات الإنتاج المهمة، وأبلغ عن المشكلات القابلة لإعادة الإنتاج عبر [GitHub Issues](https://github.com/HakanSeven12/OpenCADStudio/issues).

## أبرز الميزات

- **سير عمل أصلي للرسومات** — فتح ملفات DWG وDXF وتحريرها واستعادتها وحفظها من دون خدمة تحويل.
- **رسم ثنائي الأبعاد دقيق** — خطوط وخطوط متعددة ومنحنيات ومنحنيات spline وتهشير والتقاط الكائنات والتتبع والطبقات والكتل والمراجع الخارجية.
- **أدوات التوثيق** — نصوص وأبعاد وخطوط إشارة وتفاوتات وجداول ومساحة النموذج ومساحة الورق وإطارات العرض وأنماط الطباعة.
- **نمذجة ثلاثية الأبعاد مدعومة بنواة هندسية** — مجسمات أولية وبثق ودوران ومسح وloft وعمليات منطقية وتجزئة كيانات ACIS.
- **تصيير عبر GPU** — إطارات عرض ثنائية وثلاثية الأبعاد مسرّعة بواسطة `wgpu` مع كاميرات متعامدة ومنظورية.
- **سير عمل قابل للتوسعة** — إضافات أصلية ونصوص أوامر وتحويل من دون واجهة وواجهة JSON للأتمتة تعتمد على الأسطر.

<p align="center"><img src="../../site/modeling.png" alt="نموذج ثلاثي الأبعاد في Open CAD Studio" width="100%"></p>

## سير عمل الملفات

| التنسيق أو سير العمل | الدعم |
| --- | --- |
| DWG | قراءة وكتابة؛ اختيار إصدارات الحفظ من R14 إلى 2018 |
| DXF | قراءة وكتابة؛ اختيار إصدارات الحفظ من R14 إلى 2018 |
| BAK / SV$ | فتح النسخ الاحتياطية وملفات الحفظ التلقائي للرسومات |
| OBJ | استيراد الشبكات متعددة الأضلاع |
| LandXML | استيراد نقاط المسح `CgPoint` |
| STL | تصدير بيانات الشبكات ثلاثية الأبعاد |
| STEP AP203 | تصدير بيانات الشبكات ثلاثية الأبعاد |
| PDF | طباعة التخطيطات والعناصر الهندسية المحددة في إصدار سطح المكتب |
| CSV | استخراج بيانات خصائص الكيانات |
| CTB / STB | تحميل جداول أنماط الطباعة وتحريرها |

## سطح المكتب أم الويب

استخدم [تطبيق الويب](https://www.opencadstudio.com) للوصول الفوري من دون تثبيت. تُختار الرسومات من خلال المتصفح وتُحفظ كتنزيلات محلية.

استخدم تطبيق سطح المكتب لارتباطات الملفات الأصلية والصور المصغرة في مدير الملفات وطباعة النظام وإخراج PDF والإضافات الخارجية ونصوص الأوامر والأتمتة من دون واجهة. تتوفر إصدارات لنظام Windows وLinux وmacOS بمعالجات Apple Silicon.

## التثبيت

نزّل جميع الحزم الحالية من [أحدث إصدار](https://github.com/HakanSeven12/OpenCADStudio/releases/latest).

### Windows

اختر إحدى حزم x86-64 الموقّعة:

- `OpenCADStudio-*-windows-x86_64-installer.msi` — برنامج التثبيت الموصى به، ويتضمن اختصارات قائمة ابدأ وارتباطات ملفات DWG/DXF وصوراً مصغرة للرسومات.
- `OpenCADStudio-*-windows-x86_64-portable.exe` — تطبيق مستقل لا يحتاج إلى تثبيت.

### Linux

نزّل AppImage لمعمارية x86-64 واجعله قابلاً للتنفيذ ثم شغّله:

```bash
chmod +x OpenCADStudio-*-linux-x86_64.AppImage
./OpenCADStudio-*-linux-x86_64.AppImage
```

### macOS

تدعم حزمة macOS المنشورة أجهزة Apple Silicon:

1. نزّل `OpenCADStudio-*-macos-arm64.dmg`.
2. افتح الصورة واسحب `OpenCADStudio.app` إلى **Applications**.
3. إذا منع Gatekeeper التشغيل الأول، فاسمح بالتطبيق من **System Settings → Privacy & Security**.

التطبيق موقّع بتوقيع ad hoc، لكنه غير موثّق حالياً لدى Apple.

## اللغات

يمكن لـ Open CAD Studio اتباع لغة النظام أو استخدام إحدى لغات الواجهة العشرين التالية:

> العربية · البرتغالية البرازيلية · البلغارية · التشيكية · الهولندية · الإنجليزية · الفنلندية · الفرنسية · الألمانية · الهندية · المجرية · الإيطالية · اليابانية · الكورية · البولندية · الروسية · الصينية المبسطة · الإسبانية · الصينية التقليدية · التركية

غيّر اللغة من إعدادات التطبيق. عند اختيار **النظام**، يستخدم إصدار المتصفح أيضاً الإعدادات المحلية المفضلة للمتصفح.

## البناء من المصدر

### سطح المكتب

المتطلبات:

- Git
- سلسلة أدوات Rust المستقرة الحالية
- مكتبات تطوير الرسوم والخطوط الخاصة بالمنصة

ثبّت الاعتماديات الأصلية على Ubuntu أو Debian:

```bash
sudo apt update
sudo apt install libgl1-mesa-dev libx11-dev libxcursor-dev libxi-dev \
  libxrandr-dev libxkbcommon-dev libwayland-dev libfontconfig1-dev \
  libfreetype6-dev
```

ثم ابنِ التطبيق:

```bash
git clone https://github.com/HakanSeven12/OpenCADStudio.git
cd OpenCADStudio
cargo build --release --bin OpenCADStudio
```

يُكتب الملف التنفيذي الناتج في `target/release/OpenCADStudio` (`OpenCADStudio.exe` على Windows).

### الويب

ثبّت هدف WebAssembly وأدوات البناء مرة واحدة:

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk wasm-bindgen-cli
```

شغّل خادم التطوير:

```bash
trunk serve
```

## الأتمتة

يدعم الملف التنفيذي لسطح المكتب التحويل لمرة واحدة وخادماً دائماً من دون واجهة:

```bash
OpenCADStudio --export input.dwg output.dxf
OpenCADStudio --serve
OpenCADStudio --serve --port 4242
```

يتبادل الخادم كائن JSON واحداً في كل سطر عبر الإدخال/الإخراج القياسي أو مقبس TCP محلي. راجع [دليل الأتمتة](../automation/README.md) و[عميل Python](../automation/ocs.py) المرفق.

## الإضافات

تعمل إضافات سطح المكتب في عمليات منفصلة وتتواصل مع المضيف عبر واجهة إضافات ذات إصدارات. لا يحمّل إصدار المتصفح الإضافات الأصلية.

- [بنية الإضافات](../plugin-architecture.md)
- [قالب الإضافة](../plugin-template/README.md)
- [سجل الإضافات](../../plugins/README.md)

## وثائق المشروع

- [واجهة الأتمتة](../automation/README.md)
- [بنية الإضافات](../plugin-architecture.md)
- [مسار التجزئة](../tessellation.md)
- [سياسة الأمان](../../SECURITY.md)

## المساهمة

نرحب بتقارير الأخطاء وطلبات السحب المركزة والترجمات وتحسينات الوثائق والمساهمات في الإضافات.

- ابحث في [المشكلات](https://github.com/HakanSeven12/OpenCADStudio/issues) الحالية قبل فتح تقرير جديد.
- استخدم [Discussions](https://github.com/HakanSeven12/OpenCADStudio/discussions) للأسئلة والأفكار.
- أبلغ عن الثغرات بشكل خاص وفق [سياسة الأمان](../../SECURITY.md).

## نمو المشروع

<a href="https://github.com/HakanSeven12/OpenCADStudio/stargazers">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://www.opencadstudio.com/star-history-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://www.opencadstudio.com/star-history-light.svg">
    <img alt="نجوم Open CAD Studio وتنزيلات الإصدارات" src="https://www.opencadstudio.com/star-history-light.svg">
  </picture>
</a>

## دعم المشروع

إذا ساعدك Open CAD Studio في عملك، فادعم استمرار التطوير عبر [GitHub Sponsors](https://github.com/sponsors/HakanSeven12) أو [Patreon](https://www.patreon.com/HakanSeven12).

## الترخيص

يُوزّع Open CAD Studio بموجب [رخصة جنو العمومية الإصدار 3.0](../../LICENSE).
