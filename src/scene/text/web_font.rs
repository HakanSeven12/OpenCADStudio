//! Shared web font store + per-script fetch (#141).
//!
//! The desktop build outlines glyphs for non-LFF scripts from the user's
//! installed system fonts (see `ttf_glyph::build_fallback`). The web build has
//! no system-font access, so startup fetches the active UI language's Noto
//! subset and shares those bytes with UI, drawing text, and navigation labels.
//! Every language file also carries the common Latin ranges; other scripts are
//! still fetched on demand when a drawing needs them. CJK remains split by
//! language because the same code point can require a different glyph form.
//!
//! The store and fetch are web-only; the desktop side keeps no-op stubs so the
//! shared call sites (`ttf_glyph`, the app message loop) compile unchanged.

use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

/// A script we ship a Noto subset for. [`script_of`] maps a char to one.
///
/// CJK is split by language — Simplified Chinese, Traditional Chinese,
/// Japanese and Korean each get their own file. Their ideographs (Han,
/// U+4E00–9FFF) share the same code points but differ in glyph shape, so the
/// shared block is routed by the document's language (see
/// [`set_cjk_lang_from_codepage`]); kana is always Japanese and Hangul always
/// Korean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Script {
    Latin,
    Cyrillic,
    Greek,
    Arabic,
    Hebrew,
    Thai,
    Devanagari,
    Chinese,
    TraditionalChinese,
    Japanese,
    Korean,
}

impl Script {
    /// Same-origin asset path the web build fetches this script's font from.
    /// Must match the files produced by `web/fonts/generate.sh`.
    pub fn asset(self) -> &'static str {
        match self {
            Script::Latin => "fonts/latin.ttf",
            Script::Cyrillic => "fonts/cyrillic.ttf",
            Script::Greek => "fonts/greek.ttf",
            Script::Arabic => "fonts/arabic.ttf",
            Script::Hebrew => "fonts/hebrew.ttf",
            Script::Thai => "fonts/thai.ttf",
            Script::Devanagari => "fonts/devanagari.ttf",
            Script::Chinese => "fonts/chinese.ttf",
            Script::TraditionalChinese => "fonts/traditional_chinese.ttf",
            Script::Japanese => "fonts/japanese.ttf",
            Script::Korean => "fonts/korean.ttf",
        }
    }

    pub fn family(self) -> &'static str {
        match self {
            Script::Chinese => "Noto Sans CJK SC",
            Script::TraditionalChinese => "Noto Sans CJK TC",
            Script::Japanese => "Noto Sans CJK JP",
            Script::Korean => "Noto Sans CJK KR",
            Script::Latin
            | Script::Cyrillic
            | Script::Greek
            | Script::Arabic
            | Script::Hebrew
            | Script::Thai
            | Script::Devanagari => "Noto Sans",
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            1 => Script::Cyrillic,
            2 => Script::Greek,
            3 => Script::Arabic,
            4 => Script::Hebrew,
            5 => Script::Thai,
            6 => Script::Devanagari,
            7 => Script::Chinese,
            8 => Script::Japanese,
            9 => Script::Korean,
            10 => Script::TraditionalChinese,
            _ => Script::Latin,
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn as_u8(self) -> u8 {
        match self {
            Script::Latin => 0,
            Script::Cyrillic => 1,
            Script::Greek => 2,
            Script::Arabic => 3,
            Script::Hebrew => 4,
            Script::Thai => 5,
            Script::Devanagari => 6,
            Script::Chinese => 7,
            Script::Japanese => 8,
            Script::Korean => 9,
            Script::TraditionalChinese => 10,
        }
    }
}

/// Language used to render shared Han ideographs: 0 = Simplified Chinese,
/// 1 = Japanese, 2 = Korean, 3 = Traditional Chinese.
static CJK_LANG: AtomicU8 = AtomicU8::new(0);
static PRIMARY_SCRIPT: AtomicU8 = AtomicU8::new(0);
static FONT_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn generation() -> u64 {
    FONT_GENERATION.load(Ordering::Relaxed)
}

fn bump_generation() {
    FONT_GENERATION.fetch_add(1, Ordering::Relaxed);
}

