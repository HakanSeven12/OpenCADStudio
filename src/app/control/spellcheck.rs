//! Native OS spell-checker abstraction supporting Windows, macOS, and Linux.
//!
//! - **Windows**: Windows 8+ native Spell Checking API (`ISpellChecker` via COM).
//! - **macOS**: Apple AppKit `NSSpellChecker` via Objective-C.
//! - **Linux**: Standard system dictionary wordlists (`/usr/share/hunspell`, `/usr/share/myspell`,
//!   `/usr/share/dict`) and Hunspell / Aspell pipe integration.
//! - **Other**: Graceful no-op fallback.

/// Unified system spell checker handle.
pub struct SystemSpeller {
    backend: Backend,
    language: String,
}

enum Backend {
    #[cfg(target_os = "windows")]
    Windows(windows_impl::WindowsSpeller),
    #[cfg(target_os = "macos")]
    Mac(macos_impl::MacSpeller),
    #[cfg(target_os = "linux")]
    Linux(linux_impl::LinuxSpeller),
    Fallback(fallback_impl::FallbackSpeller),
}

impl SystemSpeller {
    /// Create a system speller for the given BCP-47 language tag (e.g. "fr-FR", "en-US", "de-DE", "es-ES"),
    /// or default to the host system's UI locale.
    pub fn new(requested_lang: Option<&str>) -> Self {
        let lang = normalize_language_tag(requested_lang);

        #[cfg(target_os = "windows")]
        {
            if let Some(win_speller) = windows_impl::WindowsSpeller::new(&lang) {
                let actual_lang = win_speller.language().to_string();
                return Self {
                    backend: Backend::Windows(win_speller),
                    language: actual_lang,
                };
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Some(mac_speller) = macos_impl::MacSpeller::new(&lang) {
                let actual_lang = mac_speller.language().to_string();
                return Self {
                    backend: Backend::Mac(mac_speller),
                    language: actual_lang,
                };
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Some(linux_speller) = linux_impl::LinuxSpeller::new(&lang) {
                let actual_lang = linux_speller.language().to_string();
                return Self {
                    backend: Backend::Linux(linux_speller),
                    language: actual_lang,
                };
            }
        }

        Self {
            backend: Backend::Fallback(fallback_impl::FallbackSpeller),
            language: lang,
        }
    }

    /// Whether a native system spell-checker is active and available.
    pub fn is_available(&self) -> bool {
        match &self.backend {
            #[cfg(target_os = "windows")]
            Backend::Windows(_) => true,
            #[cfg(target_os = "macos")]
            Backend::Mac(_) => true,
            #[cfg(target_os = "linux")]
            Backend::Linux(l) => l.is_available(),
            Backend::Fallback(_) => false,
        }
    }

    /// Returns the active backend name ("windows", "macos", "linux_system", or "none").
    pub fn backend_name(&self) -> &'static str {
        match &self.backend {
            #[cfg(target_os = "windows")]
            Backend::Windows(_) => "windows",
            #[cfg(target_os = "macos")]
            Backend::Mac(_) => "macos",
            #[cfg(target_os = "linux")]
            Backend::Linux(_) => "linux_system",
            Backend::Fallback(_) => "none",
        }
    }

    /// The active language tag (e.g. "fr-FR", "en-US").
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Check if a word is spelled correctly according to the system spell-checker.
    ///
    /// Returns `(is_correct, suggestions)`.
    /// When `is_correct` is false, `suggestions` contains system-suggested corrections.
    pub fn check_word(&self, word: &str, want_suggestions: bool) -> (bool, Vec<String>) {
        if word.trim().is_empty() {
            return (true, Vec::new());
        }

        match &self.backend {
            #[cfg(target_os = "windows")]
            Backend::Windows(w) => w.check_word(word, want_suggestions),
            #[cfg(target_os = "macos")]
            Backend::Mac(m) => m.check_word(word, want_suggestions),
            #[cfg(target_os = "linux")]
            Backend::Linux(l) => l.check_word(word, want_suggestions),
            Backend::Fallback(_) => (true, Vec::new()),
        }
    }
}

fn normalize_language_tag(input: Option<&str>) -> String {
    let raw: String = match input {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            #[cfg(target_os = "windows")]
            {
                let mut buf = [0u16; 85];
                let n = unsafe { windows_impl::GetUserDefaultLocaleName(buf.as_mut_ptr(), 85) };
                if n > 1 {
                    String::from_utf16_lossy(&buf[..(n - 1) as usize])
                } else {
                    "en-US".to_string()
                }
            }
            #[cfg(not(target_os = "windows"))]
            {
                if let Ok(lang) = std::env::var("LANG").or_else(|_| std::env::var("LC_ALL")) {
                    let cleaned = lang.split('.').next().unwrap_or("").replace('_', "-");
                    if !cleaned.is_empty() {
                        cleaned
                    } else {
                        "en-US".to_string()
                    }
                } else {
                    "en-US".to_string()
                }
            }
        }
    };

