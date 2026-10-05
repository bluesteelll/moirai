//! Idempotency ([API §7]; [F17 §11.1]; I14′): which commands are keyed, the key hash of an explicit and of a default
//! key, the payload of a command that compiles to no `TX` block (with its canonical JSON, [API §5.6]), the lookup
//! with its two windows under the injected clock, and what a replay rebuilds its result from ([API §7.5], §17.1).

use crate::clock::Hlc;
use crate::err::{Kv, Refusal};
use crate::value::{blake3_128, hex};
use std::collections::BTreeMap;

/// A canonical-JSON value ([API §5.6]): objects keep their members in the order written (`Obj`) or in bytewise key
/// order (`Map`, for maps whose keys are data).
#[derive(Clone, PartialEq, Debug)]
pub enum Cj {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// An integer (§5.1: a string of digits at or beyond 2^53).
    Int(i64),
    /// A finite f64 ([API §5.2]).
    F64(f64),
    /// A string.
    Str(String),
    /// An array in the given order.
    Arr(Vec<Cj>),
    /// An object whose members are in the owning table's order.
    Obj(Vec<(String, Cj)>),
    /// An object whose keys are data, written in bytewise key order (CJ rule 2).
    Map(BTreeMap<String, Cj>),
}

/// Writes a string by [F19 §8.3]: `"`, `\`, LF, CR and TAB by their escapes, every other U+0000–U+001F as `\u00xx`.
fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The shortest round-trip form of a finite f64, with a `.` or an exponent; −0.0 is +0.0 ([API §5.2]; [F08 §5.3]).
fn f64_text(x: f64) -> String {
    let x = if x == 0.0 { 0.0 } else { x };
    let s = format!("{x:?}");
    if s.contains(['.', 'e', 'E']) {
        s
    } else {
        format!("{s}.0")
    }
}

impl Cj {
    /// The canonical JSON text ([API §5.6] rules 1–5).
    // spec: [API §5.6]
    pub fn text(&self) -> String {
        let mut s = String::new();
        self.write(&mut s);
        s
    }

    fn write(&self, out: &mut String) {
        match self {
            Cj::Null => out.push_str("null"),
            Cj::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Cj::Int(i) => {
                if i.unsigned_abs() >= 1 << 53 {
                    write_str(out, &i.to_string());
                } else {
                    out.push_str(&i.to_string());
                }
            }
            Cj::F64(x) => out.push_str(&f64_text(*x)),
            Cj::Str(s) => write_str(out, s),
            Cj::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    x.write(out);
                }
                out.push(']');
            }
            Cj::Obj(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
            Cj::Map(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_str(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

/// An explicit key's hash: `BLAKE3-128(lp("moirai-idem-key-v1") ‖ lp(k))` ([API §7.2]; [F06 §4.4.7]).
// spec: [API §7.2]
pub fn explicit_key(k: &str) -> [u8; 16] {
    blake3_128(&[b"moirai-idem-key-v1", k.as_bytes()])
}

/// The default key: `BLAKE3-128(lp("moirai-idem-default-v1") ‖ lp(s) ‖ lp(a) ‖ lp(b) ‖ lp(P))` over the resolved
/// session `s`, the attested thread or agent `a`, the name `b` of the command's branch and the payload `P` ([API §7.2];
/// [F06 §4.4.7]): one command on two branches within the default window has two keys.
// spec: [API §7.2]
pub fn default_key(session: &str, agent: &str, branch: &str, payload: &[u8; 16]) -> [u8; 16] {
    blake3_128(&[
        b"moirai-idem-default-v1",
        session.as_bytes(),
        agent.as_bytes(),
        branch.as_bytes(),
        payload,
    ])
}

/// The payload of a keyed command that compiles to no `TX` block: `BLAKE3-128(lp("moirai-api-payload-v1") ‖ lp(name) ‖
/// lp(CJ(args′)))` ([API §7.3]); `args` is `args′` already (uids for node ids, full commit ids, omitted defaults
/// omitted, keys bytewise).
// spec: [API §7.3]
pub fn payload(name: &str, args: &BTreeMap<String, Cj>) -> [u8; 16] {
    let cj = Cj::Map(args.clone()).text();
    blake3_128(&[b"moirai-api-payload-v1", name.as_bytes(), cj.as_bytes()])
}

/// One item a replay rebuilds a family-W result from ([API §7.5]): the items of an `IdemResult` section
/// ([API §17.1]) — of the `Idem` record of a command that appends no commit, or of the `Lease` records in the group
/// of the commit that carries the result.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ResultItem {
    /// `rtype` 2 `lease-end`: the lease and its release reason ([F05 §9.4] field 18).
    LeaseEnd {
        /// The lease.
        lease: u64,
        /// The release reason.
        reason: u8,
    },
    /// `rtype` 3 `ref-move`: the ref, the reason of [F05 §9.2] field 1 (1 create, 2 delete), the old and new tips.
    RefMove {
        /// The ref id.
        ref_id: u32,
        /// The reason.
        reason: u8,
        /// The tip before, by seq.
        old: Option<u64>,
        /// The tip after, by seq.
        new: Option<u64>,
    },
}

/// One `IDEM` entry ([F11 §8]; [API §15.7] `idem`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// `idem_payload`.
    pub payload: [u8; 16],
    /// The branch the command wrote: its ref id and name.
    pub ref_id: u32,
    /// The branch name at the time.
    pub branch: String,
    /// The commit that carries the result, with its `ref_seq`, or `None` (an `Idem` record).
    pub commit: Option<(u64, u64)>,
    /// Created under a default key.
    pub default_key: bool,
    /// The `append_hlc` that opens the window.
    pub append_hlc: u64,
    /// The explicit key text, when there was one.
    pub key_text: Option<String>,
    /// The recorded result the replay rebuilds its data from ([API §17.1]).
    pub result: Recorded,
}

/// What a replay rebuilds its result from ([API §7.5]): the kind of command, the items of its `IdemResult` or of its
/// group's `Lease` records, and, for family T, the yield rows those records give.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Recorded {
    /// The command name.
    pub cmd: String,
    /// The `IdemResult` items in the order the result lists them.
    pub items: Vec<ResultItem>,
    /// The yields of a family-T result, as its group's `Lease` records give them ([API §7.5]); the members a section
    /// marks "not replayed" hold their replay value.
    pub yields: Vec<crate::tx::Yield>,
}

/// The decision of one lookup ([API §7.4]).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Lookup {
    /// Row 1: no usable entry; the command executes.
    Execute,
    /// Rows 2 and 3: replay the recorded result.
    Replay(Box<Entry>),
    /// Rows 4 and 5: E408.
    Mismatch(Box<Refusal>),
}

