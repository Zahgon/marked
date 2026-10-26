// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! Sets of characters for character classes, the Unicode properties `\p{..}` names, and the case folding of the `i`
//! flag.

use std::sync::OnceLock;

use crate::js::string::is_js_whitespace;
use crate::js::unicode_data;

/// The largest code point.
pub const MAX_CODE_POINT: u32 = 0x10_FFFF;
const MAX_UNIT: u32 = 0xFFFF;

/// A set of characters as sorted, disjoint inclusive ranges.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CharSet {
    ranges: Vec<(u32, u32)>,
}

impl CharSet {
    /// A set from inclusive ranges.
    pub fn from_ranges(ranges: &[(u32, u32)]) -> CharSet {
        let mut set = CharSet {
            ranges: ranges.to_vec(),
        };
        set.normalize();
        set
    }

    /// Adds an inclusive range.
    pub fn add_range(&mut self, low: u32, high: u32) {
        self.ranges.push((low, high));
    }

    /// Adds every member of another set.
    pub fn add_set(&mut self, other: &CharSet) {
        self.ranges.extend_from_slice(&other.ranges);
    }

    /// Sorts the ranges and merges the ones that touch.
    pub fn normalize(&mut self) {
        self.ranges.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(self.ranges.len());
        for &(low, high) in &self.ranges {
            match merged.last_mut() {
                Some(last) if low <= last.1 + 1 => last.1 = last.1.max(high),
                _ => merged.push((low, high)),
            }
        }
        self.ranges = merged;
    }

    /// The complement of a normalized set among the characters up to `max`.
    pub fn inverted(&self, max: u32) -> CharSet {
        let mut out = CharSet::default();
        let mut next = 0u32;
        for &(low, high) in &self.ranges {
            if low > max {
                break;
            }
            if low > next {
                out.add_range(next, low - 1);
            }
            next = high + 1;
        }
        if next <= max {
            out.add_range(next, max);
        }
        out
    }

    /// Whether a normalized set holds a character.
    pub fn contains(&self, c: u32) -> bool {
        self.ranges
            .binary_search_by(|&(low, high)| {
                if high < c {
                    std::cmp::Ordering::Less
                } else if low > c {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .is_ok()
    }

    /// The members of the set that are code units.
    pub fn units(&self) -> impl Iterator<Item = u32> + '_ {
        self.ranges
            .iter()
            .filter(|&&(low, _)| low <= MAX_UNIT)
            .flat_map(|&(low, high)| low..=high.min(MAX_UNIT))
    }

    /// `\d`.
    pub fn digits() -> CharSet {
        CharSet {
            ranges: vec![(0x30, 0x39)],
        }
    }

    /// `\w`.
    pub fn word() -> CharSet {
        CharSet {
            ranges: vec![(0x30, 0x39), (0x41, 0x5A), (0x5F, 0x5F), (0x61, 0x7A)],
        }
    }

    /// `\s`: JavaScript white space and line terminators.
    pub fn space() -> CharSet {
        let mut set = CharSet::default();
        for unit in (0..=MAX_UNIT as u16).filter(|&unit| is_js_whitespace(unit)) {
            set.add_range(u32::from(unit), u32::from(unit));
        }
        set.normalize();
        set
    }

    /// The set a `\p{..}` escape names, for the property names the library's patterns use.
    pub fn property(name: &str) -> Option<CharSet> {
        let ranges = match name {
            "P" | "Punctuation" => unicode_data::PUNCTUATION,
            "S" | "Symbol" => unicode_data::SYMBOL,
            "L" | "Letter" => unicode_data::LETTER,
            "N" | "Number" => unicode_data::NUMBER,
            "Pi" | "Initial_Punctuation" => unicode_data::INITIAL_PUNCTUATION,
            "Ps" | "Open_Punctuation" => unicode_data::OPEN_PUNCTUATION,
            _ => return None,
        };
        Some(CharSet::from_ranges(ranges))
    }
}

/// The result of ECMAScript's `Canonicalize` without the `u` flag: the upper case of a unit when that is one code
/// unit, and never a non-ASCII unit folding to an ASCII one.
pub fn canonicalize(unit: u32) -> u32 {
    if unit < 0x80 {
        return if (0x61..=0x7A).contains(&unit) {
            unit - 0x20
        } else {
            unit
        };
    }
    match fold_table().get(unit as usize) {
        Some(&folded) => u32::from(folded),
        None => unit,
    }
}

fn fold_table() -> &'static [u16] {
    static TABLE: OnceLock<Vec<u16>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: Vec<u16> = (0..=MAX_UNIT as u16).collect();
        for &(code, upper) in unicode_data::UPPERCASE {
            if let [single] = upper {
                if (0x80..=MAX_UNIT).contains(&code) && (0x80..=MAX_UNIT).contains(single) {
                    table[code as usize] = *single as u16;
                }
            }
        }
        table
    })
}

/// A set tested through case folding: one bit per canonical unit.
#[derive(Clone, Debug)]
pub struct FoldedSet {
    bits: Vec<u64>,
}

impl FoldedSet {
    /// The canonical units of a set's members.
    pub fn new(set: &CharSet) -> FoldedSet {
        let mut bits = vec![0u64; 1024];
        for unit in set.units() {
            let folded = canonicalize(unit);
            bits[(folded >> 6) as usize] |= 1 << (folded & 63);
        }
        FoldedSet { bits }
    }

    /// Whether some member folds to the same unit as `unit`.
    pub fn contains(&self, unit: u32) -> bool {
        let folded = canonicalize(unit);
        self.bits
            .get((folded >> 6) as usize)
            .is_some_and(|word| word & (1 << (folded & 63)) != 0)
    }
}