    let lower = raw.to_lowercase();
    match lower.as_str() {
        "fr" | "french" | "francais" | "français" => "fr-FR".to_string(),
        "en" | "english" => "en-US".to_string(),
        "de" | "german" | "deutsch" => "de-DE".to_string(),
        "es" | "spanish" | "espanol" | "español" => "es-ES".to_string(),
        "it" | "italian" | "italiano" => "it-IT".to_string(),
        _ => raw.to_string(),
    }
}

// ============================================================================
// Windows Backend: Win32 Spell Checking API (ISpellChecker via COM)
// ============================================================================
#[cfg(target_os = "windows")]
pub(crate) mod windows_impl {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;

    type HRESULT = i32;
    type BOOL = i32;

    #[repr(C)]
    struct GUID {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }

    const CLSID_SPELL_CHECKER_FACTORY: GUID = GUID {
        data1: 0x7ab36653,
        data2: 0x1796,
        data3: 0x484b,
        data4: [0xbd, 0xfa, 0xe7, 0x4f, 0x1d, 0xb7, 0xc1, 0xdc],
    };

    const IID_ISPELL_CHECKER_FACTORY: GUID = GUID {
        data1: 0x8e018a9d,
        data2: 0x2415,
        data3: 0x4677,
        data4: [0xbf, 0x08, 0x79, 0x4e, 0xa6, 0x1f, 0x94, 0xbb],
    };

    #[repr(C)]
    struct ISpellCheckerFactoryVtbl {
        query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_supported_languages: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
        is_supported: unsafe extern "system" fn(*mut c_void, *const u16, *mut BOOL) -> HRESULT,
        create_spell_checker: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    struct ISpellCheckerVtbl {
        query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_language_tag: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
        check: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HRESULT,
        suggest: unsafe extern "system" fn(*mut c_void, *const u16, *mut *mut c_void) -> HRESULT,
        add: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
        ignore: unsafe extern "system" fn(*mut c_void, *const u16) -> HRESULT,
        auto_correct: unsafe extern "system" fn(*mut c_void, *const u16, *const u16) -> HRESULT,
        get_option_value: unsafe extern "system" fn(*mut c_void, *const u16, *mut u8) -> HRESULT,
        get_option_ids: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
        get_id: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
        get_localized_name: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
    }

    #[repr(C)]
    struct IEnumSpellingErrorVtbl {
        query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        next: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    }

    #[repr(C)]
    struct ISpellingErrorVtbl {
        query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        get_start_index: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
        get_length: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
        get_corrective_action: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
        get_replacement: unsafe extern "system" fn(*mut c_void, *mut *mut u16) -> HRESULT,
    }

