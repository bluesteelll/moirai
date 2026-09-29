//! [RULES/policy-keys] as data: every key pattern of [CFG §10] and every policy-data row of [CFG §10.13] with the model
//! function that implements it, the allowed values it is tested at ([CFG §9.5]) and its checker ([CFG §9.4]). The
//! table is checked against the registry ([`crate::registry`]) in both directions, and every value of every row is
//! tested: through its function, or by the invariance of the model's reference stream ([RULES/policy-keys] §2).

use crate::config::{self, Parsed};
use crate::registry::{self, EMPTY, UNSET};
use crate::rules::{Row, rules};

/// One `policy-keys` row.
#[derive(Clone, Debug)]
pub struct KeyRow {
    /// The row id.
    pub id: String,
    /// `key`: the pattern.
    pub key: String,
    /// `instance`.
    pub instance: String,
    /// `vis`.
    pub vis: String,
    /// `function`.
    pub function: String,
    /// `values`.
    pub values: Vec<String>,
    /// `checker`: the checker of the class `vis`, then the checker of a second class the key has for some callers
    /// (KY-011: I, and Rs for agent verbs).
    pub checker: Vec<String>,
}

fn key_row(r: &Row) -> KeyRow {
    KeyRow {
        id: r.id.clone(),
        key: r.tok("key").to_string(),
        instance: r.tok("instance").to_string(),
        vis: r.tok("vis").to_string(),
        function: r.tok("function").to_string(),
        values: r.toks("values").iter().map(|v| v.to_string()).collect(),
        checker: r.toks("checker").iter().map(|c| c.to_string()).collect(),
    }
}

/// The `policy-keys` rows ([RULES/policy-keys] §5).
// spec: [RULES/policy-keys] policy-keys
pub fn key_rows() -> Vec<KeyRow> {
    rules()
        .table("policy-keys")
        .rows
        .iter()
        .map(key_row)
        .collect()
}

/// The `policy-rows` rows ([RULES/policy-keys] §6): (id, row name, instance, function, values).
// spec: [RULES/policy-keys] policy-rows
pub fn policy_rows() -> Vec<(String, String, String, String, Vec<String>)> {
    rules()
        .table("policy-rows")
        .rows
        .iter()
        .map(|r| {
            (
                r.id.clone(),
                r.tok("row_name").to_string(),
                r.tok("instance").to_string(),
                r.tok("function").to_string(),
                r.toks("values").iter().map(|v| v.to_string()).collect(),
            )
        })
        .collect()
}

/// The work package that builds a function ([RULES/policy-keys] `key-functions` `wp`).
pub fn wp_of(function: &str) -> &'static str {
    rules()
        .table("key-functions")
        .rows
        .iter()
        .find(|r| r.tok("function") == function)
        .map(|r| r.tok("wp"))
        .unwrap_or_else(|| panic!("key-functions has no {function}"))
}

/// The text a value token stands for: [`EMPTY`] is the empty value; [`UNSET`] has none.
pub fn value_text(v: &str) -> Option<&str> {
    match v {
        UNSET => None,
        EMPTY => Some(""),
        x => Some(x),
    }
}

/// The companion values that make a value reachable when it breaks a constraint alone ([RULES/policy-keys] §2;
/// [CFG §5.3] fallback): C-1 (`P05 ≤ P01 / 8`), C-2 (`P09 ≤ P10`), C-3 (`P29 ≤ P28`) and K-1 (the quota sums).
pub fn companions(key: &str, value: &str) -> Vec<(String, String)> {
    let n = |k: &str, v: &str| -> u64 {
        registry::find(k)
            .and_then(|(d, _)| config::parse(d.ty, v))
            .and_then(|p| p.number)
            .unwrap_or(0)
    };
    let dflt = |k: &str| {
        registry::default_parsed(k, registry::Proc::Cli)
            .and_then(|p| p.number)
            .unwrap_or(0)
    };
    let mut out = Vec::new();
    match key {
        "store.log-extent-bytes" => {
            let p01 = n(key, value);
            if dflt("store.commit.inline-max-bytes") > p01 / 8 {
                out.push((
                    "store.commit.inline-max-bytes".into(),
                    config::size_canon(p01 / 8),
                ));
            }
        }
        "store.commit.inline-max-bytes" => {
            let p05 = n(key, value);
            if p05 > dflt("store.log-extent-bytes") / 8 {
                out.push((
                    "store.log-extent-bytes".into(),
                    config::size_canon((p05 * 8).next_power_of_two()),
                ));
            }
        }
        "store.tail.max-overlay-bytes" => {
            if n(key, value) > dflt("store.tail.max-overlay-bytes.quiet") {
                out.push((
                    "store.tail.max-overlay-bytes.quiet".into(),
                    value.to_string(),
                ));
            }
        }
        "store.tail.max-overlay-bytes.quiet" => {
            if n(key, value) < dflt("store.tail.max-overlay-bytes") {
                out.push(("store.tail.max-overlay-bytes".into(), value.to_string()));
            }
        }
        "idempotency.retention" => {
            if n(key, value) < dflt("idempotency.default-window") {
                out.push(("idempotency.default-window".into(), value.to_string()));
            }
        }
        "idempotency.default-window" => {
            if n(key, value) > dflt("idempotency.retention") {
                out.push(("idempotency.retention".into(), value.to_string()));
            }
        }
        k if k.starts_with("pack.quota.") => {
            let v = n(key, value);
            let others = [
                "pack.quota.c2",
                "pack.quota.c3",
                "pack.quota.c4-dev",
                "pack.quota.c4-critic",
                "pack.quota.c5",
            ];
            let sum_ok = |set: &[(&str, u64)]| {
                let q = |x: &str| {
                    set.iter()
                        .find(|(k, _)| *k == x)
                        .map_or(dflt(x), |(_, v)| *v)
                };
                let base = q("pack.quota.c2") + q("pack.quota.c3") + q("pack.quota.c5");
                base + q("pack.quota.c4-dev") <= 100 && base + q("pack.quota.c4-critic") <= 100
            };
            if !sum_ok(&[(k, v)]) {
                for o in others.iter().filter(|o| **o != k) {
                    out.push((o.to_string(), "0".to_string()));
                }
            }
        }
        _ => {}
    }
    out
}

/// How a test sets a key instance to a value token: `Init` params for store keys and `ConfigSet`s for user keys, each
/// with its companions; both lists are empty for [`UNSET`], which sets nothing.
pub fn settings(key: &str, value: &str) -> (Vec<String>, Vec<(String, String)>) {
    let Some(text) = value_text(value) else {
        return (Vec::new(), Vec::new());
    };
    assert!(registry::find(key).is_some(), "{key} is not registered");
    let mut init = Vec::new();
    let mut user = Vec::new();
    for (k, v) in std::iter::once((key.to_string(), text.to_string())).chain(companions(key, text))
    {
        let (dk, _) = registry::find(&k).expect("a registered companion");
        if dk.scope.is_user() {
            user.push((k, v));
        } else {
            init.push(format!("{k}={v}"));
        }
    }
    (init, user)
}

/// The parsed value of a token for a key instance (`None` for [`UNSET`]).
pub fn parsed(key: &str, value: &str) -> Option<Parsed> {
    let (d, _) = registry::find(key)?;
    registry::validate(d, key, value_text(value)?)
}

#[cfg(test)]
mod tests;
