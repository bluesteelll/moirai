//! Pattern matching by nested loops ([50 §3.4], §3.6, §3.7; [LQ/canonical-ast §5.4], §5.7): one binding per matched
//! assignment of every pattern element, named or anonymous; within one clause two edge patterns never bind the same
//! edge, while nodes may repeat; a symmetric kind or an undirected pattern matches both orientations; quantified parts
//! bind endpoint pairs, by walk lengths ([50 §3.7] item 2), computed level by level and stopping at deleted nodes
//! unless the group's node patterns admit `DELETED`; node patterns never bind a deleted node without the pseudo-label
//! `DELETED`.
//!
//! In a `TX` block a variable an earlier `MATCH … EXPECT` bound to several values holds them as a list ([LQ/canonical-ast
//! §5.7] V10); a later pattern that joins on it binds each of its elements in turn, and the variable reads as that one
//! element in the rest of the row.

use super::expr::{Env, Row, truth};
use super::val::{self, EdgeV, V};
use super::view::Ev;
use crate::err::Res;
use crate::lq::cast::{CEdgeP, CExpr, CGroup, CNode, CPath, CStep};
use crate::lq::diag::Code;
use crate::value::Nid;
use std::collections::BTreeSet;

/// The state of one clause's match: its patterns, `WHERE`, parameters, the rows found, and whether one row is enough.
struct M<'p> {
    paths: &'p [CPath],
    where_: Option<&'p CExpr>,
    params: &'p [V],
    stop: bool,
    out: Vec<Row>,
}

