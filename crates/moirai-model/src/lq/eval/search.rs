//! `search()` and `text_match()` ([50 §5.5]; [LQ/std §2.9]): tokenizer v1 of [F09 §12.1] (Unicode 17.0.0 letter and
//! number runs, simple lowercase, `ё` → `е`, terms cut at 64 bytes) read from the pinned UCD files, the query side of
//! [LQ/std] (`term*` prefix, `-term` exclusion), and the two scorers LQ-Bench's ablation compares ([50 §7.4] item 7):
//! BM25 with statistics on the view's live documents (k1 = 1.2, b = 0.75, field weights title 3, abstract 2, body 1,
//! terms in query order, fields in that order, ties by id) and the statistics-free scorer (matched terms weighted
//! 3, 2, 1; ties by recency, then id). Rankings only ever compare scores with each other.

use super::val::{self, V};
use super::view::Ev;
use crate::value::Nid;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The tokenizer's Unicode data: the letter and number ranges, and the simple lowercase mappings.
struct Ucd {
    word: Vec<(u32, u32)>,
    lower: HashMap<u32, u32>,
}

/// The data, read once from `UnicodeData.txt` (fields 2 and 13; `<…, First>`/`<…, Last>` pairs are ranges).
fn ucd() -> &'static Ucd {
    static U: OnceLock<Ucd> = OnceLock::new();
    U.get_or_init(|| {
        let bytes = crate::r4::ucd::read_pinned("UnicodeData.txt");
        let text = String::from_utf8_lossy(&bytes);
        let mut word: Vec<(u32, u32)> = Vec::new();
        let mut lower = HashMap::new();
        let mut first: Option<u32> = None;
        for line in text.lines() {
            let f: Vec<&str> = line.split(';').collect();
            if f.len() < 14 {
                continue;
            }
            let Ok(cp) = u32::from_str_radix(f[0], 16) else {
                continue;
            };
            let is_word = matches!(f[2], "Lu" | "Ll" | "Lt" | "Lm" | "Lo" | "Nd" | "Nl" | "No");
            let (lo, hi) = if f[1].ends_with(", First>") {
                first = Some(cp);
                continue;
            } else if f[1].ends_with(", Last>") {
                (first.take().unwrap_or(cp), cp)
            } else {
                (cp, cp)
            };
            if is_word {
                match word.last_mut() {
                    Some(last) if last.1 + 1 == lo => last.1 = hi,
                    _ => word.push((lo, hi)),
                }
            }
            if let Ok(l) = u32::from_str_radix(f[13], 16) {
                lower.insert(cp, l);
            }
        }
        Ucd { word, lower }
    })
}

