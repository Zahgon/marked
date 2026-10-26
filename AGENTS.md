# AGENTS.md

Instructions and guidelines for AI agents and LLMs writing code in `marked`.

---

## 1. Project Overview & Core Architecture

`marked` is a Markdown compiler, ported from the JavaScript library of the same name to one Rust crate with no
dependencies outside the standard library. Its output is the JavaScript library's output byte for byte, for CommonMark,
GitHub Flavored Markdown and the pedantic `Markdown.pl` flavour.

### Code Pipeline

1. **Grammar**: `src/rules.rs` assembles the block and inline regular expressions of every flavour from named parts,
   as the original does.
2. **Regular expressions**: `src/regexp/` runs them with JavaScript's `RegExp` semantics (UTF-16 code units, the `u`
   flag, lookbehind, backreferences, the `i` flag's case folding).
3. **Block tokenization**: `Lexer::lex` (`src/lexer.rs`) splits the source into block tokens with the rules in
   `src/tokenizer.rs`, queueing inline content.
4. **Inline tokenization**: the queued inline content is lexed into emphasis, links, code spans and so on.
5. **Rendering**: `Parser` (`src/parser.rs`) renders tokens as HTML, or as plain text for image alt text.
6. **Entry points**: `src/instance.rs` (`parse`, `parse_markdown`) and the `marked` command (`src/cli.rs`,
   `src/main.rs`), which answers JSON-lines requests.

`src/js/` holds the JavaScript runtime pieces the library depends on: UTF-16 strings and their methods, Unicode data,
JSON and errors.

---

## 2. Working on the crate

- Build, test, format and lint with cargo:
  `cargo build --release`, `cargo test`, `cargo fmt --all -- --check`, `cargo clippy --all-targets -- -D warnings`.
- Keep the output identical to marked 18.1.2. A change of behaviour is a bug unless the JavaScript library changed
  the same way.
- Keep the licence header (MIT, MarkedJS and Christopher Jeffrey) on every source file.
- No `unsafe`, no panics on any input, errors as `Result` values.
