//! The typed merge on hand-built states: the rows of each class, the rules over conflict-valued bases ([F12 §5.4]),
//! Kleppmann's moves, the existence policies, and properties over random edits of both sides.

use super::*;
use crate::state::{Creator, EdgeProps, Tomb};
use proptest::prelude::*;

fn uid(n: Nid) -> Uid {
    let mut b = [0u8; 16];
    b[12..].copy_from_slice(&n.0.to_be_bytes());
    Uid(b)
}

fn no_nid(_: Uid) -> Option<Nid> {
    None
}

/// A context with no commit steps on either side: every move is a side's value in its first step (RS-007 as the WP-91
/// review's spec finding S1b states it), as for a merge whose base is not where the sides' commits start.
fn ctx<'a>(auto: &'a BTreeMap<String, String>, op: Op) -> Ctx<'a> {
    Ctx {
        op,
        dst_main: false,
        dst_plan: false,
        policy: None,
        auto,
        start: Start::Base,
        moves: [&[], &[]],
        uid: &uid,
        nid: &no_nid,
        origin: None,
    }
}

fn run(b: &State, o: &State, t: &State, op: Op) -> Merged {
    let auto = BTreeMap::new();
    merge(b, o, t, &ctx(&auto, op), &mut Fresh::default())
}

/// The steps of a side's commits, each turning one state of `states` into the next, with order keys (10·(i + 1),
/// [i + 1; 32]): built as [`crate::dag::Dag::move_steps`] builds a real side's, from each commit's net changeset.
fn steps(states: &[&State]) -> Vec<Step> {
    states
        .windows(2)
        .enumerate()
        .filter_map(|(i, w)| {
            let c = i as u64 + 1;
            Step::of((10 * c, [c as u8; 32]), &crate::state::diff(w[0], w[1]))
        })
        .collect()
}

/// The step of one commit that turned `from` into `to`, with order key (hlc, [id; 32]).
fn step(hlc: u64, id: u8, from: &State, to: &State) -> Vec<Step> {
    Step::of((hlc, [id; 32]), &crate::state::diff(from, to))
        .into_iter()
        .collect()
}

fn cs(st: &State) -> canon::Cs {
    canon::canonical_state(st, &uid)
}

/// The base every case edits: tasks #1–#4 and a note #5.
fn base() -> State {
    let s = Schema::default();
    let mut st = State::default();
    for n in 1..=5u32 {
        let kind = if n == 5 { "note" } else { "task" };
        let mut x = Node::new(uid(Nid(n)), kind, &s, Creator::default());
        x.set_field(&s, "title", Some(Value::Text(format!("t{n}"))));
        st.nodes.insert(Nid(n), x);
    }
    st
}

/// One edit a side makes to the base.
#[derive(Clone, Debug)]
enum Edit {
    Priority(u32, u8),
    Title(u32, u8),
    Status(u32, bool),
    Counter(u32, i64),
    Body(u32, u8),
    Edge(u32, u32, bool),
    Labels(u32, u8),
    Delete(u32),
    /// Moves a node under another, or to the root with `None`; skipped when it would close a cycle.
    Move(u32, Option<u32>),
}

impl Edit {
    fn owner(&self) -> u32 {
        match self {
            Edit::Priority(n, _)
            | Edit::Title(n, _)
            | Edit::Status(n, _)
            | Edit::Counter(n, _)
            | Edit::Body(n, _)
            | Edit::Edge(n, _, _)
            | Edit::Labels(n, _)
            | Edit::Delete(n)
            | Edit::Move(n, _) => *n,
        }
    }

    /// The other node the edit names: an edge's destination or the new parent.
    fn other(&self) -> Option<u32> {
        match self {
            Edit::Edge(_, d, _) => Some(*d),
            Edit::Move(_, p) => *p,
            _ => None,
        }
    }
}

/// The operations the properties run: a virtual merge (no validators) and a real one (the validators of [F13 §5]).
const OPS: [Op; 2] = [Op::Virtual, Op::Merge];

fn edit() -> impl Strategy<Value = Edit> {
    let n = 1u32..5;
    prop_oneof![
        (n.clone(), 0u8..4).prop_map(|(a, b)| Edit::Priority(a, b)),
        (n.clone(), 0u8..3).prop_map(|(a, b)| Edit::Title(a, b)),
        (n.clone(), any::<bool>()).prop_map(|(a, b)| Edit::Status(a, b)),
        (n.clone(), 1i64..3).prop_map(|(a, b)| Edit::Counter(a, b)),
        (n.clone(), 0u8..3).prop_map(|(a, b)| Edit::Body(a, b)),
        (n.clone(), 1u32..6, any::<bool>()).prop_map(|(a, b, c)| Edit::Edge(a, b, c)),
        (n.clone(), 0u8..4).prop_map(|(a, b)| Edit::Labels(a, b)),
        n.clone().prop_map(Edit::Delete),
        (n, proptest::option::of(1u32..5)).prop_map(|(a, p)| Edit::Move(a, p)),
    ]
}

