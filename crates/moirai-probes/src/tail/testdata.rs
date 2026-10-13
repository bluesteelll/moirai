//! Hand-built tails for the unit tests: every byte follows from the chapters' rules through the encoder, with digests
//! as fixed bytes (no fixture is copied from an informative example, [F01 §2.1]).

use super::commit::{Affected, Body, Commit, Edge, Git, Image, Op, Parent, TValue, blake3_16};
use super::rec::{
    Appender, ExtentHead, InitParams, Payload, Record, SymDef, encode_symdefs, sequence, sym,
};
use super::rt::{Epoch, FileObsRow, Row, RowBatch, TreeReg};
use super::scan::Expect;
use super::{Geometry, Locator, PrevLoc, REC_HDR_LEN};
use std::borrow::Cow;

/// E = 2^16, the test profile's extent ([F17] P01, `17-store-parameters.md:166`).
pub const E: u64 = 1 << 16;
/// The tail starts at extent 2's first byte.
pub const START: u64 = E;
/// The chain value the tail's first extent head carries.
pub const CHAIN_IN: u64 = 0x0C0F_FEE0_1234_5678;

/// The `HEAD` fields the scan checks, in the night's form.
pub fn expect() -> Expect {
    expect_in(Locator::None)
}

/// The `HEAD` fields the scan checks, with the tail's locator in `form`.
pub fn expect_in(form: Locator) -> Expect {
    Expect {
        geo: Geometry::new(E).unwrap(),
        epoch: 0x1122_3344_5566_7788,
        epoch_lsn: 0,
        init: InitParams {
            log_extent_bytes: E,
            hist_frame_commits: 4,
            hist_frame_bytes: 4_096,
            store_id: [0x5A; 16],
        }
        .encode(),
        project_oid_algo: 1,
        locator: form,
    }
}

/// The extent-head template of [`expect`].
pub fn head(x: &Expect) -> ExtentHead {
    ExtentHead {
        epoch_lsn: x.epoch_lsn,
        chain_in: 0,
        init: x.init,
        project_oid_algo: x.project_oid_algo,
        hflags: 0,
        commit_seq: 100,
        next_id: 51,
        next_anchor: 1,
        fence: 7,
        next_file_no: 9,
        next_ref_id: 1,
        hlc_seq: 1 << 40,
        hlc_commit: 1 << 40,
    }
}

