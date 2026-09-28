//! Diagnostics of the LQ-3 front end: the codes of [LQ/errors §5] that the reference model raises, spans in the
//! position convention of [LQ/lexical §2.3], and the value renderings of [LQ/errors §2.2].
//!
//! A [`Diag`] carries what the JSON error object of [LQ/errors §4.1] carries: the code, the span, the message rendered
//! with the value cap `V` = 256 and not fitted, the inline text, the help text, the did-you-mean candidates, the parser's
//! expected tokens and the detail lines. Fitting to 600 bytes and the located text form are the reference renderer's
//! (WP-71a).

use std::fmt;

/// A byte span into the source after the byte-order mark is removed ([LQ/lexical §2.3]); `end` is exclusive.
///
/// Spans are not part of a tree's identity: [LQ/canonical-ast §3.4] compares syntax trees "ignoring spans", so two
/// spans always compare equal. Code that needs a position reads `start` and `end` directly.
#[derive(Clone, Copy, Debug, Default)]
pub struct Span {
    /// First byte.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
}

impl Span {
    /// A span from `start` to `end`.
    pub const fn new(start: u32, end: u32) -> Span {
        Span { start, end }
    }

    /// The smallest span covering both.
    pub fn to(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

impl PartialEq for Span {
    fn eq(&self, _other: &Span) -> bool {
        true
    }
}

impl Eq for Span {}

/// The 1-based line and column of a byte offset ([LQ/lexical §2.3]): lines end at LF (a CR LF pair is one line end; a
/// lone CR ends no line), and columns count Unicode scalar values.
pub fn line_col(src: &str, offset: u32) -> (u32, u32) {
    let offset = (offset as usize).min(src.len());
    let before = &src.as_bytes()[..offset];
    let line = 1 + before.iter().filter(|&&b| b == b'\n').count() as u32;
    let line_start = before
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |p| p + 1);
    let col = 1 + src[line_start..offset].chars().count() as u32;
    (line, col)
}

/// Severity of a code ([LQ/errors §5.1]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// An error: the query does not run.
    Error,
    /// A warning: the query runs.
    Warning,
    /// A notice: the query runs.
    Notice,
}

macro_rules! codes {
    ($($code:ident $name:literal $exit:literal $sev:ident,)*) => {
        /// The diagnostic codes the reference model's front end raises ([LQ/errors §5.1], column "M0" = `model`,
        /// raised by the lexer, the parser or the binder).
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum Code {
            $(
                #[doc = $name]
                $code,
            )*
        }

        impl Code {
            /// The code as printed: `E001`, `W02`, `N08`.
            pub fn as_str(self) -> &'static str {
                match self { $(Code::$code => stringify!($code),)* }
            }

            /// The snake_case name of [LQ/errors §5.1].
            pub fn name(self) -> &'static str {
                match self { $(Code::$code => $name,)* }
            }

            /// The exit code of [LQ/errors §5.1] (0 for warnings and notices, which change no exit code, §4.4).
            pub fn exit(self) -> u8 {
                match self { $(Code::$code => $exit,)* }
            }

            /// The severity of [LQ/errors §5.1].
            pub fn severity(self) -> Severity {
                match self { $(Code::$code => Severity::$sev,)* }
            }
        }
    };
}

