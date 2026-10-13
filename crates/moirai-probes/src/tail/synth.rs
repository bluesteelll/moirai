//! The generator of measurement 10's tails (WP-53d): a tail of local agent commits on `main` and the settles between
//! them, from the spec-derived mix below (R-24 Q10: no real owner mix exists; the aggregate states this mix).
//!
//! A tail is a function of (clause, threshold, `N`, seed, profile). It starts at the first byte of extent 2 of an epoch
//! whose `epoch_lsn` is 0, with the extent head whose `chain_in` stands for extent 1's last trailer
//! (`05-log.md:111-112`, `:183-184`, `:1119`); the scan starts there (`:278-279`). Groups are placed by G-3. The base
//! is [`Base`]: nodes `1..=N` live, kind 1, status `#N mod 4`; new `#N` start at `N + 1`.
//!
//! | Mix constant | Value | Source |
//! |---|---|---|
//! | commits per settle | 5: about 500 commits a day on `main` and 100 settles a day | `60-roadmap.md:759` ([70 S1]); `70-audit-speed.md:298` (the upper end of 50–100) |
//! | a settle | one lazy group: a `TreeReg` sub-2 epoch and one `FileObs` batch | `70-audit-speed.md:300` fix (1); `11-runtime-tables.md:891-892` |
//! | settle rows | `max(1, ⌊83 × P12 / 2 MiB⌋)` rows of state 1 `ok`, no `var`: 83 is the most whose group stays ≤ 16,000 B | `70-audit-speed.md:300` ("< 16 KB", read as 10^3 by `measurement-protocol.md:45-46`) |
//! | commit length (without body data) | 9 in 10 uniform in [300, 600] B; 1 in 10 log-uniform in [600, P05] | shape `60-roadmap.md:758`; the split and the form are the probe's |
//! | ops per commit | `max(1, ⌊(length − 32 − header) / b⌋)`, b uniform in [190, 240] | `17-store-parameters.md:293` ([50 §5.12]) |
//! | header | one parent, git provenance, the idempotency pair, a named mutation, a message, `affected` | `06-commit.md:523-525`, `:1209-1210` |
//! | op mix (%) | `SetField` 35, `SetStatus` 20, `AddEdge` 15, `Create` 10, `Move` 5, `SetBody` 5, `RemoveEdge` 5, `Delete` 5 | the probe's choice: no spec states an op mix |
//! | reaching the length | `SetField` text values and the message are sized so the record has the drawn length, or its minimum when that is longer; with no `SetField`, the message stops at F06's 65,535 bytes (`06-commit.md:252`), and a longer draw gives a shorter record | the probe's choice |
//! | body raw size | 1,020 B in the mix (≈ 340 B stored at 3×); the body family `min(64 KiB, P05 / 2)` | `20-critique-perf-ram-windows.md:33`; `06-commit.md:1049` |
//! | touched `#N` | uniform over [1, `N`] and the tail's creates, one op per owner per commit | the probe's choice (the worst case for the node index) |
//!
//! **Stop rules** (a full tail is a tail at the threshold, `17-store-parameters.md:353-354`; groups are whole): `ops`
//! ends when `ops(T)` = P06, the last commit cut to fit; `body` ends when `body_bytes(T)` = P08, one body of the
//! family size per commit; `rec`, `ovl` and `quiet` end before the first group that would take their measure past the
//! threshold (`quiet`: `ops` ≤ q·P06, `rec_bytes` ≤ q·P07, `body_bytes` ≤ q·P08 and `ovl` ≤ P10, q = P11, the other
//! thresholds at their design figures, `:356-369`, `:215-217`), except that a tail always holds its first commit
//! (`:354`: a writer's own commit may pass the threshold). Every tail keeps `rt_bytes(T)` within C2's
//! `m·q·P12` (`:329-333`: m = P13 and q = 1, or m = 1 and q = P11 in quiet mode, `:362-363`); the generator refuses a
//! target that would break it.

use super::commit::{
    Affected, Body, Commit, Edge, Git, Image, Op, Parent, TValue, blake3_16, loc_len,
};
use super::rec::{
    Appender, ExtentHead, InitParams, Payload, Record, SymDef, encode_symdefs, sequence, sym,
};
use super::rt::{Epoch, FileObsRow, Row, RowBatch, TreeReg};
use super::scan::{Expect, MemLog};
use super::{
    BODY_RAW_CAP, EXTENT_HEAD_GROUP_LEN, Locator, Params, PrevLoc, REC_HDR_LEN, TAIL_READ_CHUNK,
    in_k_rt,
};
use crate::overlay::{Base, Builder, Overlay};
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Commits per settle (`60-roadmap.md:759`, `70-audit-speed.md:298`).
pub const COMMITS_PER_SETTLE: u64 = 5;
/// The size a settle's group stays within (`70-audit-speed.md:300`).
pub const SETTLE_GROUP_CAP: u64 = 16_000;
/// The `FileObs` rows of a production settle: the most whose group stays ≤ [`SETTLE_GROUP_CAP`].
pub const SETTLE_ROWS_PRODUCTION: u64 = 83;
/// The short commit lengths, 9 in 10 (`60-roadmap.md:758`: typical 0.3–0.6 KB).
pub const SHORT_LEN: (u64, u64) = (300, 600);
/// Bytes per op (`17-store-parameters.md:293`: ≈ 190–240 B per op).
pub const OP_BYTES: (u64, u64) = (190, 240);
/// The op mix in percent, in the order of [`OpKind`].
pub const OP_MIX: [(OpKind, u64); 8] = [
    (OpKind::SetField, 35),
    (OpKind::SetStatus, 20),
    (OpKind::AddEdge, 15),
    (OpKind::Create, 10),
    (OpKind::Move, 5),
    (OpKind::SetBody, 5),
    (OpKind::RemoveEdge, 5),
    (OpKind::Delete, 5),
];
/// A mix body's raw size (`20-critique-perf-ram-windows.md:33`: ≈ 340 B stored at 3×).
pub const MIX_BODY: u64 = 1_020;
/// The base nodes' kind.
pub const BASE_KIND: u8 = 1;
/// Node kinds, statuses, field names, edge kinds and trees the probe draws from (opaque codes).
pub const KINDS: u8 = 4;
/// Status codes.
pub const STATUSES: u8 = 4;
/// Field names.
pub const FIELDS: u32 = 8;
/// Edge kinds.
pub const EKINDS: u8 = 4;
/// Trees settled in turn.
pub const TREES: u8 = 4;
/// The `SYMTAB` the tail is assumed to start from: its greatest `session` and `stmt` ids (SD-1).
pub const SYMTAB_SESSIONS: u32 = 16;
/// Its greatest `stmt` id.
pub const SYMTAB_STMTS: u32 = 8;

/// The op kinds of the mix.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OpKind {
    /// `SetField`.
    SetField,
    /// `SetStatus`.
    SetStatus,
    /// `AddEdge`.
    AddEdge,
    /// `Create`.
    Create,
    /// `Move`.
    Move,
    /// `SetBody`.
    SetBody,
    /// `RemoveEdge`.
    RemoveEdge,
    /// `Delete`.
    Delete,
}

/// The clause a tail grows to ([F17 §5.2] C1 and §5.3's quiet cap).
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Clause {
    /// `ops(T)` reaches P06.
    Ops,
    /// `rec_bytes(T)` reaches P07.
    Rec,
    /// `body_bytes(T)` reaches P08.
    Body,
    /// `ovl(T)` reaches P09.
    Ovl,
    /// The quiet cap: P11 × the thresholds, or P10 of overlay, whichever first.
    Quiet,
}

impl Clause {
    /// The quantity-name family.
    pub const fn as_str(self) -> &'static str {
        match self {
            Clause::Ops => "ops",
            Clause::Rec => "rec",
            Clause::Body => "body",
            Clause::Ovl => "ovl",
            Clause::Quiet => "quiet",
        }
    }
}

/// What to generate.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Target {
    /// The clause.
    pub clause: Clause,
    /// Its threshold: P06, P07, P08, P09 or (quiet) P10.
    pub threshold: u64,
    /// The quiet multiplier P11 (1 for the other clauses).
    pub mult: u64,
    /// The base's node count `N`.
    pub n: u32,
    /// The seed.
    pub seed: u64,
    /// The profile's parameters (E, P05, the design figures of P06–P08, P12, P13).
    pub params: Params,
    /// C2's bound on `rt_bytes(T)`; `None` is the clause's `m·q·P12`. Only a test that is not a grid point sets it.
    pub rt_bound: Option<u64>,
}

impl Target {
    /// C2's bound: `P13·P12`, or `P11·P12` in quiet mode.
    pub fn rt_cap(&self) -> u64 {
        self.rt_bound.unwrap_or(match self.clause {
            Clause::Quiet => self.mult * self.params.p12,
            _ => self.params.p13 * self.params.p12,
        })
    }
}

