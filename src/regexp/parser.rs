// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The syntax of JavaScript regular expressions: the Annex B syntax without the `u` flag, and with it code points,
//! `\u{..}` and the `\p{..}` property classes.

use super::ast::{Node, Pattern, Repeat};
use super::charset::{CharSet, MAX_CODE_POINT};
use super::SyntaxError;

/// V8's stand-in for an unbounded or overflowing quantifier number.
const QUANTIFIER_INFINITY: u32 = i32::MAX as u32;

/// How deep groups may nest; the parser and the compiler recurse.
const MAX_NESTING: usize = 1_000;

const NOTHING_TO_REPEAT: SyntaxError = SyntaxError("Nothing to repeat");
const INVALID_GROUP: SyntaxError = SyntaxError("Invalid group");
const INVALID_NAMED_REFERENCE: SyntaxError = SyntaxError("Invalid named reference");
const INVALID_CAPTURE_NAME: SyntaxError = SyntaxError("Invalid capture group name");
const INVALID_ESCAPE: SyntaxError = SyntaxError("Invalid escape");
const INVALID_PROPERTY_NAME: SyntaxError = SyntaxError("Invalid property name");
const LONE_QUANTIFIER_BRACKETS: SyntaxError = SyntaxError("Lone quantifier brackets");

/// Parses a pattern; `unicode` is the `u` flag.
pub fn parse(source: &[u16], unicode: bool) -> Result<Pattern, SyntaxError> {
    let (total_groups, has_named_groups) = count_groups(source);
    let mut parser = Parser {
        src: source,
        pos: 0,
        unicode,
        total_groups,
        has_named_groups: has_named_groups || unicode,
        next_group: 1,
        names: Vec::new(),
        references: Vec::new(),
        depth: 0,
    };
    let mut root = parser.disjunction()?;
    if parser.pos < source.len() {
        return Err(SyntaxError("Unmatched ')'"));
    }
    parser.resolve_references(&mut root)?;
    Ok(Pattern {
        root,
        groups: total_groups,
    })
}

const fn ch(c: char) -> u16 {
    c as u16
}

/// Counts the capture groups ahead of parsing: a `\N` escape depends on how many groups the whole pattern has.
fn count_groups(src: &[u16]) -> (usize, bool) {
    let mut count = 0;
    let mut named = false;
    let mut in_class = false;
    let mut i = 0;
    while i < src.len() {
        let unit = src[i];
        if unit == ch('\\') {
            i += 2;
            continue;
        }
        if in_class {
            in_class = unit != ch(']');
        } else if unit == ch('[') {
            in_class = true;
        } else if unit == ch('(') {
            if src.get(i + 1) != Some(&ch('?')) {
                count += 1;
            } else if src.get(i + 2) == Some(&ch('<'))
                && src.get(i + 3) != Some(&ch('='))
                && src.get(i + 3) != Some(&ch('!'))
            {
                count += 1;
                named = true;
            }
        }
        i += 1;
    }
    (count, named)
}

/// A class member: one character, or a whole class escape such as `\d`.
enum ClassAtom {
    Char(u32),
    Set(CharSet),
}

