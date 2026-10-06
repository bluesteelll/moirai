//! The write kernel of the model: one `TX` block's candidate ([API §9.1]; [50 §3.10]; [F13 §2] EP-W4, EP-WD). The
//! data-level statements of [API §9.2] and the coordination procedures of [API §10] run in order on a candidate state
//! and lease table, each op checked by the immediate validators — schema, the plan mask, CAS guards, the lease token,
//! the role write policy, the status machine, the delete policies, forest depth — and the whole block by the deferred
//! validators in I37′ order ([F13 §5]). The net changeset is the diff of the candidate against the view
//! ([AR §4.6]); a refusal writes nothing.

use crate::clock::{Env, after, due_for_renewal};
use crate::coord::{self, Oracle};
use crate::dag::{Dag, RefKind};
use crate::delete::{self, DeleteOpts, EdgePolicies};
use crate::derived::{self, Index};
use crate::err::{Kv, Refusal, Res};
use crate::lease::{self, AnchorKind, EndReason, Lease, LeaseKind};
use crate::lq::ctx::Value as P;
use crate::policy::{Rights, Scopes};
use crate::schema::{Card, EdgeClass, Elem, Schema, Shape, Ty};
use crate::state::{
    Alloc, Aspect, Conflict, Creator, EdgeKey, EdgeProps, KState, KVal, Key, Node, OBSERVATION,
    State,
};
use crate::status::{self, Door, History};
use crate::value::{Algo, F64, Nid, Oid, PathVal, Uid, Value, blake3_128};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};
use std::rc::Rc;

/// A statement target ([API §9.2] "Targets").
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// `#N`.
    Id(Nid),
    /// `#u:<32 hex>`.
    Uid(Uid),
    /// `$<name>` of an earlier `create`.
    Var(String),
}

/// A position among ordered siblings ([API §9.5]).
#[derive(Clone, Debug, PartialEq)]
pub enum Position {
    /// `first`.
    First,
    /// `last`.
    Last,
    /// `{"before": y}`.
    Before(Target),
    /// `{"after": y}`.
    After(Target),
}

/// The guard of a `set` ([API §9.2]; [LQ/std §7.2] `tx.set`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Guard {
    /// `if_rev`.
    pub if_rev: Option<u64>,
    /// `if_status`.
    pub if_status: Option<String>,
    /// `if_holder`.
    pub if_holder: Option<String>,
}

/// What a `resolve` takes ([API §9.2]).
#[derive(Clone, Debug, PartialEq)]
pub enum Take {
    /// `ours`.
    Ours,
    /// `theirs`.
    Theirs,
    /// `base`.
    Base,
    /// `value`.
    Value(P),
    /// `repoint`.
    Repoint(Target),
    /// `drop`: a flagged edge only ([F12 §6.5]; [F06 §7.7] `choice` 5).
    Drop,
}

/// A data-level statement ([API §9.2]).
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    /// `create`.
    Create {
        /// `as`.
        name: Option<String>,
        /// `kind`.
        kind: String,
        /// `fields`, in the order given.
        fields: Vec<(String, P)>,
        /// `body`.
        body: Option<String>,
        /// `under`.
        under: Option<Target>,
        /// `position`.
        position: Option<Position>,
        /// `edges_out`: (kind, dst).
        edges_out: Vec<(String, Target)>,
        /// `edges_in`: (kind, src).
        edges_in: Vec<(String, Target)>,
    },
    /// `set`.
    Set {
        /// `target`.
        target: Target,
        /// `fields`: `P::Null` removes.
        fields: Vec<(String, P)>,
        /// `incr`.
        incr: Vec<(String, i64)>,
        /// `body`: `Some(None)` removes.
        body: Option<Option<String>>,
        /// `guard`.
        guard: Option<Guard>,
    },
    /// `patch`.
    Patch {
        /// `target`.
        target: Target,
        /// `remove`.
        remove: String,
        /// `add`.
        add: String,
    },
    /// `link`.
    Link {
        /// `src`.
        src: Target,
        /// `kind`.
        kind: String,
        /// `dst`.
        dst: Target,
        /// `pinned`: a commit.
        pinned: Option<String>,
    },
    /// `unlink`.
    Unlink {
        /// `src`.
        src: Target,
        /// `kind`.
        kind: String,
        /// `dst`.
        dst: Target,
    },
    /// `move`.
    Move {
        /// `target`.
        target: Target,
        /// `under`, or `None` to detach.
        under: Option<Target>,
        /// `position`.
        position: Option<Position>,
    },
    /// `reopen`.
    Reopen {
        /// `target`.
        target: Target,
        /// `reason`.
        reason: String,
    },
    /// `delete`.
    Delete {
        /// `target`.
        target: Target,
        /// `policy`: `restrict`, `cascade`, `reparent`, `reassign` (`POLICY REASSIGN`, DO-005; spec sync 2b).
        policy: Option<String>,
        /// `replaced_by`.
        replaced_by: Option<Target>,
        /// `release`.
        release: bool,
        /// `reason`.
        reason: Option<String>,
    },
    /// `resolve`.
    Resolve {
        /// The key text ([API §5.3]).
        key: String,
        /// What it takes.
        take: Take,
    },
    /// `call`: a coordination procedure ([API §10]).
    Call {
        /// `tx.claim`, `tx.complete`, `tx.heartbeat`, `tx.release`, `tx.reclaim`.
        proc: String,
        /// The procedure's parameters by name.
        args: Vec<(String, P)>,
    },
}

/// The LQ equivalent of a data-level block ([API §9.3]): its text, one entry per LQ statement, and its parameters.
#[derive(Clone, Debug, Default)]
pub struct Equivalent {
    /// The `TX { … }` text.
    pub text: String,
    /// Each LQ statement's text with the index of the data-level statement it renders.
    pub stmts: Vec<(usize, String)>,
    /// `$p<k>` → value.
    pub params: Vec<(String, P)>,
}

/// The LQ name of a stored edge kind.
fn lq_edge(schema: &Schema, kind: &str) -> String {
    schema
        .edge(kind)
        .map_or_else(|| kind.to_uppercase(), |e| e.lq_name.clone())
}

/// Renders a data-level block as its LQ equivalent ([API §9.3]): every value a parameter `$p<k>` in the order values
/// occur, targets as node literals or variables, each `create` named by its `as` or `v<i>`. A position given with
/// `under` renders as a `MOVE` of the new node (grammar v1's `create_stmt` takes no position).
// spec: [API §9.3]
pub fn equivalent(schema: &Schema, stmts: &[Stmt]) -> Equivalent {
    let mut eq = Equivalent::default();
    let mut unnamed = 0;
    let mut vars: Vec<String> = Vec::new();
    let param = |eq: &mut Equivalent, v: P| -> String {
        let name = format!("p{}", eq.params.len() + 1);
        eq.params.push((name.clone(), v));
        format!("${name}")
    };
    let tgt = |t: &Target| match t {
        Target::Id(n) => format!("#{}", n.0),
        Target::Uid(u) => format!("#u:{}", u.hex()),
        Target::Var(v) => v.clone(),
    };
    let pos = |p: &Option<Position>| match p {
        None => String::new(),
        Some(Position::First) => " FIRST".into(),
        Some(Position::Last) => " LAST".into(),
        Some(Position::Before(t)) => format!(" BEFORE {}", tgt(t)),
        Some(Position::After(t)) => format!(" AFTER {}", tgt(t)),
    };
    for (i, s) in stmts.iter().enumerate() {
        let i1 = i + 1;
        match s {
            Stmt::Create {
                name,
                kind,
                fields,
                body,
                under,
                position,
                edges_out,
                edges_in,
            } => {
                let v = name.clone().unwrap_or_else(|| {
                    unnamed += 1;
                    format!("v{unnamed}")
                });
                vars.push(v.clone());
                let mut props = Vec::new();
                for (f, val) in fields {
                    let p = param(&mut eq, val.clone());
                    props.push(format!("{f}: {p}"));
                }
                let mut text = format!("CREATE ({v}:{kind}");
                if !props.is_empty() {
                    text.push_str(&format!(" {{{}}}", props.join(", ")));
                }
                text.push(')');
                if let Some(u) = under {
                    text.push_str(&format!(" UNDER {}", tgt(u)));
                }
                eq.stmts.push((i1, text));
                if let (Some(u), Some(_)) = (under, position) {
                    eq.stmts
                        .push((i1, format!("MOVE {v} UNDER {}{}", tgt(u), pos(position))));
                }
                for (k, d) in edges_out {
                    eq.stmts.push((
                        i1,
                        format!("CREATE ({v})-[:{}]->({})", lq_edge(schema, k), tgt(d)),
                    ));
                }
                for (k, s) in edges_in {
                    eq.stmts.push((
                        i1,
                        format!("CREATE ({})-[:{}]->({v})", tgt(s), lq_edge(schema, k)),
                    ));
                }
                if let Some(b) = body {
                    let p = param(&mut eq, P::Text(b.clone()));
                    eq.stmts.push((i1, format!("SET {v}.body = {p}")));
                }
            }
            Stmt::Set {
                target,
                fields,
                incr,
                body,
                guard,
            } => {
                let (prefix, t) = match guard {
                    Some(g) => {
                        let mut conds = Vec::new();
                        if let Some(r) = g.if_rev {
                            let p = param(&mut eq, P::Int(r as i64));
                            conds.push(format!("n.rev = {p}"));
                        }
                        if let Some(s) = &g.if_status {
                            let p = param(&mut eq, P::Text(s.clone()));
                            conds.push(format!("n.status = {p}"));
                        }
                        if let Some(h) = &g.if_holder {
                            let p = param(&mut eq, P::Text(h.clone()));
                            conds.push(format!("n.lease.holder = {p}"));
                        }
                        let w = if conds.is_empty() {
                            String::new()
                        } else {
                            format!(" WHERE {}", conds.join(" AND "))
                        };
                        (
                            format!("MATCH (n {{id: {}}}){w} EXPECT 1 ", tgt(target)),
                            "n".to_string(),
                        )
                    }
                    None => (String::new(), tgt(target)),
                };
                let mut sets = Vec::new();
                let mut removes = Vec::new();
                for (f, v) in fields {
                    if *v == P::Null {
                        removes.push(format!("{t}.{f}"));
                    } else {
                        let p = param(&mut eq, v.clone());
                        sets.push(format!("{t}.{f} = {p}"));
                    }
                }
                for (c, d) in incr {
                    let p = param(&mut eq, P::Int(*d));
                    sets.push(format!("{t}.{c} = {t}.{c} + {p}"));
                }
                match body {
                    Some(Some(b)) => {
                        let p = param(&mut eq, P::Text(b.clone()));
                        sets.push(format!("{t}.body = {p}"));
                    }
                    Some(None) => removes.push(format!("{t}.body")),
                    None => {}
                }
                let mut text = prefix;
                if !sets.is_empty() {
                    text.push_str(&format!("SET {}", sets.join(", ")));
                }
                if !removes.is_empty() {
                    if !sets.is_empty() {
                        text.push(' ');
                    }
                    text.push_str(&format!("REMOVE {}", removes.join(", ")));
                }
                eq.stmts.push((i1, text));
            }
            Stmt::Patch {
                target,
                remove,
                add,
            } => {
                let a = param(&mut eq, P::Text(remove.clone()));
                let b = param(&mut eq, P::Text(add.clone()));
                eq.stmts
                    .push((i1, format!("PATCH {}.body REMOVE {a} ADD {b}", tgt(target))));
            }
            Stmt::Link {
                src,
                kind,
                dst,
                pinned,
            } => {
                let props = match pinned {
                    Some(c) => {
                        let p = param(&mut eq, P::Text(c.clone()));
                        format!(" {{pinned: {p}}}")
                    }
                    None => String::new(),
                };
                eq.stmts.push((
                    i1,
                    format!(
                        "CREATE ({})-[:{}{props}]->({})",
                        tgt(src),
                        lq_edge(schema, kind),
                        tgt(dst)
                    ),
                ));
            }
            Stmt::Unlink { src, kind, dst } => {
                eq.stmts.push((
                    i1,
                    format!(
                        "MATCH ({})-[e:{}]->({}) EXPECT 1 DELETE e",
                        tgt(src),
                        lq_edge(schema, kind),
                        tgt(dst)
                    ),
                ));
            }
            Stmt::Move {
                target,
                under,
                position,
            } => match under {
                Some(u) => eq.stmts.push((
                    i1,
                    format!("MOVE {} UNDER {}{}", tgt(target), tgt(u), pos(position)),
                )),
                None => eq
                    .stmts
                    .push((i1, format!("SET {}.parent = NULL", tgt(target)))),
            },
            Stmt::Reopen { target, reason } => {
                let p = param(&mut eq, P::Text(reason.clone()));
                eq.stmts
                    .push((i1, format!("REOPEN {} REASON {p}", tgt(target))));
            }
            Stmt::Delete {
                target,
                policy,
                replaced_by,
                release,
                reason,
            } => {
                let mut text = format!("DELETE {}", tgt(target));
                if let Some(p) = policy {
                    text.push_str(&format!(" POLICY {}", p.to_uppercase()));
                }
                if let Some(y) = replaced_by {
                    text.push_str(&format!(" REPLACED BY {}", tgt(y)));
                }
                if *release {
                    text.push_str(" RELEASE");
                }
                if let Some(r) = reason {
                    let p = param(&mut eq, P::Text(r.clone()));
                    text.push_str(&format!(" REASON {p}"));
                }
                eq.stmts.push((i1, text));
            }
            Stmt::Resolve { key, take } => {
                let quoted = format!("'{}'", key.replace('\\', "\\\\").replace('\'', "\\'"));
                let t = match take {
                    Take::Ours => "TAKE OURS".to_string(),
                    Take::Theirs => "TAKE THEIRS".to_string(),
                    Take::Base => "TAKE BASE".to_string(),
                    Take::Value(v) => format!("TAKE VALUE {}", param(&mut eq, v.clone())),
                    Take::Repoint(y) => format!("TAKE REPOINT {}", tgt(y)),
                    Take::Drop => "DROP".to_string(),
                };
                eq.stmts.push((i1, format!("RESOLVE {quoted} {t}")));
            }
            Stmt::Call { proc, args } => {
                let mut a = Vec::new();
                for (k, v) in args {
                    let p = param(&mut eq, v.clone());
                    a.push(format!("{k}: {p}"));
                }
                eq.stmts
                    .push((i1, format!("CALL {proc}({})", a.join(", "))));
            }
        }
    }
    let _ = vars;
    eq.text = format!(
        "TX {{ {} }}",
        eq.stmts
            .iter()
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    );
    eq
}

/// `between(a, b)` of [API §17.2]: an order key strictly between `a` (or ⊥) and `b` (or ⊤), never ending in `0`.
// spec: [API §17.2]
pub fn between(a: Option<&str>, b: Option<&str>) -> String {
    const A: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    fn d(c: u8) -> usize {
        A.iter().position(|x| *x == c).expect("an order-key digit")
    }
    fn mid(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
        if let Some(b) = b {
            let pad = |i: usize| a.get(i).copied().unwrap_or(b'0');
            let n = (0..b.len()).take_while(|i| pad(*i) == b[*i]).count();
            if n > 0 {
                let mut out = b[..n].to_vec();
                let rest_a = if n >= a.len() { &[][..] } else { &a[n..] };
                out.extend(mid(rest_a, Some(&b[n..])));
                return out;
            }
        }
        let da = a.first().map_or(0, |c| d(*c));
        let db = b.map_or(62, |b| d(b[0]));
        if db - da > 1 {
            return vec![A[(da + db) / 2]];
        }
        if let Some(b) = b
            && b.len() > 1
        {
            return b[..1].to_vec();
        }
        let mut out = vec![A[da]];
        let rest = if a.is_empty() { &[][..] } else { &a[1..] };
        out.extend(mid(rest, None));
        out
    }
    String::from_utf8(mid(a.unwrap_or("").as_bytes(), b.map(str::as_bytes))).expect("ASCII")
}

/// What one command's write needs from the store: read-only parts and the caller.
pub struct Ctx<'a> {
    /// The DAG.
    pub dag: &'a Dag,
    /// The store-wide allocation.
    pub alloc: &'a dyn Alloc,
    /// uid → `#N`.
    pub uidx: &'a BTreeMap<Uid, Nid>,
    /// The environment.
    pub env: &'a Env,
    /// The branch written.
    pub branch: String,
    /// Its kind.
    pub view: RefKind,
    /// The caller's rights.
    pub rights: Rights,
    /// The resolved actor (CX-3); empty for none.
    pub actor: String,
    /// The resolved session (CX-4).
    pub session: Option<String>,
    /// The stream's seed and command number, for random uids ([API §17.4]).
    pub seed: u64,
    /// The command number `n`.
    pub n: u64,
    /// `HEAD.next_id`.
    pub next_id: u32,
    /// `HEAD.fence`.
    pub fence: u64,
    /// The TTLs and policy data the procedures read.
    pub cfg: &'a KernelCfg,
    /// Whether the presented lease names the caller's thread for a session role lease (the minting thread's hash).
    pub thread_hash: Option<[u8; 16]>,
    /// `root_session` of a lease a Codex holder is granted: BLAKE3-128 of `codex:` + `ctx.meta.sessionId`
    /// ([API §10.1]).
    pub root_session: Option<[u8; 16]>,
    /// Known subagent (WT-014) or dispatched worker (WT-015).
    pub subagent_or_worker: bool,
}

