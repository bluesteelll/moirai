//! Invariant predicates over one view's state ([F13 §3]; [F13 §1.4]: pure functions over the materialised state),
//! each tagged with its invariant; the write path's deferred validators ([F13 §5]) run them on the candidate, and the
//! model's suites run them on every head.

use crate::api::AllocTable;
use crate::schema::{Card, EdgeClass, Elem, Props, Ty};
use crate::state::State;
use crate::value::{Nid, Value};
use std::collections::BTreeMap;

/// I1: a `#N` is unique and never reused, and a uid is unique: the allocation is a bijection between the allocated
/// `#N`s (1 … `next_id` − 1, holes excepted) and their uids.
// spec: [F13 §3.1] I1
pub fn i1_ids_unique(a: &AllocTable, next_id: u32) -> Result<(), String> {
    if a.rows.len() != a.uidx.len() {
        return Err("two #N share a uid".into());
    }
    for (n, (u, ..)) in &a.rows {
        if n.0 == 0 || n.0 >= next_id {
            return Err(format!("{n} lies outside 1..next_id"));
        }
        if a.uidx.get(u) != Some(n) || *u == crate::value::Uid::ZERO {
            return Err(format!("{n}'s uid does not map back to it"));
        }
    }
    Ok(())
}

/// I35′: a `#N` is bound to at most one uid over the store's life: every uid a state holds for a `#N` is the
/// allocation's.
// spec: [F13 §3.1] I35′
pub fn i35p_id_binds_one_uid(a: &AllocTable, st: &State) -> Result<(), String> {
    for (n, x) in &st.nodes {
        match a.rows.get(n) {
            Some((u, ..)) if *u == x.uid => {}
            _ => return Err(format!("{n} holds a uid its allocation does not bind")),
        }
    }
    Ok(())
}

/// I2 on a state: every structural edge has live endpoints, except a flagged `blocks` or `gates` edge whose source is a
/// tombstone (FL-004).
// spec: [F13 §3.2] I2
// rule: FL-004
pub fn i2_structural_edges_live(st: &State) -> Result<(), String> {
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
                return Err(format!("edge:{n}:{}:{} has a dead endpoint", k.kind, k.dst));
            }
        }
        if x.live()
            && let Some(p) = x.parent
            && st.live(p).is_none()
        {
            return Err(format!("{n}'s parent {p} is not live"));
        }
    }
    Ok(())
}

/// I4: `parent` is a forest of depth ≤ 12.
// spec: [F13 §3.2] I4
pub fn i4_parent_forest(st: &State) -> Result<(), String> {
    for (n, x) in &st.nodes {
        if !x.live() {
            continue;
        }
        let mut cur = x.parent;
        let mut depth = 0;
        while let Some(p) = cur {
            depth += 1;
            if p == *n || depth > 12 {
                return Err(format!("{n}: parent cycle or depth above 12"));
            }
            cur = st.live(p).and_then(|y| y.parent);
        }
    }
    Ok(())
}

/// I5′: the combined precedence graph is acyclic (DFS, [`crate::derived::i5p_cycle_witness`]).
// spec: [F13 §3.2] I5′
pub fn i5p_precedence_acyclic(st: &State) -> Result<(), String> {
    match crate::derived::i5p_cycle_witness(st) {
        None => Ok(()),
        Some((a, k, b)) => Err(format!("{a} {k} {b} lies on a cycle")),
    }
}

/// Whether a node is **active**: live, with a status that is not a side state ([F08 §8.4.6] `max-1-active-per-dst`).
fn active(st: &State, n: Nid) -> bool {
    st.live(n).is_some_and(|x| {
        st.schema
            .value(&x.kind, "status", &x.status)
            .is_some_and(|v| !v.side)
    })
}

/// I6: `supersedes(new, old)` implies `old.status = superseded`, and a target has at most one active superseder.
// spec: [F13 §3.2] I6
pub fn i6_supersedes(st: &State) -> Result<(), String> {
    let mut by_dst: BTreeMap<Nid, Vec<Nid>> = BTreeMap::new();
    for (n, x) in &st.nodes {
        for k in x.out.keys().filter(|k| k.kind == "supersedes") {
            by_dst.entry(k.dst).or_default().push(*n);
            if x.live()
                && let Some(old) = st.live(k.dst)
                && old.status != "superseded"
            {
                return Err(format!(
                    "{n} supersedes {} whose status is {}",
                    k.dst, old.status
                ));
            }
        }
    }
    for (d, srcs) in by_dst {
        if srcs.iter().filter(|s| active(st, **s)).count() > 1 {
            return Err(format!("{d} has more than one active superseder"));
        }
    }
    Ok(())
}

