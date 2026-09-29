//! The value types of [CFG §4.1] with their accepted texts and canonical forms, `Init`'s configuration ([API §8.1];
//! [CFG §7.6] `--set`), and the cross-key constraints C-1 to C-4 of [F17 §3] and K-1 of [CFG §10.8] ([CFG §5.3]).
//!
//! The key registry itself is [`crate::registry`]'s; this module parses and checks values against it.

use crate::err::{Refusal, Res};
use crate::registry::{self, Reload};
use std::collections::BTreeMap;

pub(crate) const KIB: u64 = 1 << 10;
pub(crate) const MIB: u64 = 1 << 20;
pub(crate) const GIB: u64 = 1 << 30;
pub(crate) const S: u64 = 1000;
pub(crate) const MIN: u64 = 60_000;
pub(crate) const H: u64 = 3_600_000;
pub(crate) const D: u64 = 86_400_000;

/// A value type of [CFG §4.1] with its parameters.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ty {
    /// `bool`.
    Bool,
    /// `int[lo..hi]`.
    Int(i64, i64),
    /// `size[lo..hi]`, bytes.
    Size(u64, u64),
    /// `size` restricted to a power of two in `[lo..hi]` (P01).
    SizePow2(u64, u64),
    /// `duration[lo..hi]`, milliseconds.
    Duration(u64, u64),
    /// `percent[lo..hi]`.
    Percent(u64, u64),
    /// `enum(a|b|…)`.
    Enum(&'static [&'static str]),
    /// `set(a|b|…)`.
    Set(&'static [&'static str]),
    /// `ref`: a branch name by [F12 §2.5] IN-4.
    Ref,
    /// `words`: open-vocabulary words separated by `,`, canonically sorted bytewise.
    Words,
    /// `word`: one open-vocabulary word.
    Word,
    /// `path`: an absolute path in [80 §2.10] P12's machine-local form.
    Path,
    /// `glob-list`: items separated by `,`, in written order.
    GlobList,
    /// `url-list`: items separated by `,`, in written order, without white space.
    UrlList,
    /// `git-ref`: a git branch name, short or full.
    GitRef,
    /// `family`: a `word`, or the reserved word `unknown` ([CFG §4.3]).
    Family,
}

/// A parsed value: its canonical text ([CFG §4.1] "Canonical form") and, for the numeric types, its number in the
/// type's unit (bytes, milliseconds, the integer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    /// The canonical form.
    pub canonical: String,
    /// The number of a numeric type.
    pub number: Option<u64>,
}

/// `int` text: decimal digits, optionally `e`/`E` and one or two digits ([CFG §4.1]); a sign only when `neg_ok`.
fn int_text(s: &str, neg_ok: bool) -> Option<i64> {
    let (neg, body) = match s.strip_prefix('-') {
        Some(b) if neg_ok => (true, b),
        Some(_) => return None,
        None => (false, s),
    };
    let (mant, exp) = match body.find(['e', 'E']) {
        Some(i) => (&body[..i], Some(&body[i + 1..])),
        None => (body, None),
    };
    if mant.is_empty() || !mant.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut v: i64 = mant.parse().ok()?;
    if let Some(e) = exp {
        if e.is_empty() || e.len() > 2 || !e.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let e: u32 = e.parse().ok()?;
        v = v.checked_mul(10i64.checked_pow(e)?)?;
    }
    Some(if neg { -v } else { v })
}

/// The canonical form of a size ([CFG §4.1]): the largest of `GiB`, `MiB`, `KiB` that divides a non-zero value
/// exactly, else plain decimal.
pub fn size_canon(v: u64) -> String {
    for (u, n) in [("GiB", GIB), ("MiB", MIB), ("KiB", KIB)] {
        if v != 0 && v.is_multiple_of(n) {
            return format!("{}{u}", v / n);
        }
    }
    v.to_string()
}

