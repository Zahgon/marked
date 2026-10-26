// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The lexer: splits a document into block tokens, then lexes the inline content of every block in the order the
//! blocks queued it.
//!
//! Inline lexing waits until every block is known, because a reference may use a definition further down. Block
//! tokens therefore get a queue slot for their inline tokens (see [`Inline`]); [`Lexer::lex`] fills the slots in
//! queue order and hands the slots' tokens to their owners at the end. The tokenizer rules themselves are in
//! [`crate::tokenizer`].

use crate::helpers::normalize_label;
use crate::js::{JsError, JsString};
use crate::options::Options;
use crate::rules::{Flavour, Rules};
use crate::tokens::{Inline, Links, Token};

/// How deeply block and inline content may nest before lexing stops with the `RangeError` JavaScript throws
/// when its stack runs out.
const MAX_NESTING: usize = 2_000;

/// One entry of the inline queue: the source to lex and the slot its tokens go to.
#[derive(Debug)]
pub(crate) struct QueueEntry {
    pub(crate) src: JsString,
    pub(crate) slot: usize,
}

/// The lexer's state while it runs.
#[derive(Clone, Copy, Debug)]
pub(crate) struct State {
    /// Inside a link's text or after an opening `<a>` tag.
    pub(crate) in_link: bool,
    /// After an opening `<pre>`, `<code>`, `<kbd>` or `<script>` tag: text is left as written.
    pub(crate) in_raw_block: bool,
    /// A link was produced in the inline run being lexed.
    pub(crate) link_emitted: bool,
    /// Whether the inline run being lexed holds a `)`, without which no inline link can match.
    pub(crate) link_paren_possible: bool,
    /// Lexing at the top level of a document or block quote, where paragraphs are made.
    pub(crate) top: bool,
}

/// A lexer for one document.
pub struct Lexer {
    pub(crate) options: Options,
    pub(crate) rules: Rules,
    pub(crate) links: Links,
    pub(crate) state: State,
    pub(crate) queue: Vec<QueueEntry>,
    slots: Vec<Vec<Token>>,
    /// How many block and inline runs are being lexed inside one another.
    depth: usize,
}

/// The result of lexing a document: its tokens and its link reference definitions.
#[derive(Debug, Default)]
pub struct TokensList {
    pub tokens: Vec<Token>,
    pub links: Links,
}

impl Lexer {
    /// A lexer with the grammar the options select.
    pub fn new(options: Options) -> Lexer {
        let flavour = if options.pedantic {
            Flavour::Pedantic
        } else if options.gfm {
            if options.breaks {
                Flavour::Breaks
            } else {
                Flavour::Gfm
            }
        } else {
            Flavour::Normal
        };
        Lexer {
            options,
            rules: Rules::of(flavour),
            links: Links::default(),
            state: State {
                in_link: false,
                in_raw_block: false,
                link_emitted: false,
                link_paren_possible: true,
                top: true,
            },
            queue: Vec::new(),
            slots: Vec::new(),
            depth: 0,
        }
    }

    /// Lexes a document (`Lexer.lex`).
    pub fn lex(mut self, src: &JsString) -> Result<TokensList, JsError> {
        let src = JsString::from_units(self.rules.other.carriage_return.replace(src.units(), "\n"));
        let mut tokens = Vec::new();
        self.block_tokens(src, &mut tokens, false)?;
        let queue = std::mem::take(&mut self.queue);
        for entry in &queue {
            // An inline <pre>, <code>, <kbd> or <script> left open ends with the block holding it.
            self.state.in_raw_block = false;
            let mut slot_tokens = std::mem::take(&mut self.slots[entry.slot]);
            self.inline_tokens(&entry.src, &mut slot_tokens)?;
            self.slots[entry.slot] = slot_tokens;
        }
        self.resolve(&mut tokens);
        Ok(TokensList {
            tokens,
            links: self.links,
        })
    }

