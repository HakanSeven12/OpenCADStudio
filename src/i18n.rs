//! Application language selection and embedded Fluent resources.

use i18n_embed::fluent::{fluent_language_loader, FluentLanguageLoader};
#[cfg(all(not(target_arch = "wasm32"), not(test)))]
use i18n_embed::DesktopLanguageRequester;
use i18n_embed::LanguageLoader;
#[cfg(target_arch = "wasm32")]
use i18n_embed::WebLanguageRequester;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

#[path = "locale_catalog.rs"]
mod locale_catalog;

#[derive(RustEmbed)]
#[folder = "locales/"]
struct Localizations;

/// User-selectable UI language. `System` keeps following the platform's
/// preferred locale while explicit choices remain stable across restarts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "en-US")]
    EnUs,
    #[serde(rename = "bg-BG")]
    BgBg,
    #[serde(rename = "tr-TR")]
    TrTr,
    #[serde(rename = "nl-NL")]
    NlNl,
    #[serde(rename = "fr-FR")]
    FrFr,
    #[serde(rename = "de-DE")]
    DeDe,
    #[serde(rename = "hi-IN")]
    HiIn,
    #[serde(rename = "ru-RU")]
    RuRu,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "es-ES")]
    EsEs,
    #[serde(rename = "pt-BR")]
    PtBr,
    #[serde(rename = "ar-SA")]
    ArSa,
    #[serde(rename = "ja-JP")]
    JaJp,
    #[serde(rename = "ko-KR")]
    KoKr,
    #[serde(rename = "lo-LA")]
    LoLa,
    #[serde(rename = "cs-CZ")]
    CsCz,
    #[serde(rename = "it-IT")]
    ItIt,
    #[serde(rename = "fi-FI")]
    FiFi,
    #[serde(rename = "hu-HU")]
    HuHu,
    #[serde(rename = "pl-PL")]
    PlPl,
    #[serde(rename = "zh-TW")]
    ZhTw,
    #[serde(rename = "el-GR")]
    ElGr,
    #[serde(rename = "vi-VN")]
    ViVn,
}

impl Language {
    pub const ALL: [Language; 24] = [
        Language::System,
        Language::EnUs,
        Language::BgBg,
        Language::PtBr,
        Language::CsCz,
        Language::NlNl,
        Language::FrFr,
        Language::FiFi,
        Language::DeDe,
        Language::ElGr,
        Language::HuHu,
        Language::ItIt,
        Language::JaJp,
        Language::KoKr,
        Language::LoLa,
        Language::PlPl,
        Language::RuRu,
        Language::ZhCn,
        Language::EsEs,
        Language::ZhTw,
        Language::TrTr,
        Language::HiIn,
        Language::ArSa,
        Language::ViVn,
    ];

    fn requested(self) -> Vec<i18n_embed::unic_langid::LanguageIdentifier> {
        match self {
            Language::System => system_languages(),
            Language::EnUs => vec!["en-US".parse().expect("valid locale")],
            Language::BgBg => vec!["bg-BG".parse().expect("valid locale")],
            Language::TrTr => vec!["tr-TR".parse().expect("valid locale")],
            Language::NlNl => vec!["nl-NL".parse().expect("valid locale")],
            Language::FrFr => vec!["fr-FR".parse().expect("valid locale")],
            Language::DeDe => vec!["de-DE".parse().expect("valid locale")],
            Language::HiIn => vec!["hi-IN".parse().expect("valid locale")],
            Language::RuRu => vec!["ru-RU".parse().expect("valid locale")],
            Language::ZhCn => vec!["zh-CN".parse().expect("valid locale")],
            Language::EsEs => vec!["es-ES".parse().expect("valid locale")],
            Language::PtBr => vec!["pt-BR".parse().expect("valid locale")],
            Language::ArSa => vec!["ar-SA".parse().expect("valid locale")],
            Language::JaJp => vec!["ja-JP".parse().expect("valid locale")],
            Language::KoKr => vec!["ko-KR".parse().expect("valid locale")],
            Language::LoLa => vec!["lo-LA".parse().expect("valid locale")],
            Language::CsCz => vec!["cs-CZ".parse().expect("valid locale")],
            Language::ItIt => vec!["it-IT".parse().expect("valid locale")],
            Language::FiFi => vec!["fi-FI".parse().expect("valid locale")],
            Language::HuHu => vec!["hu-HU".parse().expect("valid locale")],
            Language::PlPl => vec!["pl-PL".parse().expect("valid locale")],
            Language::ZhTw => vec!["zh-TW".parse().expect("valid locale")],
            Language::ElGr => vec!["el-GR".parse().expect("valid locale")],
            Language::ViVn => vec!["vi-VN".parse().expect("valid locale")],
        }
    }

