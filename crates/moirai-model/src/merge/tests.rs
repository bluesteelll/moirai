//! The typed merge on hand-built states: the rows of each class, the rules over conflict-valued bases ([F12 §5.4]),
//! RS-007's hierarchy rule, the existence policies, and properties over random edits of both sides.

use super::*;
use crate::state::{Body, Creator, EdgeProps, Tomb};
use proptest::prelude::*;

fn uid(n: Nid) -> Uid {
    let mut b = [0u8; 16];
    b[12..].copy_from_slice(&n.0.to_be_bytes());
    Uid(b)
}

fn no_nid(_: Uid) -> Option<Nid> {
    None
}

/// A context with no history: every origin RS-007 reads is (0, 0), so a two-sided hierarchy key takes theirs' value
/// (MR-040), unless a test sets [`Ctx::origin`].
fn ctx<'a>(auto: &'a BTreeMap<String, String>, op: Op) -> Ctx<'a> {
    Ctx {
        op,
        dst_main: false,
        dst_plan: false,
        policy: None,
        auto,
        uid: &uid,
        nid: &no_nid,
        origin: None,
        strict: false,
        dropped: &NO_DROPPED,
    }
}

fn run(b: &State, o: &State, t: &State, op: Op) -> Merged {
    let auto = BTreeMap::new();
    merge(b, o, t, &ctx(&auto, op), &mut Fresh::default())
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
            Edit::Body(_, b) => x.body = Some(Body::Text(format!("line a\nline {b}\nline c\n"))),
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
    /// hierarchy moves included (every hierarchy key is one-sided, RS-007) and with no violation of a real merge; two
    /// equal sides give that state (`same`).
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
        x.body = Some(Body::Text("a\nb\nc\n".into()));
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
        x.body = Some(Body::Text(body.into()));
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
    assert_eq!(x.body.as_ref().map(Body::text), Some("A\nb\nC\n"));
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

/// RS-007's cycle repair ([RULES/merge-table] RS-007, MR-039, CS-013): ours puts #1 under #2 (origin 10), theirs #2
/// under #1 (origin 20). Both keys are one-sided and together close a cycle, so the repair resets the one whose value
/// has the later origin, theirs' #2, which stages as `HierarchyCycle` (MR-039, V01), silently inside a virtual merge
/// (VBC-10).
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
    let og = |side: usize, _: Nid, _: bool, _: &Option<KVal>| {
        if side == 1 {
            (10, [1; 32])
        } else {
            (20, [2; 32])
        }
    };
    let mut cx = ctx(&auto, Op::Merge);
    cx.origin = Some(&og);
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert_eq!(m.st.nodes[&Nid(1)].parent, Some(Nid(2)));
    assert_eq!(m.st.nodes[&Nid(2)].parent, None, "the later move is reset");
    assert_eq!(m.violations.len(), 1);
    assert_eq!(m.violations[0].class, "HierarchyCycle");
    assert_eq!(
        m.violations[0].key,
        Some(Key::Node(Nid(2), Aspect::Hierarchy))
    );
    cx.op = Op::Virtual;
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert!(m.violations.is_empty());
    assert_eq!(m.st.nodes[&Nid(2)].parent, None);
}

