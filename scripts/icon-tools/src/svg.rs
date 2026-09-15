//! SVG normalisation for inline embedding in `index.ts`.
//!
//! The curated index stores every SVG on a single line, without XML
//! prologue, doctype or comments, with the root element sized to `1em` and
//! carrying the `flex:none;line-height:1` style used throughout the UI.

/// Style applied to the root `<svg>` element when it has none.
pub const ROOT_STYLE: &str = "flex:none;line-height:1";

/// Size applied to the root `<svg>` element's `width`/`height`.
pub const ROOT_SIZE: &str = "1em";

/// Removes the XML prologue, doctype and comments, then collapses
/// whitespace so the markup fits on one line.
pub fn strip_and_collapse(input: &str) -> String {
    let without_bom = input.strip_prefix('\u{feff}').unwrap_or(input);
    let stripped = strip_non_element_nodes(without_bom);
    collapse_whitespace(&stripped)
}

/// Full normalisation: [`strip_and_collapse`] followed by
/// [`apply_root_attributes`].
pub fn normalize(input: &str) -> String {
    apply_root_attributes(&strip_and_collapse(input))
}

/// Escapes `\`, `` ` `` and `$` so the text can be embedded in a JavaScript
/// template literal.
pub fn escape_template_literal(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '$' => out.push_str("\\$"),
            other => out.push(other),
        }
    }
    out
}

/// Reverses [`escape_template_literal`] for the escapes it produces.
pub fn unescape_template_literal(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('\\') | Some('`') | Some('$') => {
                    out.push(chars.next().unwrap());
                    continue;
                }
                _ => {}
            }
        }
        out.push(c);
    }
    out
}

fn strip_non_element_nodes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find('<') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        if let Some(after) = tail.strip_prefix("<?") {
            rest = skip_past(after, "?>");
        } else if let Some(after) = tail.strip_prefix("<!--") {
            rest = skip_past(after, "-->");
        } else if let Some(after) = tail.strip_prefix("<!") {
            rest = skip_doctype(after);
        } else {
            out.push('<');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

fn skip_past<'a>(input: &'a str, terminator: &str) -> &'a str {
    match input.find(terminator) {
        Some(pos) => &input[pos + terminator.len()..],
        None => "",
    }
}

/// Skips a `<!DOCTYPE ...>` declaration, honouring an internal subset in
/// square brackets.
fn skip_doctype(input: &str) -> &str {
    let mut depth = 0usize;
    for (idx, c) in input.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '>' if depth == 0 => return &input[idx + 1..],
            _ => {}
        }
    }
    ""
}

/// Trims every line, joins them with single spaces and drops whitespace that
/// sits between two tags.
fn collapse_whitespace(input: &str) -> String {
    let joined = input
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");

    let mut out = String::with_capacity(joined.len());
    let mut pending_ws = String::new();
    let mut after_tag_close = false;
    for c in joined.chars() {
        if c.is_whitespace() {
            pending_ws.push(c);
            continue;
        }
        if !pending_ws.is_empty() {
            if !(after_tag_close && c == '<') {
                out.push_str(&pending_ws);
            }
            pending_ws.clear();
        }
        out.push(c);
        after_tag_close = c == '>';
    }
    out.trim().to_string()
}