    pub fn label(self) -> String {
        match self {
            Language::System => crate::tr!("language", "system"),
            Language::EnUs => crate::tr!("language", "english"),
            Language::BgBg => crate::tr!("language", "bulgarian"),
            Language::TrTr => crate::tr!("language", "turkish"),
            Language::NlNl => crate::tr!("language", "dutch"),
            Language::FrFr => crate::tr!("language", "french"),
            Language::DeDe => crate::tr!("language", "german"),
            Language::HiIn => crate::tr!("language", "hindi"),
            Language::RuRu => crate::tr!("language", "russian"),
            Language::ZhCn => crate::tr!("language", "chinese-simplified"),
            Language::EsEs => crate::tr!("language", "spanish"),
            Language::PtBr => crate::tr!("language", "portuguese"),
            Language::ArSa => crate::tr!("language", "arabic"),
            Language::JaJp => crate::tr!("language", "japanese"),
            Language::KoKr => crate::tr!("language", "korean"),
            Language::LoLa => crate::tr!("language", "lao"),
            Language::CsCz => crate::tr!("language", "czech"),
            Language::ItIt => crate::tr!("language", "italian"),
            Language::FiFi => crate::tr!("language", "finnish"),
            Language::HuHu => crate::tr!("language", "hungarian"),
            Language::PlPl => crate::tr!("language", "polish"),
            Language::ZhTw => crate::tr!("language", "chinese-traditional"),
            Language::ElGr => crate::tr!("language", "greek"),
            Language::ViVn => crate::tr!("language", "vietnamese"),
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = self.label();
        f.write_str(&label)
    }
}

fn system_languages() -> Vec<i18n_embed::unic_langid::LanguageIdentifier> {
    // The unit tests compare English prompts and messages through one
    // process-wide loader, and every `apply_config` of a default config asks
    // for the system language, so the machine's locale must never reach
    // them — not even from a test that runs in parallel.
    #[cfg(test)]
    {
        vec!["en-US"
            .parse()
            .expect("en-US is a valid language identifier")]
    }

    #[cfg(not(test))]
    {
        #[cfg(target_arch = "wasm32")]
        let mut requested = WebLanguageRequester::requested_languages();
        #[cfg(not(target_arch = "wasm32"))]
        let requested = DesktopLanguageRequester::requested_languages();

        // `navigator.languages` may be empty in privacy-restricted browser
        // contexts. The singular preference is still exposed by mainstream
        // browsers, so keep it as the first fallback before English.
        #[cfg(target_arch = "wasm32")]
        if requested.is_empty() {
            if let Some(language) = web_sys::window()
                .and_then(|window| window.navigator().language())
                .and_then(|language| language.parse().ok())
            {
                requested.push(language);
            }
        }

        requested
    }
}

fn load_language(
    loader: &FluentLanguageLoader,
    language: Language,
) -> Result<(), i18n_embed::I18nEmbedError> {
    let mut requested = language.requested();
    if requested.is_empty() {
        requested.push(loader.fallback_language().clone());
    }
    #[allow(unused_variables)]
    let selected = i18n_embed::select(loader, &Localizations, &requested)?;
    #[cfg(target_arch = "wasm32")]
    if let Some(language) = selected.first() {
        if let Some(root) = web_sys::window()
            .and_then(|window| window.document())
            .and_then(|document| document.document_element())
        {
            let _ = root.set_attribute("lang", &language.to_string());
            let direction = if language.to_string().starts_with("ar") {
                "rtl"
            } else {
                "ltr"
            };
            let _ = root.set_attribute("dir", direction);
        }
    }
    Ok(())
}

pub fn loader() -> &'static FluentLanguageLoader {
    static LOADER: OnceLock<FluentLanguageLoader> = OnceLock::new();
    LOADER.get_or_init(|| {
        let loader = fluent_language_loader!();
        if let Err(error) = load_language(&loader, Language::System) {
            eprintln!("Unable to load system UI language: {error}");
            loader
                .load_languages(&Localizations, &[loader.fallback_language().clone()])
                .expect("fallback UI language must be embedded");
        }
        loader
    })
}

