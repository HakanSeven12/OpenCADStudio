//! Application language selection and embedded Fluent resources.

use i18n_embed::fluent::{fluent_language_loader, FluentLanguageLoader};
use i18n_embed::LanguageLoader;
#[cfg(not(target_arch = "wasm32"))]
use i18n_embed::DesktopLanguageRequester;
#[cfg(target_arch = "wasm32")]
use i18n_embed::WebLanguageRequester;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
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
}

impl Language {
    pub const ALL: [Language; 22] = [
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
        Language::PlPl,
        Language::RuRu,
        Language::ZhCn,
        Language::EsEs,
        Language::ZhTw,
        Language::TrTr,
        Language::HiIn,
        Language::ArSa,
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
            Language::CsCz => vec!["cs-CZ".parse().expect("valid locale")],
            Language::ItIt => vec!["it-IT".parse().expect("valid locale")],
            Language::FiFi => vec!["fi-FI".parse().expect("valid locale")],
            Language::HuHu => vec!["hu-HU".parse().expect("valid locale")],
            Language::PlPl => vec!["pl-PL".parse().expect("valid locale")],
            Language::ZhTw => vec!["zh-TW".parse().expect("valid locale")],
            Language::ElGr => vec!["el-GR".parse().expect("valid locale")],
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
            Language::CsCz => crate::tr!("language", "czech"),
            Language::ItIt => crate::tr!("language", "italian"),
            Language::FiFi => crate::tr!("language", "finnish"),
            Language::HuHu => crate::tr!("language", "hungarian"),
            Language::PlPl => crate::tr!("language", "polish"),
            Language::ZhTw => crate::tr!("language", "chinese-traditional"),
            Language::ElGr => crate::tr!("language", "greek"),
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
    load_language(loader(), language)
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

/// Translate an application-facing source label from the complete UI catalog.
///
/// Stable semantic ids remain preferable for new code. This compatibility
/// layer lets existing UI surfaces move to Fluent without turning command
/// tokens, property ids, file-format values, or plug-in supplied text into
/// translatable data.
pub fn translate(source: impl AsRef<str>) -> Cow<'static, str> {
    let source = source.as_ref();
    locale_catalog::message_attribute(source)
        .map(|(message_id, attribute_id)| Cow::Owned(loader().get_attr(message_id, attribute_id)))
        .unwrap_or_else(|| Cow::Owned(source.to_string()))
}

/// Translate a catalog message and replace the named values used by legacy
/// command prompts. Catalog generation protects these markers from machine
/// translation and Fluent parsing.
pub fn translate_args(
    source: impl AsRef<str>,
    args: &[(&str, String)],
) -> Cow<'static, str> {
    let source = source.as_ref();
    let mut translated = locale_catalog::message_attribute(source)
        .map(|(message_id, attribute_id)| loader().get_attr(message_id, attribute_id))
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
    let Some((message_id, attribute_id)) = locale_catalog::message_attribute(template) else {
        return Cow::Owned(rendered);
    };
    let Some(values) = format_values(template, &rendered) else {
        return Cow::Owned(rendered);
    };
    let mut translated = loader().get_attr(message_id, attribute_id);
    for (index, value) in values.iter().enumerate() {
        translated = translated.replace(&format!("__ocs_fmt_{index}__"), value);
    }
    Cow::Owned(translated)
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
        "annotate" => crate::tr!("ribbon-tab", "annotate"),
        "insert" => crate::tr!("ribbon-tab", "insert"),
        "model" => crate::tr!("ribbon-tab", "model"),
        "layout" => crate::tr!("ribbon-tab", "layout"),
        "manage" => crate::tr!("ribbon-tab", "manage"),
        "view" => crate::tr!("ribbon-tab", "view"),
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
    fn donation_text_is_translated_in_every_locale() {
        for language in Language::ALL
            .into_iter()
            .filter(|language| *language != Language::System)
        {
            let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
            load_language(&loader, language).expect("Fluent resources must parse");
            let attributes: BTreeSet<String> = loader.with_message_iter(
                &language.requested()[0],
                |messages| {
                    messages
                        .filter(|message| message.id.name == "donation")
                        .flat_map(|message| {
                            message.attributes.iter().map(|attr| attr.id.name.to_owned())
                        })
                        .collect()
                },
            );
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
    fn greek_catalog_loads_and_covers_the_source_catalog() {
        let loader = FluentLanguageLoader::new("opencadstudio", "en-US".parse().unwrap());
        load_language(&loader, Language::ElGr).expect("Greek Fluent resources must parse");
        let keys = |language: &str| -> BTreeSet<(String, String)> {
            loader.with_message_iter(&language.parse().unwrap(), |messages| {
                messages
                    .flat_map(|message| {
                        std::iter::once((message.id.name.to_owned(), String::new())).chain(
                            message.attributes.iter().map(|attribute| {
                                (message.id.name.to_owned(), attribute.id.name.to_owned())
                            }),
                        )
                    })
                    .collect()
            })
        };
        let source = keys("en-US");
        assert!(!source.is_empty());
        assert_eq!(keys("el-GR"), source);
        assert_eq!(loader.get_attr("language", "greek"), "Ελληνικά");
        assert_eq!(serde_json::to_string(&Language::ElGr).unwrap(), "\"el-GR\"");
    }
}
