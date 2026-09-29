//! [F14 §2] the text rules of image files: characters (§2.2), common tokens (§2.3), JSON strings with the canonical
//! escape set and the reading superset (§2.4), bare values and tokens (§2.5), base64url (§2.6), time text (§2.7) and the
//! canonical `f64` text (§2.8); and the named `ImageParse` rules every image refusal carries ([`Rule`], [F14 §9.2]).

use crate::prim::{Error, Result};

/// The rule an image refusal breaks: the clauses of [F14 §9.2] and §10.9 under the ids of the `moi/` fixture catalogue
/// (`fixtures/moi/INDEX.md` §4.2), and four refusals outside `ImageParse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    /// The marker is missing or breaks §4.
    Marker,
    /// An entry under `nodes/`, `schema/` or `refs/` breaks §3.4.
    Layout,
    /// A node file's `uid:` differs from its file name.
    UidFileName,
    /// A query file's name does not hash to its file name.
    QueryFileName,
    /// A file that is not valid UTF-8 after §9.1.
    NotUtf8,
    /// U+0000 outside a body or a block.
    NulOutsideBody,
    /// An unknown magic line or version.
    MagicVersion,
    /// A line that matches no production of its file.
    NoProduction,
    /// An unknown header key.
    UnknownHeaderKey,
    /// A required line missing.
    RequiredLineMissing,
    /// A line repeated.
    LineRepeated,
    /// A line not allowed in its form.
    LineNotAllowed,
    /// An ordinary line and a `conflict` line for one key.
    OrdinaryAndConflict,
    /// A body with a body conflict.
    BodyAndConflict,
    /// An unknown kind, field, enumeration value, edge kind or conflict class.
    UnknownName,
    /// A value that does not parse by its field's type.
    ValueType,
    /// A value that breaks its field's constraints.
    ValueConstraint,
    /// A `path` that breaks I-F8.
    PathRules,
    /// A `relink` outside R-17's grammar.
    RelinkGrammar,
    /// NaN, an infinity, an out-of-range number, a ledger whose sum leaves `i64`.
    NumberRange,
    /// Anchor texts that do not match their digests.
    AnchorTextDigest,
    /// Anchor texts partly present.
    AnchorTextsPartial,
    /// An anchor line without the digests its kind requires.
    AnchorDigests,
    /// An anchor line that breaks I-F9.
    AnchorIf9,
    /// `end_h` on a kind other than `range`.
    AnchorEndH,
    /// Non-canonical base64url.
    Base64url,
    /// A hint whose first line is greater than its last.
    AnchorHintOrder,
    /// `v=0`.
    AnchorV0,
    /// A lone surrogate in a JSON string.
    JsonLoneSurrogate,
    /// A JSON string that breaks §2.4.
    JsonString,
    /// A scope text that is not the image of a scope value.
    ScopeText,
    /// A named-query file that breaks §7.2.2's consistency rules.
    QueryConsistency,
    /// A named-query file that breaks §7.2.2's portability rules.
    QueryPortability,
    /// Leftover merge markers.
    MergeMarkers,
    /// A malformed value of a known trailer.
    TrailerMalformed,
    /// A repeated trailer.
    TrailerRepeated,
    /// `Moirai-Git-Head` and `Moirai-Git-Base` of different algorithms.
    TrailerAlgorithms,
    /// A `Moirai-Sync-Base` that differs from its second parent's stated id.
    TrailerSyncBase,
    /// A schema version other than 1.
    TrailerSchema,
    /// A foreign commit with more than two parents ([F07 §12.3]).
    ForeignParents,
    /// Outside `ImageParse`: a git object that breaks git's own format ([git-objects]).
    GitObject,
    /// Outside `ImageParse`: a native commit whose exported bytes or rebuilt id fail gate 0 ([F14 §10.1]–§10.5, §12.6).
    Export,
    /// Outside `ImageParse`: a native candidate that is demoted, so it has no native items ([F14 §10.9], §12.1).
    Demoted,
    /// Outside `ImageParse`: a carrier context that disagrees with the commit (a harness input, [F14 §12.6]).
    Context,
}

