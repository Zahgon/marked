// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! JavaScript's `RegExp`, for the grammar the library is written in.
//!
//! Matching works on UTF-16 code units with the ECMAScript backtracking semantics: without the `u` flag a character
//! is a code unit, with it a code point, so a surrogate pair is one character and a lone surrogate is one too. A
//! pattern goes through three stages: [`parser`] checks the syntax and builds a tree, [`compiler`] lowers the tree to a
//! program, and [`vm`] runs the program. The string methods that take a regular expression (`exec`, `test`,
//! `search`, `replace` and `matchAll`) are methods of [`RegExp`] here, with `lastIndex` handled the way those methods
//! handle it.

mod ast;
mod charset;
mod compiler;
mod parser;
mod vm;

use compiler::Program;
use vm::Machine;

/// Why a pattern or its flags were rejected, with the text V8 reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntaxError(pub &'static str);

/// The flags of a regular expression.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// `g`: `replace` and `matchAll` find every match.
    pub global: bool,
    /// `i`: characters compare by their canonical upper case.
    pub ignore_case: bool,
    /// `m`: `^` and `$` also match at line terminators.
    pub multiline: bool,
    /// `s`: `.` also matches line terminators.
    pub dot_all: bool,
    /// `u`: the pattern and the input are read as code points.
    pub unicode: bool,
    /// `y`: matching happens at `lastIndex` only.
    pub sticky: bool,
}

impl Flags {
    /// Parses a flags string; every flag may appear once.
    pub fn parse(text: &str) -> Result<Flags, SyntaxError> {
        let mut flags = Flags::default();
        let mut has_indices = false;
        for c in text.chars() {
            let slot = match c {
                'g' => &mut flags.global,
                'i' => &mut flags.ignore_case,
                'm' => &mut flags.multiline,
                's' => &mut flags.dot_all,
                'u' => &mut flags.unicode,
                'y' => &mut flags.sticky,
                'd' => &mut has_indices,
                _ => return Err(SyntaxError("Invalid flags")),
            };
            if std::mem::replace(slot, true) {
                return Err(SyntaxError("Invalid flags"));
            }
        }
        Ok(flags)
    }
}

/// One capture: a range of code units of the input.
pub type Capture = Option<(usize, usize)>;

/// A successful match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Capture 0 is the whole match; a capture that took no part is `None`.
    pub captures: Vec<Capture>,
}

impl Match {
    /// Where the match starts (`match.index`).
    pub fn index(&self) -> usize {
        self.captures[0].map_or(0, |(start, _)| start)
    }

    /// Where the match ends.
    pub fn end(&self) -> usize {
        self.captures[0].map_or(0, |(_, end)| end)
    }

    /// The text of capture `group` (`match[group]`), `None` when it took no part.
    pub fn group<'a>(&self, input: &'a [u16], group: usize) -> Option<&'a [u16]> {
        let (start, end) = (*self.captures.get(group)?)?;
        Some(&input[start..end])
    }

    /// The text of capture `group`, empty when it took no part (`match[group] || ''`).
    pub fn group_or_empty<'a>(&self, input: &'a [u16], group: usize) -> &'a [u16] {
        self.group(input, group).unwrap_or_default()
    }

    /// Whether capture `group` matched something non-empty (the truthiness of `match[group]`).
    pub fn has(&self, group: usize) -> bool {
        matches!(self.captures.get(group), Some(Some((start, end))) if end > start)
    }
}

/// A compiled regular expression.
#[derive(Clone, Debug)]
pub struct RegExp {
    program: Program,
    source: String,
}

impl RegExp {
    /// `new RegExp(source, flags)`.
    pub fn new(source: &str, flags: &str) -> Result<RegExp, SyntaxError> {
        let flags = Flags::parse(flags)?;
        let units: Vec<u16> = source.encode_utf16().collect();
        let pattern = parser::parse(&units, flags.unicode)?;
        Ok(RegExp {
            program: compiler::compile(&pattern, flags),
            source: source.to_string(),
        })
    }