/// The settle rows at a profile: `max(1, ⌊83 × P12 / P12_production⌋)`.
pub fn settle_rows(p: &Params) -> u64 {
    (SETTLE_ROWS_PRODUCTION * p.p12 / Params::PRODUCTION.p12).max(1)
}

/// The body family's body size: `min(64 KiB, P05 / 2)`.
pub fn family_body(p: &Params) -> u64 {
    u64::from(BODY_RAW_CAP).min(p.p05 / 2)
}

/// The tail measures of [F17 §5.1] (`17-store-parameters.md:313-321`) and counts.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct Measures {
    /// `ops(T)`.
    pub ops: u64,
    /// `rec_bytes(T)`: the lengths of the records outside `K_RT`, less their body data.
    pub rec_bytes: u64,
    /// `body_bytes(T)`: the `data` bytes of the carried bodies (cut finding F-2).
    pub body_bytes: u64,
    /// `rt_bytes(T)`: the lengths of the `K_RT` records.
    pub rt_bytes: u64,
    /// The bytes of item 1a's locators in the tail's op frames, part of `rec_bytes` (M-A5; 0 in the night's form).
    pub loc_bytes: u64,
    /// `ovl(T)`: the overlay's charge.
    pub ovl: u64,
    /// Records.
    pub records: u64,
    /// Groups.
    pub groups: u64,
    /// `Commit` records.
    pub commits: u64,
    /// Settles.
    pub settles: u64,
    /// Bytes from the start to the end.
    pub bytes: u64,
}

/// A generated tail.
#[derive(Clone, Debug)]
pub struct Tail {
    /// The bytes from `start`.
    pub bytes: Vec<u8>,
    /// What the scan checks the tail against.
    pub expect: Expect,
    /// The first lsn: extent 2's first byte (`HEAD.checkpoint_lsn`).
    pub start: u64,
    /// The end (`HEAD.committed_lsn`).
    pub end: u64,
    /// The chain value at `start`.
    pub chain_in: u64,
    /// The base the tail applies to.
    pub base: Base,
    /// The measures.
    pub measures: Measures,
    /// The measures the refused next group would have produced, for the clauses that end before one.
    pub refused: Option<Measures>,
    /// Every record in log order, when asked for.
    pub records: Vec<Record<'static>>,
}

impl Tail {
    /// Replays the tail from memory into its overlay.
    pub fn overlay(&self) -> Result<Overlay, String> {
        Overlay::replay(
            &mut MemLog::new(self.start, &self.bytes),
            &self.expect,
            self.start,
            self.end,
            self.chain_in,
            self.base,
            TAIL_READ_CHUNK,
        )
        .map_err(|e| e.to_string())
    }
}

