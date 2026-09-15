//! Normalisation of SVG sources into the shape used by `index.ts`.
//!
//! Every inline icon in the index is a single line, sizes itself with
//! `width="1em" height="1em"`, carries `style="flex:none;line-height:1"` and
//! starts with a `<title>` element. This mirrors the upstream
//! `@lobehub/icons-static-svg` conventions so custom icons blend in.

/// Style attribute applied to every inline icon.
pub const ICON_STYLE: &str = "flex:none;line-height:1";

/// Normalises an SVG document for inclusion in `index.ts`.
///
/// * strips a leading BOM and any `<?xml ... ?>` declaration
/// * joins all lines, collapsing indentation and inter-tag whitespace
/// * forces `width`/`height` to `1em` and adds the shared `style`
/// * inserts `<title>` with `title` when the document has none
pub fn normalize(svg: &str, title: &str) -> String {
    let stripped = strip_declaration(svg.trim_start_matches('\u{feff}'));
    let flat = flatten(&stripped);
    let Some((before, tag, after)) = split_root_tag(&flat) else {
        return flat;
    };
    let tag = normalize_root_tag(tag);
    let mut out = String::with_capacity(flat.len() + 64);
    out.push_str(before);
    out.push_str(&tag);
    if !after.contains("<title>") && !after.contains("<title ") {
        out.push_str("<title>");
        out.push_str(&escape_xml_text(title));
        out.push_str("</title>");
    }
    out.push_str(after);
    out
}

/// Removes an XML declaration at the start of the document.
fn strip_declaration(svg: &str) -> String {
    let trimmed = svg.trim_start();
    if let Some(rest) = trimmed.strip_prefix("<?xml") {
        if let Some(end) = rest.find("?>") {
            return rest[end + 2..].to_string();
        }
    }
    trimmed.to_string()
}

/// Joins lines into one, then removes whitespace between adjacent tags.
fn flatten(svg: &str) -> String {
    let joined = svg
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = String::with_capacity(joined.len());
    let mut chars = joined.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '>' {
            out.push('>');
            // Drop whitespace that only separates two tags.
            let mut lookahead = chars.clone();
            let mut skipped = 0usize;
            while matches!(lookahead.peek(), Some(c) if c.is_whitespace()) {
                lookahead.next();
                skipped += 1;
            }
            if skipped > 0 && lookahead.peek() == Some(&'<') {
                chars = lookahead;
            }
        } else {
            out.push(ch);
        }
    }
    out.replace(" />", "/>")
}

/// Splits the document into (prefix, root `<svg ...>` tag, remainder).
fn split_root_tag(svg: &str) -> Option<(&str, &str, &str)> {
    let start = svg.find("<svg")?;
    let after_start = &svg[start..];
    // Make sure this is the `<svg` element and not e.g. `<svgfoo`.
    let next = after_start.chars().nth(4)?;
    if !(next.is_whitespace() || next == '>' || next == '/') {
        return None;
    }
    let end = after_start.find('>')? + start;
    Some((&svg[..start], &svg[start..=end], &svg[end + 1..]))
}

/// An attribute of the root tag with its original ordering preserved.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Attr {
    name: String,
    value: String,
}

/// Parses `name="value"` / `name='value'` pairs of a start tag.
fn parse_attrs(tag: &str) -> Vec<Attr> {
    let inner = tag
        .trim_start_matches("<svg")
        .trim_end_matches('>')
        .trim_end_matches('/');
    let mut attrs = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let name_start = i;
        while i < chars.len() && chars[i] != '=' && !chars[i].is_whitespace() {
            i += 1;
        }
        let name: String = chars[name_start..i].iter().collect();
        if name.is_empty() {
            break;
        }
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let quote = if i < chars.len() { chars[i] } else { '"' };
            if quote == '"' || quote == '\'' {
                i += 1;
                let value_start = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                let value: String = chars[value_start..i].iter().collect();
                i += 1;
                attrs.push(Attr { name, value });
            } else {
                let value_start = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                let value: String = chars[value_start..i].iter().collect();
                attrs.push(Attr { name, value });
            }
        } else {
            attrs.push(Attr {
                name,
                value: String::new(),
            });
        }
    }
    attrs
}