/// A forest over the five nodes: each node's parent, index 0 for #1. It is drawn as an order of the nodes and, for each
/// node, the root or a node before it in the order, so that every forest can come out.
fn forest() -> impl Strategy<Value = [Option<u32>; 5]> {
    (
        Just(vec![1u32, 2, 3, 4, 5]).prop_shuffle(),
        proptest::collection::vec(0usize..6, 5),
    )
        .prop_map(|(order, picks)| {
            let mut parent = [None; 5];
            for (i, n) in order.iter().enumerate() {
                let pick = picks[i] % (i + 1);
                parent[*n as usize - 1] = (pick > 0).then(|| order[pick - 1]);
            }
            parent
        })
}

/// A state with the parents of a forest.
fn with_forest(st: &State, f: &[Option<u32>; 5]) -> State {
    let mut s = st.clone();
    for (i, p) in f.iter().enumerate() {
        s.nodes
            .get_mut(&Nid(i as u32 + 1))
            .expect("a base node")
            .parent = p.map(Nid);
    }
    s
}

/// Whether `n` is `p` or an ancestor of `p` in a state's parent forest.
fn above(st: &State, n: Nid, p: Nid) -> bool {
    let mut x = Some(p);
    for _ in 0..=st.nodes.len() {
        match x {
            None => return false,
            Some(y) if y == n => return true,
            Some(y) => x = st.nodes.get(&y).and_then(|z| z.parent),
        }
    }
    true
}

fn apply(st: &mut State, edits: &[Edit]) {
    let s = Schema::default();
    for e in edits {
        let n = Nid(e.owner());
        match e {
            // A move under a live node that would not close a cycle, so each side's state stays a forest.
            Edit::Move(_, Some(p)) if st.live(Nid(*p)).is_none() || above(st, n, Nid(*p)) => {
                continue;
            }
            // A node with a live child is not deleted, so no live node is left under a tombstone.
            Edit::Delete(_) if st.nodes.values().any(|y| y.live() && y.parent == Some(n)) => {
                continue;
            }
            _ => {}
        }
        let Some(x) = st.nodes.get_mut(&n).filter(|x| x.live()) else {
            continue;
        };
        match e {
            Edit::Priority(_, p) => x.set_field(&s, "priority", Some(Value::Enum(format!("P{p}")))),
            Edit::Title(_, t) => x.set_field(&s, "title", Some(Value::Text(format!("title {t}")))),
            Edit::Status(_, d) => {
                x.status = if *d { "done" } else { "in_progress" }.into();
                x.resolution = if *d { "completed" } else { "none" }.into();
            }
            Edit::Counter(_, d) => {
                let c = match x.fields.get("reopen_count") {
                    Some(Value::Counter(c)) => *c,
                    _ => 0,
                };
                x.set_field(&s, "reopen_count", Some(Value::Counter(c + d)));
            }
            Edit::Body(_, b) => x.body = Some(format!("line a\nline {b}\nline c\n")),
            Edit::Edge(_, d, on) => {
                let k = EdgeKey {
                    kind: "relates".into(),
                    dst: Nid(*d),
                    disc: None,
                };
                if *on && Nid(*d) != n {
                    x.out.insert(k, EdgeProps::default());
                } else {
                    x.out.remove(&k);
                }
            }
            Edit::Labels(_, l) => {
                let mut v: Vec<Value> = x
                    .fields
                    .get("labels")
                    .map(|v| v.elems().to_vec())
                    .unwrap_or_default();
                v.push(Value::Text(format!("l{l}")));
                x.set_field(&s, "labels", Value::set(v));
            }
            Edit::Delete(_) => {
                x.tomb = Some(Tomb {
                    reason: Some("gone".into()),
                    replaced_by: None,
                });
                x.fields.retain(|f, _| f == "title");
                x.body = None;
                x.parent = None;
                x.out.clear();
            }
            Edit::Move(_, p) => x.parent = p.map(Nid),
        }
    }
}

proptest! {
    /// A side that did not move gives the other side's state ([RULES/merge-table] ours-only, theirs-only rows), its
    /// hierarchy moves included (with no commit steps, the side's values move in its first step) and with no violation
    /// of a real merge; two equal sides give that state (`same`).
    #[test]
    fn one_sided_and_equal_merges_take_the_changed_side(es in proptest::collection::vec(edit(), 0..12)) {
        let b = base();
        let mut x = b.clone();
        apply(&mut x, &es);
        for op in OPS {
            for (o, t) in [(&b, &x), (&x, &b)] {
                let m = run(&b, o, t, op);
                prop_assert!(canon::entries(&cs(&m.st), &cs(&x)).is_empty(), "{op:?} {:?}", m.rows);
                prop_assert!(m.conflicts.is_empty());
                prop_assert!(
                    !m.violations.iter().any(|v| v.class == "HierarchyCycle"),
                    "{op:?} {:?}",
                    m.violations
                );
            }
            // Equal sides: every key but a counter, whose two deltas both count (RS-004).
            let m = run(&b, &x, &x, op);
            let no_counters = |c: canon::Cs| -> canon::Cs {
                c.into_iter().filter(|(k, _)| !matches!(k, canon::CKey::Node { class: 6, .. })).collect()
            };
            prop_assert!(canon::entries(&no_counters(cs(&m.st)), &no_counters(cs(&x))).is_empty());
            prop_assert!(m.conflicts.is_empty());
        }
    }

    /// Changes to disjoint nodes commute: the merge holds both ([RULES/merge-table] PR-008, PR-017). Each side's edges
    /// and moves stay among its own nodes.
    #[test]
    fn disjoint_changes_commute(eo in proptest::collection::vec(edit(), 0..10), et in proptest::collection::vec(edit(), 0..10)) {
        let b = base();
        let eo: Vec<Edit> = eo
            .into_iter()
            .filter(|e| e.owner() <= 2 && e.other().is_none_or(|d| d <= 2))
            .collect();
        let et: Vec<Edit> = et
            .into_iter()
            .filter(|e| e.owner() >= 3 && e.other().is_none_or(|d| d >= 3))
            .collect();
        let (mut o, mut t, mut both) = (b.clone(), b.clone(), b.clone());
        apply(&mut o, &eo);
        apply(&mut t, &et);
        apply(&mut both, &eo);
        apply(&mut both, &et);
        for op in OPS {
            let m = run(&b, &o, &t, op);
            prop_assert!(canon::entries(&cs(&m.st), &cs(&both)).is_empty(), "{op:?}");
        }
    }

    /// A merge is a function of its inputs, and exchanging the sides exchanges each value conflict's sides on the keys
    /// no directional row decides ([F12 §7.7]).
    #[test]
    fn exchanging_the_sides_exchanges_the_conflicts(eo in proptest::collection::vec(edit(), 0..8), et in proptest::collection::vec(edit(), 0..8)) {
        let b = base();
        let (mut o, mut t) = (b.clone(), b.clone());
        apply(&mut o, &eo);
        apply(&mut t, &et);
        for op in OPS {
            let m1 = run(&b, &o, &t, op);
            let m1b = run(&b, &o, &t, op);
            prop_assert_eq!(&m1.st, &m1b.st);
            prop_assert_eq!(&m1.violations, &m1b.violations);
            let m2 = run(&b, &t, &o, op);
            exchanged(&m1, &m2)?;
        }
    }
}

