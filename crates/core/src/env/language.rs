use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ts_rs::TS;

/// The language picked under Omarchy's Setup > Language.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Language {
    /// Omarchy language code, e.g. "en", "da", "pt-BR".
    pub code: String,
    /// BCP 47 locale for number and date formatting, e.g. "da-DK".
    pub format_locale: String,
    pub rtl: bool,
}

pub fn language_file(home: &Path) -> PathBuf {
    home.join(".config/omarchy/language")
}

pub fn locales_file(omarchy_path: &Path) -> PathBuf {
    omarchy_path.join("default/i18n/locales.json")
}

const RTL: &[&str] = &["ar", "he", "fa", "ur"];

/// Reads the language from `language_file`, looking up formatting details in Omarchy's
/// registry when available. Missing files mean English.
pub fn read_language(language_file: &Path, locales_file: &Path) -> Language {
    let code = std::fs::read_to_string(language_file)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "en".into());
    let registry: Option<serde_json::Value> = std::fs::read_to_string(locales_file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let entry = registry.as_ref().and_then(|r| r.get(&code));
    let format_locale = entry
        .and_then(|e| e.get("formatLocale"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| code.clone());
    let base = code.split('-').next().unwrap_or(&code);
    let rtl = entry
        .and_then(|e| e.get("direction"))
        .and_then(|v| v.as_str())
        .map(|d| d == "rtl")
        .unwrap_or(RTL.contains(&base));
    Language {
        code,
        format_locale,
        rtl,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_english_and_reads_registry() {
        let dir = tempfile::tempdir().unwrap();
        let lang = dir.path().join("language");
        let reg = dir.path().join("locales.json");
        assert_eq!(read_language(&lang, &reg).code, "en");
        std::fs::write(&reg, r#"{"ar": {"formatLocale": "ar-AE", "direction": "rtl"}, "da": {"formatLocale": "da-DK"}}"#).unwrap();
        std::fs::write(&lang, "da\n").unwrap();
        let l = read_language(&lang, &reg);
        assert_eq!(
            (l.code.as_str(), l.format_locale.as_str(), l.rtl),
            ("da", "da-DK", false)
        );
        std::fs::write(&lang, "ar").unwrap();
        assert!(read_language(&lang, &reg).rtl);
    }
}