/// Rewrites the root tag with normalised sizing and style attributes.
fn normalize_root_tag(tag: &str) -> String {
    let self_closing = tag.trim_end_matches('>').ends_with('/');
    let mut attrs = parse_attrs(tag);
    for attr in attrs.iter_mut() {
        if (attr.name == "width" || attr.name == "height") && !attr.value.ends_with("em") {
            attr.value = "1em".to_string();
        }
    }
    for name in ["width", "height"] {
        if !attrs.iter().any(|a| a.name == name) {
            attrs.push(Attr {
                name: name.to_string(),
                value: "1em".to_string(),
            });
        }
    }
    if !attrs.iter().any(|a| a.name == "style") {
        attrs.push(Attr {
            name: "style".to_string(),
            value: ICON_STYLE.to_string(),
        });
    }
    if !attrs.iter().any(|a| a.name == "xmlns") {
        attrs.push(Attr {
            name: "xmlns".to_string(),
            value: "http://www.w3.org/2000/svg".to_string(),
        });
    }
    let mut out = String::from("<svg");
    for attr in attrs {
        out.push(' ');
        out.push_str(&attr.name);
        out.push_str("=\"");
        out.push_str(&attr.value.replace('"', "&quot;"));
        out.push('"');
    }
    if self_closing {
        out.push('/');
    }
    out.push('>');
    out
}

/// Escapes text for use as XML character data.
fn escape_xml_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Whether an SVG file should be imported by URL instead of inlined.
///
/// Files that embed raster data or are very large are kept out of the
/// JavaScript bundle and served as assets, matching the existing index.
pub fn prefers_url(svg: &str) -> bool {
    svg.contains("<image") || svg.len() > 32 * 1024
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_multiline_custom_icon() {
        let raw = "<svg width=\"128\" height=\"128\" viewBox=\"0 0 128 128\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\">\n  <path\n    d=\"M4 96\n       C4 96, 24 12, 64 12\n       Z\"\n    fill=\"currentColor\"\n  />\n</svg>\n";
        assert_eq!(
            normalize(raw, "Amux"),
            "<svg width=\"1em\" height=\"1em\" viewBox=\"0 0 128 128\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\" style=\"flex:none;line-height:1\"><title>Amux</title><path d=\"M4 96 C4 96, 24 12, 64 12 Z\" fill=\"currentColor\"/></svg>"
        );
    }

    #[test]
    fn strips_declaration_and_keeps_existing_title() {
        let raw = "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 24 24'>\n<title>Kept</title>\n<g/>\n</svg>";
        assert_eq!(
            normalize(raw, "Ignored"),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"1em\" height=\"1em\" style=\"flex:none;line-height:1\"><title>Kept</title><g/></svg>"
        );
    }

    #[test]
    fn lobehub_icons_pass_through_unchanged() {
        let upstream = "<svg fill=\"currentColor\" fill-rule=\"evenodd\" height=\"1em\" style=\"flex:none;line-height:1\" viewBox=\"0 0 24 24\" width=\"1em\" xmlns=\"http://www.w3.org/2000/svg\"><title>OpenAI</title><path d=\"M1 1\"/></svg>";
        assert_eq!(normalize(upstream, "OpenAI"), upstream);
    }

    #[test]
    fn escapes_title_text() {
        let raw = "<svg viewBox=\"0 0 1 1\"></svg>";
        let out = normalize(raw, "A & B <C>");
        assert!(out.contains("<title>A &amp; B &lt;C&gt;</title>"));
    }

    #[test]
    fn keeps_text_content_spacing() {
        let raw = "<svg viewBox=\"0 0 1 1\"><text>hello world</text></svg>";
        assert!(normalize(raw, "T").contains("<text>hello world</text>"));
    }

    #[test]
    fn non_svg_input_is_returned_flattened() {
        assert_eq!(normalize("<div>\n  x\n</div>", "T"), "<div> x </div>");
    }

    #[test]
    fn detects_url_candidates() {
        assert!(prefers_url("<svg><image href=\"data:...\"/></svg>"));
        assert!(!prefers_url("<svg/>"));
        assert!(prefers_url(&"x".repeat(40 * 1024)));
    }
}