/// The configuration values the kernel reads ([CFG] §10.3, [AR §13] policy data, [F17 §8.2]).
#[derive(Clone, Debug)]
pub struct KernelCfg {
    /// `lease.ttl-default`.
    pub ttl_default_ms: u64,
    /// `lease.reclaim-older-than`.
    pub reclaim_older_than_ms: u64,
    /// `lease.orchestrator-ttl`.
    pub orchestrator_ttl_ms: u64,
    /// `store.suspect-budget`.
    pub suspect_budget: u32,
    /// `tx.max-statements`.
    pub max_statements: u64,
    /// `tx.max-ops`.
    pub max_ops: u64,
    /// `knowledge.owner-authority` is `strict` ([CFG §10.5]; [RULES/status-machines] GR-019).
    pub knowledge_strict: bool,
    /// `gc.reflog-expire`: the reflog window a revision a write names resolves in ([F12 §3]).
    pub reflog_expire_ms: u64,
}

impl Default for KernelCfg {
    fn default() -> KernelCfg {
        KernelCfg {
            ttl_default_ms: 15 * 60_000,
            reclaim_older_than_ms: 30 * 60_000,
            orchestrator_ttl_ms: 12 * 3_600_000,
            suspect_budget: 10_000,
            max_statements: 1000,
            max_ops: 10_000,
            knowledge_strict: false,
            reflog_expire_ms: 90 * 86_400_000,
        }
    }
}

/// The fields of an op with `authority` last, so that WA-001's requirements read the values the same op writes
/// ([RULES/role-write-policy] WA-001: `owner_quote`).
fn authority_last(fields: &[(String, P)]) -> impl Iterator<Item = &(String, P)> {
    fields
        .iter()
        .filter(|(f, _)| f != "authority")
        .chain(fields.iter().filter(|(f, _)| f == "authority"))
}

/// The candidate state of a block, shared copy-on-write ([50 §3.10] item 2: the statements of an LQ block read the
/// candidate where it stands): a statement's view takes it by reference count, and a write copies it only while such
/// a view still holds it.
#[derive(Clone, Debug, Default)]
pub struct CandState(Rc<State>);

impl CandState {
    /// The state, shared with the caller.
    pub fn shared(&self) -> Rc<State> {
        self.0.clone()
    }

    /// The state, given up.
    pub fn into_rc(self) -> Rc<State> {
        self.0
    }
}

impl Deref for CandState {
    type Target = State;

    fn deref(&self) -> &State {
        &self.0
    }
}

impl DerefMut for CandState {
    fn deref_mut(&mut self) -> &mut State {
        Rc::make_mut(&mut self.0)
    }
}

/// The commit a write names ([F08 §5.1] `commitref`, [F08 §10] `pinned_commit`): `c` and 64 lower-case hexadecimal
/// digits is that id, whether or not the store holds it (a cited commit need not be one it holds); any other text is
/// a revision of this store ([F12 §3]), resolved for the branch written. An all-zero id is `bad_value`; a revision
/// that names no commit is E301.
// spec: [F08 §5.1] commitref
pub fn commit_ref(
    dag: &Dag,
    branch: &str,
    env: &Env,
    reflog_expire_ms: u64,
    field: &str,
    text: &str,
) -> Res<[u8; 32]> {
    if let Some(h) = text.strip_prefix('c')
        && h.len() == 64
        && h.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        let mut id = [0u8; 32];
        for (i, x) in id.iter_mut().enumerate() {
            *x = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).expect("hex digits");
        }
        if id == [0u8; 32] {
            return Err(bad(field, "a commit id other than all zero", text));
        }
        return Ok(id);
    }
    let cx = crate::vcs::RevCtx {
        head: Ok(branch.to_string()),
        now_ms: env.wall_ms,
        reflog_expire_ms,
    };
    match dag.rev_commit(text, &cx)? {
        Some(seq) => Ok(dag.commits[&seq].id),
        None => Err(Refusal::lq(
            "E301",
            format!("{field}: {text} names no commit"),
        )),
    }
}

/// A lease event the block produced ([F05 §9.4]): the runtime records of the group.
#[derive(Clone, Debug, PartialEq)]
pub enum LeaseEvent {
    /// Event 1: a grant (`reused` when the claim returned an existing lease).
    Grant {
        /// The lease id.
        id: u64,
        /// Whether it is an existing lease returned again.
        reused: bool,
    },
    /// Event 2: an end with its reason.
    End {
        /// The lease id.
        id: u64,
        /// Why.
        reason: EndReason,
    },
    /// Event 3 with mask bit 1: the lease moved to another branch (`--move-lease`, [AR §5a.4]).
    Moved {
        /// The lease id.
        id: u64,
    },
    /// A renewal: a lazy heartbeat record (cause 1 heartbeat, 2 by use) or a durable event 4 of an expired lease.
    Renew {
        /// The lease id.
        id: u64,
        /// Durable (event 4).
        durable: bool,
    },
}

/// One procedure call's yield rows ([API §3.3] `yields`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Yield {
    /// The data-level statement index.
    pub index: usize,
    /// `tx.<name>`.
    pub proc: String,
    /// The rows: member → value text, in the command section's order.
    pub rows: Vec<Vec<(String, String)>>,
}

/// The candidate of one block.
pub struct Cand<'a> {
    /// The context.
    pub cx: Ctx<'a>,
    /// The view before the block.
    pub base: Rc<State>,
    /// The candidate state.
    pub st: CandState,
    /// The candidate lease table.
    pub leases: BTreeMap<u64, Lease>,
    /// Lease events in order.
    pub events: Vec<LeaseEvent>,
    /// Variables of creates.
    pub vars: BTreeMap<String, Nid>,
    /// Nodes created, in order.
    pub created: Vec<Nid>,
    /// The same nodes as a set (WT-006 `created-in-tx`).
    pub created_ids: BTreeSet<Nid>,
    /// New allocations: `#N` → (uid, creator).
    pub new_alloc: BTreeMap<Nid, (Uid, Creator)>,
    /// The next `#N`.
    pub next_id: u32,
    /// The fence.
    pub fence: u64,
    /// Random uids derived so far in this command ([API §17.4] i).
    pub random_i: u32,
    /// Targets per data-level statement index.
    pub targets: BTreeMap<usize, BTreeSet<Nid>>,
    /// Nodes named in `affected` with a reason besides derived changes.
    pub notified: BTreeSet<Nid>,
    /// Edge kinds each created node got as a source in this block.
    pub edges_from: BTreeMap<Nid, Vec<String>>,
    /// Yields.
    pub yields: Vec<Yield>,
    /// The data-level statement running.
    pub stmt: usize,
    /// A `complete` in the block: the commit message it writes.
    pub message: Option<String>,
    /// The door of `set`'s status changes when a verb names another (`retract`, DR-007).
    pub door: Option<Door>,
    /// For each kernel statement (1-based), the 1-based index of the first LQ statement of the block's LQ form that
    /// renders it ([API §9.3]; [LQ/std §7.2]); a refusal names the LQ statement ([LQ/errors §5.5]).
    pub lq_first: Vec<usize>,
    /// The number of LQ statements of the block's LQ form.
    pub lq_total: usize,
    /// The LQ statement of the running kernel statement, as an offset from its first.
    pub sub: usize,
    /// The LQ statement that created each node of the block.
    pub created_at: BTreeMap<Nid, usize>,
    /// The keys the block's `Resolve` ops name ([F06 §7.7]), one whose value stayed as it was included.
    pub resolves: BTreeSet<Key>,
    /// The keys of the companion ops [F12 §6.5] puts in a `Resolve`'s commit: a live restore's `Move`, `AddEdge` and
    /// `SetEdgeProps`, a `repoint`'s `AddEdge`, a `SupersedeFork`'s `RemoveEdge` ([F12 §9.3]).
    pub companions: BTreeSet<Key>,
}

fn e405(what: impl Into<String>) -> Refusal {
    Refusal::lq("E405", what)
}

/// The history a status guard reads on the candidate: the view's history, with every edge the block added counted as
/// added by the block's actor and every aspect the block changed as changed now.
struct CandHistory<'c> {
    base: &'c State,
    st: &'c State,
    actor: &'c str,
    now_ms: u64,
    view: crate::dag::ViewHistory<'c>,
}

impl History for CandHistory<'_> {
    fn edge_actor(&self, src: Nid, key: &EdgeKey) -> Option<String> {
        let before = self
            .base
            .nodes
            .get(&src)
            .is_some_and(|x| x.out.contains_key(key));
        let after = self
            .st
            .nodes
            .get(&src)
            .is_some_and(|x| x.out.contains_key(key));
        if after && !before {
            return Some(self.actor.to_string());
        }
        self.view.edge_actor(src, key)
    }

    fn changed_ms(&self, n: Nid, aspect: &Aspect) -> Option<u64> {
        let get = |s: &State| s.nodes.get(&n).map(|x| x.kstate(&s.schema, aspect));
        if get(self.base) != get(self.st) {
            return Some(self.now_ms);
        }
        self.view.changed_ms(n, aspect)
    }
}

/// Parses a node reference text `#N` or `#u:<hex>`.
pub fn parse_node(s: &str) -> Option<Target> {
    if let Some(h) = s.strip_prefix("#u:") {
        let b = h.as_bytes();
        if b.len() != 32
            || !b
                .iter()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
        {
            return None;
        }
        let mut u = [0u8; 16];
        for (i, u8x) in u.iter_mut().enumerate() {
            *u8x = u8::from_str_radix(&h[2 * i..2 * i + 2], 16).ok()?;
        }
        return Some(Target::Uid(Uid(u)));
    }
    let d = s.strip_prefix('#')?;
    if d.is_empty() || d.starts_with('0') || !d.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    d.parse::<u32>().ok().map(|n| Target::Id(Nid(n)))
}

/// A duration argument ([API §5.1]): integer milliseconds, or the LQ duration text — digits and one unit of `s`, `m`,
/// `h`, `d`, `w` ([LQ/lexical §5.6]) — with its value at most 2^63 − 1 ms. Anything else is `usage` naming the
/// argument.
// spec: [API §5.1]
// spec: [LQ/lexical §5.6]
pub fn duration_arg(name: &str, v: &P) -> Res<u64> {
    let bad = || Refusal::usage_arg(name, format!("{name} takes milliseconds or an LQ duration"));
    match v {
        P::Int(ms) if *ms >= 0 => Ok(*ms as u64),
        P::Text(t) => {
            let i = t
                .find(|c: char| !c.is_ascii_digit())
                .filter(|i| *i > 0)
                .ok_or_else(bad)?;
            let n: u64 = t[..i].parse().map_err(|_| bad())?;
            let unit: u64 = match &t[i..] {
                "s" => 1000,
                "m" => 60_000,
                "h" => 3_600_000,
                "d" => 86_400_000,
                "w" => 604_800_000,
                _ => return Err(bad()),
            };
            n.checked_mul(unit)
                .filter(|ms| *ms <= i64::MAX as u64)
                .ok_or_else(bad)
        }
        _ => Err(bad()),
    }
}

/// A lease id `L-<n>`.
pub fn parse_lease(s: &str) -> Option<u64> {
    let d = s.strip_prefix("L-")?;
    if d.is_empty() || d.starts_with('0') || !d.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    d.parse().ok()
}

/// Converts a Unix time string `YYYY-MM-DD[THH:MM:SS[.fff]Z]` to seconds (the coercion of [50 §3.2]).
fn iso_seconds(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Option<i64> { s.get(r)?.parse().ok() };
    if b.len() < 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (mut hh, mut mm, mut ss) = (0, 0, 0);
    if b.len() > 10 {
        if b.len() < 20 || b[10] != b'T' || !s.ends_with('Z') {
            return None;
        }
        hh = num(11..13)?;
        mm = num(14..16)?;
        ss = num(17..19)?;
    }
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 60 {
        return None;
    }
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// Normalises a text value: CR LF and lone CR become LF ([F08 §5.3]).
fn norm_text(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn bad(field: &str, shape: &str, got: &str) -> Refusal {
    Refusal::bad_value("shape", format!("{field} expects {shape}; got {got}"))
}

/// Checks a glob of [F08 §5.4.3].
fn glob_ok(g: &str) -> bool {
    if g.is_empty() || g.starts_with('/') || g.ends_with('/') {
        return false;
    }
    g.split('/').all(|seg| {
        if seg == "**" {
            return true;
        }
        if seg.is_empty() || seg.contains("**") {
            return false;
        }
        let mut in_class = false;
        let mut class_len = 0;
        for c in seg.chars() {
            if (c as u32) < 0x20 || c == '\\' {
                return false;
            }
            match (in_class, c) {
                (false, '[') => {
                    in_class = true;
                    class_len = 0;
                }
                (true, ']') => {
                    if class_len == 0 {
                        return false;
                    }
                    in_class = false;
                }
                (true, '!') if class_len == 0 => {}
                (true, _) => class_len += 1,
                _ => {}
            }
        }
        !in_class
    })
}

/// Converts an argument value to a stored value of the field's type ([API §5.2]; [F08 §5.3], §5.4), or refuses.
pub fn convert(
    schema: &Schema,
    kind: &str,
    field: &str,
    v: &P,
    uidx: &BTreeMap<Uid, Nid>,
    next_id: u32,
    commit: &dyn Fn(&str) -> Res<[u8; 32]>,
) -> Res<Option<Value>> {
    if *v == P::Null {
        return Ok(None);
    }
    let f = schema
        .field(kind, field)
        .ok_or_else(|| Refusal::lq("E101", format!("{kind} has no field {field}")))?;
    let text_of = |v: &P| -> Res<String> {
        match v {
            P::Text(s) => Ok(norm_text(s)),
            other => Err(Refusal::lq(
                "E103",
                format!("{field} takes text; got {other:?}"),
            )),
        }
    };
    let one = |v: &P, ty: Ty| -> Res<Value> {
        match ty {
            Ty::Bool => match v {
                P::Bool(b) => Ok(Value::Bool(*b)),
                o => Err(Refusal::lq(
                    "E103",
                    format!("{field} takes a bool; got {o:?}"),
                )),
            },
            Ty::Int => {
                let i = match v {
                    P::Int(i) => *i,
                    P::Text(s)
                        if f.name == "defer_until"
                            || f.name == "due"
                            || f.name == "since"
                            || f.name == "started"
                            || f.name == "ended"
                            || f.name == "review_after" =>
                    {
                        iso_seconds(s).ok_or_else(|| bad(field, "an ISO 8601 time", s))?
                    }
                    o => {
                        return Err(Refusal::lq(
                            "E103",
                            format!("{field} takes an int; got {o:?}"),
                        ));
                    }
                };
                if let Some((lo, hi)) = f.range
                    && !(lo..=hi).contains(&i)
                {
                    return Err(bad(field, &format!("an int in {lo}..{hi}"), &i.to_string()));
                }
                Ok(Value::Int(i))
            }
            Ty::Counter => match v {
                P::Int(i) => Ok(Value::Counter(*i)),
                o => Err(Refusal::lq(
                    "E103",
                    format!("{field} is a counter; got {o:?}"),
                )),
            },
            Ty::F64 => {
                let x = match v {
                    P::Float(x) => *x,
                    P::Int(i) => *i as f64,
                    o => {
                        return Err(Refusal::lq(
                            "E103",
                            format!("{field} takes a number; got {o:?}"),
                        ));
                    }
                };
                F64::new(x).map(Value::F64).ok_or_else(|| {
                    Refusal::bad_value("nan", format!("NaN is not a value of {field}"))
                })
            }
            Ty::Enum => {
                let name = match (v, f.name.as_str()) {
                    (P::Int(i), "priority") if (0..=4).contains(i) => format!("P{i}"),
                    (P::Text(s), "priority")
                        if s.len() == 1 && s.as_bytes()[0].is_ascii_digit() =>
                    {
                        format!("P{s}")
                    }
                    (P::Text(s), _) => s.clone(),
                    (o, _) => {
                        return Err(Refusal::lq(
                            "E103",
                            format!("{field} takes a name; got {o:?}"),
                        ));
                    }
                };
                if schema.value(kind, field, &name).is_none_or(|e| e.retired) {
                    return Err(Refusal::lq(
                        "E102",
                        format!("{name} is not a value of {kind}.{field}"),
                    ));
                }
                // [F08 §9.2] row 7: a finding takes only `unset`, `confirmed` and `plausible`; every other kind every value
                // but `confirmed` and `plausible` (`bad_value`; spec sync 2b).
                if field == "confidence" {
                    let finding_only = matches!(name.as_str(), "confirmed" | "plausible");
                    let ok = if kind == "finding" {
                        finding_only || name == "unset"
                    } else {
                        !finding_only
                    };
                    if !ok {
                        return Err(bad(
                            field,
                            if kind == "finding" {
                                "unset, confirmed or plausible on a finding"
                            } else {
                                "a value other than confirmed and plausible"
                            },
                            &name,
                        ));
                    }
                }
                Ok(Value::Enum(name))
            }
            Ty::Text | Ty::Sym => {
                let s = text_of(v)?;
                if s.contains('\0') {
                    return Err(bad(field, "text without U+0000", "U+0000"));
                }
                if (f.one_line || ty == Ty::Sym) && s.contains('\n') {
                    return Err(bad(field, "one line", &s));
                }
                if f.ascii && !s.bytes().all(|b| (0x20..=0x7E).contains(&b)) {
                    return Err(bad(field, "ASCII", &s));
                }
                let max = if field == "title" {
                    200
                } else if ty == Ty::Sym {
                    4096
                } else {
                    65_536
                };
                if s.len() > max {
                    return Err(bad(
                        field,
                        &format!("at most {max} bytes"),
                        &format!("{} bytes", s.len()),
                    ));
                }
                match f.shape {
                    Shape::Records(members) => {
                        for rec in s.split('\n') {
                            let parts: Vec<&str> = rec.split('\t').collect();
                            if parts.len() != members.len()
                                || members
                                    .iter()
                                    .zip(&parts)
                                    .any(|((_, req), p)| *req && p.is_empty())
                            {
                                return Err(bad(field, "its record list shape", rec));
                            }
                        }
                    }
                    Shape::OrderKey
                        if !s.bytes().all(|b| b.is_ascii_alphanumeric()) || s.ends_with('0') =>
                    {
                        return Err(bad(field, "an order key", &s));
                    }
                    _ => {}
                }
                Ok(Value::Text(s))
            }
            Ty::Ref => match v {
                P::Text(s) => match parse_node(s) {
                    Some(Target::Id(n)) if n.0 >= 1 && n.0 < next_id => Ok(Value::Ref(n)),
                    Some(Target::Uid(u)) => uidx.get(&u).map(|n| Value::Ref(*n)).ok_or_else(|| {
                        Refusal::lq("E111", format!("{s} is not a node of this store"))
                    }),
                    _ => Err(Refusal::lq("E111", format!("{s} names no node"))),
                },
                o => Err(Refusal::lq(
                    "E103",
                    format!("{field} takes a node; got {o:?}"),
                )),
            },
            Ty::Commit => {
                let s = text_of(v)?;
                commit(&s).map(Value::Commit)
            }
            Ty::Path => {
                let s = text_of(v)?;
                let (root, text) = s
                    .split_once(':')
                    .ok_or_else(|| bad(field, "root:path", &s))?;
                if root.is_empty() || text.is_empty() {
                    return Err(bad(field, "root:path", &s));
                }
                Ok(Value::Path(PathVal {
                    root: root.into(),
                    text: text.into(),
                }))
            }
            Ty::Oid => {
                let s = text_of(v)?;
                let (a, h) = s
                    .split_once(':')
                    .ok_or_else(|| bad(field, "algo:hex", &s))?;
                let algo = match a {
                    "sha1" => Algo::Sha1,
                    "sha256" => Algo::Sha256,
                    _ => return Err(bad(field, "sha1 or sha256", a)),
                };
                if h.len() != algo.digest_len() * 2
                    || !h
                        .bytes()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                {
                    return Err(bad(field, "a digest of its algorithm", h));
                }
                let digest = (0..algo.digest_len())
                    .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).expect("hex"))
                    .collect();
                Ok(Value::Oid(Oid { algo, digest }))
            }
            Ty::PathMove | Ty::Set(_) | Ty::Body => Err(Refusal::lq(
                "E115",
                format!("{field} is not written by a value"),
            )),
        }
    };
    match f.ty {
        Ty::Set(elem) => {
            let P::List(items) = v else {
                return Err(Refusal::lq("E103", format!("{field} takes a list")));
            };
            let ety = match elem {
                Elem::Sym => Ty::Sym,
                Elem::Path => Ty::Path,
                Elem::Int => Ty::Int,
                Elem::PathMove => Ty::PathMove,
            };
            let mut out = Vec::new();
            for it in items {
                let x = one(it, ety)?;
                if out.contains(&x) {
                    return Err(Refusal::bad_value(
                        "shape",
                        format!("{field} lists {x:?} twice"),
                    ));
                }
                if let (Shape::Globs, Value::Text(g)) = (f.shape, &x)
                    && !glob_ok(g)
                {
                    return Err(bad(field, "a glob", g));
                }
                if let (Shape::Tagged, Value::Text(g)) = (f.shape, &x) {
                    let ok = ["role:", "phase:", "lane:"].iter().any(|p| {
                        g.strip_prefix(p).is_some_and(|r| {
                            !r.is_empty()
                                && r.len() <= 64
                                && r.bytes().all(|b| {
                                    b.is_ascii_lowercase()
                                        || b.is_ascii_digit()
                                        || b"_./-".contains(&b)
                                })
                        })
                    }) || g.strip_prefix("path:").is_some_and(glob_ok);
                    if !ok {
                        return Err(bad(field, "a tagged scope element", g));
                    }
                }
                out.push(x);
            }
            Ok(Value::set(out))
        }
        ty => one(v, ty).map(Some),
    }
}