/// The canonical form of a duration in ms ([CFG §4.1]): the largest of `d`, `h`, `m`, `s`, `ms` that divides it.
pub fn duration_canon(ms: u64) -> String {
    for (u, n) in [("d", D), ("h", H), ("m", MIN), ("s", S)] {
        if ms.is_multiple_of(n) {
            return format!("{}{u}", ms / n);
        }
    }
    format!("{ms}ms")
}

/// An open-vocabulary word ([CFG §4.2]): 1 to 64 bytes of lower-case ASCII letters, digits, `-` and `_`, beginning
/// with a letter or a digit.
// spec: [CFG §4.2]
pub fn is_word(w: &str) -> bool {
    let b = w.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'-' || *c == b'_')
}

/// Splits a list value at `,`, each item trimmed of white space; the empty value (after trimming) is the empty list.
fn items(text: &str) -> Vec<&str> {
    if text.trim().is_empty() {
        Vec::new()
    } else {
        text.split(',')
            .map(|s| s.trim_matches([' ', '\t']))
            .collect()
    }
}

/// A `path` value ([CFG §4.1]) in [80 §2.10] P12's machine-local form: `/` separators, an upper-case drive letter, no
/// trailing `/`. The model runs on every target and reads no OS, so it accepts the Windows forms (`X:` then `/` or
/// `\`, a UNC `//server/share`) and the Unix form (a leading `/`) alike; no empty segment (but a trailing one), no
/// `.` or `..` segment, no C0 control.
// spec: [CFG §4.1] path
fn path_canon(text: &str) -> Option<String> {
    if text.is_empty() || text.chars().any(|c| (c as u32) < 0x20) {
        return None;
    }
    let s = text.replace('\\', "/");
    let (prefix, rest) = if let Some(r) = s.strip_prefix("//") {
        // UNC: //server/share/...
        let mut it = r.splitn(3, '/');
        let (server, share) = (it.next()?, it.next()?);
        if server.is_empty() || share.is_empty() {
            return None;
        }
        (
            format!("//{server}/{share}"),
            it.next().unwrap_or("").to_string(),
        )
    } else if let Some(r) = s.strip_prefix('/') {
        (String::new(), r.to_string())
    } else {
        let b = s.as_bytes();
        if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'/' {
            return None;
        }
        (
            format!("{}:", (b[0] as char).to_ascii_uppercase()),
            s[3..].to_string(),
        )
    };
    let rest = rest.strip_suffix('/').unwrap_or(&rest);
    let mut out = prefix;
    if !rest.is_empty() {
        for seg in rest.split('/') {
            if seg.is_empty() || seg == "." || seg == ".." {
                return None;
            }
            out.push('/');
            out.push_str(seg);
        }
    }
    if out.is_empty() || (out.len() == 2 && out.ends_with(':')) {
        out.push('/');
    }
    Some(out)
}

/// A git branch name, short (`main`) or full (`refs/heads/main`), by git's ref-name rules (`git check-ref-format`):
/// no control byte, space, `~`, `^`, `:`, `?`, `*`, `[`, `\`; no `..` or `@{`; no empty component, none beginning with
/// `.` or ending with `.lock`; not ending with `.` or `/`; not `@`.
// spec: [CFG §4.1] git-ref
pub fn is_git_ref(s: &str) -> bool {
    let name = s.strip_prefix("refs/heads/").unwrap_or(s);
    if name.is_empty()
        || name == "@"
        || name.ends_with('.')
        || name.contains("..")
        || name.contains("@{")
        || name
            .bytes()
            .any(|b| b < 0x20 || b == 0x7f || b" ~^:?*[\\".contains(&b))
    {
        return false;
    }
    name.split('/')
        .all(|c| !c.is_empty() && !c.starts_with('.') && !c.ends_with(".lock"))
}