    #[repr(C)]
    struct IEnumStringVtbl {
        query_interface: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        next: unsafe extern "system" fn(*mut c_void, u32, *mut *mut u16, *mut u32) -> HRESULT,
    }

    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(pv_reserved: *const c_void, dw_co_init: u32) -> HRESULT;
        fn CoCreateInstance(
            rclsid: *const GUID,
            p_unk_outer: *mut c_void,
            dw_cls_context: u32,
            riid: *const GUID,
            ppv: *mut *mut c_void,
        ) -> HRESULT;
        fn CoTaskMemFree(pv: *mut c_void);
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetUserDefaultLocaleName(lp_locale_name: *mut u16, cch_locale_name: i32) -> i32;
    }

    fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    unsafe fn from_wide_ptr(ptr: *mut u16) -> String {
        if ptr.is_null() {
            return String::new();
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        let s = String::from_utf16_lossy(slice);
        CoTaskMemFree(ptr as *mut c_void);
        s
    }

    pub struct WindowsSpeller {
        checker_obj: *mut c_void,
        language: String,
    }

    // Windows COM objects created with CLSCTX_INPROC_SERVER are safe to invoke from our control thread.
    unsafe impl Send for WindowsSpeller {}
    unsafe impl Sync for WindowsSpeller {}

    impl WindowsSpeller {
        pub fn new(lang_tag: &str) -> Option<Self> {
            unsafe {
                CoInitializeEx(std::ptr::null(), 0);

                let mut factory_obj: *mut c_void = std::ptr::null_mut();
                let hr = CoCreateInstance(
                    &CLSID_SPELL_CHECKER_FACTORY,
                    std::ptr::null_mut(),
                    1, // CLSCTX_INPROC_SERVER
                    &IID_ISPELL_CHECKER_FACTORY,
                    &mut factory_obj,
                );
                if hr != 0 || factory_obj.is_null() {
                    return None;
                }

                let factory_vtbl = *(factory_obj as *const *const ISpellCheckerFactoryVtbl);

                // Try requested language, then fallback languages
                let mut candidates = vec![lang_tag.to_string()];
                if !candidates.contains(&"en-US".to_string()) {
                    candidates.push("en-US".to_string());
                }

                let mut chosen_checker = None;
                let mut chosen_lang = String::new();

                for candidate in candidates {
                    let wide_cand = to_wide(&candidate);
                    let mut supported: BOOL = 0;
                    let hr_supp = ((*factory_vtbl).is_supported)(
                        factory_obj,
                        wide_cand.as_ptr(),
                        &mut supported,
                    );
                    if hr_supp == 0 && supported != 0 {
                        let mut checker_obj: *mut c_void = std::ptr::null_mut();
                        let hr_create = ((*factory_vtbl).create_spell_checker)(
                            factory_obj,
                            wide_cand.as_ptr(),
                            &mut checker_obj,
                        );
                        if hr_create == 0 && !checker_obj.is_null() {
                            chosen_checker = Some(checker_obj);
                            chosen_lang = candidate;
                            break;
                        }
                    }
                }

                ((*factory_vtbl).release)(factory_obj);

                chosen_checker.map(|obj| Self {
                    checker_obj: obj,
                    language: chosen_lang,
                })
            }
        }

        pub fn language(&self) -> &str {
            &self.language
        }

        pub fn check_word(&self, word: &str, want_suggestions: bool) -> (bool, Vec<String>) {
            if self.checker_obj.is_null() {
                return (true, Vec::new());
            }

            unsafe {
                let checker_vtbl = *(self.checker_obj as *const *const ISpellCheckerVtbl);
                let wide_text = to_wide(word);

                let has_err_exact = has_spelling_error(checker_vtbl, self.checker_obj, &wide_text);

                let is_all_caps = word.chars().any(char::is_alphabetic)
                    && word == word.to_uppercase();

                let mut is_misspelled = has_err_exact;

                // Windows spellcheck by default ignores all-caps words (to avoid flagging acronyms).
                // In CAD drawings, virtually all text is uppercase (e.g. CIRTUITS, VANNE).
                // If it wasn't flagged in all-caps, test its lowercase form to check if it's a real word.
                if !is_misspelled && is_all_caps {
                    let lower = word.to_lowercase();
                    let wide_lower = to_wide(&lower);
                    if has_spelling_error(checker_vtbl, self.checker_obj, &wide_lower) {
                        is_misspelled = true;
                    }
                }

                let mut suggestions = Vec::new();
                if is_misspelled && want_suggestions {
                    // Query suggestions for word and/or its lowercase counterpart
                    let mut enum_str: *mut c_void = std::ptr::null_mut();
                    let target_wide = if is_all_caps {
                        to_wide(&word.to_lowercase())
                    } else {
                        wide_text.clone()
                    };

                    if ((*checker_vtbl).suggest)(self.checker_obj, target_wide.as_ptr(), &mut enum_str) == 0
                        && !enum_str.is_null()
                    {
                        let enum_vtbl = *(enum_str as *const *const IEnumStringVtbl);
                        loop {
                            let mut str_ptr: *mut u16 = std::ptr::null_mut();
                            let mut fetched: u32 = 0;
                            if ((*enum_vtbl).next)(enum_str, 1, &mut str_ptr, &mut fetched) != 0
                                || fetched == 0
                            {
                                break;
                            }
                            let s = from_wide_ptr(str_ptr);
                            if !s.is_empty() {
                                // Match uppercase if original word was uppercase
                                let final_s = if is_all_caps { s.to_uppercase() } else { s };
                                if !suggestions.contains(&final_s) {
                                    suggestions.push(final_s);
                                }
                            }
                            if suggestions.len() >= 5 {
                                break;
                            }
                        }
                        ((*enum_vtbl).release)(enum_str);
                    }
                }

                (!is_misspelled, suggestions)
            }
        }
    }

    unsafe fn has_spelling_error(
        checker_vtbl: *const ISpellCheckerVtbl,
        checker_obj: *mut c_void,
        wide_text: &[u16],
    ) -> bool {
        let mut errors_obj: *mut c_void = std::ptr::null_mut();
        let hr_check = ((*checker_vtbl).check)(checker_obj, wide_text.as_ptr(), &mut errors_obj);
        let mut has_error = false;
        if hr_check == 0 && !errors_obj.is_null() {
            let enum_vtbl = *(errors_obj as *const *const IEnumSpellingErrorVtbl);
            let mut err_item: *mut c_void = std::ptr::null_mut();
            if ((*enum_vtbl).next)(errors_obj, &mut err_item) == 0 && !err_item.is_null() {
                has_error = true;
                ((*(*(err_item as *const *const ISpellingErrorVtbl))).release)(err_item);
            }
            ((*enum_vtbl).release)(errors_obj);
        }
        has_error
    }

    impl Drop for WindowsSpeller {
        fn drop(&mut self) {
            if !self.checker_obj.is_null() {
                unsafe {
                    let vtbl = *(self.checker_obj as *const *const ISpellCheckerVtbl);
                    ((*vtbl).release)(self.checker_obj);
                }
            }
        }
    }
}

// ============================================================================
// macOS Backend: NSSpellChecker via AppKit
// ============================================================================
#[cfg(target_os = "macos")]
pub(crate) mod macos_impl {
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::NSString;