/// A local ordinary commit on `main` with every group of the subset present.
pub fn commit(seq: u64, parent_lsn: u64, ops: Vec<Op<'static>>, msg: &str) -> Commit<'static> {
    let mut ids: Vec<u32> = ops.iter().map(Op::owner).collect();
    ids.dedup();
    Commit {
        commit_id: [seq as u8; 32],
        parent: Some(Parent {
            id16: [seq as u8 ^ 0xFF; 16],
            lsn: parent_lsn,
        }),
        gen_no: seq as u32,
        seq,
        ref_sym: 1,
        ref_id: 0,
        ref_old: Some([seq as u8 ^ 0xFF; 16]),
        prev_on_ref: Some(parent_lsn),
        ref_seq: seq as u32,
        hlc: (1_790_000_000_000 + seq) << 16,
        actor: 5,
        role: 2,
        session: 17,
        schema_version: 1,
        git: Some(Git {
            algo: 1,
            head: Some(Cow::Owned(vec![0xAB; 20])),
            branch: 1,
            worktree: 1,
            base: None,
        }),
        idem: Some(([0x11; 16], [0x22; 16])),
        stmt_origin: 1,
        actor_src: 1,
        stmt_sym: 9,
        stmt_hash: Some([0x33; 16]),
        msg: Some(Cow::Owned(msg.to_string())),
        affected: Some(Affected {
            complete: true,
            ids,
        }),
        changeset_digest: [0x44; 32],
        ops,
        form: Locator::None,
        locs: Vec::new(),
        bodies: Vec::new(),
    }
}

/// Each placed commit's lsn and where each owner's frames lie in it, in op order.
pub type Placed = Vec<(u64, Vec<(u32, PrevLoc)>)>;

/// A change to the locators the writer would write (u20 cut §3.3's refused pointers): given the owner, its true locator
/// and the spans of every owner in the record its `prev` names, the locator to write.
pub type Perturb<'p> = &'p dyn Fn(u32, PrevLoc, &[(u32, PrevLoc)]) -> PrevLoc;

/// The locators of `ops` at `lsn` in `form` (u20 cut §3.1, SoT-1 and SoT-4): for each owner whose first op has
/// `prev` ≠ 0, where its frames lie in the record `prev` names, from the commits `placed` so far, passed through
/// `perturb`; (0, 0) for a record below the tail.
pub fn locs_at(
    form: Locator,
    ops: &[Op<'_>],
    lsn: u64,
    placed: &Placed,
    perturb: Perturb<'_>,
) -> Vec<(u32, PrevLoc)> {
    if form == Locator::None {
        return Vec::new();
    }
    let mut locs = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        if (i == 0 || ops[i - 1].owner() != op.owner()) && op.prev() != 0 {
            let older = lsn - op.prev();
            let loc = placed
                .iter()
                .find(|p| p.0 == older)
                .and_then(|p| {
                    let own = p.1.iter().find(|s| s.0 == op.owner())?;
                    Some(perturb(op.owner(), own.1, &p.1))
                })
                .unwrap_or_default();
            locs.push((op.owner(), loc));
        }
    }
    locs
}

/// Where each owner's frames lie in the commit `c` placed with `defs` (its `SymDefs` block), counted from its lsn with
/// the header included, in op order.
pub fn spans_of(c: &Commit<'_>, defs: &[SymDef<'_>]) -> Vec<(u32, PrevLoc)> {
    let mut block = Vec::new();
    if !defs.is_empty() {
        encode_symdefs(defs, &mut block).expect("valid symbols");
    }
    let head = (REC_HDR_LEN + block.len()) as u32;
    c.op_spans()
        .expect("an encodable commit")
        .into_iter()
        .map(|(owner, at, len)| (owner, PrevLoc { at: head + at, len }))
        .collect()
}

/// The two symbols the first commit defines (SD-1: the next ids after an assumed `SYMTAB` of 16 sessions and 8 stmts).
pub fn symdefs() -> Vec<SymDef<'static>> {
    vec![
        SymDef {
            class: sym::SESSION,
            id: 17,
            text: Cow::Borrowed("probe:session-1"),
        },
        SymDef {
            class: sym::STMT,
            id: 9,
            text: Cow::Borrowed("tx.claim"),
        },
    ]
}

/// A one-record commit group at `lsn`.
pub fn commit_group(
    lsn: u64,
    c: Commit<'static>,
    defs: Vec<SymDef<'static>>,
) -> Vec<Record<'static>> {
    vec![Record {
        lsn,
        group_end: true,
        symdefs: defs,
        payload: Payload::Commit(Box::new(c)),
    }]
}

