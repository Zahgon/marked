// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The options of a parse. The original tests each of them for truthiness, so they are plain booleans here and the
//! command decides them with JavaScript's truthiness from whatever JSON value a request gives.

/// The options of a parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// GitHub Flavored Markdown: tables, strikethrough, task lists and URL autolinks.
    pub gfm: bool,
    /// With `gfm`, every newline in a paragraph is a line break.
    pub breaks: bool,
    /// The original `Markdown.pl` behaviour, overriding `gfm`.
    pub pedantic: bool,
    /// Errors become an HTML message instead of being returned.
    pub silent: bool,
}

impl Default for Options {
    /// The original's defaults: `gfm` on, everything else off.
    fn default() -> Self {
        Options {
            gfm: true,
            breaks: false,
            pedantic: false,
            silent: false,
        }
    }
}