impl Rule {
    /// The rule's id in `fixtures/moi/INDEX.md` §4.2 (the four refusals outside `ImageParse` have ids of their own).
    pub fn id(self) -> &'static str {
        match self {
            Rule::Marker => "marker",
            Rule::Layout => "layout",
            Rule::UidFileName => "uid-file-name",
            Rule::QueryFileName => "query-file-name",
            Rule::NotUtf8 => "not-utf8",
            Rule::NulOutsideBody => "nul-outside-body",
            Rule::MagicVersion => "magic-version",
            Rule::NoProduction => "no-production",
            Rule::UnknownHeaderKey => "unknown-header-key",
            Rule::RequiredLineMissing => "required-line-missing",
            Rule::LineRepeated => "line-repeated",
            Rule::LineNotAllowed => "line-not-allowed",
            Rule::OrdinaryAndConflict => "ordinary-and-conflict",
            Rule::BodyAndConflict => "body-and-conflict",
            Rule::UnknownName => "unknown-name",
            Rule::ValueType => "value-type",
            Rule::ValueConstraint => "value-constraint",
            Rule::PathRules => "path-rules",
            Rule::RelinkGrammar => "relink-grammar",
            Rule::NumberRange => "number-range",
            Rule::AnchorTextDigest => "anchor-text-digest",
            Rule::AnchorTextsPartial => "anchor-texts-partial",
            Rule::AnchorDigests => "anchor-digests",
            Rule::AnchorIf9 => "anchor-if9",
            Rule::AnchorEndH => "anchor-end-h",
            Rule::Base64url => "base64url",
            Rule::AnchorHintOrder => "anchor-hint-order",
            Rule::AnchorV0 => "anchor-v0",
            Rule::JsonLoneSurrogate => "json-lone-surrogate",
            Rule::JsonString => "json-string",
            Rule::ScopeText => "scope-text",
            Rule::QueryConsistency => "query-consistency",
            Rule::QueryPortability => "query-portability",
            Rule::MergeMarkers => "merge-markers",
            Rule::TrailerMalformed => "trailer-malformed",
            Rule::TrailerRepeated => "trailer-repeated",
            Rule::TrailerAlgorithms => "trailer-algorithms",
            Rule::TrailerSyncBase => "trailer-sync-base",
            Rule::TrailerSchema => "trailer-schema",
            Rule::ForeignParents => "foreign-parents",
            Rule::GitObject => "git-object",
            Rule::Export => "export",
            Rule::Demoted => "demoted",
            Rule::Context => "context",
        }
    }
}

/// An `ImageParse` failure ([F14 §9.2]) of `rule` at a byte offset of the file.
pub fn parse_err<T>(rule: Rule, at: usize, m: impl Into<String>) -> Result<T> {
    Err(Error {
        offset: at,
        reason: format!("ImageParse: {}", m.into()),
        rule: Some(rule),
    })
}

/// A control character ([F14 §2.2]): U+0000–U+001F, U+007F, U+0080–U+009F.
pub fn is_control(c: char) -> bool {
    (c as u32) < 0x20 || (0x7F..=0x9F).contains(&(c as u32))
}

/// `LHEX` text of exactly `n` digits.
pub fn is_lhex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// `dec` ([F14 §2.3]): unsigned decimal without leading zeros.
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return None;
    }
    s.parse().ok()
}

/// `sdec` ([F14 §2.3]): signed decimal, "-0" not written.
pub fn parse_sdec(s: &str) -> Option<i64> {
    match s.strip_prefix('-') {
        Some(rest) if rest != "0" => {
            let v = parse_dec(rest)?;
            if v <= i64::MAX as u64 {
                Some(-(v as i64))
            } else if v == i64::MAX as u64 + 1 {
                Some(i64::MIN)
            } else {
                None
            }
        }
        Some(_) => None,
        None => parse_dec(s).and_then(|v| i64::try_from(v).ok()),
    }
}

/// True when `s` is written as an `sdec` ([F14 §2.3]: `0`, or an optional `-` and NZDIGIT *DIGIT), whatever its magnitude:
/// such a text that [`parse_sdec`] refuses is an out-of-range number, not a malformed one ([F14 §9.2]).
pub fn is_sdec_text(s: &str) -> bool {
    let d = s.strip_prefix('-').unwrap_or(s);
    s == "0" || (!d.is_empty() && !d.starts_with('0') && d.bytes().all(|b| b.is_ascii_digit()))
}

/// `commit-id` ([F14 §2.3]): `c` and 64 lower-case hex digits; returns the 32 bytes.
pub fn parse_commit_id(s: &str) -> Option<[u8; 32]> {
    let h = s.strip_prefix('c')?;
    if !is_lhex(h, 64) {
        return None;
    }
    crate::prim::unhex(h)?.try_into().ok()
}