    pub struct MacSpeller {
        checker: *mut AnyObject,
        language: String,
    }

    unsafe impl Send for MacSpeller {}
    unsafe impl Sync for MacSpeller {}

    impl MacSpeller {
        pub fn new(lang_tag: &str) -> Option<Self> {
            unsafe {
                let cls = AnyClass::get("NSSpellChecker")?;
                let checker: *mut AnyObject = msg_send![cls, sharedSpellChecker];
                if checker.is_null() {
                    return None;
                }

                let ns_lang = NSString::from_str(lang_tag);
                let ok: bool = msg_send![checker, setLanguage: &*ns_lang];
                let actual_lang = if ok {
                    lang_tag.to_string()
                } else {
                    let cur_lang: *mut AnyObject = msg_send![checker, language];
                    if !cur_lang.is_null() {
                        let cur: &NSString = &*(cur_lang as *const NSString);
                        cur.to_string()
                    } else {
                        lang_tag.to_string()
                    }
                };

                Some(Self {
                    checker,
                    language: actual_lang,
                })
            }
        }

        pub fn language(&self) -> &str {
            &self.language
        }

        pub fn check_word(&self, word: &str, want_suggestions: bool) -> (bool, Vec<String>) {
            if self.checker.is_null() || word.is_empty() {
                return (true, Vec::new());
            }

            unsafe {
                let is_all_caps = word.chars().any(char::is_alphabetic)
                    && word == word.to_uppercase();

                let ns_word = NSString::from_str(word);
                let range: objc2_foundation::NSRange = msg_send![
                    self.checker,
                    checkSpellingOfString: &*ns_word,
                    startingAt: 0isize,
                    language: std::ptr::null::<AnyObject>(),
                    wrap: false,
                    inSpellDocumentWithTag: 0isize,
                    wordCount: std::ptr::null_mut::<isize>()
                ];

                let mut is_misspelled = range.length > 0;

                // Also check lowercase if all-caps
                if !is_misspelled && is_all_caps {
                    let lower = word.to_lowercase();
                    let ns_lower = NSString::from_str(&lower);
                    let range_lower: objc2_foundation::NSRange = msg_send![
                        self.checker,
                        checkSpellingOfString: &*ns_lower,
                        startingAt: 0isize,
                        language: std::ptr::null::<AnyObject>(),
                        wrap: false,
                        inSpellDocumentWithTag: 0isize,
                        wordCount: std::ptr::null_mut::<isize>()
                    ];
                    if range_lower.length > 0 {
                        is_misspelled = true;
                    }
                }

                let mut suggestions = Vec::new();
                if is_misspelled && want_suggestions {
                    let target_str = if is_all_caps { word.to_lowercase() } else { word.to_string() };
                    let ns_target = NSString::from_str(&target_str);
                    let word_len = target_str.encode_utf16().count();
                    let r = objc2_foundation::NSRange {
                        location: 0,
                        length: word_len,
                    };
                    let arr: *mut AnyObject = msg_send![
                        self.checker,
                        guessesForWordRange: r,
                        inString: &*ns_target,
                        language: std::ptr::null::<AnyObject>(),
                        inSpellDocumentWithTag: 0isize
                    ];
                    if !arr.is_null() {
                        let count: usize = msg_send![arr, count];
                        for i in 0..count.min(5) {
                            let item: *mut AnyObject = msg_send![arr, objectAtIndex: i];
                            if !item.is_null() {
                                let ns_item: &NSString = &*(item as *const NSString);
                                let mut s = ns_item.to_string();
                                if is_all_caps {
                                    s = s.to_uppercase();
                                }
                                suggestions.push(s);
                            }
                        }
                    }
                }

                (!is_misspelled, suggestions)
            }
        }
    }
}

// ============================================================================
// Linux Backend: System Dictionaries & Hunspell / Aspell
// ============================================================================
#[cfg(target_os = "linux")]
pub(crate) mod linux_impl {
    use std::collections::HashSet;
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    const LINUX_DICT_DIRS: &[&str] = &[
        "/usr/share/hunspell",
        "/usr/share/myspell",
        "/usr/share/myspell/dicts",
        "/var/lib/dictionaries-common/hunspell_dic",
        "/usr/share/dict",
    ];

