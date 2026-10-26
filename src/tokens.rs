// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The token tree the lexer builds and the parser renders, and its JSON form.
//!
//! Every token serializes with the members the original's token object has, in the order the original creates them,
//! so that `JSON.stringify` of the original's tokens and [`write_tokens`] agree byte for byte.

use crate::js::json::quote;
use crate::js::number::number_to_string;
use crate::js::JsString;

/// The inline tokens of a block token. While the lexer runs, a block token's inline tokens are a slot of the lexer's
/// inline queue, filled once every block is known (the original shares the array between the token and the
/// queue); afterwards they are the tokens themselves.
#[derive(Clone, Debug, Default)]
pub struct Inline {
    /// The queue slot holding the tokens while the lexer runs.
    pub(crate) slot: Option<usize>,
    /// The tokens.
    pub tokens: Vec<Token>,
}

impl Inline {
    /// Tokens that are already known.
    pub fn new(tokens: Vec<Token>) -> Inline {
        Inline { slot: None, tokens }
    }

    /// Tokens the lexer fills in later from queue slot `slot`.
    pub(crate) fn queued(slot: usize) -> Inline {
        Inline {
            slot: Some(slot),
            tokens: Vec::new(),
        }
    }
}

/// A table cell's alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    /// The name the HTML `align` attribute and the token use.
    pub fn name(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
        }
    }
}

/// A code block, indented or fenced.
#[derive(Clone, Debug)]
pub struct Code {
    pub raw: JsString,
    /// Whether the block is an indented one (`codeBlockStyle: "indented"`).
    pub indented: bool,
    /// The info string of a fenced block.
    pub lang: Option<JsString>,
    pub text: JsString,
}

/// A text token. Block text holds inline tokens; inline text says whether it is already escaped; the text of a
/// failed reference or of an autolink holds neither.
#[derive(Clone, Debug)]
pub struct Text {
    pub raw: JsString,
    pub text: JsString,
    pub tokens: Option<Inline>,
    pub escaped: Option<bool>,
}

/// A link or an image.
#[derive(Clone, Debug)]
pub struct Link {
    pub raw: JsString,
    pub href: JsString,
    /// The title; `None` serializes as `null`.
    pub title: Option<JsString>,
    pub text: JsString,
    pub tokens: Vec<Token>,
    /// Whether the link is an autolink (`<...>` or a GFM URL), which has no title member.
    pub autolink: bool,
}