/// Ensures the root `<svg>` element has `width="1em"`, `height="1em"` and a
/// `style` attribute. Existing non-numeric sizes (e.g. `100%`) are kept;
/// numeric sizes such as `24` or `128px` are replaced with `1em`.
pub fn apply_root_attributes(svg: &str) -> String {
    let Some((start, end)) = find_root_tag(svg) else {
        return svg.to_string();
    };
    let tag = &svg[start..end];
    let self_closing = tag.trim_end_matches('>').ends_with('/');
    let inner = tag
        .trim_start_matches("<svg")
        .trim_end_matches('>')
        .trim_end_matches('/')
        .trim();

    let mut attrs = parse_attributes(inner);
    let mut has_style = false;
    for (name, value) in attrs.iter_mut() {
        match name.as_str() {
            "width" | "height" if is_numeric_size(value) => *value = ROOT_SIZE.to_string(),
            "style" => has_style = true,
            _ => {}
        }
    }
    for required in ["width", "height"] {
        if !attrs.iter().any(|(name, _)| name == required) {
            attrs.push((required.to_string(), ROOT_SIZE.to_string()));
        }
    }
    if !has_style {
        attrs.push(("style".to_string(), ROOT_STYLE.to_string()));
    }

    let mut rebuilt = String::from("<svg");
    for (name, value) in &attrs {
        rebuilt.push(' ');
        rebuilt.push_str(name);
        rebuilt.push_str("=\"");
        rebuilt.push_str(value);
        rebuilt.push('"');
    }
    if self_closing {
        rebuilt.push('/');
    }
    rebuilt.push('>');

    let mut out = String::with_capacity(svg.len() + 64);
    out.push_str(&svg[..start]);
    out.push_str(&rebuilt);
    out.push_str(&svg[end..]);
    out
}

fn is_numeric_size(value: &str) -> bool {
    let trimmed = value.trim().trim_end_matches("px").trim();
    !trimmed.is_empty() && trimmed.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// Finds the byte range of the first `<svg ...>` opening tag.
fn find_root_tag(svg: &str) -> Option<(usize, usize)> {
    let mut search_from = 0;
    while let Some(rel) = svg[search_from..].find("<svg") {
        let start = search_from + rel;
        let after = &svg[start + 4..];
        let is_tag = after
            .chars()
            .next()
            .map(|c| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(false);
        if is_tag {
            let end = find_tag_end(svg, start)?;
            return Some((start, end));
        }
        search_from = start + 4;
    }
    None
}

/// Returns the index just past the `>` that closes the tag starting at
/// `start`, skipping `>` characters inside quoted attribute values.
fn find_tag_end(svg: &str, start: usize) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (idx, c) in svg[start..].char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, '>') => return Some(start + idx + 1),
            _ => {}
        }
    }
    None
}

