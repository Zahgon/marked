// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The `marked` command against answers of the original: blocks, inlines, options, tokens and errors.

use std::io::Write;
use std::process::{Command, Stdio};

/// Runs the command on `input`; returns its standard output and exit status.
fn run(input: &str) -> (String, Option<i32>) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_marked"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the marked binary starts");
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).expect("the request is written");
    }
    let output = child.wait_with_output().expect("the command finishes");
    (
        String::from_utf8(output.stdout).expect("the response is UTF-8"),
        output.status.code(),
    )
}

/// Asserts the response to one request line.
fn check(request: &str, expected: &str) {
    let (stdout, _) = run(&format!("{request}\n"));
    assert_eq!(stdout, format!("{expected}\n"), "request: {request}");
}

#[test]
fn atx_heading_with_closing_hashes() {
    check(
        "{\"markdown\": \"# Heading ##\", \"output\": \"html\"}",
        "{\"html\":\"<h1>Heading</h1>\\n\"}",
    );
}

#[test]
fn setext_heading_level_two() {
    check(
        "{\"markdown\": \"Title\\n---\", \"output\": \"html\"}",
        "{\"html\":\"<h2>Title</h2>\\n\"}",
    );
}

#[test]
fn fenced_code_keeps_info_string() {
    check(
        "{\"markdown\": \"```js\\nlet a = 1 < 2;\\n```\", \"output\": \"html\"}",
        "{\"html\":\"<pre><code class=\\\"language-js\\\">let a = 1 &lt; 2;\\n</code></pre>\\n\"}",
    );
}

#[test]
fn indented_code_after_paragraph_is_continuation() {
    check(
        "{\"markdown\": \"para\\n    not code\", \"output\": \"html\"}",
        "{\"html\":\"<p>para\\n    not code</p>\\n\"}",
    );
}

#[test]
fn nested_blockquote_with_lazy_line() {
    check(
        "{\"markdown\": \"> > a\\nlazy\\n> b\", \"output\": \"html\"}",
        "{\"html\":\"<blockquote>\\n<blockquote>\\n<p>a\\nlazy\\nb</p>\\n</blockquote>\\n</blockquote>\\n\"}",
    );
}

#[test]
fn tight_and_loose_lists() {
    check(
        "{\"markdown\": \"- a\\n- b\\n\\n- c\", \"output\": \"html\"}",
        "{\"html\":\"<ul>\\n<li><p>a</p>\\n</li>\\n<li><p>b</p>\\n</li>\\n<li><p>c</p>\\n</li>\\n</ul>\\n\"}",
    );
}

#[test]
fn ordered_list_start_number() {
    check(
        "{\"markdown\": \"7. seven\\n8. eight\", \"output\": \"html\"}",
        "{\"html\":\"<ol start=\\\"7\\\">\\n<li>seven</li>\\n<li>eight</li>\\n</ol>\\n\"}",
    );
}

#[test]
fn task_list_items() {
    check(
        "{\"markdown\": \"- [ ] todo\\n- [x] done\", \"output\": \"html\"}",
        "{\"html\":\"<ul>\\n<li><input disabled=\\\"\\\" type=\\\"checkbox\\\"> todo</li>\\n<li><input checked=\\\"\\\" disabled=\\\"\\\" type=\\\"checkbox\\\"> done</li>\\n</ul>\\n\"}",
    );
}

#[test]
fn task_list_ignored_without_gfm() {
    check(
        "{\"markdown\": \"- [ ] todo\", \"output\": \"html\", \"options\": {\"gfm\": false}}",
        "{\"html\":\"<ul>\\n<li>[ ] todo</li>\\n</ul>\\n\"}",
    );
}

#[test]
fn table_alignment_and_escaped_pipe() {
    check(
        "{\"markdown\": \"| a | b \\\\| c |\\n|:-|-:|\\n| 1 | 2 |\", \"output\": \"html\"}",
        "{\"html\":\"<table>\\n<thead>\\n<tr>\\n<th align=\\\"left\\\">a</th>\\n<th align=\\\"right\\\">b | c</th>\\n</tr>\\n</thead>\\n<tbody><tr>\\n<td align=\\\"left\\\">1</td>\\n<td align=\\\"right\\\">2</td>\\n</tr>\\n</tbody></table>\\n\"}",
    );
}

#[test]
fn emphasis_rules_of_three() {
    check(
        "{\"markdown\": \"*a**b*\", \"output\": \"inline\"}",
        "{\"html\":\"<em>a**b</em>\"}",
    );
}

