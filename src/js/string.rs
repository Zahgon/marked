// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! JavaScript strings: sequences of UTF-16 code units, which may hold unpaired surrogates.
//!
//! Every length, index and slice the library takes counts UTF-16 code units, so strings are kept in that form and
//! converted to UTF-8 only when they are written out. The methods here are the `String.prototype` methods the
//! library calls, with their JavaScript semantics.

use std::fmt;

use super::error::JsError;
use super::unicode_data;

/// A JavaScript string value.
#[derive(Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JsString(Vec<u16>);

impl JsString {
    /// The empty string.
    pub fn new() -> Self {
        JsString(Vec::new())
    }

    /// Wraps UTF-16 code units.
    pub fn from_units(units: Vec<u16>) -> Self {
        JsString(units)
    }

    /// A copy of UTF-16 code units.
    pub fn from_slice(units: &[u16]) -> Self {
        JsString(units.to_vec())
    }

    /// The code units of the string.
    pub fn units(&self) -> &[u16] {
        &self.0
    }

    /// Consumes the string, returning its code units.
    pub fn into_units(self) -> Vec<u16> {
        self.0
    }

    /// The length in UTF-16 code units, as `String.prototype.length`.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the string is empty (and so falsy).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Appends another string.
    pub fn push_js(&mut self, other: &JsString) {
        self.0.extend_from_slice(&other.0);
    }

    /// Appends UTF-16 code units.
    pub fn push_units(&mut self, units: &[u16]) {
        self.0.extend_from_slice(units);
    }

    /// Appends a Rust string.
    pub fn push_str(&mut self, text: &str) {
        self.0.extend(text.encode_utf16());
    }

    /// The concatenation of code unit slices.
    pub fn concat(parts: &[&[u16]]) -> JsString {
        JsString(parts.concat())
    }

    /// The code unit at `index`, if any.
    pub fn unit_at(&self, index: usize) -> Option<u16> {
        self.0.get(index).copied()
    }

    /// `String.prototype.charAt`: the code unit at `index` as a string, empty past the end.
    pub fn char_at(&self, index: usize) -> JsString {
        JsString(self.0.get(index).map(|&unit| vec![unit]).unwrap_or_default())
    }

    /// `String.prototype.slice(start, end)`: negative positions count from the end, and both are clamped.
    pub fn slice(&self, start: isize, end: Option<isize>) -> JsString {
        let (from, to) = slice_bounds(self.0.len(), start, end);
        JsString(self.0[from..to.max(from)].to_vec())
    }

    /// `String.prototype.slice(start)` for a position from the start.
    pub fn slice_from(&self, start: usize) -> JsString {
        JsString(self.0.get(start..).unwrap_or_default().to_vec())
    }

    /// `String.prototype.substring(start, end)` for positions that are not negative: clamped and swapped when
    /// reversed.
    pub fn substring(&self, start: usize, end: usize) -> JsString {
        let len = self.0.len();
        let (a, b) = (start.min(len), end.min(len));
        JsString(self.0[a.min(b)..a.max(b)].to_vec())
    }

    /// `String.prototype.split` with a string separator.
    pub fn split_str(&self, separator: &str) -> Vec<JsString> {
        let separator: Vec<u16> = separator.encode_utf16().collect();
        split_units(&self.0, &separator)
            .into_iter()
            .map(JsString::from_slice)
            .collect()
    }

    /// `String.prototype.split(separator, 1)[0]`: everything before the first separator.
    pub fn first_part(&self, separator: &str) -> JsString {
        let separator: Vec<u16> = separator.encode_utf16().collect();
        match index_of(&self.0, &separator, 0) {
            Some(end) if !separator.is_empty() => JsString::from_slice(&self.0[..end]),
            _ => self.clone(),
        }
    }