/// Parses a value by its type ([CFG §4.1]): `None` when the text is not a valid value of the type and range.
// spec: [CFG §4.1]
pub fn parse(ty: Ty, text: &str) -> Option<Parsed> {
    let lower = text.to_ascii_lowercase();
    let num = |canonical: String, n: u64| {
        Some(Parsed {
            canonical,
            number: Some(n),
        })
    };
    let plain = |canonical: String| {
        Some(Parsed {
            canonical,
            number: None,
        })
    };
    match ty {
        Ty::Bool => {
            let b = match lower.as_str() {
                "true" | "yes" | "on" | "1" => true,
                "false" | "no" | "off" | "0" | "" => false,
                _ => return None,
            };
            num(b.to_string(), u64::from(b))
        }
        Ty::Int(lo, hi) => {
            let v = int_text(text, lo < 0)?;
            (lo..=hi).contains(&v).then(|| Parsed {
                canonical: v.to_string(),
                number: u64::try_from(v).ok(),
            })
        }
        Ty::Size(lo, hi) | Ty::SizePow2(lo, hi) => {
            let i = text
                .find(|c: char| !(c.is_ascii_digit() || c == 'e' || c == 'E'))
                .unwrap_or(text.len());
            let n = u64::try_from(int_text(&text[..i], false)?).ok()?;
            let m = match &text[i..] {
                "" | "B" => 1,
                "k" | "K" | "KiB" => KIB,
                "m" | "M" | "MiB" => MIB,
                "g" | "G" | "GiB" => GIB,
                _ => return None,
            };
            let b = n.checked_mul(m)?;
            let pow2_ok = !matches!(ty, Ty::SizePow2(..)) || b.is_power_of_two();
            ((lo..=hi).contains(&b) && pow2_ok).then(|| Parsed {
                canonical: size_canon(b),
                number: Some(b),
            })
        }
        Ty::Duration(lo, hi) => {
            let i = text
                .find(|c: char| !c.is_ascii_digit())
                .filter(|i| *i > 0)?;
            let n: u64 = text[..i].parse().ok()?;
            let m = match &text[i..] {
                "ms" => 1,
                "s" => S,
                "m" => MIN,
                "h" => H,
                "d" => D,
                _ => return None,
            };
            let ms = n.checked_mul(m)?;
            (lo..=hi).contains(&ms).then(|| Parsed {
                canonical: duration_canon(ms),
                number: Some(ms),
            })
        }
        Ty::Percent(lo, hi) => {
            let digits = text.strip_suffix('%').unwrap_or(text);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            let v: u64 = digits.parse().ok()?;
            (lo..=hi).contains(&v).then(|| Parsed {
                canonical: v.to_string(),
                number: Some(v),
            })
        }
        Ty::Enum(words) => words.contains(&lower.as_str()).then(|| Parsed {
            canonical: lower.clone(),
            number: None,
        }),
        Ty::Set(words) => {
            let mut members: Vec<String> = Vec::new();
            for w in items(&lower) {
                if !words.contains(&w) || members.iter().any(|m| m == w) {
                    return None;
                }
                members.push(w.to_string());
            }
            let ordered: Vec<&str> = words
                .iter()
                .copied()
                .filter(|w| members.iter().any(|m| m == w))
                .collect();
            plain(ordered.join(","))
        }
        Ty::Ref => crate::dag::check_branch_name(text)
            .ok()
            .and_then(|_| plain(text.to_string())),
        Ty::Words => {
            let mut v: Vec<&str> = items(&lower);
            if !v.iter().all(|w| is_word(w)) {
                return None;
            }
            v.sort_unstable();
            let n = v.len();
            v.dedup();
            (v.len() == n).then(|| Parsed {
                canonical: v.join(","),
                number: None,
            })
        }
        Ty::Word => is_word(&lower).then(|| Parsed {
            canonical: lower.clone(),
            number: None,
        }),
        Ty::Family => (lower == "unknown" || is_word(&lower)).then(|| Parsed {
            canonical: lower.clone(),
            number: None,
        }),
        Ty::Path => path_canon(text).and_then(plain),
        Ty::GlobList => {
            let v = items(text);
            (!v.iter().any(|i| i.is_empty())).then(|| Parsed {
                canonical: v.join(","),
                number: None,
            })
        }
        Ty::UrlList => {
            let v = items(text);
            (!v.iter()
                .any(|i| i.is_empty() || i.chars().any(char::is_whitespace)))
            .then(|| Parsed {
                canonical: v.join(","),
                number: None,
            })
        }
        Ty::GitRef => is_git_ref(text).then(|| Parsed {
            canonical: text.to_string(),
            number: None,
        }),
    }
}