/// The conflict keys of two merges with exchanged sides are equal, and each value conflict's sides are exchanged.
fn exchanged(m1: &Merged, m2: &Merged) -> Result<(), TestCaseError> {
    let k1: BTreeSet<Key> = m1.conflicts.iter().map(|c| c.0.clone()).collect();
    let k2: BTreeSet<Key> = m2.conflicts.iter().map(|c| c.0.clone()).collect();
    prop_assert_eq!(&k1, &k2);
    for (n, x) in &m1.st.nodes {
        let Some(y) = m2.st.nodes.get(n) else {
            continue;
        };
        for (a, c) in &x.conflicts {
            if *a == Aspect::Existence {
                continue;
            }
            let d = &y.conflicts[a];
            prop_assert_eq!(&c.class, &d.class);
            prop_assert_eq!(&c.ours, &d.theirs);
            prop_assert_eq!(&c.theirs, &d.ours);
        }
    }
    Ok(())
}

/// [F12 §5.4]: over a conflict-valued base, equal sides are clean (RVB-1), one untouched side takes the other's value
/// (RVB-2, RVB-3), and two sides that changed it differently conflict with the inner base (RVB-4).
#[test]
fn keys_over_a_conflict_valued_base_follow_rvb_1_to_4() {
    let s = Schema::default();
    let k = Aspect::Field("priority".into());
    let p = |x: &str| Some(KVal::Value(Value::Enum(x.into())));
    let mut b = base();
    let c = Conflict {
        class: "FieldEdit".into(),
        base: p("P3"),
        ours: p("P0"),
        theirs: p("P1"),
        prov: None,
        images: [None, None, None],
    };
    b.nodes
        .get_mut(&Nid(1))
        .unwrap()
        .set_kstate(&s, &k, KState::Conflict(Box::new(c)));
    let with = |v: Option<KVal>| {
        let mut x = b.clone();
        x.nodes
            .get_mut(&Nid(1))
            .unwrap()
            .set_kstate(&s, &k, KState::Plain(v));
        x
    };
    let key = Key::Node(Nid(1), k.clone());
    let get = |m: &Merged| m.st.kstate(&key);
    // RVB-1: both resolved to P0.
    let m = run(&b, &with(p("P0")), &with(p("P0")), Op::Merge);
    assert_eq!(get(&m), KState::Plain(p("P0")));
    assert!(m.conflicts.is_empty());
    // RVB-2: dst untouched, src resolved.
    let m = run(&b, &b, &with(p("P1")), Op::Merge);
    assert_eq!(get(&m), KState::Plain(p("P1")));
    // RVB-3: src untouched, dst resolved.
    let m = run(&b, &with(p("P0")), &b, Op::Merge);
    assert_eq!(get(&m), KState::Plain(p("P0")));
    // RVB-4: resolved differently: FieldEdit over the inner base.
    let m = run(&b, &with(p("P0")), &with(p("P1")), Op::Merge);
    match get(&m) {
        KState::Conflict(c) => {
            assert_eq!(c.class, "FieldEdit");
            assert_eq!(c.base, p("P3"));
            assert_eq!((c.ours, c.theirs), (p("P0"), p("P1")));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(m.rows[&key], "MR-002");
}

/// Text edited in different lines merges by diff3 (MR-031), in one line conflicts (MR-032); a counter sums (MR-026); a
/// set is `union3` (MR-027).
#[test]
fn typed_rules_merge_text_counters_and_sets() {
    let b = {
        let mut b = base();
        let x = b.nodes.get_mut(&Nid(1)).unwrap();
        x.body = Some("a\nb\nc\n".into());
        x.fields.insert("reopen_count".into(), Value::Counter(2));
        x.fields.insert(
            "labels".into(),
            Value::set(vec![Value::Text("k".into()), Value::Text("r".into())]).unwrap(),
        );
        b
    };
    let side = |body: &str, count: i64, labels: &[&str]| {
        let mut s = b.clone();
        let x = s.nodes.get_mut(&Nid(1)).unwrap();
        x.body = Some(body.into());
        x.fields
            .insert("reopen_count".into(), Value::Counter(count));
        x.fields.insert(
            "labels".into(),
            Value::set(labels.iter().map(|l| Value::Text(l.to_string())).collect()).unwrap(),
        );
        s
    };
    let o = side("A\nb\nc\n", 3, &["k", "o"]);
    let t = side("a\nb\nC\n", 5, &["k", "r", "t"]);
    let m = run(&b, &o, &t, Op::Merge);
    let x = &m.st.nodes[&Nid(1)];
    assert_eq!(x.body.as_deref(), Some("A\nb\nC\n"));
    assert_eq!(x.fields["reopen_count"], Value::Counter(6));
    assert_eq!(
        x.fields["labels"],
        Value::set(vec![
            Value::Text("k".into()),
            Value::Text("o".into()),
            Value::Text("t".into())
        ])
        .unwrap()
    );
    let t2 = side("a\nX\nc\n", 2, &["k", "r"]);
    let o2 = side("a\nY\nc\n", 2, &["k", "r"]);
    let m = run(&b, &o2, &t2, Op::Merge);
    assert_eq!(
        m.conflicts,
        vec![(Key::Node(Nid(1), Aspect::Body), "TextHunk".to_string())]
    );
}

/// Kleppmann's moves in (hlc, commit id) order; a move that would close a cycle is skipped and stages as
/// `HierarchyCycle` (MR-039, V01), silently inside a virtual merge (VBC-10).
#[test]
fn a_cycle_closing_move_is_skipped() {
    let b = base();
    let under = |st: &State, c: u32, p: u32| {
        let mut s = st.clone();
        s.nodes.get_mut(&Nid(c)).unwrap().parent = Some(Nid(p));
        s
    };
    let o = under(&b, 1, 2);
    let t = under(&b, 2, 1);
    let auto = BTreeMap::new();
    let (mo, mt) = (step(10, 1, &b, &o), step(20, 2, &b, &t));
    let mut cx = ctx(&auto, Op::Merge);
    cx.moves = [&mo, &mt];
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert_eq!(m.st.nodes[&Nid(1)].parent, Some(Nid(2)));
    assert_eq!(
        m.st.nodes[&Nid(2)].parent,
        None,
        "the later move is skipped"
    );
    assert_eq!(m.violations.len(), 1);
    assert_eq!(m.violations[0].class, "HierarchyCycle");
    cx.op = Op::Virtual;
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert!(m.violations.is_empty());
}

/// A commit that swaps a parent and its child moves both at one order key: its two moves are one Kleppmann step, so a
/// merge with an untouched other side takes the swap and records no `HierarchyCycle` (the WP-91 review's spec finding
/// S1; applied one at a time from the base, the first move met a cycle and was skipped).
#[test]
fn a_swap_in_one_commit_is_one_step() {
    let mut b = base();
    b.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(1));
    let mut x = b.clone();
    x.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(2));
    x.nodes.get_mut(&Nid(2)).unwrap().parent = None;
    let auto = BTreeMap::new();
    let swap = step(10, 1, &b, &x);
    assert_eq!(swap[0].moves.len(), 2, "one commit moves both");
    for op in OPS {
        for (o, t, moves) in [(&x, &b, [&swap[..], &[]]), (&b, &x, [&[], &swap[..]])] {
            let mut cx = ctx(&auto, op);
            cx.moves = moves;
            let m = merge(&b, o, t, &cx, &mut Fresh::default());
            assert_eq!(m.st.nodes[&Nid(1)].parent, Some(Nid(2)), "{op:?}");
            assert_eq!(m.st.nodes[&Nid(2)].parent, None, "{op:?}");
            assert!(m.violations.is_empty(), "{op:?}: {:?}", m.violations);
        }
    }
}