codes! {
    E001 "syntax" 2 Error,
    E002 "unterminated" 2 Error,
    E003 "bad_literal" 2 Error,
    E004 "not_in_lq" 2 Error,
    E005 "one_statement" 2 Error,
    E006 "read_only" 2 Error,
    E007 "expect_required" 2 Error,
    E009 "empty_tx" 2 Error,
    E101 "unknown_field" 2 Error,
    E102 "unknown_value" 2 Error,
    E103 "type_mismatch" 2 Error,
    E104 "unknown_edge_type" 2 Error,
    E105 "unknown_kind" 2 Error,
    E106 "edge_direction" 2 Error,
    E107 "ambiguous_edge_name" 2 Error,
    E108 "unknown_enum_word" 2 Error,
    E109 "unknown_function" 2 Error,
    E110 "bad_parameter" 2 Error,
    E111 "no_such_node" 2 Error,
    E112 "aggregate_misuse" 2 Error,
    E113 "path_variable" 2 Error,
    E114 "bad_quantifier" 2 Error,
    E115 "not_writable" 2 Error,
    E116 "step_variable_out_of_scope" 2 Error,
    E117 "store_local_in_definition" 2 Error,
    E118 "null_comparison" 2 Error,
    E301 "unknown_revision" 3 Error,
    E302 "not_at_this_view" 2 Error,
    E304 "too_many_refs" 10 Error,
    E305 "read_only_view" 6 Error,
    E308 "use_in_subquery" 2 Error,
    E406 "role_policy" 6 Error,
    E411 "unknown_model_write" 6 Error,
    W01 "absent_decided" 0 Warning,
    W02 "match_mode_ignored" 0 Warning,
    W07 "hand_derived_readiness" 0 Warning,
    W10 "link_state_none_admitted" 0 Warning,
    N08 "endpoint_pair_count" 0 Notice,
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One diagnostic: the content of [LQ/errors §4.1]'s error object (or §4.3's warning and notice object).
#[derive(Clone, Debug)]
pub struct Diag {
    /// The code.
    pub code: Code,
    /// The span it concerns; `None` for an unlocated diagnostic ([LQ/errors §3.3]).
    pub span: Option<Span>,
    /// Line 1's message, values capped at 256 bytes ([LQ/errors §4.1]).
    pub message: String,
    /// The inline text after the carets ([LQ/errors §3.2] line 5), if the case has one.
    pub inline: Option<Box<str>>,
    /// The help text, if the case has one.
    pub help: Option<Box<str>>,
    /// Did-you-mean candidates, nearest first, at most 5.
    pub suggest: Box<[String]>,
    /// Expected token spellings (E001 only), at most 10.
    pub expected: Box<[String]>,
    /// Detail lines ([LQ/errors §3.3]).
    pub detail: Box<[String]>,
}

impl Diag {
    /// A located diagnostic with a message only.
    pub fn new(code: Code, span: Span, message: impl Into<String>) -> Diag {
        Diag {
            code,
            span: Some(span),
            message: message.into(),
            inline: None,
            help: None,
            suggest: Box::default(),
            expected: Box::default(),
            detail: Box::default(),
        }
    }

    /// An unlocated diagnostic.
    pub fn unlocated(code: Code, message: impl Into<String>) -> Diag {
        Diag {
            span: None,
            ..Diag::new(code, Span::default(), message)
        }
    }

    /// Sets the inline text.
    pub fn inline(mut self, text: impl Into<String>) -> Diag {
        self.inline = Some(text.into().into_boxed_str());
        self
    }

    /// Sets the help text.
    pub fn help(mut self, text: impl Into<String>) -> Diag {
        self.help = Some(text.into().into_boxed_str());
        self
    }

    /// Adds a detail line.
    pub fn detail(mut self, text: impl Into<String>) -> Diag {
        let mut v = std::mem::take(&mut self.detail).into_vec();
        v.push(text.into());
        self.detail = v.into_boxed_slice();
        self
    }

    /// The start offset, or `u32::MAX` for an unlocated diagnostic (so located ones sort first).
    pub fn start(&self) -> u32 {
        self.span.map_or(u32::MAX, |s| s.start)
    }
}

impl fmt::Display for Diag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "error[{} {}]: {}",
            self.code,
            self.code.name(),
            self.message
        )?;
        if let Some(s) = self.span {
            write!(f, " @{}..{}", s.start, s.end)?;
        }
        Ok(())
    }
}

/// The value cap of the JSON message ([LQ/errors §4.1]: `V` = 256).
pub const VALUE_CAP: usize = 256;

