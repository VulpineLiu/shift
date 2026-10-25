//! Which language the interface should use before the webview can say otherwise.
//!
//! The tray menu is built in Rust, so it needs the same language the UI shows.
//! A language chosen in Settings wins; otherwise the operating system's own UI
//! language decides, which is what the webview does on a first run.

pub const ENGLISH: &str = "en";
pub const CHINESE: &str = "zh";

/// Normalise anything language-shaped ("zh", "zh-CN", "zh_Hans") to a UI language.
pub fn normalize(candidate: Option<&str>) -> Option<&'static str> {
    let value = candidate?.trim().to_ascii_lowercase();
    if value.is_empty() {
        return None;
    }
    if value.starts_with("zh") {
        return Some(CHINESE);
    }
    Some(ENGLISH)
}

/// The operating system's UI language, as a UI language.
#[cfg(not(target_os = "windows"))]
pub fn system_language() -> &'static str {
    normalize(Some(&system_locale())).unwrap_or(ENGLISH)
}

/// The operating system's UI language, as a UI language.
#[cfg(target_os = "windows")]
pub fn system_language() -> &'static str {
    system_language_windows()
}

/// The first locale in the `AppleLanguages` list, which `defaults` prints as a
/// parenthesised list with the most preferred locale first.
#[cfg(target_os = "macos")]
pub fn first_apple_locale(text: &str) -> Option<&str> {
    text.lines()
        .map(|line| line.trim().trim_end_matches(',').trim())
        .filter(|entry| *entry != "(" && *entry != ")")
        .map(|entry| entry.trim_matches('"').trim())
        .find(|entry| !entry.is_empty())
}

#[cfg(target_os = "macos")]
fn system_locale() -> String {
    // `AppleLanguages` is what System Settings writes, and it survives the
    // stripped environment a LaunchAgent starts the app with.
    if let Ok(output) = crate::process::output("defaults", &["read", "-g", "AppleLanguages"]) {
        if output.status.success() {
            let text = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = first_apple_locale(&text) {
                return first.to_string();
            }
        }
    }
    std::env::var("LANG").unwrap_or_default()
}

/// `LANG_CHINESE` from the Windows language identifiers: one primary language
/// covers every regional Chinese, the sub-language only picks the script.
#[cfg(target_os = "windows")]
const WINDOWS_LANG_CHINESE: u16 = 0x04;

/// Map a Windows UI LANGID to a UI language. The primary language sits in the
/// low 10 bits; the sub-language is deliberately ignored because Shift offers one
/// Chinese, the same reading the webview takes from `navigator.language`.
#[cfg(target_os = "windows")]
pub fn from_windows_langid(langid: u16) -> &'static str {
    if (langid & 0x03FF) == WINDOWS_LANG_CHINESE {
        CHINESE
    } else {
        ENGLISH
    }
}

#[cfg(target_os = "windows")]
fn system_language_windows() -> &'static str {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;

    from_windows_langid(unsafe { GetUserDefaultUILanguage() })
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn system_locale() -> String {
    std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_MESSAGES"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default()
}

/// A stored setting only counts when it is one of the two UI languages.
pub fn from_setting(stored: Option<&str>) -> Option<&'static str> {
    match stored.map(str::trim).map(str::to_ascii_lowercase) {
        Some(value) if value == CHINESE => Some(CHINESE),
        Some(value) if value == ENGLISH => Some(ENGLISH),
        _ => None,
    }
}

/// The language the tray and any other native surface should use.
pub fn preferred(stored: Option<&str>) -> &'static str {
    from_setting(stored).unwrap_or_else(system_language)
}

/// The tray row that advertises a newer release, e.g. "Update available 0.2.0".
pub fn update_available(language: &str, version: &str) -> String {
    format!("{}{}", tray_titles(language).update, version)
}

/// Tray titles. Kept here rather than in the webview bundle because the tray has
/// to be right before the first window has loaded.
pub struct TrayTitles {
    pub open: &'static str,
    pub capture: &'static str,
    pub fullscreen: &'static str,
    pub record: &'static str,
    pub ocr: &'static str,
    /// Suffix for the update row, which the caller formats with a version number.
    pub update: &'static str,
    pub quit: &'static str,
}