    /// `Array.prototype.join`.
    pub fn join(parts: &[JsString], separator: &str) -> JsString {
        let separator: Vec<u16> = separator.encode_utf16().collect();
        let mut out = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&separator);
            }
            out.extend_from_slice(&part.0);
        }
        JsString(out)
    }

    /// Replaces every occurrence of `from` with `to`.
    pub fn replace_all(&self, from: &str, to: &str) -> JsString {
        let from: Vec<u16> = from.encode_utf16().collect();
        let to: Vec<u16> = to.encode_utf16().collect();
        let mut out = Vec::with_capacity(self.0.len());
        for (i, part) in split_units(&self.0, &from).into_iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&to);
            }
            out.extend_from_slice(part);
        }
        JsString(out)
    }

    /// `String.prototype.includes` of Rust text.
    pub fn includes(&self, needle: &str) -> bool {
        let needle: Vec<u16> = needle.encode_utf16().collect();
        index_of(&self.0, &needle, 0).is_some()
    }

    /// `String.prototype.indexOf` of Rust text.
    pub fn index_of(&self, needle: &str) -> Option<usize> {
        let needle: Vec<u16> = needle.encode_utf16().collect();
        index_of(&self.0, &needle, 0)
    }

    /// `String.prototype.lastIndexOf` of Rust text.
    pub fn last_index_of(&self, needle: &str) -> Option<usize> {
        let needle: Vec<u16> = needle.encode_utf16().collect();
        last_index_of(&self.0, &needle, self.0.len())
    }

    /// `String.prototype.startsWith` of Rust text.
    pub fn starts_with(&self, prefix: &str) -> bool {
        self.0.starts_with(&prefix.encode_utf16().collect::<Vec<u16>>())
    }

    /// `String.prototype.endsWith` of Rust text.
    pub fn ends_with(&self, suffix: &str) -> bool {
        self.0.ends_with(&suffix.encode_utf16().collect::<Vec<u16>>())
    }

    /// Whether the string equals Rust text.
    pub fn eq_str(&self, text: &str) -> bool {
        self.0.iter().copied().eq(text.encode_utf16())
    }

    /// `String.prototype.repeat`.
    pub fn repeat(text: &str, count: usize) -> JsString {
        JsString::from(text.repeat(count).as_str())
    }

    /// `String.prototype.trim`: drops JavaScript white space and line terminators at both ends.
    pub fn trim(&self) -> JsString {
        let start = self.0.iter().position(|&unit| !is_js_whitespace(unit));
        match start {
            None => JsString::new(),
            Some(start) => {
                let end = self
                    .0
                    .iter()
                    .rposition(|&unit| !is_js_whitespace(unit))
                    .map_or(start, |end| end + 1);
                JsString(self.0[start..end].to_vec())
            }
        }
    }

    /// `String.prototype.trimStart`.
    pub fn trim_start(&self) -> JsString {
        let start = self
            .0
            .iter()
            .position(|&unit| !is_js_whitespace(unit))
            .unwrap_or(self.0.len());
        JsString(self.0[start..].to_vec())
    }

    /// `String.prototype.trimEnd`.
    pub fn trim_end(&self) -> JsString {
        let end = self
            .0
            .iter()
            .rposition(|&unit| !is_js_whitespace(unit))
            .map_or(0, |end| end + 1);
        JsString(self.0[..end].to_vec())
    }

    /// The characters of `[...string]`: code points, a surrogate pair as one, a lone surrogate on its own.
    pub fn code_points(&self) -> Vec<JsString> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.0.len() {
            let width = code_point_width(&self.0, i);
            out.push(JsString(self.0[i..i + width].to_vec()));
            i += width;
        }
        out
    }

    /// The number of characters of `[...string]`.
    pub fn code_point_count(&self) -> usize {
        let mut count = 0;
        let mut i = 0;
        while i < self.0.len() {
            i += code_point_width(&self.0, i);
            count += 1;
        }
        count
    }

    /// `String.prototype.toLowerCase`, with the final sigma rule.
    pub fn to_lower_case(&self) -> JsString {
        let points = decode(&self.0);
        let mut out = Vec::with_capacity(self.0.len());
        for (i, &code) in points.iter().enumerate() {
            if code == 0x3A3 {
                let final_sigma = preceded_by_cased(&points[..i]) && !followed_by_cased(&points[i + 1..]);
                encode_into(&mut out, if final_sigma { 0x3C2 } else { 0x3C3 });
            } else {
                map_case(&mut out, code, unicode_data::LOWERCASE);
            }
        }
        JsString(out)
    }

    /// `String.prototype.toUpperCase`.
    pub fn to_upper_case(&self) -> JsString {
        let mut out = Vec::with_capacity(self.0.len());
        for code in decode(&self.0) {
            map_case(&mut out, code, unicode_data::UPPERCASE);
        }
        JsString(out)
    }

    /// `encodeURI`: every character outside the URI's reserved and unreserved sets and `#` as `%XX` escapes of its
    /// UTF-8 bytes; a lone surrogate throws a `URIError`.
    pub fn encode_uri(&self) -> Result<JsString, JsError> {
        const KEEP: &str = ";/?:@&=+$,-_.!~*'()#";
        let mut out = JsString::new();
        let mut i = 0;
        while i < self.0.len() {
            let unit = self.0[i];
            if unit < 0x80 && (char::from(unit as u8).is_ascii_alphanumeric() || KEEP.contains(char::from(unit as u8)))
            {
                out.0.push(unit);
                i += 1;
                continue;
            }
            let width = code_point_width(&self.0, i);
            let Some(c) = char::decode_utf16(self.0[i..i + width].iter().copied())
                .next()
                .and_then(Result::ok)
            else {
                return Err(JsError::new("URIError", "URI malformed"));
            };
            let mut bytes = [0u8; 4];
            for byte in c.encode_utf8(&mut bytes).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
            i += width;
        }
        Ok(out)
    }

    /// The string as UTF-8, with every unpaired surrogate replaced by U+FFFD.
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }

    /// Whether the string is a canonical array index ("0" .. "4294967294"), the keys JavaScript objects order
    /// first and numerically.
    pub fn array_index(&self) -> Option<u32> {
        array_index(&self.0)
    }
}

