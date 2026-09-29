//! The merge validators of I37′ ([F13 §5] V01–V13; [RULES/merge-table] `validators`), each checked by recomputation
//! over the whole candidate ([60 §4.2]), in their normative order: violations and conflicts are emitted in validator
//! order and, within one validator, in canonical key order ([F13 §5] VO-2). Every validator runs even after an earlier
//! one reported (VO-2). A value-conflict validator (V06 `SupersedeFork`, V08 `PathClaim`) puts its conflict value on
//! the candidate; the others report structural violations, whose keys are [F12 §7.9]'s.

use crate::canon::CKey;
use crate::derived::{Index, descendants, precedence_edges, scc};
use crate::merge::{Hint, Violation, canonical_key, flat, key_text, violation_code};
use crate::schema::{Acyclic, Card, EdgeClass, ItemKey, Props};
use crate::state::{Aspect, Conflict, EdgeKey, KState, KVal, Key, State};
use crate::value::{Nid, Uid, Value};
use std::collections::{BTreeMap, BTreeSet};

/// What the validators found.
#[derive(Default)]
pub struct Found {
    /// The value conflicts they put on the candidate, (key, class), in order.
    pub conflicts: Vec<(Key, String)>,
    /// The structural violations, in order.
    pub violations: Vec<Violation>,
    /// The hints (V13).
    pub hints: Vec<Hint>,
}

struct V<'a> {
    uid: &'a dyn Fn(Nid) -> Uid,
    out: Found,
}

impl V<'_> {
    fn ck(&self, st: &State, k: &Key) -> CKey {
        canonical_key(k, self.uid, &st.schema)
    }

    /// Emits the violations of one validator in canonical key order.
    fn emit(&mut self, st: &State, class: &'static str, mut v: Vec<(Key, String, String)>) {
        v.sort_by_cached_key(|(k, _, _)| self.ck(st, k));
        for (k, description, suggested) in v {
            self.out.violations.push(Violation {
                class,
                code: violation_code(class),
                key: Some(k),
                description,
                suggested,
            });
        }
    }

    fn text(&self, k: &Key) -> String {
        key_text(k, self.uid)
    }
}

/// Runs V01–V13 on the candidate `st` of a merge whose sides are `[b, o, t]` ([F13 §5]).
// rule: VA-001, VA-002, VA-003, VA-004, VA-005, VA-006, VA-007, VA-008, VA-009, VA-010, VA-011, VA-012, VA-013
// rule: VA-014, VA-015, VA-016, PC-001, PC-002, PC-003, PC-004, PC-005
pub fn validate(
    st: &mut State,
    sides: [&State; 3],
    dst_plan: bool,
    skipped: &[Nid],
    uid: &dyn Fn(Nid) -> Uid,
) -> Found {
    let mut v = V {
        uid,
        out: Found::default(),
    };
    let o = sides[1];
    // V01: the moves Kleppmann skipped (MR-039).
    let hc: Vec<(Key, String, String)> = skipped
        .iter()
        .map(|n| {
            let k = Key::Node(*n, Aspect::Hierarchy);
            (
                k.clone(),
                format!("moving {n} would make it its own ancestor; the move was skipped"),
                format!("moirai resolve '{}' --take ours", v.text(&k)),
            )
        })
        .collect();
    v.emit(st, "HierarchyCycle", hc);
    // V02 (implied exogenous edges) is part of V03's graph ([`precedence_edges`]); V03 and VA-016.
    v03_cycles(&mut v, st, o);
    v04_dangling(&mut v, st);
    v05_forest(&mut v, st);
    v06_supersedes(&mut v, st, o);
    v07_cardinality(&mut v, st);
    v08_path_claims(&mut v, st, sides);
    v09_schema(&mut v, st);
    v10_v11_queries(&mut v, st, o, sides);
    if dst_plan {
        v12_plan_mask(&mut v, st, o);
    }
    v13_hints(&mut v, st, o);
    v.out
}

/// The precedence edges of a state with their kinds, as (from, to, kind).
fn prec(st: &State) -> Vec<(Nid, Nid, &'static str)> {
    precedence_edges(&Index::new(st))
}