/// A step that closes a cycle with an earlier move skips its moves one at a time, the least uid on the cycle first,
/// until the forest has none: ours' commit (10) puts #1 under #2; theirs' one commit (20) puts #2 under #3 and #3
/// under #1; #2's move is skipped and #3's stays (MR-039, V01).
#[test]
fn a_cycle_closing_step_skips_the_least_uid_first() {
    let b = base();
    let mut o = b.clone();
    o.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(2));
    let mut t = b.clone();
    t.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(3));
    t.nodes.get_mut(&Nid(3)).unwrap().parent = Some(Nid(1));
    let auto = BTreeMap::new();
    let (mo, mt) = (step(10, 1, &b, &o), step(20, 2, &b, &t));
    let mut cx = ctx(&auto, Op::Merge);
    cx.moves = [&mo, &mt];
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
    assert_eq!(
        (parent(1), parent(2), parent(3)),
        (Some(Nid(2)), None, Some(Nid(1)))
    );
    assert_eq!(
        m.violations
            .iter()
            .map(|v| (v.class, v.key.clone()))
            .collect::<Vec<_>>(),
        vec![("HierarchyCycle", Some(Key::Node(Nid(2), Aspect::Hierarchy)))]
    );
}

/// A move that sets its node's current value is never undone and never makes its key `kleppmann-skipped`
/// ([RULES/merge-table] RS-007, MR-039, CS-013; [RULES/merge-table] open point 35 case (iii), decided by [AR §11]
/// OQ-A-6 (a)). Ours' commit (10) puts #3 under #2 and #1 under #3; theirs' commit (20) puts #1 under #3 too and #2
/// under #1. Replayed after ours', theirs' step leaves #1 where it is and closes the cycle #2 → #1 → #3 → #2: #1 has
/// the least uid of the step on the cycle, but its move changed nothing, so only #2's move is undone. Only `#2.parent`
/// stages, the sides' real disagreement; `#1.parent`, which both sides hold under #3, is clean (MR-040). When ours then
/// puts #2 under #4 in a still later commit (30), that move applies and decides #2's value, and nothing stages.
#[test]
fn a_cycle_closing_step_never_undoes_its_no_op_move() {
    let b = base();
    let mut o = b.clone();
    o.nodes.get_mut(&Nid(3)).unwrap().parent = Some(Nid(2));
    o.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(3));
    let mut t = b.clone();
    t.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(3));
    t.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(1));
    let auto = BTreeMap::new();
    let (mo, mt) = (step(10, 1, &b, &o), step(20, 2, &b, &t));
    assert!(
        mt[0].moves.iter().any(|(n, _)| *n == Nid(1)),
        "theirs' step holds #1's move"
    );
    assert!(uid(Nid(1)) < uid(Nid(2)), "#1 has the lower uid");
    let mut cx = ctx(&auto, Op::Merge);
    cx.moves = [&mo, &mt];
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
    assert_eq!(
        (parent(1), parent(2), parent(3)),
        (Some(Nid(3)), None, Some(Nid(2)))
    );
    let hc = |n: u32| Key::Node(Nid(n), Aspect::Hierarchy);
    assert_eq!(
        m.violations
            .iter()
            .map(|v| (v.class, v.key.clone()))
            .collect::<Vec<_>>(),
        vec![("HierarchyCycle", Some(hc(2)))]
    );
    assert_eq!(
        (
            m.rows[&hc(1)].as_str(),
            m.rows[&hc(2)].as_str(),
            m.rows[&hc(3)].as_str()
        ),
        ("MR-040", "MR-039", "MR-040")
    );
    // Inside a virtual merge the same undo is silent (VBC-10).
    cx.op = Op::Virtual;
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert!(m.violations.is_empty());
    assert_eq!(m.st.nodes[&Nid(2)].parent, None);
    // Ours' later move of #2 applies after theirs' step: nothing stages.
    let mut o2 = o.clone();
    o2.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(4));
    let mut mo2 = mo.clone();
    mo2.extend(step(30, 3, &o, &o2));
    let mut cx = ctx(&auto, Op::Merge);
    cx.moves = [&mo2, &mt];
    let m = merge(&b, &o2, &t, &cx, &mut Fresh::default());
    let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
    assert_eq!(
        (parent(1), parent(2), parent(3)),
        (Some(Nid(3)), Some(Nid(4)), Some(Nid(2)))
    );
    assert!(m.violations.is_empty(), "{:?}", m.violations);
    assert!(
        [1, 2, 3].iter().all(|n| m.rows[&hc(*n)] == "MR-040"),
        "{:?}",
        m.rows
    );
}