    /// A regular expression that never matches.
    pub fn never() -> RegExp {
        RegExp {
            program: compiler::compile(&ast::Pattern::never(), Flags::default()),
            source: String::from("(?!)"),
        }
    }

    /// The pattern text (`regex.source`).
    pub fn source(&self) -> &str {
        &self.source
    }

    /// The flags.
    pub fn flags(&self) -> Flags {
        self.program.flags
    }

    /// `AdvanceStringIndex`: the next start position after `index`, past a whole surrogate pair with the `u` flag.
    fn advance(&self, input: &[u16], index: usize) -> usize {
        if self.program.flags.unicode
            && matches!(input.get(index), Some(0xD800..=0xDBFF))
            && matches!(input.get(index + 1), Some(0xDC00..=0xDFFF))
        {
            index + 2
        } else {
            index + 1
        }
    }

    /// `RegExpBuiltinExec` with `lastIndex` at `last_index`: the first match starting at or after it (only at it
    /// with the `y` flag).
    pub fn exec_at(&self, input: &[u16], last_index: usize) -> Option<Match> {
        let mut machine = Machine::new(&self.program, input);
        let mut index = last_index;
        while index <= input.len() {
            if let Some(captures) = machine.run(index) {
                return Some(Match { captures });
            }
            if self.program.flags.sticky || self.program.anchored {
                return None;
            }
            index = self.advance(input, index);
        }
        None
    }

    /// `regex.exec(input)` from the start (`lastIndex` 0, as every non-global use in the library has it).
    pub fn exec(&self, input: &[u16]) -> Option<Match> {
        self.exec_at(input, 0)
    }

    /// `regex.test(input)`.
    pub fn test(&self, input: &[u16]) -> bool {
        self.exec(input).is_some()
    }

    /// `input.search(regex)`: where the first match starts.
    pub fn search(&self, input: &[u16]) -> Option<usize> {
        self.exec(input).map(|found| found.index())
    }

    /// `input.matchAll(regex)` for a global expression: every match, an empty one moving the search on by a
    /// character.
    pub fn match_all(&self, input: &[u16]) -> Vec<Match> {
        let mut found = Vec::new();
        let mut last_index = 0;
        while last_index <= input.len() {
            let Some(next) = self.exec_at(input, last_index) else {
                break;
            };
            last_index = if next.end() == next.index() {
                self.advance(input, next.end())
            } else {
                next.end()
            };
            let global = self.program.flags.global;
            found.push(next);
            if !global {
                break;
            }
        }
        found
    }

    /// `input.replace(regex, replacer)`: every match with the `g` flag, otherwise the first; the replacer gets the
    /// match and the input.
    pub fn replace_with(&self, input: &[u16], mut replacer: impl FnMut(&Match, &[u16]) -> Vec<u16>) -> Vec<u16> {
        let mut out = Vec::with_capacity(input.len());
        let mut copied = 0;
        for found in self.match_all(input) {
            if found.index() < copied {
                continue;
            }
            out.extend_from_slice(&input[copied..found.index()]);
            out.extend(replacer(&found, input));
            copied = found.end();
        }
        out.extend_from_slice(&input[copied..]);
        out
    }

    /// `input.replace(regex, replacement)` with a replacement pattern: `$$`, `$&`, `` $` ``, `$'` and `$n`/`$nn`
    /// are substituted as `GetSubstitution` does.
    pub fn replace(&self, input: &[u16], replacement: &str) -> Vec<u16> {
        let template: Vec<u16> = replacement.encode_utf16().collect();
        self.replace_with(input, |found, input| substitute(&template, found, input))
    }
}