    /// Lexes inline content on its own (`Lexer.lexInline`).
    pub fn lex_inline(mut self, src: &JsString) -> Result<Vec<Token>, JsError> {
        let mut tokens = Vec::new();
        self.inline_tokens(src, &mut tokens)?;
        Ok(tokens)
    }

    /// Queues inline content for lexing once every block is known (`lexer.inline`).
    pub(crate) fn inline(&mut self, src: JsString) -> Inline {
        let slot = self.slots.len();
        self.slots.push(Vec::new());
        self.queue.push(QueueEntry { src, slot });
        Inline::queued(slot)
    }

    /// The tokens of an inline list, wherever they are kept while the lexer runs.
    pub(crate) fn inline_tokens_mut<'a>(&'a mut self, inline: &'a mut Inline) -> &'a mut Vec<Token> {
        match inline.slot {
            Some(slot) => &mut self.slots[slot],
            None => &mut inline.tokens,
        }
    }

    /// Points the last queue entry at new source, as the original does after merging a block into the last one.
    fn requeue_last(&mut self, src: &JsString) {
        if let Some(entry) = self.queue.last_mut() {
            entry.src = src.clone();
        }
    }

    /// Hands every filled slot to the token that owns it.
    fn resolve(&mut self, tokens: &mut [Token]) {
        for token in tokens {
            match token {
                Token::Heading { tokens, .. } | Token::Paragraph { tokens, .. } => self.resolve_inline(tokens),
                Token::Text(text) => {
                    if let Some(tokens) = &mut text.tokens {
                        self.resolve_inline(tokens);
                    }
                }
                Token::Blockquote { tokens, .. } => self.resolve(tokens),
                Token::List(list) => {
                    for item in &mut list.items {
                        self.resolve(&mut item.tokens);
                    }
                }
                Token::Table(table) => {
                    for cell in table.header.iter_mut().chain(table.rows.iter_mut().flatten()) {
                        self.resolve_inline(&mut cell.tokens);
                    }
                }
                _ => {}
            }
        }
    }

    fn resolve_inline(&mut self, inline: &mut Inline) {
        if let Some(slot) = inline.slot.take() {
            inline.tokens = std::mem::take(&mut self.slots[slot]);
        }
    }

    /// The error for a source that no rule consumed: thrown, or with the silent option written to standard error as
    /// the original's `console.error` does.
    fn infinite_loop_error(&self, src: &JsString) -> Result<(), JsError> {
        let message = format!("Infinite loop on byte: {}", src.unit_at(0).unwrap_or(0));
        if self.options.silent {
            eprintln!("{message}");
            Ok(())
        } else {
            Err(JsError::error(message))
        }
    }

    /// Lexes block content into `tokens` (`blockTokens`). `last_paragraph_clipped` makes a paragraph at the start
    /// continue the paragraph `tokens` ends with, as block quote continuations need.
    pub(crate) fn block_tokens(
        &mut self,
        src: JsString,
        tokens: &mut Vec<Token>,
        last_paragraph_clipped: bool,
    ) -> Result<(), JsError> {
        self.nested(|lexer| lexer.block_tokens_inner(src, tokens, last_paragraph_clipped))
    }

    /// Runs a nested lexing step, failing as JavaScript does when the nesting is too deep for the stack.
    fn nested<T>(&mut self, step: impl FnOnce(&mut Lexer) -> Result<T, JsError>) -> Result<T, JsError> {
        if self.depth >= MAX_NESTING {
            return Err(JsError::new("RangeError", "Maximum call stack size exceeded"));
        }
        self.depth += 1;
        let result = step(self);
        self.depth -= 1;
        result
    }

