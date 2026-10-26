// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The grammar: the block and inline regular expressions of every flavour (normal, GFM, GFM with line breaks and
//! pedantic) and the helper expressions the tokenizer uses. Patterns are assembled from named parts with
//! [`edit`], exactly as the original assembles them, and compiled once per flavour.

use std::sync::OnceLock;

use crate::regexp::RegExp;

/// Compiles a pattern of the grammar. The grammar is fixed, so a pattern that does not compile is a defect of this
/// module, which debug builds and the tests report; in a release build such a rule never matches.
fn compile(source: &str, flags: &str) -> RegExp {
    match RegExp::new(source, flags) {
        Ok(regex) => regex,
        Err(error) => {
            debug_assert!(false, "the grammar pattern /{source}/{flags} is invalid: {}", error.0);
            RegExp::never()
        }
    }
}

/// The rule of a flavour that leaves the construct out (the original's `noopTest`).
fn never() -> RegExp {
    RegExp::never()
}

/// The caret of a part spliced into a larger pattern is dropped, unless it negates a class (`[^`).
fn strip_caret(source: &str) -> String {
    static CARET: OnceLock<RegExp> = OnceLock::new();
    let caret = CARET.get_or_init(|| compile(r"(^|[^\[])\^", "g"));
    let units: Vec<u16> = source.encode_utf16().collect();
    String::from_utf16_lossy(&caret.replace(&units, "$1"))
}

/// Builds a pattern from a template by replacing named parts, as the original's `edit()` does.
struct Edit {
    source: String,
    flags: &'static str,
}

/// Starts editing a pattern source with the flags of the result.
fn edit(source: &str, flags: &'static str) -> Edit {
    Edit {
        source: source.to_string(),
        flags,
    }
}

impl Edit {
    /// Replaces the first occurrence of `name` (a string name in the original).
    fn first(mut self, name: &str, value: &str) -> Edit {
        let value = strip_caret(value);
        if let Some(at) = self.source.find(name) {
            self.source.replace_range(at..at + name.len(), &value);
        }
        self
    }

    /// Replaces every occurrence of `name` (a global regular expression name in the original).
    fn all(mut self, name: &str, value: &str) -> Edit {
        let value = strip_caret(value);
        self.source = self.source.replace(name, &value);
        self
    }

    fn source(&self) -> &str {
        &self.source
    }

    fn get(self) -> RegExp {
        compile(&self.source, self.flags)
    }
}

/// A rule built for a list item's indentation: one pattern per indentation of 1 to 4 columns (the original caches
/// them by `min(3, indent - 1)` and builds each with that number).
pub struct IndentRule {
    by_indent: [RegExp; 4],
}

impl IndentRule {
    fn new(build: impl Fn(usize) -> String, flags: &str) -> IndentRule {
        IndentRule {
            by_indent: [0, 1, 2, 3].map(|indent| compile(&build(indent), flags)),
        }
    }

    /// The pattern for an indentation.
    pub fn get(&self, indent: usize) -> &RegExp {
        &self.by_indent[indent.saturating_sub(1).min(3)]
    }
}

/// The helper expressions the tokenizer and the renderer use.
pub struct Other {
    pub code_remove_indent: RegExp,
    pub output_link_replace: RegExp,
    pub indent_code_compensation: RegExp,
    pub beginning_space: RegExp,
    pub ending_hash: RegExp,
    pub starting_space_char: RegExp,
    pub ending_space_char: RegExp,
    pub ending_space_tab_char: RegExp,
    pub non_space_char: RegExp,
    pub new_line_char_global: RegExp,
    pub tab_char_global: RegExp,
    pub leading_space_tab: RegExp,
    pub multiple_space_global: RegExp,
    pub blank_line: RegExp,
    pub double_blank_line: RegExp,
    pub blockquote_start: RegExp,
    pub blockquote_setext_replace: RegExp,
    pub blockquote_setext_replace2: RegExp,
    pub blockquote_marker_whitespace: RegExp,
    pub list_replace_nesting: RegExp,
    pub list_is_task: RegExp,
    pub list_replace_task: RegExp,
    pub list_task_checkbox: RegExp,
    pub any_line: RegExp,
    pub href_brackets: RegExp,
    pub table_delimiter: RegExp,
    pub table_align_chars: RegExp,
    pub table_row_blank_line: RegExp,
    pub table_align_right: RegExp,
    pub table_align_center: RegExp,
    pub table_align_left: RegExp,
    pub start_a_tag: RegExp,
    pub end_a_tag: RegExp,
    pub start_pre_script_tag: RegExp,
    pub end_pre_script_tag: RegExp,
    pub start_angle_bracket: RegExp,
    pub end_angle_bracket: RegExp,
    pub pedantic_href_title: RegExp,
    pub unicode_alpha_numeric: RegExp,
    pub numeric_character_reference: RegExp,
    pub numeric_character_reference_or_ampersand: RegExp,
    pub escape_test: RegExp,
    pub escape_replace: RegExp,
    pub escape_test_no_encode: RegExp,
    pub escape_replace_no_encode: RegExp,
    pub percent_decode: RegExp,
    pub carriage_return: RegExp,
    pub space_line: RegExp,
    pub not_space_start: RegExp,
    pub ending_newline: RegExp,
    pub next_bullet: IndentRule,
    pub hr: IndentRule,
    pub fences_begin: IndentRule,
    pub heading_begin: IndentRule,
    pub pedantic_heading_begin: IndentRule,
    pub html_begin: IndentRule,
    pub blockquote_begin: IndentRule,
    /// The setext underline a lazy list continuation may look like.
    pub setext_underline_line: RegExp,
    /// The indentation of up to three spaces a setext continuation loses.
    pub leading_three_spaces: RegExp,
    /// A newline at the start of the text a nested blockquote left over.
    pub leading_newline: RegExp,
    /// A backtick, masked inside code spans.
    pub backtick_global: RegExp,
}

