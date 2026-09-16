//! Port of `src/utils/textNormalization.ts`.
//!
//! Quote normalisation applied to user-edited TOML before parsing. Note that,
//! exactly like the TypeScript original, CRLF line endings are **not**
//! normalised here: all the line-index arithmetic in
//! [`crate::provider_config`] relies on `\r` staying attached to its line.

/// Port of `normalizeQuotes` (`textNormalization.ts`).
///
/// Replaces the CJK / fullwidth / curly quote families with their ASCII
/// counterparts so TOML parses:
/// - `“ ” „ ‟ ＂` → `"`
/// - `‘ ’ ＇` → `'`
///
/// Book-title marks such as `《》` or `「」` are deliberately left untouched.
/// Empty input is returned as-is.
pub fn normalize_quotes(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    text.chars()
        .map(|ch| match ch {
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' | '\u{FF02}' => '"',
            '\u{2018}' | '\u{2019}' | '\u{FF07}' => '\'',
            other => other,
        })
        .collect()
}

/// Port of `normalizeTomlText` (`textNormalization.ts`).
///
/// Currently an alias of [`normalize_quotes`]; kept as the extension point for
/// future whitespace / end-of-line normalisation. It intentionally does not
/// touch CRLF today.
pub fn normalize_toml_text(text: &str) -> String {
    normalize_quotes(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_returned_as_is() {
        assert_eq!(normalize_quotes(""), "");
        assert_eq!(normalize_toml_text(""), "");
    }

    #[test]
    fn replaces_double_quote_family() {
        assert_eq!(normalize_quotes("“a” „b‟ ＂c＂"), "\"a\" \"b\" \"c\"");
    }

    #[test]
    fn replaces_single_quote_family() {
        assert_eq!(normalize_quotes("‘a’ ＇b＇"), "'a' 'b'");
    }

    #[test]
    fn leaves_book_title_marks_alone() {
        assert_eq!(normalize_quotes("《书》「名」"), "《书》「名」");
    }

    #[test]
    fn does_not_normalize_crlf() {
        assert_eq!(
            normalize_toml_text("a = “x”\r\nb = 1\r\n"),
            "a = \"x\"\r\nb = 1\r\n"
        );
    }

    #[test]
    fn plain_ascii_is_unchanged() {
        let text = "model = \"gpt\"\nname = 'x'\n";
        assert_eq!(normalize_toml_text(text), text);
    }
}