/// The key a precedence edge stands for: a `blocks` or `gates` edge key; the hierarchy key of a child→parent edge;
/// for an implied edge X→D, the `blocks` edge X→P it is implied through (the least P by uid).
fn edge_key(st: &State, a: Nid, b: Nid, kind: &str, uid: &dyn Fn(Nid) -> Uid) -> Key {
    match kind {
        "parent" => Key::Node(a, Aspect::Hierarchy),
        "implied" => {
            let ix = Index::new(st);
            let p = st.nodes[&a]
                .out
                .keys()
                .filter(|k| k.kind == "blocks" && descendants(&ix, k.dst).contains(&b))
                .map(|k| k.dst)
                .min_by_key(|p| uid(*p))
                .unwrap_or(b);
            Key::Node(
                a,
                Aspect::Edge(EdgeKey {
                    kind: "blocks".into(),
                    dst: p,
                    disc: None,
                }),
            )
        }
        k => Key::Node(
            a,
            Aspect::Edge(EdgeKey {
                kind: k.to_string(),
                dst: b,
                disc: None,
            }),
        ),
    }
}

/// The least edge (by source uid, kind, destination uid) among the edges that lie on a cycle and that `o` does not
/// hold (every cycle of a candidate holds one, since dst's head is acyclic, I12); the least cycle edge otherwise.
fn witness(
    edges: &[(Nid, Nid, String)],
    old: &BTreeSet<(Nid, Nid, String)>,
    uid: &dyn Fn(Nid) -> Uid,
) -> Option<(Nid, Nid, String)> {
    let mut adj: BTreeMap<Nid, Vec<Nid>> = BTreeMap::new();
    for (a, b, _) in edges {
        adj.entry(*a).or_default().push(*b);
        adj.entry(*b).or_default();
    }
    let comp = scc(&adj);
    let on_cycle: Vec<&(Nid, Nid, String)> = edges
        .iter()
        .filter(|(a, b, _)| a == b || comp[a] == comp[b])
        .collect();
    let key = |e: &(Nid, Nid, String)| (uid(e.0), e.2.clone(), uid(e.1));
    on_cycle
        .iter()
        .filter(|e| !old.contains(**e))
        .min_by_key(|e| key(e))
        .or_else(|| on_cycle.iter().min_by_key(|e| key(e)))
        .map(|e| (**e).clone())
}

/// V03: the combined precedence graph (I5′) and every other kind declared acyclic (VA-016), each reported once with its
/// canonical witness ([F13 §5] V03, VO-5).
fn v03_cycles(v: &mut V<'_>, st: &State, o: &State) {
    let uid = v.uid;
    let mut found = Vec::new();
    let to_s = |e: Vec<(Nid, Nid, &'static str)>| -> Vec<(Nid, Nid, String)> {
        e.into_iter()
            .map(|(a, b, k)| (a, b, k.to_string()))
            .collect()
    };
    let cand = to_s(prec(st));
    let old: BTreeSet<(Nid, Nid, String)> = to_s(prec(o)).into_iter().collect();
    if let Some((a, b, k)) = witness(&cand, &old, uid) {
        let key = edge_key(st, a, b, &k, uid);
        found.push((
            key,
            format!("{a} {k} {b} closes a cycle of the precedence graph (I5')"),
            "moirai resolve the edge with --take ours".to_string(),
        ));
    }
    let kinds: Vec<String> = st
        .schema
        .edges()
        .iter()
        .filter(|e| matches!(e.acyclic, Acyclic::Dag | Acyclic::ByConstruction))
        .map(|e| e.name.clone())
        .collect();
    for kind in kinds {
        let collect = |s: &State| -> Vec<(Nid, Nid, String)> {
            s.nodes
                .iter()
                .filter(|(_, x)| x.live())
                .flat_map(|(n, x)| {
                    x.out
                        .keys()
                        .filter(|k| k.kind == kind && s.nodes.contains_key(&k.dst))
                        .map(move |k| (*n, k.dst, k.kind.clone()))
                })
                .collect()
        };
        let cand = collect(st);
        let old: BTreeSet<(Nid, Nid, String)> = collect(o).into_iter().collect();
        if let Some((a, b, k)) = witness(&cand, &old, uid) {
            found.push((
                edge_key(st, a, b, &k, uid),
                format!("{a} {k} {b} closes a cycle of {k} edges"),
                "moirai resolve the edge with --take ours".to_string(),
            ));
        }
    }
    v.emit(st, "Cycle", found);
}