/// A move that changes only its node's order is never undone ([RULES/merge-table] RS-007, MR-039, CS-013; [F12 §7.4]
/// row "Kleppmann steps"; [RULES/merge-table] open point 35 (vi), decided by [AR §11] OQ-A-11 11.2 (a)). The base has #1
/// under #3. Ours' commit (10) puts #3 under #2; theirs' commit (20), in one transaction, gives #1 a new order under #3
/// and puts #2 under #1. Replayed after ours', theirs' step closes the cycle #2 → #1 → #3 → #2. #1 has the least uid on
/// the cycle, but its move keeps its parent and closes no cycle, so only #2's move is undone: `#2.parent` stages, the
/// sides' real disagreement, and #1 takes theirs' order. RS-007 before the decision undid #1's reorder first, which left
/// the cycle in place, and staged both keys, `#1.parent` although both sides keep #1 under #3.
#[test]
fn a_cycle_closing_step_never_undoes_an_order_only_move() {
    let mut b = base();
    b.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(3));
    let mut o = b.clone();
    o.nodes.get_mut(&Nid(3)).unwrap().parent = Some(Nid(2));
    let mut t = b.clone();
    t.nodes.get_mut(&Nid(1)).unwrap().order = Some("V".into());
    t.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(1));
    let auto = BTreeMap::new();
    let (mo, mt) = (step(10, 1, &b, &o), step(20, 2, &b, &t));
    assert_eq!(
        mt[0].moves.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        vec![Nid(1), Nid(2)],
        "theirs' step holds #1's reorder and #2's move"
    );
    assert!(uid(Nid(1)) < uid(Nid(2)), "#1 has the lower uid");
    let mut cx = ctx(&auto, Op::Merge);
    cx.moves = [&mo, &mt];
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    let node = |n: u32| &m.st.nodes[&Nid(n)];
    assert_eq!(
        (node(1).parent, node(2).parent, node(3).parent),
        (Some(Nid(3)), None, Some(Nid(2)))
    );
    assert_eq!(node(1).order.as_deref(), Some("V"), "#1's reorder stands");
    let hc = |n: u32| Key::Node(Nid(n), Aspect::Hierarchy);
    assert_eq!(
        m.violations
            .iter()
            .map(|v| (v.class, v.key.clone()))
            .collect::<Vec<_>>(),
        vec![("HierarchyCycle", Some(hc(2)))]
    );
    assert_eq!(
        (
            m.rows[&hc(1)].as_str(),
            m.rows[&hc(2)].as_str(),
            m.rows[&hc(3)].as_str()
        ),
        ("MR-040", "MR-039", "MR-040")
    );
}