/// How a node breaks I4 ([AR §3.4]; [F13 §5] V05).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Forest {
    /// Its parent chain comes back to it.
    Cycle,
    /// Its parent chain is longer than 12 without coming back to it.
    TooDeep,
}

/// I4 for one live node of a state: `None` when its parent chain ends within 12 steps; at most 13 steps are walked.
fn forest_fault(st: &State, n: Nid) -> Option<Forest> {
    let mut cur = st.live(n).and_then(|x| x.parent);
    let mut depth = 0;
    while let Some(p) = cur {
        if p == n {
            return Some(Forest::Cycle);
        }
        depth += 1;
        if depth > 12 {
            return Some(Forest::TooDeep);
        }
        cur = st.live(p).and_then(|x| x.parent);
    }
    None
}

impl<'a> Cand<'a> {
    /// A candidate over the view's state and the store's lease table.
    pub fn new(cx: Ctx<'a>, base: Rc<State>, leases: BTreeMap<u64, Lease>) -> Cand<'a> {
        let next_id = cx.next_id;
        let fence = cx.fence;
        Cand {
            cx,
            st: CandState(base.clone()),
            base,
            leases,
            events: Vec::new(),
            vars: BTreeMap::new(),
            created: Vec::new(),
            created_ids: BTreeSet::new(),
            new_alloc: BTreeMap::new(),
            next_id,
            fence,
            random_i: 0,
            targets: BTreeMap::new(),
            notified: BTreeSet::new(),
            edges_from: BTreeMap::new(),
            yields: Vec::new(),
            stmt: 0,
            message: None,
            door: None,
            lq_first: Vec::new(),
            lq_total: 0,
            sub: 0,
            created_at: BTreeMap::new(),
            resolves: BTreeSet::new(),
            companions: BTreeSet::new(),
        }
    }

    /// Sets the LQ form's statement map: the kernel statement index each LQ statement renders, in order.
    pub fn set_lq_map(&mut self, kernel_of: &[usize]) {
        self.lq_total = kernel_of.len();
        self.lq_first.clear();
        for (i, k) in kernel_of.iter().enumerate() {
            while self.lq_first.len() < *k {
                self.lq_first.push(i + 1);
            }
        }
    }

    /// The 1-based index of the LQ statement running now ([LQ/errors §5.5]); the kernel statement's own index when the
    /// block's LQ form was not given.
    pub fn lq_stmt(&self) -> usize {
        self.lq_first
            .get(self.stmt.wrapping_sub(1))
            .map_or(self.stmt, |f| f + self.sub)
    }

    /// The index of the block's last LQ statement: the statement a deferred validator names, since the deferred
    /// validators run at the end of the block ([API §9.1]).
    pub fn last_lq_stmt(&self) -> usize {
        if self.lq_total > 0 {
            self.lq_total
        } else {
            self.stmt
        }
    }

    fn touch(&mut self, n: Nid) {
        self.targets.entry(self.stmt).or_default().insert(n);
    }

    /// The node object of an E401 `current` entry ([LQ/errors §5.7]): the node's id, kind, status, title and `rev`,
    /// with `changed_by` naming the commit of that `rev` (its actor, ref and message).
    pub fn current(&self, n: Nid) -> Kv {
        let tip = self.cx.dag.live(&self.cx.branch).and_then(|r| r.tip);
        let rev = self.cx.dag.local_seqs(tip, n).0;
        let x = self.st.nodes.get(&n);
        let mut m = vec![
            ("id".to_string(), Kv::Node(n)),
            (
                "kind".into(),
                x.map_or(Kv::Null, |x| Kv::Str(x.kind.clone())),
            ),
            (
                "status".into(),
                x.map_or(Kv::Null, |x| Kv::Str(x.status.clone())),
            ),
            (
                "title".into(),
                x.and_then(|x| x.text("title")).map_or(Kv::Null, Kv::from),
            ),
            ("rev".into(), Kv::Int(rev as i64)),
        ];
        let changed_by = self.cx.dag.commits.get(&rev).map_or(Kv::Null, |c| {
            Kv::Obj(vec![
                ("commit".into(), Kv::Commit(c.seq)),
                ("actor".into(), Kv::Str(c.actor.clone())),
                (
                    "ref".into(),
                    Kv::Str(
                        self.cx
                            .dag
                            .refs
                            .get(&c.ref_id)
                            .map_or(String::new(), |r| r.name.clone()),
                    ),
                ),
                ("message".into(), Kv::Str(c.message.clone())),
            ])
        });
        m.push(("changed_by".into(), changed_by));
        Kv::Obj(m)
    }

    /// E407 naming a lease and its holder ([LQ/errors §5.7]).
    fn e407(&self, id: u64, detail: String) -> Refusal {
        Refusal::e407(
            Some(format!("L-{id}")),
            self.leases.get(&id).map(|l| l.holder.clone()),
            detail,
        )
    }

    /// E401 for a `MATCH … EXPECT 1` that matched nothing, with `expect`, `matched` and `current` ([LQ/errors §5.7]).
    fn e401(&self, detail: String, literal: &[Nid]) -> Refusal {
        Refusal::lq("E401", detail)
            .key("expect", "1")
            .key("matched", 0i64)
            .key(
                "current",
                Kv::List(literal.iter().take(10).map(|n| self.current(*n)).collect()),
            )
    }

    fn scopes(&self) -> Scopes<'_> {
        Scopes {
            st: &self.st,
            created: &self.created_ids,
        }
    }

    /// Resolves a target to a live node of the view (not_found: "node is not live on the ref"; E111 for an id never
    /// allocated).
    // rule: GR-005
    pub fn resolve(&self, t: &Target) -> Res<Nid> {
        let n = match t {
            Target::Id(n) => {
                if n.0 == 0 || n.0 >= self.next_id {
                    return Err(Refusal::lq("E111", format!("{n} was never allocated")));
                }
                *n
            }
            Target::Uid(u) => *self
                .cx
                .uidx
                .get(u)
                .or_else(|| {
                    self.new_alloc
                        .iter()
                        .find(|(_, (x, _))| x == u)
                        .map(|(n, _)| n)
                })
                .ok_or_else(|| Refusal::lq("E111", format!("{u} is not a node of this store")))?,
            Target::Var(v) => *self
                .vars
                .get(v)
                .ok_or_else(|| Refusal::lq("E108", format!("${v} is not a created node")))?,
        };
        if self.st.live(n).is_none() {
            return Err(Refusal::new(
                "not_found",
                3,
                format!("node {n} is not live on {}", self.cx.branch),
            )
            .key("what", "node")
            .key("value", n.to_string()));
        }
        Ok(n)
    }

    /// The uid of a new node: [API §17.4] under the injected entropy, drawn again while zero or known.
    // spec: [API §17.4]
    fn random_uid(&mut self) -> Uid {
        let mut t: u32 = 0;
        loop {
            let u = Uid(blake3_128(&[
                b"moirai-api-uid-v1",
                &self.cx.seed.to_le_bytes(),
                &self.cx.n.to_le_bytes(),
                &self.random_i.to_le_bytes(),
                &t.to_le_bytes(),
            ]));
            if u != Uid::ZERO
                && !self.cx.uidx.contains_key(&u)
                && !self.new_alloc.values().any(|(x, _)| *x == u)
            {
                self.random_i += 1;
                return u;
            }
            t += 1;
        }
    }

    /// I4 for one node: its parent chain ends within 12 steps and never returns to it; at most 13 steps are walked.
    fn check_depth(&self, n: Nid) -> Res<()> {
        match forest_fault(&self.st, n) {
            None => Ok(()),
            Some(_) => Err(e405(format!(
                "{n}: the parent forest would have a cycle or a depth above 12 (I4)"
            ))),
        }
    }

    /// Whether the view before the block fails V05 ([F13 §5]): some node of it breaks I4.
    fn view_breaks_i4(&self) -> bool {
        self.base
            .nodes
            .keys()
            .any(|m| forest_fault(&self.base, *m).is_some())
    }

    /// I4 for one node under VO-3 ([F13 §5]). On a staging ref a `RESOLVE` block is refused when it makes a node its
    /// own ancestor — a node on a cycle that was not on one in the view before the block — and for a depth above 12
    /// only when the view before passed V05, the check as a whole; `view_broke` answers that once per block. On any
    /// other ref every fault refuses.
    fn check_forest(&self, n: Nid, view_broke: &std::cell::OnceCell<bool>) -> Res<()> {
        let staged = self.cx.view == RefKind::Merge;
        match forest_fault(&self.st, n) {
            None => Ok(()),
            Some(Forest::Cycle) if staged && forest_fault(&self.base, n) == Some(Forest::Cycle) => {
                Ok(())
            }
            Some(Forest::TooDeep)
                if staged && *view_broke.get_or_init(|| self.view_breaks_i4()) =>
            {
                Ok(())
            }
            Some(_) => self.check_depth(n),
        }
    }

    /// The order key of a position among the ordered live children of `parent` ([API §9.5]).
    fn order_for(&self, parent: Nid, me: Nid, pos: &Position) -> Res<String> {
        let mut sibs: Vec<(String, Uid, Nid)> = self
            .st
            .nodes
            .iter()
            .filter(|(n, x)| **n != me && x.live() && x.parent == Some(parent))
            .filter_map(|(n, x)| x.order.clone().map(|o| (o, x.uid, *n)))
            .collect();
        sibs.sort();
        let at = |t: &Target| -> Res<usize> {
            let y = self.resolve(t)?;
            sibs.iter().position(|(_, _, n)| *n == y).ok_or_else(|| {
                Refusal::lq("E404", format!("{y} is not an ordered child of {parent}"))
            })
        };
        Ok(match pos {
            Position::First => between(None, sibs.first().map(|s| s.0.as_str())),
            Position::Last => between(sibs.last().map(|s| s.0.as_str()), None),
            Position::Before(t) => {
                let i = at(t)?;
                between(
                    if i == 0 {
                        None
                    } else {
                        Some(sibs[i - 1].0.as_str())
                    },
                    Some(sibs[i].0.as_str()),
                )
            }
            Position::After(t) => {
                let i = at(t)?;
                between(
                    Some(sibs[i].0.as_str()),
                    sibs.get(i + 1).map(|s| s.0.as_str()),
                )
            }
        })
    }

    /// Moves `n` under `parent` (or detaches it) with an optional position; `doc` nodes get `last` by default.
    fn place(&mut self, n: Nid, parent: Option<Nid>, pos: Option<&Position>) -> Res<()> {
        if let Some(p) = parent {
            let (ck, pk) = (
                self.st.nodes[&n].kind.clone(),
                self.st.nodes[&p].kind.clone(),
            );
            let e = self.st.schema.edge("parent").expect("core edge");
            if !e.src.allows(&ck) || !e.dst.allows(&pk) || ck != pk {
                return Err(e405(format!("{n} ({ck}) cannot be a child of {p} ({pk})")));
            }
            if p == n {
                return Err(e405(format!("{n} cannot be its own parent (I4)")));
            }
        }
        let order = match (parent, pos) {
            (Some(p), Some(pos)) => Some(self.order_for(p, n, pos)?),
            (Some(p), None) if self.st.nodes[&n].kind == "doc" => {
                Some(self.order_for(p, n, &Position::Last)?)
            }
            _ => None,
        };
        let x = self.st.nodes.get_mut(&n).expect("placed node");
        x.parent = parent;
        x.order = if parent.is_some() { order } else { None };
        self.check_depth(n)
    }

    /// The status change of a node through a door, with the branch mask, the transition row, its guards and the role
    /// grant ([RULES/status-machines] §2 "Evaluation").
    fn transit(&mut self, n: Nid, to: &str, resolution: &str, door: Door) -> Res<()> {
        let (kind, from) = {
            let x = &self.st.nodes[&n];
            (x.kind.clone(), x.status.clone())
        };
        if from == to && door != Door::Reopen {
            return Ok(());
        }
        status::branch_mask(self.cx.view.token())?;
        if kind == "artifact" && door == Door::SetStatus {
            return Err(status::artifact_set());
        }
        if status::is_project_kind(&kind) {
            status::project_transition(&self.st.schema, &kind, &from, to, door)?;
        } else {
            status::transition(&kind, &from, to, door)?;
        }
        status::resolution_ok(&kind, &from, to, resolution)?;
        self.cx
            .rights
            .status(self.stmt, &self.scopes(), n, &from, to, door.token())?;
        {
            let x = self.st.nodes.get_mut(&n).expect("node");
            x.status = to.to_string();
            x.resolution = if to == "open" {
                "none".into()
            } else {
                resolution.to_string()
            };
        }
        let h = CandHistory {
            base: &self.base,
            st: &self.st,
            actor: &self.cx.actor,
            now_ms: self.cx.env.wall_ms.max(0) as u64,
            view: self
                .cx
                .dag
                .history(self.cx.dag.live(&self.cx.branch).and_then(|r| r.tip)),
        };
        status::check_guards(&self.st, n, &kind, &from, to, &h)?;
        self.touch(n);
        Ok(())
    }