    pub struct LinuxSpeller {
        words: HashSet<String>,
        language: String,
        has_cli_tool: bool,
    }

    impl LinuxSpeller {
        pub fn new(lang_tag: &str) -> Option<Self> {
            let mut words = HashSet::new();
            let mut found_any = false;

            // Search system directories for .dic or wordlist files matching the requested language
            let short_lang = lang_tag.split('-').next().unwrap_or(lang_tag);
            let candidates = [
                format!("{}.dic", lang_tag.replace('-', "_")),
                format!("{}.dic", lang_tag),
                format!("{}.dic", short_lang),
                "words".to_string(),
                short_lang.to_string(),
            ];

            for dir in LINUX_DICT_DIRS {
                let base = Path::new(dir);
                if !base.is_dir() {
                    continue;
                }
                for candidate in &candidates {
                    let path = base.join(candidate);
                    if path.is_file() {
                        if load_wordlist(&path, &mut words) {
                            found_any = true;
                            break;
                        }
                    }
                }
                if found_any {
                    break;
                }
            }

            // Only hunspell is ever invoked (`check_via_cli`), so only it counts.
            let has_cli = is_command_in_path("hunspell");

            if !found_any && !has_cli {
                return None;
            }

            Some(Self {
                words,
                language: lang_tag.to_string(),
                has_cli_tool: has_cli,
            })
        }

