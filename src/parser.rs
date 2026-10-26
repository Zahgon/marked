// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The parser: renders a token tree as HTML. Block tokens go through [`Parser::parse`], inline tokens through
//! [`Parser::parse_inline`] with either the HTML renderer or, for the alt text of images, the text renderer, which
//! keeps only the textual part of every token.

use crate::helpers::{clean_url, escape_html_entities};
use crate::js::number::number_to_string;
use crate::js::{JsError, JsString};
use crate::options::Options;
use crate::rules::{Flavour, Rules};
use crate::tokens::{Code, Link, List, Table, TableCell, Text, Token};

/// Which renderer inline tokens go through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererKind {
    /// HTML.
    Html,
    /// Text only, as image alt text needs.
    Text,
}

/// Renders tokens as HTML.
pub struct Parser {
    options: Options,
}

impl Parser {
    /// A parser with the given options.
    pub fn new(options: Options) -> Parser {
        Parser { options }
    }

    /// The error for a token type the parser does not know: returned, or with the silent option written to
    /// standard error, as the original's `console.error` does, leaving an empty result.
    fn unknown(&self, token: &Token) -> Result<Option<JsString>, JsError> {
        let message = format!("Token with \"{}\" type was not found.", token.type_name());
        if self.options.silent {
            eprintln!("{message}");
            Ok(None)
        } else {
            Err(JsError::error(message))
        }
    }

    /// Renders block tokens.
    pub fn parse(&self, tokens: &[Token]) -> Result<JsString, JsError> {
        let mut out = JsString::new();
        for token in tokens {
            match token {
                Token::Space { .. } | Token::Def(_) => {}
                Token::Hr { .. } => out.push_str("<hr>\n"),
                Token::Heading { depth, tokens, .. } => {
                    out.push_str(&format!("<h{depth}>"));
                    out.push_js(&self.parse_inline(&tokens.tokens, RendererKind::Html)?);
                    out.push_str(&format!("</h{depth}>\n"));
                }
                Token::Code(code) => out.push_js(&render_code(code)),
                Token::Table(table) => out.push_js(&self.table(table)?),
                Token::Blockquote { tokens, .. } => {
                    out.push_str("<blockquote>\n");
                    out.push_js(&self.parse(tokens)?);
                    out.push_str("</blockquote>\n");
                }
                Token::List(list) => out.push_js(&self.list(list)?),
                Token::Checkbox { checked, .. } => out.push_str(checkbox(*checked)),
                Token::Html(html) => out.push_js(&html.text),
                Token::Paragraph { tokens, .. } => {
                    out.push_str("<p>");
                    out.push_js(&self.parse_inline(&tokens.tokens, RendererKind::Html)?);
                    out.push_str("</p>\n");
                }
                Token::Text(text) => out.push_js(&self.text(text, RendererKind::Html)?),
                _ => match self.unknown(token)? {
                    Some(rendered) => out.push_js(&rendered),
                    None => return Ok(JsString::new()),
                },
            }
        }
        Ok(out)
    }

    /// Renders inline tokens with the given renderer.
    pub fn parse_inline(&self, tokens: &[Token], renderer: RendererKind) -> Result<JsString, JsError> {
        let mut out = JsString::new();
        for token in tokens {
            let rendered = match (token, renderer) {
                (Token::Escape { text, .. }, RendererKind::Html) => escape_html_entities(text, false),
                (Token::Escape { text, .. }, RendererKind::Text) => text.clone(),
                (Token::Html(html), _) => html.text.clone(),
                (Token::Link(link), RendererKind::Html) => self.link(link)?,
                (Token::Image(image), RendererKind::Html) => self.image(image)?,
                (Token::Link(link) | Token::Image(link), RendererKind::Text) => {
                    self.parse_inline(&link.tokens, RendererKind::Text)?
                }
                (Token::Checkbox { checked, .. }, RendererKind::Html) => JsString::from(checkbox(*checked)),
                (Token::Checkbox { raw, .. }, RendererKind::Text) => raw.clone(),
                (Token::Strong(span), RendererKind::Html) => {
                    self.wrap("strong", &self.parse_inline(&span.tokens, renderer)?)
                }
                (Token::Em(span), RendererKind::Html) => self.wrap("em", &self.parse_inline(&span.tokens, renderer)?),
                (Token::Del(span), RendererKind::Html) => self.wrap("del", &self.parse_inline(&span.tokens, renderer)?),
                (Token::Strong(span) | Token::Em(span) | Token::Del(span), RendererKind::Text) => {
                    self.parse_inline(&span.tokens, renderer)?
                }
                (Token::Codespan { text, .. }, RendererKind::Html) => {
                    self.wrap("code", &escape_html_entities(text, true))
                }
                (Token::Codespan { text, .. }, RendererKind::Text) => text.clone(),
                (Token::Br { .. }, RendererKind::Html) => JsString::from("<br>"),
                (Token::Br { .. }, RendererKind::Text) => JsString::new(),
                (Token::Text(text), _) => self.text(text, renderer)?,
                _ => match self.unknown(token)? {
                    Some(rendered) => rendered,
                    None => return Ok(JsString::new()),
                },
            };
            out.push_js(&rendered);
        }
        Ok(out)
    }

    fn wrap(&self, tag: &str, content: &JsString) -> JsString {
        let mut out = JsString::from(format!("<{tag}>"));
        out.push_js(content);
        out.push_str(&format!("</{tag}>"));
        out
    }

