// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The tokenizer: one method per construct, each trying its rule at the start of the source and building the token.
//! The methods belong to the [`Lexer`], whose state and inline queue they use.

use crate::helpers::{
    decode_numeric_character_references, expand_tabs, find_closing_bracket, normalize_label, rtrim, split_cells,
    trim_trailing_blank_lines, Closing,
};
use crate::js::{JsError, JsString};
use crate::lexer::Lexer;
use crate::regexp::{Match, RegExp};
use crate::tokens::{Align, Code, Def, Html, Link, LinkDef, List, ListItem, Span, Table, TableCell, Text, Token};

const NEWLINE: u16 = b'\n' as u16;

/// A match together with the input it was made on, read the way the original reads `cap[n]`.
struct Captures {
    input: Vec<u16>,
    found: Match,
}

impl Captures {
    fn exec(regex: &RegExp, src: &JsString) -> Option<Captures> {
        regex.exec(src.units()).map(|found| Captures {
            input: src.units().to_vec(),
            found,
        })
    }

    /// `cap[n]`, `None` when the group took no part.
    fn get(&self, group: usize) -> Option<JsString> {
        self.found.group(&self.input, group).map(JsString::from_slice)
    }

    /// `cap[n] || ''`.
    fn text(&self, group: usize) -> JsString {
        self.get(group).unwrap_or_default()
    }

    /// The truthiness of `cap[n]`.
    fn has(&self, group: usize) -> bool {
        self.found.has(group)
    }
}

/// Removes up to the fence's own indentation from every line of a fenced code block's content.
fn indent_code_compensation(lexer: &Lexer, raw: &JsString, text: &JsString) -> JsString {
    let other = lexer.rules.other;
    let Some(indent_to_code) = Captures::exec(&other.indent_code_compensation, raw).map(|cap| cap.text(1)) else {
        return text.clone();
    };
    let lines: Vec<JsString> = text
        .split_str("\n")
        .into_iter()
        .map(|node| match other.beginning_space.exec(node.units()) {
            None => node,
            Some(found) => node.slice_from((found.end() - found.index()).min(indent_to_code.len())),
        })
        .collect();
    JsString::join(&lines, "\n")
}

/// The flavour-independent part of the label check: whether the label's end falls inside an HTML tag or autolink
/// that starts in the label.
fn is_label_end_inside_token(lexer: &Lexer, src: &JsString, label: &JsString, label_start: usize) -> bool {
    if !label.includes("<") {
        return false;
    }
    let inline = lexer.rules.inline;
    let units = label.units();
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        if unit == u16::from(b'\\') {
            i += 2;
            continue;
        }
        if unit == u16::from(b'`') {
            if let Some(code) = inline.code.exec(&units[i..]) {
                i += code.end();
                continue;
            }
        }
        if unit != u16::from(b'<') {
            i += 1;
            continue;
        }
        let token_src = src.slice_from(label_start + i);
        let token = inline
            .tag
            .exec(token_src.units())
            .or_else(|| inline.autolink.exec(token_src.units()));
        let Some(token) = token else {
            i += 1;
            continue;
        };
        if token.end() > units.len() - i {
            return true;
        }
        i += token.end().max(1);
    }
    false
}

fn concat(parts: &[&JsString]) -> JsString {
    let mut out = JsString::new();
    for part in parts {
        out.push_js(part);
    }
    out
}

impl Lexer {
    /// Blank lines.
    pub(crate) fn space(&self, src: &JsString) -> Option<Token> {
        let found = self.rules.block.newline.exec(src.units())?;
        (found.end() > 0).then(|| Token::Space {
            raw: src.substring(0, found.end()),
        })
    }