impl From<&str> for JsString {
    fn from(text: &str) -> Self {
        JsString(text.encode_utf16().collect())
    }
}

impl From<String> for JsString {
    fn from(text: String) -> Self {
        JsString(text.encode_utf16().collect())
    }
}

impl fmt::Debug for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.to_string_lossy())
    }
}

impl fmt::Display for JsString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

/// The bounds `slice(start, end)` takes on a string of `len` code units.
fn slice_bounds(len: usize, start: isize, end: Option<isize>) -> (usize, usize) {
    let resolve = |position: isize| -> usize {
        if position < 0 {
            len.saturating_sub(position.unsigned_abs())
        } else {
            position.unsigned_abs().min(len)
        }
    };
    (resolve(start), end.map_or(len, resolve))
}

/// How many code units the character at `index` takes: 2 for a surrogate pair, 1 otherwise.
pub fn code_point_width(units: &[u16], index: usize) -> usize {
    match (units.get(index), units.get(index + 1)) {
        (Some(0xD800..=0xDBFF), Some(0xDC00..=0xDFFF)) => 2,
        _ => 1,
    }
}

/// The code points of UTF-16 code units, a lone surrogate standing for itself.
fn decode(units: &[u16]) -> Vec<u32> {
    char::decode_utf16(units.iter().copied())
        .map(|result| match result {
            Ok(c) => u32::from(c),
            Err(error) => u32::from(error.unpaired_surrogate()),
        })
        .collect()
}

/// Appends a code point as UTF-16.
fn encode_into(out: &mut Vec<u16>, code: u32) {
    match char::from_u32(code) {
        Some(c) => out.extend_from_slice(c.encode_utf16(&mut [0; 2])),
        None => out.push(code as u16),
    }
}

/// Appends the case mapping of a code point from one of the tables, or the code point itself.
fn map_case(out: &mut Vec<u16>, code: u32, table: &[(u32, &[u32])]) {
    match table.binary_search_by_key(&code, |&(from, _)| from) {
        Ok(found) => table[found].1.iter().for_each(|&mapped| encode_into(out, mapped)),
        Err(_) => encode_into(out, code),
    }
}