/// RS-007's step (1) resets a one-sided key before a key both sides set alike ([RULES/merge-table] RS-007, MR-039,
/// MR-040, CS-013). Ours puts #3 under #2 and #1 under #3; theirs puts #1 under #3 too and #2 under #1. #1 is
/// both-same, #2 one-sided (theirs) and #3 one-sided (ours), and they close the cycle #2 → #1 → #3 → #2. Ours' values
/// have the origin 10, theirs' #2 20 and #1 30, so the both-same #1 has the latest origin (30), and only the tie-break
/// "a one-sided key before a both-same one" resets #2 (20, later than #3's 10): `#2.parent` stages, the sides' real
/// disagreement, and `#1.parent` is clean (MR-040). When ours then puts #2 under #4 at a still later origin (40), #2 is
/// two-sided and takes ours' later value, and the merge lands with no hint.
#[test]
fn a_cycle_repair_resets_a_one_sided_key_before_a_both_same_one() {
    let b = base();
    let mut o = b.clone();
    o.nodes.get_mut(&Nid(3)).unwrap().parent = Some(Nid(2));
    o.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(3));
    let mut t = b.clone();
    t.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(3));
    t.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(1));
    let auto = BTreeMap::new();
    let og = |side: usize, n: Nid, _: bool, _: &Option<KVal>| match (side, n.0) {
        (1, 2) => (40, [4; 32]),
        (1, _) => (10, [1; 32]),
        (_, 1) => (30, [3; 32]),
        _ => (20, [2; 32]),
    };
    let mut cx = ctx(&auto, Op::Merge);
    cx.origin = Some(&og);
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
    // Inside a virtual merge the same reset is silent (VBC-10).
    cx.op = Op::Virtual;
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert!(m.violations.is_empty());
    assert_eq!(m.st.nodes[&Nid(2)].parent, None);
    // Ours' later move of #2 (origin 40) wins the two-sided key: nothing stages.
    let mut o2 = o.clone();
    o2.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(4));
    let mut cx = ctx(&auto, Op::Merge);
    cx.origin = Some(&og);
    let m = merge(&b, &o2, &t, &cx, &mut Fresh::default());
    let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
    assert_eq!(
        (parent(1), parent(2), parent(3)),
        (Some(Nid(3)), Some(Nid(4)), Some(Nid(2)))
    );
    assert!(
        m.violations.is_empty() && m.hints.is_empty(),
        "{:?} {:?}",
        m.violations,
        m.hints
    );
    assert!(
        [1, 2, 3].iter().all(|n| m.rows[&hc(*n)] == "MR-040"),
        "{:?}",
        m.rows
    );
}