/// V04: a structural edge whose endpoints are not both live, except a flagged `blocks` or `gates` edge of a tombstone
/// (FL-004); a live node's parent that is not live (I2).
fn v04_dangling(v: &mut V<'_>, st: &State) {
    let mut found = Vec::new();
    for (n, x) in &st.nodes {
        for (k, p) in &x.out {
            let structural = st
                .schema
                .edge(&k.kind)
                .is_some_and(|e| e.class == EdgeClass::Structural);
            if !structural {
                continue;
            }
            let src_ok = x.live() || (p.flagged && (k.kind == "blocks" || k.kind == "gates"));
            let dst_ok = st.live(k.dst).is_some();
            if !(src_ok && dst_ok) {
                let key = Key::Node(*n, Aspect::Edge(k.clone()));
                found.push((
                    key,
                    format!(
                        "the {} edge {n} -> {} has a deleted endpoint",
                        k.kind, k.dst
                    ),
                    "apply the edge kind's delete policy, or --replaced-by".to_string(),
                ));
            }
        }
        if x.live()
            && let Some(p) = x.parent
            && st.live(p).is_none()
        {
            found.push((
                Key::Node(*n, Aspect::Hierarchy),
                format!("the parent {p} of {n} is deleted"),
                "move the node, or resolve its parent key".to_string(),
            ));
        }
    }
    v.emit(st, "DanglingEdge", found);
}

/// V05: the `parent` forest (I4): a cycle is `HierarchyCycle` keyed by the least uid on it; a depth above 12 is one
/// `DepthExceeded` keyed by the least uid deeper than the bound ([F12 §7.9]).
fn v05_forest(v: &mut V<'_>, st: &State) {
    let uid = v.uid;
    let mut cycles: BTreeSet<Nid> = BTreeSet::new();
    let mut deep: Vec<Nid> = Vec::new();
    for (n, x) in &st.nodes {
        if !x.live() {
            continue;
        }
        let mut seen = vec![*n];
        let mut cur = x.parent;
        let mut depth = 0;
        while let Some(p) = cur {
            if let Some(i) = seen.iter().position(|s| *s == p) {
                // The cycle is seen[i..]; key it by its least uid.
                if let Some(m) = seen[i..].iter().min_by_key(|m| uid(**m)) {
                    cycles.insert(*m);
                }
                break;
            }
            seen.push(p);
            depth += 1;
            cur = st.live(p).and_then(|y| y.parent);
        }
        if depth > 12 {
            deep.push(*n);
        }
    }
    let hc: Vec<(Key, String, String)> = cycles
        .into_iter()
        .map(|n| {
            (
                Key::Node(n, Aspect::Hierarchy),
                format!("{n} lies on a parent cycle"),
                "moirai resolve the hierarchy key with --take ours".to_string(),
            )
        })
        .collect();
    v.emit(st, "HierarchyCycle", hc);
    if let Some(n) = deep.into_iter().min_by_key(|n| uid(*n)) {
        v.emit(
            st,
            "DepthExceeded",
            vec![(
                Key::Node(n, Aspect::Hierarchy),
                format!("{n} lies deeper than 12 levels"),
                "move the subtree up".to_string(),
            )],
        );
    }
}

/// Whether a node is active: live, with a status that is not a side state.
fn active(st: &State, n: Nid) -> bool {
    st.live(n).is_some_and(|x| {
        st.schema
            .value(&x.kind, "status", &x.status)
            .is_some_and(|e| !e.side)
    })
}

/// V06 (I6): a target with a second active superseder: the conflict value sits on each `supersedes` edge the merge
/// added (the src side's), with base and ours `absent` and theirs the edge ([F12 §6.2]).
fn v06_supersedes(v: &mut V<'_>, st: &mut State, o: &State) {
    let mut by_dst: BTreeMap<Nid, Vec<(Nid, EdgeKey)>> = BTreeMap::new();
    for (n, x) in &st.nodes {
        for k in x.out.keys().filter(|k| k.kind == "supersedes") {
            if active(st, *n) {
                by_dst.entry(k.dst).or_default().push((*n, k.clone()));
            }
        }
    }
    let mut found = Vec::new();
    for srcs in by_dst.values().filter(|s| s.len() > 1) {
        for (s, k) in srcs {
            let a = Aspect::Edge(k.clone());
            let in_o = o.nodes.get(s).is_some_and(|x| x.out.contains_key(k));
            let conflicted = st.nodes[s].conflicts.contains_key(&a);
            if in_o || conflicted {
                continue;
            }
            found.push((*s, a));
        }
    }
    found.sort_by_cached_key(|(s, a)| v.ck(st, &Key::Node(*s, a.clone())));
    for (s, a) in found {
        let Aspect::Edge(k) = &a else { continue };
        let p = st.nodes[&s].out[k].clone();
        let c = Conflict {
            class: "SupersedeFork".into(),
            base: None,
            ours: None,
            theirs: Some(KVal::Edge(p)),
            prov: None,
            images: [None, None, None],
        };
        let schema = st.schema.clone();
        st.nodes.get_mut(&s).expect("the superseder").set_kstate(
            &schema,
            &a,
            KState::Conflict(Box::new(c)),
        );
        v.out
            .conflicts
            .push((Key::Node(s, a), "SupersedeFork".into()));
    }
}