fn in_ranges(ranges: &[(u32, u32)], code: u32) -> bool {
    ranges
        .binary_search_by(|&(low, high)| {
            if high < code {
                std::cmp::Ordering::Less
            } else if low > code {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The first code point that is not case-ignorable, scanning `points` in order, is cased.
fn first_cased(points: impl Iterator<Item = u32>) -> bool {
    for code in points {
        if in_ranges(unicode_data::CASE_IGNORABLE, code) {
            continue;
        }
        return in_ranges(unicode_data::CASED, code);
    }
    false
}

fn preceded_by_cased(before: &[u32]) -> bool {
    first_cased(before.iter().rev().copied())
}

fn followed_by_cased(after: &[u32]) -> bool {
    first_cased(after.iter().copied())
}

/// The canonical array index a key denotes, if it is one.
pub fn array_index(units: &[u16]) -> Option<u32> {
    if units.is_empty() || units.len() > 10 {
        return None;
    }
    if units[0] == u16::from(b'0') {
        return if units.len() == 1 { Some(0) } else { None };
    }
    let mut value: u64 = 0;
    for &unit in units {
        if !(u16::from(b'0')..=u16::from(b'9')).contains(&unit) {
            return None;
        }
        value = value * 10 + u64::from(unit - u16::from(b'0'));
    }
    if value < u64::from(u32::MAX) {
        Some(value as u32)
    } else {
        None
    }
}

/// Splits code units at every occurrence of a non-empty separator.
pub fn split_units<'a>(units: &'a [u16], separator: &[u16]) -> Vec<&'a [u16]> {
    let mut parts = Vec::new();
    if separator.is_empty() {
        parts.push(units);
        return parts;
    }
    let mut start = 0;
    let mut i = 0;
    while i + separator.len() <= units.len() {
        if &units[i..i + separator.len()] == separator {
            parts.push(&units[start..i]);
            i += separator.len();
            start = i;
        } else {
            i += 1;
        }
    }
    parts.push(&units[start..]);
    parts
}

/// `String.prototype.indexOf`: the first occurrence of `needle` at or after `from`.
pub fn index_of(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    let from = from.min(haystack.len());
    if needle.is_empty() {
        return Some(from);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// `String.prototype.lastIndexOf`: the last occurrence of `needle` starting at or before `from`.
pub fn last_index_of(haystack: &[u16], needle: &[u16], from: usize) -> Option<usize> {
    if needle.len() > haystack.len() {
        return None;
    }
    let last = (haystack.len() - needle.len()).min(from);
    (0..=last).rev().find(|&i| haystack[i..i + needle.len()] == *needle)
}

/// Whether a code unit is JavaScript white space or a line terminator (`\s` in a regular expression, and what
/// `String.prototype.trim` drops).
pub fn is_js_whitespace(unit: u16) -> bool {
    matches!(
        unit,
        0x0009..=0x000D
            | 0x0020
            | 0x00A0
            | 0x1680
            | 0x2000..=0x200A
            | 0x2028
            | 0x2029
            | 0x202F
            | 0x205F
            | 0x3000
            | 0xFEFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn js(text: &str) -> JsString {
        JsString::from(text)
    }

    #[test]
    fn slices_like_javascript() {
        let s = js("abcdef");
        assert_eq!(s.slice(1, Some(-1)), js("bcde"));
        assert_eq!(s.slice(-2, None), js("ef"));
        assert_eq!(s.slice(4, Some(2)), js(""));
        assert_eq!(s.substring(4, 2), js("cd"));
        assert_eq!(s.first_part("c"), js("ab"));
    }

    #[test]
    fn trims_javascript_white_space() {
        assert_eq!(js("\u{FEFF}\u{3000} a b\u{2029}\n").trim(), js("a b"));
        assert_eq!(js("\u{85}a").trim(), js("\u{85}a"));
        assert_eq!(js("  a ").trim_start(), js("a "));
        assert_eq!(js("  a ").trim_end(), js("  a"));
    }

    #[test]
    fn maps_case_with_final_sigma() {
        assert_eq!(js("ΑΣ").to_lower_case(), js("ας"));
        assert_eq!(js("Σ").to_lower_case(), js("σ"));
        assert_eq!(js("ΑΣΑ").to_lower_case(), js("ασα"));
        assert_eq!(js("ß").to_upper_case(), js("SS"));
        assert_eq!(js("İ").to_lower_case(), js("i\u{307}"));
        let lone = JsString::from_units(vec![0xD800, 0x41]);
        assert_eq!(lone.to_lower_case(), JsString::from_units(vec![0xD800, 0x61]));
    }

    #[test]
    fn encodes_uris() {
        assert_eq!(js("a b/é#%").encode_uri().ok(), Some(js("a%20b/%C3%A9#%25")));
        assert!(JsString::from_units(vec![0xDC00]).encode_uri().is_err());
    }

    #[test]
    fn counts_code_points() {
        let s = JsString::from_units(vec![0x61, 0xD83D, 0xDE00, 0xD800]);
        assert_eq!(s.code_point_count(), 3);
        assert_eq!(s.code_points()[1].len(), 2);
    }

    #[test]
    fn array_indices() {
        assert_eq!(js("0").array_index(), Some(0));
        assert_eq!(js("4294967294").array_index(), Some(4_294_967_294));
        assert_eq!(js("4294967295").array_index(), None);
        assert_eq!(js("01").array_index(), None);
    }
}