/// The values `Init` set, by key instance, validated: (canonical text, number).
pub type Values = BTreeMap<String, Parsed>;

/// Parses `Init`'s `params` ([API §8.1]; [CFG §7.6]): each `key=value` must name an instance of a store-scope key of
/// the registry with a valid value, else `config_key` or `config_value` (exit 2); a user key, a retired name or policy
/// data is not a store key. Then C-1 to C-4 and K-1 hold on the given values and the defaults of every other key,
/// else `config_value` naming the constraint (exit 2).
// spec: [CFG §7.6]
// spec: [F17 §3]
pub fn parse_init(params: &[String], default_branch: Option<&str>) -> Res<Values> {
    let mut out = Values::new();
    let mut set = |key: &str, value: &str, whole: &str| -> Res<()> {
        let key = key.to_ascii_lowercase();
        let d = registry::find(&key)
            .filter(|(d, _)| !d.scope.is_user())
            .ok_or_else(|| {
                Refusal::new(
                    "config_key",
                    2,
                    format!("unknown configuration key {key}: init --set takes store keys"),
                )
                .key("key", key.as_str())
            })?
            .0;
        let p = registry::validate(d, &key, value).ok_or_else(|| {
            Refusal::new(
                "config_value",
                2,
                format!("{whole}: {key} takes another value"),
            )
            .key("key", key.as_str())
        })?;
        out.insert(key, p);
        Ok(())
    };
    for p in params {
        let (k, v) = p.split_once('=').ok_or_else(|| {
            Refusal::new("config_key", 2, format!("{p} is not key=value")).key("key", p.as_str())
        })?;
        set(k.trim(), v.trim(), p)?;
    }
    if let Some(b) = default_branch {
        set("default-branch", b, b)?;
    }
    check_constraints(&|k| out.get(k).cloned(), &|k| out.contains_key(k))?;
    Ok(out)
}

/// The number of a key instance: the value `Init` set, else its default.
pub fn number(v: &Values, key: &str) -> u64 {
    v.get(key).and_then(|p| p.number).unwrap_or_else(|| {
        registry::default_parsed(key, registry::Proc::Cli)
            .and_then(|p| p.number)
            .unwrap_or_else(|| panic!("{key} has no numeric default"))
    })
}

/// Whether a registry key is init-fixed ([F17 §2]).
pub fn is_init(key: &str) -> bool {
    registry::find(key).is_some_and(|(d, _)| d.reload == Reload::Init && !d.key.contains('<'))
}