impl Other {
    /// `^( {0,3}<bull>)((?:[\t ][^\n]*)?(?:\n|$))`: one list item with the given bullet pattern.
    pub fn list_item(bull: &str) -> RegExp {
        compile(&format!("^( {{0,3}}{bull})((?:[\t ][^\\n]*)?(?:\\n|$))"), "")
    }

    fn new() -> Other {
        Other {
            code_remove_indent: compile(r"^(?: {0,3}\t| {1,4})", "gm"),
            output_link_replace: compile(r"\\([\[\]])", "g"),
            indent_code_compensation: compile(r"^(\s+)(?:```)", ""),
            beginning_space: compile(r"^\s+", ""),
            ending_hash: compile(r"#$", ""),
            starting_space_char: compile(r"^ ", ""),
            ending_space_char: compile(r" $", ""),
            ending_space_tab_char: compile(r"[ \t]$", ""),
            non_space_char: compile(r"[^ ]", ""),
            new_line_char_global: compile(r"\n", "g"),
            tab_char_global: compile(r"\t", "g"),
            leading_space_tab: compile(r"^[ \t]+", ""),
            multiple_space_global: compile(r"\s+", "g"),
            blank_line: compile(r"^[ \t]*$", ""),
            double_blank_line: compile(r"\n[ \t]*\n[ \t]*$", ""),
            blockquote_start: compile(r"^ {0,3}>", ""),
            blockquote_setext_replace: compile(r"\n {0,3}((?:=+|-+) *)(?=\n|$)", "g"),
            blockquote_setext_replace2: compile(r"^ {0,3}>[ \t]?", "gm"),
            blockquote_marker_whitespace: compile(r"^( {0,3}>)([ \t]+)", "gm"),
            list_replace_nesting: compile(r"^ {1,4}(?=( {4})*[^ ])", "g"),
            list_is_task: compile(r"^\[[ xX]\] +\S", ""),
            list_replace_task: compile(r"^\[[ xX]\] +", ""),
            list_task_checkbox: compile(r"\[[ xX]\]", ""),
            any_line: compile(r"\n.*\n", ""),
            href_brackets: compile(r"^<(.*)>$", ""),
            table_delimiter: compile(r"[:|]", ""),
            table_align_chars: compile(r"^\||\| *$", "g"),
            table_row_blank_line: compile(r"\n[ \t]*$", ""),
            table_align_right: compile(r"^ *-+: *$", ""),
            table_align_center: compile(r"^ *:-+: *$", ""),
            table_align_left: compile(r"^ *:-+ *$", ""),
            start_a_tag: compile(r"^<a ", "i"),
            end_a_tag: compile(r"^</a>", "i"),
            start_pre_script_tag: compile(r"^<(pre|code|kbd|script)(\s|>)", "i"),
            end_pre_script_tag: compile(r"^</(pre|code|kbd|script)(\s|>)", "i"),
            start_angle_bracket: compile(r"^<", ""),
            end_angle_bracket: compile(r">$", ""),
            pedantic_href_title: compile(r#"^([^'"]*[^\s])\s+(['"])(.*)\2"#, ""),
            unicode_alpha_numeric: compile(r"[\p{L}\p{N}]", "u"),
            numeric_character_reference: compile(r"&#(?:(\d{1,7})|[Xx]([A-Fa-f0-9]{1,6}));", ""),
            numeric_character_reference_or_ampersand: compile(
                r"&#(?:(\d{1,7})|[Xx]([A-Fa-f0-9]{1,6}));|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)",
                "g",
            ),
            escape_test: compile(r#"[&<>"']"#, ""),
            escape_replace: compile(r#"[&<>"']"#, "g"),
            escape_test_no_encode: compile(r#"[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)"#, ""),
            escape_replace_no_encode: compile(r#"[<>"']|&(?!(#\d{1,7}|#[Xx][a-fA-F0-9]{1,6}|\w+);)"#, "g"),
            percent_decode: compile(r"%25", "g"),
            carriage_return: compile(r"\r\n|\r", "g"),
            space_line: compile(r"^ +$", "gm"),
            not_space_start: compile(r"^\S*", ""),
            ending_newline: compile(r"\n$", ""),
            next_bullet: IndentRule::new(
                |indent| format!("^ {{0,{indent}}}(?:[*+-]|\\d{{1,9}}[.)])((?:[ \t][^\\n]*)?(?:\\n|$))"),
                "",
            ),
            hr: IndentRule::new(
                |indent| format!("^ {{0,{indent}}}((?:-[ \t]*){{3,}}|(?:_[ \t]*){{3,}}|(?:\\*[ \t]*){{3,}})(?:\\n+|$)"),
                "",
            ),
            fences_begin: IndentRule::new(|indent| format!("^ {{0,{indent}}}(?:```|~~~)"), ""),
            heading_begin: IndentRule::new(|indent| format!("^ {{0,{indent}}}#{{1,6}}(?:\\s|$)"), ""),
            pedantic_heading_begin: IndentRule::new(|indent| format!("^ {{0,{indent}}}#"), ""),
            html_begin: IndentRule::new(
                |indent| {
                    format!(
                        "^ {{0,{indent}}}(?:</?(?:{TAG})(?: +|$|/?>)|<(?:(?:script|pre|style|textarea)(?:[\\s>]|$)|!--)|<\\?|<![A-Za-z]|<!\\[CDATA\\[)"
                    )
                },
                "i",
            ),
            blockquote_begin: IndentRule::new(|indent| format!("^ {{0,{indent}}}>"), ""),
            setext_underline_line: compile(r"^ {0,3}(?:=+|-+) *$", ""),
            leading_three_spaces: compile(r"^ {0,3}", ""),
            leading_newline: compile(r"^\n", ""),
            backtick_global: compile(r"`", "g"),
        }
    }
}

/// The block-level grammar of one flavour.
pub struct BlockRules {
    pub blockquote: RegExp,
    pub code: RegExp,
    pub def: RegExp,
    pub fences: RegExp,
    pub heading: RegExp,
    pub hr: RegExp,
    pub html: RegExp,
    pub lheading: RegExp,
    pub list: RegExp,
    pub newline: RegExp,
    pub paragraph: RegExp,
    pub table: RegExp,
    pub text: RegExp,
}

/// The inline-level grammar of one flavour.
#[derive(Clone)]
pub struct InlineRules {
    pub backpedal: RegExp,
    pub any_punctuation: RegExp,
    pub any_punctuation_outside_code: RegExp,
    pub autolink: RegExp,
    pub block_skip: RegExp,
    pub br: RegExp,
    pub code: RegExp,
    pub del: RegExp,
    pub del_l_delim: RegExp,
    pub del_r_delim: RegExp,
    pub em_strong_l_delim: RegExp,
    pub em_strong_r_delim_ast: RegExp,
    pub em_strong_r_delim_und: RegExp,
    pub escape: RegExp,
    pub link: RegExp,
    pub nolink: RegExp,
    pub punctuation: RegExp,
    pub reflink: RegExp,
    pub reflink_search: RegExp,
    pub tag: RegExp,
    pub text: RegExp,
    pub url: RegExp,
}

/// The rules a lexer works with: the helpers and the grammar of its flavour.
#[derive(Clone, Copy)]
pub struct Rules {
    pub other: &'static Other,
    pub block: &'static BlockRules,
    pub inline: &'static InlineRules,
}

/// The flavours of the grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavour {
    /// CommonMark.
    Normal,
    /// GitHub Flavored Markdown.
    Gfm,
    /// GitHub Flavored Markdown where every newline is a line break.
    Breaks,
    /// The original `Markdown.pl`.
    Pedantic,
}

impl Rules {
    /// The rules of a flavour, built on first use.
    pub fn of(flavour: Flavour) -> Rules {
        let grammar = grammar();
        let (block, inline) = match flavour {
            Flavour::Normal => (&grammar.block_normal, &grammar.inline_normal),
            Flavour::Gfm => (&grammar.block_gfm, &grammar.inline_gfm),
            Flavour::Breaks => (&grammar.block_gfm, &grammar.inline_breaks),
            Flavour::Pedantic => (&grammar.block_pedantic, &grammar.inline_pedantic),
        };
        Rules {
            other: &grammar.other,
            block,
            inline,
        }
    }
}

struct Grammar {
    other: Other,
    block_normal: BlockRules,
    block_gfm: BlockRules,
    block_pedantic: BlockRules,
    inline_normal: InlineRules,
    inline_gfm: InlineRules,
    inline_breaks: InlineRules,
    inline_pedantic: InlineRules,
}

fn grammar() -> &'static Grammar {
    static GRAMMAR: OnceLock<Grammar> = OnceLock::new();
    GRAMMAR.get_or_init(Grammar::new)
}

const TAG: &str = "address|article|aside|base|basefont|blockquote|body|caption\
|center|col|colgroup|dd|details|dialog|dir|div|dl|dt|fieldset|figcaption\
|figure|footer|form|frame|frameset|h[1-6]|head|header|hr|html|iframe\
|legend|li|link|main|menu|menuitem|meta|nav|noframes|ol|optgroup|option\
|p|param|search|section|summary|table|tbody|td|tfoot|th|thead|title\
|tr|track|ul";

// Block-level parts.
const NEWLINE: &str = r"^(?:[ \t]*(?:\n|$))+";
const BLOCK_CODE: &str = r"^((?: {4}| {0,3}\t)[^\n]+(?:\n(?:[ \t]*(?:\n|$))*)?)+";
const FENCES: &str =
    r"^ {0,3}(`{3,}(?=[^`\n]*(?:\n|$))|~{3,})([^\n]*)(?:\n|$)(?:|([\s\S]*?)(?:\n|$))(?: {0,3}\1[~`]* *(?=\n|$)|$)";
const HR: &str = r"^ {0,3}((?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)";
const HEADING: &str = r"^ {0,3}(#{1,6})(?=\s|$)(.*)(?:\n+|$)";
const BULLET: &str = r" {0,3}(?:[*+-]|\d{1,9}[.)])";
const LHEADING_CORE: &str = r"^(?!bull |blockCode|fences|blockquote|heading|html|table)((?:.|\n(?!\s*?\n|bull |fences|blockquote|heading|hr|html|table))+?)\n {0,3}(=+|-+) *(?:\n+|$)";
const PARAGRAPH: &str = r"^([^\n]+(?:\n(?!hr|heading|lheading|blockquote|fences|list|html|table|[ \t]+\n)[^\n]+)*)";
const BLOCK_TEXT: &str = r"^[^\n]+";
const BLOCK_LABEL: &str = r"(?!\s*\])(?:\\[\s\S]|[^\[\]\\])+";
const COMMENT: &str = r"<!--(?:-?>|[\s\S]*?(?:-->|$))";
const PARAGRAPH_HTML: &str = " {0,3}(?:</?(?:tag)(?: +|\\n|/?>)|<(?:(?:script|pre|style|textarea)(?:[\\s>]|$)|!--)|<\\?|<![A-Za-z]|<!\\[CDATA\\[)";
const PARAGRAPH_FENCES: &str = " {0,3}(?:`{3,}(?=[^`\\n]*(?:\\n|$))|~~~)[^\\n]*(?:\\n|$)";
const HEADING_INTERRUPT: &str = " {0,3}#{1,6}(?:\\s|$)";

// Inline-level parts.
const PUNCTUATION: &str = r"[\p{P}\p{S}]";
const PUNCTUATION_OR_SPACE: &str = r"[\s\p{P}\p{S}]";
const NOT_PUNCTUATION_OR_SPACE: &str = r"[^\s\p{P}\p{S}]";
const OPEN_QUOTE: &str = r#"[\p{Pi}\p{Ps}"']"#;
const PUNCTUATION_GFM_STRONG_EM: &str = r"(?!~)[\p{P}\p{S}]";
const PUNCTUATION_OR_SPACE_GFM_STRONG_EM: &str = r"(?!~)[\s\p{P}\p{S}]";
const NOT_PUNCTUATION_OR_SPACE_GFM_STRONG_EM: &str = r"(?:[^\s\p{P}\p{S}]|~)";
const EM_STRONG_L_DELIM_CORE: &str = r"^(?:\*+(?:((?!\*)punct)|([^\s*]))?)|^_+(?:((?!_)punct)|([^\s_]))?";
const EM_STRONG_L_DELIM_PEDANTIC_CORE: &str =
    r"^(?:\*+(?:((?!\*)(?!openQuote)punct)|([^\s*]))?)|^_+(?:((?!_)(?!openQuote)punct)|([^\s_]))?";
const EM_STRONG_R_DELIM_AST_CORE: &str = concat!(
    r"^[^_*]*?__[^_*]*?\*[^_*]*?(?=__)",
    r"|[^*]+(?=[^*])",
    r"|(?!\*)punct(\*+)(?=[\s]|$)",
    r"|notPunctSpace(\*+)(?!\*)(?=punctSpace|$)",
    r"|(?!\*)punctSpace(\*+)(?=notPunctSpace)",
    r"|[\s](\*+)(?!\*)(?=punct)",
    r"|(?!\*)punct(\*+)(?!\*)(?=punct)",
    r"|notPunctSpace(\*+)(?=notPunctSpace)",
);
const EM_STRONG_R_DELIM_AST_PEDANTIC_CORE: &str = concat!(
    r"^[^_*]*?__[^_*]*?\*[^_*]*?(?=__)",
    r"|[^*]+(?=[^*])",
    r"|(?!\*)punct(\*+)(?=[\s]|$)",
    r"|notPunctSpace(\*+)(?!\*)(?=punctSpace|$)",
    r"|(?!\*)[\s](\*+)(?=notPunctSpace)",
    r"|[\s](\*+)(?!\*)(?=punct)",
    r"|(?!\*)punct(\*+)(?!\*)(?=punct)",
    r"|(?:(?!\*)punct|notPunctSpace)(\*+)(?!\*)(?=notPunctSpace)",
);
const EM_STRONG_R_DELIM_UND: &str = concat!(
    r"^[^_*]*?\*\*[^_*]*?_[^_*]*?(?=\*\*)",
    r"|[^_]+(?=[^_])",
    r"|(?!_)punct(_+)(?=[\s]|$)",
    r"|notPunctSpace(_+)(?!_)(?=punctSpace|$)",
    r"|(?!_)punctSpace(_+)(?=notPunctSpace)",
    r"|[\s](_+)(?!_)(?=punct)",
    r"|(?!_)punct(_+)(?!_)(?=punct)",
);
const EM_STRONG_R_DELIM_UND_PEDANTIC_CORE: &str = concat!(
    r"^[^_*]*?\*\*[^_*]*?_[^_*]*?(?=\*\*)",
    r"|[^_]+(?=[^_])",
    r"|(?!_)punct(_+)(?=[\s]|$)",
    r"|notPunctSpace(_+)(?!_)(?=punctSpace|$)",
    r"|(?!_)[\s](_+)(?=notPunctSpace)",
    r"|[\s](_+)(?!_)(?=punct)",
    r"|(?!_)punct(_+)(?!_)(?=punct)",
    r"|(?:(?!_)punct|notPunctSpace)(_+)(?!_)(?=notPunctSpace)",
);
const DEL_R_DELIM_CORE: &str = concat!(
    r"^[^~]+(?=[^~])",
    r"|(?!~)punct(~~?)(?=[\s]|$)",
    r"|notPunctSpace(~~?)(?!~)(?=punctSpace|$)",
    r"|(?!~)punctSpace(~~?)(?=notPunctSpace)",
    r"|[\s](~~?)(?!~)(?=punct)",
    r"|(?!~)punct(~~?)(?!~)(?=punct)",
    r"|notPunctSpace(~~?)(?=notPunctSpace)",
);
const INLINE_LABEL_BRACKETS: &str = r"\[(?:\\[\s\S]|[^\[\]\\])*\]";
const CASE_INSENSITIVE_PROTOCOL: &str = r"[hH][tT][tT][pP][sS]?|[fF][tT][pP]";
const EXTENDED_EMAIL: &str = r"[A-Za-z0-9._+-]+@[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![\w-])";
const INLINE_BR: &str = r"^( {2,}|\\)\n(?!\s*$)[ \t]*";

/// Puts the three punctuation classes into a delimiter template.
fn delimiters(core: &str, flags: &'static str, not_punct_space: &str, punct_space: &str, punct: &str) -> RegExp {
    edit(core, flags)
        .all("notPunctSpace", not_punct_space)
        .all("punctSpace", punct_space)
        .all("punct", punct)
        .get()
}

impl Grammar {
    fn new() -> Grammar {
        let other = Other::new();

        // Block grammar.
        let lheading_interrupts = |edit: Edit| {
            edit.all("bull", BULLET)
                .all("blockCode", r"(?: {4}| {0,3}\t)")
                .all("fences", r" {0,3}(?:`{3,}|~{3,})")
                .all("blockquote", r" {0,3}>")
                .all("heading", r" {0,3}#{1,6}(?:\s|$)")
                .all(
                    "hr",
                    r" {0,3}(?:(?:-[\t ]*){3,}|(?:_[ \t]*){3,}|(?:\*[ \t]*){3,})(?:\n+|$)",
                )
                .all("html", r" {0,3}<[^\n>]+>\n")
        };
        let lheading = lheading_interrupts(edit(LHEADING_CORE, "")).all("|table", "").get();
        let lheading_gfm = lheading_interrupts(edit(LHEADING_CORE, ""))
            .all("table", r" {0,3}\|?(?:[:\- ]*\|)+[\:\- ]*\n")
            .get();
        let def = edit(
            r#"^ {0,3}\[(label)\]: *(?:\n[ \t]*)?([^<\s][^\s]*|<.*?>)(?:(?: +(?:\n[ \t]*)?| *\n[ \t]*)(title))? *(?:\n+|$)"#,
            "",
        )
        .first("label", BLOCK_LABEL)
        .first("title", r#"(?:"(?:\\"?|[^"\\])*"|'[^'\n]*(?:\n[^'\n]+)*\n?'|\([^()]*\))"#)
        .get();
        let list = edit(r"^(bull)([ \t][^\n]*?)?(?:\n|$)", "").all("bull", BULLET).get();
        let html = edit(
            concat!(
                "^ {0,3}(?:",
                "<(script|pre|style|textarea)[\\s>][\\s\\S]*?(?:</\\1>[^\\n]*\\n*|$)",
                "|comment[^\\n]*(\\n+|$)",
                "|<\\?[\\s\\S]*?(?:\\?>[^\\n]*\\n*|$)",
                "|<![A-Z][\\s\\S]*?(?:>[^\\n]*\\n*|$)",
                "|<!\\[CDATA\\[[\\s\\S]*?(?:\\]\\]>[^\\n]*\\n*|$)",
                "|</?(tag)(?:[ \\t]+|\\n|/?>|$)[\\s\\S]*?(?:(?:\\n[ \t]*)+\\n|$)",
                "|<(?!(?:script|pre|style|textarea)(?![a-z0-9-]))([a-z][a-z0-9-]*)(?:attribute)*? */?>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ \t]*)+\\n|$)",
                "|</[a-z][a-z0-9-]*\\s*>(?=[ \\t]*(?:\\n|$))[\\s\\S]*?(?:(?:\\n[ \t]*)+\\n|$)",
                ")",
            ),
            "i",
        )
        .first("comment", COMMENT)
        .first("tag", TAG)
        .first(
            "attribute",
            r#" +[a-zA-Z:_][\w.:-]*(?: *= *"[^"\n]*"| *= *'[^'\n]*'| *= *[^\s"'=<>`]+)?"#,
        )
        .get();
        let create_paragraph = |list_interrupt: &str| {
            edit(PARAGRAPH, "")
                .first("hr", HR)
                .first("heading", HEADING_INTERRUPT)
                .first("|lheading", "")
                .first("|table", "")
                .first("blockquote", " {0,3}>")
                .first("fences", PARAGRAPH_FENCES)
                .first("list", list_interrupt)
                .first("html", PARAGRAPH_HTML)
                .first("tag", TAG)
                .get()
        };
        let paragraph = create_paragraph(r" {0,3}(?:[*+-]|1[.)])[ \t]+[^ \t\n]");
        let blockquote_paragraph = create_paragraph(r" {0,3}(?:[*+-]|\d{1,9}[.)])(?:[ \t]|\n|$)");
        let blockquote = edit(r"^( {0,3}> ?(?:(?![ \t]*(?:\n|$))(paragraph)|[^\n]*)(?:\n|$))+", "")
            .first("paragraph", blockquote_paragraph.source())
            .get();

        let gfm_table = edit(
            concat!(
                "^ *([^\\n ].*)\\n",
                " {0,3}((?:\\| *)?:?-+:? *(?:\\| *:?-+:? *)*(?:\\| *)?)",
                "(?:\\n((?:(?! *\\n|hr|heading|blockquote|code|fences|list|html).*(?:\\n|$))*)\\n*|$)",
            ),
            "",
        )
        .first("hr", HR)
        .first("heading", HEADING_INTERRUPT)
        .first("blockquote", " {0,3}>")
        .first("code", "(?: {4}| {0,3}\t)[^\\n]")
        .first("fences", PARAGRAPH_FENCES)
        .first("list", " {0,3}(?:[*+-]|1[.)])[ \\t]")
        .first("html", PARAGRAPH_HTML)
        .first("tag", TAG)
        .get();
        let paragraph_gfm = edit(PARAGRAPH, "")
            .first("hr", HR)
            .first("heading", HEADING_INTERRUPT)
            .first("|lheading", "")
            .first("table", gfm_table.source())
            .first("blockquote", " {0,3}>")
            .first("fences", PARAGRAPH_FENCES)
            .first("list", " {0,3}(?:[*+-]|1[.)])[ \\t]+[^ \\t\\n]")
            .first("html", PARAGRAPH_HTML)
            .first("tag", TAG)
            .get();
        let pedantic_tag = concat!(
            "(?!(?:",
            "a|em|strong|small|s|cite|q|dfn|abbr|data|time|code|var|samp|kbd|sub",
            "|sup|i|b|u|mark|ruby|rt|rp|bdi|bdo|span|br|wbr|ins|del|img)",
            "\\b)\\w+(?!:|[^\\w\\s@]*@)\\b",
        );
        let html_pedantic = edit(
            concat!(
                "^ *(?:comment *(?:\\n|\\s*$)",
                "|<(tag)[\\s\\S]+?</\\1> *(?:\\n{2,}|\\s*$)",
                "|<tag(?:\"[^\"]*\"|'[^']*'|\\s[^'\"/>\\s]*)*?/?> *(?:\\n{2,}|\\s*$))",
            ),
            "",
        )
        .first("comment", COMMENT)
        .all("tag", pedantic_tag)
        .get();
        let paragraph_pedantic = edit(PARAGRAPH, "")
            .first("hr", HR)
            .first("heading", " *#{1,6} *[^\n]")
            .first("lheading", lheading.source())
            .first("|table", "")
            .first("blockquote", " {0,3}>")
            .first("|fences", "")
            .first("|list", "")
            .first("|html", "")
            .first("|tag", "")
            .get();

        let block_normal = BlockRules {
            blockquote: blockquote.clone(),
            code: compile(BLOCK_CODE, ""),
            def: def.clone(),
            fences: compile(FENCES, ""),
            heading: compile(HEADING, ""),
            hr: compile(HR, ""),
            html: html.clone(),
            lheading: lheading.clone(),
            list: list.clone(),
            newline: compile(NEWLINE, ""),
            paragraph,
            table: never(),
            text: compile(BLOCK_TEXT, ""),
        };
        let block_gfm = BlockRules {
            blockquote: blockquote.clone(),
            code: compile(BLOCK_CODE, ""),
            def,
            fences: compile(FENCES, ""),
            heading: compile(HEADING, ""),
            hr: compile(HR, ""),
            html,
            lheading: lheading_gfm,
            list: list.clone(),
            newline: compile(NEWLINE, ""),
            paragraph: paragraph_gfm,
            table: gfm_table,
            text: compile(BLOCK_TEXT, ""),
        };
        let block_pedantic = BlockRules {
            blockquote,
            code: compile(BLOCK_CODE, ""),
            def: compile(
                r#"^ *\[([^\]]+)\]: *<?([^\s>]+)>?(?: +(["(][^\n]+[")]))? *(?:\n+|$)"#,
                "",
            ),
            fences: never(),
            heading: compile(r"^(#{1,6})(.*)(?:\n+|$)", ""),
            hr: compile(HR, ""),
            html: html_pedantic,
            lheading: compile(r"^(.+?)\n {0,3}(=+|-+) *(?:\n+|$)", ""),
            list,
            newline: compile(NEWLINE, ""),
            paragraph: paragraph_pedantic,
            table: never(),
            text: compile(BLOCK_TEXT, ""),
        };

        // Inline grammar.
        let escape = compile(r##"^\\([!"#$%&'()*+,\-./:;<=>?@\[\]\\^_`{|}~])"##, "");
        let inline_code = compile(r"^(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)", "");
        let inline_text = compile(
            r"^(`+|[^`])(?:(?= {2,}\n)|[\s\S]*?(?:(?=[\\<!\[`*_]|\b_|$)|[^ ](?= {2,}\n)))",
            "",
        );
        let punctuation = edit(r"^((?![*_])punctSpace)", "u")
            .all("punctSpace", PUNCTUATION_OR_SPACE)
            .get();
        let block_skip = edit("link|precode-code|html", "g")
            .first(
                "link",
                r"\[(?:[^\[\]`]|(?<a>`+)[^`]+\k<a>(?!`))*?\]\((?:\\[\s\S]|[^\\\(\)]|\((?:\\[\s\S]|[^\\\(\)])*\))*\)",
            )
            .first("precode-", "(?<!`)()")
            .first("code", r"(?<b>`+)[^`]+\k<b>(?!`)")
            .first("html", r"<(?! )[^<>]*?>")
            .get();
        let em_strong_l_delim = edit(EM_STRONG_L_DELIM_CORE, "u").all("punct", PUNCTUATION).get();
        let em_strong_l_delim_gfm = edit(EM_STRONG_L_DELIM_CORE, "u")
            .all("punct", PUNCTUATION_GFM_STRONG_EM)
            .get();
        let em_strong_l_delim_pedantic = edit(EM_STRONG_L_DELIM_PEDANTIC_CORE, "u")
            .all("openQuote", OPEN_QUOTE)
            .all("punct", PUNCTUATION)
            .get();
        let normal_delimiters =
            |core: &str| delimiters(core, "gu", NOT_PUNCTUATION_OR_SPACE, PUNCTUATION_OR_SPACE, PUNCTUATION);
        let em_strong_r_delim_ast = normal_delimiters(EM_STRONG_R_DELIM_AST_CORE);
        let em_strong_r_delim_ast_gfm = delimiters(
            EM_STRONG_R_DELIM_AST_CORE,
            "gu",
            NOT_PUNCTUATION_OR_SPACE_GFM_STRONG_EM,
            PUNCTUATION_OR_SPACE_GFM_STRONG_EM,
            PUNCTUATION_GFM_STRONG_EM,
        );
        let em_strong_r_delim_ast_pedantic = normal_delimiters(EM_STRONG_R_DELIM_AST_PEDANTIC_CORE);
        let em_strong_r_delim_und = normal_delimiters(EM_STRONG_R_DELIM_UND);
        let em_strong_r_delim_und_pedantic = normal_delimiters(EM_STRONG_R_DELIM_UND_PEDANTIC_CORE);
        let del_l_delim = edit(r"^~~?(?:((?!~)punct)|[^\s~])", "u")
            .all("punct", PUNCTUATION)
            .get();
        let del_r_delim = normal_delimiters(DEL_R_DELIM_CORE);
        let any_punctuation = edit(r"\\(punct)", "gu").all("punct", PUNCTUATION).get();
        let any_punctuation_outside_code = edit(r"(`+)(?:[^`]|[^`][\s\S]*?[^`])\1(?!`)|`+|\\(punct)", "gu")
            .all("punct", PUNCTUATION)
            .get();
        let autolink = edit(r"^<(scheme:[^\s\x00-\x1f<>]*|email)>", "")
            .first("scheme", r"[a-zA-Z][a-zA-Z0-9+.-]{1,31}")
            .first(
                "email",
                r"[a-zA-Z0-9.!#$%&'*+/=?^_`{|}~-]+(@)[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?(?:\.[a-zA-Z0-9](?:[a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?)+(?![-_])",
            )
            .get();
        let inline_comment = edit(COMMENT, "").first("(?:-->|$)", "-->");
        let tag = edit(
            concat!(
                "^comment",
                "|^</[a-zA-Z][a-zA-Z0-9-]*\\s*>",
                "|^<[a-zA-Z][a-zA-Z0-9-]*(?:attribute)*?\\s*/?>",
                "|^<\\?[\\s\\S]*?\\?>",
                "|^<![a-zA-Z]+\\s[\\s\\S]*?>",
                "|^<!\\[CDATA\\[[\\s\\S]*?\\]\\]>",
            ),
            "",
        )
        .first("comment", inline_comment.source())
        .first(
            "attribute",
            r#"\s+[a-zA-Z:_][\w.:-]*(?:\s*=\s*"[^"]*"|\s*=\s*'[^']*'|\s*=\s*[^\s"'=<>`]+)?"#,
        )
        .get();
        let inline_label = edit(
            r"(?:\[(?:brackets|\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|`+(?!`)[^`]*?`+(?!`)|``+(?=\])|[^\[\]\\`])*?",
            "",
        )
        .first("brackets", INLINE_LABEL_BRACKETS);
        let link = edit(
            r"^!?\[(label)\]\([ \t\n]*(href)(?:(?:[ \t]+(?:\n[ \t]*)?|\n[ \t]*)(title))?[ \t\n]*\)",
            "",
        )
        .first("label", inline_label.source())
        .first("href", r"<(?:\\.|[^\n<>\\])+>|[^ \t\n\x00-\x1f]+|(?=\))")
        .first(
            "title",
            r#""(?:\\"?|[^"\\])*"|'(?:\\'?|[^'\\])*'|\((?:\\\)?|[^)\\])*\)"#,
        )
        .get();
        let reflink = edit(r"^!?\[(label)\]\[(ref)\]", "")
            .first("label", inline_label.source())
            .first("ref", BLOCK_LABEL)
            .get();
        let nolink = edit(r"^!?\[(ref)\](?:\[\])?", "").first("ref", BLOCK_LABEL).get();
        let bounded_block_label = r"(?!\s*\])(?:\\[\s\S]|[^\[\]\\]){1,999}";
        let bounded_inline_label = edit(
            r"(?:[^\[\]\\`]*(?:\[(?:brackets|\\[\s\S]|[^\[\]\\])*\]|\\[\s\S]|`+(?!`)[^`]*?`+(?!`)|``+(?=\]))){0,999}?[^\[\]\\`]*?",
            "",
        )
        .first("brackets", INLINE_LABEL_BRACKETS);
        let reflink_search = edit(r"reflink|nolink(?!\()", "g")
            .first(
                "reflink",
                edit(r"^!?\[(label)\]\[(ref)\]", "")
                    .first("label", bounded_inline_label.source())
                    .first("ref", bounded_block_label)
                    .source(),
            )
            .first(
                "nolink",
                edit(r"^!?\[(ref)\](?:\[\])?", "")
                    .first("ref", bounded_block_label)
                    .source(),
            )
            .get();
        let extended_email_protocol =
            edit(r"(?:mailto:email|xmpp:email(?:\/[A-Za-z0-9@.]+)?)", "").all("email", EXTENDED_EMAIL);

        let inline_normal = InlineRules {
            backpedal: never(),
            any_punctuation: any_punctuation.clone(),
            any_punctuation_outside_code: any_punctuation_outside_code.clone(),
            autolink: autolink.clone(),
            block_skip: block_skip.clone(),
            br: compile(INLINE_BR, ""),
            code: inline_code.clone(),
            del: never(),
            del_l_delim: never(),
            del_r_delim: never(),
            em_strong_l_delim: em_strong_l_delim.clone(),
            em_strong_r_delim_ast: em_strong_r_delim_ast.clone(),
            em_strong_r_delim_und: em_strong_r_delim_und.clone(),
            escape: escape.clone(),
            link: link.clone(),
            nolink: nolink.clone(),
            punctuation: punctuation.clone(),
            reflink: reflink.clone(),
            reflink_search: reflink_search.clone(),
            tag: tag.clone(),
            text: inline_text,
            url: never(),
        };

        let inline_pedantic = InlineRules {
            em_strong_l_delim: em_strong_l_delim_pedantic,
            em_strong_r_delim_ast: em_strong_r_delim_ast_pedantic,
            em_strong_r_delim_und: em_strong_r_delim_und_pedantic,
            link: edit(r"^!?\[(label)\]\((.*?)\)", "")
                .first("label", inline_label.source())
                .get(),
            reflink: edit(r"^!?\[(label)\]\s*\[([^\]]*)\]", "")
                .first("label", inline_label.source())
                .get(),
            ..inline_normal.clone()
        };

        let gfm_text_source = edit(
            r"^(?:[^a-zA-Z0-9](?=emailProtocol)|(`+|~+|[^`~])(?:(?=[`~])|(?= {2,}\n)|(?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@)|[\s\S]*?(?:(?=[\\<!\[`*~_]|\b_|protocol:\/\/|www\.|$)|[^ ](?= {2,}\n)|[^a-zA-Z0-9](?=emailProtocol)|[^a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-](?=[a-zA-Z0-9.!#$%&'*+\/=?_`{\|}~-]+@))))",
            "",
        )
        .first("protocol", CASE_INSENSITIVE_PROTOCOL)
        .all("emailProtocol", r"(?:mailto|xmpp):");
        let inline_gfm = InlineRules {
            em_strong_r_delim_ast: em_strong_r_delim_ast_gfm,
            em_strong_l_delim: em_strong_l_delim_gfm,
            del_l_delim,
            del_r_delim,
            url: edit(
                r"^emailProtocol|^((?:protocol):\/\/|www\.)(?:[a-zA-Z0-9\-]+\.?)+[^\s<]*|^email",
                "",
            )
            .first("emailProtocol", extended_email_protocol.source())
            .first("protocol", CASE_INSENSITIVE_PROTOCOL)
            .first(
                "email",
                r"[A-Za-z0-9._+-]+(@)[a-zA-Z0-9-_]+(?:\.[a-zA-Z0-9-_]*[a-zA-Z0-9])+(?![\w-])",
            )
            .get(),
            backpedal: compile(
                r#"(?:[^?!.,:;*_'"~()&]+|\([^)]*\)|&(?![a-zA-Z0-9]+;$)|[?!.,:;*_'"~)]+(?!$))+"#,
                "",
            ),
            del: compile(
                r"^(~~?)(?=[^\s~])((?:\\[\s\S]|[^\\])*?(?:\\[\s\S]|[^\s~\\]))\1(?=[^~]|$)",
                "",
            ),
            text: compile(gfm_text_source.source(), ""),
            ..inline_normal.clone()
        };
        let inline_breaks = InlineRules {
            br: edit(INLINE_BR, "").first("{2,}", "*").get(),
            text: edit(gfm_text_source.source(), "")
                .first("\\b_", "\\b_| {2,}\\n")
                .all("{2,}", "*")
                .get(),
            ..inline_gfm.clone()
        };

        Grammar {
            other,
            block_normal,
            block_gfm,
            block_pedantic,
            inline_normal,
            inline_gfm,
            inline_breaks,
            inline_pedantic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pattern_compiles() {
        for flavour in [Flavour::Normal, Flavour::Gfm, Flavour::Breaks, Flavour::Pedantic] {
            let rules = Rules::of(flavour);
            assert!(rules.block.paragraph.test(&"a".encode_utf16().collect::<Vec<u16>>()));
        }
        assert!(Other::list_item("\\*").test(&"* a".encode_utf16().collect::<Vec<u16>>()));
    }

    #[test]
    fn edits_splice_parts_without_carets() {
        let edited = edit("^(a)x(a)", "").first("a", "^b").all("x", "[^c]");
        assert_eq!(edited.source(), "^(b)[^c](a)");
    }
}