#[test]
fn underscore_inside_words() {
    check(
        "{\"markdown\": \"snake_case_word and _em_\", \"output\": \"inline\"}",
        "{\"html\":\"snake_case_word and <em>em</em>\"}",
    );
}

#[test]
fn strikethrough_single_and_double() {
    check(
        "{\"markdown\": \"~a~ ~~b~~ ~~~c~~~\", \"output\": \"inline\"}",
        "{\"html\":\"<del>a</del> <del>b</del> ~~~c~~~\"}",
    );
}

#[test]
fn code_span_strips_one_space() {
    check(
        "{\"markdown\": \"`` `a` ``\", \"output\": \"inline\"}",
        "{\"html\":\"<code>`a`</code>\"}",
    );
}

#[test]
fn link_with_title_and_entity() {
    check(
        "{\"markdown\": \"[a &amp; b](/u?x=1&y=2 \\\"t\\\")\", \"output\": \"inline\"}",
        "{\"html\":\"<a href=\\\"/u?x=1&amp;y=2\\\" title=\\\"t\\\">a &amp; b</a>\"}",
    );
}

#[test]
fn image_alt_is_text_only() {
    check(
        "{\"markdown\": \"![*a* `b`](/i.png)\", \"output\": \"inline\"}",
        "{\"html\":\"<img src=\\\"/i.png\\\" alt=\\\"a b\\\">\"}",
    );
}

#[test]
fn reference_label_case_fold() {
    check(
        "{\"markdown\": \"[\\u1e9e]\\n\\n[SS]: /ss\", \"output\": \"html\"}",
        "{\"html\":\"<p><a href=\\\"/ss\\\">\u{1e9e}</a></p>\\n\"}",
    );
}

#[test]
fn first_definition_wins() {
    check(
        "{\"markdown\": \"[a]: /one\\n[A]: /two\\n\\n[a]\", \"output\": \"html\"}",
        "{\"html\":\"<p><a href=\\\"/one\\\">a</a></p>\\n\"}",
    );
}

#[test]
fn gfm_url_backpedals_punctuation() {
    check(
        "{\"markdown\": \"see www.example.com/a.\", \"output\": \"inline\"}",
        "{\"html\":\"see <a href=\\\"http://www.example.com/a\\\">www.example.com/a</a>.\"}",
    );
}

#[test]
fn angle_autolink_email() {
    check(
        "{\"markdown\": \"<a@b.co>\", \"output\": \"inline\"}",
        "{\"html\":\"<a href=\\\"mailto:a@b.co\\\">a@b.co</a>\"}",
    );
}

#[test]
fn html_block_and_inline_raw_text() {
    check(
        "{\"markdown\": \"<div>\\n*x*\\n</div>\\n\\n<pre>&#65;</pre> &#65;\", \"output\": \"html\"}",
        "{\"html\":\"<div>\\n*x*\\n</div><pre>&#65;</pre> &#65;\"}",
    );
}

#[test]
fn numeric_references_decode() {
    check(
        "{\"markdown\": \"&#0; &#x41; &#38; &#xD800;\", \"output\": \"inline\"}",
        "{\"html\":\"\u{fffd} A &amp; \u{fffd}\"}",
    );
}

#[test]
fn breaks_option() {
    check(
        "{\"markdown\": \"a\\nb\", \"output\": \"inline\", \"options\": {\"breaks\": true}}",
        "{\"html\":\"a<br>b\"}",
    );
}

#[test]
fn pedantic_heading_without_space() {
    check(
        "{\"markdown\": \"#a\\n\\n* x\", \"output\": \"html\", \"options\": {\"pedantic\": true}}",
        "{\"html\":\"<h1>a</h1>\\n<ul>\\n<li>x</li>\\n</ul>\\n\"}",
    );
}

#[test]
fn astral_punctuation_flanking() {
    check(
        "{\"markdown\": \"a*\\ud83d\\ude00*b *\\ud83d\\ude00*\", \"output\": \"inline\"}",
        "{\"html\":\"a*\u{1f600}*b <em>\u{1f600}</em>\"}",
    );
}

#[test]
fn line_separator_is_not_dot() {
    check(
        "{\"markdown\": \"# a\\u2028b\", \"output\": \"html\"}",
        "{\"html\":\"<p># a\u{2028}b</p>\\n\"}",
    );
}

