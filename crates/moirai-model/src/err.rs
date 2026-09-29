//! Refusals: the error code, its exit code and its code-specific keys ([F19 §10.2], §10.3; [LQ/errors §5]). The
//! message, help and detail texts are outside the comparison of model and engine ([API §16.3]); the model keeps one
//! line of detail for its own tests and triage.
//!
//! The code-specific keys are typed values ([`Kv`]) in the order of [LQ/errors §5.7] and [F19 §10.3]; the testkit
//! writes them as JSON (the model has no JSON code, [PLAN §3.2] item 9). A commit is named by its store sequence number
//! until WP-91's canonical encoder gives it its id ([`Kv::Commit`]).

use crate::value::Nid;
use std::fmt;

/// A typed value of a code-specific key ([LQ/errors §5.7]; [F19 §10.3]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kv {
    /// `null`.
    Null,
    /// A bool.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A string.
    Str(String),
    /// A commit, by its store sequence number; written as its id ([LQ/envelope §7.3]).
    Commit(u64),
    /// A node, written `"#N"`.
    Node(Nid),
    /// An array.
    List(Vec<Kv>),
    /// An object with its members in order.
    Obj(Vec<(String, Kv)>),
}

impl Kv {
    /// The string of a [`Kv::Str`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Kv::Str(s) => Some(s),
            _ => None,
        }
    }

    /// A member of a [`Kv::Obj`].
    pub fn member(&self, k: &str) -> Option<&Kv> {
        match self {
            Kv::Obj(m) => m.iter().find(|(n, _)| n == k).map(|(_, v)| v),
            _ => None,
        }
    }
}

impl From<&str> for Kv {
    fn from(s: &str) -> Kv {
        Kv::Str(s.to_string())
    }
}

impl From<String> for Kv {
    fn from(s: String) -> Kv {
        Kv::Str(s)
    }
}

impl From<&String> for Kv {
    fn from(s: &String) -> Kv {
        Kv::Str(s.clone())
    }
}

impl From<bool> for Kv {
    fn from(b: bool) -> Kv {
        Kv::Bool(b)
    }
}

impl From<i64> for Kv {
    fn from(i: i64) -> Kv {
        Kv::Int(i)
    }
}

impl From<Nid> for Kv {
    fn from(n: Nid) -> Kv {
        Kv::Node(n)
    }
}

impl<T: Into<Kv>> From<Option<T>> for Kv {
    fn from(v: Option<T>) -> Kv {
        v.map_or(Kv::Null, Into::into)
    }
}

/// One refusal of a command ([API §2.3] `refused`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// The code: an LQ code (`E404`) or a named code of [F19 §10.2] (`not_found`).
    pub code: String,
    /// The exit code ([F19 §7.1]).
    pub exit: u8,
    /// The code-specific keys of [F19 §10.3] and [LQ/errors §5.7], in their order.
    pub keys: Vec<(String, Kv)>,
    /// One line naming what refused, for tests and triage; never compared.
    pub detail: String,
}

/// Every code of [LQ/errors §5.1] with its name and exit code, transcribed row for row (E008 and E307 are not
/// assigned); a test compares it with the table.
// spec: [LQ/errors §5.1]
pub const LQ_CODES: &[(&str, &str, u8)] = &[
    ("E001", "syntax", 2),
    ("E002", "unterminated", 2),
    ("E003", "bad_literal", 2),
    ("E004", "not_in_lq", 2),
    ("E005", "one_statement", 2),
    ("E006", "read_only", 2),
    ("E007", "expect_required", 2),
    ("E009", "empty_tx", 2),
    ("E101", "unknown_field", 2),
    ("E102", "unknown_value", 2),
    ("E103", "type_mismatch", 2),
    ("E104", "unknown_edge_type", 2),
    ("E105", "unknown_kind", 2),
    ("E106", "edge_direction", 2),
    ("E107", "ambiguous_edge_name", 2),
    ("E108", "unknown_enum_word", 2),
    ("E109", "unknown_function", 2),
    ("E110", "bad_parameter", 2),
    ("E111", "no_such_node", 2),
    ("E112", "aggregate_misuse", 2),
    ("E113", "path_variable", 2),
    ("E114", "bad_quantifier", 2),
    ("E115", "not_writable", 2),
    ("E116", "step_variable_out_of_scope", 2),
    ("E117", "store_local_in_definition", 2),
    ("E118", "null_comparison", 2),
    ("E201", "too_broad", 10),
    ("E202", "unbounded_sort", 10),
    ("E301", "unknown_revision", 3),
    ("E302", "not_at_this_view", 2),
    ("E303", "as_of_too_far", 10),
    ("E304", "too_many_refs", 10),
    ("E305", "read_only_view", 6),
    ("E306", "cursor_mismatch", 2),
    ("E308", "use_in_subquery", 2),
    ("E401", "expect_mismatch", 4),
    ("E402", "tip_moved", 4),
    ("E403", "assert_failed", 6),
    ("E404", "transition_refused", 6),
    ("E405", "invariant", 6),
    ("E406", "role_policy", 6),
    ("E407", "lease", 5),
    ("E408", "idempotency_mismatch", 9),
    ("E409", "restricted_delete", 6),
    ("E410", "ambiguous_bind", 6),
    ("E411", "unknown_model_write", 6),
    ("E501", "work_budget", 10),
    ("E502", "memory_budget", 10),
    ("E503", "deadline", 10),
    ("E504", "cancelled", 10),
    ("E505", "fs_budget", 10),
];