/// V07: `duplicate_of` chains of length 1 (I7), `max-1-per-src` (`runs_in`), `max-1-active-per-dst` (`answers`); each
/// instance keyed by its greatest offending edge key in canonical order ([F12 §7.9]).
fn v07_cardinality(v: &mut V<'_>, st: &State) {
    let mut groups: Vec<Vec<Key>> = Vec::new();
    let mut per_dst: BTreeMap<(String, Nid), Vec<Key>> = BTreeMap::new();
    for (n, x) in &st.nodes {
        if !x.live() {
            continue;
        }
        let mut per_src: BTreeMap<&str, Vec<Key>> = BTreeMap::new();
        for k in x.out.keys() {
            let Some(e) = st.schema.edge(&k.kind) else {
                continue;
            };
            let key = Key::Node(*n, Aspect::Edge(k.clone()));
            match e.card {
                Card::Chain1 => {
                    per_src.entry(&k.kind).or_default().push(key.clone());
                    let target_dup = st
                        .live(k.dst)
                        .is_some_and(|y| y.out.keys().any(|j| j.kind == k.kind));
                    if target_dup {
                        groups.push(vec![key]);
                    }
                }
                Card::Max1PerSrc => per_src.entry(&k.kind).or_default().push(key),
                Card::Max1ActivePerDst if k.kind != "supersedes" && active(st, *n) => {
                    per_dst
                        .entry((k.kind.clone(), k.dst))
                        .or_default()
                        .push(key);
                }
                _ => {}
            }
        }
        groups.extend(per_src.into_values().filter(|g| g.len() > 1));
    }
    groups.extend(per_dst.into_values().filter(|g| g.len() > 1));
    let mut found: Vec<(Key, String, String)> = Vec::new();
    let mut seen = BTreeSet::new();
    for g in groups {
        let Some(k) = g.into_iter().max_by_key(|k| v.ck(st, k)) else {
            continue;
        };
        if seen.insert(k.clone()) {
            let t = v.text(&k);
            found.push((
                k,
                format!("{t} breaks its edge kind's cardinality"),
                format!("moirai resolve '{t}' --take ours"),
            ));
        }
    }
    v.emit(st, "Cardinality", found);
}

/// V08 (I-F1): live file nodes with status `present` or `planned` grouped by (root, exact path bytes); a group of two
/// or more uids puts a `PathClaim` conflict value on each node's observation key, {b, o, t} of that node's composite
/// ([RULES/link-merge-rules] PC-001 to PC-005). A key already holding a conflict value keeps it.
fn v08_path_claims(v: &mut V<'_>, st: &mut State, sides: [&State; 3]) {
    let mut groups: BTreeMap<(String, String), Vec<Nid>> = BTreeMap::new();
    for (n, x) in &st.nodes {
        if !x.live() || x.kind != "artifact" || !(x.status == "present" || x.status == "planned") {
            continue;
        }
        let path = match flat(&x.kstate(&st.schema, &Aspect::Observation)) {
            Some(KVal::Observation(vs)) => match vs.first() {
                Some(Some(Value::Path(p))) => (p.root.clone(), p.text.clone()),
                _ => continue,
            },
            _ => continue,
        };
        groups.entry(path).or_default().push(*n);
    }
    let mut claim: Vec<Nid> = groups
        .into_values()
        .filter(|g| g.len() > 1)
        .flatten()
        .filter(|n| !st.nodes[n].conflicts.contains_key(&Aspect::Observation))
        .collect();
    claim.sort_by_cached_key(|n| v.ck(st, &Key::Node(*n, Aspect::Observation)));
    let schema = st.schema.clone();
    for n in claim {
        let side = |s: &State| flat(&crate::merge::cval(s, n, &Aspect::Observation));
        let c = Conflict {
            class: "PathClaim".into(),
            base: side(sides[0]),
            ours: side(sides[1]),
            theirs: side(sides[2]),
            prov: None,
            images: [None, None, None],
        };
        st.nodes.get_mut(&n).expect("a claimant").set_kstate(
            &schema,
            &Aspect::Observation,
            KState::Conflict(Box::new(c)),
        );
        v.out
            .conflicts
            .push((Key::Node(n, Aspect::Observation), "PathClaim".into()));
    }
}