    /// Text: its inline tokens when it has them, otherwise the text, escaped unless it already is.
    fn text(&self, text: &Text, renderer: RendererKind) -> Result<JsString, JsError> {
        if renderer == RendererKind::Text {
            return Ok(text.text.clone());
        }
        if let Some(tokens) = &text.tokens {
            return self.parse_inline(&tokens.tokens, renderer);
        }
        if text.escaped == Some(true) {
            return Ok(text.text.clone());
        }
        Ok(escape_html_entities(&text.text, false))
    }

    fn list(&self, list: &List) -> Result<JsString, JsError> {
        let mut body = JsString::new();
        for item in &list.items {
            body.push_str("<li>");
            body.push_js(&self.parse(&item.tokens)?);
            body.push_str("</li>\n");
        }
        let kind = if list.ordered { "ol" } else { "ul" };
        let start_attr = match list.start {
            Some(start) if list.ordered && start != 1 => {
                format!(" start=\"{}\"", number_to_string(f64::from(start)))
            }
            _ => String::new(),
        };
        let mut out = JsString::from(format!("<{kind}{start_attr}>\n"));
        out.push_js(&body);
        out.push_str(&format!("</{kind}>\n"));
        Ok(out)
    }

    fn table_cell(&self, cell: &TableCell) -> Result<JsString, JsError> {
        let kind = if cell.header { "th" } else { "td" };
        let mut out = JsString::from(match cell.align {
            Some(align) => format!("<{kind} align=\"{}\">", align.name()),
            None => format!("<{kind}>"),
        });
        out.push_js(&self.parse_inline(&cell.tokens.tokens, RendererKind::Html)?);
        out.push_str(&format!("</{kind}>\n"));
        Ok(out)
    }

    fn table_row(&self, cells: &[TableCell]) -> Result<JsString, JsError> {
        let mut out = JsString::from("<tr>\n");
        for cell in cells {
            out.push_js(&self.table_cell(cell)?);
        }
        out.push_str("</tr>\n");
        Ok(out)
    }

    fn table(&self, table: &Table) -> Result<JsString, JsError> {
        let mut out = JsString::from("<table>\n<thead>\n");
        out.push_js(&self.table_row(&table.header)?);
        out.push_str("</thead>\n");
        if !table.rows.is_empty() {
            out.push_str("<tbody>");
            for row in &table.rows {
                out.push_js(&self.table_row(row)?);
            }
            out.push_str("</tbody>");
        }
        out.push_str("</table>\n");
        Ok(out)
    }

    fn link(&self, link: &Link) -> Result<JsString, JsError> {
        // References are not resolved inside an autolink, so every `&` there is literal.
        let parsed_text = if link.autolink {
            escape_html_entities(&link.text, true)
        } else {
            self.parse_inline(&link.tokens, RendererKind::Html)?
        };
        let Some(clean_href) = clean_url(&link.href) else {
            return Ok(parsed_text);
        };
        let mut out = JsString::from("<a href=\"");
        out.push_js(&escape_html_entities(&clean_href, link.autolink));
        out.push_str("\"");
        if let Some(title) = link.title.as_ref().filter(|title| !title.is_empty()) {
            out.push_str(" title=\"");
            out.push_js(&escape_html_entities(title, false));
            out.push_str("\"");
        }
        out.push_str(">");
        out.push_js(&parsed_text);
        out.push_str("</a>");
        Ok(out)
    }

    fn image(&self, image: &Link) -> Result<JsString, JsError> {
        let text = self.parse_inline(&image.tokens, RendererKind::Text)?;
        let Some(clean_href) = clean_url(&image.href) else {
            return Ok(escape_html_entities(&text, false));
        };
        let mut out = JsString::from("<img src=\"");
        out.push_js(&escape_html_entities(&clean_href, false));
        out.push_str("\" alt=\"");
        out.push_js(&escape_html_entities(&text, false));
        out.push_str("\"");
        if let Some(title) = image.title.as_ref().filter(|title| !title.is_empty()) {
            out.push_str(" title=\"");
            out.push_js(&escape_html_entities(title, false));
            out.push_str("\"");
        }
        out.push_str(">");
        Ok(out)
    }
}

fn checkbox(checked: bool) -> &'static str {
    if checked {
        "<input checked=\"\" disabled=\"\" type=\"checkbox\"> "
    } else {
        "<input disabled=\"\" type=\"checkbox\"> "
    }
}

fn render_code(code: &Code) -> JsString {
    let other = Rules::of(Flavour::Normal).other;
    let lang = code.lang.clone().unwrap_or_default();
    let lang_string = other
        .not_space_start
        .exec(lang.units())
        .map(|found| JsString::from_slice(found.group_or_empty(lang.units(), 0)))
        .unwrap_or_default();
    // An empty code block has no content, so it gains no newline.
    let mut content = if code.text.is_empty() {
        JsString::new()
    } else {
        JsString::from_units(other.ending_newline.replace(code.text.units(), ""))
    };
    if !code.text.is_empty() {
        content.push_str("\n");
    }
    let mut out = if lang_string.is_empty() {
        JsString::from("<pre><code>")
    } else {
        let mut open = JsString::from("<pre><code class=\"language-");
        open.push_js(&escape_html_entities(&lang_string, false));
        open.push_str("\">");
        open
    };
    out.push_js(&escape_html_entities(&content, true));
    out.push_str("</code></pre>\n");
    out
}