/// Apply a user preference process-wide. The loader swaps resources atomically,
/// so the next Iced view pass immediately receives the new language.
pub fn set_language(language: Language) -> Result<(), i18n_embed::I18nEmbedError> {
    switch_language(loader(), language)
}

fn switch_language(
    loader: &FluentLanguageLoader,
    language: Language,
) -> Result<(), i18n_embed::I18nEmbedError> {
    let result = load_language(loader, language);
    LANGUAGE_GENERATION.fetch_add(1, Ordering::Release);
    result
}

#[cfg(target_arch = "wasm32")]
pub fn active_language_tag() -> String {
    loader()
        .current_languages()
        .into_iter()
        .next()
        .unwrap_or_else(|| loader().fallback_language().clone())
        .to_string()
}

/// Catalog ids of a source string, memoized per thread. The catalog is one
/// string `match` over thousands of arms, a linear scan per call; labels are
/// looked up on every view pass and per selected object in the Properties
/// panel, so the scan dominated those paths (#1999).
fn catalog_ids(source: &str) -> Option<(&'static str, &'static str)> {
    type Ids = rustc_hash::FxHashMap<Box<str>, Option<(&'static str, &'static str)>>;
    thread_local! {
        static IDS: std::cell::RefCell<Ids> = std::cell::RefCell::default();
    }
    IDS.with(|ids| {
        if let Some(found) = ids.borrow().get(source) {
            return *found;
        }
        let found = locale_catalog::message_attribute(source);
        let mut ids = ids.borrow_mut();
        // Runtime text (names, values) can reach `t!` too; stay bounded.
        if ids.len() >= 16_384 {
            ids.clear();
        }
        ids.insert(source.into(), found);
        found
    })
}

/// Translate an application-facing source label from the complete UI catalog.
///
/// Stable semantic ids remain preferable for new code. This compatibility
/// layer lets existing UI surfaces move to Fluent without turning command
/// tokens, property ids, file-format values, or plug-in supplied text into
/// translatable data.
pub fn translate(source: impl AsRef<str>) -> Cow<'static, str> {
    let source = source.as_ref();
    match catalog_ids(source) {
        Some((message_id, attribute_id)) => {
            Cow::Borrowed(cached_translation(source, message_id, attribute_id))
        }
        None => Cow::Owned(source.to_string()),
    }
}

/// Translate a catalog message and replace the named values used by legacy
/// command prompts. Markers preserve values through Fluent parsing.
pub fn translate_args(source: impl AsRef<str>, args: &[(&str, String)]) -> Cow<'static, str> {
    let source = source.as_ref();
    let mut translated = catalog_ids(source)
        .map(|(message_id, attribute_id)| {
            cached_translation(source, message_id, attribute_id).to_string()
        })
        .unwrap_or_else(|| source.to_string());
    for (name, value) in args {
        translated = translated.replace(&format!("__ocs_arg_{name}__"), value);
        translated = translated.replace(&format!("%{{{name}}}"), value);
    }
    Cow::Owned(translated)
}