/// A revert or a cherry-pick replays from o's (parent, order), dst has no step and neither side has a (0, 0) step
/// ([RULES/merge-table] RS-007; [RULES/merge-table] open point 35 case (ii), decided by [AR §11] OQ-A-6 (a)). The base
/// b (state(p₁(C)) for the pick, state(C) for the revert) has #1 under #2; dst moved #2 under #1 and then #1 under #3;
/// C changes #4's priority only. Replayed from b, dst's first move closed a cycle and staged `#2.parent`, although C
/// moved nothing; replayed from o, the pick and the revert land o's hierarchy with C's priority, whatever dst steps the
/// caller passes. Src's step then applies on o's state: a move of C that closes a cycle there is undone and stages its
/// own key, and one that does not lands.
#[test]
fn a_revert_or_cherry_pick_replays_from_dsts_state() {
    let mv = |st: &State, c: u32, p: Option<u32>| {
        let mut s = st.clone();
        s.nodes.get_mut(&Nid(c)).unwrap().parent = p.map(Nid);
        s
    };
    let b = mv(&base(), 1, Some(2));
    let o1 = mv(&mv(&b, 1, None), 2, Some(1));
    let o = mv(&o1, 1, Some(3));
    let mut t = b.clone();
    apply(&mut t, &[Edit::Priority(4, 0)]);
    let auto = BTreeMap::new();
    let mut mo = step(10, 1, &b, &o1);
    mo.extend(step(20, 2, &o1, &o));
    for op in [Op::CherryPick, Op::Revert] {
        let mut cx = ctx(&auto, op);
        cx.moves = [&mo, &[]];
        let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
        let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
        assert_eq!(
            (parent(1), parent(2)),
            (Some(Nid(3)), Some(Nid(1))),
            "{op:?}"
        );
        assert!(m.violations.is_empty(), "{op:?}: {:?}", m.violations);
        assert_eq!(
            m.st.nodes[&Nid(4)].fields.get("priority"),
            Some(&Value::Enum("P0".into())),
            "{op:?}"
        );
        // C's move of #3 under #2 closes the cycle #3 → #2 → #1 → #3 on o's state: undone, and `#3.parent` stages.
        let t3 = mv(&t, 3, Some(2));
        let mt = step(5, 9, &b, &t3);
        cx.moves = [&mo, &mt];
        let m = merge(&b, &o, &t3, &cx, &mut Fresh::default());
        assert_eq!(m.st.nodes[&Nid(3)].parent, None, "{op:?}");
        assert_eq!(
            m.violations
                .iter()
                .map(|v| (v.class, v.key.clone()))
                .collect::<Vec<_>>(),
            vec![("HierarchyCycle", Some(Key::Node(Nid(3), Aspect::Hierarchy)))],
            "{op:?}"
        );
        // C's move of #4 under #2 lands on o's state.
        let t4 = mv(&t, 4, Some(2));
        let mt = step(5, 9, &b, &t4);
        cx.moves = [&mo, &mt];
        let m = merge(&b, &o, &t4, &cx, &mut Fresh::default());
        let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
        assert_eq!(
            (parent(1), parent(2), parent(4)),
            (Some(Nid(3)), Some(Nid(1)), Some(Nid(2))),
            "{op:?}"
        );
        assert!(m.violations.is_empty(), "{op:?}: {:?}", m.violations);
    }
}

/// Two steps that share a key stay two steps ([RULES/merge-table] RS-007; [F12 §7.4] row "Kleppmann steps": "dst's
/// moves apply first, then src's"): with no commit steps, ours puts #1 under #2 and theirs #2 under #1, each in its
/// side's (0, 0) step. dst's step applies; src's then closes the cycle and is undone, so dst's move stands and src's
/// key is `kleppmann-skipped`, whichever side holds the lower uid. Fused into one step, the least uid would be undone
/// first, which is dst's #1 in the first orientation.
#[test]
fn two_zero_steps_that_close_a_cycle_undo_srcs_move() {
    let b = base();
    let under = |c: u32, p: u32| {
        let mut s = b.clone();
        s.nodes.get_mut(&Nid(c)).unwrap().parent = Some(Nid(p));
        s
    };
    let (one_under_two, two_under_one) = (under(1, 2), under(2, 1));
    let hc = |n: u32| Key::Node(Nid(n), Aspect::Hierarchy);
    for (o, t, kept, skipped) in [
        (&one_under_two, &two_under_one, 1u32, 2u32),
        (&two_under_one, &one_under_two, 2, 1),
    ] {
        let m = run(&b, o, t, Op::Merge);
        let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
        assert_eq!(parent(kept), Some(Nid(skipped)), "dst's move stands");
        assert_eq!(parent(skipped), None, "src's move is undone");
        assert_eq!(
            m.violations
                .iter()
                .map(|v| (v.class, v.key.clone()))
                .collect::<Vec<_>>(),
            vec![("HierarchyCycle", Some(hc(skipped)))]
        );
        assert_eq!(
            (m.rows[&hc(kept)].as_str(), m.rows[&hc(skipped)].as_str()),
            ("MR-040", "MR-039")
        );
        // Inside a virtual merge the same undo is silent (VBC-10).
        let m = run(&b, o, t, Op::Virtual);
        assert!(m.violations.is_empty());
        assert_eq!(m.st.nodes[&Nid(kept)].parent, Some(Nid(skipped)));
        assert_eq!(m.st.nodes[&Nid(skipped)].parent, None);
    }
}