fn is_word(c: char) -> bool {
    let u = ucd();
    let cp = c as u32;
    u.word
        .binary_search_by(|(lo, hi)| {
            if cp < *lo {
                std::cmp::Ordering::Greater
            } else if cp > *hi {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// The terms of a text by tokenizer v1 ([F09 §12.1]), with each token's byte offset.
// spec: [F09 §12.1]
pub fn tokens(text: &str) -> Vec<(usize, String)> {
    let u = ucd();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut start = 0;
    let flush = |cur: &mut String, start: usize, out: &mut Vec<(usize, String)>| {
        if !cur.is_empty() {
            let mut t = std::mem::take(cur);
            if t.len() > 64 {
                let mut end = 64;
                while !t.is_char_boundary(end) {
                    end -= 1;
                }
                t.truncate(end);
            }
            out.push((start, t));
        }
    };
    for (i, c) in text.char_indices() {
        if is_word(c) {
            if cur.is_empty() {
                start = i;
            }
            let l = u
                .lower
                .get(&(c as u32))
                .and_then(|x| char::from_u32(*x))
                .unwrap_or(c);
            cur.push(if l == '\u{451}' { '\u{435}' } else { l });
        } else {
            flush(&mut cur, start, &mut out);
        }
    }
    flush(&mut cur, start, &mut out);
    out
}

/// One query term: its text, a prefix term (`term*`), an excluded term (`-term`).
#[derive(Clone, Debug)]
struct Term {
    text: String,
    prefix: bool,
    exclude: bool,
}

/// The terms of a query text ([LQ/std] query side of [F09 §12.1]): whitespace-separated words, `-` excluding and a final
/// `*` making a prefix; each word's tokens are terms.
// spec: [LQ/std §2.9] search
fn query_terms(q: &str) -> Vec<Term> {
    let mut out = Vec::new();
    for w in q.split_whitespace() {
        let (exclude, w) = match w.strip_prefix('-') {
            Some(r) => (true, r),
            None => (false, w),
        };
        let (prefix, w) = match w.strip_suffix('*') {
            Some(r) => (true, r),
            None => (false, w),
        };
        let toks = tokens(w);
        let n = toks.len();
        for (i, (_, t)) in toks.into_iter().enumerate() {
            out.push(Term {
                text: t,
                prefix: prefix && i + 1 == n,
                exclude,
            });
        }
    }
    out
}

impl Term {
    fn hits(&self, toks: &[(usize, String)]) -> usize {
        toks.iter()
            .filter(|(_, t)| {
                if self.prefix {
                    t.starts_with(&self.text)
                } else {
                    *t == self.text
                }
            })
            .count()
    }
}

/// The tokens of a field, with their byte offsets.
type Toks = Vec<(usize, String)>;

/// A document: its node, the tokens of each searched field, and each field's text.
type Doc = (Nid, Vec<(String, Toks)>, Vec<String>);

/// The searched fields in scoring order, with their weights.
const FIELDS: [(&str, f64); 3] = [("title", 3.0), ("abstract", 2.0), ("body", 1.0)];

/// The tokens of one field of a live node; a dropped or absent body has none ([F09 §12.1]).
fn field_tokens(ev: &Ev<'_>, n: Nid, field: &str) -> (String, Vec<(usize, String)>) {
    let Some(x) = ev.st().live(n) else {
        return (String::new(), Vec::new());
    };
    let text = match field {
        "body" => x.body.clone().unwrap_or_default(),
        f => x.text(f).unwrap_or("").to_string(),
    };
    let t = tokens(&text);
    (text, t)
}

/// Whether a document matches: some positive term in a searched field and no excluded term in any.
// spec: [LQ/std §2.9] search
fn matches(terms: &[Term], fields: &[(String, Vec<(usize, String)>)]) -> bool {
    let positive = terms
        .iter()
        .filter(|t| !t.exclude)
        .any(|t| fields.iter().any(|(_, f)| t.hits(f) > 0));
    let excluded = terms
        .iter()
        .filter(|t| t.exclude)
        .any(|t| fields.iter().any(|(_, f)| t.hits(f) > 0));
    positive && !excluded
}

/// `text_match(n, terms)` ([50 §2.6]): the `search()` matcher over the title and the abstract.
// spec: [50 §2.6] text_match
pub fn text_match(ev: &Ev<'_>, n: Nid, q: &str) -> bool {
    let terms = query_terms(q);
    let fields: Vec<(String, Vec<(usize, String)>)> = ["title", "abstract"]
        .iter()
        .map(|f| (f.to_string(), field_tokens(ev, n, f).1))
        .collect();
    matches(&terms, &fields)
}

/// A snippet of a field around its first matching token: at most 80 characters, cut marks `...` where text is left out.
// spec: [LQ/std §2.9] search
fn snippet(text: &str, toks: &[(usize, String)], terms: &[Term]) -> String {
    let at = toks
        .iter()
        .find(|(_, t)| {
            terms.iter().any(|q| {
                !q.exclude
                    && if q.prefix {
                        t.starts_with(&q.text)
                    } else {
                        *t == q.text
                    }
            })
        })
        .map_or(0, |(i, _)| *i);
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    if chars.len() <= 80 {
        return text.to_string();
    }
    let pos = chars.iter().position(|(i, _)| *i >= at).unwrap_or(0);
    let from = pos.saturating_sub(20);
    let to = (from + 80).min(chars.len());
    let mut s: String = chars[from..to].iter().map(|(_, c)| *c).collect();
    if from > 0 {
        s = format!("...{s}");
    }
    if to < chars.len() {
        s.push_str("...");
    }
    s
}

/// `search(terms, kinds, fields)` ([LQ/std §2.9]; [50 §5.5]): the matching live nodes of the view with their score,
/// best field and snippet, ranked by score (BM25, or the statistics-free scorer under the ablation).
// spec: [50 §5.5]
pub fn search(ev: &Ev<'_>, q: &str, kinds: Option<&[String]>, fields: &[String]) -> Vec<Vec<V>> {
    let terms = query_terms(q);
    let searched: Vec<(&str, f64)> = FIELDS
        .iter()
        .copied()
        .filter(|(f, _)| fields.iter().any(|x| x == f))
        .collect();
    let live: Vec<Nid> = ev
        .st()
        .nodes
        .iter()
        .filter(|(_, x)| x.live())
        .map(|(n, _)| *n)
        .collect();
    // Every live document's tokens of the searched fields: the view's statistics ([50 §5.5]).
    let docs: Vec<Doc> = live
        .iter()
        .map(|n| {
            let mut texts = Vec::new();
            let f = searched
                .iter()
                .map(|(f, _)| {
                    let (text, t) = field_tokens(ev, *n, f);
                    texts.push(text);
                    (f.to_string(), t)
                })
                .collect();
            (*n, f, texts)
        })
        .collect();
    let n_docs = docs.len() as f64;
    let mut stats = Vec::new();
    for (i, _) in searched.iter().enumerate() {
        let with: Vec<&Vec<(usize, String)>> = docs.iter().map(|d| &d.1[i].1).collect();
        let nonempty = with.iter().filter(|t| !t.is_empty()).count() as f64;
        let total: usize = with.iter().map(|t| t.len()).sum();
        stats.push(if nonempty > 0.0 {
            total as f64 / nonempty
        } else {
            0.0
        });
    }
    let df: Vec<f64> = terms
        .iter()
        .map(|t| {
            docs.iter()
                .filter(|d| d.1.iter().any(|(_, f)| t.hits(f) > 0))
                .count() as f64
        })
        .collect();
    let (k1, b) = (1.2, 0.75);
    let mut scored: Vec<(f64, i64, Nid, String, String)> = Vec::new();
    for (n, fs, texts) in &docs {
        if kinds.is_some_and(|ks| {
            ev.node(*n)
                .is_none_or(|x| !ks.iter().any(|k| k.eq_ignore_ascii_case(&x.kind)))
        }) {
            continue;
        }
        if !matches(&terms, fs) {
            continue;
        }
        let mut per_field = vec![0.0f64; searched.len()];
        for (ti, t) in terms.iter().enumerate().filter(|(_, t)| !t.exclude) {
            for (fi, (_, w)) in searched.iter().enumerate() {
                let tf = t.hits(&fs[fi].1) as f64;
                if tf == 0.0 {
                    continue;
                }
                per_field[fi] += if ev.w.ab.stat_free {
                    *w
                } else {
                    let idf = (1.0 + (n_docs - df[ti] + 0.5) / (df[ti] + 0.5)).ln();
                    let len = fs[fi].1.len() as f64;
                    let norm = if stats[fi] > 0.0 {
                        len / stats[fi]
                    } else {
                        1.0
                    };
                    w * idf * tf * (k1 + 1.0) / (tf + k1 * (1.0 - b + b * norm))
                };
            }
        }
        let score: f64 = per_field.iter().sum();
        let best = (0..searched.len())
            .max_by(|a, c| per_field[*a].total_cmp(&per_field[*c]).then(c.cmp(a)))
            .unwrap_or(0);
        let recency = ev.local(*n).2 as i64;
        scored.push((
            score,
            recency,
            *n,
            searched
                .get(best)
                .map_or(String::new(), |f| f.0.to_string()),
            snippet(&texts[best], &fs[best].1, &terms),
        ));
    }
    let stat_free = ev.w.ab.stat_free;
    scored.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| {
                if stat_free {
                    b.1.cmp(&a.1)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then(a.2.cmp(&b.2))
    });
    scored
        .into_iter()
        .map(|(s, _, n, f, sn)| {
            vec![
                V::Node(n),
                V::Float(val::round_half_even(s, 3)),
                V::text(f),
                V::text(sn),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizer_v1_lowercases_folds_yo_and_splits_on_non_words() {
        let t: Vec<String> = tokens("Lease-reclaim ЁЛКА x2 naïve")
            .into_iter()
            .map(|t| t.1)
            .collect();
        assert_eq!(t, ["lease", "reclaim", "елка", "x2", "naïve"]);
        let long = "a".repeat(70);
        assert_eq!(tokens(&long)[0].1.len(), 64);
    }

    #[test]
    fn query_terms_take_prefixes_and_exclusions() {
        let q = query_terms("leas* -fencing reclaim");
        assert!(q[0].prefix && !q[0].exclude);
        assert!(q[1].exclude);
        assert_eq!(q[2].text, "reclaim");
    }
}