pub fn tray_titles(language: &str) -> TrayTitles {
    if language == CHINESE {
        TrayTitles {
            open: "打开 Shift",
            capture: "区域截图",
            fullscreen: "全屏截图",
            record: "区域录屏",
            ocr: "识别图片文字…",
            update: "发现新版本 ",
            quit: "退出 Shift",
        }
    } else {
        TrayTitles {
            open: "Open Shift",
            capture: "Capture area",
            fullscreen: "Capture full screen",
            record: "Record area",
            ocr: "OCR an image…",
            update: "Update available ",
            quit: "Quit Shift",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_update_row_carries_the_version() {
        assert_eq!(update_available(CHINESE, "0.2.0"), "发现新版本 0.2.0");
        assert_eq!(update_available(ENGLISH, "0.2.0"), "Update available 0.2.0");
    }

    #[test]
    fn chinese_variants_all_select_chinese() {
        for candidate in ["zh", "zh-CN", "zh-Hans", "zh_TW", " ZH-Hant "] {
            assert_eq!(normalize(Some(candidate)), Some(CHINESE), "{candidate}");
        }
    }

    #[test]
    fn everything_else_selects_english() {
        for candidate in ["en", "en-GB", "de", "ja", "fr-CA"] {
            assert_eq!(normalize(Some(candidate)), Some(ENGLISH), "{candidate}");
        }
    }

    #[test]
    fn an_empty_or_missing_candidate_is_no_preference() {
        assert_eq!(normalize(None), None);
        assert_eq!(normalize(Some("")), None);
        assert_eq!(normalize(Some("   ")), None);
    }

    #[test]
    fn only_the_two_ui_languages_can_be_persisted() {
        assert_eq!(from_setting(Some("zh")), Some(CHINESE));
        assert_eq!(from_setting(Some("en")), Some(ENGLISH));
        // A locale rather than a UI language, or a stray value, falls back to the OS.
        assert_eq!(from_setting(Some("zh-CN")), None);
        assert_eq!(from_setting(Some("german")), None);
        assert_eq!(from_setting(None), None);
    }

    #[test]
    fn a_stored_choice_beats_the_system_language() {
        assert_eq!(preferred(Some("zh")), CHINESE);
        assert_eq!(preferred(Some("en")), ENGLISH);
        assert!(matches!(preferred(None), ENGLISH | CHINESE));
    }

    #[test]
    fn the_tray_speaks_whichever_language_was_chosen() {
        let chinese = tray_titles(CHINESE);
        assert_eq!(chinese.quit, "退出 Shift");
        assert_eq!(chinese.capture, "区域截图");
        let english = tray_titles(ENGLISH);
        assert_eq!(english.quit, "Quit Shift");
        assert_eq!(english.capture, "Capture area");
        // Chinese is the only other language, so anything else means English.
        assert_eq!(tray_titles("de").open, english.open);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn every_windows_chinese_langid_selects_chinese() {
        // zh-CN, zh-TW and zh-SG share one primary language and differ only in
        // sub-language; anything else, including lookalike ids, stays English.
        for langid in [0x0804_u16, 0x0404, 0x1004] {
            assert_eq!(from_windows_langid(langid), CHINESE, "{langid:#06x}");
        }
        // 0x0409 en-US, 0x0407 de-DE, 0x0415 pl-PL, 0x0419 ru-RU, 0x040C fr-FR.
        for langid in [0x0409_u16, 0x0407, 0x0415, 0x0419, 0x040C] {
            assert_eq!(from_windows_langid(langid), ENGLISH, "{langid:#06x}");
        }
    }

    #[test]
    fn the_system_language_is_always_a_ui_language() {
        assert!(matches!(system_language(), ENGLISH | CHINESE));
    }
}

#[cfg(all(test, target_os = "macos"))]
mod apple_locale_parsing {
    use super::*;

    #[test]
    fn the_preferred_locale_wins() {
        let listing = "(\n    \"zh-Hans-CN\",\n    \"en-US\"\n)\n";
        assert_eq!(first_apple_locale(listing), Some("zh-Hans-CN"));
        assert_eq!(
            normalize(Some(first_apple_locale(listing).unwrap())),
            Some(CHINESE)
        );
    }

    #[test]
    fn an_english_first_list_stays_english() {
        let listing = "(\n    \"en-US\",\n    \"fr\"\n)\n";
        assert_eq!(normalize(first_apple_locale(listing)), Some(ENGLISH));
    }

    #[test]
    fn a_one_entry_list_still_parses() {
        let listing = "(\n    \"zh-Hans-CN\"\n)\n";
        assert_eq!(first_apple_locale(listing), Some("zh-Hans-CN"));
    }

    #[test]
    fn nothing_useful_is_no_preference() {
        assert_eq!(first_apple_locale("(\n)\n"), None);
        assert_eq!(first_apple_locale(""), None);
        assert_eq!(first_apple_locale("(\n    \"\"\n)\n"), None);
    }
}