    fn block_tokens_inner(
        &mut self,
        src: JsString,
        tokens: &mut Vec<Token>,
        mut last_paragraph_clipped: bool,
    ) -> Result<(), JsError> {
        let other = self.rules.other;
        let mut src = src;
        if self.options.pedantic {
            let tabs = other.tab_char_global.replace(src.units(), "    ");
            src = JsString::from_units(other.space_line.replace(&tabs, ""));
        }

        let mut src_length = usize::MAX;
        while !src.is_empty() {
            if src.len() < src_length {
                src_length = src.len();
            } else {
                self.infinite_loop_error(&src)?;
                break;
            }

            // newline
            if let Some(token) = self.space(&src) {
                src = src.slice_from(token.raw().len());
                match tokens.last_mut() {
                    // A single \n terminates the last line: move it there, so no paragraph break appears.
                    Some(last) if token.raw().len() == 1 => last.raw_mut().push_str("\n"),
                    _ => tokens.push(token),
                }
                continue;
            }

            // code
            if let Some(token) = self.code(&src) {
                src = src.slice_from(token.raw().len());
                match tokens.last_mut() {
                    // An indented code block cannot interrupt a paragraph.
                    Some(last) if last.is_paragraph_or_text() => {
                        let text = self.merge_into(last, &token, true);
                        self.requeue_last(&text);
                    }
                    _ => tokens.push(token),
                }
                continue;
            }

            // fences, heading, hr, blockquote, list, html
            if let Some(token) = self.fences(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.heading(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.hr(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.blockquote(&src)? {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.list(&src)? {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.html(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }

            // def
            if let Some(token) = self.def(&src) {
                src = src.slice_from(token.raw().len());
                match tokens.last_mut() {
                    Some(last) if last.is_paragraph_or_text() => {
                        let text = self.merge_into(last, &token, false);
                        self.requeue_last(&text);
                    }
                    _ => {
                        if let Token::Def(def) = &token {
                            // The first definition of a label wins, but a later one still consumes its text.
                            self.links.define(
                                def.tag.clone(),
                                crate::tokens::LinkDef {
                                    href: def.href.clone(),
                                    title: def.title.clone(),
                                },
                            );
                        }
                        tokens.push(token);
                    }
                }
                continue;
            }

            // table (gfm), lheading
            if let Some(token) = self.table(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.lheading(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }

            // top-level paragraph
            if self.state.top {
                if let Some(token) = self.paragraph(&src) {
                    let raw_len = token.raw().len();
                    match tokens.last_mut() {
                        Some(last @ Token::Paragraph { .. }) if last_paragraph_clipped => {
                            let text = self.merge_into(last, &token, true);
                            self.queue.pop();
                            self.requeue_last(&text);
                        }
                        _ => tokens.push(token),
                    }
                    last_paragraph_clipped = false;
                    src = src.slice_from(raw_len);
                    continue;
                }
            }

            // text
            if let Some(token) = self.text(&src) {
                src = src.slice_from(token.raw().len());
                match tokens.last_mut() {
                    Some(last @ Token::Text(_)) => {
                        let text = self.merge_into(last, &token, true);
                        self.queue.pop();
                        self.requeue_last(&text);
                    }
                    _ => tokens.push(token),
                }
                continue;
            }

            if !src.is_empty() {
                self.infinite_loop_error(&src)?;
                break;
            }
        }

        self.state.top = true;
        Ok(())
    }

    /// Appends a block to the paragraph or text before it: its raw source on a new line, and on a new line its text
    /// (or with `use_text` false its raw source, as a definition has no text). Returns the merged text.
    fn merge_into(&self, last: &mut Token, token: &Token, use_text: bool) -> JsString {
        let raw = last.raw_mut();
        if !raw.ends_with("\n") {
            raw.push_str("\n");
        }
        raw.push_js(token.raw());
        let addition = match token {
            Token::Code(code) if use_text => code.text.clone(),
            Token::Paragraph { text, .. } if use_text => text.clone(),
            Token::Text(text) if use_text => text.text.clone(),
            _ => token.raw().clone(),
        };
        match last.text_mut() {
            Some(text) => {
                text.push_str("\n");
                text.push_js(&addition);
                text.clone()
            }
            None => JsString::new(),
        }
    }

    /// Lexes inline content, appending to `tokens` (`inlineTokens`).
    pub(crate) fn inline_tokens(&mut self, src: &JsString, tokens: &mut Vec<Token>) -> Result<(), JsError> {
        // One paren scan per inline run: without a ')' ahead the link rule cannot match. A nested run is a part of
        // the outer one, so it has no ')' when the outer run has none.
        let outer = self.state.link_paren_possible;
        self.state.link_paren_possible = outer && src.includes(")");
        let result = self.nested(|lexer| lexer.inline_tokens_inner(src, tokens));
        self.state.link_paren_possible = outer;
        result
    }

    fn inline_tokens_inner(&mut self, src: &JsString, tokens: &mut Vec<Token>) -> Result<(), JsError> {
        // The masked source is only read by emphasis and strikethrough, which need a *, _ or ~ to start.
        let masked_src = if src.includes("*") || src.includes("_") || src.includes("~") {
            self.mask_src(src)
        } else {
            src.clone()
        };

        let mut src = src.clone();
        let mut keep_prev_char = false;
        let mut prev_char = JsString::new();
        let mut src_length = usize::MAX;
        while !src.is_empty() {
            if src.len() < src_length {
                src_length = src.len();
            } else {
                self.infinite_loop_error(&src)?;
                break;
            }

            if !keep_prev_char {
                prev_char = JsString::new();
            }
            keep_prev_char = false;

            if let Some(token) = self.escape(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.tag(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.link(&src)? {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.reflink(&src)? {
                src = src.slice_from(token.raw().len());
                match (tokens.last_mut(), &token) {
                    (Some(Token::Text(last)), Token::Text(text)) => {
                        last.raw.push_js(&text.raw);
                        last.text.push_js(&text.text);
                    }
                    _ => tokens.push(token),
                }
                continue;
            }
            if let Some(token) = self.em_strong(&src, &masked_src, &prev_char)? {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.codespan(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.br(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.del(&src, &masked_src, &prev_char)? {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if let Some(token) = self.autolink(&src) {
                src = src.slice_from(token.raw().len());
                tokens.push(token);
                continue;
            }
            if !self.state.in_link {
                if let Some(token) = self.url(&src) {
                    src = src.slice_from(token.raw().len());
                    tokens.push(token);
                    continue;
                }
            }

            if let Some(token) = self.inline_text(&src) {
                src = src.slice_from(token.raw().len());
                let last_unit = token.raw().slice(-1, None);
                // Track the character before a run of underscores started.
                if !last_unit.eq_str("_") {
                    prev_char = last_unit;
                }
                keep_prev_char = true;
                match (tokens.last_mut(), &token) {
                    (Some(Token::Text(last)), Token::Text(text)) => {
                        last.raw.push_js(&text.raw);
                        last.text.push_js(&text.text);
                    }
                    _ => tokens.push(token),
                }
                continue;
            }

            if !src.is_empty() {
                self.infinite_loop_error(&src)?;
                break;
            }
        }
        Ok(())
    }

    /// Whether a label is defined (`Object.hasOwn(links, normalizeLabel(label))`).
    fn has_link(&self, label: &JsString) -> bool {
        self.links.get(&normalize_label(label)).is_some()
    }

    /// Whether link text holds a link already. An image does not count: an image may hold a link, a link may not.
    fn link_in_text(&self, text: &JsString) -> bool {
        if !text.includes("[") {
            return false;
        }
        let inline = self.rules.inline;
        for found in inline.block_skip.match_all(text.units()) {
            // The `!` of an image is left out of the match, so read the character before it.
            let matched = found.group_or_empty(text.units(), 0);
            let before = found.index().checked_sub(1).and_then(|at| text.unit_at(at));
            if inline.link.test(matched) && before != Some(u16::from(b'!')) {
                return true;
            }
        }
        for found in inline.reflink_search.match_all(text.units()) {
            let matched = JsString::from_slice(found.group_or_empty(text.units(), 0));
            let ref_start = matched.last_index_of("[").unwrap_or(0);
            let label = matched.slice(ref_start as isize + 1, Some(-1));
            if matched.unit_at(0) == Some(u16::from(b'!')) || !self.has_link(&label) {
                continue;
            }
            // A candidate holding a link is not a link either, so it does not count.
            if ref_start > 1 && self.link_in_text(&matched.slice(1, Some(ref_start as isize - 1))) {
                continue;
            }
            return true;
        }
        false
    }

    /// Masks a reference link candidate whose label is defined.
    fn mask_reflink(&self, matched: &JsString) -> JsString {
        let ref_start = matched.last_index_of("[").unwrap_or(0);
        if !self.has_link(&matched.slice(ref_start as isize + 1, Some(-1))) {
            return matched.clone();
        }
        // A candidate whose text already holds a link never becomes a link, so only the links inside it are
        // masked, leaving the emphasis that still applies around them. Images are exempt: their text becomes alt
        // text.
        if ref_start > 1 && matched.unit_at(0) != Some(u16::from(b'!')) {
            let text = matched.slice(1, Some(ref_start as isize - 1));
            if self.link_in_text(&text) {
                let inner = self
                    .rules
                    .inline
                    .reflink_search
                    .replace_with(text.units(), |found, input| {
                        self.mask_reflink(&JsString::from_slice(found.group_or_empty(input, 0)))
                            .into_units()
                    });
                let mut out = JsString::from("[");
                out.push_units(&inner);
                out.push_str("][");
                out.push_js(&JsString::repeat("a", matched.len().saturating_sub(ref_start + 2)));
                out.push_str("]");
                return out;
            }
        }
        let mut out = JsString::from("[");
        out.push_js(&JsString::repeat("a", matched.len().saturating_sub(2)));
        out.push_str("]");
        out
    }

    /// Masks links, code, html and escapes so that emphasis and strikethrough do not see delimiters inside them.
    /// Every mask keeps the length of what it replaces.
    fn mask_src(&self, src: &JsString) -> JsString {
        let inline = self.rules.inline;
        let mut masked = src.units().to_vec();

        if src.includes("[") {
            masked = inline.reflink_search.replace_with(&masked, |found, input| {
                self.mask_reflink(&JsString::from_slice(found.group_or_empty(input, 0)))
                    .into_units()
            });
        }

        // An escaped character becomes `+`s; inside a code span a backslash is literal, so the span's backticks
        // are masked instead, which keeps the whole span skippable.
        let backtick = &self.rules.other.backtick_global;
        masked = inline
            .any_punctuation_outside_code
            .replace_with(&masked, |found, input| {
                let matched = found.group_or_empty(input, 0);
                if found.has(2) {
                    return vec![u16::from(b'+'); matched.len()];
                }
                if found.has(1) {
                    let fence = found.group_or_empty(input, 1);
                    let inner = &matched[fence.len()..matched.len() - fence.len()];
                    let mut out = fence.to_vec();
                    out.extend(backtick.replace(inner, "+"));
                    out.extend_from_slice(fence);
                    return out;
                }
                matched.to_vec()
            });

        masked = inline.block_skip.replace_with(&masked, |found, input| {
            let matched = found.group_or_empty(input, 0);
            let offset = found.group(input, 2).map_or(0, <[u16]>::len);
            let mut out = matched[..offset].to_vec();
            out.push(u16::from(b'['));
            out.extend(std::iter::repeat_n(
                u16::from(b'a'),
                matched.len().saturating_sub(offset + 2),
            ));
            out.push(u16::from(b']'));
            out
        });

        JsString::from_units(masked)
    }
}
