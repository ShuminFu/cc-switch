use dioxus::prelude::*;

use crate::icons;

/// Provider/brand icon, ported from `src/components/ProviderIcon.tsx`:
/// inline SVG (coloured through `currentColor`), an `<img>` for URL icons,
/// or initials as a fallback.
#[component]
pub fn ProviderIcon(
    #[props(default)] icon: Option<String>,
    name: String,
    #[props(default)] color: Option<String>,
    #[props(default = 32)] size: u32,
    #[props(default)] class: String,
    #[props(default = true)] show_fallback: bool,
) -> Element {
    let icon_name = icon.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let effective_color = color
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string)
        .or_else(|| {
            icon_name
                .and_then(icons::icon_metadata)
                .and_then(|m| m.default_color)
                .filter(|c| *c != "currentColor")
                .map(str::to_string)
        });
    let color_style = effective_color
        .map(|c| format!("; color: {c}"))
        .unwrap_or_default();
    let size_style =
        format!("width: {size}px; height: {size}px; font-size: {size}px; line-height: 1");

    if let Some(svg) = icon_name
        .filter(|n| !icons::is_url_icon(n))
        .and_then(icons::get_icon)
    {
        return rsx! {
            span {
                class: "inline-flex items-center justify-center flex-shrink-0 {class}",
                title: "{name}",
                style: "{size_style}{color_style}",
                dangerous_inner_html: "{svg}",
            }
        };
    }
    if let Some(url) = icon_name.and_then(icons::get_icon_url) {
        return rsx! {
            img {
                src: "{url}",
                alt: "{name}",
                title: "{name}",
                class: "inline-flex items-center justify-center flex-shrink-0 object-contain {class}",
                style: "width: {size}px; height: {size}px",
                loading: "lazy",
            }
        };
    }
    if !show_fallback {
        return rsx! {};
    }
    let initials: String = name
        .split(' ')
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_uppercase()
        .chars()
        .take(2)
        .collect();
    let font_size = ((size as f32) * 0.5).max(12.0);
    rsx! {
        span {
            class: "inline-flex items-center justify-center flex-shrink-0 rounded-lg bg-muted text-muted-foreground font-semibold {class}",
            title: "{name}",
            style: "{size_style}",
            span { style: "font-size: {font_size}px", "{initials}" }
        }
    }
}