/// RS-007's cycle repair never resets an order-only key ([RULES/merge-table] RS-007, MR-039, CS-013; [AR §11] OQ-A-11
/// 11.2, OQ-A-13). The base has #1 under #3. Ours puts #3 under #2 (origin 10); theirs gives #1 a new order under #3
/// and puts #2 under #1 (origin 20). The result closes the cycle #2 → #1 → #3 → #2. #1 has the least uid on the cycle,
/// but its value keeps its parent and closes no cycle, so it never takes part in the repair: #2, the later of the two
/// one-sided keys that change a parent, is reset. `#2.parent` stages, the sides' real disagreement, and #1 takes
/// theirs' order. (`merge::threeway::tests::an_order_only_key_is_never_reset` pins the same rule on a cycle no other
/// key can break.)
#[test]
fn a_cycle_repair_never_resets_an_order_only_key() {
    let mut b = base();
    b.nodes.get_mut(&Nid(1)).unwrap().parent = Some(Nid(3));
    let mut o = b.clone();
    o.nodes.get_mut(&Nid(3)).unwrap().parent = Some(Nid(2));
    let mut t = b.clone();
    t.nodes.get_mut(&Nid(1)).unwrap().order = Some("V".into());
    t.nodes.get_mut(&Nid(2)).unwrap().parent = Some(Nid(1));
    assert!(uid(Nid(1)) < uid(Nid(2)), "#1 has the lower uid");
    let auto = BTreeMap::new();
    let og = |side: usize, _: Nid, _: bool, _: &Option<KVal>| {
        if side == 1 {
            (10, [1; 32])
        } else {
            (20, [2; 32])
        }
    };
    let mut cx = ctx(&auto, Op::Merge);
    cx.origin = Some(&og);
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

/// A revert or a cherry-pick keeps dst's hierarchy where C moved nothing, and its cycle repair resets C's key first
/// ([RULES/merge-table] RS-007, DM-003 to DM-005; [AR §11] OQ-A-13). The base b (state(p₁(C)) for the pick, state(C)
/// for the revert) has #1 under #2; dst moved #2 under #1 and then #1 under #3 (origin 20); C changes #4's priority
/// only (origin 5). The pick and the revert land dst's hierarchy with C's priority. A move of C's that closes a cycle
/// on dst's hierarchy is reset and stages its own key, although dst's keys have the later origin: for a revert or a
/// cherry-pick the tie-breaks reset a key only C changed first. A move of C's that closes no cycle lands.
#[test]
fn a_revert_or_cherry_pick_keeps_dsts_hierarchy_and_resets_cs_key_first() {
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
    let og = |side: usize, _: Nid, _: bool, _: &Option<KVal>| {
        if side == 1 {
            (20, [2; 32])
        } else {
            (5, [9; 32])
        }
    };
    for op in [Op::CherryPick, Op::Revert] {
        let mut cx = ctx(&auto, op);
        cx.origin = Some(&og);
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
        // C's move of #3 under #2 closes the cycle #3 → #2 → #1 → #3 on dst's hierarchy: reset, and `#3.parent`
        // stages.
        let t3 = mv(&t, 3, Some(2));
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
        // C's move of #4 under #2 lands on dst's hierarchy.
        let t4 = mv(&t, 4, Some(2));
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

/// A dead-parent hierarchy key whose other value would close a cycle keeps its three-way value and stages, also
/// outside strict ([RULES/merge-table] RS-007, VA-004, HT-004; [RULES/delete-policy-matrix] XB-004). Ours puts #1 under
/// #2 and deletes #3; theirs puts #2 under #1 and #1 under #3. #1 is two-sided and takes theirs' value, #3, on equal
/// origins (MR-040). The result deletes #3, and ours' value, #2, has a live parent, so outside strict #1 tries ours'
/// value first. Ours' value closes #1 ↔ #2 with theirs' one-sided #2, so #1 keeps its value under #3, with no hint and
/// no reset, and V04 stages `#1.parent` as `DanglingEdge`. Under strict, where the preference is off, the outcome is
/// the same. Without theirs' move of #2, ours' value lands with a `dead-parent` hint and nothing stages.
#[test]
fn a_dead_parent_key_whose_other_value_closes_a_cycle_stages_outside_strict() {
    let b = base();
    let mut o = b.clone();
    apply(&mut o, &[Edit::Move(1, Some(2)), Edit::Delete(3)]);
    let mut t = b.clone();
    apply(&mut t, &[Edit::Move(2, Some(1)), Edit::Move(1, Some(3))]);
    let hk = Key::Node(Nid(1), Aspect::Hierarchy);
    let auto = BTreeMap::new();
    for strict in [false, true] {
        let mut cx = ctx(&auto, Op::Merge);
        cx.strict = strict;
        let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
        let parent = |n: u32| m.st.nodes[&Nid(n)].parent;
        assert_eq!(
            (parent(1), parent(2)),
            (Some(Nid(3)), Some(Nid(1))),
            "strict {strict}"
        );
        assert!(
            !m.st.nodes[&Nid(3)].live(),
            "strict {strict}: the result deletes #3"
        );
        assert_eq!(
            m.violations
                .iter()
                .map(|v| (v.class, v.key.clone()))
                .collect::<Vec<_>>(),
            vec![("DanglingEdge", Some(hk.clone()))],
            "strict {strict}"
        );
        assert!(m.hints.is_empty(), "strict {strict}: {:?}", m.hints);
        assert_eq!(m.rows[&hk], "MR-040", "strict {strict}");
    }
    // Without theirs' move of #2, ours' value closes no cycle: it lands with a `dead-parent` hint.
    let mut t1 = b.clone();
    apply(&mut t1, &[Edit::Move(1, Some(3))]);
    let m = run(&b, &o, &t1, Op::Merge);
    assert_eq!(m.st.nodes[&Nid(1)].parent, Some(Nid(2)));
    assert!(m.violations.is_empty(), "{:?}", m.violations);
    assert_eq!(
        m.hints,
        vec![Hint {
            class: "MoveSkipped",
            text: "#1.parent took ours; theirs skipped (dead-parent)".into(),
        }]
    );
}

/// A two-sided hierarchy key both of whose values are under deleted parents keeps its three-way value and stages, also
/// outside strict ([RULES/merge-table] RS-007, VA-004, open point 15; [RULES/delete-policy-matrix] XB-004). Ours puts
/// #1 under #3 and deletes #4; theirs puts #1 under #4 and deletes #3. The result deletes #3 and #4. #1 takes theirs'
/// value, #4, on equal origins (MR-040). Ours' value, #3, has no live parent either, so the dead-parent preference does
/// not apply: #1 stays under #4, with no hint, and V04 stages `#1.parent` as `DanglingEdge`, under both values of
/// strict. When theirs keeps #3, ours' value lands with a `dead-parent` hint and nothing stages.
#[test]
fn a_two_sided_key_with_both_values_under_deleted_parents_stages_outside_strict() {
    let b = base();
    let mut o = b.clone();
    apply(&mut o, &[Edit::Move(1, Some(3)), Edit::Delete(4)]);
    let mut t = b.clone();
    apply(&mut t, &[Edit::Move(1, Some(4)), Edit::Delete(3)]);
    let hk = Key::Node(Nid(1), Aspect::Hierarchy);
    let auto = BTreeMap::new();
    for strict in [false, true] {
        let mut cx = ctx(&auto, Op::Merge);
        cx.strict = strict;
        let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
        assert_eq!(m.st.nodes[&Nid(1)].parent, Some(Nid(4)), "strict {strict}");
        assert!(
            !m.st.nodes[&Nid(3)].live() && !m.st.nodes[&Nid(4)].live(),
            "strict {strict}: the result deletes #3 and #4"
        );
        assert_eq!(
            m.violations
                .iter()
                .map(|v| (v.class, v.key.clone()))
                .collect::<Vec<_>>(),
            vec![("DanglingEdge", Some(hk.clone()))],
            "strict {strict}"
        );
        assert!(m.hints.is_empty(), "strict {strict}: {:?}", m.hints);
        assert_eq!(m.rows[&hk], "MR-040", "strict {strict}");
    }
    // When theirs keeps #3, ours' value has a live parent: #1 tries it first and lands there with a `dead-parent` hint.
    let mut t1 = b.clone();
    apply(&mut t1, &[Edit::Move(1, Some(4))]);
    let m = run(&b, &o, &t1, Op::Merge);
    assert_eq!(m.st.nodes[&Nid(1)].parent, Some(Nid(3)));
    assert!(m.violations.is_empty(), "{:?}", m.violations);
    assert_eq!(
        m.hints,
        vec![Hint {
            class: "MoveSkipped",
            text: "#1.parent took ours; theirs skipped (dead-parent)".into(),
        }]
    );
}

proptest! {
    /// Two sides that each pass through several forests, merged at their last forests with no history: the
    /// candidate's parents are a forest, and in a real merge the `HierarchyCycle` keys are exactly the keys MR-039
    /// decided (V01).
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
        let auto = BTreeMap::new();
        for op in OPS {
            let cx = ctx(&auto, op);
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
    o.nodes.get_mut(&Nid(5)).unwrap().body = Some(Body::Text("edited".into()));
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
    assert_eq!(note.body.as_ref().map(Body::text), Some("edited"));
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

/// A kept case of [F12 §6.3] "The node's other keys" (RS-008; PR-007): the existence policy's `DeleteVsModify` over two
/// plain sides (MR-042) fixes the node to RS-008's provisional state. Under `--policy resurrect` the modifying side is
/// provisional and the node is live "with that side's keys": b's #1 holds a priority, o retitles #1 and leaves the
/// priority, t deletes #1. The priority stays, where a key-by-key merge would take the tombstone's `absent`.
#[test]
fn a_policy_delete_versus_modify_under_resurrect_keeps_an_untouched_key() {
    let mut b = base();
    apply(&mut b, &[Edit::Priority(1, 1)]);
    let mut o = b.clone();
    apply(&mut o, &[Edit::Title(1, 1)]);
    let mut t = b.clone();
    apply(&mut t, &[Edit::Delete(1)]);
    let auto = BTreeMap::new();
    let mut cx = ctx(&auto, Op::Merge);
    cx.policy = Some("resurrect");
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert_eq!(
        m.rows
            .get(&Key::Node(Nid(1), Aspect::Existence))
            .map(String::as_str),
        Some("MR-042")
    );
    let x = &m.st.nodes[&Nid(1)];
    let c = &x.conflicts[&Aspect::Existence];
    assert_eq!(
        (c.class.as_str(), c.prov, x.live()),
        ("DeleteVsModify", Some(Side::Ours), true)
    );
    assert_eq!(x.text("title"), Some("title 1"));
    assert_eq!(
        x.fields.get("priority"),
        Some(&Value::Enum("P1".into())),
        "RS-008: live with the modifying side's keys"
    );
}

/// The other kept case of [F12 §6.3] "The node's other keys" (RS-015; RS-008): a new `DeleteVsModify` whose flat sides
/// are one live and one deleted takes RS-008's provisional state. b's #1 holds a priority; o holds #1 with its own
/// `DeleteVsModify` value provisional on the live side, retitled, its priority untouched; t deletes #1. MR-005 makes a
/// new `DeleteVsModify` over a live and a deleted flat side, provisional on ours under `--policy resurrect`, so #1 is
/// live with o's keys and keeps the priority a key-by-key merge would drop.
#[test]
fn a_new_one_live_delete_versus_modify_under_resurrect_keeps_rs008s_state() {
    let s = Schema::default();
    let mut b = base();
    apply(&mut b, &[Edit::Priority(1, 1)]);
    let mut o = b.clone();
    apply(&mut o, &[Edit::Title(1, 2)]);
    let gone = KVal::Deleted {
        kind: "task".into(),
        reason: Some("gone".into()),
        replaced_by: None,
    };
    let image = |st: &State| canon::node_image(&s, &st.nodes[&Nid(1)]);
    let held = Conflict {
        class: "DeleteVsModify".into(),
        base: Some(KVal::Live("task".into())),
        ours: Some(KVal::Live("task".into())),
        theirs: Some(gone),
        prov: Some(Side::Ours),
        images: [Some(image(&b)), Some(image(&o)), None],
    };
    o.nodes
        .get_mut(&Nid(1))
        .unwrap()
        .conflicts
        .insert(Aspect::Existence, held);
    let mut t = b.clone();
    apply(&mut t, &[Edit::Delete(1)]);
    let auto = BTreeMap::new();
    let mut cx = ctx(&auto, Op::Merge);
    cx.policy = Some("resurrect");
    let m = merge(&b, &o, &t, &cx, &mut Fresh::default());
    assert_eq!(
        m.rows
            .get(&Key::Node(Nid(1), Aspect::Existence))
            .map(String::as_str),
        Some("MR-005")
    );
    let x = &m.st.nodes[&Nid(1)];
    let c = &x.conflicts[&Aspect::Existence];
    assert_eq!(
        (c.class.as_str(), c.prov, x.live()),
        ("DeleteVsModify", Some(Side::Ours), true)
    );
    assert!(
        matches!(c.ours, Some(KVal::Live(_))) && matches!(c.theirs, Some(KVal::Deleted { .. }))
    );
    assert_eq!(x.text("title"), Some("title 2"));
    assert_eq!(
        x.fields.get("priority"),
        Some(&Value::Enum("P1".into())),
        "RS-008: live with the modifying side's keys"
    );
}

/// The tombstone value of a task deleted for the reason `gone`.
fn gone() -> KVal {
    KVal::Deleted {
        kind: "task".into(),
        reason: Some("gone".into()),
        replaced_by: None,
    }
}

/// #1's node image on a state.
fn img(st: &State) -> Image {
    canon::node_image(&Schema::default(), &st.nodes[&Nid(1)])
}

/// A `DeleteVsModify` value of #1 over a live base: ours live and theirs `gone` when `ours_live`, else the reverse.
fn dvm(ours_live: bool, prov: Side, images: [Option<Image>; 3]) -> Conflict {
    let (ours, theirs) = if ours_live {
        (Some(KVal::Live("task".into())), Some(gone()))
    } else {
        (Some(gone()), Some(KVal::Live("task".into())))
    };
    Conflict {
        class: "DeleteVsModify".into(),
        base: Some(KVal::Live("task".into())),
        ours,
        theirs,
        prov: Some(prov),
        images,
    }
}

/// Sets #1's existence key on a state to a conflict value.
fn hold(st: &mut State, c: Conflict) {
    st.nodes
        .get_mut(&Nid(1))
        .unwrap()
        .conflicts
        .insert(Aspect::Existence, c);
}

/// A real merge under `merge.policy.task` = `side`.
fn run_auto(b: &State, o: &State, t: &State, side: &str) -> Merged {
    let auto: BTreeMap<String, String> = [("task".to_string(), side.to_string())]
        .into_iter()
        .collect();
    merge(b, o, t, &ctx(&auto, Op::Merge), &mut Fresh::default())
}

/// The row that decided #1's existence key.
fn existence_row(m: &Merged) -> Option<&str> {
    m.rows
        .get(&Key::Node(Nid(1), Aspect::Existence))
        .map(String::as_str)
}

/// An automatic side's existence value whose flat sides are one live and one deleted ([RULES/merge-table] PR-007,
/// AP-002; RS-007's scope): the node is taken whole from that side, live with its keys, and counts as a node an
/// existence policy fixed. b's #1 holds a priority; o holds #1 with its own `DeleteVsModify` value provisional on the
/// live side, retitled, its priority untouched; t deletes #1. Under `ours` MR-005 takes o's value: #1 is o's node,
/// title and priority included, where a key-by-key merge would take the tombstone's `absent` priority.
#[test]
fn an_automatic_ours_keeps_its_live_conflicted_node_whole_against_a_deleted_side() {
    let mut b = base();
    apply(&mut b, &[Edit::Priority(1, 1)]);
    let mut o = b.clone();
    apply(&mut o, &[Edit::Title(1, 2)]);
    let held = dvm(true, Side::Ours, [Some(img(&b)), Some(img(&o)), None]);
    hold(&mut o, held.clone());
    let mut t = b.clone();
    apply(&mut t, &[Edit::Delete(1)]);
    let m = run_auto(&b, &o, &t, "ours");
    assert_eq!(existence_row(&m), Some("MR-005+AP-002"));
    let x = &m.st.nodes[&Nid(1)];
    assert!(x.live());
    assert_eq!(x.text("title"), Some("title 2"));
    assert_eq!(
        x.fields.get("priority"),
        Some(&Value::Enum("P1".into())),
        "taken whole from the automatic side, live with its keys"
    );
    assert_eq!(x.conflicts.get(&Aspect::Existence), Some(&held));
    assert!(m.conflicts.is_empty());
}

/// As the case above with o's value provisional on `theirs` ([RULES/merge-table] PR-007, AP-002; RS-007's scope): the
/// node is taken whole from the automatic side, not from the side the value's `prov` names, so #1 is o's live node.
#[test]
fn an_automatic_ours_keeps_its_node_whole_when_its_values_prov_names_theirs() {
    let mut b = base();
    apply(&mut b, &[Edit::Priority(1, 1)]);
    let mut o = b.clone();
    apply(&mut o, &[Edit::Title(1, 2)]);
    let held = dvm(false, Side::Theirs, [Some(img(&b)), None, Some(img(&o))]);
    hold(&mut o, held.clone());
    let mut t = b.clone();
    apply(&mut t, &[Edit::Delete(1)]);
    let m = run_auto(&b, &o, &t, "ours");
    assert_eq!(existence_row(&m), Some("MR-005+AP-002"));
    let x = &m.st.nodes[&Nid(1)];
    assert!(x.live());
    assert_eq!(x.text("title"), Some("title 2"));
    assert_eq!(x.fields.get("priority"), Some(&Value::Enum("P1".into())));
    assert_eq!(x.conflicts.get(&Aspect::Existence), Some(&held));
}

/// An automatic `theirs` over flat sides one live and one deleted ([RULES/merge-table] PR-007, AP-003; RS-007's
/// scope): o holds #1 retitled with a `DeleteVsModify` value provisional on the live side, t deletes #1. MR-005 takes
/// t's plain tombstone, and the node is t's tombstone whole: its title is t's `t1`, where a tombstone merge would take
/// o's retitle.
#[test]
fn an_automatic_theirs_keeps_its_tombstone_whole_against_a_live_conflicted_side() {
    let mut b = base();
    apply(&mut b, &[Edit::Priority(1, 1)]);
    let mut o = b.clone();
    apply(&mut o, &[Edit::Title(1, 2)]);
    let held = dvm(true, Side::Ours, [Some(img(&b)), Some(img(&o)), None]);
    hold(&mut o, held);
    let mut t = b.clone();
    apply(&mut t, &[Edit::Delete(1)]);
    let m = run_auto(&b, &o, &t, "theirs");
    assert_eq!(existence_row(&m), Some("MR-005+AP-003"));
    let x = &m.st.nodes[&Nid(1)];
    assert!(!x.live());
    assert_eq!(x.text("title"), Some("t1"), "t's tombstone whole");
    assert!(!x.conflicts.contains_key(&Aspect::Existence));
    assert!(m.conflicts.is_empty());
}

/// An automatic `ours` whose side holds a `DeleteVsModify` value provisionally deleted, against a live side
/// ([RULES/merge-table] PR-007, AP-002; RS-007's scope): o deleted #1 and holds such a value, its title left at b's;
/// t retitled #1 and holds its own value provisional on the live side, so its existence changed since b too. The node
/// is o's tombstone whole, with o's value: its title stays `t1`, where a tombstone merge would take t's retitle.
#[test]
fn an_automatic_ours_keeps_its_deleted_conflicted_node_whole_against_a_live_side() {
    let b = base();
    let mut o = b.clone();
    apply(&mut o, &[Edit::Delete(1)]);
    let held = dvm(true, Side::Theirs, [Some(img(&b)), Some(img(&b)), None]);
    hold(&mut o, held.clone());
    let mut t = b.clone();
    apply(&mut t, &[Edit::Title(1, 3)]);
    let other = dvm(true, Side::Ours, [Some(img(&b)), Some(img(&t)), None]);
    hold(&mut t, other);
    let m = run_auto(&b, &o, &t, "ours");
    assert_eq!(existence_row(&m), Some("MR-005+AP-002"));
    let x = &m.st.nodes[&Nid(1)];
    assert!(!x.live());
    assert_eq!(x.text("title"), Some("t1"), "o's tombstone whole");
    assert_eq!(x.conflicts.get(&Aspect::Existence), Some(&held));
}

/// An automatic `ours` whose side resolved a conflicted base live, against a side whose value is provisionally deleted
/// ([RULES/merge-table] MR-002, PR-007, AP-002; RS-007's scope): b's #1 holds a priority and a `DeleteVsModify` value
/// provisional on the live side; o resolved it, plain live; t deleted #1 and holds the value provisional on the deleted
/// side. The node is o's whole: it keeps b's priority, where a key-by-key merge would take the tombstone's `absent`.
#[test]
fn an_automatic_ours_keeps_its_plain_live_node_whole_against_a_deleted_conflicted_side() {
    let mut b = base();
    apply(&mut b, &[Edit::Priority(1, 1)]);
    let base_value = dvm(true, Side::Ours, [Some(img(&b)), Some(img(&b)), None]);
    let o = b.clone();
    hold(&mut b, base_value.clone());
    let mut t = b.clone();
    apply(&mut t, &[Edit::Delete(1)]);
    hold(
        &mut t,
        Conflict {
            prov: Some(Side::Theirs),
            ..base_value
        },
    );
    let m = run_auto(&b, &o, &t, "ours");
    assert_eq!(existence_row(&m), Some("MR-002+AP-002"));
    let x = &m.st.nodes[&Nid(1)];
    assert!(x.live());
    assert_eq!(
        x.fields.get("priority"),
        Some(&Value::Enum("P1".into())),
        "taken whole from the automatic side, live with its keys"
    );
    assert!(!x.conflicts.contains_key(&Aspect::Existence));
}

/// MR-042 under an automatic side ([RULES/merge-table] MR-042, PR-007, AP-002, AP-003): the value that side takes fixes
/// the node, which takes that side's state whole. b's #1 holds a priority; one side retitles #1, the other deletes it.
/// With the automatic side on the retitling side, #1 is that side's node and keeps the priority, where a key-by-key
/// merge would take the tombstone's `absent`.
#[test]
fn an_automatic_side_keeps_the_modified_node_whole_over_a_delete_versus_modify() {
    let mut b = base();
    apply(&mut b, &[Edit::Priority(1, 1)]);
    let mut edited = b.clone();
    apply(&mut edited, &[Edit::Title(1, 1)]);
    let mut deleted = b.clone();
    apply(&mut deleted, &[Edit::Delete(1)]);
    for (o, t, side, row) in [
        (&edited, &deleted, "ours", "MR-042+AP-002"),
        (&deleted, &edited, "theirs", "MR-042+AP-003"),
    ] {
        let m = run_auto(&b, o, t, side);
        assert_eq!(existence_row(&m), Some(row));
        let x = &m.st.nodes[&Nid(1)];
        assert!(x.live(), "{side}");
        assert_eq!(x.text("title"), Some("title 1"), "{side}");
        assert_eq!(
            x.fields.get("priority"),
            Some(&Value::Enum("P1".into())),
            "{side}: the side's state whole"
        );
        assert!(!x.conflicts.contains_key(&Aspect::Existence), "{side}");
        assert!(m.conflicts.is_empty(), "{side}");
    }
}

/// b, o and t of a dropped-body case: #1, a task, holds a `text` body key and #6, a `doc` section, a `section-text`
/// one ([RULES/merge-table] MR-032, MR-038). Each side's body of both nodes is `side(i, base)`.
fn dropped_body_states(side: impl Fn(usize, &str) -> Body) -> [State; 3] {
    let s = Schema::default();
    let mut b = base();
    let mut doc = Node::new(uid(Nid(6)), "doc", &s, Creator::default());
    doc.set_field(&s, "title", Some(Value::Text("t6".into())));
    doc.fields
        .insert("doc_kind".into(), Value::Enum("section".into()));
    b.nodes.insert(Nid(6), doc);
    [0, 1, 2].map(|i| {
        let mut st = b.clone();
        for n in [1, 6] {
            st.nodes.get_mut(&Nid(n)).unwrap().body = Some(side(i, "a\nb\nc\n"));
        }
        st
    })
}

/// The merge of b, o and t with `dropped` as the store's dropped set.
fn merge_dropping(st: &[State; 3], dropped: &BTreeSet<[u8; 16]>) -> Merged {
    let auto = BTreeMap::new();
    let mut cx = ctx(&auto, Op::Merge);
    cx.dropped = dropped;
    merge(&st[0], &st[1], &st[2], &cx, &mut Fresh::default())
}

/// The body key of `n` holds a `TextHunk` (MR-032 for #1's `text`, MR-038 for #6's `section-text`) whose base, ours
/// and theirs are canonically the three bodies, compared by their hashes ([F07 §7.3]).
fn assert_text_hunk(m: &Merged, n: u32, want: [[u8; 16]; 3]) {
    let row = if n == 1 { "MR-032" } else { "MR-038" };
    let key = Key::Node(Nid(n), Aspect::Body);
    assert_eq!(m.rows.get(&key).map(String::as_str), Some(row), "#{n}");
    let c = &m.st.nodes[&Nid(n)].conflicts[&Aspect::Body];
    let hash = |v: &Option<KVal>| match v {
        Some(KVal::Body(x)) => canon::body_hash(x),
        other => panic!("#{n}: not a body: {other:?}"),
    };
    assert_eq!(c.class, "TextHunk", "#{n}");
    assert_eq!(
        [hash(&c.base), hash(&c.ours), hash(&c.theirs)],
        want,
        "#{n}"
    );
}

/// S3-M-9 ([RULES/merge-table] CS-011, PR-016; [F12 §7.5] "A dropped body"): a two-sided body key one of whose sides
/// is a body known only by a hash in the store's dropped set runs no diff3, which would read its text, and takes the
/// class's `both` row: a `TextHunk` whose sides are the three bodies' hashes, under `text` (MR-032) and `section-text`
/// (MR-038). With an empty dropped set the same bodies merge by diff3.
#[test]
fn a_body_key_with_a_dropped_hash_side_takes_a_text_hunk_of_the_three_hashes() {
    let ours = "A\nb\nc\n";
    let h = canon::body_hash(&Body::Text(ours.into()));
    let st = dropped_body_states(|i, b| match i {
        0 => Body::Text(b.into()),
        1 => Body::Hash(h),
        _ => Body::Text("a\nb\nC\n".into()),
    });
    let m = merge_dropping(&st, &BTreeSet::from([h]));
    let want = [
        canon::body_hash(&Body::Text("a\nb\nc\n".into())),
        h,
        canon::body_hash(&Body::Text("a\nb\nC\n".into())),
    ];
    for n in [1, 6] {
        assert_text_hunk(&m, n, want);
    }
}

/// S3-M-9's membership by hash ([RULES/merge-table] CS-011: "a hash in the store's dropped set"; [F07 §7.3]): a side
/// that holds the dropped body as its text counts as a hash-only side does, so edits diff3 would merge cleanly still
/// take a `TextHunk` of the three hashes. The rows that compare hashes and read no dropped set (`same`, `ours-only`,
/// `theirs-only`) give the same result with the body dropped as with an empty dropped set.
#[test]
fn a_dropped_body_held_as_text_counts_by_its_hash() {
    let (b, ours, theirs) = ("a\nb\nc\n", "A\nb\nc\n", "a\nb\nC\n");
    let hash = |x: &str| canon::body_hash(&Body::Text(x.into()));
    let text = |x: &str| Body::Text(x.into());
    let st = dropped_body_states(|i, _| text([b, ours, theirs][i]));
    let clean = merge_dropping(&st, &NO_DROPPED);
    assert_eq!(
        clean.st.nodes[&Nid(1)].body,
        Some(text("A\nb\nC\n")),
        "an empty dropped set: diff3 (MR-031)"
    );
    let m = merge_dropping(&st, &BTreeSet::from([hash(ours)]));
    for n in [1, 6] {
        assert_text_hunk(&m, n, [hash(b), hash(ours), hash(theirs)]);
    }
    // `same`, `ours-only` and `theirs-only` of the dropped body.
    for sides in [[b, ours, ours], [b, ours, b], [b, b, ours]] {
        let st = dropped_body_states(|i, _| text(sides[i]));
        let dropped = merge_dropping(&st, &BTreeSet::from([hash(ours)]));
        let kept = merge_dropping(&st, &NO_DROPPED);
        assert_eq!(cs(&dropped.st), cs(&kept.st), "{sides:?}");
        assert_eq!(dropped.rows, kept.rows, "{sides:?}");
        assert_eq!(
            dropped.st.nodes[&Nid(1)].body,
            Some(text(ours)),
            "{sides:?}"
        );
    }
}