/// V09 (I11): every node conforms to the merged schema ([F08 §8.6]); one violation per non-conforming node, keyed by
/// its first non-conforming key in canonical order ([F12 §7.9]).
fn v09_schema(v: &mut V<'_>, st: &State) {
    let s = &st.schema;
    let mut found = Vec::new();
    for (n, x) in &st.nodes {
        let mut bad: Vec<(Key, String)> = Vec::new();
        let key = |a: Aspect| Key::Node(*n, a);
        let Some(kind) = s.kind(&x.kind) else {
            bad.push((
                key(Aspect::Existence),
                format!("kind {} is not in the schema", x.kind),
            ));
            found.push(first(v, st, bad));
            continue;
        };
        if s.value(&x.kind, "status", &x.status).is_none() {
            bad.push((
                key(Aspect::Status),
                format!("{} is not a status of {}", x.status, x.kind),
            ));
        }
        if x.live() {
            for (f, val) in &x.fields {
                let a = if matches!(val, Value::Counter(_)) {
                    Aspect::Counter(f.clone())
                } else {
                    Aspect::Field(f.clone())
                };
                match s.field(&x.kind, f) {
                    None => bad.push((key(a), format!("{} has no field {f}", x.kind))),
                    Some(fi) if !crate::inv::value_fits(fi.ty, val) => bad.push((
                        key(a),
                        format!("{}.{f} holds a value of another type", x.kind),
                    )),
                    Some(_) => {
                        if let Value::Enum(name) = val
                            && s.value(&x.kind, f, name).is_none_or(|e| e.retired)
                        {
                            bad.push((key(a), format!("{name} is not a value of {}.{f}", x.kind)));
                        }
                    }
                }
            }
            for fi in s.fields_of(&x.kind) {
                let required = !fi.optional
                    && fi.default.is_none()
                    && fi.name != "status"
                    && !(fi.name == "title" && kind.title_derived)
                    && !(fi.name == "root" && x.kind == "area");
                if required && !x.fields.contains_key(&fi.name) {
                    bad.push((
                        key(Aspect::Field(fi.name.clone())),
                        format!("the required field {} is absent", fi.name),
                    ));
                }
            }
            if x.kind == "rule"
                && matches!(x.fields.get("authority"), Some(Value::Enum(a)) if a == "owner")
                && !x.fields.contains_key("owner_quote")
            {
                bad.push((
                    key(Aspect::Field("owner_quote".into())),
                    "a rule with authority owner needs owner_quote".into(),
                ));
            }
            for (k, p) in &x.out {
                let a = key(Aspect::Edge(k.clone()));
                let Some(e) = s.edge(&k.kind) else {
                    bad.push((a, format!("edge kind {} is not in the schema", k.kind)));
                    continue;
                };
                if let Some(d) = st.nodes.get(&k.dst)
                    && (d.live() || e.class != EdgeClass::Historical)
                    && (!e.dst.allows(&d.kind)
                        || !e.src.allows(&x.kind)
                        || (e.same_kind && x.kind != d.kind))
                {
                    bad.push((
                        a.clone(),
                        format!("{} joins {} to {}", k.kind, x.kind, d.kind),
                    ));
                }
                if (p.flagged && e.props != Props::Flagged)
                    || (p.pinned.is_some() && e.props != Props::Pinned)
                {
                    bad.push((
                        a,
                        format!("{} carries a property its kind does not admit", k.kind),
                    ));
                }
            }
        }
        if !bad.is_empty() {
            found.push(first(v, st, bad));
        }
    }
    let found: Vec<(Key, String, String)> = found
        .into_iter()
        .map(|(k, d)| {
            let t = v.text(&k);
            (k, d, format!("moirai resolve '{t}' --take ours"))
        })
        .collect();
    v.emit(st, "SchemaConflict", found);
}