/// I7: the target of `duplicate_of` is canonical (the chain has length 1).
// spec: [F13 §3.2] I7
pub fn i7_duplicate_canonical(st: &State) -> Result<(), String> {
    for (n, x) in &st.nodes {
        if !x.live() {
            continue;
        }
        let outs: Vec<Nid> = x
            .out
            .keys()
            .filter(|k| k.kind == "duplicate_of")
            .map(|k| k.dst)
            .collect();
        if outs.len() > 1 {
            return Err(format!("{n} is a duplicate of more than one node"));
        }
        for d in outs {
            if st
                .live(d)
                .is_some_and(|y| y.out.keys().any(|k| k.kind == "duplicate_of"))
            {
                return Err(format!("{n} duplicates {d}, which is itself a duplicate"));
            }
        }
    }
    Ok(())
}

/// V07: the cardinalities other than I6's ([F13 §5]; [F08 §8.4.6] `card`): `duplicate_of` chain length 1 (I7),
/// `max-1-per-src` (`runs_in`), `max-1-active-per-dst` (`answers`).
// spec: [F13 §5] V07
pub fn cardinalities(st: &State) -> Result<(), String> {
    i7_duplicate_canonical(st)?;
    let mut per_dst: BTreeMap<(String, Nid), usize> = BTreeMap::new();
    for (n, x) in &st.nodes {
        if !x.live() {
            continue;
        }
        let mut per_src: BTreeMap<&str, usize> = BTreeMap::new();
        for k in x.out.keys() {
            let Some(e) = st.schema.edge(&k.kind) else {
                continue;
            };
            match e.card {
                Card::Max1PerSrc => {
                    let c = per_src.entry(&k.kind).or_default();
                    *c += 1;
                    if *c > 1 {
                        return Err(format!("{n} has more than one {} edge", k.kind));
                    }
                }
                Card::Max1ActivePerDst if k.kind != "supersedes" && active(st, *n) => {
                    let c = per_dst.entry((k.kind.clone(), k.dst)).or_default();
                    *c += 1;
                    if *c > 1 {
                        return Err(format!(
                            "{} has more than one active {} in-edge",
                            k.dst, k.kind
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// Whether a stored value is of a field type ([F08 §5.1]): the value's variant is the type's, and a set's elements
/// are of its element type.
pub fn value_fits(ty: Ty, v: &Value) -> bool {
    match (ty, v) {
        (Ty::Bool, Value::Bool(_))
        | (Ty::Int, Value::Int(_))
        | (Ty::Counter, Value::Counter(_))
        | (Ty::F64, Value::F64(_))
        | (Ty::Enum, Value::Enum(_))
        | (Ty::Text | Ty::Sym, Value::Text(_))
        | (Ty::Ref, Value::Ref(_))
        | (Ty::Commit, Value::Commit(_))
        | (Ty::Path, Value::Path(_))
        | (Ty::Oid, Value::Oid(_))
        | (Ty::PathMove, Value::PathMove(_)) => true,
        (Ty::Set(e), Value::Set(items)) => {
            let et = match e {
                Elem::Sym => Ty::Sym,
                Elem::Path => Ty::Path,
                Elem::Int => Ty::Int,
                Elem::PathMove => Ty::PathMove,
            };
            items.iter().all(|x| value_fits(et, x))
        }
        _ => false,
    }
}

/// I11: schema conformance at write time ([F08 §8.6] rules 1, 2's kinds, 4 and 5): a live node's kind, header values
/// and fields belong to the effective schema with values of their fields' types, its required fields are present
/// (the title of a kind whose title is derived is not stored, [F08 §9.2]), a rule with `authority = owner` has an
/// `owner_quote`, and every edge meets its kind's endpoint kinds, `same_kind` and properties.
// spec: [F13 §3.3] I11
pub fn i11_schema_conformance(st: &State) -> Result<(), String> {
    let s = &st.schema;
    let mut required: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (n, x) in &st.nodes {
        let Some(k) = s.kind(&x.kind) else {
            return Err(format!("{n}: kind {} is not in the schema", x.kind));
        };
        if s.value(&x.kind, "status", &x.status).is_none() {
            return Err(format!("{n}: {} is not a status of {}", x.status, x.kind));
        }
        if !x.live() {
            continue;
        }
        for (f, v) in &x.fields {
            let Some(fi) = s.field(&x.kind, f) else {
                return Err(format!("{n}: {} has no field {f}", x.kind));
            };
            if !value_fits(fi.ty, v) {
                return Err(format!("{n}: {}.{f} holds a value of another type", x.kind));
            }
            if let Value::Enum(name) = v
                && s.value(&x.kind, f, name).is_none_or(|e| e.retired)
            {
                return Err(format!("{n}: {name} is not a value of {}.{f}", x.kind));
            }
        }
        let req = required.entry(x.kind.as_str()).or_insert_with(|| {
            s.fields_of(&x.kind)
                .into_iter()
                .filter(|fi| {
                    !fi.optional
                        && fi.default.is_none()
                        && fi.name != "status"
                        && !(fi.name == "title" && k.title_derived)
                        && !(fi.name == "root" && x.kind == "area")
                })
                .map(|fi| fi.name.clone())
                .collect()
        });
        if let Some(f) = req.iter().find(|f| !x.fields.contains_key(*f)) {
            return Err(format!("{n}: the required field {f} is absent"));
        }
        if x.kind == "rule"
            && matches!(x.fields.get("authority"), Some(Value::Enum(a)) if a == "owner")
            && !x.fields.contains_key("owner_quote")
        {
            return Err(format!(
                "{n}: a rule with authority owner needs owner_quote"
            ));
        }
        for (k, p) in &x.out {
            let Some(e) = s.edge(&k.kind) else {
                return Err(format!("{n}: edge kind {} is not in the schema", k.kind));
            };
            if let Some(d) = st.nodes.get(&k.dst) {
                let dead_ok = e.class == EdgeClass::Historical;
                if d.live() || !dead_ok {
                    if !e.dst.allows(&d.kind) || !e.src.allows(&x.kind) {
                        return Err(format!(
                            "edge:{n}:{}:{} joins {} to {}",
                            k.kind, k.dst, x.kind, d.kind
                        ));
                    }
                    if e.same_kind && x.kind != d.kind {
                        return Err(format!("edge:{n}:{}:{} joins two kinds", k.kind, k.dst));
                    }
                }
            }
            if (p.flagged && e.props != Props::Flagged)
                || (p.pinned.is_some() && e.props != Props::Pinned)
            {
                return Err(format!(
                    "edge:{n}:{}:{} carries a property its kind does not admit",
                    k.kind, k.dst
                ));
            }
        }
    }
    Ok(())
}

/// I12 at one head: I2, I4, I5′, I6, I7, I8 and I11 hold ([F13 §3.3]).
// spec: [F13 §3.3] I12
pub fn i12_heads_valid(st: &State) -> Result<(), String> {
    i2_structural_edges_live(st)?;
    i4_parent_forest(st)?;
    i5p_precedence_acyclic(st)?;
    i6_supersedes(st)?;
    cardinalities(st)?;
    crate::status::i8_status_machine(st)?;
    i11_schema_conformance(st)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Schema;
    use crate::state::{Creator, EdgeKey, EdgeProps, Node};
    use crate::value::Uid;

    fn node(n: u8, kind: &str, title: Option<&str>) -> Node {
        let s = Schema::default();
        let mut x = Node::new(Uid([n; 16]), kind, &s, Creator::default());
        if let Some(t) = title {
            x.set_field(&s, "title", Some(Value::Text(t.into())));
        }
        x
    }

    #[test]
    fn i11_checks_required_fields_and_value_types() {
        let mut st = State::default();
        st.nodes.insert(Nid(1), node(1, "task", Some("a")));
        assert!(i11_schema_conformance(&st).is_ok());
        // An artifact's title is derived, not stored ([F08 §9.2]); its other required fields still bind.
        let mut a = node(2, "artifact", None);
        for (f, v) in [
            ("root", Value::Text("project".into())),
            (
                "origin_path",
                Value::Path(crate::value::PathVal {
                    root: "project".into(),
                    text: "a.rs".into(),
                }),
            ),
            (
                "path",
                Value::Path(crate::value::PathVal {
                    root: "project".into(),
                    text: "a.rs".into(),
                }),
            ),
        ] {
            a.fields.insert(f.into(), v);
        }
        st.nodes.insert(Nid(2), a);
        assert!(
            i11_schema_conformance(&st).is_ok(),
            "{:?}",
            i11_schema_conformance(&st)
        );
        // A task without a title fails.
        st.nodes.insert(Nid(3), node(3, "task", None));
        assert!(i11_schema_conformance(&st).unwrap_err().contains("title"));
        st.nodes.remove(&Nid(3));
        // A value of another type fails.
        st.nodes
            .get_mut(&Nid(1))
            .unwrap()
            .fields
            .insert("estimate".into(), Value::Text("3".into()));
        assert!(
            i11_schema_conformance(&st)
                .unwrap_err()
                .contains("another type")
        );
        assert!(value_fits(
            Ty::Set(Elem::Sym),
            &Value::Set(vec![Value::Text("x".into())])
        ));
        assert!(!value_fits(
            Ty::Set(Elem::Int),
            &Value::Set(vec![Value::Text("x".into())])
        ));
    }

    #[test]
    fn structural_edges_need_live_ends_and_the_forest_its_depth() {
        let mut st = State::default();
        for n in 1..=14u8 {
            let mut x = node(n, "task", Some("t"));
            if n > 1 {
                x.parent = Some(Nid(u32::from(n) - 1));
            }
            st.nodes.insert(Nid(u32::from(n)), x);
        }
        assert!(i4_parent_forest(&st).is_err(), "depth 13");
        st.nodes.get_mut(&Nid(14)).unwrap().parent = None;
        assert!(i4_parent_forest(&st).is_ok());
        st.nodes.get_mut(&Nid(1)).unwrap().out.insert(
            EdgeKey {
                kind: "blocks".into(),
                dst: Nid(99),
                disc: None,
            },
            EdgeProps::default(),
        );
        assert!(i2_structural_edges_live(&st).is_err());
    }
}
