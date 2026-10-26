// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The `marked` command: JSON-lines requests on standard input, one JSON response line each on standard output.
//!
//! A request is an object with `markdown` (any JSON value, handed to the parse as it is), `output` (`"html"`,
//! `"inline"`, `"tokens"` or `"inline-tokens"`) and optional `options` (`gfm`, `breaks`, `pedantic`, `silent`, any
//! JSON value each, read for truthiness). The responses are `{"html":...}`, `{"tokens":...,"links":...}`,
//! `{"tokens":...}`, or `{"error":{"name":...,"message":...}}` for a line that is not JSON, an invalid request, or an
//! error of the parse.

use crate::instance::{self, Mode};
use crate::js::json;
use crate::js::{JsError, JsString, Object, Value};
use crate::options::Options;
use crate::tokens::{write_links, write_tokens};

/// The usage text, written to standard error when the command is given arguments.
pub const USAGE: &str = "usage: marked < requests.jsonl\n\
\n\
Reads one JSON request per line on standard input and writes one JSON response per line.\n\
Request: {\"markdown\":S,\"output\":\"html\"|\"inline\"|\"tokens\"|\"inline-tokens\",\"options\":{...}}\n\
Options: gfm, breaks, pedantic, silent.\n";

const OPTION_NAMES: [&str; 4] = ["gfm", "breaks", "pedantic", "silent"];

/// The responses to a whole input, and whether any of them is an error.
pub struct Responses {
    /// The response lines, each ending in `\n`.
    pub output: String,
    /// Whether some request failed.
    pub failed: bool,
}

/// Answers every non-empty line of the input.
pub fn respond(input: &str) -> Responses {
    let mut output = String::new();
    let mut failed = false;
    for line in input.split('\n').filter(|line| !line.is_empty()) {
        let response = json::parse_str(line).and_then(|request| handle(&request));
        let text = match response {
            Ok(text) => text,
            Err(error) => {
                failed = true;
                error_response(&error)
            }
        };
        output.push_str(&text.to_string_lossy());
        output.push('\n');
    }
    Responses { output, failed }
}

fn request_error(message: impl Into<JsString>) -> JsError {
    JsError::new("RequestError", message)
}

fn error_response(error: &JsError) -> JsString {
    let mut out = JsString::from("{\"error\":{\"name\":");
    json::quote(&mut out, &error.name.encode_utf16().collect::<Vec<u16>>());
    out.push_str(",\"message\":");
    json::quote(&mut out, error.message.units());
    out.push_str("}}");
    out
}

/// `JSON.stringify(value)`, or `undefined` where it gives nothing.
fn stringify_for_message(value: &Value) -> JsString {
    json::stringify(value).unwrap_or_else(|| JsString::from("undefined"))
}

/// The options a request selects, on top of the defaults.
fn build_options(spec: Option<&Value>) -> Result<Options, JsError> {
    let mut options = Options::default();
    let Some(spec) = spec.filter(|spec| !spec.is_undefined()) else {
        return Ok(options);
    };
    let Value::Object(spec) = spec else {
        return Err(request_error("options must be an object"));
    };
    if let Some((key, _)) = spec
        .iter()
        .find(|(key, _)| !OPTION_NAMES.iter().any(|name| key.eq_str(name)))
    {
        let mut message = JsString::from("unknown option: ");
        message.push_js(&stringify_for_message(&Value::String(key.clone())));
        return Err(request_error(message));
    }
    let flag = |spec: &Object, name: &str, default: bool| spec.get_str(name).map_or(default, Value::truthy);
    options.gfm = flag(spec, "gfm", options.gfm);
    options.breaks = flag(spec, "breaks", options.breaks);
    options.pedantic = flag(spec, "pedantic", options.pedantic);
    options.silent = flag(spec, "silent", options.silent);
    Ok(options)
}

fn handle(request: &Value) -> Result<JsString, JsError> {
    let Value::Object(request) = request else {
        return Err(request_error("a request must be a JSON object"));
    };
    let output = request.get_str("output").cloned().unwrap_or_default();
    let kind = match &output {
        Value::String(kind) => ["html", "inline", "tokens", "inline-tokens"]
            .into_iter()
            .find(|name| kind.eq_str(name)),
        _ => None,
    };
    let Some(kind) = kind else {
        let mut message = JsString::from("unknown output: ");
        message.push_js(&stringify_for_message(&output));
        return Err(request_error(message));
    };
    let options = build_options(request.get_str("options"))?;
    let markdown = request.get_str("markdown").cloned().unwrap_or_default();
    let mode = if kind == "inline" || kind == "inline-tokens" {
        Mode::Inline
    } else {
        Mode::Block
    };
    let result = instance::parse(&markdown, options, mode)?;
    let mut out = JsString::new();
    match kind {
        "html" | "inline" => {
            out.push_str("{\"html\":");
            json::quote(&mut out, result.html.units());
            out.push_str("}");
        }
        _ => {
            out.push_str("{\"tokens\":");
            match &result.tokens {
                Some(list) => {
                    write_tokens(&mut out, &list.tokens);
                    if mode == Mode::Block {
                        out.push_str(",\"links\":");
                        write_links(&mut out, &list.links);
                    }
                }
                None => out.push_str("null"),
            }
            out.push_str("}");
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_requests_in_order() {
        let responses = respond(
            "{\"markdown\":\"# a\",\"output\":\"html\"}\n\n{\"markdown\":\"*b*\",\"output\":\"inline\"}\nnope\n",
        );
        assert_eq!(
            responses.output,
            "{\"html\":\"<h1>a</h1>\\n\"}\n{\"html\":\"<em>b</em>\"}\n\
             {\"error\":{\"name\":\"SyntaxError\",\"message\":\"Unexpected token 'o', \\\"nope\\\" is not valid JSON\"}}\n"
        );
        assert!(responses.failed);
    }

    #[test]
    fn reports_request_errors() {
        let responses = respond("{\"output\":\"x\"}\n{\"output\":\"html\",\"options\":{\"b\":1,\"2\":1}}\n");
        assert_eq!(
            responses.output,
            "{\"error\":{\"name\":\"RequestError\",\"message\":\"unknown output: \\\"x\\\"\"}}\n\
             {\"error\":{\"name\":\"RequestError\",\"message\":\"unknown option: \\\"2\\\"\"}}\n"
        );
    }

    #[test]
    fn silent_errors_become_html() {
        let responses = respond("{\"markdown\":5,\"output\":\"tokens\",\"options\":{\"silent\":1}}\n");
        assert_eq!(responses.output, "{\"tokens\":null}\n");
        assert!(!responses.failed);
    }
}