/// The WP-91 review's case for spec finding S1b: the base has #2 under #1; one side restructures over three commits —
/// #2 to the root, #1 under #2, #2 under #3 — each state a forest, and the other side changes only #4's priority. Each
/// commit is one step, so the replay passes through the side's own states: the result is #1 under #2 under #3 with no
/// `HierarchyCycle`, from either side. With each node keyed by its newest commit alone, #1's move (commit 2) met #2
/// still under #1 in the base and was skipped.
#[test]
fn a_restructure_over_three_commits_replays_commit_by_commit() {
    let mut b = base();
    b.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(1));
    let mv = |st: &State, c: u32, p: Option<u32>| {
        let mut s = st.clone();
        s.nodes.get_mut(&Nid(c)).unwrap().parent = p.map(Nid);
        s
    };
    let x1 = mv(&b, 2, None);
    let x2 = mv(&x1, 1, Some(2));
    let x3 = mv(&x2, 2, Some(3));
    let lane = steps(&[&b, &x1, &x2, &x3]);
    assert_eq!(lane.len(), 3);
    let mut other = b.clone();
    apply(&mut other, &[Edit::Priority(4, 1)]);
    let auto = BTreeMap::new();
    for op in OPS {
        for (o, t, moves) in [
            (&x3, &other, [&lane[..], &[]]),
            (&other, &x3, [&[], &lane[..]]),
        ] {
            let mut cx = ctx(&auto, op);
            cx.moves = moves;
            let m = merge(&b, o, t, &cx, &mut Fresh::default());
            let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
            assert_eq!(
                (parent(1), parent(2), parent(3)),
                (Some(Nid(2)), Some(Nid(3)), None),
                "{op:?}"
            );
            assert!(m.violations.is_empty(), "{op:?}: {:?}", m.violations);
            assert_eq!(
                m.st.nodes[&Nid(4)].fields.get("priority"),
                Some(&Value::Enum("P1".into()))
            );
        }
    }
}

/// A node whose move was undone is not skipped when a later move of it applies: its last move decides its value
/// (MR-040; RS-007 as spec finding S1b states it). Ours' commit (10) puts #1 under #2; theirs' commit (20) puts #2 under
/// #1, which closes a cycle and is undone, and its commit (30) puts #2 under #3, which applies.
#[test]
fn a_later_applied_move_supersedes_an_undone_one() {
    let b = base();
    let under = |st: &State, c: u32, p: u32| {
        let mut s = st.clone();
        s.nodes.get_mut(&Nid(c)).unwrap().parent = Some(Nid(p));
        s
    };
    let o = under(&b, 1, 2);
    let t1 = under(&b, 2, 1);
    let t2 = under(&t1, 2, 3);
    let mo = step(10, 1, &b, &o);
    let mut mt = step(20, 2, &b, &t1);
    mt.extend(step(30, 3, &t1, &t2));
    let auto = BTreeMap::new();
    let mut cx = ctx(&auto, Op::Merge);
    cx.moves = [&mo, &mt];
    let m = merge(&b, &o, &t2, &cx, &mut Fresh::default());
    let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
    assert_eq!((parent(1), parent(2)), (Some(Nid(2)), Some(Nid(3))));
    assert!(m.violations.is_empty(), "{:?}", m.violations);
    assert_eq!(m.rows[&Key::Node(Nid(2), Aspect::Hierarchy)], "MR-040");
}

