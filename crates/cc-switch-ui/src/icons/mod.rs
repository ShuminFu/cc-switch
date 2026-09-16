//! Provider/brand icon set, generated from `src/icons/extracted` by
//! `icon-tools emit-rust` (`generated.rs`). Mirrors `src/icons/extracted/index.ts`.

#[rustfmt::skip]
mod generated;

pub use generated::IconMeta;

/// Inline SVG markup for `name`, if it is an inline icon.
pub fn get_icon(name: &str) -> Option<&'static str> {
    let key = name.to_lowercase();
    generated::INLINE
        .binary_search_by(|(k, _)| k.cmp(&key.as_str()))
        .ok()
        .map(|i| generated::INLINE[i].1)
}

/// Asset URL for `name`, if it is served as an image.
pub fn get_icon_url(name: &str) -> Option<String> {
    generated::url_icon(&name.to_lowercase()).map(|a| a.to_string())
}

pub fn is_url_icon(name: &str) -> bool {
    let key = name.to_lowercase();
    generated::URL_ICON_NAMES.contains(&key.as_str())
}

pub fn has_icon(name: &str) -> bool {
    get_icon(name).is_some() || is_url_icon(name)
}

/// Every icon name, sorted, like `iconList` in the React app.
pub fn icon_list() -> Vec<&'static str> {
    let mut all: Vec<&'static str> = generated::INLINE.iter().map(|(k, _)| *k).collect();
    all.extend(generated::URL_ICON_NAMES.iter().copied());
    all.sort_unstable();
    all
}

pub fn icon_metadata(name: &str) -> Option<&'static IconMeta> {
    let key = name.to_lowercase();
    generated::METADATA.iter().find(|m| m.name == key)
}

/// Icons whose name, display name or keywords contain `query`
/// (case-insensitive), like `searchIcons`.
pub fn search_icons(query: &str) -> Vec<&'static str> {
    let q = query.to_lowercase();
    generated::METADATA
        .iter()
        .filter(|m| {
            m.name.contains(&q)
                || m.display_name.to_lowercase().contains(&q)
                || m.keywords.iter().any(|k| k.contains(&q))
        })
        .map(|m| m.name)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_table_is_sorted_for_binary_search() {
        let keys: Vec<&str> = generated::INLINE.iter().map(|(k, _)| *k).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_eq!(keys, sorted);
    }

    #[test]
    fn lookups_match_the_react_module() {
        assert!(has_icon("openai"));
        assert!(has_icon("OpenAI"));
        assert!(
            has_icon("subrouter"),
            "port of subrouterProviderPresets.test.ts"
        );
        assert!(is_url_icon("subrouter"));
        assert!(!is_url_icon("openai"));
        assert!(get_icon("openai").unwrap().starts_with("<svg"));
        assert!(!has_icon("definitely-missing"));
        assert_eq!(
            icon_list().len(),
            generated::INLINE.len() + generated::URL_ICON_NAMES.len()
        );
        assert_eq!(icon_metadata("openai").unwrap().display_name, "OpenAI");
        assert!(search_icons("chatgpt").contains(&"openai"));
        assert!(search_icons("阶跃").contains(&"stepfun"));
    }
}