struct Parser<'a> {
    src: &'a [u16],
    pos: usize,
    unicode: bool,
    total_groups: usize,
    has_named_groups: bool,
    next_group: usize,
    names: Vec<(Vec<u16>, usize)>,
    /// Named backreferences waiting for the names: `\k<name>` may come before its group.
    references: Vec<Vec<u16>>,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u16> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u16> {
        self.src.get(self.pos + offset).copied()
    }

    fn eat(&mut self, unit: char) -> bool {
        if self.peek() == Some(ch(unit)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    /// The largest character: a code unit without the `u` flag, a code point with it.
    fn max_char(&self) -> u32 {
        if self.unicode {
            MAX_CODE_POINT
        } else {
            0xFFFF
        }
    }

    /// The source character at the position, a surrogate pair joined with the `u` flag; the position moves past it.
    fn next_char(&mut self) -> Option<u32> {
        let unit = self.peek()?;
        self.pos += 1;
        if self.unicode {
            if let (0xD800..=0xDBFF, Some(low @ 0xDC00..=0xDFFF)) = (unit, self.peek()) {
                self.pos += 1;
                return Some(surrogate_pair(unit, low));
            }
        }
        Some(u32::from(unit))
    }

    fn disjunction(&mut self) -> Result<Node, SyntaxError> {
        let mut alternatives = vec![self.alternative()?];
        while self.eat('|') {
            alternatives.push(self.alternative()?);
        }
        Ok(match alternatives.len() {
            1 => alternatives.remove(0),
            _ => Node::Alternation(alternatives),
        })
    }

    fn alternative(&mut self) -> Result<Node, SyntaxError> {
        let mut terms = Vec::new();
        while let Some(unit) = self.peek() {
            if unit == ch('|') || unit == ch(')') {
                break;
            }
            terms.push(self.term()?);
        }
        Ok(Node::Sequence(terms))
    }

    fn term(&mut self) -> Result<Node, SyntaxError> {
        let first_capture = self.next_group;
        let start = self.pos;
        let Some(c) = self.next_char() else {
            return Err(NOTHING_TO_REPEAT);
        };
        let atom = match char::from_u32(c) {
            Some('^') => return Ok(Node::Start),
            Some('$') => return Ok(Node::End),
            Some('(') => match self.group()? {
                Node::Look { behind: true, .. } if self.at_quantifier()? => return Err(NOTHING_TO_REPEAT),
                node @ Node::Look { behind: true, .. } => return Ok(node),
                node @ Node::Look { .. } if self.unicode => {
                    if self.at_quantifier()? {
                        return Err(NOTHING_TO_REPEAT);
                    }
                    return Ok(node);
                }
                node => node,
            },
            Some('.') => Node::Any,
            Some('[') => self.class()?,
            Some('\\') => match self.peek() {
                None => return Err(SyntaxError("\\ at end of pattern")),
                Some(next) if next == ch('b') || next == ch('B') => {
                    self.pos += 1;
                    return Ok(Node::WordBoundary {
                        negated: next == ch('B'),
                    });
                }
                Some(_) => self.atom_escape()?,
            },
            Some('*' | '+' | '?') => return Err(NOTHING_TO_REPEAT),
            Some('{') => {
                self.pos = start;
                if self.interval()?.is_some() {
                    return Err(NOTHING_TO_REPEAT);
                }
                if self.unicode {
                    return Err(LONE_QUANTIFIER_BRACKETS);
                }
                self.pos = start + 1;
                Node::Char(c)
            }
            Some(']' | '}') if self.unicode => return Err(LONE_QUANTIFIER_BRACKETS),
            _ => Node::Char(c),
        };
        self.quantified(atom, first_capture)
    }

    /// Whether a quantifier starts here.
    fn at_quantifier(&mut self) -> Result<bool, SyntaxError> {
        match self.peek() {
            Some(unit) if unit == ch('*') || unit == ch('+') || unit == ch('?') => Ok(true),
            Some(unit) if unit == ch('{') => {
                let start = self.pos;
                let found = self.interval()?.is_some();
                self.pos = start;
                Ok(found)
            }
            _ => Ok(false),
        }
    }

    /// `{n}`, `{n,}` or `{n,m}` at the current position; anything else leaves the position alone.
    fn interval(&mut self) -> Result<Option<(u32, Option<u32>)>, SyntaxError> {
        let start = self.pos;
        self.pos += 1;
        let Some(min) = self.decimal() else {
            self.pos = start;
            return Ok(None);
        };
        let max = if self.eat('}') {
            return Ok(Some((min, Some(min))));
        } else if self.eat(',') {
            if self.eat('}') {
                return Ok(Some((min, None)));
            }
            match self.decimal() {
                Some(max) if self.eat('}') => max,
                _ => {
                    self.pos = start;
                    return Ok(None);
                }
            }
        } else {
            self.pos = start;
            return Ok(None);
        };
        if min > max {
            return Err(SyntaxError("numbers out of order in quantifier"));
        }
        Ok(Some((min, Some(max))))
    }

    fn decimal(&mut self) -> Option<u32> {
        let start = self.pos;
        let mut value = 0u32;
        while let Some(digit) = self.peek().and_then(decimal_digit) {
            value = value.saturating_mul(10).saturating_add(digit).min(QUANTIFIER_INFINITY);
            self.pos += 1;
        }
        (self.pos > start).then_some(value)
    }

    fn quantified(&mut self, atom: Node, first_capture: usize) -> Result<Node, SyntaxError> {
        let (min, max) = match self.peek() {
            Some(unit) if unit == ch('*') => {
                self.pos += 1;
                (0, None)
            }
            Some(unit) if unit == ch('+') => {
                self.pos += 1;
                (1, None)
            }
            Some(unit) if unit == ch('?') => {
                self.pos += 1;
                (0, Some(1))
            }
            Some(unit) if unit == ch('{') => match self.interval()? {
                Some(bounds) => bounds,
                None if self.unicode => return Err(INVALID_ESCAPE),
                None => return Ok(atom),
            },
            _ => return Ok(atom),
        };
        let greedy = !self.eat('?');
        let max = max.filter(|&max| max != QUANTIFIER_INFINITY);
        Ok(Node::Repeat(Repeat {
            body: Box::new(atom),
            min,
            max,
            greedy,
            captures: (first_capture, self.next_group),
        }))
    }

    fn group(&mut self) -> Result<Node, SyntaxError> {
        if !self.eat('?') {
            let number = self.take_group_number();
            return self.close_group(|body| Node::Capture(number, body));
        }
        let kind = self.peek().ok_or(INVALID_GROUP)?;
        self.pos += 1;
        match char::from_u32(u32::from(kind)) {
            Some(':') => self.close_group(|body| *body),
            Some('=') => self.close_group(|body| look(body, false, false)),
            Some('!') => self.close_group(|body| look(body, false, true)),
            Some('<') => {
                if self.eat('=') {
                    return self.close_group(|body| look(body, true, false));
                }
                if self.eat('!') {
                    return self.close_group(|body| look(body, true, true));
                }
                let name = self.group_name()?;
                if self.names.iter().any(|(known, _)| *known == name) {
                    return Err(SyntaxError("Duplicate capture group name"));
                }
                let number = self.take_group_number();
                self.names.push((name, number));
                self.close_group(|body| Node::Capture(number, body))
            }
            _ => Err(INVALID_GROUP),
        }
    }

    fn take_group_number(&mut self) -> usize {
        let number = self.next_group;
        self.next_group += 1;
        number
    }

    fn close_group(&mut self, wrap: impl FnOnce(Box<Node>) -> Node) -> Result<Node, SyntaxError> {
        if self.depth >= MAX_NESTING {
            return Err(SyntaxError("Regular expression too large"));
        }
        self.depth += 1;
        let body = self.disjunction();
        self.depth -= 1;
        let body = body?;
        if !self.eat(')') {
            return Err(SyntaxError("Unterminated group"));
        }
        Ok(wrap(Box::new(body)))
    }

    /// A group name after `<`, through the closing `>`: ASCII letters, digits, `$` and `_`, not starting with a digit.
    fn group_name(&mut self) -> Result<Vec<u16>, SyntaxError> {
        let mut name = Vec::new();
        loop {
            let unit = self.peek().ok_or(INVALID_CAPTURE_NAME)?;
            self.pos += 1;
            if unit == ch('>') {
                break;
            }
            let valid = unit == ch('$')
                || unit == ch('_')
                || is_ascii_letter(unit)
                || (!name.is_empty() && decimal_digit(unit).is_some());
            if !valid {
                return Err(INVALID_CAPTURE_NAME);
            }
            name.push(unit);
        }
        if name.is_empty() {
            return Err(INVALID_CAPTURE_NAME);
        }
        Ok(name)
    }

    fn fixed_hex(&mut self, count: usize) -> Option<u32> {
        let mut value = 0;
        for offset in 0..count {
            value = value * 16 + self.peek_at(offset).and_then(hex_digit)?;
        }
        self.pos += count;
        Some(value)
    }

    fn atom_escape(&mut self) -> Result<Node, SyntaxError> {
        let unit = self.src[self.pos];
        if let Some(digit) = decimal_digit(unit).filter(|&digit| digit != 0) {
            let start = self.pos;
            let mut value = 0usize;
            while let Some(digit) = self.peek().and_then(decimal_digit) {
                value = value.saturating_mul(10).saturating_add(digit as usize);
                self.pos += 1;
            }
            if value <= self.total_groups {
                return Ok(Node::BackReference(value));
            }
            if self.unicode {
                return Err(INVALID_ESCAPE);
            }
            self.pos = start;
            if digit >= 8 {
                self.pos += 1;
                return Ok(Node::Char(u32::from(unit)));
            }
            return Ok(Node::Char(self.legacy_octal()));
        }
        if unit == ch('k') {
            if !self.has_named_groups {
                self.pos += 1;
                return Ok(Node::Char(u32::from(unit)));
            }
            self.pos += 1;
            if !self.eat('<') {
                return Err(INVALID_NAMED_REFERENCE);
            }
            let name = self.group_name().map_err(|_| INVALID_NAMED_REFERENCE)?;
            self.references.push(name);
            // The number is filled in once every name is known.
            return Ok(Node::BackReference(usize::MAX - (self.references.len() - 1)));
        }
        if let Some(set) = self.class_escape()? {
            return Ok(Node::Class { set, negated: false });
        }
        Ok(Node::Char(self.character_escape(false)?))
    }

    /// `\d`, `\D`, `\s`, `\S`, `\w`, `\W`, and with the `u` flag `\p{..}` and `\P{..}`, at the current position.
    fn class_escape(&mut self) -> Result<Option<CharSet>, SyntaxError> {
        let Some(c) = self.peek().and_then(|unit| char::from_u32(u32::from(unit))) else {
            return Ok(None);
        };
        let max = self.max_char();
        let set = match c {
            'd' => CharSet::digits(),
            'D' => CharSet::digits().inverted(max),
            's' => CharSet::space(),
            'S' => CharSet::space().inverted(max),
            'w' => CharSet::word(),
            'W' => CharSet::word().inverted(max),
            'p' | 'P' if self.unicode => {
                self.pos += 1;
                let set = self.property()?;
                return Ok(Some(if c == 'P' { set.inverted(max) } else { set }));
            }
            _ => return Ok(None),
        };
        self.pos += 1;
        Ok(Some(set))
    }

    /// The `{Name}` of a property escape.
    fn property(&mut self) -> Result<CharSet, SyntaxError> {
        if !self.eat('{') {
            return Err(INVALID_PROPERTY_NAME);
        }
        let mut name = String::new();
        loop {
            let unit = self.peek().ok_or(INVALID_PROPERTY_NAME)?;
            self.pos += 1;
            if unit == ch('}') {
                break;
            }
            match char::from_u32(u32::from(unit)) {
                Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '=' => name.push(c),
                _ => return Err(INVALID_PROPERTY_NAME),
            }
        }
        let name = name.strip_prefix("General_Category=").unwrap_or(&name);
        let name = name.strip_prefix("gc=").unwrap_or(name);
        CharSet::property(name).ok_or(INVALID_PROPERTY_NAME)
    }

    /// The character a character escape stands for; the position is after the backslash.
    fn character_escape(&mut self, in_class: bool) -> Result<u32, SyntaxError> {
        let unit = self.peek().ok_or(SyntaxError("\\ at end of pattern"))?;
        let Some(c) = char::from_u32(u32::from(unit)) else {
            if self.unicode {
                return Err(INVALID_ESCAPE);
            }
            self.pos += 1;
            return Ok(u32::from(unit));
        };
        Ok(match c {
            'f' | 'n' | 'r' | 't' | 'v' => {
                self.pos += 1;
                match c {
                    'f' => 0x0C,
                    'n' => 0x0A,
                    'r' => 0x0D,
                    't' => 0x09,
                    _ => 0x0B,
                }
            }
            'c' => match self.peek_at(1) {
                Some(letter) if is_ascii_letter(letter) => {
                    self.pos += 2;
                    u32::from(letter % 32)
                }
                Some(extra) if !self.unicode && in_class && (decimal_digit(extra).is_some() || extra == ch('_')) => {
                    self.pos += 2;
                    u32::from(extra % 32)
                }
                _ if self.unicode => return Err(INVALID_ESCAPE),
                // The backslash stands for itself and the `c` is read again.
                _ => u32::from(ch('\\')),
            },
            '0' if self.unicode => {
                self.pos += 1;
                if self.peek().and_then(decimal_digit).is_some() {
                    return Err(INVALID_ESCAPE);
                }
                0
            }
            '0'..='7' if !self.unicode => self.legacy_octal(),
            'x' => {
                self.pos += 1;
                match self.fixed_hex(2) {
                    Some(value) => value,
                    None if self.unicode => return Err(INVALID_ESCAPE),
                    None => u32::from(ch('x')),
                }
            }
            'u' => {
                self.pos += 1;
                if self.unicode {
                    return self.unicode_escape();
                }
                self.fixed_hex(4).unwrap_or(u32::from(ch('u')))
            }
            'k' if in_class && self.has_named_groups => return Err(INVALID_ESCAPE),
            '-' if self.unicode && in_class => {
                self.pos += 1;
                u32::from(ch('-'))
            }
            _ if self.unicode => {
                if !"^$\\.*+?()[]{}|/".contains(c) {
                    return Err(INVALID_ESCAPE);
                }
                self.pos += 1;
                u32::from(unit)
            }
            _ => {
                self.pos += 1;
                u32::from(unit)
            }
        })
    }

    /// `\u` with the `u` flag: `XXXX` (joining an escaped surrogate pair) or `{X..}`.
    fn unicode_escape(&mut self) -> Result<u32, SyntaxError> {
        const INVALID: SyntaxError = SyntaxError("Invalid Unicode escape");
        if self.eat('{') {
            let mut value = 0u32;
            let mut digits = 0;
            while let Some(digit) = self.peek().and_then(hex_digit) {
                value = value.saturating_mul(16).saturating_add(digit);
                digits += 1;
                self.pos += 1;
            }
            if digits == 0 || value > MAX_CODE_POINT || !self.eat('}') {
                return Err(INVALID);
            }
            return Ok(value);
        }
        let lead = self.fixed_hex(4).ok_or(INVALID)?;
        if (0xD800..=0xDBFF).contains(&lead) && self.peek() == Some(ch('\\')) && self.peek_at(1) == Some(ch('u')) {
            let start = self.pos;
            self.pos += 2;
            if let Some(trail @ 0xDC00..=0xDFFF) = self.fixed_hex(4) {
                return Ok(0x10000 + ((lead - 0xD800) << 10) + (trail - 0xDC00));
            }
            self.pos = start;
        }
        Ok(lead)
    }

    /// A legacy octal escape: up to three digits, at most `\377`.
    fn legacy_octal(&mut self) -> u32 {
        let mut value = 0;
        let mut digits = 0;
        while digits < 3 {
            match self.peek().and_then(decimal_digit) {
                Some(digit) if digit < 8 && value * 8 + digit <= 0o377 => {
                    value = value * 8 + digit;
                    digits += 1;
                    self.pos += 1;
                }
                _ => break,
            }
        }
        value
    }

    fn class(&mut self) -> Result<Node, SyntaxError> {
        let negated = self.eat('^');
        let mut set = CharSet::default();
        loop {
            let unit = self.peek().ok_or(SyntaxError("Unterminated character class"))?;
            if unit == ch(']') {
                self.pos += 1;
                break;
            }
            let first = self.class_atom()?;
            if self.peek() != Some(ch('-')) {
                add_atom(&mut set, first);
                continue;
            }
            self.pos += 1;
            match self.peek() {
                None => return Err(SyntaxError("Unterminated character class")),
                Some(next) if next == ch(']') => {
                    add_atom(&mut set, first);
                    set.add_range(u32::from(ch('-')), u32::from(ch('-')));
                }
                Some(_) => match (first, self.class_atom()?) {
                    (ClassAtom::Char(low), ClassAtom::Char(high)) => {
                        if low > high {
                            return Err(SyntaxError("Range out of order in character class"));
                        }
                        set.add_range(low, high);
                    }
                    _ if self.unicode => return Err(SyntaxError("Invalid character class")),
                    (first, second) => {
                        add_atom(&mut set, first);
                        set.add_range(u32::from(ch('-')), u32::from(ch('-')));
                        add_atom(&mut set, second);
                    }
                },
            }
        }
        set.normalize();
        Ok(Node::Class { set, negated })
    }

    fn class_atom(&mut self) -> Result<ClassAtom, SyntaxError> {
        let start = self.pos;
        let c = self.next_char().ok_or(SyntaxError("Unterminated character class"))?;
        if c != u32::from(ch('\\')) {
            return Ok(ClassAtom::Char(c));
        }
        if let Some(set) = self.class_escape()? {
            return Ok(ClassAtom::Set(set));
        }
        if self.eat('b') {
            return Ok(ClassAtom::Char(0x08));
        }
        let escaped = self.character_escape(true);
        if escaped.is_err() && !self.unicode {
            self.pos = start + 1;
        }
        escaped.map(ClassAtom::Char)
    }

    /// Replaces the placeholders of named backreferences with capture numbers.
    fn resolve_references(&self, node: &mut Node) -> Result<(), SyntaxError> {
        match node {
            Node::BackReference(number) if *number > self.total_groups => {
                let name = &self.references[usize::MAX - *number];
                let (_, resolved) = self
                    .names
                    .iter()
                    .find(|(known, _)| known == name)
                    .ok_or(INVALID_NAMED_REFERENCE)?;
                *number = *resolved;
            }
            Node::Sequence(items) | Node::Alternation(items) => {
                for item in items {
                    self.resolve_references(item)?;
                }
            }
            Node::Capture(_, body) | Node::Look { body, .. } => self.resolve_references(body)?,
            Node::Repeat(repeat) => self.resolve_references(&mut repeat.body)?,
            _ => {}
        }
        Ok(())
    }
}

fn look(body: Box<Node>, behind: bool, negated: bool) -> Node {
    Node::Look { body, behind, negated }
}

fn add_atom(set: &mut CharSet, atom: ClassAtom) {
    match atom {
        ClassAtom::Char(c) => set.add_range(c, c),
        ClassAtom::Set(other) => set.add_set(&other),
    }
}

/// The code point of a surrogate pair.
pub fn surrogate_pair(lead: u16, trail: u16) -> u32 {
    0x10000 + ((u32::from(lead) - 0xD800) << 10) + (u32::from(trail) - 0xDC00)
}

fn decimal_digit(unit: u16) -> Option<u32> {
    char::from_u32(u32::from(unit))?.to_digit(10)
}

fn hex_digit(unit: u16) -> Option<u32> {
    char::from_u32(u32::from(unit))?.to_digit(16)
}

fn is_ascii_letter(unit: u16) -> bool {
    (0x41..=0x5A).contains(&unit) || (0x61..=0x7A).contains(&unit)
}