/// `oid-text` ([F14 §2.3]).
pub fn parse_oid_text(s: &str) -> Option<crate::prim::Oid> {
    if let Some(h) = s.strip_prefix("sha1:") {
        is_lhex(h, 40).then(|| {
            crate::prim::Oid::Sha1(crate::prim::unhex(h).expect("hex").try_into().expect("20"))
        })
    } else if let Some(h) = s.strip_prefix("sha256:") {
        is_lhex(h, 64).then(|| {
            crate::prim::Oid::Sha256(crate::prim::unhex(h).expect("hex").try_into().expect("32"))
        })
    } else {
        None
    }
}

/// Writes an `oid-text`.
pub fn oid_text(o: &crate::prim::Oid) -> String {
    match o {
        crate::prim::Oid::None => String::new(),
        crate::prim::Oid::Sha1(d) => format!("sha1:{}", crate::prim::hex(d)),
        crate::prim::Oid::Sha256(d) => format!("sha256:{}", crate::prim::hex(d)),
    }
}

/// `iname` ([F14 §2.3], [F08 §8.2]): `[a-z][a-z0-9_]*`, 1–64 bytes.
pub fn is_iname(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

/// `vname`: `[A-Za-z0-9_][A-Za-z0-9_-]*`, 1–64 bytes.
pub fn is_vname(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_alphanumeric() || b[0] == b'_')
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

/// `lqname`: `[A-Z][A-Z0-9_]*`, 1–64 bytes.
pub fn is_lqname(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_uppercase()
        && b.iter()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == b'_')
}

/// `rootname` ([F08 §5.4.1]): 1–64 bytes of `[a-z0-9_-]` starting with a letter.
pub fn is_rootname(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_' || *c == b'-')
}