/// Emphasis, strong emphasis or strikethrough.
#[derive(Clone, Debug)]
pub struct Span {
    pub raw: JsString,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

/// Raw HTML: a block, or an inline tag with the lexer's link and raw-block state after it.
#[derive(Clone, Debug)]
pub struct Html {
    pub raw: JsString,
    pub text: JsString,
    /// For a block: whether it is a `pre`, `script` or `style` block.
    pub pre: bool,
    /// For a tag: `(inLink, inRawBlock)`; `None` for a block.
    pub inline_state: Option<(bool, bool)>,
}

/// A link reference definition.
#[derive(Clone, Debug)]
pub struct Def {
    pub tag: JsString,
    pub raw: JsString,
    pub href: JsString,
    /// The title; `None` leaves the member out.
    pub title: Option<JsString>,
}

/// A list.
#[derive(Clone, Debug)]
pub struct List {
    pub raw: JsString,
    pub ordered: bool,
    /// The start number of an ordered list; `None` serializes as `""`.
    pub start: Option<u32>,
    pub loose: bool,
    pub items: Vec<ListItem>,
}

/// One item of a list.
#[derive(Clone, Debug)]
pub struct ListItem {
    pub raw: JsString,
    pub task: bool,
    /// Set once the item's checkbox is placed.
    pub checked: Option<bool>,
    pub loose: bool,
    pub text: JsString,
    pub tokens: Vec<Token>,
}

/// One table cell.
#[derive(Clone, Debug)]
pub struct TableCell {
    pub text: JsString,
    pub tokens: Inline,
    pub header: bool,
    pub align: Option<Align>,
}

/// A table.
#[derive(Clone, Debug)]
pub struct Table {
    pub raw: JsString,
    pub header: Vec<TableCell>,
    pub align: Vec<Option<Align>>,
    pub rows: Vec<Vec<TableCell>>,
}

/// A token.
#[derive(Clone, Debug)]
pub enum Token {
    Space {
        raw: JsString,
    },
    Code(Code),
    Heading {
        raw: JsString,
        depth: usize,
        text: JsString,
        tokens: Inline,
    },
    Hr {
        raw: JsString,
    },
    Blockquote {
        raw: JsString,
        tokens: Vec<Token>,
        text: JsString,
    },
    List(List),
    Checkbox {
        raw: JsString,
        checked: bool,
    },
    Html(Html),
    Def(Def),
    Table(Table),
    Paragraph {
        raw: JsString,
        text: JsString,
        tokens: Inline,
    },
    Text(Text),
    Escape {
        raw: JsString,
        text: JsString,
    },
    Link(Link),
    Image(Link),
    Strong(Span),
    Em(Span),
    Del(Span),
    Codespan {
        raw: JsString,
        text: JsString,
    },
    Br {
        raw: JsString,
    },
}

impl Token {
    /// The token's `type`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Token::Space { .. } => "space",
            Token::Code(_) => "code",
            Token::Heading { .. } => "heading",
            Token::Hr { .. } => "hr",
            Token::Blockquote { .. } => "blockquote",
            Token::List(_) => "list",
            Token::Checkbox { .. } => "checkbox",
            Token::Html(_) => "html",
            Token::Def(_) => "def",
            Token::Table(_) => "table",
            Token::Paragraph { .. } => "paragraph",
            Token::Text(_) => "text",
            Token::Escape { .. } => "escape",
            Token::Link(_) => "link",
            Token::Image(_) => "image",
            Token::Strong(_) => "strong",
            Token::Em(_) => "em",
            Token::Del(_) => "del",
            Token::Codespan { .. } => "codespan",
            Token::Br { .. } => "br",
        }
    }

    /// The source text the token was made from.
    pub fn raw(&self) -> &JsString {
        match self {
            Token::Space { raw }
            | Token::Heading { raw, .. }
            | Token::Hr { raw }
            | Token::Blockquote { raw, .. }
            | Token::Checkbox { raw, .. }
            | Token::Paragraph { raw, .. }
            | Token::Escape { raw, .. }
            | Token::Codespan { raw, .. }
            | Token::Br { raw } => raw,
            Token::Code(code) => &code.raw,
            Token::List(list) => &list.raw,
            Token::Html(html) => &html.raw,
            Token::Def(def) => &def.raw,
            Token::Table(table) => &table.raw,
            Token::Text(text) => &text.raw,
            Token::Link(link) | Token::Image(link) => &link.raw,
            Token::Strong(span) | Token::Em(span) | Token::Del(span) => &span.raw,
        }
    }

    /// The source text, to change.
    pub fn raw_mut(&mut self) -> &mut JsString {
        match self {
            Token::Space { raw }
            | Token::Heading { raw, .. }
            | Token::Hr { raw }
            | Token::Blockquote { raw, .. }
            | Token::Checkbox { raw, .. }
            | Token::Paragraph { raw, .. }
            | Token::Escape { raw, .. }
            | Token::Codespan { raw, .. }
            | Token::Br { raw } => raw,
            Token::Code(code) => &mut code.raw,
            Token::List(list) => &mut list.raw,
            Token::Html(html) => &mut html.raw,
            Token::Def(def) => &mut def.raw,
            Token::Table(table) => &mut table.raw,
            Token::Text(text) => &mut text.raw,
            Token::Link(link) | Token::Image(link) => &mut link.raw,
            Token::Strong(span) | Token::Em(span) | Token::Del(span) => &mut span.raw,
        }
    }

    /// The `text` member of the tokens that have one, to change.
    pub fn text_mut(&mut self) -> Option<&mut JsString> {
        match self {
            Token::Heading { text, .. }
            | Token::Blockquote { text, .. }
            | Token::Paragraph { text, .. }
            | Token::Escape { text, .. }
            | Token::Codespan { text, .. } => Some(text),
            Token::Code(code) => Some(&mut code.text),
            Token::Html(html) => Some(&mut html.text),
            Token::Text(text) => Some(&mut text.text),
            Token::Link(link) | Token::Image(link) => Some(&mut link.text),
            Token::Strong(span) | Token::Em(span) | Token::Del(span) => Some(&mut span.text),
            _ => None,
        }
    }

    /// The inline tokens of a paragraph or of block text, to change.
    pub fn inline_mut(&mut self) -> Option<&mut Inline> {
        match self {
            Token::Paragraph { tokens, .. } => Some(tokens),
            Token::Text(Text {
                tokens: Some(tokens), ..
            }) => Some(tokens),
            _ => None,
        }
    }

    /// Whether this is a paragraph or a text token (the tokens the lexer merges continuation lines into).
    pub fn is_paragraph_or_text(&self) -> bool {
        matches!(self, Token::Paragraph { .. } | Token::Text(_))
    }
}