/// The `IDEM` table of a store.
#[derive(Clone, Debug, Default)]
pub struct Table {
    /// Entries by key hash.
    pub entries: BTreeMap<[u8; 16], Entry>,
}

/// The two windows of [F17 §11.1]: `idempotency.retention` and `idempotency.default-window`, in ms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Windows {
    /// P28.
    pub retention_ms: u64,
    /// P29.
    pub default_ms: u64,
}

impl Default for Windows {
    /// 30 d and 10 min ([CFG §10.2] P28, P29).
    fn default() -> Windows {
        Windows {
            retention_ms: 30 * 86_400_000,
            default_ms: 600_000,
        }
    }
}

impl Windows {
    /// The window of an entry: the retention, or for a default key the default window within it.
    pub fn of(self, e: &Entry) -> u64 {
        if e.default_key {
            self.default_ms.min(self.retention_ms)
        } else {
            self.retention_ms
        }
    }
}

impl Table {
    /// The lookup of [API §7.4] ([F17 §11.1]; I14′): entries older than the retention, or, for a default key, the
    /// default window, are ignored (CK-6); then the first matching row decides. `ref_id` is the ref of the branch the
    /// command writes; `absorbed` answers, when row 3 needs it, whether that branch has absorbed (`ref_id`, `ref_seq`)
    /// ([F11 §8]).
    // spec: [F17 §11.1]
    // spec: [F13 §3.4] I14′
    #[allow(clippy::too_many_arguments)]
    pub fn lookup(
        &self,
        key: &[u8; 16],
        payload: &[u8; 16],
        ref_id: Option<u32>,
        wall_ms: i64,
        hlc: &Hlc,
        w: Windows,
        absorbed: &dyn Fn(u32, u64) -> bool,
    ) -> Lookup {
        let Some(e) = self.entries.get(key) else {
            return Lookup::Execute;
        };
        if !hlc.within(wall_ms, e.append_hlc, w.of(e)) {
            return Lookup::Execute;
        }
        let original = match e.commit {
            Some((seq, _)) => Kv::Obj(vec![
                ("rev".into(), Kv::Int(seq as i64)),
                ("commit".into(), Kv::Commit(seq)),
                ("ref".into(), Kv::Str(e.branch.clone())),
            ]),
            None => Kv::Null,
        };
        let mismatch = |why: &str| {
            Lookup::Mismatch(Box::new(
                Refusal::lq("E408", why.to_string())
                    .key("key", e.key_text.clone())
                    .key("original", original.clone()),
            ))
        };
        if &e.payload != payload {
            return mismatch("the key was used for a different payload");
        }
        if ref_id == Some(e.ref_id) {
            return Lookup::Replay(Box::new(e.clone()));
        }
        if let Some((_, rs)) = e.commit
            && absorbed(e.ref_id, rs)
        {
            return Lookup::Replay(Box::new(e.clone()));
        }
        mismatch("the key was used on another branch")
    }

