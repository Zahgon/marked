<a href="https://marked.js.org">
  <img width="60px" height="60px" src="https://marked.js.org/img/logo-black.svg" align="right" />
</a>

# Marked

[![github actions](https://github.com/markedjs/marked/workflows/Tests/badge.svg)](https://github.com/markedjs/marked/actions)
[![snyk](https://snyk.io/test/npm/marked/badge.svg)](https://snyk.io/test/npm/marked)
[![inspect.software](https://raw.githubusercontent.com/inspect-software/badges/main/v1/m/markedjs/marked.svg)](https://inspect.software/software/markedjs/marked)

- ⚡ built for speed
- ⬇️ low-level compiler for parsing markdown without caching or blocking for long periods of time
- ⚖️ light-weight while implementing all markdown features from the supported flavors & specifications
- 🦀 written in Rust, as a library and a command line program

## Demo

Check out the [demo page](https://marked.js.org/demo/) to see Marked in action ⛹️

## Docs

Our [documentation pages](https://marked.js.org) are also rendered using marked 💯

Also read about:

* [Options](https://marked.js.org/using_advanced)
* [Extensibility](https://marked.js.org/using_pro)

## Building

Marked is one Rust crate with no dependencies outside the standard library. It needs Rust 1.96 or later.

```sh
cargo build --release      # the library and the `marked` binary in target/release
cargo test                 # unit tests and the command's integration tests
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
```

## Usage

### Warning: 🚨 Marked does not sanitize the output HTML. Please use a sanitizer on the *output* HTML! 🚨

**Library**

```rust
let html = marked::parse_markdown("# Marked in Rust\n\nRendered by **marked**.")?;
assert_eq!(html, "<h1>Marked in Rust</h1>\n<p>Rendered by <strong>marked</strong>.</p>\n");
```

`marked::parse(&source, options, mode)` takes the source as a JSON value (anything but a string is an error, as in
the JavaScript library), the options (`gfm`, `breaks`, `pedantic`, `silent`) and the mode (`Mode::Block` for
`marked.parse`, `Mode::Inline` for `marked.parseInline`), and returns the HTML together with the tokens the lexer made
and the link definitions it collected.

**Command line**

The `marked` command takes no arguments. It reads one JSON request per line on standard input and writes one JSON
response per line on standard output:

```sh
$ printf '%s\n' '{"markdown":"*hello* world","output":"html"}' | marked
{"html":"<p><em>hello</em> world</p>\n"}
```

A request has `markdown` (the source), `output` (`"html"`, `"inline"`, `"tokens"` or `"inline-tokens"`) and optional
`options` (`gfm`, `breaks`, `pedantic`, `silent`, read for truthiness). `"tokens"` answers the token list of a document
with its link definitions (`{"tokens":[...],"links":{...}}`), `"inline-tokens"` the token list of inline content. An
invalid line or request, or an error of the parse, is answered with `{"error":{"name":...,"message":...}}`, and the
command then exits with status 1. Any argument is a usage error (exit status 2).

## Compatibility

The output is the output of marked 18.1.2, the JavaScript library this crate was ported from, byte for byte: the same
HTML, the same tokens with the same `raw` text, and the same JavaScript semantics underneath (UTF-16 strings, the
JavaScript regular expression dialect the grammar is written in, JSON as `JSON.parse` and `JSON.stringify` see it).
Extensions, hooks, `walkTokens` and the `async` option of the JavaScript library are not part of the port.

## License

Copyright (c) 2018+, MarkedJS. (MIT License)
Copyright (c) 2011-2018, Christopher Jeffrey. (MIT License)