fn first(v: &V<'_>, st: &State, mut bad: Vec<(Key, String)>) -> (Key, String) {
    bad.sort_by_cached_key(|(k, _)| v.ck(st, k));
    bad.swap_remove(0)
}

/// The project queries a query's text calls, by the names its `CALL`s write that resolve to a project query (`std`
/// first, [50 §4.4] "Invocation"); `None` when the text does not parse ([F19 §12.5.4]: it contributes no edge).
fn callees(text: &str, projects: &BTreeSet<String>) -> Option<BTreeSet<String>> {
    use crate::lq::lexer::{Lexer, Punct, TokKind};
    crate::lq::parser::parse_define(text, crate::lq::parser::ParseOptions::default()).ok()?;
    let lx = Lexer::new(text);
    let mut toks = Vec::new();
    let mut pos = 0;
    while pos < text.len() {
        match lx.token(pos) {
            Ok(t) if t.kind == TokKind::Eof => break,
            Ok(t) => {
                pos = t.end.max(pos + 1);
                toks.push(t);
            }
            Err(_) => pos += 1,
        }
    }
    let word = |i: usize| -> Option<String> {
        let t = toks.get(i)?;
        match t.kind {
            TokKind::Word => Some(text[t.start..t.end].to_string()),
            TokKind::QIdent => Some(lx.qident_value(t)),
            _ => None,
        }
    };
    let mut out = BTreeSet::new();
    for i in 0..toks.len() {
        if !word(i).is_some_and(|w| w.eq_ignore_ascii_case("CALL")) {
            continue;
        }
        if i > 0 && toks[i - 1].kind == TokKind::Punct(Punct::Dot) {
            continue;
        }
        let mut name = match word(i + 1) {
            Some(w) => w,
            None => continue,
        };
        let mut j = i + 2;
        while toks
            .get(j)
            .is_some_and(|t| t.kind == TokKind::Punct(Punct::Dot))
        {
            match word(j + 1) {
                Some(w) => {
                    name.push('.');
                    name.push_str(&w);
                    j += 2;
                }
                None => break,
            }
        }
        if name.starts_with("std.") || name.starts_with("tx.") {
            continue;
        }
        if crate::lq::catalog::std_query(&name).is_some() {
            continue;
        }
        if projects.contains(&name) {
            out.insert(name);
        }
    }
    Some(out)
}