/// One link reference definition as the lexer keeps it.
#[derive(Clone, Debug)]
pub struct LinkDef {
    pub href: JsString,
    pub title: Option<JsString>,
}

/// The link reference definitions of a document, by normalized label, in the order they were defined.
#[derive(Clone, Debug, Default)]
pub struct Links {
    entries: Vec<(JsString, LinkDef)>,
}

impl Links {
    /// The definition of a label.
    pub fn get(&self, label: &JsString) -> Option<&LinkDef> {
        self.entries.iter().find(|(key, _)| key == label).map(|(_, def)| def)
    }

    /// Adds a definition; the first definition of a label wins.
    pub fn define(&mut self, label: JsString, def: LinkDef) {
        if self.get(&label).is_none() {
            self.entries.push((label, def));
        }
    }

    /// The definitions in JavaScript's property order: array index keys in numeric order, then the others in
    /// definition order.
    pub fn ordered(&self) -> Vec<&(JsString, LinkDef)> {
        let mut indexed: Vec<(u32, &(JsString, LinkDef))> = self
            .entries
            .iter()
            .filter_map(|entry| entry.0.array_index().map(|index| (index, entry)))
            .collect();
        indexed.sort_by_key(|&(index, _)| index);
        let mut out: Vec<&(JsString, LinkDef)> = indexed.into_iter().map(|(_, entry)| entry).collect();
        out.extend(self.entries.iter().filter(|entry| entry.0.array_index().is_none()));
        out
    }
}

/// A JSON object writer that adds members in order.
struct ObjectWriter<'a> {
    out: &'a mut JsString,
    first: bool,
}

impl<'a> ObjectWriter<'a> {
    fn new(out: &'a mut JsString) -> Self {
        out.push_str("{");
        ObjectWriter { out, first: true }
    }

    fn key(&mut self, key: &str) -> &mut JsString {
        self.key_units(&key.encode_utf16().collect::<Vec<u16>>())
    }

    fn key_units(&mut self, key: &[u16]) -> &mut JsString {
        if !self.first {
            self.out.push_str(",");
        }
        self.first = false;
        quote(self.out, key);
        self.out.push_str(":");
        self.out
    }

    fn string(&mut self, key: &str, value: &JsString) {
        let out = self.key(key);
        quote(out, value.units());
    }

    fn literal(&mut self, key: &str, value: &str) {
        self.key(key).push_str(value);
    }

    fn boolean(&mut self, key: &str, value: bool) {
        self.literal(key, if value { "true" } else { "false" });
    }

    fn tokens(&mut self, key: &str, tokens: &[Token]) {
        let out = self.key(key);
        write_tokens(out, tokens);
    }

    fn end(self) {
        self.out.push_str("}");
    }
}

/// Appends the JSON array of a token list.
pub fn write_tokens(out: &mut JsString, tokens: &[Token]) {
    out.push_str("[");
    for (i, token) in tokens.iter().enumerate() {
        if i > 0 {
            out.push_str(",");
        }
        write_token(out, token);
    }
    out.push_str("]");
}

/// Appends the JSON object of the link definitions, as `JSON.stringify` writes `tokens.links`.
pub fn write_links(out: &mut JsString, links: &Links) {
    let mut object = ObjectWriter::new(out);
    for (label, def) in links.ordered() {
        let out = object.key_units(label.units());
        let mut inner = ObjectWriter::new(out);
        inner.string("href", &def.href);
        if let Some(title) = &def.title {
            inner.string("title", title);
        }
        inner.end();
    }
    object.end();
}