#[test]
fn heading_tokens_carry_raw() {
    check(
        "{\"markdown\": \"## x\\n\\n\", \"output\": \"tokens\"}",
        "{\"tokens\":[{\"type\":\"heading\",\"raw\":\"## x\",\"depth\":2,\"text\":\"x\",\"tokens\":[{\"type\":\"text\",\"raw\":\"x\",\"text\":\"x\",\"escaped\":false}]},{\"type\":\"space\",\"raw\":\"\\n\\n\"}],\"links\":{}}",
    );
}

#[test]
fn link_definitions_in_property_order() {
    check(
        "{\"markdown\": \"[b]: /b\\n[10]: /ten\\n[2]: /two\", \"output\": \"tokens\"}",
        "{\"tokens\":[{\"type\":\"def\",\"tag\":\"b\",\"raw\":\"[b]: /b\\n\",\"href\":\"/b\"},{\"type\":\"def\",\"tag\":\"10\",\"raw\":\"[10]: /ten\\n\",\"href\":\"/ten\"},{\"type\":\"def\",\"tag\":\"2\",\"raw\":\"[2]: /two\",\"href\":\"/two\"}],\"links\":{\"2\":{\"href\":\"/two\"},\"10\":{\"href\":\"/ten\"},\"b\":{\"href\":\"/b\"}}}",
    );
}

#[test]
fn inline_tokens_have_no_links() {
    check(
        "{\"markdown\": \"**a** [b](/c)\", \"output\": \"inline-tokens\"}",
        "{\"tokens\":[{\"type\":\"strong\",\"raw\":\"**a**\",\"text\":\"a\",\"tokens\":[{\"type\":\"text\",\"raw\":\"a\",\"text\":\"a\",\"escaped\":false}]},{\"type\":\"text\",\"raw\":\" \",\"text\":\" \",\"escaped\":false},{\"type\":\"link\",\"raw\":\"[b](/c)\",\"href\":\"/c\",\"title\":null,\"text\":\"b\",\"tokens\":[{\"type\":\"text\",\"raw\":\"b\",\"text\":\"b\",\"escaped\":false}]}]}",
    );
}

#[test]
fn lone_surrogate_survives() {
    check(
        "{\"markdown\": \"a\\ud800b\", \"output\": \"tokens\"}",
        "{\"tokens\":[{\"type\":\"paragraph\",\"raw\":\"a\\ud800b\",\"text\":\"a\\ud800b\",\"tokens\":[{\"type\":\"text\",\"raw\":\"a\\ud800b\",\"text\":\"a\\ud800b\",\"escaped\":false}]}],\"links\":{}}",
    );
}

#[test]
fn non_string_source_is_an_error() {
    check(
        "{\"markdown\": 5, \"output\": \"html\"}",
        "{\"error\":{\"name\":\"Error\",\"message\":\"marked(): input parameter is of type [object Number], string expected\\nPlease report this to https://github.com/markedjs/marked.\"}}",
    );
}

#[test]
fn silent_error_becomes_html() {
    check(
        "{\"output\": \"html\", \"options\": {\"silent\": \"yes\"}}",
        "{\"html\":\"<p>An error occurred:</p><pre>marked(): input parameter is undefined or null\\nPlease report this to https://github.com/markedjs/marked.</pre>\"}",
    );
}

#[test]
fn silent_tokens_are_null() {
    check(
        "{\"markdown\": [1], \"output\": \"tokens\", \"options\": {\"silent\": 1}}",
        "{\"tokens\":null}",
    );
}

#[test]
fn option_truthiness() {
    check(
        "{\"markdown\": \"~~a~~\", \"output\": \"inline\", \"options\": {\"gfm\": \"false\"}}",
        "{\"html\":\"<del>a</del>\"}",
    );
}

#[test]
fn errors_and_exit_status() {
    let input = "nope\n{\"output\":\"x\"}\n{\"markdown\":\"a\",\"output\":\"html\",\"options\":{\"async\":true}}\n[]\n";
    let expected = "{\"error\":{\"name\":\"SyntaxError\",\"message\":\"Unexpected token 'o', \\\"nope\\\" is not valid JSON\"}}\n{\"error\":{\"name\":\"RequestError\",\"message\":\"unknown output: \\\"x\\\"\"}}\n{\"error\":{\"name\":\"RequestError\",\"message\":\"unknown option: \\\"async\\\"\"}}\n{\"error\":{\"name\":\"RequestError\",\"message\":\"a request must be a JSON object\"}}\n";
    let (stdout, status) = run(input);
    assert_eq!(stdout, expected);
    assert_eq!(status, Some(1));
}

#[test]
fn arguments_are_a_usage_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_marked"))
        .arg("--help")
        .output()
        .expect("the marked binary runs");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("usage: marked < requests.jsonl"));
}