/// SplitMix64.
#[derive(Clone, Debug)]
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[lo, hi]`.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }

    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let mut b = [0u8; N];
        for c in b.chunks_mut(8) {
            let w = self.next().to_le_bytes();
            c.copy_from_slice(&w[..c.len()]);
        }
        b
    }

    fn vec(&mut self, n: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(n);
        while v.len() < n {
            let w = self.next().to_le_bytes();
            v.extend_from_slice(&w[..(n - v.len()).min(8)]);
        }
        v
    }

    /// Log-uniform in `[lo, hi]`.
    fn log_uniform(&mut self, lo: u64, hi: u64) -> u64 {
        let u = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        let v = (lo as f64) * ((hi as f64) / (lo as f64)).powf(u);
        (v.round() as u64).clamp(lo, hi)
    }
}

/// ASCII filler text of `n` bytes.
fn filler(n: usize, seed: u64) -> String {
    (0..n)
        .map(|i| char::from(b'a' + ((i as u64 + seed) % 26) as u8))
        .collect()
}

/// The state of a node the tail touched.
#[derive(Clone, Debug)]
struct Node {
    live: bool,
    kind: u8,
    status: (u8, u8),
    body: Option<[u8; 16]>,
    fields: BTreeMap<u32, Vec<u8>>,
    parent: u32,
    order: String,
    edges: BTreeSet<(u8, u32)>,
    last: Option<u64>,
    /// Where the node's frames lie in `last` (A1 only): the writer's overlay entry the locator is taken from (u20 cut
    /// §3.1, SoT-1).
    at: Option<PrevLoc>,
}

/// One op of a commit's plan: every value but `prev`, which follows from the record's lsn.
#[derive(Clone, Debug)]
enum Planned {
    Create {
        id: u32,
        uid: [u8; 16],
        kind: u8,
        status: u8,
        title: String,
    },
    Delete {
        id: u32,
    },
    SetField {
        id: u32,
        name: u32,
        old: Vec<u8>,
    },
    SetStatus {
        id: u32,
        new: u8,
    },
    SetBody {
        id: u32,
        data: Vec<u8>,
    },
    AddEdge {
        id: u32,
        ekind: u8,
        dst: u32,
    },
    RemoveEdge {
        id: u32,
        ekind: u8,
        dst: u32,
    },
    Move {
        id: u32,
        parent: u32,
        order: String,
    },
}

impl Planned {
    fn owner(&self) -> u32 {
        match self {
            Planned::Create { id, .. }
            | Planned::Delete { id }
            | Planned::SetField { id, .. }
            | Planned::SetStatus { id, .. }
            | Planned::SetBody { id, .. }
            | Planned::AddEdge { id, .. }
            | Planned::RemoveEdge { id, .. }
            | Planned::Move { id, .. } => *id,
        }
    }
}

/// The generator's state.
struct Gen {
    t: Target,
    rng: Rng,
    base: Base,
    nodes: HashMap<u32, Node>,
    edged: Vec<u32>,
    next_id: u32,
    start: u64,
    seq: u64,
    hlc: u64,
    parent: Parent,
    commits: u64,
    settles: u64,
    session: u32,
    stmt: u32,
}

/// A planned commit: its ops' plans and its drawn length.
struct Plan {
    ops: Vec<Planned>,
    len: u64,
    msg_seed: u64,
    commit_id: [u8; 32],
    head: [u8; 20],
    idem: ([u8; 16], [u8; 16]),
    stmt_hash: [u8; 16],
    digest: [u8; 32],
}

impl Gen {
    fn node(&self, id: u32) -> Node {
        self.nodes.get(&id).cloned().unwrap_or_else(|| Node {
            live: id >= 1 && id <= self.base.n,
            kind: self.base.kind,
            status: (self.base.status(id), 0),
            body: None,
            fields: BTreeMap::new(),
            parent: 0,
            order: String::new(),
            edges: BTreeSet::new(),
            last: None,
            at: None,
        })
    }

    fn live(&self, id: u32) -> bool {
        self.nodes
            .get(&id)
            .map_or(id >= 1 && id <= self.base.n, |n| n.live)
    }

    /// A live owner not yet used by this commit.
    fn pick(&mut self, used: &BTreeSet<u32>) -> Option<u32> {
        for _ in 0..64 {
            let id = self.rng.range(1, u64::from(self.next_id - 1)) as u32;
            if self.live(id) && !used.contains(&id) {
                return Some(id);
            }
        }
        None
    }

    fn draw_kind(&mut self) -> OpKind {
        let mut r = self.rng.range(0, 99);
        for (k, w) in OP_MIX {
            if r < w {
                return k;
            }
            r -= w;
        }
        OpKind::SetField
    }

    /// Plans one op of kind `k` (with fallbacks when the state cannot take it).
    fn plan_op(
        &mut self,
        k: OpKind,
        used: &mut BTreeSet<u32>,
        creates: &mut u32,
        body: u64,
    ) -> Option<Planned> {
        let k = match k {
            OpKind::RemoveEdge
                if self
                    .edged
                    .iter()
                    .all(|i| used.contains(i) || !self.live(*i)) =>
            {
                OpKind::AddEdge
            }
            k => k,
        };
        let p = match k {
            OpKind::Create => {
                let id = self.next_id + *creates;
                *creates += 1;
                Planned::Create {
                    id,
                    uid: self.rng.bytes(),
                    kind: self.rng.range(1, u64::from(KINDS)) as u8,
                    status: self.rng.range(0, u64::from(STATUSES) - 1) as u8,
                    title: format!("t{id}"),
                }
            }
            OpKind::RemoveEdge => {
                let cands: Vec<u32> = self
                    .edged
                    .iter()
                    .copied()
                    .filter(|i| !used.contains(i) && self.live(*i))
                    .collect();
                let id = cands[self.rng.range(0, cands.len() as u64 - 1) as usize];
                let n = self.node(id);
                let i = self.rng.range(0, n.edges.len() as u64 - 1) as usize;
                let &(ekind, dst) = n.edges.iter().nth(i)?;
                Planned::RemoveEdge { id, ekind, dst }
            }
            _ => {
                let id = self.pick(used)?;
                let n = self.node(id);
                match k {
                    OpKind::SetField => {
                        let name = self.rng.range(1, u64::from(FIELDS)) as u32;
                        let old = n
                            .fields
                            .get(&name)
                            .cloned()
                            .unwrap_or_else(|| TValue::absent().0.into_owned());
                        Planned::SetField { id, name, old }
                    }
                    OpKind::SetStatus => Planned::SetStatus {
                        id,
                        new: (n.status.0 + self.rng.range(1, u64::from(STATUSES) - 1) as u8)
                            % STATUSES,
                    },
                    OpKind::AddEdge => {
                        let ekind = self.rng.range(1, u64::from(EKINDS)) as u8;
                        let mut dst = None;
                        for _ in 0..16 {
                            let d = self.rng.range(1, u64::from(self.next_id - 1)) as u32;
                            if d != id && self.live(d) && !n.edges.contains(&(ekind, d)) {
                                dst = Some(d);
                                break;
                            }
                        }
                        Planned::AddEdge {
                            id,
                            ekind,
                            dst: dst?,
                        }
                    }
                    OpKind::Move => {
                        let parent = self.rng.range(0, u64::from(id - 1)) as u32;
                        let mut order = filler(2, self.rng.next());
                        if parent == n.parent && order == n.order {
                            order.push('z');
                        }
                        Planned::Move { id, parent, order }
                    }
                    OpKind::SetBody => Planned::SetBody {
                        id,
                        data: self.rng.vec(body as usize),
                    },
                    OpKind::Delete => Planned::Delete { id },
                    OpKind::Create | OpKind::RemoveEdge => return None,
                }
            }
        };
        used.insert(p.owner());
        Some(p)
    }

    /// `prev` of `owner` at `lsn` ([F06 §7.3]): 0 for a node the commit creates; the distance to the owner's newest
    /// tail commit; for a base node's first op in the tail, the distance to a notional record just below the tail.
    fn prev(&self, owner: u32, lsn: u64, creates: bool) -> u64 {
        if creates {
            return 0;
        }
        match self.nodes.get(&owner).and_then(|n| n.last) {
            Some(l) => lsn - l,
            None => lsn - (self.start - 1),
        }
    }

    /// The ops of `plan` for a record at `lsn`, in op order, the bodies they carry, and their locators in the tail's
    /// form: for each owner whose first op has `prev` ≠ 0, where its frames lie in the record `prev` names (SoT-1), or
    /// (0, 0) when that record is below the tail (SoT-4, a base node's first op).
    #[allow(clippy::type_complexity)]
    fn ops(
        &self,
        plan: &Plan,
        lsn: u64,
        extra: &[usize],
    ) -> (Vec<Op<'static>>, Vec<Body<'static>>, Vec<(u32, PrevLoc)>) {
        let mut ops = Vec::with_capacity(plan.ops.len());
        let mut bodies = Vec::new();
        for (i, p) in plan.ops.iter().enumerate() {
            let owner = p.owner();
            let prev = self.prev(owner, lsn, matches!(p, Planned::Create { .. }));
            let n = self.node(owner);
            ops.push(match p {
                Planned::Create {
                    id,
                    uid,
                    kind,
                    status,
                    title,
                } => Op::Create {
                    id: *id,
                    prev,
                    uid: *uid,
                    kind: *kind,
                    c_actor: 5,
                    c_role: 2,
                    image: Image {
                        status: Some((*status, 0)),
                        body: None,
                        fields: vec![(1, TValue::text(title))],
                    },
                },
                Planned::Delete { id } => Op::Delete {
                    id: *id,
                    prev,
                    reason: 0,
                    replaced_by: 0,
                    before: Image {
                        status: Some(n.status),
                        body: n.body,
                        fields: n
                            .fields
                            .iter()
                            .map(|(k, v)| (*k, TValue(Cow::Owned(v.clone()))))
                            .collect(),
                    },
                },
                Planned::SetField { id, name, old } => {
                    let mut new = TValue::text(&filler(1 + extra[i], u64::from(*id)));
                    if new.0.as_ref() == old.as_slice() {
                        new = TValue::text(&filler(2 + extra[i], u64::from(*id)));
                    }
                    Op::SetField {
                        id: *id,
                        prev,
                        name: *name,
                        old: TValue(Cow::Owned(old.clone())),
                        new,
                    }
                }
                Planned::SetStatus { id, new } => Op::SetStatus {
                    id: *id,
                    prev,
                    old: n.status,
                    new: (*new, 0),
                },
                Planned::SetBody { id, data } => {
                    let h = blake3_16(data);
                    bodies.push(Body {
                        hash: h,
                        data: Cow::Owned(data.clone()),
                    });
                    Op::SetBody {
                        id: *id,
                        prev,
                        old: n.body,
                        new: Some(h),
                    }
                }
                Planned::AddEdge { id, ekind, dst } => Op::AddEdge(Edge {
                    src: *id,
                    prev,
                    ekind: *ekind,
                    dst: *dst,
                }),
                Planned::RemoveEdge { id, ekind, dst } => Op::RemoveEdge(Edge {
                    src: *id,
                    prev,
                    ekind: *ekind,
                    dst: *dst,
                }),
                Planned::Move { id, parent, order } => Op::Move {
                    id: *id,
                    prev,
                    old_parent: n.parent,
                    new_parent: *parent,
                    old_order: Cow::Owned(n.order.clone()),
                    new_order: Cow::Owned(order.clone()),
                },
            });
        }
        ops.sort_by_key(Op::order_key);
        bodies.sort_by_key(|b| b.hash);
        let mut locs = Vec::new();
        if self.t.params.shape.locator != Locator::None {
            for (i, op) in ops.iter().enumerate() {
                if (i == 0 || ops[i - 1].owner() != op.owner()) && op.prev() != 0 {
                    let at = self.nodes.get(&op.owner()).and_then(|n| n.at);
                    locs.push((op.owner(), at.unwrap_or_default()));
                }
            }
        }
        (ops, bodies, locs)
    }

    fn commit(&self, plan: &Plan, lsn: u64, extra: &[usize], msg: usize) -> Commit<'static> {
        let (ops, bodies, locs) = self.ops(plan, lsn, extra);
        let mut ids: Vec<u32> = ops.iter().map(Op::owner).collect();
        ids.dedup();
        Commit {
            commit_id: plan.commit_id,
            parent: Some(self.parent),
            gen_no: self.seq as u32,
            seq: self.seq,
            ref_sym: 1,
            ref_id: 0,
            ref_old: Some(self.parent.id16),
            prev_on_ref: Some(self.parent.lsn),
            ref_seq: self.seq as u32,
            hlc: self.hlc,
            actor: 5,
            role: 2,
            session: self.session,
            schema_version: 1,
            git: Some(Git {
                algo: 1,
                head: Some(Cow::Owned(plan.head.to_vec())),
                branch: 1,
                worktree: 1,
                base: None,
            }),
            idem: Some(plan.idem),
            stmt_origin: 1,
            actor_src: 1,
            stmt_sym: self.stmt,
            stmt_hash: Some(plan.stmt_hash),
            msg: Some(Cow::Owned(filler(msg, plan.msg_seed))),
            affected: Some(Affected {
                complete: true,
                ids,
            }),
            changeset_digest: plan.digest,
            ops,
            form: self.t.params.shape.locator,
            locs,
            bodies,
        }
    }

    fn symdefs(&self) -> Vec<SymDef<'static>> {
        if self.commits > 0 {
            return Vec::new();
        }
        vec![
            SymDef {
                class: sym::SESSION,
                id: self.session,
                text: Cow::Borrowed("probe:session"),
            },
            SymDef {
                class: sym::STMT,
                id: self.stmt,
                text: Cow::Borrowed("tx.edit"),
            },
        ]
    }

    /// The commit group of `plan` at `lsn`, its text sized so that `len − body data` is the drawn length when it
    /// can be (the minimum otherwise); with no `SetField`, the message stops at F06's 65,535 bytes and a longer draw
    /// gives a shorter record.
    fn commit_group(&self, plan: &Plan, lsn: u64) -> Result<Vec<Record<'static>>, String> {
        let fields: Vec<usize> = plan
            .ops
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p, Planned::SetField { .. }))
            .map(|(i, _)| i)
            .collect();
        let mut extra = vec![0usize; plan.ops.len()];
        let mut msg = 1usize;
        let rec = |extra: &[usize], msg: usize| -> Result<(Record<'static>, u64), String> {
            let c = self.commit(plan, lsn, extra, msg);
            let body = c.body_bytes();
            let r = Record {
                lsn,
                group_end: true,
                symdefs: self.symdefs(),
                payload: Payload::Commit(Box::new(c)),
            };
            let len = r.encoded_len()? - body;
            Ok((r, len))
        };
        let (mut r, len0) = rec(&extra, msg)?;
        if len0 >= plan.len {
            return Ok(vec![r]);
        }
        let deficit = (plan.len - len0) as usize;
        if fields.is_empty() || deficit < 64 {
            // F06's `msg` is a `vstr` of 1 to 65,535 bytes (`06-commit.md:252`): past it the record stays shorter
            // than drawn.
            msg = (msg + deficit).min(usize::from(u16::MAX));
        } else {
            let each = deficit / fields.len();
            for &i in &fields {
                extra[i] = each;
            }
            extra[*fields.last().expect("a field")] += deficit - each * fields.len();
        }
        for _ in 0..8 {
            let (r2, len) = rec(&extra, msg)?;
            r = r2;
            if len == plan.len {
                break;
            }
            let delta = plan.len as i64 - len as i64;
            let m = msg as i64 + delta;
            if (1..=i64::from(u16::MAX)).contains(&m) {
                msg = m as usize;
            } else if let Some(&i) = fields.last() {
                extra[i] = (extra[i] as i64 + delta).max(0) as usize;
            } else {
                break;
            }
        }
        Ok(vec![r])
    }

    /// Draws the next commit's plan.
    fn plan(&mut self, hdr: u64, ops_left: Option<u64>) -> Plan {
        let p = self.t.params;
        let body_family = self.t.clause == Clause::Body;
        let fb = family_body(&p);
        let mut len = if self.rng.range(0, 9) < 9 {
            self.rng.range(SHORT_LEN.0, SHORT_LEN.1)
        } else {
            self.rng.log_uniform(SHORT_LEN.1, p.p05)
        };
        if body_family {
            len = len.min(p.p05 - fb - 64);
        }
        let b = self.rng.range(OP_BYTES.0, OP_BYTES.1);
        let mut k = (len.saturating_sub(REC_HDR_LEN as u64 + hdr) / b).max(1);
        if let Some(left) = ops_left {
            k = k.min(left).max(1);
        }
        let mut used = BTreeSet::new();
        let mut creates = 0u32;
        let mut ops = Vec::new();
        let mut cs_body = 0u64;
        if body_family && let Some(op) = self.plan_op(OpKind::SetBody, &mut used, &mut creates, fb)
        {
            cs_body += fb + 24;
            ops.push(op);
        }
        while (ops.len() as u64) < k {
            let mut kind = self.draw_kind();
            if kind == OpKind::SetBody && (body_family || len + cs_body + MIX_BODY + 24 > p.p05) {
                kind = OpKind::SetField;
            }
            if kind == OpKind::SetBody {
                cs_body += MIX_BODY + 24;
            }
            match self.plan_op(kind, &mut used, &mut creates, MIX_BODY) {
                Some(op) => ops.push(op),
                None => {
                    if let Some(op) = self.plan_op(OpKind::SetStatus, &mut used, &mut creates, 0) {
                        ops.push(op);
                    } else {
                        break;
                    }
                }
            }
        }
        // A long commit carries at least one SetField, whose text takes up its length.
        if ops.len() > 50
            && !ops.iter().any(|o| matches!(o, Planned::SetField { .. }))
            && let Some(pos) = ops
                .iter()
                .position(|o| matches!(o, Planned::SetStatus { .. }))
        {
            let id = ops[pos].owner();
            let name = 1;
            let old = self
                .node(id)
                .fields
                .get(&name)
                .cloned()
                .unwrap_or_else(|| vec![0]);
            ops[pos] = Planned::SetField { id, name, old };
        }
        Plan {
            ops,
            len,
            msg_seed: self.rng.next(),
            commit_id: self.rng.bytes(),
            head: self.rng.bytes(),
            idem: (self.rng.bytes(), self.rng.bytes()),
            stmt_hash: self.rng.bytes(),
            digest: self.rng.bytes(),
        }
    }

    /// Applies a placed commit to the state; in a form with a locator, also where each owner's frames lie in it,
    /// counted from `lsn` with the header and the record's `SymDefs` block `defs` included.
    fn commit_state(
        &mut self,
        c: &Commit<'_>,
        lsn: u64,
        creates: u32,
        defs: &[SymDef<'_>],
    ) -> Result<(), String> {
        let mut spans = Vec::new();
        if c.form != Locator::None {
            let mut block = Vec::new();
            if !defs.is_empty() {
                encode_symdefs(defs, &mut block)?;
            }
            let head = (REC_HDR_LEN + block.len()) as u32;
            spans = c
                .op_spans()?
                .into_iter()
                .map(|(owner, at, len)| (owner, PrevLoc { at: head + at, len }))
                .collect();
        }
        for op in &c.ops {
            let id = op.owner();
            let mut n = self.node(id);
            match op {
                Op::Create { kind, image, .. } => {
                    n.live = true;
                    n.kind = *kind;
                    n.status = image.status.unwrap_or((0, 0));
                    n.fields = image
                        .fields
                        .iter()
                        .map(|(k, v)| (*k, v.0.to_vec()))
                        .collect();
                }
                Op::Delete { .. } => n.live = false,
                Op::SetField { name, new, .. } => {
                    n.fields.insert(*name, new.0.to_vec());
                }
                Op::SetStatus { new, .. } => n.status = *new,
                Op::SetBody { new, .. } => n.body = *new,
                Op::AddEdge(e) => {
                    if n.edges.is_empty() {
                        self.edged.push(id);
                    }
                    n.edges.insert((e.ekind, e.dst));
                }
                Op::RemoveEdge(e) => {
                    n.edges.remove(&(e.ekind, e.dst));
                    if n.edges.is_empty() {
                        self.edged.retain(|&i| i != id);
                    }
                }
                Op::Move {
                    new_parent,
                    new_order,
                    ..
                } => {
                    n.parent = *new_parent;
                    n.order = new_order.to_string();
                }
            }
            n.last = Some(lsn);
            n.at = spans.iter().find(|s| s.0 == id).map(|s| s.1);
            self.nodes.insert(id, n);
        }
        self.next_id += creates;
        self.parent = Parent {
            id16: c.commit_id[..16].try_into().expect("16 bytes"),
            lsn,
        };
        self.seq += 1;
        self.hlc += 1_000 << 16;
        self.commits += 1;
        Ok(())
    }

    /// The next settle's group at `lsn`.
    fn settle_group(&mut self, lsn: u64, rows: u64) -> Result<Vec<Record<'static>>, String> {
        let tree = [0xE0 + (self.settles % u64::from(TREES)) as u8; 16];
        let hlc = self.hlc + 1;
        let mut seen = BTreeSet::new();
        let mut out = Vec::with_capacity(rows as usize);
        while (out.len() as u64) < rows {
            let n = self.rng.range(1, u64::from(self.base.n)) as u32;
            if !seen.insert(n) && seen.len() < self.base.n as usize {
                continue;
            }
            let mut file_id = [0u8; 57];
            file_id[0] = 1;
            file_id[1..49].copy_from_slice(&self.rng.bytes::<48>());
            let mut times = [[0u8; 9]; 4];
            for t in times.iter_mut().take(3) {
                t[..8].copy_from_slice(
                    &(1_700_000_000_000_000_000i64 + (self.rng.next() >> 20) as i64).to_le_bytes(),
                );
                t[8] = 2;
            }
            times[3][8] = 0xFF;
            let mut last_oid = [0u8; 33];
            last_oid[0] = 1;
            last_oid[1..21].copy_from_slice(&self.rng.bytes::<20>());
            let row = FileObsRow {
                n,
                tree,
                state: 1,
                resolver_version: 1,
                file_id,
                size: self.rng.range(1, 1 << 20),
                times,
                attrs: 0,
                last_oid,
                verified_at: hlc,
                missing_since: 0,
            };
            out.push(Row {
                op: 1,
                image: Cow::Owned(row.image().to_vec()),
            });
        }
        let mut g = vec![
            Record {
                lsn,
                group_end: false,
                symdefs: Vec::new(),
                payload: Payload::TreeReg(TreeReg {
                    tree,
                    epoch: Epoch {
                        scope_kind: 0,
                        scope_ref: 0,
                        digest: [0; 16],
                        hlc,
                    },
                }),
            },
            Record {
                lsn: 0,
                group_end: true,
                symdefs: Vec::new(),
                payload: Payload::FileObs(RowBatch { rows: out }),
            },
        ];
        sequence(lsn, &mut g)?;
        Ok(g)
    }
}

/// The measures a group adds: (records, `rec_bytes`, `body_bytes`, `rt_bytes`, ops, `loc_bytes`).
#[allow(clippy::type_complexity)]
fn group_measures(group: &[Record<'_>]) -> Result<(u64, u64, u64, u64, u64, u64), String> {
    let (mut rec, mut body, mut rt, mut ops, mut loc) = (0, 0, 0, 0, 0);
    for r in group {
        let len = r.encoded_len()?;
        let b = match &r.payload {
            Payload::Commit(c) => {
                ops += c.ops.len() as u64;
                loc += c.locs.iter().map(|(_, l)| loc_len(c.form, *l)).sum::<u64>();
                c.body_bytes()
            }
            _ => 0,
        };
        if in_k_rt(r.kind()) {
            rt += len;
        } else {
            rec += len - b;
        }
        body += b;
    }
    Ok((group.len() as u64, rec, body, rt, ops, loc))
}

/// Generates the tail of `t`; keeps every record when `keep` is set.
pub fn generate(t: &Target, keep: bool) -> Result<Tail, String> {
    let p = t.params;
    let geo = p.geometry();
    let mut rng = Rng(t.seed ^ 0x6D31_305F_7461_696C);
    let mut store_id: [u8; 16] = rng.bytes();
    store_id[0] |= 1;
    let expect = Expect {
        geo,
        epoch: rng.next(),
        epoch_lsn: 0,
        init: InitParams {
            log_extent_bytes: p.e,
            hist_frame_commits: p.hist_frame_commits,
            hist_frame_bytes: p.hist_frame_bytes,
            store_id,
        }
        .encode(),
        project_oid_algo: 1,
        locator: p.shape.locator,
    };
    let start = p.e;
    let chain_in = rng.next();
    let base = Base {
        n: t.n,
        kind: BASE_KIND,
        statuses: STATUSES,
    };
    let seq0 = 1_000;
    let hlc0 = 1_790_000_000_000u64 << 16;
    let mut g = Gen {
        t: *t,
        parent: Parent {
            id16: rng.bytes(),
            lsn: start - 1,
        },
        rng,
        base,
        nodes: HashMap::new(),
        edged: Vec::new(),
        next_id: t.n + 1,
        start,
        seq: seq0,
        hlc: hlc0,
        commits: 0,
        settles: 0,
        session: SYMTAB_SESSIONS + 1,
        stmt: SYMTAB_STMTS + 1,
    };
    let head = ExtentHead {
        epoch_lsn: 0,
        chain_in: 0,
        init: expect.init,
        project_oid_algo: 1,
        hflags: 0,
        commit_seq: seq0 - 1,
        next_id: t.n + 1,
        next_anchor: 1,
        fence: 1,
        next_file_no: 9,
        next_ref_id: 1,
        hlc_seq: hlc0 - (1_000 << 16),
        hlc_commit: hlc0 - (1_000 << 16),
    };
    let mut app = Appender::new(geo, expect.epoch, start, chain_in, head);
    let mut builder = Builder::new(start, base);
    let mut m = Measures::default();
    let mut records = Vec::new();
    let mut refused = None;
    let rows = settle_rows(&p);
    let (q6, q7, q8) = (t.mult * p.p06, t.mult * p.p07, t.mult * p.p08);
    // The header part of a commit with a 1-byte message and one affected id.
    let hdr = {
        let plan = Plan {
            ops: Vec::new(),
            len: 0,
            msg_seed: 0,
            commit_id: [0; 32],
            head: [0; 20],
            idem: ([0; 16], [0; 16]),
            stmt_hash: [0; 16],
            digest: [0; 32],
        };
        let mut c = g.commit(&plan, start, &[], 1);
        c.affected = Some(Affected {
            complete: true,
            ids: vec![t.n],
        });
        let mut out = Vec::new();
        c.encode(&mut out)?;
        out.len() as u64 - 2
    };
    let max_bytes = 1u64 << 31;
    loop {
        let settle = g.commits > 0
            && g.commits.is_multiple_of(COMMITS_PER_SETTLE)
            && g.settles < g.commits / COMMITS_PER_SETTLE;
        let ops_left = (t.clause == Clause::Ops).then(|| t.threshold - m.ops);
        let plan = if settle {
            None
        } else {
            Some(g.plan(hdr, ops_left))
        };
        let creates = plan.as_ref().map_or(0, |p| {
            p.ops
                .iter()
                .filter(|o| matches!(o, Planned::Create { .. }))
                .count() as u32
        });
        let mut lsn = app.next_lsn();
        let mut group = match &plan {
            Some(p) => g.commit_group(p, lsn)?,
            None => g.settle_group(lsn, rows)?,
        };
        let mut tries = 0;
        loop {
            let len: u64 = group
                .iter()
                .map(|r| r.encoded_len())
                .sum::<Result<u64, String>>()?;
            let (to, _) = app.slot(len)?;
            if to == lsn {
                break;
            }
            lsn = to;
            match &plan {
                // A commit's `prev` deltas follow its lsn: rebuild it there.
                Some(p) => group = g.commit_group(p, lsn)?,
                None => sequence(lsn, &mut group)?,
            }
            tries += 1;
            if tries > 3 {
                return Err("a group's placement did not settle".into());
            }
        }
        let pre = lsn - app.pos();
        let pre_records = match pre {
            0 => 0,
            EXTENT_HEAD_GROUP_LEN => 1,
            _ => 2,
        };
        let (n_rec, rec, body, rt, ops, loc) = group_measures(&group)?;
        for r in &group {
            builder.apply(r);
        }
        let next = Measures {
            ops: m.ops + ops,
            rec_bytes: m.rec_bytes + pre + rec,
            body_bytes: m.body_bytes + body,
            rt_bytes: m.rt_bytes + rt,
            loc_bytes: m.loc_bytes + loc,
            ovl: builder.charge(),
            records: m.records + pre_records + n_rec,
            groups: m.groups + pre_records + 1,
            commits: m.commits + u64::from(plan.is_some()),
            settles: m.settles + u64::from(plan.is_none()),
            bytes: lsn - start
                + group
                    .iter()
                    .map(|r| r.encoded_len())
                    .sum::<Result<u64, String>>()?,
        };
        let over = match t.clause {
            Clause::Rec => next.rec_bytes > t.threshold,
            Clause::Ovl => next.ovl > t.threshold,
            Clause::Quiet => {
                next.ops > q6
                    || next.rec_bytes > q7
                    || next.body_bytes > q8
                    || next.ovl > t.threshold
            }
            Clause::Ops | Clause::Body => false,
        };
        // A tail holds at least one commit: when the first alone passes the threshold, the tail is that commit
        // (a writer's own commit may take a tail past its threshold, `17-store-parameters.md:354`).
        if over && m.commits > 0 {
            refused = Some(next);
            break;
        }
        let prelude = app.append(&group)?;
        if let Some(Payload::Commit(c)) = group.first().map(|r| &r.payload) {
            g.commit_state(c, lsn, creates, &group[0].symdefs)?;
            app.head.commit_seq = g.seq - 1;
            app.head.next_id = g.next_id;
            app.head.hlc_seq = g.hlc - (1_000 << 16);
            app.head.hlc_commit = g.hlc - (1_000 << 16);
        } else {
            g.settles += 1;
        }
        m = next;
        if keep {
            records.extend(prelude);
            records.extend(group);
        }
        let done = match t.clause {
            Clause::Ops => m.ops >= t.threshold,
            Clause::Body => m.body_bytes >= t.threshold,
            _ => false,
        };
        if done {
            break;
        }
        if m.bytes > max_bytes {
            return Err(format!(
                "the tail passed {max_bytes} bytes without reaching its threshold"
            ));
        }
    }
    if m.rt_bytes > t.rt_cap() {
        return Err(format!(
            "rt_bytes {} would pass C2's bound {} (F17 §5.2): the generator refuses this target",
            m.rt_bytes,
            t.rt_cap()
        ));
    }
    let bytes = app.into_bytes();
    debug_assert_eq!(bytes.len() as u64, m.bytes);
    let end = start + bytes.len() as u64;
    Ok(Tail {
        bytes,
        expect,
        start,
        end,
        chain_in,
        base,
        measures: m,
        refused,
        records,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::tail::enc::tests::config;
    use crate::tail::rec::encode;
    use crate::tail::scan::decode;
    use crate::tail::{Geometry, kind};
    use proptest::prelude::*;

    /// A target at the test profile ([F17 §3], `17-store-parameters.md:166-178`) with the profile's threshold of
    /// `clause`.
    pub(crate) fn test_target(clause: Clause, n: u32, seed: u64) -> Target {
        let p = Params::TEST;
        let (threshold, mult) = match clause {
            Clause::Ops => (p.p06, 1),
            Clause::Rec => (p.p07, 1),
            Clause::Body => (p.p08, 1),
            Clause::Ovl => (p.p09, 1),
            Clause::Quiet => (p.p10, p.p11),
        };
        Target {
            clause,
            threshold,
            mult,
            n,
            seed,
            params: p,
            rt_bound: None,
        }
    }

    /// A long test-profile tail that is not a grid point: an `ops` tail of `ops` ops, its C2 bound lifted.
    fn long(ops: u64, n: u32, seed: u64) -> Tail {
        let mut t = test_target(Clause::Ops, n, seed);
        t.threshold = ops;
        t.rt_bound = Some(u64::MAX);
        generate(&t, true).unwrap()
    }

    /// `t` with its tail in A1 (u20 cut §3.1).
    pub(crate) fn a1(mut t: Target) -> Target {
        t.params.shape = crate::tail::Shape::A1;
        t
    }

    /// [`long`] in A1.
    fn long_a1(ops: u64, n: u32, seed: u64) -> Tail {
        let mut t = a1(test_target(Clause::Ops, n, seed));
        t.threshold = ops;
        t.rt_bound = Some(u64::MAX);
        generate(&t, true).unwrap()
    }

    /// The commits of `tail`'s bytes, decoded under its own form, with their lsns.
    fn decoded_commits(tail: &Tail) -> Vec<(u64, Commit<'_>)> {
        decode(&tail.bytes, tail.start, &tail.expect, tail.chain_in)
            .unwrap()
            .into_iter()
            .filter_map(|r| match r.payload {
                Payload::Commit(c) => Some((r.lsn, *c)),
                _ => None,
            })
            .collect()
    }

    const CLAUSES: [Clause; 5] = [
        Clause::Ops,
        Clause::Rec,
        Clause::Body,
        Clause::Ovl,
        Clause::Quiet,
    ];

    fn m_commits(recs: &[Record<'_>]) -> usize {
        recs.iter().filter(|r| r.kind() == kind::COMMIT).count()
    }

    fn commits(tail: &Tail) -> Vec<&Commit<'static>> {
        tail.records
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(c) => Some(&**c),
                _ => None,
            })
            .collect()
    }

    /// The decoder reads back the generator's tail byte for byte (spec item 1): `decode(T)` is the generator's records
    /// and `encode(decode(T)) == T`, for every clause.
    #[test]
    fn decoder_reads_back_the_generators_tail_byte_for_byte() {
        for clause in CLAUSES {
            let tail = generate(&test_target(clause, 1_000_000, 7), true).unwrap();
            assert!(tail.records.len() >= 3, "{clause:?}: a non-empty tail");
            let recs = decode(&tail.bytes, tail.start, &tail.expect, tail.chain_in).unwrap();
            assert_eq!(recs, tail.records, "{clause:?}");
            let again = encode(
                &recs,
                tail.expect.geo,
                tail.start,
                tail.expect.epoch,
                tail.chain_in,
            );
            assert_eq!(again.as_ref(), Ok(&tail.bytes), "{clause:?}");
            assert_eq!(tail.end - tail.start, tail.bytes.len() as u64);
            assert_eq!(tail.measures.bytes, tail.bytes.len() as u64);
        }
    }

    /// Each family stops at its rule ([F17 §5.2]–§5.3; a full tail is a tail at the threshold, `:353-354`): `ops`
    /// and `body` exactly at P06 and P08; `rec`, `ovl` and `quiet` at most at their thresholds, the next group
    /// passing one.
    #[test]
    fn generated_tail_stops_at_its_clause_threshold() {
        let p = Params::TEST;
        for seed in 1..=4 {
            let m = |c| generate(&test_target(c, 1_000_000, seed), false).unwrap();
            assert_eq!(m(Clause::Ops).measures.ops, p.p06);
            assert_eq!(m(Clause::Body).measures.body_bytes, p.p08);
            let rec = m(Clause::Rec);
            assert!(rec.measures.rec_bytes <= p.p07 && rec.measures.rec_bytes > 0);
            assert!(
                rec.refused.unwrap().rec_bytes > p.p07,
                "the next group passes P07"
            );
            let ovl = m(Clause::Ovl);
            assert!(ovl.measures.ovl <= p.p09 && ovl.measures.ovl > 0);
            assert!(
                ovl.refused.unwrap().ovl > p.p09,
                "the next group passes P09"
            );
            let q = m(Clause::Quiet);
            let (mq, r) = (q.measures, q.refused.unwrap());
            assert!(mq.ops <= 2 * p.p06 && mq.rec_bytes <= 2 * p.p07 && mq.body_bytes <= 2 * p.p08);
            assert!(mq.ovl <= p.p10);
            assert!(
                r.ops > 2 * p.p06
                    || r.rec_bytes > 2 * p.p07
                    || r.body_bytes > 2 * p.p08
                    || r.ovl > p.p10,
                "the next group passes one quiet clause"
            );
        }
        // The production design figures and the grid's candidates are those of F17 (`:170-176`, `:757-762`).
        let pp = Params::PRODUCTION;
        assert_eq!(
            (pp.p06, pp.p07, pp.p08, pp.p09, pp.p10, pp.p11),
            (4_096, 4 << 20, 32 << 20, 1 << 20, 2 << 20, 8)
        );
    }

    /// Commit lengths ([60 §5.2] item 2, `60-roadmap.md:758`): about 9 in 10 in [300, 600] B, the rest above 600 up to
    /// P05, counted without body data, over more than 1,000 commits.
    #[test]
    fn commit_sizes_follow_the_spec_distribution() {
        let tail = long(2_000, 1_000_000, 11);
        let lens: Vec<u64> = tail
            .records
            .iter()
            .filter_map(|r| match &r.payload {
                Payload::Commit(c) => Some(r.encoded_len().unwrap() - c.body_bytes()),
                _ => None,
            })
            .collect();
        assert!(lens.len() > 1_000, "{} commits", lens.len());
        let short = lens.iter().filter(|&&l| (300..=600).contains(&l)).count();
        let share = short as f64 / lens.len() as f64;
        assert!(
            (0.86..=0.94).contains(&share),
            "share in [300, 600]: {share}"
        );
        assert!(
            lens.iter().all(|&l| l <= Params::TEST.p05),
            "every commit within P05"
        );
        assert!(
            lens.iter().all(|&l| l >= 300),
            "none below the drawn minimum"
        );
        assert!(
            lens.iter().any(|&l| l > 2_000),
            "the log-uniform tail reaches above 2 KB"
        );
        // cs_bytes ≤ P05: no commit is a bulk commit (W1).
        assert!(
            tail.bytes.len() < 1 << 20,
            "the test tail stays below 1 MiB"
        );
    }

    /// [70 S8]'s fix (`70-audit-speed.md:298-300`): one settle per 5 commits, each a `TreeReg` epoch and a `FileObs`
    /// batch whose group stays within 16,000 B; at production 83 rows (15,892 B), 84 would pass it.
    #[test]
    fn lazy_share_follows_70_s8() {
        let tail = long(400, 1_000_000, 3);
        let m = tail.measures;
        assert!(m.commits >= 200);
        assert_eq!(
            m.settles,
            m.commits / COMMITS_PER_SETTLE,
            "one settle after every 5th commit"
        );
        // Each settle follows the 5th, 10th, … commit.
        let mut since = 0;
        let mut i = 0;
        while i < tail.records.len() {
            match &tail.records[i].payload {
                Payload::Commit(_) => since += 1,
                Payload::TreeReg(_) => {
                    assert_eq!(since, COMMITS_PER_SETTLE, "a settle after 5 commits");
                    since = 0;
                    let Payload::FileObs(b) = &tail.records[i + 1].payload else {
                        panic!("a FileObs batch after the epoch")
                    };
                    assert_eq!(b.rows.len() as u64, settle_rows(&Params::TEST));
                    let g = tail.records[i].encoded_len().unwrap()
                        + tail.records[i + 1].encoded_len().unwrap();
                    assert!(g <= SETTLE_GROUP_CAP);
                    i += 1;
                }
                _ => {}
            }
            i += 1;
        }
        assert_eq!(settle_rows(&Params::TEST), 1);
        assert_eq!(settle_rows(&Params::PRODUCTION), 83);
        let prod = |rows: u32| {
            crate::tail::testdata::settle_group(0, 1, 1, rows, 1)
                .iter()
                .map(|r| r.encoded_len().unwrap())
                .sum::<u64>()
        };
        assert_eq!(prod(83), 15_892);
        assert!(prod(83) <= SETTLE_GROUP_CAP && prod(84) > SETTLE_GROUP_CAP);
    }

    /// C2 ([F17 §5.2], `17-store-parameters.md:329-333`): every grid-shaped tail keeps `rt_bytes` within `m·q·P12`,
    /// and the generator refuses a target that would break it.
    #[test]
    fn generated_tails_respect_c2() {
        let p = Params::TEST;
        for clause in CLAUSES {
            for seed in 1..=3 {
                let t = test_target(clause, 1_000_000, seed);
                let tail = generate(&t, false).unwrap();
                let cap = if clause == Clause::Quiet {
                    p.p11 * p.p12
                } else {
                    p.p13 * p.p12
                };
                assert_eq!(t.rt_cap(), cap);
                assert!(
                    tail.measures.rt_bytes <= cap,
                    "{clause:?}: {}",
                    tail.measures.rt_bytes
                );
            }
        }
        let mut big = test_target(Clause::Ops, 1_000_000, 1);
        big.threshold = 400;
        let e = generate(&big, false).unwrap_err();
        assert!(e.contains("C2"), "{e}");
    }

    /// Every op's `prev` ([F06 §7.3], `06-commit.md:705-722`) names its owner's previous commit in the tail, is 0 for a
    /// node the commit creates, and leaves the tail for a base node's first op; owners repeat at N = 50.
    #[test]
    fn generated_prev_chains_link_each_owner() {
        let tail = long(600, 50, 5);
        let mut last: HashMap<u32, u64> = HashMap::new();
        let mut repeats = 0;
        let mut checked = 0;
        for r in &tail.records {
            let Payload::Commit(c) = &r.payload else {
                continue;
            };
            for op in &c.ops {
                let o = op.owner();
                match (op, last.get(&o)) {
                    (Op::Create { prev, .. }, _) => assert_eq!(*prev, 0),
                    (_, Some(&l)) => {
                        assert_eq!(r.lsn - op.prev(), l, "#{o} links to its previous commit");
                        repeats += 1;
                    }
                    (_, None) => {
                        assert!(
                            o <= 50,
                            "only a base node is first touched without a Create"
                        );
                        assert_eq!(
                            r.lsn - op.prev(),
                            tail.start - 1,
                            "a base node's chain leaves the tail"
                        );
                    }
                }
                checked += 1;
            }
            for op in &c.ops {
                last.insert(op.owner(), r.lsn);
            }
        }
        assert!(
            checked > 500 && repeats > 100,
            "{checked} ops, {repeats} repeated owners"
        );
    }

    /// A test-profile tail with its threshold raised crosses at least 4 extents of 64 KiB, each opened by its head and
    /// the earlier ones closed by a pad (G-3), and still decodes.
    #[test]
    fn generated_tail_crosses_extents_at_the_test_profile() {
        let tail = long(1_200, 1_000_000, 9);
        let geo = Geometry::new(Params::TEST.e).unwrap();
        let first = geo.extent(tail.start);
        let last = geo.extent(tail.end - 1);
        assert!(last - first + 1 >= 4, "{} extents", last - first + 1);
        let heads = tail
            .records
            .iter()
            .filter(|r| r.kind() == kind::EXTENT_HEAD)
            .count() as u64;
        assert_eq!(heads, last - first + 1, "one head per extent");
        let pads = tail
            .records
            .iter()
            .filter(|r| r.kind() == kind::NOOP)
            .count() as u64;
        assert!(pads >= 3 && pads < heads, "{pads} pads");
        assert!(tail.bytes.len() < 1 << 20);
        assert_eq!(
            decode(&tail.bytes, tail.start, &tail.expect, tail.chain_in).unwrap(),
            tail.records
        );
    }

    /// A tail is a function of its target: the same seed gives the same bytes, another seed others.
    #[test]
    fn generator_is_deterministic_per_seed() {
        for clause in CLAUSES {
            let a = generate(&test_target(clause, 100_000, 42), false).unwrap();
            let b = generate(&test_target(clause, 100_000, 42), false).unwrap();
            let c = generate(&test_target(clause, 100_000, 43), false).unwrap();
            assert!(!a.bytes.is_empty());
            assert_eq!(a.bytes, b.bytes, "{clause:?}");
            assert_eq!(a.measures, b.measures);
            assert_ne!(a.bytes, c.bytes, "{clause:?}");
        }
    }

    /// A production-profile generator built by hand for `commit_group` alone. One commit is already placed, so a
    /// record carries no `SymDefs` block and its payload is the commit. `commit_group` draws nothing (`&self`), so
    /// `rng` is never used.
    fn bare_gen() -> Gen {
        let p = Params::PRODUCTION;
        let n = 1_000;
        Gen {
            t: Target {
                clause: Clause::Ops,
                threshold: p.p06,
                mult: 1,
                n,
                seed: 0,
                params: p,
                rt_bound: None,
            },
            rng: Rng(0),
            base: Base {
                n,
                kind: BASE_KIND,
                statuses: STATUSES,
            },
            nodes: HashMap::new(),
            edged: Vec::new(),
            next_id: n + 1,
            start: p.e,
            seq: 1_000,
            hlc: 1_790_000_000_000u64 << 16,
            parent: Parent {
                id16: [7; 16],
                lsn: p.e - 1,
            },
            commits: 1,
            settles: 0,
            session: SYMTAB_SESSIONS + 1,
            stmt: SYMTAB_STMTS + 1,
        }
    }

    /// A plan of `ops` whose drawn length is `len`.
    fn plan_of(ops: Vec<Planned>, len: u64) -> Plan {
        Plan {
            ops,
            len,
            msg_seed: 3,
            commit_id: [1; 32],
            head: [2; 20],
            idem: ([3; 16], [4; 16]),
            stmt_hash: [5; 16],
            digest: [6; 32],
        }
    }

    /// `len − body data` of `plan`'s record at `lsn` with a message of `msg` bytes and no text added to a
    /// `SetField`, built as `commit_group` builds it.
    fn len_at(g: &Gen, plan: &Plan, lsn: u64, msg: usize) -> u64 {
        let c = g.commit(plan, lsn, &vec![0; plan.ops.len()], msg);
        let body = c.body_bytes();
        let r = Record {
            lsn,
            group_end: true,
            symdefs: g.symdefs(),
            payload: Payload::Commit(Box::new(c)),
        };
        r.encoded_len().unwrap() - body
    }

    /// R-38: when no `SetField` can carry a long drawn length, the message stops at F06's 65,535 bytes
    /// (`06-commit.md:252`) and the record is shorter than drawn. A length that the capped message reaches, or one a
    /// step below it, is still met exactly, and a `SetField` still carries a long length with a 1-byte message. Every
    /// case runs, and a red names each failing case with the encoder's message.
    #[test]
    fn message_stops_at_f06s_cap_when_no_set_field_carries_the_length() {
        let g = bare_gen();
        let lsn = g.start;
        let cap = usize::from(u16::MAX);
        let status = |len| plan_of(vec![Planned::SetStatus { id: 1, new: 2 }], len);
        let at_cap = len_at(&g, &status(0), lsn, cap);
        // The message's `vstr` length prefix grows from 1 byte to 3 between 1 and 65,535 bytes.
        assert_eq!(at_cap, len_at(&g, &status(0), lsn, 1) + 65_534 + 2);
        let field = plan_of(
            vec![Planned::SetField {
                id: 1,
                name: 1,
                old: TValue::absent().0.into_owned(),
            }],
            200_000,
        );
        // (case, plan, the record's expected `len − body data`, the expected message length)
        let cases = [
            ("c", status(at_cap - 2), at_cap - 2, cap - 2),
            ("a", status(200_000), at_cap, cap),
            ("b", status(at_cap), at_cap, cap),
            ("e", status(at_cap - 1), at_cap - 1, cap - 1),
            ("d", field, 200_000, 1),
        ];
        assert!(at_cap < 200_000, "case (a) is shorter than drawn");
        let mut failed = Vec::new();
        for (case, plan, want_len, want_msg) in &cases {
            let group = match g.commit_group(plan, lsn) {
                Ok(group) => group,
                Err(e) => {
                    failed.push(format!("case ({case}): {e}"));
                    continue;
                }
            };
            assert_eq!(group.len(), 1, "case ({case}): one record");
            let r = &group[0];
            let Payload::Commit(c) = &r.payload else {
                panic!("case ({case}): a Commit record")
            };
            let len = r.encoded_len().unwrap() - c.body_bytes();
            let msg = c.msg.as_ref().map_or(0, |m| m.len());
            if (len, msg) != (*want_len, *want_msg) {
                failed.push(format!(
                    "case ({case}): length {len} with a message of {msg} bytes, not {want_len} with {want_msg}"
                ));
                continue;
            }
            // The record decodes back equal: with no `SymDefs` block, its payload is the commit alone.
            assert!(r.symdefs.is_empty(), "case ({case})");
            let mut payload = Vec::new();
            r.encode_payload(&mut payload).unwrap();
            let back = Commit::decode(&payload).unwrap_or_else(|e| panic!("case ({case}): {e}"));
            assert_eq!(back, **c, "case ({case})");
        }
        assert!(failed.is_empty(), "{}", failed.join("; "));
    }

    /// R-38, the night's failure at its smallest: a production `ops` tail of one op is one commit. For each of 256
    /// seeds the tail generates and decodes back equal; every commit whose message stops at F06's cap carries no
    /// `SetField`, and at least one seed reaches the cap.
    #[test]
    fn production_tails_complete_past_the_message_cap() {
        let cap = usize::from(u16::MAX);
        let mut hits = 0;
        for seed in 0..256 {
            let t = Target {
                clause: Clause::Ops,
                threshold: 1,
                mult: 1,
                n: 1_000,
                seed,
                params: Params::PRODUCTION,
                rt_bound: None,
            };
            let tail = generate(&t, true).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            assert_eq!(tail.measures.ops, 1, "seed {seed}");
            let recs = decode(&tail.bytes, tail.start, &tail.expect, tail.chain_in)
                .unwrap_or_else(|e| panic!("seed {seed}: {e}"));
            assert_eq!(recs, tail.records, "seed {seed}");
            for c in commits(&tail) {
                if c.msg.as_ref().is_some_and(|m| m.len() == cap) {
                    hits += 1;
                    assert!(
                        !c.ops.iter().any(|o| matches!(o, Op::SetField { .. })),
                        "seed {seed}: a capped message beside a SetField"
                    );
                }
            }
        }
        assert!(hits >= 1, "{hits} of 256 seeds reach the message cap");
    }

    /// A1's locator names the owner's frames in the record its `prev` names (u20 cut §3.1, SoT-1; `design-A.md:109-112`):
    /// over A1 test-profile tails at N = 50 (owners repeat), decoded from their bytes, every owner's first op with
    /// `prev` ≠ 0 carries a locator, and when `prev` names a record of the tail, the locator's `prev_len` bytes at
    /// `prev_at` from that record's lsn decode to exactly the owner's ops there, the first in the first-of-owner form.
    #[test]
    fn a1_locator_names_the_owners_frames_in_the_record_prev_names() {
        let mut checked = 0;
        for seed in [5, 6] {
            let tail = long_a1(600, 50, seed);
            let commits = decoded_commits(&tail);
            let by_lsn: BTreeMap<u64, &Commit<'_>> = commits.iter().map(|(l, c)| (*l, c)).collect();
            for (lsn, c) in &commits {
                assert_eq!(c.form, Locator::A1);
                for (i, op) in c.ops.iter().enumerate() {
                    let o = op.owner();
                    if (i > 0 && c.ops[i - 1].owner() == o) || op.prev() == 0 {
                        continue;
                    }
                    let Some(&(_, loc)) = c.locs.iter().find(|(x, _)| *x == o) else {
                        panic!("seed {seed} lsn {lsn}: no locator in #{o}'s first frame");
                    };
                    let older = lsn - op.prev();
                    let Some(prev_rec) = by_lsn.get(&older) else {
                        assert!(
                            older < tail.start,
                            "seed {seed} lsn {lsn} #{o}: prev {older}"
                        );
                        continue;
                    };
                    assert!(
                        loc.at as usize > REC_HDR_LEN && loc.len > 0,
                        "#{o}: {loc:?}"
                    );
                    let from = (older - tail.start) as usize + loc.at as usize;
                    let mut r =
                        crate::tail::enc::Reader::new(&tail.bytes[from..from + loc.len as usize]);
                    let mut got = Vec::new();
                    let mut last = None;
                    while r.remaining() > 0 {
                        let (x, _) = Op::decode_in(&mut r, Locator::A1, last).unwrap_or_else(|e| {
                            panic!("seed {seed} #{o} at {older} + {}: {e}", loc.at)
                        });
                        last = Some(x.owner());
                        got.push(x);
                    }
                    let want: Vec<&Op<'_>> =
                        prev_rec.ops.iter().filter(|x| x.owner() == o).collect();
                    assert_eq!(
                        got.iter().collect::<Vec<_>>(),
                        want,
                        "seed {seed} lsn {lsn} #{o}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 100, "{checked} locators into the tail");
    }

    /// SoT-4 (u20 cut §3.1; `design-A.md:120-122`): a locator whose `prev` names a record below the tail's start, a base
    /// node's first op in the tail, is (0, 0); every locator into the tail is not. At N = 1e6 most owners are base nodes
    /// touched once, so both cases occur.
    #[test]
    fn a1_locator_is_zero_below_the_checkpoint() {
        let (mut below, mut inside) = (0, 0);
        for clause in CLAUSES {
            let tail = generate(&a1(test_target(clause, 1_000_000, 7)), true).unwrap();
            for (lsn, c) in decoded_commits(&tail) {
                for (o, loc) in &c.locs {
                    let op = c.ops.iter().find(|x| x.owner() == *o).unwrap();
                    if lsn - op.prev() < tail.start {
                        assert_eq!(*loc, PrevLoc::default(), "{clause:?} lsn {lsn} #{o}");
                        below += 1;
                    } else {
                        assert_ne!(*loc, PrevLoc::default(), "{clause:?} lsn {lsn} #{o}");
                        inside += 1;
                    }
                }
            }
        }
        let tail = long_a1(600, 50, 5);
        for (lsn, c) in decoded_commits(&tail) {
            for (o, loc) in &c.locs {
                let op = c.ops.iter().find(|x| x.owner() == *o).unwrap();
                if lsn - op.prev() >= tail.start {
                    assert_ne!(*loc, PrevLoc::default());
                    inside += 1;
                }
            }
        }
        assert!(below > 20 && inside > 20, "{below} below, {inside} inside");
    }

    /// M-A5's count (u20 cut D-10): `loc_bytes` is the sum of the A1 fields' bytes in the tail's decoded commits, part of
    /// `rec_bytes`; it is 0 in the night's form, whose tails are the same plans without them.
    #[test]
    fn loc_bytes_count_the_a1_fields() {
        for clause in CLAUSES {
            let t = test_target(clause, 1_000_000, 7);
            let night = generate(&t, false).unwrap();
            assert_eq!(night.measures.loc_bytes, 0, "{clause:?}");
            let tail = generate(&a1(t), true).unwrap();
            let sum: u64 = decoded_commits(&tail)
                .iter()
                .flat_map(|(_, c)| c.locs.iter().map(|(_, l)| loc_len(c.form, *l)))
                .sum();
            assert_eq!(tail.measures.loc_bytes, sum, "{clause:?}");
            assert!(
                sum > 0 && sum < tail.measures.rec_bytes,
                "{clause:?}: {sum}"
            );
        }
        let tail = long_a1(600, 50, 5);
        let pairs: u64 = decoded_commits(&tail)
            .iter()
            .map(|(_, c)| c.locs.len() as u64)
            .sum();
        assert!(tail.measures.loc_bytes >= 2 * pairs && pairs > 100);
    }

    /// The identity rule (u20 cut D-3; R-38, R-40): a tail in the night's form is byte-identical to ba4fc23's. The
    /// xxh3-64 of chosen tails, computed at ba4fc23 in a scratch worktree: each clause at the test profile (N = 1e6,
    /// seed 7), [`long`]`(600, 50, 5)`, production `ops` tails of one op for seeds 0–3, the hand-built tail, and a
    /// production `ops` tail of 64 ops (N = 1e6, seed 99).
    #[test]
    fn night_form_tails_are_unchanged() {
        use xxhash_rust::xxh3::xxh3_64;
        let mut got: Vec<u64> = Vec::new();
        for clause in CLAUSES {
            let t = test_target(clause, 1_000_000, 7);
            assert_eq!(t.params.shape, crate::tail::Shape::NIGHT);
            got.push(xxh3_64(&generate(&t, false).unwrap().bytes));
        }
        got.push(xxh3_64(&long(600, 50, 5).bytes));
        for seed in 0..4 {
            let t = Target {
                clause: Clause::Ops,
                threshold: 1,
                mult: 1,
                n: 1_000,
                seed,
                params: Params::PRODUCTION,
                rt_bound: None,
            };
            got.push(xxh3_64(&generate(&t, false).unwrap().bytes));
        }
        got.push(xxh3_64(&crate::tail::testdata::hand_built().bytes));
        let t = Target {
            clause: Clause::Ops,
            threshold: 64,
            mult: 1,
            n: 1_000_000,
            seed: 99,
            params: Params::PRODUCTION,
            rt_bound: None,
        };
        got.push(xxh3_64(&generate(&t, false).unwrap().bytes));
        assert_eq!(
            got,
            [
                0x4a20_2b6b_05a8_79ab,
                0xf4e2_1e08_c730_d9df,
                0x59e2_02b8_718e_3ea8,
                0x109a_0f47_0f66_84f5,
                0x4a20_2b6b_05a8_79ab,
                0xc124_4784_f4cd_9fbe,
                0x9915_1f95_1532_6f66,
                0x256d_c71f_9e2d_4401,
                0x3743_05f8_feda_a220,
                0x363f_7f07_ea2c_554a,
                0xb55e_aebf_cc35_d351,
                0x5354_863d_c7a2_93b3,
            ],
            "a night-form tail moved"
        );
    }

    proptest! {
        #![proptest_config(config())]

        /// Random seeds, clauses and N at the test profile: every tail decodes, re-encodes byte for byte and keeps
        /// its measures within its thresholds.
        #[test]
        fn generated_tails_decode(seed in any::<u64>(), c in 0usize..5, n in 50u32..2_000_000) {
            let t = test_target(CLAUSES[c], n, seed);
            let tail = generate(&t, true).unwrap();
            let recs = decode(&tail.bytes, tail.start, &tail.expect, tail.chain_in).unwrap();
            prop_assert!(recs.len() >= 2 && m_commits(&recs) >= 1);
            prop_assert_eq!(&recs, &tail.records);
            let again = encode(&recs, tail.expect.geo, tail.start, tail.expect.epoch, tail.chain_in).unwrap();
            prop_assert_eq!(&again, &tail.bytes);
            let m = tail.measures;
            let p = Params::TEST;
            match CLAUSES[c] {
                Clause::Ops => prop_assert_eq!(m.ops, p.p06),
                Clause::Rec => prop_assert!(m.rec_bytes <= p.p07 || m.commits == 1),
                Clause::Body => prop_assert_eq!(m.body_bytes, p.p08),
                Clause::Ovl => prop_assert!(m.ovl <= p.p09 || m.commits == 1),
                Clause::Quiet => {
                    prop_assert!((m.ovl <= p.p10 && m.ops <= 2 * p.p06) || m.commits == 1);
                }
            }
            prop_assert!(m.rt_bytes <= t.rt_cap());
            prop_assert_eq!(commits(&tail).len() as u64, m.commits);
        }
    }
}