/// Decodes a JSON string starting at `s[0] == '"'` with the reading superset of [F14 §2.4]; returns the text and the
/// number of bytes consumed.
pub fn read_jstring(s: &str, at: usize) -> Result<(String, usize)> {
    let b = s.as_bytes();
    if b.first() != Some(&b'"') {
        return parse_err(
            Rule::JsonString,
            at,
            "a JSON string must begin with a double quote [F14 §2.4]",
        );
    }
    let mut out = String::new();
    let mut i = 1;
    loop {
        let Some(&c) = b.get(i) else {
            return parse_err(
                Rule::JsonString,
                at,
                "an unterminated JSON string [F14 §2.4]",
            );
        };
        match c {
            b'"' => return Ok((out, i + 1)),
            b'\\' => {
                let Some(&e) = b.get(i + 1) else {
                    return parse_err(
                        Rule::JsonString,
                        at + i,
                        "a JSON escape is cut off [F14 §2.4]",
                    );
                };
                i += 2;
                match e {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex4 = |j: usize| -> Option<u32> {
                            s.get(j..j + 4)
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                        };
                        let Some(u) = hex4(i).filter(|_| {
                            s.get(i..i + 4)
                                .is_some_and(|h| h.bytes().all(|x| x.is_ascii_hexdigit()))
                        }) else {
                            return parse_err(
                                Rule::JsonString,
                                at + i,
                                "a \\u escape needs four hexadecimal digits [F14 §2.4]",
                            );
                        };
                        i += 4;
                        let ch = if (0xD800..0xDC00).contains(&u) {
                            let lo = if s.get(i..i + 2) == Some("\\u") {
                                hex4(i + 2)
                            } else {
                                None
                            };
                            match lo {
                                Some(l) if (0xDC00..0xE000).contains(&l) => {
                                    i += 6;
                                    char::from_u32(0x10000 + ((u - 0xD800) << 10) + (l - 0xDC00))
                                }
                                _ => None,
                            }
                        } else {
                            char::from_u32(u)
                        };
                        let Some(ch) = ch else {
                            return parse_err(
                                Rule::JsonLoneSurrogate,
                                at + i,
                                "a lone surrogate in a JSON string [F14 §2.4]",
                            );
                        };
                        out.push(ch);
                    }
                    _ => {
                        return parse_err(
                            Rule::JsonString,
                            at + i,
                            "an unknown JSON escape [F14 §2.4]",
                        );
                    }
                }
            }
            0x00..=0x1F => {
                return parse_err(
                    Rule::JsonString,
                    at + i,
                    "a raw control character in a JSON string [F14 §2.4]",
                );
            }
            _ => {
                let ch = s[i..].chars().next().expect("char");
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    }
}

/// The canonical JSON string of a text ([F14 §2.4] "Writing").
pub fn jstring(t: &str) -> String {
    let mut o = String::with_capacity(t.len() + 2);
    o.push('"');
    for c in t.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\u{8}' => o.push_str("\\b"),
            '\u{C}' => o.push_str("\\f"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// [F14 §2.5]: whether a text is written bare as a line value.
pub fn bare_ok(t: &str) -> bool {
    !t.is_empty()
        && t.len() <= 4096
        && !t.chars().any(is_control)
        && !t.starts_with(' ')
        && !t.ends_with(' ')
        && !t.starts_with(['"', '[', '{'])
        && t != "<<"
}

/// Writes a line value (`sval`, [F14 §2.5]).
pub fn sval(t: &str) -> String {
    if bare_ok(t) { t.to_owned() } else { jstring(t) }
}

/// Reads a line value: a JSON string when it begins with `"`, else the bare text, which must obey `bare`.
pub fn read_sval(s: &str, at: usize) -> Result<String> {
    if s.starts_with('"') {
        let (t, n) = read_jstring(s, at)?;
        if n != s.len() {
            return parse_err(
                Rule::NoProduction,
                at + n,
                "bytes after a JSON line value [F14 §2.5]",
            );
        }
        Ok(t)
    } else {
        if s.is_empty()
            || s.starts_with(['[', '{', ' '])
            || s.ends_with(' ')
            || s.chars().any(is_control)
        {
            return parse_err(
                Rule::NoProduction,
                at,
                "a bare line value breaks `bare` [F14 §2.5]",
            );
        }
        Ok(s.to_owned())
    }
}

/// [F14 §2.5]: whether a text is written as a bare token.
pub fn tbare_ok(t: &str) -> bool {
    !t.is_empty()
        && !t.contains(' ')
        && !t.chars().any(is_control)
        && !t.starts_with(['"', '%', '[', '{'])
}

/// Writes a token of a text.
pub fn token(t: &str) -> String {
    if tbare_ok(t) {
        t.to_owned()
    } else {
        jstring(t)
    }
}

/// Reads one token at the start of `s` (a JSON string, or bare up to the next SP); returns (text, consumed, was_json).
pub fn read_token(s: &str, at: usize) -> Result<(String, usize, bool)> {
    if s.starts_with('"') {
        let (t, n) = read_jstring(s, at)?;
        Ok((t, n, true))
    } else {
        let n = s.find(' ').unwrap_or(s.len());
        let t = &s[..n];
        if t.is_empty() || t.starts_with(['%', '[', '{']) || t.chars().any(is_control) {
            return parse_err(
                Rule::NoProduction,
                at,
                "a bare token breaks `tbare` [F14 §2.5]",
            );
        }
        Ok((t.to_owned(), n, false))
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url without padding ([F14 §2.6]).
pub fn b64url(b: &[u8]) -> String {
    let mut o = String::new();
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        let k = c.len() + 1;
        for j in 0..k {
            o.push(B64[((n >> (18 - 6 * j)) & 63) as usize] as char);
        }
    }
    o
}

/// Decodes base64url, refusing what [F14 §2.6] refuses.
pub fn un_b64url(s: &str, at: usize) -> Result<Vec<u8>> {
    if s.len() % 4 == 1 {
        return parse_err(
            Rule::Base64url,
            at,
            "base64url length with remainder 1 [F14 §2.6]",
        );
    }
    let mut vals = Vec::with_capacity(s.len());
    for c in s.bytes() {
        match B64.iter().position(|&x| x == c) {
            Some(v) => vals.push(v as u32),
            None => {
                return parse_err(
                    Rule::Base64url,
                    at,
                    "a character outside the base64url alphabet, or padding [F14 §2.6]",
                );
            }
        }
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for c in vals.chunks(4) {
        let mut n = 0u32;
        for (j, v) in c.iter().enumerate() {
            n |= v << (18 - 6 * j);
        }
        let bytes = c.len() - 1;
        let unused = match bytes {
            1 => n & 0xFFFF,
            2 => n & 0xFF,
            _ => 0,
        };
        if unused != 0 {
            return parse_err(
                Rule::Base64url,
                at,
                "non-zero unused base64url bits [F14 §2.6]",
            );
        }
        for j in 0..bytes {
            out.push((n >> (16 - 8 * j)) as u8);
        }
    }
    Ok(out)
}

/// The `rfc3339ms` time text of an `hlc` ([F14 §2.7]); `None` from year 10000 on.
pub fn hlc_time(hlc: u64) -> Option<String> {
    let ms = hlc >> 16;
    let (secs, milli) = (ms / 1000, ms % 1000);
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil-from-days (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    if y >= 10_000 {
        return None;
    }
    Some(format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{milli:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    ))
}

/// `rfc3339ms` syntax ([F14 §2.7]).
pub fn is_rfc3339ms(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 24
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            10 => *c == b'T',
            13 | 16 => *c == b':',
            19 => *c == b'.',
            23 => *c == b'Z',
            _ => c.is_ascii_digit(),
        })
}

/// The canonical `f64` text of [F14 §2.8] (ECMA-262 `Number::toString`, then `.0` when neither `.` nor `e`).
pub fn f64_text(x: f64) -> String {
    if x == 0.0 {
        return "0.0".into();
    }
    let sci = format!("{:e}", x.abs());
    let (mant, exp) = sci.split_once('e').expect("exponent");
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let e: i32 = exp.parse().expect("exponent");
    let k = digits.len() as i32;
    let n = e + 1;
    let sign = if x < 0.0 { "-" } else { "" };
    let s = if (-5..=21).contains(&n) && x.abs() >= 1e-6 && x.abs() < 1e21 {
        if n >= k {
            format!("{digits}{}", "0".repeat((n - k) as usize))
        } else if n > 0 {
            format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
        } else {
            format!("0.{}{digits}", "0".repeat((-n) as usize))
        }
    } else {
        let es = if e >= 0 {
            format!("e+{e}")
        } else {
            format!("e-{}", -e)
        };
        if k == 1 {
            format!("{digits}{es}")
        } else {
            format!("{}.{}{es}", &digits[..1], &digits[1..])
        }
    };
    let s = if s.contains('.') || s.contains('e') {
        s
    } else {
        format!("{s}.0")
    };
    format!("{sign}{s}")
}

/// Parses `v-f64` ([F14 §2.8]); refuses NaN, infinities and values outside binary64.
pub fn parse_f64(s: &str, at: usize) -> Result<f64> {
    let body = s.strip_prefix('-').unwrap_or(s);
    let ok_syntax = if let Some((m, e)) = body.split_once('e') {
        let (ip, fp) = m.split_once('.').map_or((m, None), |(a, b)| (a, Some(b)));
        ip.len() == 1
            && ip.bytes().all(|c| c.is_ascii_digit())
            && fp.is_none_or(|f| !f.is_empty() && f.bytes().all(|c| c.is_ascii_digit()))
            && (e.starts_with('+') || e.starts_with('-'))
            && parse_dec(&e[1..]).is_some_and(|v| v > 0)
    } else if let Some((ip, fp)) = body.split_once('.') {
        !ip.is_empty()
            && !fp.is_empty()
            && ip.bytes().all(|c| c.is_ascii_digit())
            && fp.bytes().all(|c| c.is_ascii_digit())
    } else {
        false
    };
    if !ok_syntax {
        // [F14 §2.8]: NaN and the infinities are out-of-range numbers; any other text is not an f64 at all.
        let rule = if ["nan", "inf", "infinity"].contains(&body.to_ascii_lowercase().as_str()) {
            Rule::NumberRange
        } else {
            Rule::ValueType
        };
        return parse_err(rule, at, format!("{s:?} is not a v-f64 [F14 §2.8]"));
    }
    match s.parse::<f64>() {
        Ok(v) if v.is_finite() => Ok(if v == 0.0 { 0.0 } else { v }),
        _ => parse_err(
            Rule::NumberRange,
            at,
            "an f64 outside the finite binary64 range [F14 §2.8]",
        ),
    }
}

/// [F14 §9.1] rules 1–2: strips one BOM and turns every CR LF and lone CR into LF.
pub fn import_normalise(b: &[u8]) -> Vec<u8> {
    let b = b.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(b);
    let mut o = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\r' {
            o.push(b'\n');
            if b.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
        } else {
            o.push(b[i]);
        }
        i += 1;
    }
    o
}

/// Checks valid UTF-8 ([F14 §9.2]).
pub fn utf8_file(b: &[u8]) -> Result<&str> {
    core::str::from_utf8(b).map_err(|e| Error {
        offset: e.valid_up_to(),
        reason: "ImageParse: the file is not valid UTF-8 [F14 §9.2]".into(),
        rule: Some(Rule::NotUtf8),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [F14 §2.8] examples.
    #[test]
    fn f64_examples() {
        for (x, t) in [
            (28.6, "28.6"),
            (0.0, "0.0"),
            (0.001, "0.001"),
            (-3.0, "-3.0"),
            (1e21, "1e+21"),
            (1.5e-7, "1.5e-7"),
        ] {
            assert_eq!(f64_text(x), t);
            assert_eq!(parse_f64(t, 0).unwrap(), x);
        }
        assert_eq!(f64_text(123_456.0), "123456.0");
        assert_eq!(f64_text(1e-6), "0.000001");
        assert!(parse_f64("NaN", 0).is_err());
        assert!(parse_f64("1", 0).is_err());
        assert!(parse_f64("1e400", 0).is_err());
    }

    /// [F14 §17.2]: the window value's base64url; §2.6 refusals.
    #[test]
    fn b64url_example() {
        let w = [
            0x03, 0x00, 0x02, 0x00, 0x1F, 0x3A, 0x02, 0x9C, 0x7D, 0x0E, 0xB4, 0x51, 0xC8, 0xD2,
        ];
        assert_eq!(b64url(&w), "AwACAB86Apx9DrRRyNI");
        assert_eq!(un_b64url("AwACAB86Apx9DrRRyNI", 0).unwrap(), w);
        assert!(
            un_b64url("AwACAB86Apx9DrRRyNJ", 0).is_err(),
            "non-zero unused bits"
        );
        assert!(un_b64url("A", 0).is_err());
        assert!(un_b64url("AA==", 0).is_err());
    }

    /// [F14 §2.4]: canonical escapes and the reading superset.
    #[test]
    fn json_strings() {
        assert_eq!(
            jstring("a\"b\\\n\u{1}\u{7F}é"),
            "\"a\\\"b\\\\\\n\\u0001\u{7F}é\""
        );
        assert_eq!(
            read_jstring("\"\\u00E9\\/\\ud83d\\ude00\"", 0).unwrap().0,
            "é/😀"
        );
        assert!(read_jstring("\"\\ud83d\"", 0).is_err());
        assert!(!bare_ok(" x") && !bare_ok("<<") && !bare_ok("[x") && bare_ok("a b"));
        assert_eq!(sval("\"q"), "\"\\\"q\"");
    }

    /// [F14 §2.7] the time text of the [F01 §5.7] example `hlc`.
    #[test]
    fn time_text() {
        let hlc = (1_790_000_000_000u64 << 16) | 3;
        assert_eq!(hlc_time(hlc).unwrap(), "2026-09-21T14:13:20.000Z");
        assert!(is_rfc3339ms("2026-09-21T14:02:11.483Z"));
        assert!(!is_rfc3339ms("2026-09-21T14:02:11Z"));
    }
}

#[cfg(test)]
mod props {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// [F14 §2.8]: every finite `f64` survives its canonical text, and the text is a fixed point.
        #[test]
        fn f64_text_round_trip(bits in any::<u64>()) {
            let x = f64::from_bits(bits);
            prop_assume!(x.is_finite());
            let t = f64_text(x);
            let back = parse_f64(&t, 0).unwrap();
            prop_assert_eq!(back.to_bits(), if x == 0.0 { 0 } else { bits });
            prop_assert_eq!(f64_text(back), t);
        }

        /// [F14 §2.4]: the canonical JSON string of any text reads back to that text, consuming every byte.
        #[test]
        fn jstring_round_trip(t in any::<String>()) {
            let j = jstring(&t);
            let (back, n) = read_jstring(&j, 0).unwrap();
            prop_assert_eq!(back, t);
            prop_assert_eq!(n, j.len());
        }

        /// [F14 §2.5]: line values and tokens read back to the text they were written from.
        #[test]
        fn sval_and_token_round_trip(t in any::<String>()) {
            prop_assert_eq!(read_sval(&sval(&t), 0).unwrap(), t.clone());
            let k = token(&t);
            let (back, n, json) = read_token(&k, 0).unwrap();
            prop_assert_eq!(back, t.clone());
            prop_assert_eq!(n, k.len());
            prop_assert_eq!(json, !tbare_ok(&t));
        }

        /// [F14 §2.6]: base64url without padding round-trips any byte string.
        #[test]
        fn b64url_round_trip(b in proptest::collection::vec(any::<u8>(), 0..64)) {
            prop_assert_eq!(un_b64url(&b64url(&b), 0).unwrap(), b);
        }
    }
}