#[cfg(target_arch = "wasm32")]
pub fn scripts_for_language_tag(language: &str) -> Vec<Script> {
    let full_language = language.to_ascii_lowercase().replace('_', "-");
    let language = full_language
        .split(['-', '_'])
        .next()
        .unwrap_or(&full_language)
        .to_ascii_lowercase();
    let script = if matches!(full_language.as_str(), "zh-tw" | "zh-hk")
        || full_language.starts_with("zh-hant")
    {
        Some(Script::TraditionalChinese)
    } else {
        match language.as_str() {
            "ar" | "fa" | "ur" => Some(Script::Arabic),
            "el" => Some(Script::Greek),
            "he" | "yi" => Some(Script::Hebrew),
            "hi" | "mr" | "ne" => Some(Script::Devanagari),
            "ja" => Some(Script::Japanese),
            "ko" => Some(Script::Korean),
            "ru" | "uk" | "bg" | "sr" => Some(Script::Cyrillic),
            "th" => Some(Script::Thai),
            "zh" => Some(Script::Chinese),
            _ => None,
        }
    };
    vec![script.unwrap_or(Script::Latin)]
}

#[cfg(target_arch = "wasm32")]
pub fn preload_language(language: &str) -> Script {
    let script = scripts_for_language_tag(language)
        .into_iter()
        .next()
        .unwrap_or(Script::Latin);
    PRIMARY_SCRIPT.store(script.as_u8(), Ordering::Relaxed);
    bump_generation();
    let _ = imp::request(script);
    script
}

pub fn primary_script() -> Script {
    Script::from_u8(PRIMARY_SCRIPT.load(Ordering::Relaxed))
}

pub fn requires_shaping(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x0590..=0x05FF
                | 0x0600..=0x06FF
                | 0x0750..=0x077F
                | 0x08A0..=0x08FF
                | 0x0900..=0x097F
                | 0x0E00..=0x0E7F
                | 0xA8E0..=0xA8FF
                | 0xFB1D..=0xFB4F
                | 0xFB50..=0xFDFF
                | 0xFE70..=0xFEFF
        )
    })
}

#[cfg(target_arch = "wasm32")]
fn cjk_lang() -> Script {
    match CJK_LANG.load(Ordering::Relaxed) {
        1 => Script::Japanese,
        2 => Script::Korean,
        3 => Script::TraditionalChinese,
        _ => Script::Chinese,
    }
}

/// Point the shared-Han routing at a language based on a DWG/DXF code page
/// (`$DWGCODEPAGE`), e.g. `ANSI_932` → Japanese, `ANSI_949` → Korean and
/// `ANSI_950` → Traditional Chinese. Returns `true` if the language changed.
pub fn set_cjk_lang_from_codepage(code_page: &str) -> bool {
    let c = code_page.to_ascii_uppercase();
    let lang = if c.contains("932") || c.contains("SJIS") || c.contains("SHIFT") {
        1 // Japanese (Shift-JIS)
    } else if c.contains("949") || c.contains("KOR") || c.contains("UHC") {
        2 // Korean
    } else if c.contains("950") || c.contains("BIG5") {
        3 // Traditional Chinese
    } else {
        0 // Simplified Chinese (936 / GB) or non-CJK default
    };
    CJK_LANG.swap(lang, Ordering::Relaxed) != lang
}

/// The script font that covers `ch`, or `None` for control / uncovered code
/// points. Ranges mirror the subset unicode ranges in `web/fonts/generate.sh`.
#[cfg(target_arch = "wasm32")]
pub fn script_of(ch: char) -> Option<Script> {
    Some(match ch as u32 {
        0x0000..=0x024F | 0x1E00..=0x1EFF | 0x2000..=0x206F => Script::Latin,
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Script::Greek,
        0x0400..=0x052F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F => Script::Cyrillic,
        0x0590..=0x05FF | 0xFB1D..=0xFB4F => Script::Hebrew,
        0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
            Script::Arabic
        }
        0x0900..=0x097F | 0xA8E0..=0xA8FF => Script::Devanagari,
        // Hangul → always Korean; kana → always Japanese.
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7A3 => Script::Korean,
        0x3040..=0x30FF | 0x31F0..=0x31FF => Script::Japanese,
        // Shared Han + CJK symbols + fullwidth → routed by the document language.
        0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF => {
            cjk_lang()
        }
        _ => return None,
    })
}