/// A settle group at `lsn`: a `TreeReg` epoch and a `FileObs` batch of `rows` rows for files `first..`.
pub fn settle_group(lsn: u64, tree: u8, first: u32, rows: u32, hlc: u64) -> Vec<Record<'static>> {
    let row = |n: u32| FileObsRow {
        n,
        tree: [tree; 16],
        state: 1,
        resolver_version: 1,
        file_id: [1; 57],
        size: u64::from(n) * 10,
        times: [[0; 9]; 4],
        attrs: 0,
        last_oid: [0; 33],
        verified_at: hlc,
        missing_since: 0,
    };
    let mut g = vec![
        Record {
            lsn,
            group_end: false,
            symdefs: Vec::new(),
            payload: Payload::TreeReg(TreeReg {
                tree: [tree; 16],
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
            payload: Payload::FileObs(RowBatch {
                rows: (first..first + rows)
                    .map(|n| Row {
                        op: 1,
                        image: Cow::Owned(row(n).image().to_vec()),
                    })
                    .collect(),
            }),
        },
    ];
    sequence(lsn, &mut g).unwrap();
    g
}

/// The ops of one commit, given each owner's newest commit lsn so far and the commit's own lsn.
type OpsFn = dyn Fn(&[(u32, u64)], u64) -> Vec<Op<'static>>;

/// A hand-built tail and the records it holds, preludes included, in log order.
pub struct HandBuilt {
    /// What the scan checks it against, its form included.
    pub expect: Expect,
    /// The bytes from [`START`].
    pub bytes: Vec<u8>,
    /// Every record.
    pub records: Vec<Record<'static>>,
    /// The lsn of each commit record, in order.
    pub commits: Vec<u64>,
}

/// The tail of the overlay and scan tests (base `N` = 50): an extent head, three commits that touch #3, #5 and #7
/// more than once and create #51 and #52, a settle, three large commits of which the third rotates the tail into
/// extent 3 after a pad, a commit after the rotation and a second settle.
pub fn hand_built() -> HandBuilt {
    hand_built_in(Locator::None)
}

/// [`hand_built`] with its op frames in `form`: in A1 each owner's first frame names its frames in the record its
/// `prev` names (#3's two frames in the second commit, `Move` and `SetField`, are one span), or (0, 0) below the tail.
pub fn hand_built_in(form: Locator) -> HandBuilt {
    hand_built_with(form, &|_, loc, _| loc)
}

/// [`hand_built_in`] with every locator the writer writes passed through `perturb` (u20 cut §3.3: a pointer one frame
/// off, of a wrong length, or (0, 0) into the tail).
pub fn hand_built_with(form: Locator, perturb: Perturb<'_>) -> HandBuilt {
    let x = expect_in(form);
    let mut app = Appender::new(x.geo, x.epoch, START, CHAIN_IN, head(&x));
    let mut records = Vec::new();
    let mut commits: Vec<u64> = Vec::new();
    let below = START - 1;
    // `prev` of `owner` at `lsn`: back to its newest commit in `last`, or to the notional record below the tail.
    let prev = move |last: &[(u32, u64)], owner: u32, lsn: u64| {
        last.iter()
            .rev()
            .find(|(o, _)| *o == owner)
            .map_or(lsn - below, |(_, l)| lsn - l)
    };
    let mut last: Vec<(u32, u64)> = Vec::new();
    let body = vec![0x5Bu8; 300];
    let bh = blake3_16(&body);
    let plans: Vec<Box<OpsFn>> = vec![
        Box::new(move |l, lsn| {
            vec![
                Op::SetStatus {
                    id: 3,
                    prev: prev(l, 3, lsn),
                    old: (3, 0),
                    new: (1, 0),
                },
                Op::SetField {
                    id: 5,
                    prev: prev(l, 5, lsn),
                    name: 2,
                    old: TValue::absent(),
                    new: TValue::text("first"),
                },
                Op::Create {
                    id: 51,
                    prev: 0,
                    uid: [0xC1; 16],
                    kind: 2,
                    c_actor: 5,
                    c_role: 2,
                    image: Image {
                        status: Some((0, 0)),
                        body: None,
                        fields: vec![(1, TValue::text("title 51"))],
                    },
                },
            ]
        }),
        Box::new(move |l, lsn| {
            vec![
                Op::Move {
                    id: 3,
                    prev: prev(l, 3, lsn),
                    old_parent: 0,
                    new_parent: 2,
                    old_order: Cow::Borrowed(""),
                    new_order: Cow::Borrowed("a0"),
                },
                Op::SetField {
                    id: 3,
                    prev: prev(l, 3, lsn),
                    name: 4,
                    old: TValue::absent(),
                    new: TValue::int(42),
                },
                Op::AddEdge(Edge {
                    src: 7,
                    prev: prev(l, 7, lsn),
                    ekind: 1,
                    dst: 51,
                }),
                Op::SetBody {
                    id: 51,
                    prev: prev(l, 51, lsn),
                    old: None,
                    new: Some(bh),
                },
            ]
        }),
        Box::new(move |l, lsn| {
            vec![
                Op::SetField {
                    id: 5,
                    prev: prev(l, 5, lsn),
                    name: 2,
                    old: TValue::text("first"),
                    new: TValue::text("second"),
                },
                // `reason` and `replaced_by` differ, so the tail's round trip and the overlay's pinned values catch a
                // swap of the two (R-28); both still encode as one-byte `uvar32`s, so no offset moves.
                Op::Delete {
                    id: 9,
                    prev: prev(l, 9, lsn),
                    reason: 3,
                    replaced_by: 52,
                    before: Image {
                        status: Some((1, 0)),
                        body: None,
                        fields: Vec::new(),
                    },
                },
                Op::Create {
                    id: 52,
                    prev: 0,
                    uid: [0x0A; 16],
                    kind: 1,
                    c_actor: 5,
                    c_role: 2,
                    image: Image::default(),
                },
            ]
        }),
        Box::new(move |l, lsn| {
            vec![
                Op::RemoveEdge(Edge {
                    src: 7,
                    prev: prev(l, 7, lsn),
                    ekind: 1,
                    dst: 51,
                }),
                Op::SetStatus {
                    id: 51,
                    prev: prev(l, 51, lsn),
                    old: (0, 0),
                    new: (2, 1),
                },
            ]
        }),
    ];
    let mut seq = 101;
    let mut placed: Placed = Vec::new();
    let mut put = |app: &mut Appender,
                   records: &mut Vec<Record<'static>>,
                   commits: &mut Vec<u64>,
                   last: &mut Vec<(u32, u64)>,
                   ops: &OpsFn,
                   msg: &str,
                   bodies: Vec<Body<'static>>| {
        let parent = commits.last().copied().unwrap_or(below);
        let defs = if commits.is_empty() {
            symdefs()
        } else {
            Vec::new()
        };
        let (pre, g) = app
            .place(|lsn| {
                let mut c = commit(seq, parent, ops(last, lsn), msg);
                c.bodies = bodies.clone();
                c.form = form;
                c.locs = locs_at(form, &c.ops, lsn, &placed, perturb);
                Ok(commit_group(lsn, c, defs.clone()))
            })
            .unwrap();
        records.extend(pre);
        let lsn = g[0].lsn;
        if let Payload::Commit(c) = &g[0].payload {
            for op in &c.ops {
                last.push((op.owner(), lsn));
            }
            placed.push((lsn, spans_of(c, &defs)));
        }
        commits.push(lsn);
        records.extend(g);
        seq += 1;
    };
    put(
        &mut app,
        &mut records,
        &mut commits,
        &mut last,
        &*plans[0],
        "claim --start",
        Vec::new(),
    );
    put(
        &mut app,
        &mut records,
        &mut commits,
        &mut last,
        &*plans[1],
        "edit",
        vec![Body {
            hash: bh,
            data: Cow::Owned(body.clone()),
        }],
    );
    let (pre, g) = app
        .place(|lsn| Ok(settle_group(lsn, 0xE1, 10, 2, 5 << 40)))
        .unwrap();
    records.extend(pre);
    records.extend(g);
    put(
        &mut app,
        &mut records,
        &mut commits,
        &mut last,
        &*plans[2],
        "rework",
        Vec::new(),
    );
    // Three commits with 25-KB messages: the third does not fit extent 2 and rotates.
    let big = "m".repeat(25_000);
    let filler = |id: u32| {
        move |l: &[(u32, u64)], lsn: u64| {
            vec![Op::SetField {
                id,
                prev: prev(l, id, lsn),
                name: 6,
                old: TValue::absent(),
                new: TValue::text("x"),
            }]
        }
    };
    put(
        &mut app,
        &mut records,
        &mut commits,
        &mut last,
        &filler(20),
        &big,
        Vec::new(),
    );
    put(
        &mut app,
        &mut records,
        &mut commits,
        &mut last,
        &filler(21),
        &big,
        Vec::new(),
    );
    put(
        &mut app,
        &mut records,
        &mut commits,
        &mut last,
        &filler(22),
        &big,
        Vec::new(),
    );
    put(
        &mut app,
        &mut records,
        &mut commits,
        &mut last,
        &*plans[3],
        "after rotation",
        Vec::new(),
    );
    let (pre, g) = app
        .place(|lsn| Ok(settle_group(lsn, 0xE1, 11, 1, 6 << 40)))
        .unwrap();
    records.extend(pre);
    records.extend(g);
    HandBuilt {
        expect: x,
        bytes: app.into_bytes(),
        records,
        commits,
    }
}