        pub fn is_available(&self) -> bool {
            !self.words.is_empty() || self.has_cli_tool
        }

        pub fn language(&self) -> &str {
            &self.language
        }

        pub fn check_word(&self, word: &str, want_suggestions: bool) -> (bool, Vec<String>) {
            let clean = word.trim().to_lowercase();
            if clean.is_empty() {
                return (true, Vec::new());
            }

            if self.words.contains(&clean) {
                return (true, Vec::new());
            }

            // If CLI hunspell is available, fallback to pipe check
            if self.has_cli_tool {
                if let Some((valid, suggs)) = check_via_cli(word, &self.language, want_suggestions) {
                    return (valid, suggs);
                }
            }

            // Not in the wordlist and no CLI answer: misspelled when there is
            // a wordlist to judge by, unjudged (accepted) when there is none.
            (self.words.is_empty(), Vec::new())
        }
    }

    fn load_wordlist(path: &Path, words: &mut HashSet<String>) -> bool {
        let Ok(file) = File::open(path) else { return false };
        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            let entry = line.trim();
            if entry.is_empty() || entry.starts_with('#') {
                continue;
            }
            // Strip Hunspell flags: "word/flags"
            let word_part = entry.split('/').next().unwrap_or(entry).trim();
            if !word_part.is_empty() && !word_part.chars().all(|c| c.is_ascii_digit()) {
                words.insert(word_part.to_lowercase());
            }
        }
        !words.is_empty()
    }

    fn is_command_in_path(cmd: &str) -> bool {
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                if dir.join(cmd).is_file() {
                    return true;
                }
            }
        }
        false
    }

    fn check_via_cli(word: &str, lang: &str, want_suggestions: bool) -> Option<(bool, Vec<String>)> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let lang_param = lang.replace('-', "_");
        let mut child = Command::new("hunspell")
            .arg("-d")
            .arg(&lang_param)
            .arg("-a")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = writeln!(stdin, "^{word}");
        }

        let output = child.wait_with_output().ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with('*') || line.starts_with('+') || line.starts_with('-') {
                return Some((true, Vec::new()));
            }
            if line.starts_with('&') {
                let mut suggs = Vec::new();
                if want_suggestions {
                    if let Some(pos) = line.find(':') {
                        for s in line[pos + 1..].split(',') {
                            let item = s.trim();
                            if !item.is_empty() && suggs.len() < 5 {
                                suggs.push(item.to_string());
                            }
                        }
                    }
                }
                return Some((false, suggs));
            }
        }
        None
    }
}

// ============================================================================
// Fallback Backend
// ============================================================================
mod fallback_impl {
    pub struct FallbackSpeller;
}