proptest! {
    /// One side passes through several commits from a base forest, each commit's state any forest and its moves one
    /// step as `Dag::move_steps` derives them from its net changeset: the replay passes through the side's own states,
    /// so a merge with an other side that moved nothing takes the side's hierarchy with no `HierarchyCycle`, from
    /// either side — the WP-91 review's minimum guarantee for spec finding S1b.
    #[test]
    fn a_side_of_several_commits_replays_without_a_cycle(
        base_forest in forest(),
        forests in proptest::collection::vec(forest(), 1..6),
        other in proptest::collection::vec(edit(), 0..6),
    ) {
        let b = with_forest(&base(), &base_forest);
        let mut states = vec![b.clone()];
        states.extend(forests.iter().map(|f| with_forest(&b, f)));
        let x = states.last().expect("the last commit's state");
        let lane = steps(&states.iter().collect::<Vec<_>>());
        // The other side edits keys other than the hierarchy: no move and no delete.
        let other: Vec<Edit> = other
            .into_iter()
            .filter(|e| !matches!(e, Edit::Move(..) | Edit::Delete(_)))
            .collect();
        let mut y = b.clone();
        apply(&mut y, &other);
        let auto = BTreeMap::new();
        for op in OPS {
            for (o, t, moves) in [(x, &y, [&lane[..], &[]]), (&y, x, [&[], &lane[..]])] {
                let mut cx = ctx(&auto, op);
                cx.moves = moves;
                let m = merge(&b, o, t, &cx, &mut Fresh::default());
                prop_assert!(
                    !m.violations.iter().any(|v| v.class == "HierarchyCycle"),
                    "{op:?} {:?}",
                    m.violations
                );
                for (n, xn) in x.nodes.iter().filter(|(_, xn)| xn.live()) {
                    if let Some(mn) = m.st.live(*n) {
                        prop_assert_eq!(mn.parent, xn.parent, "{:?} {}", op, n);
                    }
                }
            }
        }
    }

    /// Two sides that each pass through several forests, their commits interleaved in (hlc, id) order: the candidate's
    /// parents are a forest, and in a real merge the `HierarchyCycle` keys are exactly the keys MR-039 decided (V01).
    #[test]
    fn two_sides_of_several_commits_merge_to_a_forest(
        base_forest in forest(),
        fo in proptest::collection::vec(forest(), 0..4),
        ft in proptest::collection::vec(forest(), 0..4),
    ) {
        let b = with_forest(&base(), &base_forest);
        let side = |fs: &[[Option<u32>; 5]]| -> Vec<State> {
            std::iter::once(b.clone()).chain(fs.iter().map(|f| with_forest(&b, f))).collect()
        };
        let (so, st) = (side(&fo), side(&ft));
        // Ours' commits at odd hlcs, theirs' at even ones.
        let steps_at = |states: &[State], first: u64, id: u8| -> Vec<Step> {
            states
                .windows(2)
                .enumerate()
                .filter_map(|(i, w)| {
                    let key = (first + 2 * i as u64, [id + i as u8; 32]);
                    Step::of(key, &crate::state::diff(&w[0], &w[1]))
                })
                .collect()
        };
        let (mo, mt) = (steps_at(&so, 11, 1), steps_at(&st, 12, 101));
        let auto = BTreeMap::new();
        for op in OPS {
            let mut cx = ctx(&auto, op);
            cx.moves = [&mo, &mt];
            let m = merge(&b, so.last().expect("ours"), st.last().expect("theirs"), &cx, &mut Fresh::default());
            for (n, x) in &m.st.nodes {
                if let Some(p) = x.parent {
                    prop_assert!(!above(&m.st, *n, p), "{:?}: {} is its own ancestor", op, n);
                }
            }
            if op == Op::Merge {
                let hc: BTreeSet<Key> = m
                    .violations
                    .iter()
                    .filter(|v| v.class == "HierarchyCycle")
                    .filter_map(|v| v.key.clone())
                    .collect();
                let skipped: BTreeSet<Key> = m
                    .rows
                    .iter()
                    .filter(|(_, r)| r.as_str() == "MR-039")
                    .map(|(k, _)| k.clone())
                    .collect();
                prop_assert_eq!(hc, skipped);
            }
        }
    }
}

/// `DeleteVsModify`: a task (EP-001 `delete-wins`) is provisionally deleted, a note (EP-003 `resurrect`) stays live
/// with the modifying side's keys; the live side carries its node image (MR-042, RS-008); `merge.policy.task =
/// theirs` resolves it with no conflict (AP-003).
#[test]
fn delete_versus_modify_follows_the_existence_policy() {
    let b = base();
    let mut o = b.clone();
    apply(&mut o, &[Edit::Priority(1, 0)]);
    o.nodes.get_mut(&Nid(5)).unwrap().body = Some("edited".into());
    let mut t = b.clone();
    for n in [1u32, 5] {
        let x = t.nodes.get_mut(&Nid(n)).unwrap();
        x.tomb = Some(Tomb {
            reason: Some("gone".into()),
            replaced_by: None,
        });
    }
    let m = run(&b, &o, &t, Op::Merge);
    let task = &m.st.nodes[&Nid(1)];
    assert!(!task.live());
    let c = &task.conflicts[&Aspect::Existence];
    assert_eq!(
        (c.class.as_str(), c.prov),
        ("DeleteVsModify", Some(Side::Theirs))
    );
    assert!(
        c.images[1]
            .as_ref()
            .is_some_and(|i| i.contains_key(&Aspect::Field("priority".into())))
    );
    let note = &m.st.nodes[&Nid(5)];
    assert!(note.live());
    assert_eq!(note.conflicts[&Aspect::Existence].prov, Some(Side::Ours));
    assert_eq!(note.body.as_deref(), Some("edited"));
    let auto: BTreeMap<String, String> = [("task".to_string(), "theirs".to_string())]
        .into_iter()
        .collect();
    let m = merge(&b, &o, &t, &ctx(&auto, Op::Merge), &mut Fresh::default());
    assert!(!m.st.nodes[&Nid(1)].live() && m.st.nodes[&Nid(1)].conflicts.is_empty());
}

#[test]
fn land_or_stage_reads_its_table() {
    assert_eq!(land_or_stage(1, 0, false), "stage");
    assert_eq!(land_or_stage(0, 2, true), "stage");
    assert_eq!(land_or_stage(0, 2, false), "land-conflicted");
    assert_eq!(land_or_stage(0, 0, true), "land");
}