/// Renders one interpolated value by [LQ/errors §2.1] and §2.3: C0 controls and DEL as `\u{XX}` (lower-case hex, no
/// leading zeros), TAB as one space, then cut at the last scalar boundary that leaves room for `...` within `cap` bytes.
pub fn value(text: &str, cap: usize) -> String {
    let mut pieces: Vec<String> = Vec::new();
    for c in text.chars() {
        let piece = match c {
            '\t' => " ".to_string(),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => format!("\\u{{{:x}}}", c as u32),
            c => c.to_string(),
        };
        pieces.push(piece);
    }
    let total: usize = pieces.iter().map(String::len).sum();
    if total <= cap {
        return pieces.concat();
    }
    let mut out = String::new();
    for p in pieces {
        if out.len() + p.len() + 3 > cap {
            break;
        }
        out.push_str(&p);
    }
    out.push_str("...");
    out
}

/// A name placeholder (`<kind>`, `<field>`, `<var>`, ...): the name inside back-quotes ([LQ/errors §2.2]).
pub fn q(name: &str) -> String {
    format!("`{}`", value(name, VALUE_CAP))
}

/// A `<list>` placeholder: up to 5 items joined by `, `, with `, ...` appended when more exist ([LQ/errors §2.2]).
pub fn list<S: AsRef<str>>(items: &[S]) -> String {
    let mut out = items
        .iter()
        .take(5)
        .map(|s| s.as_ref().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > 5 {
        out.push_str(", ...");
    }
    out
}

/// The Levenshtein distance of two strings, over Unicode scalar values, ASCII case folded.
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().map(|c| c.to_ascii_lowercase()).collect();
    let b: Vec<char> = b.chars().map(|c| c.to_ascii_lowercase()).collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ca != cb);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The did-you-mean candidates of [LQ/errors §5.3]: names within Levenshtein distance 2 of `word`, nearest first (ties
/// in the given order), at most 5, the word itself excluded.
pub fn near<'a, I: IntoIterator<Item = &'a str>>(word: &str, candidates: I) -> Vec<String> {
    let mut scored: Vec<(usize, usize, &str)> = candidates
        .into_iter()
        .enumerate()
        .map(|(i, c)| (levenshtein(word, c), i, c))
        .filter(|&(d, _, c)| d <= 2 && c != word)
        .collect();
    scored.sort();
    let mut out: Vec<String> = Vec::new();
    for (_, _, c) in scored {
        if !out.iter().any(|o| o == c) {
            out.push(c.to_string());
        }
        if out.len() == 5 {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_counts_scalars_and_lf_only() {
        let src = "ab\r\ncä\rd";
        assert_eq!(line_col(src, 0), (1, 1));
        assert_eq!(line_col(src, 4), (2, 1));
        // `ä` is two bytes, one scalar; a lone CR ends no line.
        assert_eq!(line_col(src, 7), (2, 3));
        assert_eq!(line_col(src, 8), (2, 4));
    }

    #[test]
    fn value_escapes_and_cuts() {
        assert_eq!(value("a\tb\u{1}", 64), "a b\\u{1}");
        assert_eq!(value("abcdefgh", 6), "abc...");
        assert_eq!(list(&["a", "b", "c", "d", "e", "f"]), "a, b, c, d, e, ...");
    }

    #[test]
    fn near_orders_by_distance() {
        assert_eq!(
            near("stauts", ["title", "status", "state"]),
            vec!["status", "state"]
        );
        assert_eq!(levenshtein("BLOCKS", "blocks"), 0);
    }

    #[test]
    fn codes_have_their_exit_codes() {
        assert_eq!(Code::E301.exit(), 3);
        assert_eq!(Code::E305.exit(), 6);
        assert_eq!(Code::E118.name(), "null_comparison");
        assert_eq!(Code::W02.severity(), Severity::Warning);
    }
}
