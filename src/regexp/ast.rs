// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The syntax tree of a regular expression.

use super::charset::CharSet;

/// A parsed pattern.
#[derive(Clone, Debug)]
pub struct Pattern {
    /// The root of the tree.
    pub root: Node,
    /// The number of capture groups, not counting capture 0.
    pub groups: usize,
}

impl Pattern {
    /// `(?!)`, which matches nowhere.
    pub fn never() -> Pattern {
        Pattern {
            root: Node::Look {
                body: Box::new(Node::Sequence(Vec::new())),
                behind: false,
                negated: true,
            },
            groups: 0,
        }
    }
}

/// A node of the tree. A character is a code unit without the `u` flag and a code point with it.
#[derive(Clone, Debug)]
pub enum Node {
    /// One literal character.
    Char(u32),
    /// `.`.
    Any,
    /// A character class; `negated` is the leading `^`, which applies after any case folding.
    Class {
        /// The members as written.
        set: CharSet,
        /// Whether the class was written with `^`.
        negated: bool,
    },
    /// Terms in source order.
    Sequence(Vec<Node>),
    /// Alternatives in source order.
    Alternation(Vec<Node>),
    /// A capture group with its number.
    Capture(usize, Box<Node>),
    /// A quantified atom.
    Repeat(Repeat),
    /// `^`.
    Start,
    /// `$`.
    End,
    /// `\b`, or `\B` when negated.
    WordBoundary {
        /// Whether this is `\B`.
        negated: bool,
    },
    /// A lookahead or lookbehind.
    Look {
        /// The asserted pattern.
        body: Box<Node>,
        /// Whether this is a lookbehind.
        behind: bool,
        /// Whether the assertion is negative.
        negated: bool,
    },
    /// A backreference to a capture group.
    BackReference(usize),
}

impl Node {
    /// Whether every match of the node has to start at the beginning of the input (`^` first in every alternative).
    pub fn anchored(&self) -> bool {
        match self {
            Node::Start => true,
            Node::Sequence(items) => items.first().is_some_and(Node::anchored),
            Node::Alternation(items) => items.iter().all(Node::anchored),
            Node::Capture(_, body) => body.anchored(),
            _ => false,
        }
    }
}

/// A quantified atom.
#[derive(Clone, Debug)]
pub struct Repeat {
    /// The repeated atom.
    pub body: Box<Node>,
    /// The fewest iterations.
    pub min: u32,
    /// The most iterations; `None` is unbounded.
    pub max: Option<u32>,
    /// Whether more iterations are tried first.
    pub greedy: bool,
    /// The capture groups inside the atom, which every iteration resets: `first..end`.
    pub captures: (usize, usize),
}