    /// LP-3: the first `set` of the leased task that presents its lease while the task is `open` also performs
    /// `open → in_progress` (door `lease-first-write`, DR-004).
    // spec: [API §10.6] LP-3
    fn lease_first_write(&mut self, n: Nid) -> Res<()> {
        let leased = self
            .cx
            .rights
            .lease
            .as_ref()
            .is_some_and(|l| l.task == Some(n));
        if leased && self.st.nodes[&n].status == "open" && self.st.nodes[&n].kind == "task" {
            self.transit(n, "in_progress", "none", Door::LeaseFirstWrite)?;
        }
        Ok(())
    }

    /// Sets one field of a node with its role checks ([RULES/role-write-policy] WR-009).
    // rule: GR-013
    fn set_one(&mut self, n: Nid, field: &str, v: &P) -> Res<()> {
        let kind = self.st.nodes[&n].kind.clone();
        // GR-013: derived and runtime properties are never written.
        if [
            "blocked",
            "ready",
            "unblocked",
            "stale",
            "claimed",
            "container",
            "answered",
            "conflicted",
            "suspect",
        ]
        .contains(&field)
        {
            return Err(Refusal::lq(
                "E115",
                format!("{field} is derived and never written (GR-013)"),
            ));
        }
        match field {
            "status" | "resolution" | "done" => {
                return Err(Refusal::lq(
                    "E115",
                    format!("{field} is written with its transition"),
                ));
            }
            "parent" | "order" | "body" => {
                return Err(Refusal::lq(
                    "E115",
                    format!("{field} is written with MOVE or SET body"),
                ));
            }
            _ => {}
        }
        // [API §9.1]: "a counter assigned" is E103 (a counter moves only by `incr`), before any policy check.
        if self
            .st
            .schema
            .field(&kind, field)
            .is_some_and(|f| f.ty == Ty::Counter)
        {
            return Err(Refusal::lq(
                "E103",
                format!("{kind}.{field} is a counter: write it with incr"),
            ));
        }
        crate::policy::i33p_plan_mask(self.cx.view.token(), field)?;
        let value = convert(
            &self.st.schema,
            &kind,
            field,
            v,
            self.cx.uidx,
            self.next_id,
            &|t| self.commit_id(field, t),
        )?;
        if let Some(fi) = self.st.schema.field(&kind, field)
            && matches!(fi.class, "identity" | "observation")
        {
            return Err(Refusal::lq(
                "E115",
                format!("{kind}.{field} is written by capture"),
            ));
        }
        let immutable = self
            .st
            .schema
            .kind(&kind)
            .is_some_and(|k| k.immutable_fields);
        let created_here = self.created.contains(&n);
        if immutable && !created_here {
            return Err(e405(format!(
                "{kind} fields are read-only after Create (I11)"
            )));
        }
        if kind == "decision" && self.st.nodes[&n].status == "accepted" && !created_here {
            return Err(e405(format!(
                "{n}: an accepted decision is never edited (I11)"
            )));
        }
        if field == "authority" {
            if let Some(v) = &value {
                self.cx.rights.value(self.stmt, &self.st, n, field, v)?;
            }
        } else {
            self.cx.rights.field(self.stmt, &self.scopes(), n, field)?;
            if let Some(v) = &value {
                self.cx.rights.value(self.stmt, &self.st, n, field, v)?;
            }
        }
        let schema = self.st.schema.clone();
        self.st
            .nodes
            .get_mut(&n)
            .expect("node")
            .set_field(&schema, field, value);
        self.touch(n);
        Ok(())
    }

