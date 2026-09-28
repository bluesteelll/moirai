//! The binding context of [LQ/canonical-ast §5.1]: what the binder is given and the encoder never computes. The schema
//! of the view ([`crate::lq::schema::Schema`]), the store's identity maps ([`Identities`]), the bound parameter values
//! ([`Params`]) and the caller ([`Caller`]: role, surface, model profile, branch, `--at`, tree, safelists and the
//! `refs` budget).

use crate::lq::cast::{CommitId, Uid};
use crate::lq::printer::Spelling;
use crate::lq::schema::Schema;
use std::collections::BTreeMap;

/// The store's identity maps ([LQ/canonical-ast §5.1] item 2): `#N` → uid, uid → `#N`, sequence number or commit
/// prefix → full commit id, and the kind of a live node of the view.
pub trait Identities: Sync {
    /// The uid of `#N`, if the store ever allocated it.
    fn uid(&self, n: u32) -> Option<Uid>;
    /// The `#N` of a uid the store knows.
    fn nid(&self, uid: &Uid) -> Option<u32>;
    /// `HEAD.next_id`: the next `#N` the store would allocate.
    fn next_id(&self) -> u32;
    /// The kind name of the node with this uid when it is live in the view.
    fn kind_of(&self, uid: &Uid) -> Option<&str>;
    /// The commit with this store sequence number.
    fn commit_by_seq(&self, seq: u64) -> Option<CommitId>;
    /// Every commit whose id starts with these lower-case hex digits: (id, sequence number, ref of its header).
    fn commits_by_prefix(&self, hex: &str) -> Vec<(CommitId, u64, String)>;
}

/// An in-memory [`Identities`].
#[derive(Clone, Debug, Default)]
pub struct MapIds {
    nodes: BTreeMap<u32, (Uid, Option<String>)>,
    by_uid: BTreeMap<Uid, u32>,
    commits: BTreeMap<u64, (CommitId, String)>,
    next: u32,
}

impl MapIds {
    /// No nodes and no commits; `next_id` is 1.
    pub fn new() -> MapIds {
        MapIds {
            next: 1,
            ..MapIds::default()
        }
    }

    /// Records `#N` with its uid and, when live in the view, its kind.
    pub fn node(&mut self, n: u32, uid: Uid, kind: Option<&str>) -> &mut MapIds {
        self.nodes.insert(n, (uid, kind.map(str::to_string)));
        self.by_uid.insert(uid, n);
        self.next = self.next.max(n + 1);
        self
    }

    /// Records a commit with its sequence number and the ref of its header.
    pub fn commit(&mut self, seq: u64, id: CommitId, r: &str) -> &mut MapIds {
        self.commits.insert(seq, (id, r.to_string()));
        self
    }

    /// Sets `next_id`.
    pub fn next_id_at(&mut self, n: u32) -> &mut MapIds {
        self.next = n;
        self
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Identities for MapIds {
    fn uid(&self, n: u32) -> Option<Uid> {
        self.nodes.get(&n).map(|(u, _)| *u)
    }
    fn nid(&self, uid: &Uid) -> Option<u32> {
        self.by_uid.get(uid).copied()
    }
    fn next_id(&self) -> u32 {
        self.next
    }
    fn kind_of(&self, uid: &Uid) -> Option<&str> {
        let n = self.by_uid.get(uid)?;
        self.nodes.get(n)?.1.as_deref()
    }
    fn commit_by_seq(&self, seq: u64) -> Option<CommitId> {
        self.commits.get(&seq).map(|(c, _)| *c)
    }
    fn commits_by_prefix(&self, prefix: &str) -> Vec<(CommitId, u64, String)> {
        self.commits
            .iter()
            .filter(|(_, (c, _))| hex(c).starts_with(prefix))
            .map(|(s, (c, r))| (*c, *s, r.clone()))
            .collect()
    }
}

/// The door a call came through ([RULES/role-write-policy] `surface`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// The CLI (`moirai q`, `moirai tx`, verbs).
    Cli,
    /// MCP (`query`, `write`).
    Mcp,
}

/// The caller's model profile ([90 §8.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// `gated`.
    Gated,
    /// `compatible`.
    Compatible,
    /// `unknown`.
    Unknown,
}

/// The caller of one bind ([LQ/canonical-ast §5.9]; [RULES/role-write-policy] WT-001, `role-reads`,
/// `role-statements`).
#[derive(Clone, Debug)]
pub struct Caller {
    /// The effective role (`orchestrator`, `owner`, `developer`, `tester`, `general-purpose`, ...).
    pub role: String,
    /// The door.
    pub surface: Surface,
    /// The model profile.
    pub profile: Profile,
    /// The caller's resolved branch: the view of a part without `USE` and the branch of a `TX` without `ON`.
    pub branch: String,
    /// `--at REV` or the MCP `use` parameter: the `USE` of every part that has none ([LQ/canonical-ast §5.6]).
    pub at: Option<String>,
    /// A tree resolves for tree-derived state ([50 §3.8]).
    pub tree: bool,
    /// `query.safelist.<role>` = `named-only` ([RULES/role-write-policy] WQ-003).
    pub named_only: bool,
    /// `query.safelist.model.unknown` = `dry-targets` ([RULES/role-write-policy] WQ-005).
    pub unknown_dry_targets: bool,
    /// The `refs` budget: the most views one query may read ([50 §3.9] item 8, [50 §5.10]).
    pub refs: u32,
    /// The display spelling of quantifiers in the reading echo (`HOLE(LQ-display-spelling)`, [LQ/gql-spelling §4]).
    pub display: Spelling,
}

impl Default for Caller {
    /// The orchestrator on the CLI, `compatible`, on `main`, with a tree and the default `refs` budget of 4.
    fn default() -> Caller {
        Caller {
            role: "orchestrator".into(),
            surface: Surface::Cli,
            profile: Profile::Compatible,
            branch: "main".into(),
            at: None,
            tree: true,
            named_only: false,
            unknown_dry_targets: false,
            refs: 4,
            display: Spelling::default(),
        }
    }
}

/// A bound parameter value: a JSON value, or a `k=v` text that the use site's type converts ([LQ/std §2.2]).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// JSON `null`: an absent value.
    Null,
    /// A boolean.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A float.
    Float(f64),
    /// A text (argv `k=v`, MCP `"k=v"`, or a JSON string).
    Text(String),
    /// A JSON array.
    List(Vec<Value>),
}

/// The bound parameter values of a call, by name without `$` ([LQ/canonical-ast §5.1] item 3).
#[derive(Clone, Debug, Default)]
pub struct Params(pub BTreeMap<String, Value>);

impl Params {
    /// No parameters.
    pub fn new() -> Params {
        Params::default()
    }

    /// Adds one parameter.
    pub fn with(mut self, name: &str, v: Value) -> Params {
        self.0.insert(name.to_string(), v);
        self
    }

    /// The value of a parameter.
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.0.get(name)
    }
}

/// Everything one bind reads.
#[derive(Clone, Copy)]
pub struct BindCtx<'a> {
    /// The effective schema of the view.
    pub schema: &'a Schema,
    /// The identity maps.
    pub ids: &'a dyn Identities,
    /// The bound parameter values.
    pub params: &'a Params,
    /// The caller.
    pub caller: &'a Caller,
}
