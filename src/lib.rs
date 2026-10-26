// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! marked, a markdown parser and compiler, in Rust.
//!
//! [`parse_markdown`] compiles a document to HTML with the default options; [`instance::parse`] takes any input
//! value, the options and the mode, and returns the HTML and the tokens. The grammar is the original's regular
//! expressions, run by [`regexp`] with JavaScript's semantics over UTF-16 strings ([`js`]).

pub mod cli;
pub mod helpers;
pub mod instance;
pub mod js;
pub mod lexer;
pub mod options;
pub mod parser;
pub mod regexp;
pub mod rules;
mod tokenizer;
pub mod tokens;

pub use instance::{parse, parse_markdown, Mode, Output};
pub use options::Options;