/// Translate a Rust formatting template after its values have been rendered.
/// The catalog stores positional markers, while this function recovers each
/// rendered value from the English template and places it in the localized
/// sentence. File names, handles, counts, and command values therefore remain
/// data instead of being sent through translation.
pub fn translate_format(template: &str, rendered: String) -> Cow<'static, str> {
    let Some((message_id, attribute_id)) = catalog_ids(template) else {
        return Cow::Owned(rendered);
    };
    let Some(values) = format_values(template, &rendered) else {
        return Cow::Owned(rendered);
    };
    let mut translated = cached_translation(template, message_id, attribute_id).to_string();
    for (index, value) in values.iter().enumerate() {
        translated = translated.replace(&format!("__ocs_fmt_{index}__"), value);
    }
    Cow::Owned(translated)
}

/// Bumped whenever `set_language` swaps the loaded Fluent resources, so the
/// per-thread label memo below re-renders instead of serving the old language.
static LANGUAGE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// A label that is already rendered is the same string every time, so the
/// translated text is memoized per thread on top of the id memo. A view pass
/// translates hundreds of labels and the Properties panel translates dozens
/// per selected object; without this each hit still walked the Fluent bundles
/// and allocated a fresh `String`.
fn cached_translation(source: &str, message_id: &str, attribute_id: &str) -> &'static str {
    type Labels = RefCell<(u64, rustc_hash::FxHashMap<Box<str>, &'static str>)>;
    thread_local! {
        static LABELS: Labels = RefCell::new((0, rustc_hash::FxHashMap::default()));
    }
    LABELS.with(|labels| {
        cached_translation_with(
            loader(),
            labels,
            LANGUAGE_GENERATION.load(Ordering::Acquire),
            source,
            message_id,
            attribute_id,
        )
    })
}

fn cached_translation_with(
    loader: &FluentLanguageLoader,
    labels: &RefCell<(u64, rustc_hash::FxHashMap<Box<str>, &'static str>)>,
    generation: u64,
    source: &str,
    message_id: &str,
    attribute_id: &str,
) -> &'static str {
    let mut labels = labels.borrow_mut();
    if labels.0 != generation {
        labels.0 = generation;
        labels.1.clear();
    }
    if let Some(&label) = labels.1.get(source) {
        return label;
    }
    let label: &'static str = Box::leak(loader.get_attr(message_id, attribute_id).into_boxed_str());
    labels.1.insert(source.into(), label);
    label
}

fn format_values(template: &str, rendered: &str) -> Option<Vec<String>> {
    let mut literals = vec![String::new()];
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        match (ch, chars.peek().copied()) {
            ('{', Some('{')) => {
                chars.next();
                literals.last_mut()?.push('{');
            }
            ('}', Some('}')) => {
                chars.next();
                literals.last_mut()?.push('}');
            }
            ('{', _) => {
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return None;
                }
                literals.push(String::new());
            }
            ('}', _) => return None,
            _ => literals.last_mut()?.push(ch),
        }
    }

    let mut cursor = 0;
    if !rendered.starts_with(&literals[0]) {
        return None;
    }
    cursor += literals[0].len();
    let mut values = Vec::with_capacity(literals.len().saturating_sub(1));
    for index in 1..literals.len() {
        let separator = &literals[index];
        if index + 1 == literals.len() && separator.is_empty() {
            values.push(rendered[cursor..].to_string());
            cursor = rendered.len();
        } else {
            let offset = rendered[cursor..].find(separator)?;
            values.push(rendered[cursor..cursor + offset].to_string());
            cursor += offset + separator.len();
        }
    }
    (cursor == rendered.len()).then_some(values)
}

/// Built-in ribbon modules have stable ids; plug-ins keep their supplied title
/// until they provide their own localization bundle.
pub fn ribbon_module_title(id: &str, fallback: &str) -> String {
    match id {
        "draw" => crate::tr!("ribbon-tab", "draw"),
        "parametric" => crate::tr!("ribbon-tab", "parametric"),
        "annotate" => crate::tr!("ribbon-tab", "annotate"),
        "insert" => crate::tr!("ribbon-tab", "insert"),
        "model" => crate::tr!("ribbon-tab", "model"),
        "layout" => crate::tr!("ribbon-tab", "layout"),
        "manage" => crate::tr!("ribbon-tab", "manage"),
        "view" => crate::tr!("ribbon-tab", "view"),
        "block_editor" => crate::tr!("ribbon-tab", "block-editor"),
        _ => fallback.to_string(),
    }
}