// ── Web store ───────────────────────────────────────────────────────────────

#[cfg(target_arch = "wasm32")]
mod imp {
    use super::Script;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    enum State {
        Loading,
        Loaded(Arc<Vec<u8>>),
        Failed,
    }

    #[derive(Default)]
    struct Store {
        states: HashMap<Script, State>,
        /// Scripts requested but not yet fetched; the app drains this and kicks
        /// off the fetch tasks.
        pending: Vec<Script>,
    }

    fn store() -> &'static Mutex<Store> {
        static S: OnceLock<Mutex<Store>> = OnceLock::new();
        S.get_or_init(|| Mutex::new(Store::default()))
    }

    /// Loaded font bytes for `script`, or `None` — queueing a fetch the first
    /// time a script is missed so the app loop can load it.
    pub fn request(script: Script) -> Option<Arc<Vec<u8>>> {
        let mut s = store().lock().unwrap();
        match s.states.get(&script) {
            Some(State::Loaded(b)) => Some(b.clone()),
            Some(_) => None, // Loading or Failed — don't re-queue.
            None => {
                s.states.insert(script, State::Loading);
                s.pending.push(script);
                None
            }
        }
    }

    /// Drain the scripts awaiting a fetch.
    pub fn take_pending() -> Vec<Script> {
        std::mem::take(&mut store().lock().unwrap().pending)
    }

    /// Record a fetch result: `Some(bytes)` on success, `None` on failure.
    pub fn insert(script: Script, bytes: Option<Vec<u8>>) {
        let mut store = store().lock().unwrap();
        match bytes {
            Some(bytes) => {
                let bytes = Arc::new(bytes);
                store.states.insert(script, State::Loaded(bytes.clone()));
                if script != Script::Latin {
                    store
                        .states
                        .entry(Script::Latin)
                        .or_insert_with(|| State::Loaded(bytes));
                }
            }
            None => {
                store.states.insert(script, State::Failed);
            }
        }
        drop(store);
        super::bump_generation();
    }

    pub fn loaded(script: Script) -> Option<Arc<Vec<u8>>> {
        let s = store().lock().unwrap();
        match s.states.get(&script) {
            Some(State::Loaded(bytes)) => Some(bytes.clone()),
            _ => None,
        }
    }

    /// Fetch a script font over HTTP from the same origin.
    pub async fn fetch(script: Script) -> Result<Vec<u8>, String> {
        use wasm_bindgen::JsCast;
        use wasm_bindgen_futures::JsFuture;
        let win = web_sys::window().ok_or("no window")?;
        let resp_val = JsFuture::from(win.fetch_with_str(script.asset()))
            .await
            .map_err(|e| format!("{e:?}"))?;
        let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "bad response".to_string())?;
        if !resp.ok() {
            return Err(format!("HTTP {}", resp.status()));
        }
        let ab = JsFuture::from(resp.array_buffer().map_err(|e| format!("{e:?}"))?)
            .await
            .map_err(|e| format!("{e:?}"))?;
        Ok(js_sys::Uint8Array::new(&ab).to_vec())
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use super::{Arc, Script};

    // Kept for parity with the wasm impl; only the wasm path has a caller.
    #[allow(dead_code)]
    pub fn request(_script: Script) -> Option<Arc<Vec<u8>>> {
        None
    }
    pub fn take_pending() -> Vec<Script> {
        Vec::new()
    }
    pub fn insert(_script: Script, _bytes: Option<Vec<u8>>) {
        super::bump_generation();
    }
    pub fn loaded(_script: Script) -> Option<Arc<Vec<u8>>> {
        None
    }
    pub async fn fetch(_script: Script) -> Result<Vec<u8>, String> {
        Err("web only".into())
    }
}

pub use imp::{fetch, insert, loaded, take_pending};
// `request` only has a caller in the wasm font-fallback path.
#[cfg(target_arch = "wasm32")]
pub use imp::request;