/// C-1 to C-4 of [F17 §3] and K-1 of [CFG §10.8] over the effective values `get` gives (a key it does not give takes
/// its default); `given` says whether the command under check set a key, which the refusal names ([CFG §5.3];
/// [CFG §7.6]).
// spec: [F17 §3] C-1, C-2, C-3, C-4
// spec: [CFG §10.8] K-1
pub fn check_constraints(
    get: &dyn Fn(&str) -> Option<Parsed>,
    given: &dyn Fn(&str) -> bool,
) -> Res<()> {
    let n = |k: &str| {
        get(k).and_then(|p| p.number).unwrap_or_else(|| {
            registry::default_parsed(k, registry::Proc::Cli)
                .and_then(|p| p.number)
                .unwrap_or_else(|| panic!("{k} has no numeric default"))
        })
    };
    let fail = |c: &str, key: &str, why: String| {
        Err(Refusal::new("config_value", 2, format!("{c}: {why}")).key("key", key))
    };
    let pick = |a: &'static str, b: &'static str| if given(a) { a } else { b };
    let (p01, p05) = (
        n("store.log-extent-bytes"),
        n("store.commit.inline-max-bytes"),
    );
    if p05 > p01 / 8 {
        return fail(
            "C-1",
            pick("store.commit.inline-max-bytes", "store.log-extent-bytes"),
            format!(
                "store.commit.inline-max-bytes ({}) does not fit store.log-extent-bytes ({})",
                size_canon(p05),
                size_canon(p01)
            ),
        );
    }
    if n("store.tail.max-overlay-bytes") > n("store.tail.max-overlay-bytes.quiet") {
        return fail(
            "C-2",
            pick(
                "store.tail.max-overlay-bytes",
                "store.tail.max-overlay-bytes.quiet",
            ),
            "store.tail.max-overlay-bytes exceeds its quiet bound".into(),
        );
    }
    if n("idempotency.default-window") > n("idempotency.retention") {
        return fail(
            "C-3",
            pick("idempotency.default-window", "idempotency.retention"),
            "idempotency.default-window exceeds idempotency.retention".into(),
        );
    }
    // C-4 with n_other = 1 ([F17 §3]).
    if 2 + n("store.fold-width") + 1 > 8 {
        return fail(
            "C-4",
            "store.fold-width",
            "store.fold-width leaves no segment slot".into(),
        );
    }
    let (c2, c3, c5) = (n("pack.quota.c2"), n("pack.quota.c3"), n("pack.quota.c5"));
    for c4 in ["pack.quota.c4-dev", "pack.quota.c4-critic"] {
        if c2 + c3 + n(c4) + c5 > 100 {
            let key = ["pack.quota.c2", "pack.quota.c3", c4, "pack.quota.c5"]
                .into_iter()
                .find(|k| given(k))
                .unwrap_or(c4);
            return fail(
                "K-1",
                key,
                format!("pack.quota.c2 + .c3 + .{} + .c5 exceeds 100", &c4[11..]),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_parse_to_their_canonical_forms() {
        let size = Ty::Size(0, GIB);
        assert_eq!(parse(size, "64m").unwrap().canonical, "64MiB");
        assert_eq!(parse(size, "24000").unwrap().canonical, "24000");
        assert_eq!(parse(size, "16000000").unwrap().canonical, "15625KiB");
        assert_eq!(parse(size, "0").unwrap().canonical, "0");
        assert_eq!(
            parse(Ty::Int(0, 10_000_000), "2e6").unwrap().canonical,
            "2000000"
        );
        assert!(parse(Ty::Int(0, 10), "-1").is_none());
        assert!(parse(Ty::Int(0, 10), "1e123").is_none());
        let dur = Ty::Duration(MIN, 30 * D);
        assert_eq!(parse(dur, "900s").unwrap().canonical, "15m");
        assert!(parse(dur, "1ms").is_none(), "below 1m");
        assert!(parse(dur, "15").is_none(), "a bare number");
        assert!(parse(dur, "2w").is_none(), "no weeks in configuration");
        assert_eq!(parse(Ty::Bool, "Yes").unwrap().canonical, "true");
        assert_eq!(parse(Ty::Bool, "").unwrap().canonical, "false");
        assert_eq!(
            parse(
                Ty::Set(&["heartbeat", "cursor", "session-mark"]),
                "session-mark, heartbeat"
            )
            .unwrap()
            .canonical,
            "heartbeat,session-mark"
        );
        assert_eq!(parse(Ty::Set(&["a", "b"]), "").unwrap().canonical, "");
        assert!(parse(Ty::Set(&["a", "b"]), "a,a").is_none());
        assert!(parse(Ty::SizePow2(1 << 16, 1 << 30), "96KiB").is_none());
        assert!(parse(Ty::Ref, "lane/dev").is_some() && parse(Ty::Ref, "dev").is_none());
        assert_eq!(
            parse(Ty::Words, "owner, Orchestrator").unwrap().canonical,
            "orchestrator,owner"
        );
        assert!(parse(Ty::Words, "a,a").is_none() && parse(Ty::Words, "-a").is_none());
        assert_eq!(parse(Ty::Word, "Dev_1").unwrap().canonical, "dev_1");
        assert_eq!(parse(Ty::Family, "unknown").unwrap().canonical, "unknown");
        assert_eq!(
            parse(Ty::GlobList, " target/ ,build/").unwrap().canonical,
            "target/,build/"
        );
        assert!(parse(Ty::GlobList, "a,,b").is_none());
        assert_eq!(parse(Ty::GlobList, "").unwrap().canonical, "");
        assert!(parse(Ty::UrlList, "https://x/a b").is_none());
        assert!(parse(Ty::GitRef, "refs/heads/main").is_some());
        assert!(parse(Ty::GitRef, "a..b").is_none() && parse(Ty::GitRef, "x.lock").is_none());
    }

    #[test]
    fn paths_take_the_machine_local_form() {
        let p = |s: &str| parse(Ty::Path, s).map(|x| x.canonical);
        assert_eq!(p("d:\\notes\\").as_deref(), Some("D:/notes"));
        assert_eq!(p("D:/").as_deref(), Some("D:/"));
        assert_eq!(p("/home/a/b").as_deref(), Some("/home/a/b"));
        assert_eq!(p("\\\\srv\\share\\x").as_deref(), Some("//srv/share/x"));
        assert_eq!(p("relative/x"), None);
        assert_eq!(p("/a/../b"), None);
        assert_eq!(p("/a//b"), None);
    }

    #[test]
    fn init_refuses_unknown_keys_bad_values_and_constraints() {
        let code = |ps: &[&str]| {
            let v: Vec<String> = ps.iter().map(|s| s.to_string()).collect();
            parse_init(&v, None)
                .err()
                .map(|e| (e.code.clone(), e.get_str("key").map(str::to_string)))
        };
        assert_eq!(code(&["no.such.key=1"]).unwrap().0, "config_key");
        assert_eq!(
            code(&["roots.docs=/tmp"]).unwrap().0,
            "config_key",
            "a user key"
        );
        assert_eq!(
            code(&["edges.blocks.on-src-deleted=flag"]).unwrap().0,
            "config_key",
            "policy data is not a key"
        );
        assert_eq!(code(&["lease.ttl-default=5ms"]).unwrap().0, "config_value");
        assert_eq!(code(&["store.suspect-budget=0"]).unwrap().0, "config_value");
        assert_eq!(code(&["default-branch=dev"]).unwrap().0, "config_value");
        let c1 = code(&["store.log-extent-bytes=64KiB"]).unwrap();
        assert_eq!(
            c1,
            ("config_value".into(), Some("store.log-extent-bytes".into()))
        );
        assert_eq!(
            code(&["idempotency.retention=1m", "idempotency.default-window=2m"])
                .unwrap()
                .1
                .as_deref(),
            Some("idempotency.default-window"),
            "C-3"
        );
        assert_eq!(
            code(&["store.tail.max-overlay-bytes=4MiB"]).unwrap().0,
            "config_value",
            "C-2 against the design figure of the quiet bound"
        );
        assert_eq!(
            code(&["pack.quota.c2=60"]).unwrap().1.as_deref(),
            Some("pack.quota.c2"),
            "K-1"
        );
        assert!(
            code(&[
                "Store.Log-Extent-Bytes=64KiB",
                "store.commit.inline-max-bytes=4KiB",
                "query.safelist.model.unknown=off",
                "query.caps.developer.rows=100",
            ])
            .is_none()
        );
    }
}