/// `GetSubstitution` for a match without named groups.
fn substitute(template: &[u16], found: &Match, input: &[u16]) -> Vec<u16> {
    const DOLLAR: u16 = b'$' as u16;
    let groups = found.captures.len() - 1;
    let digit = |unit: Option<&u16>| unit.and_then(|&unit| char::from_u32(u32::from(unit))?.to_digit(10));
    let mut out = Vec::new();
    let mut i = 0;
    while i < template.len() {
        if template[i] != DOLLAR || i + 1 == template.len() {
            out.push(template[i]);
            i += 1;
            continue;
        }
        let next = template[i + 1];
        match char::from_u32(u32::from(next)) {
            Some('$') => {
                out.push(DOLLAR);
                i += 2;
            }
            Some('&') => {
                out.extend_from_slice(found.group_or_empty(input, 0));
                i += 2;
            }
            Some('`') => {
                out.extend_from_slice(&input[..found.index()]);
                i += 2;
            }
            Some('\'') => {
                out.extend_from_slice(input.get(found.end()..).unwrap_or_default());
                i += 2;
            }
            Some('0'..='9') => {
                let first = digit(template.get(i + 1)).unwrap_or(0) as usize;
                let two = digit(template.get(i + 2)).map(|second| first * 10 + second as usize);
                match two {
                    Some(number) if (1..=groups).contains(&number) => {
                        out.extend_from_slice(found.group_or_empty(input, number));
                        i += 3;
                    }
                    _ if (1..=groups).contains(&first) => {
                        out.extend_from_slice(found.group_or_empty(input, first));
                        i += 2;
                    }
                    _ => {
                        out.push(DOLLAR);
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(DOLLAR);
                i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn text(units: &[u16]) -> String {
        String::from_utf16_lossy(units)
    }

    fn regex(source: &str, flags: &str) -> RegExp {
        match RegExp::new(source, flags) {
            Ok(regex) => regex,
            Err(error) => panic!("/{source}/{flags}: {}", error.0),
        }
    }

    #[test]
    fn lookbehind_and_named_backreferences() {
        let re = regex("(?<!`)()(?<b>`+)[^`]+\\k<b>(?!`)", "g");
        let input = units("a ``x`` `y` ```z``");
        let found: Vec<String> = re
            .match_all(&input)
            .iter()
            .map(|m| text(m.group_or_empty(&input, 0)))
            .collect();
        assert_eq!(found, ["``x``", "`y`"]);
    }

    #[test]
    fn unicode_flag_reads_code_points() {
        let input = units("*\u{1F600}*");
        let plain = regex("^\\*[^\\s*]", "");
        let unicode = regex("^\\*[^\\s*]", "u");
        assert_eq!(plain.exec(&input).map(|m| m.end()), Some(2));
        assert_eq!(unicode.exec(&input).map(|m| m.end()), Some(3));
        let symbol = regex("[\\p{P}\\p{S}]", "u");
        assert!(symbol.test(&units("\u{1F600}")));
        assert!(!symbol.test(&units("a")));
        let behind = regex("(?<=\u{1F600})x", "u");
        assert_eq!(behind.search(&units("\u{1F600}x")), Some(2));
    }

    #[test]
    fn case_folding_keeps_ascii_apart() {
        let re = regex("^<(script|pre)", "i");
        assert!(re.test(&units("<SCRIPT")));
        assert!(!re.test(&units("<\u{17F}cript")));
        assert!(regex("^k$", "i").test(&units("K")));
        assert!(!regex("^k$", "i").test(&units("\u{212A}")));
    }

    #[test]
    fn line_terminators_stop_the_dot() {
        let re = regex("^(.*)$", "");
        assert!(!re.test(&units("a\u{2028}b")));
        assert!(re.test(&units("a\u{85}b")));
    }

    #[test]
    fn replace_substitutes_groups() {
        let re = regex("(^|[^\\[])\\^", "g");
        assert_eq!(text(&re.replace(&units("^a[^b]^c"), "$1")), "a[^b]c");
        let first = regex("x(y)?", "");
        assert_eq!(text(&first.replace(&units("axbxy"), "[$1$$]")), "a[$]bxy");
        let empty = regex("", "gu");
        assert_eq!(text(&empty.replace(&units("a\u{1F600}"), "-")), "-a-\u{1F600}-");
    }

    #[test]
    fn empty_loop_iterations_end() {
        let re = regex("^(?:a|)*b", "");
        assert!(re.test(&units("aab")));
        let lazy = regex("^[\\s\\S]*?(?=c)", "");
        assert_eq!(lazy.exec(&units("abc")).map(|m| m.end()), Some(2));
    }
}