/// One quantified step: a single edge pattern or a group.
#[derive(Clone, Copy)]
enum QStep<'p> {
    Edge(&'p CEdgeP),
    Group(&'p CGroup),
}

impl Ev<'_> {
    /// Matches a clause's patterns for one input row and keeps the rows whose `WHERE` holds ([50 §3.1] item 2), counting
    /// W01's and W10's rows ([LQ/errors §5.6]).
    // spec: [50 §3.4]
    pub fn match_paths(
        &self,
        base: &Row,
        paths: &[CPath],
        where_: Option<&CExpr>,
        params: &[V],
        out: &mut Vec<Row>,
        stop: bool,
    ) -> Res<()> {
        let mut m = M {
            paths,
            where_,
            params,
            stop,
            out: Vec::new(),
        };
        self.literal_ids(paths)?;
        self.path_start(&mut m, 0, base.clone(), &[])?;
        out.append(&mut m.out);
        Ok(())
    }

    /// The notices of the literal ids of a clause's node patterns, at any position of a path ([50 §3.6]), each id
    /// once per view.
    fn literal_ids(&self, paths: &[CPath]) -> Res<()> {
        fn nodes<'p>(p: &'p CPath, out: &mut Vec<&'p CNode>) {
            out.push(&p.start);
            for s in &p.steps {
                match s {
                    CStep::Edge(_, n) => out.push(n),
                    CStep::Group(g, n) => {
                        nodes(&g.path, out);
                        out.push(n);
                    }
                }
            }
        }
        let mut all = Vec::new();
        for p in paths {
            nodes(p, &mut all);
        }
        for np in all {
            if let Some((_, e @ CExpr::Node(_))) = np.props.iter().find(|(k, _)| k == "id")
                && let V::Node(n) = self.eval(e, &Env::of(&Row::default(), &[]))?
            {
                self.literal_notice(n, &np.labels);
            }
        }
        Ok(())
    }

    /// Whether a binding passes a `WHERE` (a clause's, or a pattern element's inline one), with the counted facts of
    /// the row ([LQ/errors §5.6]): only the outermost evaluation counts, and a nested one keeps the enclosing row's
    /// facts.
    pub fn filter(&self, w: Option<&CExpr>, row: &Row, params: &[V]) -> Res<bool> {
        let Some(w) = w else { return Ok(true) };
        if !self.counting() {
            return self.nested(|| Ok(truth(&self.eval(w, &Env::of(row, params))?)));
        }
        let outer = std::mem::take(&mut *self.flags.borrow_mut());
        let r = self.eval(w, &Env::of(row, params));
        let f = std::mem::replace(&mut *self.flags.borrow_mut(), outer);
        let ok = truth(&r?);
        let mut c = self.w.counts.borrow_mut();
        if !ok {
            for field in f.absent {
                *c.w01.entry(field).or_insert(0) += 1;
            }
        } else if f.none_link {
            c.w10 += 1;
        }
        if f.div0 {
            c.n10 += 1;
        }
        Ok(ok)
    }

    fn path_start(&self, m: &mut M<'_>, pi: usize, row: Row, used: &[EdgeV]) -> Res<()> {
        if m.stop && !m.out.is_empty() {
            return Ok(());
        }
        let Some(p) = m.paths.get(pi) else {
            if self.filter(m.where_, &row, m.params)? {
                m.out.push(row);
            }
            return Ok(());
        };
        for n in self.candidates(&p.start, &row, m.params)? {
            let mut r = row.clone();
            if self.bind_node(&p.start, n, &mut r, m.params)? {
                self.steps(m, pi, 0, n, r, used)?;
            }
            if m.stop && !m.out.is_empty() {
                break;
            }
        }
        Ok(())
    }

    fn steps(
        &self,
        m: &mut M<'_>,
        pi: usize,
        si: usize,
        cur: Nid,
        row: Row,
        used: &[EdgeV],
    ) -> Res<()> {
        let p = &m.paths[pi];
        let Some(step) = p.steps.get(si) else {
            return self.path_start(m, pi + 1, row, used);
        };
        match step {
            CStep::Edge(ep, np) if ep.quant.is_none() => {
                for (e, other) in self.edges_from(cur, ep) {
                    if used.contains(&e) {
                        continue;
                    }
                    let mut r = row.clone();
                    if !self.bind_edge(ep, &e, &mut r, m.params)? {
                        continue;
                    }
                    if !self.bind_node(np, other, &mut r, m.params)? {
                        continue;
                    }
                    let mut u = used.to_vec();
                    u.push(e);
                    self.steps(m, pi, si + 1, other, r, &u)?;
                    if m.stop && !m.out.is_empty() {
                        break;
                    }
                }
            }
            CStep::Edge(ep, np) => {
                let q = ep.quant.unwrap_or((1, Some(1)));
                for b in self.reach(cur, QStep::Edge(ep), q, &row, m.params)? {
                    let mut r = row.clone();
                    if self.bind_node(np, b, &mut r, m.params)? {
                        self.steps(m, pi, si + 1, b, r, used)?;
                    }
                    if m.stop && !m.out.is_empty() {
                        break;
                    }
                }
            }
            CStep::Group(g, np) => {
                for b in self.reach(cur, QStep::Group(g), g.quant, &row, m.params)? {
                    let mut r = row.clone();
                    if self.bind_node(np, b, &mut r, m.params)? {
                        self.steps(m, pi, si + 1, b, r, used)?;
                    }
                    if m.stop && !m.out.is_empty() {
                        break;
                    }
                }
            }
        }
        Ok(())
    }

    /// Whether a node pattern admits a node by its labels: a kind label admits live nodes of that kind, `DELETED`
    /// admits tombstones (of the labelled kinds, if any); without labels, every live node ([50 §3.6]).
    // spec: [50 §3.6]
    pub fn admits(&self, labels: &[String], n: Nid) -> bool {
        let Some(x) = self.node(n) else { return false };
        let deleted = labels.iter().any(|l| l == "DELETED");
        let kinds: Vec<&String> = labels.iter().filter(|l| *l != "DELETED").collect();
        let kind_ok = kinds.is_empty() || kinds.iter().any(|k| x.kind.eq_ignore_ascii_case(k));
        kind_ok && (x.live() || deleted)
    }

    /// The candidate nodes of a path's first node pattern, ascending by id: its bound node (each node of a `TX`
    /// variable bound to several), the node its `id` names (with N01, N06 or N12 when that node is not live here,
    /// [50 §3.6]), or every node it admits.
    fn candidates(&self, np: &CNode, row: &Row, params: &[V]) -> Res<Vec<Nid>> {
        if let Some(b) = np.var
            && row.is_bound(b)
        {
            // A join: the bound node, or nothing for a binding an `OPTIONAL MATCH` left absent.
            let mut v: Vec<Nid> = row.get(b).elems().iter().filter_map(V::node).collect();
            v.sort_unstable();
            v.dedup();
            return Ok(v);
        }
        if let Some((_, e)) = np.props.iter().find(|(k, _)| k == "id") {
            return Ok(match self.eval(e, &Env::of(row, params))? {
                V::Node(n) => {
                    self.literal_notice(n, &np.labels);
                    vec![n]
                }
                _ => Vec::new(),
            });
        }
        let deleted = np.labels.iter().any(|l| l == "DELETED");
        Ok(self
            .st()
            .nodes
            .iter()
            .filter(|(n, x)| (x.live() || deleted) && self.admits(&np.labels, **n))
            .map(|(n, _)| *n)
            .collect())
    }

    /// The notice for a literal id that names no live node of the view ([50 §3.6]): N01 for a tombstone, N06 for a node
    /// created on another branch and not in this view, N12 for an id never allocated.
    // spec: [50 §3.6]
    pub fn literal_notice(&self, n: Nid, labels: &[String]) {
        if !self.noticed.borrow_mut().insert(n) {
            return;
        }
        let store = self.store();
        match self.node(n) {
            Some(x) if x.live() => {}
            Some(x) => {
                if labels.iter().any(|l| l == "DELETED") {
                    return;
                }
                let t = x.tomb.as_ref().expect("a tombstone");
                let del = self.st().nodes.get(&n).and_then(|_| {
                    store.dag.chain(self.v.commit).into_iter().find(|c| {
                        store.dag.commits[c].changeset.keys().any(|k| {
                            matches!(k, crate::state::Key::Node(m, crate::state::Aspect::Existence) if *m == n)
                        })
                    })
                });
                let (rev, c8, actor) = del.map_or((0, String::new(), String::new()), |c| {
                    let x = &store.dag.commits[&c];
                    (
                        c,
                        crate::value::hex(&x.id)[..8].to_string(),
                        x.actor.clone(),
                    )
                });
                let reason = t.reason.clone().unwrap_or_default();
                let repl = t.replaced_by.map_or(String::new(), |r| format!(" -> {r}"));
                self.w.note(
                    Code::N01,
                    format!("{n} is deleted in this view (rev {rev} c{c8} by {actor} \"{reason}\"{repl})"),
                    Vec::new(),
                    None,
                );
            }
            None => match store.alloc.rows.get(&n) {
                Some((_, _, r, seq)) => {
                    let c9 = store.dag.commits.get(seq).map_or(String::new(), |c| {
                        format!("c{}", &crate::value::hex(&c.id)[..8])
                    });
                    let deleted = store.dag.live(r).is_none();
                    let created = if deleted {
                        format!("created on deleted branch {r} at rev {seq} ({c9})")
                    } else {
                        format!("created on {r} at rev {seq} ({c9})")
                    };
                    self.w.note(
                        Code::N06,
                        format!(
                            "{n} is not in this view: {created}, not merged into {}",
                            self.v.ref_name
                        ),
                        vec![format!(
                            "USE {r}, or CALL across(refs: [{}, {r}], ids: [{n}])",
                            self.v.ref_name
                        )],
                        None,
                    );
                }
                None => self.w.note(
                    Code::N12,
                    format!("{n} was never allocated in this store"),
                    Vec::new(),
                    None,
                ),
            },
        }
    }

    /// Binds a node to a node pattern: the bound variable must equal it (or, bound to several, hold it); its labels,
    /// property map and inline `WHERE` must hold; a new or anonymous element joins the binding identity ([50 §3.5]).
    fn bind_node(&self, np: &CNode, n: Nid, row: &mut Row, params: &[V]) -> Res<bool> {
        let fresh = match np.var {
            Some(b) if row.is_bound(b) => {
                if !row.get(b).elems().contains(&V::Node(n)) {
                    return Ok(false);
                }
                false
            }
            _ => true,
        };
        if !self.admits(&np.labels, n) {
            return Ok(false);
        }
        if let Some(b) = np.var {
            row.set(b, V::Node(n));
        }
        for (k, e) in &np.props {
            let want = self.eval(e, &Env::of(row, params))?;
            let got = self.prop(n, k)?;
            if val::eq(&got, &want) != Some(true) {
                return Ok(false);
            }
        }
        if !self.filter(np.where_.as_ref(), row, params)? {
            return Ok(false);
        }
        if fresh {
            row.ident.push(V::Node(n));
        }
        Ok(true)
    }

    /// Binds an edge to an edge pattern: the bound variable must equal it; its property map and inline `WHERE` must
    /// hold.
    fn bind_edge(&self, ep: &CEdgeP, e: &EdgeV, row: &mut Row, params: &[V]) -> Res<bool> {
        let fresh = match ep.var {
            Some(b) if row.is_bound(b) => {
                if !row
                    .get(b)
                    .elems()
                    .iter()
                    .any(|v| matches!(v, V::Edge(x) if **x == *e))
                {
                    return Ok(false);
                }
                false
            }
            _ => true,
        };
        if let Some(b) = ep.var {
            row.set(b, V::Edge(Box::new(e.clone())));
        }
        if !self.edge_ok(ep, e, row, params)? {
            return Ok(false);
        }
        if fresh {
            row.ident.push(V::Edge(Box::new(e.clone())));
        }
        Ok(true)
    }

    /// An edge pattern's property map and inline `WHERE` on an edge.
    fn edge_ok(&self, ep: &CEdgeP, e: &EdgeV, row: &Row, params: &[V]) -> Res<bool> {
        for (k, x) in &ep.props {
            let want = self.eval(x, &Env::of(row, params))?;
            if val::eq(&self.edge_prop(e, k)?, &want) != Some(true) {
                return Ok(false);
            }
        }
        self.filter(ep.where_.as_ref(), row, params)
    }

    /// The edges of the view at a node that an edge pattern's types and directions admit, each with its other end
    /// ([LQ/canonical-ast §5.4]): a `right` type follows out-edges, `left` in-edges, `both` both; the hierarchy kind
    /// (`CHILD_OF`, stored `parent`) is the node's `parent` column.
    // spec: [LQ/canonical-ast §5.4]
    pub fn edges_from(&self, cur: Nid, ep: &CEdgeP) -> Vec<(EdgeV, Nid)> {
        let mut out: Vec<(EdgeV, Nid)> = Vec::new();
        let types: Vec<(Option<String>, u8)> = if ep.types.is_empty() {
            vec![(None, ep.dir)]
        } else {
            ep.types
                .iter()
                .filter_map(|(lq, d)| self.stored_name(lq).map(|s| (Some(s), *d)))
                .collect()
        };
        let x = self.node(cur);
        for (kind, dir) in types {
            let k = kind.as_deref();
            if (dir == 1 || dir == 3)
                && let Some(x) = x
            {
                if k.is_none_or(|k| k == "parent")
                    && let Some(p) = x.parent
                {
                    out.push((
                        EdgeV {
                            src: cur,
                            kind: "parent".into(),
                            dst: p,
                            disc: None,
                        },
                        p,
                    ));
                }
                for key in x.out.keys() {
                    if k.is_none_or(|k| k == key.kind) {
                        out.push((
                            EdgeV {
                                src: cur,
                                kind: key.kind.clone(),
                                dst: key.dst,
                                disc: key.disc,
                            },
                            key.dst,
                        ));
                    }
                }
            }
            if dir == 2 || dir == 3 {
                if k.is_none_or(|k| k == "parent")
                    && let Some(cs) = self.ix().children.get(&cur)
                {
                    for c in cs {
                        out.push((
                            EdgeV {
                                src: *c,
                                kind: "parent".into(),
                                dst: cur,
                                disc: None,
                            },
                            *c,
                        ));
                    }
                }
                if let Some(ins) = self.ix().inn.get(&cur) {
                    for (src, key, _) in ins {
                        if k.is_none_or(|k| k == key.kind) {
                            out.push((
                                EdgeV {
                                    src: *src,
                                    kind: key.kind.clone(),
                                    dst: cur,
                                    disc: key.disc,
                                },
                                *src,
                            ));
                        }
                    }
                }
            }
        }
        out.sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        out.dedup();
        out
    }

    /// The ends of one quantified step from `x`: the other ends of the qualifying edges, or the ends of the group's
    /// path matches whose `WHERE` holds ([50 §3.7] item 3); a nested evaluation, whose rows the counted warnings do not
    /// count.
    fn one_step(&self, x: Nid, s: QStep<'_>, row: &Row, params: &[V]) -> Res<BTreeSet<Nid>> {
        self.nested(|| self.one_step_in(x, s, row, params))
    }

    fn one_step_in(&self, x: Nid, s: QStep<'_>, row: &Row, params: &[V]) -> Res<BTreeSet<Nid>> {
        let mut out = BTreeSet::new();
        match s {
            QStep::Edge(ep) => {
                for (e, other) in self.edges_from(x, ep) {
                    if self.edge_ok(ep, &e, row, params)? {
                        out.insert(other);
                    }
                }
            }
            QStep::Group(g) => {
                let mut m = M {
                    paths: std::slice::from_ref(&g.path),
                    where_: g.where_.as_ref(),
                    params,
                    stop: false,
                    out: Vec::new(),
                };
                let mut r = row.clone();
                if self.bind_node(&g.path.start, x, &mut r, params)? {
                    self.steps(&mut m, 0, 0, x, r, &[])?;
                }
                let end = last_node(&g.path);
                for r in m.out {
                    if let Some(b) = end.var
                        && let V::Node(n) = r.get(b)
                    {
                        out.insert(*n);
                    } else if end.var.is_none()
                        && let Some(V::Node(n)) = r.ident.last()
                    {
                        out.insert(*n);
                    }
                }
            }
        }
        Ok(out)
    }

    /// The endpoints of a quantified part from `a` ([50 §3.7]): every b reached by a walk of k qualifying steps with
    /// m ≤ k ≤ n, level by level; a deleted node ends a walk unless the group's node patterns admit `DELETED` ([50
    /// §3.6] last item), when the group's first node pattern decides whether a walk goes on from it. Under the
    /// BFS-distance ablation ([50 §7.4] item 7), the b whose shortest distance lies in [m, n].
    // spec: [50 §3.7]
    // spec: [50 §3.6] quantified traversals
    fn reach(
        &self,
        a: Nid,
        s: QStep<'_>,
        (m, n): (u32, Option<u32>),
        row: &Row,
        params: &[V],
    ) -> Res<Vec<Nid>> {
        let through_deleted = match s {
            QStep::Group(g) => g.path.start.labels.iter().any(|l| l == "DELETED"),
            QStep::Edge(_) => false,
        };
        let live = |x: Nid| through_deleted || self.node(x).is_some_and(|y| y.live());
        let succ = |set: &BTreeSet<Nid>| -> Res<BTreeSet<Nid>> {
            let mut next = BTreeSet::new();
            for x in set.iter().filter(|x| live(**x)) {
                next.extend(self.one_step(*x, s, row, params)?);
            }
            Ok(next)
        };
        let mut out = BTreeSet::new();
        if self.w.ab.bfs_hops {
            let mut seen = BTreeSet::from([a]);
            let mut level = BTreeSet::from([a]);
            let mut k = 0u32;
            if m == 0 {
                out.insert(a);
            }
            while !level.is_empty() && n.is_none_or(|n| k < n) {
                let next: BTreeSet<Nid> = succ(&level)?.difference(&seen).copied().collect();
                k += 1;
                seen.extend(next.iter().copied());
                if k >= m {
                    out.extend(next.iter().copied());
                }
                level = next;
            }
            return Ok(out.into_iter().collect());
        }
        let mut level = BTreeSet::from([a]);
        let mut k = 0u32;
        while k < m && !level.is_empty() {
            level = succ(&level)?;
            k += 1;
        }
        if k < m {
            return Ok(Vec::new());
        }
        match n {
            None => {
                // ∪_{k ≥ m} L_k is everything reachable from L_m with zero or more steps.
                let mut seen = level.clone();
                let mut frontier = level;
                while !frontier.is_empty() {
                    let next: BTreeSet<Nid> = succ(&frontier)?.difference(&seen).copied().collect();
                    seen.extend(next.iter().copied());
                    frontier = next;
                }
                out = seen;
            }
            Some(n) => {
                out.extend(level.iter().copied());
                while k < n && !level.is_empty() {
                    level = succ(&level)?;
                    k += 1;
                    out.extend(level.iter().copied());
                }
            }
        }
        Ok(out.into_iter().collect())
    }
}

/// The last node pattern of a path.
fn last_node(p: &CPath) -> &CNode {
    match p.steps.last() {
        None => &p.start,
        Some(CStep::Edge(_, n) | CStep::Group(_, n)) => n,
    }
}