    /// Records an entry for an outcome that appended records ([API §7.4] "What is recorded").
    pub fn record(&mut self, key: [u8; 16], e: Entry) {
        self.entries.insert(key, e);
    }

    /// The entries within their windows, by key ([API §15.7] `idem`).
    pub fn live(&self, wall_ms: i64, hlc: &Hlc, w: Windows) -> Vec<([u8; 16], &Entry)> {
        self.entries
            .iter()
            .filter(|(_, e)| hlc.within(wall_ms, e.append_hlc, w.of(e)))
            .map(|(k, e)| (*k, e))
            .collect()
    }
}

/// The hex form of a key or payload in results.
pub fn key_hex(k: &[u8; 16]) -> String {
    hex(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_json_escapes_and_orders() {
        let mut m = BTreeMap::new();
        m.insert("b".to_string(), Cj::Str("x\"\n\u{1}".into()));
        m.insert("a".to_string(), Cj::Int(1 << 53));
        m.insert("c".to_string(), Cj::F64(1.0));
        m.insert("d".to_string(), Cj::F64(-0.0));
        assert_eq!(
            Cj::Map(m).text(),
            "{\"a\":\"9007199254740992\",\"b\":\"x\\\"\\n\\u0001\",\"c\":1.0,\"d\":0.0}"
        );
        assert_eq!(Cj::F64(1e300).text(), "1e300");
    }

    #[test]
    fn lookup_rows() {
        let mut t = Table::default();
        let mut h = Hlc::default();
        let at = h.commit(1_000_000);
        let k = explicit_key("k");
        let p = [1u8; 16];
        t.record(
            k,
            Entry {
                payload: p,
                ref_id: 0,
                branch: "main".into(),
                commit: Some((1, 1)),
                default_key: false,
                append_hlc: at,
                key_text: Some("k".into()),
                result: Recorded::default(),
            },
        );
        let w = Windows {
            retention_ms: 3_600_000,
            default_ms: 60_000,
        };
        let no = |_: u32, _: u64| false;
        let yes = |_: u32, _: u64| true;
        assert!(
            matches!(
                t.lookup(&k, &p, Some(0), 1_000_000, &h, w, &no),
                Lookup::Replay(_)
            ),
            "row 2"
        );
        assert!(
            matches!(
                t.lookup(&k, &p, Some(1), 1_000_000, &h, w, &yes),
                Lookup::Replay(_)
            ),
            "row 3"
        );
        match t.lookup(&k, &[2; 16], Some(0), 1_000_000, &h, w, &no) {
            Lookup::Mismatch(e) => {
                assert_eq!((e.code.as_str(), e.exit), ("E408", 9));
                assert_eq!(e.key_names(), vec!["key", "original"]);
                let o = e.get("original").unwrap();
                assert_eq!(o.member("commit"), Some(&Kv::Commit(1)));
                assert_eq!(o.member("rev"), Some(&Kv::Int(1)));
                assert_eq!(o.member("ref"), Some(&Kv::Str("main".into())));
            }
            other => panic!("row 4: {other:?}"),
        }
        assert!(
            matches!(
                t.lookup(&k, &p, Some(1), 1_000_000, &h, w, &no),
                Lookup::Mismatch(_)
            ),
            "row 5"
        );
        assert_eq!(
            t.lookup(&k, &p, Some(0), 1_000_000 + 3_600_000, &h, w, &no),
            Lookup::Execute,
            "retention"
        );
        assert_eq!(
            t.lookup(&[9; 16], &p, Some(0), 1_000_000, &h, w, &no),
            Lookup::Execute,
            "row 1"
        );
    }
}