#[macro_export]
macro_rules! tr {
    ($message_id:literal, $attribute_id:literal $(,)?) => {
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id, $attribute_id)
    };
    ($message_id:literal, $attribute_id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        i18n_embed_fl::fl!(
            $crate::i18n::loader(),
            $message_id,
            $attribute_id,
            $($name = $value),+
        )
    };
    ($message_id:literal $(,)?) => {
        i18n_embed_fl::fl!($crate::i18n::loader(), $message_id)
    };
    ($message_id:literal, $($name:ident = $value:expr),+ $(,)?) => {
        i18n_embed_fl::fl!(
            $crate::i18n::loader(),
            $message_id,
            $($name = $value),+
        )
    };
}

/// Source-catalog compatibility macro used while the existing interface is
/// migrated to semantic Fluent ids. Named arguments mirror the command prompt
/// placeholders already present in the source catalog.
#[macro_export]
macro_rules! t {
    ($source:expr $(,)?) => {
        $crate::i18n::translate($source)
    };
    ($source:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::translate_args(
            $source,
            &[$((stringify!($name), ($value).to_string())),+],
        )
    };
}

/// Localized counterpart of `format!` for application-facing messages.
#[macro_export]
macro_rules! tf {
    ($template:literal $($args:tt)*) => {{
        let rendered = format!($template $($args)*);
        $crate::i18n::translate_format($template, rendered)
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn block_grip_messages_render_in_every_locale_without_translating_authored_data() {
        let expected = BTreeSet::from(
            [
                "visibility-label",
                "visibility-value",
                "table-label",
                "table-row",
                "alignment",
                "flipped",
                "not-flipped",
            ]
            .map(String::from),
        );
        for language in Language::ALL
            .into_iter()
            .filter(|language| *language != Language::System)
        {
            let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
            load_language(&loader, language).expect("Fluent resources must parse");
            let actual: BTreeSet<_> =
                loader.with_message_iter(&language.requested()[0], |messages| {
                    messages
                        .filter(|message| message.id.name == "block-grip")
                        .flat_map(|message| {
                            message
                                .attributes
                                .iter()
                                .map(|attribute| attribute.id.name.to_owned())
                        })
                        .collect()
                });
            assert_eq!(
                actual, expected,
                "{language:?}: messages must exist in this locale, not fall back"
            );
            let authored_state = "  Façade — مسافة — 距離 { $state } \\ literal ";
            let mode = loader.get_attr("common", "tangent");
            let args = [
                ("state", authored_state),
                ("row", "17"),
                ("mode", mode.as_str()),
            ]
            .into_iter()
            .collect();
            let render = |attribute: &str| {
                loader
                    .with_fluent_message_and_bundle("block-grip", |message, bundle| {
                        let pattern = message.get_attribute(attribute).unwrap();
                        let mut errors = Vec::new();
                        let text = bundle
                            .format_pattern(pattern.value(), Some(&args), &mut errors)
                            .into_owned();
                        assert!(errors.is_empty(), "{language:?}/{attribute}: {errors:?}");
                        text
                    })
                    .unwrap()
            };
            for attribute in &expected {
                assert!(!render(attribute).is_empty());
            }
            for attribute in ["base-state-label", "flipped-state-label"] {
                assert!(!loader.get_attr("block-flip", attribute).is_empty());
            }
            assert!(render("visibility-value").contains(authored_state));
            assert!(render("table-row").contains("17"));
            assert!(render("alignment").contains(&mode));
            if language != Language::EnUs {
                assert_ne!(render("visibility-label"), "Visibility state");
                assert_ne!(render("flipped"), "Flipped");
                assert_ne!(render("not-flipped"), "Not flipped");
            }
        }
    }

    #[test]
    fn lookup_help_preserves_interval_examples_in_every_locale() {
        for language in Language::ALL
            .into_iter()
            .filter(|language| *language != Language::System)
        {
            let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
            load_language(&loader, language).expect("Fluent resources must parse");
            let args = [("closed", "[2,6]"), ("open", "(2,6]")]
                .into_iter()
                .collect();
            let rendered = loader
                .with_fluent_message_and_bundle("block-lookup", |message, bundle| {
                    let pattern = message.get_attribute("input-help").unwrap();
                    let mut errors = Vec::new();
                    let text = bundle
                        .format_pattern(pattern.value(), Some(&args), &mut errors)
                        .into_owned();
                    assert!(errors.is_empty(), "{language:?}: {errors:?}");
                    text
                })
                .unwrap();
            assert!(rendered.contains("[2,6]"), "{language:?}: {rendered}");
            assert!(rendered.contains("(2,6]"), "{language:?}: {rendered}");
            for attribute in ["invalid-table", "invalid-value", "incompatible-values"] {
                assert!(!loader.get_attr("block-lookup", attribute).is_empty());
            }
        }
    }

    #[test]
    fn donation_text_is_translated_in_every_locale() {
        for language in Language::ALL
            .into_iter()
            .filter(|language| *language != Language::System)
        {
            let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
            load_language(&loader, language).expect("Fluent resources must parse");
            let attributes: BTreeSet<String> =
                loader.with_message_iter(&language.requested()[0], |messages| {
                    messages
                        .filter(|message| message.id.name == "donation")
                        .flat_map(|message| {
                            message
                                .attributes
                                .iter()
                                .map(|attr| attr.id.name.to_owned())
                        })
                        .collect()
                });
            assert_eq!(
                attributes,
                BTreeSet::from(["title", "heading", "body", "decline"].map(String::from))
            );
            for attribute in attributes {
                assert!(!loader.get_attr("donation", &attribute).is_empty());
            }
        }
    }

    #[test]
    fn every_catalog_covers_and_formats_the_source_catalog() {
        let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
        load_language(&loader, Language::EnUs).expect("English Fluent resources must parse");
        let keys = |loader: &FluentLanguageLoader, language: Language| {
            loader.with_message_iter(&language.requested()[0], |messages| {
                let entries: Vec<_> = messages
                    .flat_map(|message| {
                        std::iter::once((message.id.name.to_owned(), String::new())).chain(
                            message.attributes.iter().map(|attribute| {
                                (message.id.name.to_owned(), attribute.id.name.to_owned())
                            }),
                        )
                    })
                    .collect();
                let unique: BTreeSet<_> = entries.iter().cloned().collect();
                assert_eq!(
                    entries.len(),
                    unique.len(),
                    "duplicate keys in {language:?}"
                );
                unique
            })
        };
        let source = keys(&loader, Language::EnUs);
        assert!(!source.is_empty());

        let english = Localizations::get("en-US/opencadstudio.ftl").unwrap();
        let english = std::str::from_utf8(&english.data).unwrap();
        let variables: Vec<_> = english
            .split('$')
            .skip(1)
            .map(|tail| {
                tail.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
                    .next()
                    .unwrap()
            })
            .collect();
        let args = variables.into_iter().map(|name| (name, 2)).collect();
        let render = |loader: &FluentLanguageLoader, message: &str, attribute: &str| {
            loader
                .with_fluent_message_and_bundle(message, |fluent_message, bundle| {
                    let pattern = if attribute.is_empty() {
                        fluent_message.value()
                    } else {
                        fluent_message
                            .get_attribute(attribute)
                            .map(|attr| attr.value())
                    };
                    pattern
                        .map(|pattern| {
                            let mut errors = Vec::new();
                            let text = bundle.format_pattern(pattern, Some(&args), &mut errors);
                            assert!(errors.is_empty(), "{message}.{attribute}: {errors:?}");
                            text.into_owned()
                        })
                        .unwrap_or_default()
                })
                .unwrap()
        };
        let markers = |text: &str| -> BTreeSet<String> {
            text.split("__ocs_")
                .skip(1)
                .filter_map(|tail| tail.split_once("__").map(|(marker, _)| marker.to_owned()))
                .collect()
        };
        for language in Language::ALL.into_iter().filter(|l| *l != Language::System) {
            let resource =
                Localizations::get(&format!("{}/opencadstudio.ftl", language.requested()[0]))
                    .unwrap();
            let text = std::str::from_utf8(&resource.data).unwrap();
            if let Err((_, errors)) = fluent_syntax::parser::parse(text) {
                panic!("Invalid Fluent syntax in {language:?}: {errors:?}");
            }
            let localized = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
            load_language(&localized, language).expect("Fluent resources must parse");
            let actual = keys(&localized, language);
            let extra: Vec<_> = actual.difference(&source).collect();
            assert!(extra.is_empty(), "extra keys in {language:?}: {extra:?}");
            let missing: Vec<_> = source.difference(&actual).collect();
            assert!(
                missing.is_empty(),
                "{} missing keys in {language:?}: {:?}",
                missing.len(),
                &missing[..missing.len().min(10)]
            );
            for (message, attribute) in &source {
                let expected = render(&loader, message, attribute);
                let translated = render(&localized, message, attribute);
                assert_eq!(
                    markers(&translated),
                    markers(&expected),
                    "{language:?}: {message}.{attribute}"
                );
                assert_eq!(
                    translated.is_empty(),
                    expected.is_empty(),
                    "{language:?}: {message}.{attribute}"
                );
            }
        }
    }

    #[test]
    fn greek_language_setting_round_trips() {
        let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
        load_language(&loader, Language::ElGr).expect("Greek Fluent resources must parse");
        assert_eq!(loader.get_attr("language", "greek"), "Ελληνικά");
        assert_eq!(serde_json::to_string(&Language::ElGr).unwrap(), "\"el-GR\"");
    }

    #[test]
    fn cached_labels_match_the_loader_in_every_locale() {
        let sources = ["Import Styles", "Existing", "Ok", "Paused"];
        for (index, language) in Language::ALL.into_iter().enumerate() {
            let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
            load_language(&loader, language).expect("every embedded locale must load");
            let generation = index as u64;
            let cache = RefCell::new((generation, rustc_hash::FxHashMap::default()));
            for source in sources {
                let (message_id, attribute_id) =
                    catalog_ids(source).unwrap_or_else(|| panic!("{source} is a catalog source"));
                let label = cached_translation_with(
                    &loader,
                    &cache,
                    generation,
                    source,
                    message_id,
                    attribute_id,
                );
                assert_eq!(
                    label,
                    loader.get_attr(message_id, attribute_id),
                    "{source} in {language:?}"
                );
                // A repeat call must serve the memo, not render again.
                assert_eq!(
                    cached_translation_with(
                        &loader,
                        &cache,
                        generation,
                        source,
                        message_id,
                        attribute_id,
                    ),
                    label
                );
            }
        }
    }

    #[test]
    fn a_new_language_generation_rerenders_cached_labels() {
        let english = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
        load_language(&english, Language::EnUs).expect("English Fluent resources must parse");
        let chinese = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
        load_language(&chinese, Language::ZhCn).expect("Chinese Fluent resources must parse");
        let cache = RefCell::new((0, rustc_hash::FxHashMap::default()));
        let source = "Draw";
        let english_label = cached_translation_with(&english, &cache, 0, source, "ribbon-tab", "draw");
        let chinese_label = cached_translation_with(&chinese, &cache, 1, source, "ribbon-tab", "draw");
        assert_eq!(english_label, "Draw");
        assert_eq!(chinese_label, chinese.get_attr("ribbon-tab", "draw"));
        assert_ne!(english_label, chinese_label, "the memo must re-render on a language switch");
    }

    #[test]
    fn switching_language_bumps_the_generation() {
        let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
        let before = LANGUAGE_GENERATION.load(Ordering::Acquire);
        switch_language(&loader, Language::DeDe).expect("German Fluent resources must parse");
        assert!(LANGUAGE_GENERATION.load(Ordering::Acquire) > before);
    }
}
