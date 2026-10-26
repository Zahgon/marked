// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! Helpers shared by the tokenizer and the renderer: HTML escaping, numeric character references, URL cleaning,
//! table cell splitting, trimming, label normalization, bracket matching and tab expansion.

use crate::js::JsString;
use crate::rules::{Flavour, Rules};

/// The helper expressions, which every flavour shares.
fn other() -> &'static crate::rules::Other {
    Rules::of(Flavour::Normal).other
}

fn escape_replacement(unit: u16) -> &'static str {
    match unit {
        0x26 => "&amp;",
        0x3C => "&lt;",
        0x3E => "&gt;",
        0x22 => "&quot;",
        _ => "&#39;",
    }
}

/// Escapes `<`, `>`, `"` and `'` for HTML, and `&`: every `&` with `encode`, otherwise only an `&` that does not
/// start a character reference.
pub fn escape_html_entities(html: &JsString, encode: bool) -> JsString {
    let other = other();
    let (test, replace) = if encode {
        (&other.escape_test, &other.escape_replace)
    } else {
        (&other.escape_test_no_encode, &other.escape_replace_no_encode)
    };
    if !test.test(html.units()) {
        return html.clone();
    }
    JsString::from_units(replace.replace_with(html.units(), |found, input| {
        let unit = input[found.index()];
        escape_replacement(unit).encode_utf16().collect()
    }))
}

/// Replaces numeric character references with the characters they name. Values that are zero, out of range, or a
/// surrogate become U+FFFD; a reference to `&` and every `&` that is not a reference become `&amp;`, so that a
/// decoded character never joins its neighbours into a new reference.
pub fn decode_numeric_character_references(text: &JsString) -> JsString {
    let other = other();
    if !other.numeric_character_reference.test(text.units()) {
        return text.clone();
    }
    JsString::from_units(
        other
            .numeric_character_reference_or_ampersand
            .replace_with(text.units(), |found, input| {
                let code = match (found.group(input, 1), found.group(input, 2)) {
                    (Some(decimal), _) => parse_digits(decimal, 10),
                    (None, Some(hex)) => parse_digits(hex, 16),
                    (None, None) => return "&amp;".encode_utf16().collect(),
                };
                if code == 0 || code > 0x10_FFFF || (0xD800..=0xDFFF).contains(&code) {
                    return vec![0xFFFD];
                }
                if code == 38 {
                    return "&amp;".encode_utf16().collect();
                }
                char::from_u32(code)
                    .map(|c| c.encode_utf16(&mut [0; 2]).to_vec())
                    .unwrap_or_else(|| vec![0xFFFD])
            }),
    )
}

/// `Number.parseInt` of a run of digits the grammar has already checked (at most seven decimal or six hex digits).
fn parse_digits(digits: &[u16], radix: u32) -> u32 {
    digits.iter().fold(0u32, |value, &unit| {
        let digit = char::from_u32(u32::from(unit))
            .and_then(|c| c.to_digit(radix))
            .unwrap_or(0);
        value.saturating_mul(radix).saturating_add(digit)
    })
}

/// `encodeURI` of a link destination with `%25` turned back into `%`; `None` when `encodeURI` throws.
pub fn clean_url(href: &JsString) -> Option<JsString> {
    let encoded = href.encode_uri().ok()?;
    Some(JsString::from_units(
        other().percent_decode.replace(encoded.units(), "%"),
    ))
}

/// Splits a table row into its cells. A pipe preceded by an odd number of backslashes is escaped and stays in the
/// cell; an empty first or last cell (from a leading or trailing pipe) is dropped; with `count`, the row is cut or
/// padded to that many cells. Cells are trimmed and `\|` becomes `|`.
pub fn split_cells(table_row: &JsString, count: usize) -> Vec<JsString> {
    let units = table_row.units();
    let mut row = Vec::with_capacity(units.len());
    for (offset, &unit) in units.iter().enumerate() {
        if unit != u16::from(b'|') {
            row.push(unit);
            continue;
        }
        let backslashes = units[..offset]
            .iter()
            .rev()
            .take_while(|&&unit| unit == u16::from(b'\\'))
            .count();
        if backslashes % 2 == 0 {
            row.push(u16::from(b' '));
        }
        row.push(unit);
    }
    let mut cells = JsString::from_units(row).split_str(" |");
    if cells.first().is_some_and(|cell| cell.trim().is_empty()) {
        cells.remove(0);
    }
    if cells.last().is_some_and(|cell| cell.trim().is_empty()) {
        cells.pop();
    }
    if count > 0 {
        cells.resize(count, JsString::new());
    }
    cells.iter().map(|cell| cell.trim().replace_all("\\|", "|")).collect()
}