/// V10 and V11 ([F19 §12.5]): when the candidate's net changeset against dst changes a schema item, every named query
/// in scope parses and binds (`QueryInvalid`), and the call graph has no cycle through a query in scope
/// (`QueryCycle`).
fn v10_v11_queries(v: &mut V<'_>, st: &State, d: &State, sides: [&State; 3]) {
    let items_differ = |a: &State, b: &State| a.schema.items != b.schema.items;
    if !items_differ(st, d) {
        return;
    }
    let queries = |s: &State| -> BTreeMap<String, String> {
        s.schema
            .items
            .values()
            .filter_map(|i| match i {
                crate::schema::Item::Query(q) => Some((q.name.clone(), q.text.clone())),
                _ => None,
            })
            .collect()
    };
    let (qc, qd) = (queries(st), queries(d));
    let names: BTreeSet<String> = qc.keys().cloned().collect();
    let other_changed = st
        .schema
        .items
        .iter()
        .filter(|(k, _)| !matches!(k, ItemKey::Query(_)))
        .any(|(k, i)| d.schema.items.get(k) != Some(i))
        || d.schema
            .items
            .keys()
            .filter(|k| !matches!(k, ItemKey::Query(_)))
            .any(|k| !st.schema.items.contains_key(k));
    let changed = |q: &str| qc.get(q) != qd.get(q);
    let calls: BTreeMap<String, Option<BTreeSet<String>>> = qc
        .iter()
        .map(|(n, t)| (n.clone(), callees(t, &names)))
        .collect();
    let scope: BTreeSet<String> = qc
        .keys()
        .filter(|q| {
            other_changed
                || changed(q)
                || calls[*q]
                    .as_ref()
                    .is_some_and(|cs| cs.iter().any(|p| changed(p)))
                || qd
                    .keys()
                    .any(|p| changed(p) && !qc.contains_key(p) && text_calls(&qc[*q], p))
        })
        .cloned()
        .collect();
    let lq = crate::lqh::lq_schema(&st.schema);
    // V10.
    let mut invalid = Vec::new();
    for q in &scope {
        let text = &qc[q];
        let Err(diags) = crate::lq::catalog::named_query_checked(q, text, &lq) else {
            continue;
        };
        let Some(first) = diags
            .iter()
            .min_by_key(|x| (x.span.map_or(u32::MAX, |s| s.start), x.code.as_str()))
        else {
            continue;
        };
        if first.message.contains("(QueryCycle)") {
            continue; // a cycle is V11's
        }
        let parses =
            crate::lq::parser::parse_define(text, crate::lq::parser::ParseOptions::default())
                .is_ok();
        let mut msg = first.message.clone();
        msg.truncate(160);
        let description = format!(
            "named query {q} no longer {}: {} {msg}",
            if parses { "binds" } else { "parses" },
            first.code.as_str()
        );
        let key = Key::Schema(ItemKey::Query(q.clone()));
        let skey = v.text(&key);
        let takes = ["ours", "theirs", "base"]
            .iter()
            .zip([sides[1], sides[2], sides[0]])
            .find(|(_, s)| {
                let mut alt = st.schema.clone();
                match s.schema.items.get(&ItemKey::Query(q.clone())) {
                    Some(i) => {
                        alt.items.insert(ItemKey::Query(q.clone()), i.clone());
                    }
                    None => {
                        alt.items.remove(&ItemKey::Query(q.clone()));
                    }
                }
                let l = crate::lqh::lq_schema(&alt);
                match alt.query(q) {
                    Some(qi) => crate::lq::catalog::named_query_checked(q, &qi.text, &l).is_ok(),
                    None => false,
                }
            })
            .map(|(n, _)| *n);
        let suggested = match takes {
            Some(side) => format!("moirai resolve '{skey}' --take {side}"),
            None => format!(
                "moirai resolve '{skey}' --value - with a definition of {q} that binds against this schema"
            ),
        };
        invalid.push((key, description, suggested));
    }
    v.emit(st, "QueryInvalid", invalid);
    // V11: the strongly connected components of the call graph with a cycle and a query in scope.
    let ids: BTreeMap<&String, Nid> = qc
        .keys()
        .enumerate()
        .map(|(i, q)| (q, Nid(i as u32 + 1)))
        .collect();
    let by_id: BTreeMap<Nid, &String> = ids.iter().map(|(q, n)| (*n, *q)).collect();
    let mut adj: BTreeMap<Nid, Vec<Nid>> = BTreeMap::new();
    for (q, cs) in &calls {
        let a = ids[q];
        adj.entry(a).or_default();
        for p in cs.iter().flatten() {
            adj.entry(a).or_default().push(ids[p]);
        }
    }
    let comp = scc(&adj);
    let mut members: BTreeMap<usize, Vec<&String>> = BTreeMap::new();
    for (n, c) in &comp {
        members.entry(*c).or_default().push(by_id[n]);
    }
    let mut cyc = Vec::new();
    for ms in members.values() {
        let self_loop = ms.len() == 1 && calls[ms[0]].as_ref().is_some_and(|cs| cs.contains(ms[0]));
        if ms.len() < 2 && !self_loop {
            continue;
        }
        if !ms.iter().any(|q| scope.contains(*q)) {
            continue;
        }
        let w = ms.iter().min().expect("a member");
        let inside: BTreeSet<&String> = ms.iter().copied().collect();
        // The first path back to w of a DFS from w over successors inside the component in ascending name order.
        let path = cycle_path(w, &calls, &inside);
        let mut shown: Vec<String> = path.iter().take(8).cloned().collect();
        if path.len() > 8 {
            shown.push("...".into());
        }
        let key = Key::Schema(ItemKey::Query((*w).clone()));
        let skey = v.text(&key);
        cyc.push((
            key,
            format!(
                "named queries call each other in a cycle: {}",
                shown.join(" -> ")
            ),
            format!(
                "moirai resolve '{skey}' --value - with a definition of {w} that does not call {}",
                path.get(1).cloned().unwrap_or_default()
            ),
        ));
    }
    v.emit(st, "QueryCycle", cyc);
}

