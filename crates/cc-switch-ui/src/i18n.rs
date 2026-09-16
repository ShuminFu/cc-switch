//! Translations. The four locale files are the React app's JSON files,
//! embedded at compile time and flattened to `section.key` paths. Only the
//! i18next features the app uses are implemented: nested keys, `{{name}}`
//! interpolation and fallback to English.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Locale {
    En,
    Zh,
    ZhTw,
    Ja,
}

impl Locale {
    pub const ALL: [Locale; 4] = [Locale::Zh, Locale::ZhTw, Locale::En, Locale::Ja];
    pub const DEFAULT: Locale = Locale::Zh;
    pub const FALLBACK: Locale = Locale::En;

    /// Code as stored in settings / localStorage (`"zh-TW"`, `"en"`, ...).
    pub fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Zh => "zh",
            Locale::ZhTw => "zh-TW",
            Locale::Ja => "ja",
        }
    }

    pub fn from_code(code: &str) -> Option<Locale> {
        Locale::ALL.into_iter().find(|l| l.code() == code)
    }

    /// Maps a BCP-47 tag from the browser to a supported locale, mirroring
    /// `getInitialLanguage()` in the React app.
    pub fn from_navigator(tag: &str) -> Option<Locale> {
        let lower = tag.to_ascii_lowercase();
        if lower.starts_with("zh-tw")
            || lower.starts_with("zh-hk")
            || lower.starts_with("zh-mo")
            || lower.starts_with("zh-hant")
        {
            Some(Locale::ZhTw)
        } else if lower.starts_with("zh") {
            Some(Locale::Zh)
        } else if lower.starts_with("ja") {
            Some(Locale::Ja)
        } else if lower.starts_with("en") {
            Some(Locale::En)
        } else {
            None
        }
    }

    fn raw_json(self) -> &'static str {
        match self {
            Locale::En => include_str!("../../../src/i18n/locales/en.json"),
            Locale::Zh => include_str!("../../../src/i18n/locales/zh.json"),
            Locale::ZhTw => include_str!("../../../src/i18n/locales/zh-TW.json"),
            Locale::Ja => include_str!("../../../src/i18n/locales/ja.json"),
        }
    }
}

type Table = HashMap<String, String>;

fn flatten(value: &serde_json::Value, prefix: &str, out: &mut Table) {
    match value {
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(v, &key, out);
            }
        }
        serde_json::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        other => {
            out.insert(prefix.to_string(), other.to_string());
        }
    }
}

fn table(locale: Locale) -> &'static Table {
    static TABLES: OnceLock<HashMap<Locale, Table>> = OnceLock::new();
    let tables = TABLES.get_or_init(|| {
        Locale::ALL
            .into_iter()
            .map(|l| {
                let value: serde_json::Value =
                    serde_json::from_str(l.raw_json()).expect("locale JSON is valid");
                let mut out = Table::new();
                flatten(&value, "", &mut out);
                (l, out)
            })
            .collect()
    });
    &tables[&locale]
}

/// Looks up `key` in `locale`, falling back to English and finally to the
/// key itself (what i18next does), then applies `{{name}}` substitutions.
pub fn translate(locale: Locale, key: &str, args: &[(&str, &str)]) -> String {
    let raw = table(locale)
        .get(key)
        .or_else(|| table(Locale::FALLBACK).get(key))
        .map(String::as_str)
        .unwrap_or(key);
    interpolate(raw, args)
}

/// Replaces every `{{name}}` occurrence with the matching argument. Unknown
/// placeholders are left untouched, like i18next's default behaviour.
pub fn interpolate(template: &str, args: &[(&str, &str)]) -> String {
    if args.is_empty() || !template.contains("{{") {
        return template.to_string();
    }
    let mut out = template.to_string();
    for (name, value) in args {
        out = out.replace(&format!("{{{{{name}}}}}"), value);
    }
    out
}

/// `true` when the key exists in the given locale or in the fallback.
pub fn has_key(locale: Locale, key: &str) -> bool {
    table(locale).contains_key(key) || table(Locale::FALLBACK).contains_key(key)
}

/// All keys of the fallback locale, for tooling and tests.
pub fn fallback_keys() -> impl Iterator<Item = &'static str> {
    table(Locale::FALLBACK).keys().map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_load_and_share_keys() {
        let en = table(Locale::En);
        assert!(en.len() > 2000, "{} keys", en.len());
        for locale in [Locale::Zh, Locale::Ja] {
            let t = table(locale);
            let missing: Vec<&String> = en.keys().filter(|k| !t.contains_key(*k)).collect();
            assert!(missing.is_empty(), "{locale:?} missing {missing:?}");
        }
    }

    #[test]
    fn translates_with_fallback_and_interpolation() {
        assert_eq!(interpolate("Hi {{name}}!", &[("name", "Ada")]), "Hi Ada!");
        assert_eq!(interpolate("{{a}}{{b}}", &[("a", "1")]), "1{{b}}");
        assert_eq!(
            translate(Locale::En, "does.not.exist", &[]),
            "does.not.exist"
        );
        let key = fallback_keys()
            .find(|k| table(Locale::En)[*k].contains("{{count}}"))
            .expect("a key with count");
        let rendered = translate(Locale::En, key, &[("count", "3")]);
        assert!(
            rendered.contains('3') && !rendered.contains("{{count}}"),
            "{rendered}"
        );
        // zh-TW is slightly behind; missing keys fall back to English.
        let behind: Vec<&str> = fallback_keys()
            .filter(|k| !table(Locale::ZhTw).contains_key(*k))
            .collect();
        for k in behind {
            assert_eq!(translate(Locale::ZhTw, k, &[]), table(Locale::En)[k]);
        }
    }

    #[test]
    fn navigator_mapping_matches_react() {
        assert_eq!(Locale::from_navigator("zh-TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_navigator("zh-Hant-HK"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_navigator("zh-CN"), Some(Locale::Zh));
        assert_eq!(Locale::from_navigator("ja-JP"), Some(Locale::Ja));
        assert_eq!(Locale::from_navigator("en-US"), Some(Locale::En));
        assert_eq!(Locale::from_navigator("fr"), None);
        assert_eq!(Locale::from_code("zh-TW"), Some(Locale::ZhTw));
    }
}