/// Removes trailing `c`s, or with `invert` the trailing run of characters other than `c`.
pub fn rtrim(text: &JsString, c: u16, invert: bool) -> JsString {
    let units = text.units();
    let suffix = units.iter().rev().take_while(|&&unit| (unit == c) != invert).count();
    JsString::from_slice(&units[..units.len() - suffix])
}

/// Drops trailing blank lines when there are at least two of them, keeping a single one.
pub fn trim_trailing_blank_lines(text: &JsString) -> JsString {
    let lines = text.split_str("\n");
    let blank_line = &other().blank_line;
    let kept = lines
        .iter()
        .rposition(|line| !blank_line.test(line.units()))
        .map_or(0, |last| last + 1);
    if lines.len() - kept < 2 {
        return text.clone();
    }
    JsString::join(&lines[..kept], "\n")
}

/// Normalizes a link label so that definitions and references meet: trimmed, then lower case of the upper case of
/// the lower case (a Unicode case fold that `toLowerCase` alone does not reach).
pub fn normalize_label(label: &JsString) -> JsString {
    label.trim().to_lower_case().to_upper_case().to_lower_case()
}

/// The outcome of looking for the bracket that closes an opened one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closing {
    /// The closing bracket that leaves more closed than opened, at this index.
    At(usize),
    /// No such bracket, and every opened bracket closed.
    Balanced,
    /// More brackets opened than closed.
    Unbalanced,
}

/// Finds the first closing bracket (`close`) without a matching opening one (`open`), skipping backslash escapes.
pub fn find_closing_bracket(text: &JsString, open: u16, close: u16) -> Closing {
    let units = text.units();
    if !units.contains(&open) && !units.contains(&close) {
        return Closing::Balanced;
    }
    let mut level = 0i64;
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        if unit == u16::from(b'\\') {
            i += 1;
        } else if unit == open {
            level += 1;
        } else if unit == close {
            level -= 1;
            if level < 0 {
                return Closing::At(i);
            }
        }
        i += 1;
    }
    if level > 0 {
        Closing::Unbalanced
    } else {
        Closing::Balanced
    }
}

/// Expands tabs to the next multiple of four columns, the line starting at column `indent`. Columns count
/// characters, a surrogate pair as one.
pub fn expand_tabs(line: &JsString, indent: usize) -> JsString {
    let mut column = indent;
    let mut expanded = Vec::with_capacity(line.len());
    for character in line.code_points() {
        if character.units() == [u16::from(b'\t')] {
            let added = 4 - column % 4;
            expanded.extend(std::iter::repeat_n(u16::from(b' '), added));
            column += added;
        } else {
            expanded.extend_from_slice(character.units());
            column += 1;
        }
    }
    JsString::from_units(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(text: &str) -> JsString {
        JsString::from(text)
    }

    #[test]
    fn escapes_html() {
        assert_eq!(escape_html_entities(&js("<a & b>"), true), js("&lt;a &amp; b&gt;"));
        assert_eq!(
            escape_html_entities(&js("&amp; & &#1; 'x'"), false),
            js("&amp; &amp; &#1; &#39;x&#39;")
        );
    }

    #[test]
    fn decodes_numeric_references() {
        assert_eq!(
            decode_numeric_character_references(&js("&#65;&#x42; & &#0;")),
            js("AB &amp; \u{FFFD}")
        );
        assert_eq!(decode_numeric_character_references(&js("a & b")), js("a & b"));
        assert_eq!(decode_numeric_character_references(&js("&#38;amp;")), js("&amp;amp;"));
    }

    #[test]
    fn splits_cells() {
        assert_eq!(split_cells(&js("| a | b \\| c |"), 0), vec![js("a"), js("b | c")]);
        assert_eq!(split_cells(&js("a|b|c"), 2), vec![js("a"), js("b")]);
        assert_eq!(split_cells(&js("a"), 3), vec![js("a"), js(""), js("")]);
    }

    #[test]
    fn trims_and_brackets() {
        assert_eq!(rtrim(&js("ab##"), u16::from(b'#'), false), js("ab"));
        assert_eq!(rtrim(&js("ab##"), u16::from(b'#'), true), js("ab##"));
        assert_eq!(find_closing_bracket(&js("a(b)c)d"), 0x28, 0x29), Closing::At(5));
        assert_eq!(find_closing_bracket(&js("(a"), 0x28, 0x29), Closing::Unbalanced);
        assert_eq!(normalize_label(&js(" ẞ ")), js("ss"));
        assert_eq!(expand_tabs(&js("a\tb"), 1), js("a  b"));
        assert_eq!(trim_trailing_blank_lines(&js("a\n\n\n")), js("a"));
        assert_eq!(trim_trailing_blank_lines(&js("a\n")), js("a\n"));
    }
}
