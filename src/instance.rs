// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The entry points `parse` and `parseInline`: input checks, lexing, rendering, and the error handling the `silent`
//! option changes.

use crate::helpers::escape_html_entities;
use crate::js::{JsError, JsString, Value};
use crate::lexer::{Lexer, TokensList};
use crate::options::Options;
use crate::parser::{Parser, RendererKind};

/// Whether a parse covers a whole document or inline content only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `marked.parse`: block and inline content.
    Block,
    /// `marked.parseInline`: inline content only.
    Inline,
}

/// What a parse produced: the HTML, and the tokens the lexer made (none when the input was rejected).
#[derive(Debug)]
pub struct Output {
    pub html: JsString,
    pub tokens: Option<TokensList>,
}

/// `marked.parse(src, options)` or `marked.parseInline(src, options)` of any JavaScript value. An error is returned
/// with the original's report suffix; with the silent option it becomes an HTML message instead.
pub fn parse(src: &Value, options: Options, mode: Mode) -> Result<Output, JsError> {
    run(src, options, mode).or_else(|mut error| {
        error
            .message
            .push_str("\nPlease report this to https://github.com/markedjs/marked.");
        if !options.silent {
            return Err(error);
        }
        let mut html = JsString::from("<p>An error occurred:</p><pre>");
        html.push_js(&escape_html_entities(&error.message, true));
        html.push_str("</pre>");
        Ok(Output { html, tokens: None })
    })
}

/// `marked.parse` of a string, with the default options.
pub fn parse_markdown(src: &str) -> Result<String, JsError> {
    parse(&Value::str(src), Options::default(), Mode::Block).map(|output| output.html.to_string_lossy())
}

fn run(src: &Value, options: Options, mode: Mode) -> Result<Output, JsError> {
    let src = match src {
        Value::Undefined | Value::Null => {
            return Err(JsError::error("marked(): input parameter is undefined or null"));
        }
        Value::String(src) => src,
        other => {
            return Err(JsError::error(format!(
                "marked(): input parameter is of type {}, string expected",
                type_tag(other)
            )));
        }
    };
    let lexer = Lexer::new(options);
    let parser = Parser::new(options);
    let tokens = match mode {
        Mode::Block => lexer.lex(src)?,
        Mode::Inline => TokensList {
            tokens: lexer.lex_inline(src)?,
            ..TokensList::default()
        },
    };
    let html = match mode {
        Mode::Block => parser.parse(&tokens.tokens)?,
        Mode::Inline => parser.parse_inline(&tokens.tokens, RendererKind::Html)?,
    };
    Ok(Output {
        html,
        tokens: Some(tokens),
    })
}

/// `Object.prototype.toString.call(value)` for the values JSON can give.
fn type_tag(value: &Value) -> &'static str {
    match value {
        Value::Undefined => "[object Undefined]",
        Value::Null => "[object Null]",
        Value::Bool(_) => "[object Boolean]",
        Value::Number(_) => "[object Number]",
        Value::String(_) => "[object String]",
        Value::Array(_) => "[object Array]",
        Value::Object(_) => "[object Object]",
    }
}