/// The exit code of an LQ code ([LQ/errors §5.1]); a code outside the table is a model bug.
pub fn lq_exit(code: &str) -> u8 {
    LQ_CODES
        .iter()
        .find(|(c, _, _)| *c == code)
        .map(|(_, _, e)| *e)
        .unwrap_or_else(|| panic!("{code} is not a code of [LQ/errors §5.1]"))
}

impl Refusal {
    /// A refusal with a code, its exit code and a detail line.
    pub fn new(code: &str, exit: u8, detail: impl Into<String>) -> Refusal {
        Refusal {
            code: code.to_string(),
            exit,
            keys: Vec::new(),
            detail: detail.into(),
        }
    }

    /// Appends a code-specific key.
    pub fn key(mut self, k: &str, v: impl Into<Kv>) -> Refusal {
        self.keys.push((k.to_string(), v.into()));
        self
    }

    /// Sets a code-specific key: replaces its value where it is present, else appends it.
    pub fn set(mut self, k: &str, v: impl Into<Kv>) -> Refusal {
        let v = v.into();
        match self.keys.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v,
            None => self.keys.push((k.to_string(), v)),
        }
        self
    }

    /// The value of a code-specific key.
    pub fn get(&self, k: &str) -> Option<&Kv> {
        self.keys.iter().find(|(n, _)| n == k).map(|(_, v)| v)
    }

    /// The string value of a code-specific key.
    pub fn get_str(&self, k: &str) -> Option<&str> {
        self.get(k).and_then(Kv::as_str)
    }

    /// The key names in order.
    pub fn key_names(&self) -> Vec<&str> {
        self.keys.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// `usage`, exit 2, with `argument` null ([F19 §10.2], §10.3).
    pub fn usage(detail: impl Into<String>) -> Refusal {
        Refusal::new("usage", 2, detail).key("argument", Kv::Null)
    }

    /// `usage`, exit 2, naming the argument ([F19 §10.3] `argument`).
    pub fn usage_arg(argument: &str, detail: impl Into<String>) -> Refusal {
        Refusal::new("usage", 2, detail).key("argument", argument)
    }

    /// `bad_value`, exit 2.
    pub fn bad_value(case: &str, detail: impl Into<String>) -> Refusal {
        Refusal::new("bad_value", 2, detail).key("case", case)
    }

    /// `not_found`, exit 3, with `what` and `value`.
    pub fn not_found(what: &str, value: impl Into<String>) -> Refusal {
        let v: String = value.into();
        Refusal::new("not_found", 3, format!("{what} {v} does not exist"))
            .key("what", what)
            .key("value", v)
    }

    /// An LQ code with its exit code of [LQ/errors §5.1]; the code-specific keys are set by [`Refusal::finish`] and the
    /// raising site.
    pub fn lq(code: &str, detail: impl Into<String>) -> Refusal {
        let r = Refusal::new(code, lq_exit(code), detail);
        if code == "E301" {
            // The commits a prefix matched ([LQ/errors §5.7]); none for a name that names nothing.
            r.key("candidates", Kv::List(Vec::new()))
        } else {
            r
        }
    }

    /// E407 with its keys `lease`, `holder` and `written` ([LQ/errors §5.7]).
    pub fn e407(
        lease: Option<String>,
        holder: Option<String>,
        detail: impl Into<String>,
    ) -> Refusal {
        Refusal::lq("E407", detail)
            .key("lease", lease)
            .key("holder", holder)
            .key("written", false)
    }

    /// Completes the keys of an E4xx refusal of a write that nothing was written for ([LQ/errors §5.7]): `statement`
    /// first (the 1-based statement index, or null for a refusal of the block as a whole), the raising site's own keys,
    /// then `written: false`. Codes with a fixed key set of their own (E407, E408, E411) and every other code are left
    /// as they are, except that E402 gets `statement: null`.
    // spec: [LQ/errors §5.7]
    pub fn finish(mut self, statement: Option<usize>) -> Refusal {
        let stmt = |s: Option<usize>| s.map_or(Kv::Null, |i| Kv::Int(i as i64));
        match self.code.as_str() {
            "E401" | "E403" | "E404" | "E405" | "E406" | "E409" | "E410" => {
                if self.get("statement").is_none() {
                    self.keys.insert(0, ("statement".into(), stmt(statement)));
                }
                if self.get("written").is_none() {
                    self.keys.push(("written".into(), Kv::Bool(false)));
                }
            }
            "E402" => {
                if self.get("statement").is_none() {
                    self.keys.insert(0, ("statement".into(), Kv::Null));
                }
                if self.get("written").is_none() {
                    self.keys.push(("written".into(), Kv::Bool(false)));
                }
            }
            "E407" => {
                for (k, v) in [("lease", Kv::Null), ("holder", Kv::Null)] {
                    if self.get(k).is_none() {
                        self.keys.push((k.into(), v));
                    }
                }
                if self.get("written").is_none() {
                    self.keys.push(("written".into(), Kv::Bool(false)));
                }
            }
            "E411" => {
                if self.get("mutation").is_none() {
                    self.keys.insert(0, ("mutation".into(), Kv::Null));
                }
                if self.get("written").is_none() {
                    self.keys.push(("written".into(), Kv::Bool(false)));
                }
            }
            _ => {}
        }
        self
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (exit {}): {}", self.code, self.exit, self.detail)
    }
}