fn write_token(out: &mut JsString, token: &Token) {
    let mut object = ObjectWriter::new(out);
    object.literal("type", &format!("\"{}\"", token.type_name()));
    match token {
        Token::Space { raw } | Token::Hr { raw } | Token::Br { raw } => object.string("raw", raw),
        Token::Code(code) => {
            object.string("raw", &code.raw);
            if code.indented {
                object.literal("codeBlockStyle", "\"indented\"");
            }
            if let Some(lang) = &code.lang {
                object.string("lang", lang);
            }
            object.string("text", &code.text);
        }
        Token::Heading {
            raw,
            depth,
            text,
            tokens,
        } => {
            object.string("raw", raw);
            object.literal("depth", &depth.to_string());
            object.string("text", text);
            object.tokens("tokens", &tokens.tokens);
        }
        Token::Blockquote { raw, tokens, text } => {
            object.string("raw", raw);
            object.tokens("tokens", tokens);
            object.string("text", text);
        }
        Token::List(list) => {
            object.string("raw", &list.raw);
            object.boolean("ordered", list.ordered);
            match list.start {
                Some(start) => object.literal("start", &number_to_string(f64::from(start))),
                None => object.literal("start", "\"\""),
            }
            object.boolean("loose", list.loose);
            let out = object.key("items");
            out.push_str("[");
            for (i, item) in list.items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",");
                }
                let mut entry = ObjectWriter::new(out);
                entry.literal("type", "\"list_item\"");
                entry.string("raw", &item.raw);
                entry.boolean("task", item.task);
                entry.boolean("loose", item.loose);
                entry.string("text", &item.text);
                entry.tokens("tokens", &item.tokens);
                if let Some(checked) = item.checked {
                    entry.boolean("checked", checked);
                }
                entry.end();
            }
            out.push_str("]");
        }
        Token::Checkbox { raw, checked } => {
            object.string("raw", raw);
            object.boolean("checked", *checked);
        }
        Token::Html(html) => match html.inline_state {
            None => {
                object.boolean("block", true);
                object.string("raw", &html.raw);
                object.boolean("pre", html.pre);
                object.string("text", &html.text);
            }
            Some((in_link, in_raw_block)) => {
                object.string("raw", &html.raw);
                object.boolean("inLink", in_link);
                object.boolean("inRawBlock", in_raw_block);
                object.boolean("block", false);
                object.string("text", &html.text);
            }
        },
        Token::Def(def) => {
            object.string("tag", &def.tag);
            object.string("raw", &def.raw);
            object.string("href", &def.href);
            if let Some(title) = &def.title {
                object.string("title", title);
            }
        }
        Token::Table(table) => {
            object.string("raw", &table.raw);
            let out = object.key("header");
            write_cells(out, &table.header);
            let out = object.key("align");
            out.push_str("[");
            for (i, align) in table.align.iter().enumerate() {
                if i > 0 {
                    out.push_str(",");
                }
                match align {
                    Some(align) => out.push_str(&format!("\"{}\"", align.name())),
                    None => out.push_str("null"),
                }
            }
            out.push_str("]");
            let out = object.key("rows");
            out.push_str("[");
            for (i, row) in table.rows.iter().enumerate() {
                if i > 0 {
                    out.push_str(",");
                }
                write_cells(out, row);
            }
            out.push_str("]");
        }
        Token::Paragraph { raw, text, tokens } => {
            object.string("raw", raw);
            object.string("text", text);
            object.tokens("tokens", &tokens.tokens);
        }
        Token::Text(text) => {
            object.string("raw", &text.raw);
            object.string("text", &text.text);
            if let Some(tokens) = &text.tokens {
                object.tokens("tokens", &tokens.tokens);
            }
            if let Some(escaped) = text.escaped {
                object.boolean("escaped", escaped);
            }
        }
        Token::Escape { raw, text } | Token::Codespan { raw, text } => {
            object.string("raw", raw);
            object.string("text", text);
        }
        Token::Link(link) | Token::Image(link) => {
            object.string("raw", &link.raw);
            if link.autolink {
                object.string("text", &link.text);
                object.string("href", &link.href);
                object.boolean("autolink", true);
            } else {
                object.string("href", &link.href);
                match &link.title {
                    Some(title) => object.string("title", title),
                    None => object.literal("title", "null"),
                }
                object.string("text", &link.text);
            }
            object.tokens("tokens", &link.tokens);
        }
        Token::Strong(span) | Token::Em(span) | Token::Del(span) => {
            object.string("raw", &span.raw);
            object.string("text", &span.text);
            object.tokens("tokens", &span.tokens);
        }
    }
    object.end();
}

fn write_cells(out: &mut JsString, cells: &[TableCell]) {
    out.push_str("[");
    for (i, cell) in cells.iter().enumerate() {
        if i > 0 {
            out.push_str(",");
        }
        let mut object = ObjectWriter::new(out);
        object.string("text", &cell.text);
        object.tokens("tokens", &cell.tokens.tokens);
        object.boolean("header", cell.header);
        match cell.align {
            Some(align) => object.literal("align", &format!("\"{}\"", align.name())),
            None => object.literal("align", "null"),
        }
        object.end();
    }
    out.push_str("]");
}