/// Whether a query text writes a `CALL` of `name` (for a query `d` held and the candidate dropped).
fn text_calls(text: &str, name: &str) -> bool {
    let all: BTreeSet<String> = [name.to_string()].into_iter().collect();
    callees(text, &all).is_some_and(|c| c.contains(name))
}

fn cycle_path(
    w: &String,
    calls: &BTreeMap<String, Option<BTreeSet<String>>>,
    inside: &BTreeSet<&String>,
) -> Vec<String> {
    fn dfs(
        x: &String,
        w: &String,
        calls: &BTreeMap<String, Option<BTreeSet<String>>>,
        inside: &BTreeSet<&String>,
        path: &mut Vec<String>,
        seen: &mut BTreeSet<String>,
    ) -> bool {
        for y in calls[x].iter().flatten() {
            if !inside.contains(y) {
                continue;
            }
            if y == w {
                path.push(w.clone());
                return true;
            }
            if seen.insert(y.clone()) {
                path.push(y.clone());
                if dfs(y, w, calls, inside, path, seen) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }
    let mut path = vec![w.clone()];
    let mut seen = BTreeSet::new();
    seen.insert(w.clone());
    dfs(w, w, calls, inside, &mut path, &mut seen);
    path
}

/// V12 (I33′): a merge into `plan/*` that would write `status`, `resolution` or `assignee` stages, keyed by the masked
/// key ([F12 §7.9]).
fn v12_plan_mask(v: &mut V<'_>, st: &State, o: &State) {
    let mut found = Vec::new();
    let nodes: BTreeSet<Nid> = st.nodes.keys().chain(o.nodes.keys()).copied().collect();
    for n in nodes {
        for a in [Aspect::Status, Aspect::Field("assignee".into())] {
            let (x, y) = (crate::merge::cval(st, n, &a), crate::merge::cval(o, n, &a));
            // A node the merge created or deleted writes its status with its existence; only a changed status of a
            // node live on both is masked.
            let both_live = st.live(n).is_some() && o.live(n).is_some();
            if both_live && x != y {
                let k = Key::Node(n, a);
                let t = v.text(&k);
                found.push((
                    k,
                    format!("{t} is masked on plan branches"),
                    format!("moirai resolve '{t}' --take ours"),
                ));
            }
        }
    }
    v.emit(st, "PlanMask", found);
}

/// V13: `Duplicate` (HT-001) and `Contradiction` (HT-002) hints.
// rule: HT-001, HT-002
fn v13_hints(v: &mut V<'_>, st: &State, o: &State) {
    let uid = v.uid;
    let mut by: BTreeMap<(String, String, Nid), Vec<Nid>> = BTreeMap::new();
    for (n, x) in &st.nodes {
        let (Some(t), Some(p)) = (x.text("title"), x.parent) else {
            continue;
        };
        if x.live() {
            by.entry((x.kind.clone(), t.to_string(), p))
                .or_default()
                .push(*n);
        }
    }
    for ((kind, _, p), mut ns) in by {
        if ns.len() < 2 || ns.iter().all(|n| o.live(*n).is_some()) {
            continue;
        }
        ns.sort_by_key(|n| uid(*n));
        v.out.hints.push(Hint {
            class: "Duplicate",
            text: format!(
                "{} and {} are {kind} nodes with the same title under {p}",
                ns[0], ns[1]
            ),
        });
    }
    let rules: Vec<Nid> = st
        .nodes
        .iter()
        .filter(|(_, x)| x.live() && x.kind == "rule" && x.status == "active")
        .map(|(n, _)| *n)
        .collect();
    for (i, a) in rules.iter().enumerate() {
        for b in &rules[i + 1..] {
            let (x, y) = (&st.nodes[a], &st.nodes[b]);
            let same_scope = x.fields.get("applies_to") == y.fields.get("applies_to");
            let contradict = x.out.keys().any(|k| k.kind == "contradicts" && k.dst == *b)
                || y.out.keys().any(|k| k.kind == "contradicts" && k.dst == *a);
            let heading = x.text("title").is_some() && x.text("title") == y.text("title");
            if same_scope && (contradict || heading) {
                let why = if contradict {
                    "contradict each other"
                } else {
                    "have the same heading"
                };
                v.out.hints.push(Hint {
                    class: "Contradiction",
                    text: format!("rules {a} and {b} apply to the same scope and {why}"),
                });
            }
        }
    }
}