    /// `create` ([API §9.2]): a new node with its fields, parent and position, edges and body.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        &mut self,
        name: Option<&str>,
        kind: &str,
        fields: &[(String, P)],
        body: Option<&str>,
        under: Option<&Target>,
        position: Option<&Position>,
        edges_out: &[(String, Target)],
        edges_in: &[(String, Target)],
    ) -> Res<Nid> {
        let k = self
            .st
            .schema
            .kind(kind)
            .ok_or_else(|| Refusal::lq("E105", format!("unknown kind {kind}")))?
            .clone();
        if kind == "artifact" {
            return Err(Refusal::lq(
                "E115",
                "CREATE (:artifact …) — file nodes are registered by capture",
            ));
        }
        if self.next_id == u32::MAX {
            return Err(Refusal::new(
                "id_space_exhausted",
                7,
                "the node id space of this store is full",
            ));
        }
        let n = Nid(self.next_id);
        self.next_id += 1;
        let uid = self.random_uid();
        let creator = Creator {
            actor: self.cx.actor.clone(),
            role: self
                .cx
                .rights
                .lease
                .as_ref()
                .map(|l| l.role.clone())
                .unwrap_or_default(),
        };
        let _ = k;
        let node = Node::new(uid, kind, &self.st.schema, creator.clone());
        self.st.nodes.insert(n, node);
        self.new_alloc.insert(n, (uid, creator));
        self.created.push(n);
        self.created_ids.insert(n);
        self.created_at.insert(n, self.lq_stmt());
        if let Some(v) = name {
            self.vars.insert(v.to_string(), n);
        }
        self.touch(n);
        // The LQ form runs `CREATE (…)[ UNDER p]` — the node, its properties, its parent and its status — then the
        // `MOVE` of a position, each edge's `CREATE`, and `SET body` ([API §9.2]); each is its own statement.
        let initial = self.st.nodes[&n].status.clone();
        let mut status_to = None;
        let mut resolution = None;
        // `authority` is checked last: WA-001's owner quote is the one the same op writes, whatever the order of the
        // op's fields.
        for (f, v) in authority_last(fields) {
            match (f.as_str(), v) {
                ("status", P::Text(to)) => status_to = Some(to.clone()),
                ("resolution", P::Text(r)) => resolution = Some(r.clone()),
                ("done", P::Bool(true)) => status_to = Some(status::done_status(kind)?.to_string()),
                ("done", P::Bool(false)) => return Err(status::done_false()),
                ("status" | "resolution" | "done", _) => {
                    return Err(Refusal::lq("E103", format!("{f} takes a name or a bool")));
                }
                _ => self.set_one(n, f, v)?,
            }
        }
        let parent = match under {
            Some(u) => {
                let p = self.resolve(u)?;
                self.cx
                    .rights
                    .field(self.stmt, &self.scopes(), n, "parent")?;
                self.place(n, Some(p), None)?;
                Some(p)
            }
            None => None,
        };
        self.create_status(n, kind, &initial, status_to, resolution)?;
        if let (Some(p), Some(pos)) = (parent, position) {
            self.sub += 1;
            self.cx
                .rights
                .field(self.stmt, &self.scopes(), n, "parent")?;
            self.place(n, Some(p), Some(pos))?;
        }
        for (ek, d) in edges_out {
            self.sub += 1;
            let d = self.resolve(d)?;
            self.link(n, ek, d, None)?;
        }
        for (ek, s) in edges_in {
            self.sub += 1;
            let s = self.resolve(s)?;
            self.link(s, ek, n, None)?;
        }
        if let Some(b) = body {
            self.sub += 1;
            self.set_body(n, Some(b))?;
        }
        Ok(n)
    }

    /// The commit a write names, resolved for the branch written ([`commit_ref`]).
    fn commit_id(&self, field: &str, text: &str) -> Res<[u8; 32]> {
        commit_ref(
            self.cx.dag,
            &self.cx.branch,
            self.cx.env,
            self.cx.cfg.reflog_expire_ms,
            field,
            text,
        )
    }

    /// The status a `Create` names: a checked path of transitions from the initial status (GR-006), or none; a
    /// resolution without a status is GR-014's refusal. A `rule` or a `decision` reaches the status GR-019 gives, and
    /// naming another is refused.
    // rule: GR-006
    fn create_status(
        &mut self,
        n: Nid,
        kind: &str,
        initial: &str,
        status_to: Option<String>,
        resolution: Option<String>,
    ) -> Res<()> {
        let owner = matches!(
            self.st.nodes[&n].fields.get("authority"),
            Some(Value::Enum(a)) if a == "owner"
        );
        if let Some(target) = status::knowledge_initial(kind, owner, self.cx.cfg.knowledge_strict) {
            return self.knowledge_status(n, kind, initial, target, status_to, resolution);
        }
        if let Some(to) = status_to {
            if self.st.schema.value(kind, "status", &to).is_none() {
                return Err(Refusal::lq(
                    "E102",
                    format!("{to} is not a status of {kind}"),
                ));
            }
            // GR-018: a project kind reaches any of its statuses in one `set-status` step.
            if status::is_project_kind(kind) {
                let res = resolution.unwrap_or_else(|| "none".into());
                return self.transit(n, &to, &res, Door::SetStatus);
            }
            // GR-006: a create in a non-initial status is a checked path of transitions.
            let path = status::create_path(kind, initial, &to)
                .ok_or_else(|| Refusal::lq("E404", format!("{kind} cannot be created {to}")))?;
            let steps = path.len();
            for (i, r) in path.into_iter().enumerate() {
                let door = match r.tok("door") {
                    "supersede" => Door::Supersede,
                    "retract" => Door::Retract,
                    "reopen" => Door::Reopen,
                    _ => Door::SetStatus,
                };
                let last = i + 1 == steps;
                let res = match (&resolution, last) {
                    (Some(r), true) => r.clone(),
                    _ if r.tok("to") == "done" && kind == "task" => "completed".into(),
                    _ => "none".into(),
                };
                self.transit(n, r.tok("to"), &res, door)?;
            }
        } else if let Some(r) = resolution
            && r != "none"
        {
            return Err(Refusal::lq(
                "E404",
                format!("resolution {r} is written with its status transition (GR-014)"),
            ));
        }
        Ok(())
    }

    /// GR-019: a `Create` of a rule or a decision starts `proposed`; an owner-authority one moves on in the same
    /// statement to `target` (`active` or `accepted`, TR-026 or TR-032) under the transition's guards and the role
    /// grant. A `Create` that names another status is refused, naming the rule and the transition to use.
    // rule: GR-019
    fn knowledge_status(
        &mut self,
        n: Nid,
        kind: &str,
        initial: &str,
        target: &str,
        named: Option<String>,
        resolution: Option<String>,
    ) -> Res<()> {
        if let Some(to) = named
            && to != target
        {
            return Err(Refusal::lq(
                "E404",
                format!(
                    "a new {kind} starts {target}, not {to} (GR-019); create it without a status, then a later write moves it"
                ),
            ));
        }
        if let Some(r) = resolution
            && r != "none"
        {
            return Err(Refusal::lq(
                "E404",
                format!("resolution {r} is written with its status transition (GR-014)"),
            ));
        }
        if target != initial {
            self.transit(n, target, "none", Door::SetStatus)?;
        }
        Ok(())
    }

    fn set_body(&mut self, n: Nid, b: Option<&str>) -> Res<()> {
        self.cx.rights.field(self.stmt, &self.scopes(), n, "body")?;
        let b = b.map(norm_text).filter(|s| !s.is_empty());
        if b.as_ref().is_some_and(|s| s.len() > 65_536) {
            return Err(bad("body", "at most 65536 bytes", "more"));
        }
        self.st.nodes.get_mut(&n).expect("node").body = b;
        self.touch(n);
        Ok(())
    }

    /// `set` ([API §9.2]).
    pub fn set(
        &mut self,
        target: &Target,
        fields: &[(String, P)],
        incr: &[(String, i64)],
        body: Option<Option<&str>>,
        guard: Option<&Guard>,
    ) -> Res<()> {
        let n = self.resolve(target)?;
        if let Some(g) = guard {
            let tip = self.cx.dag.live(&self.cx.branch).and_then(|r| r.tip);
            let rev = self.cx.dag.local_seqs(tip, n).0;
            let x = &self.st.nodes[&n];
            let holder = self
                .leases
                .values()
                .find(|l| l.task == Some(n) && lease::is_live(l, self.cx.env).is_live())
                .map(|l| l.holder.clone());
            let ok = g.if_rev.is_none_or(|r| r == rev)
                && g.if_status.as_ref().is_none_or(|s| *s == x.status)
                && g.if_holder
                    .as_ref()
                    .is_none_or(|h| holder.as_ref() == Some(h));
            if !ok {
                return Err(self.e401(
                    format!("the guard of {n} does not hold (EXPECT 1 matched 0)"),
                    &[n],
                ));
            }
        }
        self.lease_first_write(n)?;
        let mut status_to: Option<String> = None;
        let mut resolution: Option<String> = None;
        for (f, v) in authority_last(fields) {
            match (f.as_str(), v) {
                ("status", P::Text(to)) => {
                    status_to = Some(to.clone());
                    continue;
                }
                ("resolution", P::Text(r)) => {
                    resolution = Some(r.clone());
                    continue;
                }
                ("done", P::Bool(true)) => {
                    let kind = self.st.nodes[&n].kind.clone();
                    status_to = Some(status::done_status(&kind)?.to_string());
                    continue;
                }
                ("done", P::Bool(false)) => return Err(status::done_false()),
                ("status" | "resolution" | "done", _) => {
                    return Err(Refusal::lq("E103", format!("{f} takes a name or a bool")));
                }
                _ => {}
            }
            self.set_one(n, f, v)?;
            if f == "files_owned"
                && let Some(l) = self.cx.rights.lease.as_ref().filter(|l| l.task == Some(n))
                && let Some(row) = self.leases.get_mut(&l.id)
            {
                // LP-4: a set of files_owned under the lease re-captures it.
                row.files_owned = match self.st.nodes[&n].fields.get("files_owned") {
                    Some(Value::Set(v)) => v
                        .iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect(),
                    _ => Vec::new(),
                };
            }
        }
        for (c, d) in incr {
            let kind = self.st.nodes[&n].kind.clone();
            let fi = self
                .st
                .schema
                .field(&kind, c)
                .ok_or_else(|| Refusal::lq("E101", format!("{kind} has no field {c}")))?;
            if fi.ty != Ty::Counter {
                return Err(Refusal::lq("E103", format!("{c} is not a counter")));
            }
            self.cx.rights.field(self.stmt, &self.scopes(), n, c)?;
            let cur = match self.st.nodes[&n].fields.get(c) {
                Some(Value::Counter(v)) => *v,
                _ => 0,
            };
            let nv = cur
                .checked_add(*d)
                .ok_or_else(|| bad(c, "a counter in i64", "an overflow"))?;
            if let Some((lo, _)) = fi.range
                && nv < lo
            {
                return Err(bad(c, &format!("a counter ≥ {lo}"), &nv.to_string()));
            }
            let schema = self.st.schema.clone();
            self.st.nodes.get_mut(&n).expect("node").set_field(
                &schema,
                c,
                Some(Value::Counter(nv)),
            );
            self.touch(n);
        }
        if let Some(b) = body {
            self.set_body(n, b)?;
        }
        let kind = self.st.nodes[&n].kind.clone();
        match (status_to, resolution) {
            (Some(to), res) => {
                if self.st.schema.value(&kind, "status", &to).is_none() {
                    return Err(Refusal::lq(
                        "E102",
                        format!("{to} is not a status of {kind}"),
                    ));
                }
                let res = res.unwrap_or_else(|| {
                    if to == "done" && kind == "task" {
                        "completed".into()
                    } else {
                        "none".into()
                    }
                });
                if self.st.schema.value(&kind, "resolution", &res).is_none() {
                    return Err(Refusal::lq("E102", format!("{res} is not a resolution")));
                }
                let door = self.door.unwrap_or(Door::SetStatus);
                self.transit(n, &to, &res, door)?;
            }
            (None, Some(res)) if res != self.st.nodes[&n].resolution => {
                return Err(Refusal::lq(
                    "E404",
                    "a resolution is written with its status transition (GR-014)",
                ));
            }
            _ => {}
        }
        Ok(())
    }

    /// `patch` ([API §9.2]): the body with `remove`, which must occur exactly once ([LQ/std §7.2]), replaced by
    /// `add`.
    // spec: [LQ/std §7.2] PATCH
    pub fn patch(&mut self, target: &Target, remove: &str, add: &str) -> Res<()> {
        let n = self.resolve(target)?;
        self.lease_first_write(n)?;
        let body = self.st.nodes[&n].body.clone().unwrap_or_default();
        let remove = norm_text(remove);
        let i = self.patch_at(n, &body, &remove)?;
        let nb = format!(
            "{}{}{}",
            &body[..i],
            norm_text(add),
            &body[i + remove.len()..]
        );
        self.set_body(n, Some(&nb))
    }

    /// Where a `PATCH` applies ([LQ/std §7.2]; spec sync 3): the removed text must not be empty and must occur exactly
    /// once in the body, its occurrences being every byte offset where its bytes match, overlapping ones counted; else
    /// E404 with `occurrences` and the 1-based body `lines` of the occurrences (at most 10, none below two;
    /// [LQ/errors §5.5], §5.7).
    fn patch_at(&self, n: Nid, body: &str, remove: &str) -> Res<usize> {
        let refuse = |why: String, occ: Option<usize>, at: &[usize]| {
            let lines: Vec<Kv> = at
                .iter()
                .take(10)
                .map(|i| {
                    Kv::Int(
                        1 + body.as_bytes()[..*i]
                            .iter()
                            .filter(|b| **b == b'\n')
                            .count() as i64,
                    )
                })
                .collect();
            Refusal::lq("E404", format!("PATCH {n}.body refused: {why}"))
                .key("occurrences", occ.map_or(Kv::Null, |o| Kv::Int(o as i64)))
                .key(
                    "lines",
                    Kv::List(if at.len() >= 2 { lines } else { Vec::new() }),
                )
        };
        if remove.is_empty() {
            return Err(refuse("the removed text is empty".into(), None, &[]));
        }
        let (b, r) = (body.as_bytes(), remove.as_bytes());
        let at: Vec<usize> = (0..=b.len().saturating_sub(r.len()))
            .filter(|i| b.len() >= r.len() && &b[*i..*i + r.len()] == r)
            .collect();
        match at.as_slice() {
            [one] => Ok(*one),
            _ => Err(refuse(
                format!("the removed text occurs {} times", at.len()),
                Some(at.len()),
                &at,
            )),
        }
    }

    /// `link` ([API §9.2]): an edge with the write-time checks of [F08 §8.6] rule 5 and the role grant; a
    /// `supersedes` edge also moves the target to `superseded` (DR-006, I6).
    pub fn link(&mut self, src: Nid, kind: &str, dst: Nid, pinned: Option<&str>) -> Res<()> {
        let e = self
            .st
            .schema
            .edge(kind)
            .ok_or_else(|| Refusal::lq("E104", format!("unknown edge kind {kind}")))?
            .clone();
        if kind == "at" || kind == "mentions" {
            return Err(Refusal::lq(
                "E115",
                format!("{kind} edges are written by the link verbs or from text"),
            ));
        }
        if kind == "parent" {
            let p = dst;
            self.cx
                .rights
                .field(self.stmt, &self.scopes(), src, "parent")?;
            return self.place(src, Some(p), None);
        }
        // A pinned commit ([F08 §10]) is a property of the kinds whose props class is `pinned`.
        let pin = match pinned {
            Some(t) if e.props == crate::schema::Props::Pinned => {
                Some(self.commit_id("pinned", t)?)
            }
            Some(_) => {
                return Err(Refusal::lq(
                    "E115",
                    format!("{kind} edges carry no pinned commit"),
                ));
            }
            None => None,
        };
        if src == dst {
            return Err(e405(format!("a {kind} self-edge on {src}")));
        }
        let (sk, dk) = (
            self.st.nodes[&src].kind.clone(),
            self.st.nodes[&dst].kind.clone(),
        );
        if !e.src.allows(&sk) || !e.dst.allows(&dk) || (e.same_kind && sk != dk) {
            return Err(e405(format!("{kind} does not join {sk} to {dk} (I11)")));
        }
        self.cx
            .rights
            .edge(self.stmt, &self.scopes(), kind, "create", src, dst)?;
        let (s, d) = if e.symmetric && self.st.nodes[&dst].uid < self.st.nodes[&src].uid {
            (dst, src)
        } else {
            (src, dst)
        };
        let key = EdgeKey {
            kind: kind.to_string(),
            dst: d,
            disc: None,
        };
        let props = self
            .st
            .nodes
            .get_mut(&s)
            .expect("src")
            .out
            .entry(key)
            .or_default();
        if pin.is_some() {
            props.pinned = pin;
        }
        self.edges_from
            .entry(src)
            .or_default()
            .push(kind.to_string());
        self.touch(src);
        self.touch(dst);
        if kind == "supersedes" && self.st.nodes[&dst].status != "superseded" {
            self.transit(dst, "superseded", "none", Door::Supersede)?;
        }
        Ok(())
    }

    /// `unlink` ([API §9.2]): `MATCH … EXPECT 1 DELETE e`; a missing edge fails the EXPECT (E401). Removing a
    /// flagged edge is FL-006's drop.
    pub fn unlink(&mut self, src: &Target, kind: &str, dst: &Target) -> Res<()> {
        let s = match src {
            Target::Id(n) if self.st.nodes.get(n).is_some_and(|x| !x.live()) => *n,
            _ => self.resolve(src)?,
        };
        let d = self.resolve(dst)?;
        let symmetric = self.st.schema.edge(kind).is_some_and(|e| e.symmetric);
        let (s, d) = if symmetric && self.st.nodes[&d].uid < self.st.nodes[&s].uid {
            (d, s)
        } else {
            (s, d)
        };
        let key = EdgeKey {
            kind: kind.to_string(),
            dst: d,
            disc: None,
        };
        if kind == "parent" {
            return Err(Refusal::lq(
                "E115",
                "parent is detached with MOVE or SET parent = NULL",
            ));
        }
        if !self.st.nodes[&s].out.contains_key(&key) {
            return Err(self.e401(
                format!("no {kind} edge {s} -> {d} (EXPECT 1 matched 0)"),
                &[s, d],
            ));
        }
        self.cx
            .rights
            .edge(self.stmt, &self.scopes(), kind, "delete", s, d)?;
        self.st.nodes.get_mut(&s).expect("src").out.remove(&key);
        self.touch(s);
        self.touch(d);
        Ok(())
    }

    /// Removes one edge by its full key, an `at` edge's anchor included ([50 §3.10]: `DELETE a` of an `AT` edge variable
    /// removes that one anchor), under the edge rows of the role write policy. The edge is a value a statement bound
    /// on the candidate, so it is missing only when an earlier statement of the block removed it: a write to an edge
    /// that is not live, `not_found` as a write to a node that is not live is ([F19 §10.2]).
    // spec: [50 §3.10] DELETE edge
    pub fn unlink_key(&mut self, src: Nid, key: &EdgeKey) -> Res<()> {
        if key.kind == "parent" {
            return Err(Refusal::lq(
                "E115",
                "parent is detached with MOVE or SET parent = NULL",
            ));
        }
        if !self
            .st
            .nodes
            .get(&src)
            .is_some_and(|x| x.out.contains_key(key))
        {
            let edge = format!("{src} -[:{}]-> {}", key.kind, key.dst);
            return Err(Refusal::new(
                "not_found",
                3,
                format!("edge {edge} is not live on {}", self.cx.branch),
            )
            .key("what", "edge")
            .key("value", edge));
        }
        self.cx
            .rights
            .edge(self.stmt, &self.scopes(), &key.kind, "delete", src, key.dst)?;
        self.st.nodes.get_mut(&src).expect("src").out.remove(key);
        self.touch(src);
        self.touch(key.dst);
        Ok(())
    }

    /// Records the targets a statement bound ([API §3.3] `statements[].targets`).
    pub fn record_targets(&mut self, stmt: usize, nodes: &[Nid]) {
        self.targets
            .entry(stmt)
            .or_default()
            .extend(nodes.iter().copied());
    }

    /// `move` ([API §9.2], §9.5).
    pub fn move_(
        &mut self,
        target: &Target,
        under: Option<&Target>,
        pos: Option<&Position>,
    ) -> Res<()> {
        let n = self.resolve(target)?;
        let p = under.map(|u| self.resolve(u)).transpose()?;
        self.cx
            .rights
            .field(self.stmt, &self.scopes(), n, "parent")?;
        self.place(n, p, pos)?;
        self.touch(n);
        Ok(())
    }

    /// `reopen` ([API §9.2]; DR-005, GR-011): back to `open` through the `reopen` door; a task's `reopen_count` grows by 1.
    // rule: GR-011
    pub fn reopen(&mut self, target: &Target, reason: &str) -> Res<()> {
        let n = self.resolve(target)?;
        if reason.trim().is_empty() {
            return Err(Refusal::lq("E404", "REOPEN needs a reason (DR-005)"));
        }
        self.cx.rights.statement(self.stmt, "reopen")?;
        self.transit(n, "open", "none", Door::Reopen)?;
        if self.st.nodes[&n].kind == "task" {
            let cur = match self.st.nodes[&n].fields.get("reopen_count") {
                Some(Value::Counter(v)) => *v,
                _ => 0,
            };
            let schema = self.st.schema.clone();
            self.st.nodes.get_mut(&n).expect("node").set_field(
                &schema,
                "reopen_count",
                Some(Value::Counter(cur + 1)),
            );
        }
        Ok(())
    }

    /// `delete` ([API §9.2]; [RULES/delete-policy-matrix] DS rows): the preconditions in DP order, the matrix, the
    /// tombstones, the lease release under `RELEASE` (DS-007, LE-007).
    // rule: DP-001, DP-005, DS-007, LE-007
    pub fn delete(
        &mut self,
        target: &Target,
        policy: Option<&str>,
        replaced_by: Option<&Target>,
        release: bool,
        reason: Option<&str>,
    ) -> Res<()> {
        self.cx.rights.statement(self.stmt, "node-delete")?;
        let n = self.resolve(target)?;
        let y = replaced_by.map(|t| self.resolve(t)).transpose()?;
        let opts = DeleteOpts {
            replaced_by: y,
            cascade: policy == Some("cascade"),
            reparent: policy == Some("reparent"),
            reassign: policy == Some("reassign"),
            release,
            reason: reason.map(str::to_string),
        };
        // DP-005, between DP-010 and DP-006: a live lease on the deleted set without RELEASE. The `leases` key lists
        // the lease the text names, then the others its detail lines list, in `LEASES` order ([LQ/errors §5.7]).
        let (leases, env) = (&self.leases, self.cx.env);
        let lease_check = |set: &[Nid]| -> Option<Refusal> {
            if release {
                return None;
            }
            let first = lease::i32p_rm_refused_under_lease(leases.values(), env, set)?;
            let mut live: Vec<&Lease> = leases
                .values()
                .filter(|l| {
                    l.task.is_some_and(|t| set.contains(&t)) && lease::is_live(l, env).is_live()
                })
                .collect();
            live.sort_by_key(|l| (l.task, l.id));
            let obj = |l: &Lease| {
                Kv::Obj(vec![
                    ("node".into(), l.task.map_or(Kv::Null, Kv::Node)),
                    ("id".into(), Kv::Str(format!("L-{}", l.id))),
                    ("holder".into(), Kv::Str(l.holder.clone())),
                    ("branch".into(), Kv::Str(l.branch.clone())),
                ])
            };
            Some(
                Refusal::lq(
                    "E409",
                    format!(
                        "leased by {} on {} (L-{})",
                        first.holder, first.branch, first.id
                    ),
                )
                .key(
                    "leases",
                    Kv::List(live.into_iter().take(11).map(obj).collect()),
                ),
            )
        };
        let plan = delete::plan(
            &self.st,
            self.cx.view.token(),
            n,
            &opts,
            // The policy data `edges.<kind>.on-src-deleted` of the view ([CFG §10.13]).
            &EdgePolicies::of(&self.st.schema),
            &lease_check,
        )?;
        if release {
            let ids: Vec<u64> = self
                .leases
                .values()
                .filter(|l| {
                    l.task.is_some_and(|t| plan.set.contains(&t))
                        && lease::is_live(l, self.cx.env).is_live()
                })
                .map(|l| l.id)
                .collect();
            for id in ids {
                self.end_lease(id, EndReason::RmRelease);
            }
        }
        let fx = delete::apply(&mut self.st, &plan, &opts);
        self.notified.extend(fx.notified);
        for n in &plan.set {
            self.touch(*n);
        }
        for (m, _) in &plan.edges {
            self.touch(m.src);
            self.touch(m.dst);
        }
        Ok(())
    }

    /// `resolve` ([API §9.2]; [F12 §6.5]): on a work or plan branch a flagged edge's key re-points or drops it
    /// (FL-005, FL-006); a conflict key takes a side's value or a given value; on a staging ref, a key of G's staged
    /// violations takes a value too ([`Cand::resolve_violation`]). `drop` is usage on every key but a flagged edge. Any
    /// other key that parses is `not_found` ([F12 §6.6]). It is the explicit door that settles a file node's
    /// `DeleteVsModify`, which has no automatic policy ([RULES/link-merge-rules] LV-009).
    // rule: FL-007, LV-009
    pub fn resolve_key(&mut self, key: &str, take: &Take) -> Res<()> {
        self.cx.rights.statement(self.stmt, "resolve")?;
        let parsed = self.parse_key(key);
        if let Some(Key::Node(s, Aspect::Edge(ek))) = &parsed
            && matches!(self.cx.view, RefKind::Work | RefKind::Plan)
        {
            let flagged = self
                .st
                .nodes
                .get(s)
                .and_then(|x| x.out.get(ek))
                .is_some_and(|p| p.flagged);
            if flagged {
                return self.resolve_flagged_key(*s, ek, take);
            }
        }
        if *take == Take::Drop {
            return Err(Refusal::usage(format!(
                "{key} is not a flagged edge: drop takes a flagged edge only"
            )));
        }
        if let Some(Key::Node(n, a)) = &parsed
            && let Some(c) = self.st.nodes.get(n).and_then(|x| x.conflicts.get(a))
        {
            let c = c.clone();
            return self.resolve_conflict(key, *n, a, &c, take);
        }
        if self.cx.view == RefKind::Merge
            && let Some((s, k)) = self.staged_violation(key, parsed.as_ref())
        {
            return self.resolve_violation(s, &k, take);
        }
        Err(Refusal::not_found("conflict key", key))
    }

    /// A key text of [F12 §6.6] as a node or edge key of the view: `#` optional; `order` names the hierarchy key,
    /// `resolution` the status key, and an artifact's `path`, `oid`, `bytes`, `observed_git`, `observed_blob` and
    /// `relink` its observation key. `None` for a schema or query key and for a text that is no key.
    fn parse_key(&self, key: &str) -> Option<Key> {
        let node = |t: &str| match t.strip_prefix('#') {
            Some(_) => parse_node(t),
            None => parse_node(&format!("#{t}")),
        };
        if let Some(rest) = key.strip_prefix("edge:") {
            let parts: Vec<&str> = rest.split(':').collect();
            let [s, k, d] = parts.as_slice() else {
                return None;
            };
            let (Some(Target::Id(s)), Some(Target::Id(d))) = (node(s), node(d)) else {
                return None;
            };
            return Some(Key::Node(
                s,
                Aspect::Edge(EdgeKey {
                    kind: k.to_string(),
                    dst: d,
                    disc: None,
                }),
            ));
        }
        let (id, aspect) = key.split_once('.')?;
        let Some(Target::Id(n)) = node(id) else {
            return None;
        };
        let x = self.st.nodes.get(&n);
        let a = match aspect {
            "existence" => Aspect::Existence,
            "status" | "resolution" => Aspect::Status,
            "parent" | "order" => Aspect::Hierarchy,
            "body" => Aspect::Body,
            "observation" => Aspect::Observation,
            f if x.is_some_and(|x| x.kind == "artifact") && OBSERVATION.contains(&f) => {
                Aspect::Observation
            }
            f => {
                let counter = x.is_some_and(|x| {
                    self.st
                        .schema
                        .field(&x.kind, f)
                        .is_some_and(|fi| fi.ty == Ty::Counter)
                });
                if counter {
                    Aspect::Counter(f.into())
                } else {
                    Aspect::Field(f.into())
                }
            }
        };
        Some(Key::Node(n, a))
    }

    /// FL-005, FL-006 ([F12 §6.5] "A flagged edge"): `repoint` replaces a flagged edge by an edge of its kind from the
    /// target to the same destination, without the flag (the write path checks I5′ on it); `drop` removes it. `ours`,
    /// `theirs`, `base` and `value` are usage there.
    // rule: FL-005, FL-006
    fn resolve_flagged_key(&mut self, s: Nid, ek: &EdgeKey, take: &Take) -> Res<()> {
        let repoint = match take {
            Take::Repoint(t) => Some(self.resolve(t)?),
            Take::Drop => None,
            _ => {
                return Err(Refusal::usage(format!(
                    "edge:{s}:{}:{} is a flagged edge: resolve it with repoint or drop",
                    ek.kind, ek.dst
                )));
            }
        };
        if let Some(y) = repoint {
            self.cx
                .rights
                .edge(self.stmt, &self.scopes(), &ek.kind, "create", y, ek.dst)?;
        }
        delete::resolve_flagged(&mut self.st, s, ek, repoint)?;
        self.resolves.insert(Key::Node(s, Aspect::Edge(ek.clone())));
        if let Some(y) = repoint {
            self.companions
                .insert(Key::Node(y, Aspect::Edge(ek.clone())));
        }
        self.touch(ek.dst);
        if let Some(y) = repoint {
            self.touch(y);
        }
        self.check_depth(ek.dst)
    }

    /// A conflict value takes its `ours`, `theirs` or `base` side, or a given value of a field or counter ([F12 §6.5]).
    /// A `SupersedeFork` refuses `value` (usage); its `theirs` keeps the edge and removes, by `RemoveEdge`, every
    /// other active `supersedes` edge to the target (I6). A `live` existence side restores the node's value keys from
    /// its node image, and for `ours` and `theirs` its hierarchy key and out-edges from that side's state at the
    /// conflict's introducing commit ([`Cand::restore_live`]).
    fn resolve_conflict(
        &mut self,
        key: &str,
        n: Nid,
        a: &Aspect,
        c: &Conflict,
        take: &Take,
    ) -> Res<()> {
        if c.class == "SupersedeFork" && matches!(take, Take::Value(_)) {
            return Err(Refusal::usage(format!(
                "{key} is a SupersedeFork: it takes ours, theirs or base, not a value"
            )));
        }
        let v = match take {
            Take::Ours => c.ours.clone(),
            Take::Theirs => c.theirs.clone(),
            Take::Base => c.base.clone(),
            Take::Value(p) => {
                let (Aspect::Field(f) | Aspect::Counter(f)) = a else {
                    return Err(Refusal::usage(format!("{key} takes a side, not a value")));
                };
                let kind = self.st.nodes[&n].kind.clone();
                convert(
                    &self.st.schema,
                    &kind,
                    f,
                    p,
                    self.cx.uidx,
                    self.next_id,
                    &|t| self.commit_id(f, t),
                )?
                .map(KVal::Value)
            }
            Take::Repoint(_) | Take::Drop => {
                return Err(Refusal::usage(format!("{key} is not a flagged edge")));
            }
        };
        let k = Key::Node(n, a.clone());
        let live = *a == Aspect::Existence && matches!(v, Some(KVal::Live(_)));
        self.set_key(&k, v);
        self.resolves.insert(k);
        self.touch(n);
        if live {
            let side = match take {
                Take::Ours => 1,
                Take::Theirs => 2,
                _ => 0,
            };
            self.restore_live(n, c, side)?;
        }
        if c.class == "SupersedeFork"
            && *take == Take::Theirs
            && let Aspect::Edge(ek) = a
        {
            self.supersede_fork_theirs(n, ek);
        }
        Ok(())
    }

    /// [F12 §6.5] "A `live` existence side": the node's value keys from the side's node image (`snap` = 1), and for
    /// `ours` (`side` 1) and `theirs` (2) its hierarchy key and out-edges from `state(first parent of M)` and
    /// `state(second parent of M)`, M being the conflict's introducing commit, as companion `Move`, `AddEdge` and
    /// `SetEdgeProps` ops. A parent that is not live on the view refuses as a `MOVE` under it does (`not_found`); a
    /// structural edge to a deleted target is the deferred checks' (E405); I4 on the restored node follows VO-3
    /// ([`Cand::check_forest`]).
    fn restore_live(&mut self, n: Nid, c: &Conflict, side: usize) -> Res<()> {
        let schema = self.st.schema.clone();
        if let Some(img) = &c.images[side] {
            let x = self.st.nodes.get_mut(&n).expect("the restored node");
            let held: Vec<Aspect> = x
                .aspects(x.kind == "artifact")
                .into_iter()
                .filter(|a| {
                    matches!(
                        a,
                        Aspect::Status
                            | Aspect::Field(_)
                            | Aspect::Counter(_)
                            | Aspect::Body
                            | Aspect::Observation
                    )
                })
                .collect();
            for a in held {
                if !img.contains_key(&a) {
                    x.put_value(&schema, &a, None);
                }
            }
            for (a, v) in img {
                x.put_value(&schema, a, Some(v.clone()));
            }
        }
        if side == 0 {
            return Ok(());
        }
        let Some(m) = self.introducing(n, &Aspect::Existence, c) else {
            return Ok(());
        };
        let Some(p) = self.cx.dag.commits[&m].parents.get(side - 1).copied() else {
            return Ok(());
        };
        let from = self.cx.dag.state_at(Some(p), self.cx.alloc);
        let Some(y) = from.nodes.get(&n) else {
            return Ok(());
        };
        let (parent, order, out) = (y.parent, y.order.clone(), y.out.clone());
        if let Some(q) = parent
            && self.st.live(q).is_none()
        {
            return Err(Refusal::new(
                "not_found",
                3,
                format!("node {q} is not live on {}", self.cx.branch),
            )
            .key("what", "node")
            .key("value", q.to_string()));
        }
        let x = self.st.nodes.get_mut(&n).expect("the restored node");
        if (x.parent, &x.order) != (parent, &order) {
            x.parent = parent;
            x.order = order;
            self.companions.insert(Key::Node(n, Aspect::Hierarchy));
        }
        for (ek, props) in out {
            if x.out.get(&ek) != Some(&props) {
                x.out.insert(ek.clone(), props);
                self.companions
                    .insert(Key::Node(n, Aspect::Edge(ek.clone())));
                self.notified.insert(ek.dst);
            }
        }
        self.check_forest(n, &std::cell::OnceCell::new())
    }

    /// M of [F12 §6.5]: the commit on the view's first-parent chain whose `Conflict` op set the key's conflict value
    /// (`CONFLICTS.commit`, [F11 §10]) — the oldest commit of the chain from the tip that still holds it.
    fn introducing(&self, n: Nid, a: &Aspect, c: &Conflict) -> Option<u64> {
        let dag = self.cx.dag;
        let mut at = dag.live(&self.cx.branch).and_then(|r| r.tip)?;
        let holds = |seq: Option<u64>| {
            seq.is_some_and(|q| {
                dag.state_at(Some(q), self.cx.alloc)
                    .nodes
                    .get(&n)
                    .and_then(|x| x.conflicts.get(a))
                    == Some(c)
            })
        };
        if !holds(Some(at)) {
            return None;
        }
        while let Some(p) = dag.commits[&at].parents.first().copied() {
            if !holds(Some(p)) {
                break;
            }
            at = p;
        }
        Some(at)
    }

    /// `SupersedeFork` resolved `theirs` ([F12 §6.5]): S's edge stays and every other active `supersedes` edge to the
    /// target goes, by `RemoveEdge` in the same commit, so at most one remains (I6).
    fn supersede_fork_theirs(&mut self, s: Nid, ek: &EdgeKey) {
        let others: Vec<(Nid, EdgeKey)> = self
            .st
            .nodes
            .iter()
            .filter(|(m, x)| **m != s && x.live())
            .flat_map(|(m, x)| {
                x.out
                    .keys()
                    .filter(|k| k.kind == "supersedes" && k.dst == ek.dst)
                    .map(|k| (*m, k.clone()))
                    .collect::<Vec<_>>()
            })
            .filter(|(m, _)| {
                self.st.live(*m).is_some_and(|x| {
                    self.st
                        .schema
                        .value(&x.kind, "status", &x.status)
                        .is_some_and(|e| !e.side)
                })
            })
            .collect();
        for (m, k) in others {
            if let Some(x) = self.st.nodes.get_mut(&m) {
                x.out.remove(&k);
            }
            self.companions.insert(Key::Node(m, Aspect::Edge(k)));
            self.touch(m);
        }
        self.touch(ek.dst);
    }

    /// [F12 §9.3]: a commit on a staging ref after its staged commit holds only `Resolve` ops and the companion ops
    /// §6.5 puts in the same commit — a live restore's `Move`, `AddEdge` and `SetEdgeProps` (with the value keys its
    /// node image restores), a `repoint`'s `AddEdge`, a `SupersedeFork`'s `RemoveEdge`; any other key of the net
    /// changeset is E305.
    pub fn staging_ops(&self, cs: &crate::state::Changeset) -> Res<()> {
        if self.cx.view != RefKind::Merge {
            return Ok(());
        }
        let uid = |n: Nid| self.cx.alloc.uid(n);
        for k in cs.keys() {
            let restored = matches!(k, Key::Node(n, _)
                if self.resolves.contains(&Key::Node(*n, Aspect::Existence)));
            if !(self.resolves.contains(k) || self.companions.contains(k) || restored) {
                return Err(Refusal::lq(
                    "E305",
                    format!(
                        "{} is neither a Resolve nor its companion: a staging ref takes only those ([F12 §9.3])",
                        crate::merge::key_text(k, &uid)
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Sets one key of the candidate to a plain value, as a one-key changeset.
    fn set_key(&mut self, k: &Key, v: Option<KVal>) {
        let now = self.st.kstate(k);
        let want = KState::Plain(v);
        if now == want {
            return;
        }
        let mut cs = BTreeMap::new();
        cs.insert(k.clone(), (now, want));
        let alloc = self.cx.alloc;
        self.st.apply(&cs, alloc);
    }

    /// The newest staged commit of the staging ref written, and the key of its violations that `text` names: the same
    /// node key, the same edge by source, kind and destination, or the same key text ([F12 §6.6]).
    fn staged_violation(&self, text: &str, parsed: Option<&Key>) -> Option<(u64, Key)> {
        let g = self.cx.dag.live(&self.cx.branch)?;
        let s = crate::history::newest_staged(self.cx.dag, g.id)?;
        let uid = |n: Nid| self.cx.alloc.uid(n);
        let k = self.cx.dag.commits[&s]
            .violations
            .iter()
            .filter_map(|v| v.key.as_ref())
            .find(|k| match (k, parsed) {
                (Key::Node(a, Aspect::Edge(x)), Some(Key::Node(b, Aspect::Edge(y)))) => {
                    a == b && x.kind == y.kind && x.dst == y.dst
                }
                (k, Some(p)) => *k == p,
                (k, None) => crate::merge::key_text(k, &uid) == text,
            })?;
        Some((s, k.clone()))
    }

    /// `resolve` on a key of the violations of G's newest staged commit `s` ([F12 §6.5] "On a violation's key";
    /// [F06 §7.7] choices 0–4): `ours`, `theirs` and `base` take the key's value in that side of the operation `s`
    /// stands for ([`crate::history::staged_side`]); `value` gives a field or counter a value checked against its type
    /// ([F08 §8.6]); `repoint` takes an edge key only ([`Cand::repoint_violation`]). The violation is not resolved by
    /// the write: `merge --continue` overlays the key and re-checks it ([F12 §9.4] steps 2–3), so the key is recorded
    /// also when its value stays as it was.
    fn resolve_violation(&mut self, s: u64, k: &Key, take: &Take) -> Res<()> {
        let uid = |n: Nid| self.cx.alloc.uid(n);
        let text = crate::merge::key_text(k, &uid);
        let v = match take {
            Take::Ours | Take::Theirs | Take::Base => {
                let side = match take {
                    Take::Base => 0,
                    Take::Ours => 1,
                    _ => 2,
                };
                let st =
                    crate::history::staged_side(self.cx.dag, self.cx.alloc, self.cx.uidx, s, side);
                match k {
                    Key::Node(n, a) => crate::merge::flat(&crate::merge::cval(&st, *n, a)),
                    Key::Schema(_) => crate::merge::flat(&st.kstate(k)),
                }
            }
            Take::Value(p) => {
                let Key::Node(n, Aspect::Field(f) | Aspect::Counter(f)) = k else {
                    return Err(Refusal::usage(format!("{text} takes a side, not a value")));
                };
                let kind = self
                    .st
                    .nodes
                    .get(n)
                    .map(|x| x.kind.clone())
                    .ok_or_else(|| self.not_on_view(*n))?;
                convert(
                    &self.st.schema,
                    &kind,
                    f,
                    p,
                    self.cx.uidx,
                    self.next_id,
                    &|t| self.commit_id(f, t),
                )?
                .map(KVal::Value)
            }
            Take::Repoint(t) => {
                let Key::Node(src, Aspect::Edge(ek)) = k else {
                    return Err(Refusal::usage(format!("{text} is not an edge key")));
                };
                return self.repoint_violation(k, *src, ek, t);
            }
            Take::Drop => {
                return Err(Refusal::usage(format!("{text} is not a flagged edge")));
            }
        };
        // A value goes only where the view can hold it: a node it has, and on a tombstone only the keys a tombstone
        // holds — its existence, its title and its retained out-edges ([F07 §6.4]; the WP-91 closure's P3), as a
        // plain `SET` on a node that is not live is `not_found`.
        if let Key::Node(n, a) = k
            && v.is_some()
        {
            match self.st.nodes.get(n) {
                None => return Err(self.not_on_view(*n)),
                Some(x)
                    if !x.live()
                        && !matches!(a, Aspect::Existence | Aspect::Edge(_))
                        && *a != Aspect::Field("title".into()) =>
                {
                    return Err(Refusal::new(
                        "not_found",
                        3,
                        format!("node {n} is not live on {}", self.cx.branch),
                    )
                    .key("what", "node")
                    .key("value", n.to_string()));
                }
                Some(_) => {}
            }
        }
        self.set_key(k, v);
        self.resolves.insert(k.clone());
        if let Key::Node(n, a) = k {
            self.touch(*n);
            if let Aspect::Edge(ek) = a {
                self.touch(ek.dst);
            }
        }
        Ok(())
    }

    /// `not_found` for a node the view does not hold.
    fn not_on_view(&self, n: Nid) -> Refusal {
        Refusal::new(
            "not_found",
            3,
            format!("node {n} is not on {}", self.cx.branch),
        )
        .key("what", "node")
        .key("value", n.to_string())
    }

    /// `repoint` of a violation's edge key ([F06 §7.7] choice 4): the edge `src → ek.dst` becomes absent, and an edge
    /// of its kind and properties, unflagged, goes to the target in place of its endpoint that is not live on the view
    /// (the destination when both are live).
    fn repoint_violation(&mut self, k: &Key, src: Nid, ek: &EdgeKey, t: &Target) -> Res<()> {
        let y = self.resolve(t)?;
        let props = self
            .st
            .nodes
            .get(&src)
            .and_then(|x| x.out.get(ek))
            .cloned()
            .ok_or_else(|| {
                Refusal::not_found("conflict key", format!("edge:{src}:{}:{}", ek.kind, ek.dst))
            })?;
        let (from, to) = if self.st.live(ek.dst).is_none() || self.st.live(src).is_some() {
            (src, y)
        } else {
            (y, ek.dst)
        };
        self.cx
            .rights
            .edge(self.stmt, &self.scopes(), &ek.kind, "create", from, to)?;
        if let Some(x) = self.st.nodes.get_mut(&src) {
            x.out.remove(ek);
        }
        let added = EdgeKey {
            kind: ek.kind.clone(),
            dst: to,
            disc: ek.disc,
        };
        if let Some(x) = self.st.nodes.get_mut(&from) {
            x.out.insert(
                added.clone(),
                EdgeProps {
                    flagged: false,
                    ..props
                },
            );
        }
        self.companions.insert(Key::Node(from, Aspect::Edge(added)));
        self.resolves.insert(k.clone());
        for n in [src, ek.dst, y] {
            self.touch(n);
        }
        Ok(())
    }

    /// Ends a lease in the candidate with its reason ([RULES/state-definition] `lease-ends`).
    // spec: [RULES/state-definition] lease-ends
    pub fn end_lease(&mut self, id: u64, reason: EndReason) {
        if let Some(l) = self.leases.get_mut(&id)
            && l.ended.is_none()
        {
            l.ended = Some(reason);
            self.events.push(LeaseEvent::End { id, reason });
        }
    }

    /// The presented lease row, with I17′'s check: it exists, has not ended and is live; else E407.
    // spec: [API §10.6] LP-1
    // rule: WR-003
    pub fn presented(&self) -> Res<Option<Lease>> {
        let Some(p) = self.cx.rights.lease.as_ref() else {
            return Ok(None);
        };
        let l = self
            .leases
            .get(&p.id)
            .filter(|l| lease::i17p_fencing(l, p.id) && lease::is_live(l, self.cx.env).is_live())
            .ok_or_else(|| self.e407(p.id, format!("lease L-{} is lost", p.id)))?;
        Ok(Some(l.clone()))
    }

    /// LP-2: a write that presents a TTL lease due for renewal renews it (a lazy record of cause 2).
    // spec: [API §10.6] LP-2
    // rule: LE-011
    pub fn renew_by_use(&mut self) {
        let Some(p) = self.cx.rights.lease.clone() else {
            return;
        };
        let now = self.cx.env.now();
        if let Some(l) = self.leases.get_mut(&p.id)
            && l.ended.is_none()
            && !l.run_scoped
            && due_for_renewal(l.expires, l.ttl_ms, now)
        {
            l.expires = after(now, l.ttl_ms);
            self.events.push(LeaseEvent::Renew {
                id: p.id,
                durable: false,
            });
        }
    }

    /// The anchor of a new lease for the caller's session ([API §6.3] SL-1).
    // spec: [API §6.3] SL-1
    fn anchor(&self) -> (AnchorKind, Option<String>) {
        match &self.cx.session {
            Some(s) if self.cx.env.holds_slot(s) && s.starts_with("claude:") => {
                (AnchorKind::Session, Some(s.clone()))
            }
            Some(s) if self.cx.env.holds_slot(s) && s.starts_with("codex:") => {
                (AnchorKind::SessionTtl, Some(s.clone()))
            }
            _ => (AnchorKind::None, None),
        }
    }

    /// Grants a new lease ([API §10.1] "Each new lease"): `lease_id` = token = `fence + 1`.
    #[allow(clippy::too_many_arguments)]
    fn grant(
        &mut self,
        task: Option<Nid>,
        role: &str,
        holder: &str,
        ttl: Option<u64>,
        run: Option<Nid>,
        session_role: bool,
    ) -> u64 {
        self.fence += 1;
        let id = self.fence;
        let now = self.cx.env.now();
        let (anchor, session) = self.anchor();
        let run_scoped = ttl.is_none();
        let files = task
            .and_then(|t| self.st.nodes.get(&t))
            .and_then(|x| match x.fields.get("files_owned") {
                Some(Value::Set(v)) => Some(
                    v.iter()
                        .filter_map(|e| e.as_str().map(str::to_string))
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default();
        let l = Lease {
            id,
            token: id,
            task,
            kind: if task.is_some() {
                LeaseKind::Task
            } else {
                LeaseKind::Role
            },
            role: role.to_string(),
            holder: holder.to_string(),
            branch: self.cx.branch.clone(),
            anchor,
            session,
            anchor_boot_hash: now.boot_hash,
            expires: lease::deadline(now, ttl),
            ttl_ms: ttl.unwrap_or(0),
            run,
            run_scoped,
            session_role,
            // The `append_hlc` of the grant's own `Lease` record, drawn when the group is appended ([F11 §6] field
            // 36); the write path sets it.
            claimed_hlc: 0,
            bound: if session_role {
                self.cx.thread_hash
            } else {
                None
            },
            root_session: if holder == self.cx.actor && holder.starts_with("codex:") {
                self.cx.root_session
            } else {
                None
            },
            files_owned: files,
            ended: None,
        };
        self.leases.insert(id, l);
        self.events.push(LeaseEvent::Grant { id, reused: false });
        id
    }

    /// The run node named `name` on the view (LP-5): a live `run` whose title is the name.
    // spec: [API §10.6] LP-5
    pub fn run_named(&self, name: &str) -> Res<Nid> {
        self.st
            .nodes
            .iter()
            .find(|(_, x)| x.live() && x.kind == "run" && x.text("title") == Some(name))
            .map(|(n, _)| *n)
            .ok_or_else(|| Refusal::not_found("run", name))
    }

    fn lease_row(&self, id: u64) -> Vec<(String, String)> {
        let l = &self.leases[&id];
        vec![
            ("lease".into(), format!("L-{id}")),
            ("token".into(), l.token.to_string()),
            ("branch".into(), l.branch.clone()),
            (
                "expires".into(),
                format!(
                    "{}/{}/{}",
                    l.expires.wall, l.expires.boot_hash, l.expires.boot_ns
                ),
            ),
            (
                "task".into(),
                l.task.map_or("null".into(), |t| t.to_string()),
            ),
            ("role".into(), l.role.clone()),
            ("holder".into(), l.holder.clone()),
            ("anchor".into(), l.anchor.name().into()),
            (
                "run".into(),
                l.run.map_or("null".into(), |r| {
                    self.st
                        .nodes
                        .get(&r)
                        .and_then(|x| x.text("title"))
                        .unwrap_or("")
                        .to_string()
                }),
            ),
        ]
    }

    /// `fits_role(t, role)` ([LQ/std §2.6]): the role may complete the task — some `role-status` row lets it reach `done`
    /// through `tx-complete` on a task.
    fn fits_role(role: &str) -> bool {
        crate::rules::rules()
            .table("role-status")
            .rows
            .iter()
            .any(|r| {
                r.tok("role") == role
                    && (r.tok("kind") == "task" || r.tok("kind") == "*")
                    && (r.tok("to") == "done" || r.tok("to") == "*")
            })
    }

    /// `tx.claim` ([API §10.1]).
    // spec: [API §10.1]
    #[allow(clippy::too_many_arguments)]
    pub fn claim(
        &mut self,
        ids: &[Target],
        next: bool,
        scope: Option<&Target>,
        role: Option<&str>,
        agent: Option<&str>,
        ttl: Option<&P>,
        start: bool,
        run: Option<&str>,
        session: bool,
    ) -> Res<()> {
        status::branch_mask(self.cx.view.token()).map_err(|e| Refusal::lq("E305", e.detail))?;
        let task_claim = !ids.is_empty() || next;
        let mint = !task_claim && role.is_some() && (run.is_some() != session);
        if (!ids.is_empty() && next) || (task_claim && session) || (!task_claim && !mint) {
            return Err(Refusal::usage(
                "a claim names ids, or next, or mints a role lease with run or session (open point 41)",
            ));
        }
        let holder = agent
            .map(str::to_string)
            .unwrap_or_else(|| self.cx.actor.clone());
        let run_id = run.map(|r| self.run_named(r)).transpose()?;
        // `ttl`: a duration, or `run` for a run-scoped lease ([API §10.1]).
        let parse_ttl = |t: &P| -> Res<Option<u64>> {
            if *t == P::Text("run".into()) {
                return Ok(None);
            }
            duration_arg("ttl", t).map(Some)
        };
        let mut rows = Vec::new();
        if !task_claim {
            let role = role.expect("a role-lease mint names a role");
            let ttl_ms = if session {
                if role != "orchestrator" {
                    return Err(Refusal::usage(
                        "the session role lease is the orchestrator's",
                    ));
                }
                if self.cx.subagent_or_worker {
                    return Err(Refusal::lq(
                        "E406",
                        "a known subagent or dispatched worker mints no session lease (WM-005)",
                    ));
                }
                self.cx.rights.mint("session-role-lease")?;
                lease::ttl_for(
                    lease::ClaimShape::SessionRole,
                    ttl.map(parse_ttl).transpose()?,
                    self.cx.cfg.ttl_default_ms,
                    self.cx.cfg.orchestrator_ttl_ms,
                )
            } else {
                if role == "orchestrator" || role == "owner" {
                    return Err(Refusal::lq(
                        "E406",
                        "a run-scoped role lease is not orchestrator or owner (WM-004)",
                    ));
                }
                self.cx.rights.mint("run-role-lease")?;
                lease::ttl_for(
                    lease::ClaimShape::RunRole,
                    ttl.map(parse_ttl).transpose()?,
                    self.cx.cfg.ttl_default_ms,
                    self.cx.cfg.orchestrator_ttl_ms,
                )
            };
            let id = self.grant(None, role, &holder, ttl_ms, run_id, session);
            let mut r = self.lease_row(id);
            r.push(("reused".into(), "false".into()));
            rows.push(r);
        } else {
            let self_claim_roles = self.cx.rights.data.self_claim_roles.clone();
            let lease_role = role
                .map(str::to_string)
                .unwrap_or_else(|| "developer".into());
            if ids.len() > 1 || run.is_some() {
                self.cx.rights.mint("bulk-claim")?;
            } else if !self_claim_roles.contains(&lease_role) {
                self.cx.rights.mint("claim-other-role")?;
            } else {
                self.cx.rights.mint("task-self-claim")?;
            }
            let ttl_ms = lease::ttl_for(
                if run.is_some() {
                    lease::ClaimShape::TaskOfRun
                } else {
                    lease::ClaimShape::Task
                },
                ttl.map(parse_ttl).transpose()?,
                self.cx.cfg.ttl_default_ms,
                self.cx.cfg.orchestrator_ttl_ms,
            );
            let targets: Vec<Nid> = if next {
                let scope_n = scope.map(|s| self.resolve(s)).transpose()?;
                match self.next_ready(scope_n, &lease_role, &holder) {
                    Some(t) => vec![t],
                    None => Vec::new(),
                }
            } else {
                let mut v = Vec::new();
                for t in ids {
                    v.push(self.resolve(t)?);
                }
                v
            };
            for t in targets {
                if self.st.nodes[&t].kind != "task" {
                    return Err(Refusal::lq("E404", format!("{t} is not a task")));
                }
                if let Some(existing) = self
                    .leases
                    .values()
                    .find(|l| {
                        l.task == Some(t)
                            && l.holder == holder
                            && lease::is_live(l, self.cx.env).is_live()
                    })
                    .map(|l| l.id)
                {
                    self.events.push(LeaseEvent::Grant {
                        id: existing,
                        reused: true,
                    });
                    let mut r = self.lease_row(existing);
                    r.push(("reused".into(), "true".into()));
                    rows.push(r);
                    continue;
                }
                self.ready_for_claim(t, &holder)?;
                self.end_superseded(t);
                let id = self.grant(Some(t), &lease_role, &holder, ttl_ms, run_id, false);
                if start {
                    let p = crate::policy::Presented {
                        id,
                        task: Some(t),
                        run: run_id,
                        role: lease_role.clone(),
                        holder: holder.clone(),
                        session_role: false,
                    };
                    let saved = self.cx.rights.lease.replace(p);
                    let saved_role = std::mem::replace(
                        &mut self.cx.rights.role,
                        crate::policy::effective_role(self.cx.rights.lease.as_ref(), false),
                    );
                    let r = self.transit(t, "in_progress", "none", Door::ClaimStart);
                    self.cx.rights.lease = saved;
                    self.cx.rights.role = saved_role;
                    r?;
                }
                let mut r = self.lease_row(id);
                r.push(("reused".into(), "false".into()));
                rows.push(r);
            }
        }
        self.yields.push(Yield {
            index: self.stmt,
            proc: "tx.claim".into(),
            rows,
        });
        Ok(())
    }

    /// LE-012: a claim that grants a new lease on `#N` first ends every task lease on `#N` that has not ended and is not
    /// live — expired by its deadline, or with a Dead anchor — with reason 4 ([F05 §9.4]: "its anchor Dead, its
    /// deadline passed or its boot changed"), in the claim's group before the grant. So the holder of an expired lease
    /// can no longer renew it once another holder claimed the task (LE-011; [API §10.2] "that no one reclaimed"), a
    /// Dead anchor that lives again never gives the task a second live lease (LE-009), and `#N` never has two task
    /// leases that have not ended.
    // rule: LE-012
    fn end_superseded(&mut self, t: Nid) {
        let env = self.cx.env;
        let ids: Vec<u64> = self
            .leases
            .values()
            .filter(|l| l.kind == lease::LeaseKind::Task && l.task == Some(t) && l.ended.is_none())
            .filter(|l| !lease::is_live(l, env).is_live())
            .map(|l| l.id)
            .collect();
        for id in ids {
            self.end_lease(id, EndReason::Dead);
        }
    }

    /// PD-017: the task is live and `ready` on the claimer's branch with the claimer as the caller; the refusal names
    /// the failing clause.
    // rule: PD-017, LF-003, LF-004
    fn ready_for_claim(&self, t: Nid, holder: &str) -> Res<()> {
        let ix = Index::new(&self.st);
        let mut o = Oracle::new(self.cx.dag, self.cx.alloc);
        self.ready_in(&ix, &mut o, t, holder)
    }

    /// PD-017 over a built index and oracle: the refusal names the failing clause.
    fn ready_in(&self, ix: &Index<'_>, o: &mut Oracle<'_>, t: Nid, holder: &str) -> Res<()> {
        let why = if !derived::unblocked(ix, t) {
            Some(format!("{t} is not unblocked on {}", self.cx.branch))
        } else if !coord::defer_ok(&self.st, t, self.cx.env.now_s()) {
            Some(format!("{t} is deferred"))
        } else if let Some(l) = self.leases.values().find(|l| {
            l.task == Some(t) && l.holder != holder && lease::is_live(l, self.cx.env).is_live()
        }) {
            Some(format!("{t} is leased by {}", l.holder))
        } else {
            o.holders_elsewhere(&self.cx.branch, t)
                .first()
                .map(|(r, h, _)| format!("{t} is {h} on {r}"))
        };
        match why {
            Some(w) => Err(Refusal::lq("E404", w)),
            None => Ok(()),
        }
    }

    /// `claim --next`: the least ready task by (`priority`, `#N`) in the scope that fits the role ([API §10.1]).
    fn next_ready(&self, scope: Option<Nid>, role: &str, holder: &str) -> Option<Nid> {
        if !Cand::fits_role(role) {
            return None;
        }
        let ix = Index::new(&self.st);
        let mut o = Oracle::new(self.cx.dag, self.cx.alloc);
        let mut cands: Vec<(String, Nid)> = self
            .st
            .nodes
            .iter()
            .filter(|(n, x)| {
                x.live() && x.kind == "task" && scope.is_none_or(|s| ix.in_subtree(**n, s))
            })
            .filter(|(n, _)| self.ready_in(&ix, &mut o, **n, holder).is_ok())
            .map(|(n, x)| {
                let p = match x.fields.get("priority") {
                    Some(Value::Enum(p)) => p.clone(),
                    _ => "P2".into(),
                };
                (p, *n)
            })
            .collect();
        cands.sort();
        cands.first().map(|(_, n)| *n)
    }

    /// `tx.complete` ([API §10.5]; DR-002; CO rows): the leased task to `done` with the outcome's resolution, the lease
    /// released into `settled`.
    // spec: [API §10.5]
    // rule: LE-002
    pub fn complete(
        &mut self,
        id: &Target,
        outcome: &str,
        summary: &str,
        evidence: &[String],
        call_lease: Option<u64>,
    ) -> Res<()> {
        let t = self.resolve(id)?;
        let presented = match call_lease {
            Some(c) => self
                .leases
                .get(&c)
                .filter(|l| lease::i17p_fencing(l, c) && lease::is_live(l, self.cx.env).is_live())
                .cloned(),
            None => self.presented()?,
        };
        let l = presented.filter(|l| l.task == Some(t)).ok_or_else(|| {
            // E407 (missing or stale): the task's live lease and its holder, where it has one.
            let task_lease = self
                .leases
                .values()
                .find(|l| l.task == Some(t) && lease::is_live(l, self.cx.env).is_live());
            Refusal::e407(
                task_lease
                    .map(|l| format!("L-{}", l.id))
                    .or_else(|| self.cx.rights.lease.as_ref().map(|p| format!("L-{}", p.id))),
                task_lease.map(|l| l.holder.clone()),
                format!("complete of {t} needs its task lease"),
            )
        })?;
        self.cx.rights.statement(self.stmt, "call-tx-complete")?;
        if matches!(self.cx.rights.role.as_str(), "developer" | "tester")
            && self.cx.rights.lease.as_ref().and_then(|p| p.task) != Some(t)
        {
            return Err(crate::policy::refuse(
                self.stmt,
                "role-statements",
                &self.cx.rights.role,
                "call-tx-complete",
            ));
        }
        let (to, res) = status::complete_outcome(outcome)?;
        let from = self.st.nodes[&t].status.clone();
        if from != "open" && from != "in_progress" {
            return Err(Refusal::lq(
                "E404",
                format!("{t} is {from}; complete moves open or in_progress to done"),
            ));
        }
        self.transit(t, to, res, Door::TxComplete)?;
        self.end_lease(l.id, EndReason::Complete);
        self.message = Some(if evidence.is_empty() {
            summary.to_string()
        } else {
            format!("{summary}\n\nevidence: {}", evidence.join(", "))
        });
        // [API §10.5] yields: [LQ/std §7.3]'s `task`, `status`, `ready`, then `outcome`, `lease`, `settle_commit` (the
        // write path's link settle fills it, [`crate::links::sync`]; null without one) and `changed_since_pack` (null
        // without `pack_digest`).
        // `ready` is filled by the write path once the commit exists; it is not replayed (a replay gives []).
        self.yields.push(Yield {
            index: self.stmt,
            proc: "tx.complete".into(),
            rows: vec![vec![
                ("task".into(), t.to_string()),
                ("status".into(), "done".into()),
                ("ready".into(), String::new()),
                ("outcome".into(), outcome.into()),
                ("lease".into(), format!("L-{}", l.id)),
                ("settle_commit".into(), "null".into()),
                ("changed_since_pack".into(), "null".into()),
            ]],
        });
        Ok(())
    }

    /// `tx.heartbeat` ([API §10.2]): renews a TTL lease that has not ended and whose anchor is not Dead; an expired lease
    /// only by its holder (LE-011). An expired lease that another claim of its task superseded has ended (LE-012), so
    /// its renewal is E407 like that of any ended lease.
    // spec: [API §10.2]
    // rule: LE-011
    pub fn heartbeat(&mut self, id: u64) -> Res<()> {
        let env = self.cx.env;
        let now = env.now();
        let l = self
            .leases
            .get(&id)
            .filter(|l| l.ended.is_none())
            .ok_or_else(|| self.e407(id, format!("lease L-{id} has ended")))?;
        let live = lease::is_live(l, env);
        if live == lease::Live::Dead {
            return Err(self.e407(id, format!("lease L-{id}'s anchor is dead")));
        }
        let mut renewed = false;
        if !l.run_scoped {
            if live == lease::Live::Deadline {
                if Some(&l.holder) != Some(&self.cx.actor) {
                    return Err(self.e407(
                        id,
                        format!("lease L-{id} expired; only its holder renews it"),
                    ));
                }
                let ttl = l.ttl_ms;
                let row = self.leases.get_mut(&id).expect("lease");
                row.expires = after(now, ttl);
                self.events.push(LeaseEvent::Renew { id, durable: true });
                renewed = true;
            } else if due_for_renewal(l.expires, l.ttl_ms, now) {
                let ttl = l.ttl_ms;
                let row = self.leases.get_mut(&id).expect("lease");
                row.expires = after(now, ttl);
                self.events.push(LeaseEvent::Renew { id, durable: false });
                renewed = true;
            }
        }
        let e = self.leases[&id].expires;
        self.yields.push(Yield {
            index: self.stmt,
            proc: "tx.heartbeat".into(),
            rows: vec![vec![
                ("lease".into(), format!("L-{id}")),
                (
                    "expires".into(),
                    format!("{}/{}/{}", e.wall, e.boot_hash, e.boot_ns),
                ),
                ("renewed".into(), renewed.to_string()),
            ]],
        });
        Ok(())
    }

    /// `tx.release` ([API §10.3]; WM-007: the holder, with the current token).
    // spec: [API §10.3]
    // rule: LE-001
    pub fn release(&mut self, id: u64) -> Res<()> {
        let l = self
            .leases
            .get(&id)
            .filter(|l| lease::i17p_fencing(l, id))
            .ok_or_else(|| self.e407(id, format!("lease L-{id} has ended")))?;
        if l.holder != self.cx.actor {
            return Err(self.e407(id, format!("L-{id} is held by {}", l.holder)));
        }
        self.end_lease(id, EndReason::Release);
        self.yields.push(Yield {
            index: self.stmt,
            proc: "tx.release".into(),
            rows: vec![vec![("lease".into(), format!("L-{id}"))]],
        });
        Ok(())
    }

    /// `tx.reclaim` ([API §10.4]; WM-008): every task lease whose `claimed_hlc` is older than the bound, or every lease
    /// scoped to the run.
    // spec: [API §10.4]
    // rule: LE-005, LE-006
    pub fn reclaim(
        &mut self,
        older_than_ms: Option<u64>,
        run: Option<&str>,
        hlc: &crate::clock::Hlc,
    ) -> Res<()> {
        self.cx.rights.mint("reclaim")?;
        if older_than_ms.is_some() && run.is_some() {
            return Err(Refusal::usage_arg(
                "run",
                "reclaim takes older_than or run, not both",
            ));
        }
        let run_id = run.map(|r| self.run_named(r)).transpose()?;
        let bound = older_than_ms.unwrap_or(self.cx.cfg.reclaim_older_than_ms);
        let ids = lease::reclaim(
            self.leases.values(),
            run_id,
            bound,
            hlc,
            self.cx.env.wall_ms,
        );
        let mut rows = Vec::new();
        for id in ids {
            self.end_lease(id, EndReason::Reclaim);
            rows.push(vec![
                ("lease".into(), format!("L-{id}")),
                (
                    "task".into(),
                    self.leases[&id]
                        .task
                        .map_or("null".into(), |t| t.to_string()),
                ),
            ]);
        }
        self.yields.push(Yield {
            index: self.stmt,
            proc: "tx.reclaim".into(),
            rows,
        });
        Ok(())
    }

    /// Runs one data-level statement; a refusal names the LQ statement that raised it ([LQ/errors §5.7]).
    // rule: GR-007, WR-009, WR-011
    pub fn run(&mut self, index: usize, s: &Stmt, hlc: &crate::clock::Hlc) -> Res<()> {
        self.stmt = index;
        self.sub = 0;
        self.run_one(s, hlc)
            .map_err(|e| e.finish(Some(self.lq_stmt())))
    }

    fn run_one(&mut self, s: &Stmt, hlc: &crate::clock::Hlc) -> Res<()> {
        match s {
            Stmt::Create {
                name,
                kind,
                fields,
                body,
                under,
                position,
                edges_out,
                edges_in,
            } => self
                .create(
                    name.as_deref(),
                    kind,
                    fields,
                    body.as_deref(),
                    under.as_ref(),
                    position.as_ref(),
                    edges_out,
                    edges_in,
                )
                .map(|_| ()),
            Stmt::Set {
                target,
                fields,
                incr,
                body,
                guard,
            } => self.set(
                target,
                fields,
                incr,
                body.as_ref().map(|b| b.as_deref()),
                guard.as_ref(),
            ),
            Stmt::Patch {
                target,
                remove,
                add,
            } => self.patch(target, remove, add),
            Stmt::Link {
                src,
                kind,
                dst,
                pinned,
            } => {
                let s = self.resolve(src)?;
                let d = self.resolve(dst)?;
                self.link(s, kind, d, pinned.as_deref())
            }
            Stmt::Unlink { src, kind, dst } => self.unlink(src, kind, dst),
            Stmt::Move {
                target,
                under,
                position,
            } => self.move_(target, under.as_ref(), position.as_ref()),
            Stmt::Reopen { target, reason } => self.reopen(target, reason),
            Stmt::Delete {
                target,
                policy,
                replaced_by,
                release,
                reason,
            } => self.delete(
                target,
                policy.as_deref(),
                replaced_by.as_ref(),
                *release,
                reason.as_deref(),
            ),
            Stmt::Resolve { key, take } => self.resolve_key(key, take),
            Stmt::Call { proc, args } => self.call(proc, args, hlc),
        }
    }

    fn call(&mut self, proc: &str, args: &[(String, P)], hlc: &crate::clock::Hlc) -> Res<()> {
        let get = |k: &str| args.iter().find(|(n, _)| n == k).map(|(_, v)| v);
        let text = |k: &str| -> Option<String> {
            match get(k) {
                Some(P::Text(s)) => Some(s.clone()),
                _ => None,
            }
        };
        let flag = |k: &str| matches!(get(k), Some(P::Bool(true)));
        let node = |k: &str| -> Option<Target> {
            match get(k) {
                Some(P::Text(s)) => parse_node(s),
                Some(P::Int(i)) if *i > 0 => Some(Target::Id(Nid(*i as u32))),
                _ => None,
            }
        };
        let lease_arg = || -> Res<u64> {
            text("lease")
                .as_deref()
                .and_then(parse_lease)
                .ok_or_else(|| Refusal::usage_arg("lease", "lease takes L-<n>"))
        };
        match proc {
            "tx.claim" => {
                let ids: Vec<Target> = match get("ids") {
                    Some(P::List(v)) => v
                        .iter()
                        .filter_map(|x| match x {
                            P::Text(s) => parse_node(s),
                            P::Int(i) if *i > 0 => Some(Target::Id(Nid(*i as u32))),
                            _ => None,
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let scope = node("scope");
                let (role, agent, run) = (text("role"), text("agent"), text("run"));
                self.claim(
                    &ids,
                    flag("next"),
                    scope.as_ref(),
                    role.as_deref(),
                    agent.as_deref(),
                    get("ttl"),
                    flag("start"),
                    run.as_deref(),
                    flag("session"),
                )
            }
            "tx.complete" => {
                let id =
                    node("id").ok_or_else(|| Refusal::usage_arg("id", "tx.complete needs id"))?;
                let outcome = text("outcome")
                    .ok_or_else(|| Refusal::usage_arg("outcome", "tx.complete needs outcome"))?;
                let evidence: Vec<String> = match get("evidence") {
                    Some(P::List(v)) => v
                        .iter()
                        .filter_map(|x| {
                            if let P::Text(s) = x {
                                Some(s.clone())
                            } else {
                                None
                            }
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                // `lease`: the lease presented for this call only, which `apply`'s expansion passes ([API §9.4]
                // step 1; [API] open point 16).
                let lease = text("lease")
                    .map(|t| {
                        parse_lease(&t)
                            .ok_or_else(|| Refusal::usage_arg("lease", "lease takes L-<n>"))
                    })
                    .transpose()?;
                self.complete(
                    &id,
                    &outcome,
                    &text("summary").unwrap_or_default(),
                    &evidence,
                    lease,
                )
            }
            "tx.heartbeat" => self.heartbeat(lease_arg()?),
            "tx.release" => self.release(lease_arg()?),
            "tx.reclaim" => {
                let older = get("older_than")
                    .map(|v| duration_arg("older_than", v))
                    .transpose()?;
                self.reclaim(older, text("run").as_deref(), hlc)
            }
            other => Err(Refusal::lq("E109", format!("unknown procedure {other}"))),
        }
    }

    /// The deferred validators on a write, in I37′ order ([F13 §5]; VO-3: every structural violation refuses the
    /// block with E405): V03 I5′, V04 I2, V05 I4, V06 I6, V07 the other cardinalities, V09 I11.
    // spec: [F13 §5]
    // rule: DP-008, DP-009
    pub fn deferred(&self) -> Res<()> {
        self.deferred_checks()
            .map_err(|e| e.finish(Some(self.last_lq_stmt())))
    }

    /// On a staging ref the view is the staged candidate, whose structural violations `merge --continue` re-checks
    /// ([F12 §6.5] "On a violation's key", §9.4 step 3): a `RESOLVE` block there is refused only for a violation of a
    /// check the view before it passed, and always for a node it makes its own ancestor ([`Cand::check_forest`]). On
    /// any other ref every violation refuses (VO-3).
    fn deferred_checks(&self) -> Res<()> {
        let staged = self.cx.view == RefKind::Merge;
        let had = |fails: &dyn Fn(&State) -> bool| staged && fails(&self.base);
        if let Some((a, k, b)) = derived::i5p_cycle_witness(&self.st)
            && !had(&|st| derived::i5p_cycle_witness(st).is_some())
        {
            return Err(e405(format!(
                "Cycle: {a} {k} {b} closes a cycle of the precedence graph (I5′)"
            )));
        }
        if let Err(w) = crate::inv::i2_structural_edges_live(&self.st)
            && !had(&|st| crate::inv::i2_structural_edges_live(st).is_err())
        {
            return Err(e405(format!("DanglingEdge: {w} (I2)")));
        }
        let view_broke = std::cell::OnceCell::new();
        for n in self.st.nodes.keys() {
            self.check_forest(*n, &view_broke)?;
        }
        if let Err(w) = crate::inv::i6_supersedes(&self.st)
            && !had(&|st| crate::inv::i6_supersedes(st).is_err())
        {
            return Err(e405(format!("SupersedeFork: {w} (I6)")));
        }
        if let Err(w) = crate::inv::cardinalities(&self.st)
            && !had(&|st| crate::inv::cardinalities(st).is_err())
        {
            return Err(e405(format!("Cardinality: {w}")));
        }
        if let Err(w) = crate::inv::i11_schema_conformance(&self.st)
            && !had(&|st| crate::inv::i11_schema_conformance(st).is_err())
        {
            return Err(e405(format!("SchemaConflict: {w} (I11)")));
        }
        // V10 and V11 ([F19 §12.5.6]; [50 §3.10] item 5): the named queries the block touched, or whose callees or
        // schema it changed, bind and call no cycle; a `DEFINE QUERY`, a `DROP QUERY` of a callee or any other schema
        // change that breaks one refuses the block. On a staging ref `merge --continue` re-checks them.
        if !staged && self.st.schema.items != self.base.schema.items {
            let mut ids = crate::mvalid::state_ids(&self.st);
            for (seq, id, r) in crate::lqh::commit_table(self.cx.dag) {
                ids.commit(seq, id, &r);
            }
            ids.next_id_at(self.next_id);
            let (invalid, cycles) = crate::mvalid::query_violations(&self.st, &self.base, &ids);
            if let Some(b) = invalid.first() {
                return Err(e405(format!(
                    "named queries bind (QueryInvalid) would be violated; {}: {} {}",
                    b.name, b.code, b.message
                )));
            }
            if let Some(c) = cycles.first() {
                return Err(e405(format!(
                    "named-query cycle (QueryCycle) would be violated; {}",
                    crate::mvalid::cycle_text(&c.path)
                )));
            }
        }
        Ok(())
    }

    /// Maintains `mentions` ([F08 §10.4]; WE-016): every live node whose title, abstract or body the block wrote gets
    /// as its `mentions` out-edges the nodes its sigils name, itself excepted.
    // rule: WR-013
    pub fn refresh_mentions(&mut self) {
        // A staging ref takes `Resolve` ops and their companions only ([F12 §9.3]); `merge --continue` lands the
        // resolved text.
        if self.cx.view == RefKind::Merge {
            return;
        }
        let next_id = self.next_id;
        let texts = |x: &Node| -> String {
            let mut t = String::new();
            for f in ["title", "abstract"] {
                if let Some(v) = x.text(f) {
                    t.push_str(v);
                    t.push('\n');
                }
            }
            if let Some(b) = &x.body {
                t.push_str(b);
            }
            t
        };
        let same_texts = |x: &Node, y: &Node| {
            x.fields.get("title") == y.fields.get("title")
                && x.fields.get("abstract") == y.fields.get("abstract")
                && x.body == y.body
        };
        let changed: Vec<Nid> = self
            .st
            .nodes
            .iter()
            .filter(|(n, x)| x.live() && self.base.nodes.get(n).is_none_or(|y| !same_texts(x, y)))
            .map(|(n, _)| *n)
            .collect();
        for n in changed {
            let named: BTreeSet<Nid> = sigils(&texts(&self.st.nodes[&n]), next_id)
                .into_iter()
                .filter(|m| *m != n)
                .collect();
            let x = self.st.nodes.get_mut(&n).expect("node");
            x.out
                .retain(|k, _| k.kind != "mentions" || named.contains(&k.dst));
            for m in named {
                x.out
                    .entry(EdgeKey {
                        kind: "mentions".into(),
                        dst: m,
                        disc: None,
                    })
                    .or_default();
            }
        }
    }

    /// The role-create check of every node the block created, on its final values (WC rows; WR-009). It runs after the
    /// last statement and before the deferred validators, as WR-009's role checks precede WR-011's invariants; a
    /// refusal names the statement that created the node.
    pub fn check_creates(&self) -> Res<()> {
        for n in &self.created {
            let stmt = self.created_at.get(n).copied().unwrap_or(self.stmt);
            let edges = self.edges_from.get(n).cloned().unwrap_or_default();
            self.cx
                .rights
                .create(stmt, &self.st, *n, &edges)
                .map_err(|e| e.finish(Some(stmt)))?;
        }
        Ok(())
    }
}

/// The nodes a text names by sigil ([F08 §10.4]): `#` followed by a maximal run of ASCII digits without a leading `0`,
/// its value N with 1 ≤ N < `next_id`, the `#` not preceded by an ASCII letter or digit, and the run not followed by `/`
/// or by `.` and a digit.
// spec: [F08 §10.4]
pub fn sigils(text: &str, next_id: u32) -> BTreeSet<Nid> {
    let b = text.as_bytes();
    let mut out = BTreeSet::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'#' && (i == 0 || !b[i - 1].is_ascii_alphanumeric()) {
            let s = i + 1;
            let mut e = s;
            while e < b.len() && b[e].is_ascii_digit() {
                e += 1;
            }
            let followed = e < b.len()
                && (b[e] == b'/' || (b[e] == b'.' && e + 1 < b.len() && b[e + 1].is_ascii_digit()));
            if e > s
                && b[s] != b'0'
                && !followed
                && let Ok(n) = text[s..e].parse::<u32>()
                && n >= 1
                && n < next_id
            {
                out.insert(Nid(n));
            }
            i = e.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// The edge-kind cardinality rule of a kind at a node set, used by [`crate::inv::cardinalities`].
pub fn card_of(schema: &Schema, kind: &str) -> Option<Card> {
    schema.edge(kind).map(|e| e.card)
}

/// Whether an edge kind is structural.
pub fn structural(schema: &Schema, kind: &str) -> bool {
    schema
        .edge(kind)
        .is_some_and(|e| e.class == EdgeClass::Structural)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_keys_follow_the_informative_values() {
        assert_eq!(between(None, None), "V");
        assert_eq!(between(None, Some("V")), "F");
        assert_eq!(between(Some("V"), None), "k");
        assert_eq!(between(Some("V"), Some("W")), "VV");
    }

    proptest::proptest! {
        /// `a < between(a, b) < b` bytewise, and the key never ends in `0`.
        #[test]
        fn between_lies_between(x in "[0-9A-Za-z]{0,4}[1-9A-Za-z]", y in "[0-9A-Za-z]{0,4}[1-9A-Za-z]") {
            let (a, b) = if x < y { (x, y) } else if y < x { (y, x) } else { return Ok(()) };
            let k = between(Some(&a), Some(&b));
            proptest::prop_assert!(a < k && k < b, "{} < {} < {}", a, k, b);
            proptest::prop_assert!(!k.ends_with('0'));
            let lo = between(None, Some(&a));
            proptest::prop_assert!(lo < a && !lo.ends_with('0'));
            let hi = between(Some(&b), None);
            proptest::prop_assert!(hi > b && !hi.ends_with('0'));
        }
    }

    #[test]
    fn sigils_follow_the_mention_rule() {
        let v: Vec<u32> = sigils("see #12, #0 x#3 #4/5 #6.7 #7. #99 #8", 50)
            .into_iter()
            .map(|n| n.0)
            .collect();
        assert_eq!(v, vec![7, 8, 12]);
    }

    #[test]
    fn node_and_lease_texts_parse() {
        assert_eq!(parse_node("#12"), Some(Target::Id(Nid(12))));
        assert_eq!(parse_node("#012"), None);
        assert!(matches!(
            parse_node("#u:000102030405060708090a0b0c0d0e0f"),
            Some(Target::Uid(_))
        ));
        assert_eq!(parse_lease("L-19"), Some(19));
        assert_eq!(parse_lease("L-0"), None);
        assert_eq!(iso_seconds("1970-01-02T00:00:00Z"), Some(86_400));
        assert_eq!(iso_seconds("2026-09-25"), Some(1_790_294_400));
        assert!(glob_ok("crates/**/src/*.rs"));
        assert!(!glob_ok("a/**b"));
        assert!(!glob_ok("[]"));
    }

    #[test]
    fn the_equivalent_numbers_parameters_in_order() {
        let s = Schema::default();
        let eq = equivalent(
            &s,
            &[
                Stmt::Create {
                    name: Some("api".into()),
                    kind: "task".into(),
                    fields: vec![
                        ("title".into(), P::Text("a".into())),
                        ("priority".into(), P::Text("P1".into())),
                    ],
                    body: None,
                    under: None,
                    position: None,
                    edges_out: vec![],
                    edges_in: vec![],
                },
                Stmt::Create {
                    name: None,
                    kind: "task".into(),
                    fields: vec![("title".into(), P::Text("b".into()))],
                    body: None,
                    under: Some(Target::Var("api".into())),
                    position: Some(Position::Last),
                    edges_out: vec![],
                    edges_in: vec![("blocks".into(), Target::Id(Nid(7)))],
                },
            ],
        );
        assert_eq!(
            eq.text,
            "TX { CREATE (api:task {title: $p1, priority: $p2}); CREATE (v1:task {title: $p3}) UNDER api; MOVE v1 UNDER api LAST; CREATE (#7)-[:BLOCKS]->(v1) }"
        );
        assert_eq!(eq.stmts.len(), 4);
    }
}