    /// An indented code block.
    pub(crate) fn code(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.code, src)?;
        let whole = cap.text(0);
        let raw = if self.options.pedantic {
            whole
        } else {
            trim_trailing_blank_lines(&whole)
        };
        let text = JsString::from_units(self.rules.other.code_remove_indent.replace(raw.units(), ""));
        Some(Token::Code(Code {
            raw,
            indented: true,
            lang: None,
            text,
        }))
    }

    /// A fenced code block.
    pub(crate) fn fences(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.fences, src)?;
        let raw = cap.text(0);
        let text = indent_code_compensation(self, &raw, &cap.text(3));
        let lang = cap.get(2).map(|lang| {
            if lang.is_empty() {
                lang
            } else {
                JsString::from_units(self.rules.inline.any_punctuation.replace(lang.trim().units(), "$1"))
            }
        });
        Some(Token::Code(Code {
            raw,
            indented: false,
            lang,
            text,
        }))
    }

    /// An ATX heading.
    pub(crate) fn heading(&mut self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.heading, src)?;
        let other = self.rules.other;
        let mut text = cap.text(2).trim();
        if other.ending_hash.test(text.units()) {
            let trimmed = rtrim(&text, u16::from(b'#'), false);
            if self.options.pedantic || trimmed.is_empty() || other.ending_space_tab_char.test(trimmed.units()) {
                // CommonMark requires a space or tab before trailing #s.
                text = trimmed.trim();
            }
        }
        let tokens = self.inline(text.clone());
        Some(Token::Heading {
            raw: rtrim(&cap.text(0), NEWLINE, false),
            depth: cap.text(1).len(),
            text,
            tokens,
        })
    }

    /// A thematic break.
    pub(crate) fn hr(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.hr, src)?;
        Some(Token::Hr {
            raw: rtrim(&cap.text(0), NEWLINE, false),
        })
    }

    /// A block quote, with the lazy continuation lines and nested quotes and lists that follow it.
    pub(crate) fn blockquote(&mut self, src: &JsString) -> Result<Option<Token>, JsError> {
        let Some(cap) = Captures::exec(&self.rules.block.blockquote, src) else {
            return Ok(None);
        };
        let other = self.rules.other;
        let mut lines = rtrim(&cap.text(0), NEWLINE, false).split_str("\n");
        let mut raw = JsString::new();
        let mut text = JsString::new();
        let mut tokens: Vec<Token> = Vec::new();

        while !lines.is_empty() {
            let mut in_blockquote = false;
            let mut current_lines = Vec::new();
            let mut i = 0;
            while i < lines.len() {
                // Take the lines up to a continuation.
                if other.blockquote_start.test(lines[i].units()) {
                    current_lines.push(lines[i].clone());
                    in_blockquote = true;
                } else if !in_blockquote {
                    current_lines.push(lines[i].clone());
                } else {
                    break;
                }
                i += 1;
            }
            lines.drain(..i);

            let current_raw = JsString::join(&current_lines, "\n");
            // A tab after the marker runs to the next tab stop and the marker only takes one space of it.
            let expanded = other
                .blockquote_marker_whitespace
                .replace_with(current_raw.units(), |found, input| {
                    let marker = found.group_or_empty(input, 1);
                    let whitespace = JsString::from_slice(found.group_or_empty(input, 2));
                    let mut out = marker.to_vec();
                    out.extend(expand_tabs(&whitespace, marker.len()).into_units());
                    out
                });
            // Precede a setext underline with 4 spaces so that it is not one.
            let setext = other.blockquote_setext_replace.replace(&expanded, "\n    $1");
            let current_text = JsString::from_units(other.blockquote_setext_replace2.replace(&setext, ""));
            raw = if raw.is_empty() {
                current_raw.clone()
            } else {
                concat(&[&raw, &JsString::from("\n"), &current_raw])
            };
            text = if text.is_empty() {
                current_text.clone()
            } else {
                concat(&[&text, &JsString::from("\n"), &current_text])
            };

            // Lex the lines as top level blocks, merging a continued paragraph.
            let top = self.state.top;
            self.state.top = true;
            self.block_tokens(current_text, &mut tokens, true)?;
            self.state.top = top;

            if lines.is_empty() {
                break;
            }

            match tokens.last() {
                Some(Token::Code(_) | Token::Heading { .. } | Token::Hr { .. } | Token::Html(_)) => break,
                Some(Token::Blockquote {
                    raw: old_raw,
                    text: old_text,
                    ..
                }) => {
                    // The continuation lines belong to the nested quote, which already lost one `>`, so they lose
                    // one too.
                    let old_raw = old_raw.clone();
                    let old_text_len = old_text.len();
                    let continuation = JsString::join(&lines, "\n");
                    let stripped = other.blockquote_setext_replace2.replace(continuation.units(), "");
                    let new_text = concat(&[&old_raw, &JsString::from("\n"), &JsString::from_units(stripped)]);
                    let Some(new_token) = self.blockquote(&new_text)? else {
                        break;
                    };
                    let leftover = new_text.slice_from(new_token.raw().len());
                    let leftover = JsString::from_units(other.leading_newline.replace(leftover.units(), ""));
                    let leftover_count = if leftover.is_empty() {
                        0
                    } else {
                        leftover.split_str("\n").len()
                    };
                    let consumed = &lines[..lines.len().saturating_sub(leftover_count)];
                    if !consumed.is_empty() {
                        raw = concat(&[&raw, &JsString::from("\n"), &JsString::join(consumed, "\n")]);
                    }
                    let new_token_text = match &new_token {
                        Token::Blockquote { text, .. } => text.clone(),
                        _ => JsString::new(),
                    };
                    text = concat(&[
                        &text.substring(0, text.len().saturating_sub(old_text_len)),
                        &new_token_text,
                    ]);
                    if let Some(last) = tokens.last_mut() {
                        *last = new_token;
                    }
                    break;
                }
                Some(Token::List(old)) => {
                    let old_raw = old.raw.clone();
                    let new_text = concat(&[&old_raw, &JsString::from("\n"), &JsString::join(&lines, "\n")]);
                    let Some(new_token) = self.list(&new_text)? else {
                        break;
                    };
                    // The list's raw has no `>` markers, so count the continuation lines it took instead.
                    let new_len = new_token.raw().len();
                    let consumed_count = if new_len > old_raw.len() {
                        new_text
                            .slice(old_raw.len() as isize + 1, Some(new_len as isize))
                            .split_str("\n")
                            .len()
                    } else {
                        0
                    };
                    if let Some(last) = tokens.last_mut() {
                        *last = new_token;
                    }
                    if consumed_count > 0 {
                        let consumed = JsString::join(&lines[..consumed_count.min(lines.len())], "\n");
                        raw = concat(&[&raw, &JsString::from("\n"), &consumed]);
                        text = concat(&[&text, &JsString::from("\n"), &consumed]);
                    }
                    lines.drain(..consumed_count.min(lines.len()));
                }
                _ => {}
            }
        }

        Ok(Some(Token::Blockquote { raw, tokens, text }))
    }

    /// A list and its items.
    pub(crate) fn list(&mut self, src: &JsString) -> Result<Option<Token>, JsError> {
        let Some(cap) = Captures::exec(&self.rules.block.list, src) else {
            return Ok(None);
        };
        let other = self.rules.other;
        let pedantic = self.options.pedantic;
        let list_src = src.clone();
        let mut src = src.clone();
        let bull = cap.text(1).trim();
        let ordered = bull.len() > 1;
        let start = ordered.then(|| {
            bull.units()[..bull.len() - 1]
                .iter()
                .fold(0u32, |value, &unit| value * 10 + u32::from(unit - u16::from(b'0')))
        });
        let last_char = bull.slice(-1, None).to_string_lossy();
        let bull_pattern = if ordered {
            format!("\\d{{1,9}}\\{last_char}")
        } else if pedantic {
            String::from("[*+-]")
        } else {
            format!("\\{last_char}")
        };
        let item_regex = crate::rules::Other::list_item(&bull_pattern);

        let mut list = List {
            raw: JsString::new(),
            ordered,
            start,
            loose: false,
            items: Vec::new(),
        };
        let mut ends_with_blank_line = false;

        while !src.is_empty() {
            let mut end_early = false;
            let mut item_contents = JsString::new();
            let Some(cap) = Captures::exec(&item_regex, &src) else {
                break;
            };
            // A bullet that is really a thematic break ends the list.
            if self.rules.block.hr.test(src.units()) {
                break;
            }

            let item_start = list_src.len() - src.len();
            src = src.slice_from(cap.text(0).len());

            let first_line = cap.text(2).first_part("\n");
            let bullet_indent = cap.text(1).len();
            let mut line = if pedantic {
                expand_tabs(&first_line, bullet_indent)
            } else {
                JsString::from_units(
                    other
                        .leading_space_tab
                        .replace_with(first_line.units(), |found, input| {
                            expand_tabs(&JsString::from_slice(found.group_or_empty(input, 0)), bullet_indent)
                                .into_units()
                        }),
                )
            };
            let mut next_line = src.first_part("\n");
            let mut blank_line = line.trim().is_empty();

            let mut indent: isize;
            if pedantic {
                indent = 2;
                item_contents = line.trim_start();
            } else if blank_line {
                indent = bullet_indent as isize + 1;
            } else {
                indent = search(&other.non_space_char, &line);
                // Treat indented code (more than 4 spaces) as having only 1 indent.
                if indent > 4 {
                    indent = 1;
                }
                item_contents = line.slice(indent, None);
                indent += bullet_indent as isize;
            }

            // Items begin with at most one blank line.
            if blank_line && other.blank_line.test(next_line.units()) {
                src = src.slice_from(next_line.len() + 1);
                end_early = true;
            }

            if !end_early {
                let width = indent.max(0) as usize;
                let next_bullet = other.next_bullet.get(width);
                let hr = other.hr.get(width);
                let fences_begin = other.fences_begin.get(width);
                let heading_begin = if pedantic {
                    other.pedantic_heading_begin.get(width)
                } else {
                    other.heading_begin.get(width)
                };
                let html_begin = other.html_begin.get(width);
                let blockquote_begin = other.blockquote_begin.get(width);

                // Check whether the following lines belong to the item.
                while !src.is_empty() {
                    let raw_line = src.first_part("\n");
                    next_line = raw_line.clone();
                    let next_line_without_tabs;
                    if pedantic {
                        // Re-align to follow CommonMark nesting rules.
                        next_line = JsString::from_units(other.list_replace_nesting.replace(next_line.units(), "  "));
                        next_line_without_tabs = next_line.clone();
                    } else {
                        next_line_without_tabs = JsString::from_units(other.leading_space_tab.replace_with(
                            next_line.units(),
                            |found, input| {
                                expand_tabs(&JsString::from_slice(found.group_or_empty(input, 0)), 0).into_units()
                            },
                        ));
                    }

                    // A fence, heading, html block, block quote, new bullet or thematic break ends the item.
                    if fences_begin.test(next_line.units())
                        || heading_begin.test(next_line.units())
                        || html_begin.test(next_line.units())
                        || blockquote_begin.test(next_line.units())
                        || next_bullet.test(next_line.units())
                        || hr.test(next_line.units())
                    {
                        break;
                    }

                    if search(&other.non_space_char, &next_line_without_tabs) >= indent || next_line.trim().is_empty() {
                        // Dedent if possible.
                        item_contents.push_str("\n");
                        item_contents.push_js(&next_line_without_tabs.slice(indent, None));
                    } else {
                        // Not enough indentation.
                        if blank_line {
                            break;
                        }
                        // A paragraph continues unless the last line was another kind of block.
                        let detabbed = JsString::from_units(other.tab_char_global.replace(line.units(), "    "));
                        if search(&other.non_space_char, &detabbed) >= 4
                            || fences_begin.test(line.units())
                            || heading_begin.test(line.units())
                            || hr.test(line.units())
                        {
                            break;
                        }
                        // A lazy continuation can look like a setext underline once the list indentation is
                        // gone; keep it in the paragraph.
                        let setext_continuation = !pedantic
                            && other.setext_underline_line.test(next_line.units())
                            && self
                                .rules
                                .block
                                .lheading
                                .test(concat(&[&line, &JsString::from("\n"), &next_line]).units());
                        let continuation = if setext_continuation {
                            let mut out = JsString::from("    ");
                            out.push_units(&other.leading_three_spaces.replace(next_line_without_tabs.units(), ""));
                            out
                        } else {
                            next_line.clone()
                        };
                        item_contents.push_str("\n");
                        item_contents.push_js(&continuation);
                    }

                    blank_line = next_line.trim().is_empty();
                    src = src.slice_from(raw_line.len() + 1);
                    line = next_line_without_tabs.slice(indent, None);
                }
            }

            let raw = list_src.slice(item_start as isize, Some((list_src.len() - src.len()) as isize));

            if !list.loose {
                // If the previous item ended with a blank line, the list is loose.
                if ends_with_blank_line {
                    list.loose = true;
                } else if other.double_blank_line.test(raw.units()) {
                    ends_with_blank_line = true;
                }
            }

            list.items.push(ListItem {
                raw,
                task: self.options.gfm && other.list_is_task.test(item_contents.units()),
                checked: None,
                loose: false,
                text: item_contents,
                tokens: Vec::new(),
            });
        }

        // The final item keeps no trailing newlines.
        let Some(last_item) = list.items.last_mut() else {
            return Ok(None);
        };
        last_item.raw = last_item.raw.trim_end();
        last_item.text = last_item.text.trim_end();
        list.raw = list_src
            .slice(0, Some((list_src.len() - src.len()) as isize))
            .trim_end();

        // First pass: lex the items and settle looseness from their spacers.
        for index in 0..list.items.len() {
            self.state.top = false;
            let mut tokens = Vec::new();
            self.block_tokens(list.items[index].text.clone(), &mut tokens, false)?;
            if !list.loose {
                list.loose = tokens
                    .iter()
                    .any(|token| matches!(token, Token::Space { raw } if other.any_line.test(raw.units())));
            }
            list.items[index].tokens = tokens;
        }

        // Second pass: place the task checkboxes.
        for index in 0..list.items.len() {
            let first_is_text = list.items[index]
                .tokens
                .first()
                .is_some_and(Token::is_paragraph_or_text);
            if !list.items[index].task {
                continue;
            }
            if !first_is_text {
                list.items[index].task = false;
                continue;
            }
            let replace_task = &other.list_replace_task;
            let item = &mut list.items[index];
            item.text = JsString::from_units(replace_task.replace(item.text.units(), ""));
            if let Some(first) = item.tokens.first_mut() {
                let stripped_raw = replace_task.replace(first.raw().units(), "");
                *first.raw_mut() = JsString::from_units(stripped_raw);
                if let Some(text) = first.text_mut() {
                    *text = JsString::from_units(replace_task.replace(text.units(), ""));
                }
            }
            if let Some(entry) = self
                .queue
                .iter_mut()
                .rev()
                .find(|entry| other.list_is_task.test(entry.src.units()))
            {
                entry.src = JsString::from_units(replace_task.replace(entry.src.units(), ""));
            }

            let item_raw = list.items[index].raw.clone();
            let Some(task) = other.list_task_checkbox.exec(item_raw.units()) else {
                continue;
            };
            let box_text = JsString::from_slice(task.group_or_empty(item_raw.units(), 0));
            let checked = !box_text.eq_str("[ ]");
            let mut checkbox_raw = box_text;
            checkbox_raw.push_str(" ");
            let checkbox = Token::Checkbox {
                raw: checkbox_raw.clone(),
                checked,
            };
            list.items[index].checked = Some(checked);
            if list.loose {
                let mut first = list.items[index].tokens.remove(0);
                let raw = concat(&[&checkbox_raw, first.raw()]);
                *first.raw_mut() = raw;
                if let Some(text) = first.text_mut() {
                    *text = concat(&[&checkbox_raw, text]);
                }
                if let Some(inline) = first.inline_mut() {
                    self.inline_tokens_mut(inline).insert(0, checkbox);
                }
                list.items[index].tokens.insert(0, first);
            } else {
                list.items[index].tokens.insert(0, checkbox);
            }
        }

        // A loose list makes every item loose and its text paragraphs.
        if list.loose {
            for item in &mut list.items {
                item.loose = true;
                for token in &mut item.tokens {
                    if let Token::Text(Text {
                        raw,
                        text,
                        tokens: Some(tokens),
                        ..
                    }) = token
                    {
                        *token = Token::Paragraph {
                            raw: std::mem::take(raw),
                            text: std::mem::take(text),
                            tokens: std::mem::take(tokens),
                        };
                    }
                }
            }
        }

        Ok(Some(Token::List(list)))
    }

    /// An HTML block.
    pub(crate) fn html(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.html, src)?;
        let raw = trim_trailing_blank_lines(&cap.text(0));
        let kind = cap.get(1);
        let pre = kind.is_some_and(|kind| kind.eq_str("pre") || kind.eq_str("script") || kind.eq_str("style"));
        Some(Token::Html(Html {
            raw: raw.clone(),
            text: raw,
            pre,
            inline_state: None,
        }))
    }

    /// A link reference definition.
    pub(crate) fn def(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.def, src)?;
        let other = self.rules.other;
        let destination = cap.text(2);
        // Outside angle brackets a destination holds only balanced parentheses.
        if !other.start_angle_bracket.test(destination.units())
            && find_closing_bracket(&destination, u16::from(b'('), u16::from(b')')) != Closing::Balanced
        {
            return None;
        }
        let any_punctuation = &self.rules.inline.any_punctuation;
        let tag = JsString::from_units(
            other
                .multiple_space_global
                .replace(normalize_label(&cap.text(1)).units(), " "),
        );
        let href = if destination.is_empty() {
            JsString::new()
        } else {
            let unbracketed = other.href_brackets.replace(destination.units(), "$1");
            JsString::from_units(any_punctuation.replace(&unbracketed, "$1"))
        };
        let title = cap.get(3).map(|title| {
            if title.is_empty() {
                title
            } else {
                let inner = title.substring(1, title.len() - 1);
                JsString::from_units(any_punctuation.replace(inner.units(), "$1"))
            }
        });
        Some(Token::Def(Def {
            tag,
            raw: rtrim(&cap.text(0), NEWLINE, false),
            href,
            title,
        }))
    }

    /// A GFM table.
    pub(crate) fn table(&mut self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.table, src)?;
        let other = self.rules.other;
        // The delimiter row must hold a pipe or a colon, otherwise it is a setext heading.
        if !other.table_delimiter.test(cap.text(2).units()) {
            return None;
        }
        let headers = split_cells(&cap.text(1), 0);
        let aligns = JsString::from_units(other.table_align_chars.replace(cap.text(2).units(), "")).split_str("|");
        let rows = match cap.get(3) {
            Some(rows) if !rows.trim().is_empty() => {
                JsString::from_units(other.table_row_blank_line.replace(rows.units(), "")).split_str("\n")
            }
            _ => Vec::new(),
        };
        // Header and delimiter rows must have as many columns; the other rows may not.
        if headers.len() != aligns.len() {
            return None;
        }
        let align: Vec<Option<Align>> = aligns
            .iter()
            .map(|cell| {
                if other.table_align_right.test(cell.units()) {
                    Some(Align::Right)
                } else if other.table_align_center.test(cell.units()) {
                    Some(Align::Center)
                } else if other.table_align_left.test(cell.units()) {
                    Some(Align::Left)
                } else {
                    None
                }
            })
            .collect();
        let header: Vec<TableCell> = headers
            .into_iter()
            .enumerate()
            .map(|(i, text)| TableCell {
                tokens: self.inline(text.clone()),
                text,
                header: true,
                align: align[i],
            })
            .collect();
        let rows: Vec<Vec<TableCell>> = rows
            .iter()
            .map(|row| {
                split_cells(row, header.len())
                    .into_iter()
                    .enumerate()
                    .map(|(i, text)| TableCell {
                        tokens: self.inline(text.clone()),
                        text,
                        header: false,
                        align: align.get(i).copied().flatten(),
                    })
                    .collect()
            })
            .collect();
        Some(Token::Table(Table {
            raw: rtrim(&cap.text(0), NEWLINE, false),
            header,
            align,
            rows,
        }))
    }

    /// A setext heading.
    pub(crate) fn lheading(&mut self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.lheading, src)?;
        let text = cap.text(1).trim();
        let depth = if cap.text(2).unit_at(0) == Some(u16::from(b'=')) {
            1
        } else {
            2
        };
        let tokens = self.inline(text.clone());
        Some(Token::Heading {
            raw: rtrim(&cap.text(0), NEWLINE, false),
            depth,
            text,
            tokens,
        })
    }

    /// A paragraph.
    pub(crate) fn paragraph(&mut self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.paragraph, src)?;
        let content = cap.text(1);
        let text = if content.ends_with("\n") {
            content.slice(0, Some(-1))
        } else {
            content
        };
        let tokens = self.inline(text.clone());
        Some(Token::Paragraph {
            raw: cap.text(0),
            text,
            tokens,
        })
    }

    /// A line of text inside a list item.
    pub(crate) fn text(&mut self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.block.text, src)?;
        let text = cap.text(0);
        let tokens = self.inline(text.clone());
        Some(Token::Text(Text {
            raw: text.clone(),
            text,
            tokens: Some(tokens),
            escaped: None,
        }))
    }

    /// A backslash escape.
    pub(crate) fn escape(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.inline.escape, src)?;
        Some(Token::Escape {
            raw: cap.text(0),
            text: cap.text(1),
        })
    }

    /// Inline HTML, which also moves the link and raw-block state.
    pub(crate) fn tag(&mut self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.inline.tag, src)?;
        let other = self.rules.other;
        let raw = cap.text(0);
        if !self.state.in_link && other.start_a_tag.test(raw.units()) {
            self.state.in_link = true;
        } else if self.state.in_link && other.end_a_tag.test(raw.units()) {
            self.state.in_link = false;
        }
        if !self.state.in_raw_block && other.start_pre_script_tag.test(raw.units()) {
            self.state.in_raw_block = true;
        } else if self.state.in_raw_block && other.end_pre_script_tag.test(raw.units()) {
            self.state.in_raw_block = false;
        }
        Some(Token::Html(Html {
            raw: raw.clone(),
            text: raw,
            pre: false,
            inline_state: Some((self.state.in_link, self.state.in_raw_block)),
        }))
    }

    /// Builds a link or image token from a matched label and a destination, lexing the label as its text. A link
    /// whose text already holds a link is no link (CommonMark: links do not nest); an image may hold one.
    fn output_link(
        &mut self,
        cap0: &JsString,
        label: &JsString,
        href: JsString,
        title: Option<JsString>,
    ) -> Result<Option<Token>, JsError> {
        let text = JsString::from_units(self.rules.other.output_link_replace.replace(label.units(), "$1"));
        let is_image = cap0.unit_at(0) == Some(u16::from(b'!'));

        let outer = self.state;
        self.state.in_link = true;
        self.state.link_emitted = false;
        let mut tokens = Vec::new();
        self.inline_tokens(&text, &mut tokens)?;
        let text_has_link = self.state.link_emitted;
        self.state.link_emitted = outer.link_emitted;
        self.state.in_link = outer.in_link;

        if !is_image {
            if text_has_link {
                // The tokens are discarded, so the raw-block state they opened is undone.
                self.state.in_raw_block = outer.in_raw_block;
                return Ok(None);
            }
            self.state.link_emitted = true;
        }

        let link = Link {
            raw: cap0.clone(),
            href,
            title: title.filter(|title| !title.is_empty()),
            text,
            tokens,
            autolink: false,
        };
        Ok(Some(if is_image {
            Token::Image(link)
        } else {
            Token::Link(link)
        }))
    }

    /// An inline link or image.
    pub(crate) fn link(&mut self, src: &JsString) -> Result<Option<Token>, JsError> {
        // Without a `)` in the run the rule cannot match; skipping it avoids its backtracking.
        if !self.state.link_paren_possible {
            return Ok(None);
        }
        let Some(cap) = Captures::exec(&self.rules.inline.link, src) else {
            return Ok(None);
        };
        let other = self.rules.other;
        let pedantic = self.options.pedantic;
        let mut cap0 = cap.text(0);
        let label = cap.text(1);
        let mut destination = cap.text(2);
        let mut title_group = cap.get(3);
        let label_start = if cap0.unit_at(0) == Some(u16::from(b'!')) { 2 } else { 1 };
        if !pedantic && is_label_end_inside_token(self, src, &label, label_start) {
            return Ok(None);
        }

        let trimmed_url = destination.trim();
        if !pedantic && other.start_angle_bracket.test(trimmed_url.units()) {
            // CommonMark requires matching angle brackets, and the closing one cannot be escaped.
            if !other.end_angle_bracket.test(trimmed_url.units()) {
                return Ok(None);
            }
            let rtrim_slash = rtrim(&trimmed_url.slice(0, Some(-1)), u16::from(b'\\'), false);
            if (trimmed_url.len() - rtrim_slash.len()).is_multiple_of(2) {
                return Ok(None);
            }
        } else {
            match find_closing_bracket(&destination, u16::from(b'('), u16::from(b')')) {
                // More open parentheses than closed.
                Closing::Unbalanced => return Ok(None),
                Closing::At(last_paren_index) => {
                    let start = if cap0.index_of("!") == Some(0) { 5 } else { 4 };
                    let link_len = start + label.len() + last_paren_index;
                    destination = destination.substring(0, last_paren_index);
                    cap0 = cap0.substring(0, link_len).trim();
                    title_group = Some(JsString::new());
                }
                Closing::Balanced => {}
            }
        }

        let mut href = destination;
        let mut title = JsString::new();
        if pedantic {
            // Pedantic links write the title inside the destination.
            if let Some(split) = Captures::exec(&other.pedantic_href_title, &href) {
                title = split.text(3);
                href = split.text(1);
            }
        } else if let Some(group) = title_group.filter(|group| !group.is_empty()) {
            title = group.slice(1, Some(-1));
        }

        href = href.trim();
        if other.start_angle_bracket.test(href.units()) {
            href = if pedantic && !other.end_angle_bracket.test(trimmed_url.units()) {
                // Pedantic allows an opening angle bracket without a closing one.
                href.slice(1, None)
            } else {
                href.slice(1, Some(-1))
            };
        }
        let any_punctuation = &self.rules.inline.any_punctuation;
        let unescape = |text: JsString| {
            if text.is_empty() {
                text
            } else {
                JsString::from_units(any_punctuation.replace(text.units(), "$1"))
            }
        };
        self.output_link(&cap0, &label, unescape(href), Some(unescape(title)))
    }

    /// A reference link or image (full, collapsed or shortcut). An undefined reference leaves its first character as
    /// text.
    pub(crate) fn reflink(&mut self, src: &JsString) -> Result<Option<Token>, JsError> {
        let inline = self.rules.inline;
        let Some(cap) = Captures::exec(&inline.reflink, src).or_else(|| Captures::exec(&inline.nolink, src)) else {
            return Ok(None);
        };
        let cap0 = cap.text(0);
        let label = cap.text(1);
        let label_start = if cap0.unit_at(0) == Some(u16::from(b'!')) { 2 } else { 1 };
        if !self.options.pedantic && is_label_end_inside_token(self, src, &label, label_start) {
            return Ok(None);
        }
        let reference = if cap.has(2) { cap.text(2) } else { label.clone() };
        let link_string = JsString::from_units(self.rules.other.multiple_space_global.replace(reference.units(), " "));
        let Some(LinkDef { href, title }) = self.links.get(&normalize_label(&link_string)).cloned() else {
            let text = cap0.char_at(0);
            return Ok(Some(Token::Text(Text {
                raw: text.clone(),
                text,
                tokens: None,
                escaped: None,
            })));
        };
        self.output_link(&cap0, &label, href, title)
    }

    /// Emphasis or strong emphasis, following the CommonMark delimiter run rules on the masked source.
    pub(crate) fn em_strong(
        &mut self,
        src: &JsString,
        masked_src: &JsString,
        prev_char: &JsString,
    ) -> Result<Option<Token>, JsError> {
        let inline = self.rules.inline;
        let other = self.rules.other;
        let pedantic = self.options.pedantic;
        let Some(left) = Captures::exec(&inline.em_strong_l_delim, src) else {
            return Ok(None);
        };
        if !(left.has(1) || left.has(2) || left.has(3) || left.has(4)) {
            return Ok(None);
        }
        let left0 = left.text(0);
        let delim_char = left0.units()[0];
        // A mid-run opener only pairs with a delimiter that can only close.
        let mid_run = prev_char.units() == [delim_char];

        // Flanking and rules 9-10 are about the whole delimiter run, whose start may already be behind `src`.
        let masked = masked_src.units();
        let src_start = masked.len().saturating_sub(src.len());
        let mut run_start = src_start;
        while !pedantic && run_start > 0 && masked[run_start - 1] == delim_char {
            run_start -= 1;
        }
        let run_prev_char = JsString::from_slice(&masked[run_start.saturating_sub(2)..run_start])
            .code_points()
            .pop()
            .unwrap_or_default();
        let is_punctuation = |text: &JsString| inline.punctuation.test(text.units());
        // The other delimiter character counts as punctuation here (`_**a*`).
        let run_prev_punct = run_prev_char.eq_str("*") || run_prev_char.eq_str("_") || is_punctuation(&run_prev_char);
        let prev_char = if run_start < src_start {
            run_prev_char.clone()
        } else {
            prev_char.clone()
        };
        let prev_punct = if run_start < src_start {
            run_prev_punct
        } else {
            is_punctuation(&prev_char)
        };

        // `_` cannot sit between two alphanumerics.
        if left.has(4) && other.unicode_alpha_numeric.test(prev_char.units()) {
            return Ok(None);
        }

        let next_char = if left.has(1) { left.text(1) } else { left.text(3) };
        if !(next_char.is_empty() || prev_char.is_empty() || prev_punct) {
            return Ok(None);
        }

        // The unicode expressions count an astral character as one, so count characters, not code units.
        let l_length = left0.code_point_count() - 1;
        let mut delim_total = l_length as i64;
        let mut mid_delim_total = 0i64;
        let run_length = (src_start - run_start + l_length) as i64;
        let opener_can_close = !pedantic
            && !run_prev_char.trim().is_empty()
            && (!next_char.is_empty() || (delim_char == u16::from(b'*') && !run_prev_punct));
        let end_reg = if delim_char == u16::from(b'*') {
            &inline.em_strong_r_delim_ast
        } else {
            &inline.em_strong_r_delim_und
        };

        // Clip the masked source to the same section as `src`.
        let clipped = masked_src.slice(l_length as isize - src.len() as isize, None);
        let mut last_index = 0;
        while let Some(found) = end_reg.exec_at(clipped.units(), last_index) {
            last_index = found.end();
            let group = (1..=6).find(|&group| found.has(group));
            let Some(group) = group else {
                // A lone `*` inside `__abc*abc__`.
                continue;
            };
            let r_delim = JsString::from_slice(found.group_or_empty(clipped.units(), group));
            let mut r_length = r_delim.code_point_count() as i64;

            if group == 3 || group == 4 {
                // Another left delimiter.
                delim_total += r_length;
                continue;
            } else if group == 5 || group == 6 || opener_can_close {
                if mid_run && (group == 5 || group == 6) {
                    // That delimiter opens its own span instead.
                    break;
                }
                if run_length % 3 != 0 && (run_length + r_length) % 3 == 0 {
                    // CommonMark emphasis rules 9-10.
                    mid_delim_total += r_length;
                    continue;
                }
            }

            delim_total -= r_length;
            if delim_total > 0 {
                // Not enough closing delimiters yet.
                continue;
            }

            // Remove extra characters: *a*** -> *a*
            r_length = r_length.min(r_length + delim_total + mid_delim_total);
            let matched = JsString::from_slice(found.group_or_empty(clipped.units(), 0));
            let last_char_length = matched.code_points().first().map_or(0, JsString::len);
            let end = l_length as i64 + found.index() as i64 + last_char_length as i64 + r_length;
            let raw = src.slice(0, Some(end as isize));

            // An odd smallest delimiter makes emphasis, an even one strong emphasis.
            let (strip, strong) = if (l_length as i64).min(r_length) % 2 != 0 {
                (1, false)
            } else {
                (2, true)
            };
            let text = raw.slice(strip, Some(-strip));
            let mut tokens = Vec::new();
            self.inline_tokens(&text, &mut tokens)?;
            let span = Span { raw, text, tokens };
            return Ok(Some(if strong { Token::Strong(span) } else { Token::Em(span) }));
        }
        Ok(None)
    }

    /// A code span.
    pub(crate) fn codespan(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.inline.code, src)?;
        let other = self.rules.other;
        let mut text = JsString::from_units(other.new_line_char_global.replace(cap.text(2).units(), " "));
        let has_non_space_chars = other.non_space_char.test(text.units());
        let has_space_chars_on_both_ends =
            other.starting_space_char.test(text.units()) && other.ending_space_char.test(text.units());
        if has_non_space_chars && has_space_chars_on_both_ends {
            text = text.substring(1, text.len() - 1);
        }
        Some(Token::Codespan { raw: cap.text(0), text })
    }

    /// A hard line break.
    pub(crate) fn br(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.inline.br, src)?;
        Some(Token::Br { raw: cap.text(0) })
    }

    /// GFM strikethrough with one or two tildes.
    pub(crate) fn del(
        &mut self,
        src: &JsString,
        masked_src: &JsString,
        prev_char: &JsString,
    ) -> Result<Option<Token>, JsError> {
        let inline = self.rules.inline;
        let Some(left) = Captures::exec(&inline.del_l_delim, src) else {
            return Ok(None);
        };
        let next_char = left.text(1);
        if !(next_char.is_empty() || prev_char.is_empty() || inline.punctuation.test(prev_char.units())) {
            return Ok(None);
        }
        let l_length = left.text(0).code_point_count() - 1;
        let mut delim_total = l_length as i64;
        let clipped = masked_src.slice(l_length as isize - src.len() as isize, None);
        let mut last_index = 0;
        while let Some(found) = inline.del_r_delim.exec_at(clipped.units(), last_index) {
            last_index = found.end();
            let Some(group) = (1..=6).find(|&group| found.has(group)) else {
                continue;
            };
            let mut r_length =
                JsString::from_slice(found.group_or_empty(clipped.units(), group)).code_point_count() as i64;
            if r_length != l_length as i64 {
                continue;
            }
            if group == 3 || group == 4 {
                // Another left delimiter.
                delim_total += r_length;
                continue;
            }
            delim_total -= r_length;
            if delim_total > 0 {
                continue;
            }
            r_length = r_length.min(r_length + delim_total);
            let matched = JsString::from_slice(found.group_or_empty(clipped.units(), 0));
            let last_char_length = matched.code_points().first().map_or(0, JsString::len);
            let end = l_length as i64 + found.index() as i64 + last_char_length as i64 + r_length;
            let raw = src.slice(0, Some(end as isize));
            let text = raw.slice(l_length as isize, Some(-(l_length as isize)));
            let mut tokens = Vec::new();
            self.inline_tokens(&text, &mut tokens)?;
            return Ok(Some(Token::Del(Span { raw, text, tokens })));
        }
        Ok(None)
    }

    /// An autolink: `<scheme:...>` or `<address@example.com>`.
    pub(crate) fn autolink(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.inline.autolink, src)?;
        let text = cap.text(1);
        let href = if cap.get(2).is_some_and(|at| at.eq_str("@")) {
            concat(&[&JsString::from("mailto:"), &text])
        } else {
            text.clone()
        };
        Some(autolink_token(cap.text(0), text, href))
    }

    /// A GFM extended autolink: a URL, a `www.` address or an email address in the text, without the trailing
    /// punctuation that ends a sentence.
    pub(crate) fn url(&self, src: &JsString) -> Option<Token> {
        let inline = self.rules.inline;
        let cap = Captures::exec(&inline.url, src)?;
        if cap.get(2).is_some_and(|at| at.eq_str("@")) {
            let text = cap.text(0);
            let href = concat(&[&JsString::from("mailto:"), &text]);
            return Some(autolink_token(text.clone(), text, href));
        }
        let mut url = cap.text(0);
        loop {
            let previous = url.clone();
            url = inline
                .backpedal
                .exec(url.units())
                .map(|found| JsString::from_slice(found.group_or_empty(url.units(), 0)))
                .unwrap_or_default();
            if previous == url {
                break;
            }
        }
        let href = if cap.get(1).is_some_and(|prefix| prefix.eq_str("www.")) {
            concat(&[&JsString::from("http://"), &url])
        } else {
            url.clone()
        };
        Some(autolink_token(url.clone(), url, href))
    }

    /// Plain text up to the next possible inline construct. Numeric character references are decoded, except in
    /// raw HTML.
    pub(crate) fn inline_text(&self, src: &JsString) -> Option<Token> {
        let cap = Captures::exec(&self.rules.inline.text, src)?;
        let raw = cap.text(0);
        let escaped = self.state.in_raw_block;
        let text = if escaped {
            raw.clone()
        } else {
            decode_numeric_character_references(&raw)
        };
        Some(Token::Text(Text {
            raw,
            text,
            tokens: None,
            escaped: Some(escaped),
        }))
    }
}

fn autolink_token(raw: JsString, text: JsString, href: JsString) -> Token {
    Token::Link(Link {
        raw,
        href,
        title: None,
        text: text.clone(),
        tokens: vec![Token::Text(Text {
            raw: text.clone(),
            text,
            tokens: None,
            escaped: None,
        })],
        autolink: true,
    })
}

/// `text.search(regex)`: where the first match starts, -1 without one.
fn search(regex: &RegExp, text: &JsString) -> isize {
    regex.search(text.units()).map_or(-1, |index| index as isize)
}