/// A result that refuses with a [`Refusal`].
pub type Res<T> = Result<T, Refusal>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The exit column of [LQ/errors §5.1], read from the chapter's own table, equals [`LQ_CODES`] row for row.
    #[test]
    fn the_exit_codes_are_the_tables() {
        let text = include_str!("../../../docs/spec/lq/errors.md");
        let mut seen = Vec::new();
        let mut in_table = false;
        for line in text.lines() {
            if line.starts_with("| Code | Name | Sev. | Exit |") {
                in_table = true;
                continue;
            }
            if in_table {
                if !line.starts_with('|') {
                    break;
                }
                let cells: Vec<&str> = line.split('|').map(str::trim).collect();
                let (code, name, exit) = (cells[1], cells[2].trim_matches('`'), cells[4]);
                if !code.starts_with('E') || name == "—" || exit == "—" {
                    continue;
                }
                seen.push((
                    code.to_string(),
                    name.to_string(),
                    exit.parse::<u8>().unwrap(),
                ));
            }
        }
        let want: Vec<(String, String, u8)> = LQ_CODES
            .iter()
            .map(|(c, n, e)| (c.to_string(), n.to_string(), *e))
            .collect();
        assert_eq!(seen, want);
        assert_eq!(lq_exit("E403"), 6);
        assert_eq!((lq_exit("E304"), lq_exit("E505")), (10, 10));
    }

    #[test]
    fn finish_orders_the_keys() {
        let e = Refusal::lq("E404", "x").finish(Some(2));
        assert_eq!(e.key_names(), vec!["statement", "written"]);
        assert_eq!(e.get("statement"), Some(&Kv::Int(2)));
        let e = Refusal::lq("E401", "x")
            .key("expect", "1")
            .key("matched", 0i64)
            .finish(Some(1));
        assert_eq!(
            e.key_names(),
            vec!["statement", "expect", "matched", "written"]
        );
        let e = Refusal::e407(Some("L-2".into()), None, "lost").finish(None);
        assert_eq!(e.key_names(), vec!["lease", "holder", "written"]);
        assert_eq!(Refusal::usage("x").get("argument"), Some(&Kv::Null));
        assert_eq!(
            Refusal::lq("E301", "no ref x").get("candidates"),
            Some(&Kv::List(Vec::new()))
        );
    }
}