/// Parses `name="value"` pairs. Attributes without a value are kept with an
/// empty string.
fn parse_attributes(input: &str) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    let mut chars = input.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if c.is_whitespace() {
            continue;
        }
        let mut name_end = input.len();
        for (idx, nc) in input[start..].char_indices() {
            if nc == '=' || nc.is_whitespace() {
                name_end = start + idx;
                break;
            }
        }
        let name = input[start..name_end].to_string();
        while chars.peek().map(|(i, _)| *i < name_end).unwrap_or(false) {
            chars.next();
        }
        // Skip whitespace before a possible '='.
        while chars
            .peek()
            .map(|(_, ch)| ch.is_whitespace())
            .unwrap_or(false)
        {
            chars.next();
        }
        if chars.peek().map(|(_, ch)| *ch == '=').unwrap_or(false) {
            chars.next();
            while chars
                .peek()
                .map(|(_, ch)| ch.is_whitespace())
                .unwrap_or(false)
            {
                chars.next();
            }
            let value = match chars.peek().copied() {
                Some((vstart, q)) if q == '"' || q == '\'' => {
                    chars.next();
                    let rel_end = input[vstart + 1..]
                        .find(q)
                        .unwrap_or(input.len() - vstart - 1);
                    let value = input[vstart + 1..vstart + 1 + rel_end].to_string();
                    while chars
                        .peek()
                        .map(|(i, _)| *i <= vstart + rel_end + 1)
                        .unwrap_or(false)
                    {
                        chars.next();
                    }
                    value
                }
                Some((vstart, _)) => {
                    let rel_end = input[vstart..]
                        .find(|ch: char| ch.is_whitespace())
                        .unwrap_or(input.len() - vstart);
                    let value = input[vstart..vstart + rel_end].to_string();
                    while chars
                        .peek()
                        .map(|(i, _)| *i < vstart + rel_end)
                        .unwrap_or(false)
                    {
                        chars.next();
                    }
                    value
                }
                None => String::new(),
            };
            attrs.push((name, value));
        } else {
            attrs.push((name, String::new()));
        }
    }
    attrs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_prologue_doctype_and_comments() {
        let input = "\u{feff}<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n<!-- Generator: Foo -->\n<svg viewBox=\"0 0 24 24\">\n  <title>Hi</title>\n  <path d=\"M1 2\n    3 4\"/>\n</svg>\n";
        assert_eq!(
            strip_and_collapse(input),
            "<svg viewBox=\"0 0 24 24\"><title>Hi</title><path d=\"M1 2 3 4\"/></svg>"
        );
    }

    #[test]
    fn keeps_text_content_whitespace() {
        let input = "<svg>\n<text>Hello world </text>\n</svg>";
        assert_eq!(
            strip_and_collapse(input),
            "<svg><text>Hello world </text></svg>"
        );
    }

    #[test]
    fn applies_size_and_style_to_root() {
        let input = "<svg width=\"24\" height=\"24px\" viewBox=\"0 0 24 24\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\"><path d=\"M0 0\"/></svg>";
        assert_eq!(
            apply_root_attributes(input),
            "<svg width=\"1em\" height=\"1em\" viewBox=\"0 0 24 24\" fill=\"none\" xmlns=\"http://www.w3.org/2000/svg\" style=\"flex:none;line-height:1\"><path d=\"M0 0\"/></svg>"
        );
    }

    #[test]
    fn adds_missing_size_and_keeps_existing_style() {
        let input = "<svg viewBox='0 0 10 10' style=\"color:red\"><g/></svg>";
        assert_eq!(
            apply_root_attributes(input),
            "<svg viewBox=\"0 0 10 10\" style=\"color:red\" width=\"1em\" height=\"1em\"><g/></svg>"
        );
    }

    #[test]
    fn leaves_non_numeric_sizes_alone() {
        let input = "<svg width=\"100%\" height=\"1em\"></svg>";
        assert_eq!(
            apply_root_attributes(input),
            "<svg width=\"100%\" height=\"1em\" style=\"flex:none;line-height:1\"></svg>"
        );
    }

    #[test]
    fn handles_attribute_values_containing_gt() {
        let input = "<svg data-x=\"a>b\" width=\"24\"><path/></svg>";
        assert_eq!(
            apply_root_attributes(input),
            "<svg data-x=\"a>b\" width=\"1em\" height=\"1em\" style=\"flex:none;line-height:1\"><path/></svg>"
        );
    }

    #[test]
    fn root_tag_lookup_ignores_svg_prefixed_names() {
        let input = "<svgfoo/><svg viewBox=\"0 0 1 1\"/>";
        assert_eq!(
            apply_root_attributes(input),
            "<svgfoo/><svg viewBox=\"0 0 1 1\" width=\"1em\" height=\"1em\" style=\"flex:none;line-height:1\"/>"
        );
    }

    #[test]
    fn escapes_and_unescapes_template_literal_characters() {
        let raw = "a`b${c}\\d";
        let escaped = escape_template_literal(raw);
        assert_eq!(escaped, "a\\`b\\${c}\\\\d");
        assert_eq!(unescape_template_literal(&escaped), raw);
    }

    #[test]
    fn normalize_combines_steps() {
        let input = "<?xml version=\"1.0\"?>\n<svg width=\"32\" height=\"32\" viewBox=\"0 0 32 32\">\n  <circle r=\"1\"/>\n</svg>";
        assert_eq!(
            normalize(input),
            "<svg width=\"1em\" height=\"1em\" viewBox=\"0 0 32 32\" style=\"flex:none;line-height:1\"><circle r=\"1\"/></svg>"
        );
    }
}
