//! `H` of a `TX` block ([LQ/canonical-ast §7.2]; [API §7.3]): the LQ text of a block is parsed and bound by the model's
//! own LQ-3 front end ([`crate::lq`]) against the view's schema, the store's identity maps and the caller, and `H` is
//! the first 16 bytes of BLAKE3-256 over the canonical-AST encoding. The binder's refusals (the unknown-name codes,
//! E103, E115, E305, E406, E411) are the static refusals of the block ([API §9.1]; [RULES/role-write-policy] WR-010:
//! "statically known ops are refused by the binder").

use crate::err::Refusal;
use crate::lq::bind::bind_write;
use crate::lq::cast::{CommitId, Root, Uid as LqUid, encode};
use crate::lq::ctx::{BindCtx, Caller, Identities, Params};
use crate::lq::parser::{ParseOptions, parse_write};
use crate::lq::schema::{Ends as LqEnds, FieldTy, Schema as LqSchema};
use crate::schema::{Elem, Ends, Item, Schema, Ty};
use crate::state::State;
use crate::value::{Nid, Uid};
use std::collections::BTreeMap;

/// The identity maps of one view for the binder ([LQ/canonical-ast §5.1] item 2).
pub struct ViewIds<'a> {
    /// `#N` → uid, store-wide.
    pub uids: &'a BTreeMap<Nid, Uid>,
    /// uid → `#N`, store-wide.
    pub uidx: &'a BTreeMap<Uid, Nid>,
    /// The view.
    pub st: &'a State,
    /// `HEAD.next_id`.
    pub next_id: u32,
    /// The commits, for sequence numbers and commit prefixes ([LQ/canonical-ast §5.6]).
    pub commits: &'a [(u64, CommitId, String)],
}

impl Identities for ViewIds<'_> {
    fn uid(&self, n: u32) -> Option<LqUid> {
        self.uids.get(&Nid(n)).map(|u| u.0)
    }
    fn nid(&self, uid: &LqUid) -> Option<u32> {
        self.uidx.get(&Uid(*uid)).map(|n| n.0)
    }
    fn next_id(&self) -> u32 {
        self.next_id
    }
    fn kind_of(&self, uid: &LqUid) -> Option<&str> {
        let n = self.nid(uid)?;
        self.st.live(Nid(n)).map(|x| x.kind.as_str())
    }
    fn commit_by_seq(&self, seq: u64) -> Option<CommitId> {
        self.commits.iter().find(|c| c.0 == seq).map(|c| c.1)
    }
    fn commits_by_prefix(&self, hex: &str) -> Vec<(CommitId, u64, String)> {
        self.commits
            .iter()
            .filter(|c| crate::value::hex(&c.1).starts_with(hex))
            .map(|c| (c.1, c.0, c.2.clone()))
            .collect()
    }
}

/// Every commit of the store as (sequence number, id, ref of its header), the binder's map of sequence numbers and
/// commit prefixes ([LQ/canonical-ast §5.1] item 2).
pub fn commit_table(dag: &crate::dag::Dag) -> Vec<(u64, CommitId, String)> {
    dag.commits
        .values()
        .map(|c| {
            let r = dag
                .refs
                .get(&c.ref_id)
                .map_or_else(String::new, |r| r.name.clone());
            (c.seq, c.id, r)
        })
        .collect()
}

fn lq_ty(t: Ty) -> FieldTy {
    match t {
        Ty::Bool => FieldTy::Bool,
        Ty::Int => FieldTy::Int,
        Ty::Counter => FieldTy::Counter,
        Ty::F64 => FieldTy::Float,
        Ty::Enum => FieldTy::Enum,
        Ty::Text | Ty::Sym => FieldTy::Text,
        Ty::Set(Elem::Path) => FieldTy::PathSet,
        Ty::Set(Elem::PathMove) => FieldTy::PathMoveSet,
        Ty::Set(_) => FieldTy::TextSet,
        Ty::Ref => FieldTy::Ref,
        Ty::Commit => FieldTy::Commit,
        Ty::Path => FieldTy::Path,
        Ty::Oid => FieldTy::Oid,
        Ty::PathMove => FieldTy::PathMoveSet,
        Ty::Body => FieldTy::Body,
    }
}

/// The binder's schema of a view: the LQ core schema with the view's project items added ([LQ/canonical-ast §5.1]
/// item 1).
pub fn lq_schema(s: &Schema) -> LqSchema {
    let mut l = LqSchema::core();
    for item in s.items.values() {
        match item {
            Item::Kind(k) => {
                let statuses: Vec<String> = s
                    .values(&k.name, "status")
                    .iter()
                    .map(|v| {
                        if v.side {
                            format!("!{}", v.name)
                        } else {
                            v.name.clone()
                        }
                    })
                    .collect();
                let refs: Vec<&str> = statuses.iter().map(String::as_str).collect();
                l.add_kind(&k.name, &refs);
            }
            Item::Field(f) => l.add_field(f.kind.as_deref().unwrap_or("*"), &f.name, lq_ty(f.ty)),
            Item::Enum(e) if e.field != "status" => {
                l.add_value(e.kind.as_deref().unwrap_or("*"), &e.field, &e.name)
            }
            Item::Enum(_) => {}
            Item::Edge(e) => {
                let ends = |x: &Ends| match x {
                    Ends::Any => LqEnds::Any,
                    Ends::Kinds(k) => LqEnds::Kinds(k.clone()),
                };
                let rev: Vec<&str> = e.reverse.iter().map(String::as_str).collect();
                l.add_edge(
                    &e.name,
                    &e.lq_name,
                    &rev,
                    ends(&e.src),
                    ends(&e.dst),
                    &e.reading,
                );
            }
            Item::Query(q) => l.add_query(&q.name, &q.text),
            // A policy row names nothing a query binds.
            Item::Policy(_) => {}
        }
    }
    l
}

/// A binder refusal as a Store API refusal: the first diagnostic's code with its exit ([LQ/errors §5.1]), and the
/// byte offset its span starts at, which names the statement. A refusal of one statement is unlocated and names the
/// statement by its 1-based index as its text's `statement <i>: ` prefix (E406's statement case, [LQ/errors §5.5]); it
/// carries that index as its `statement` key, while the verb-level cases, whose texts name no statement, carry none
/// (§5.7; spec sync 2b S2B-F-41).
pub fn refusal(d: &crate::lq::diag::Diag) -> (Refusal, Option<usize>) {
    let mut r = Refusal::lq(d.code.as_str(), d.message.clone());
    if d.span.is_none()
        && let Some(i) = d
            .message
            .strip_prefix("statement ")
            .and_then(|m| m.split_once(": "))
            .and_then(|(i, _)| i.parse::<i64>().ok())
    {
        r = r.key("statement", crate::err::Kv::Int(i));
    }
    (r, d.span.map(|s| s.start as usize))
}

/// `H` of a named mutation run by name (R4, [LQ/canonical-ast §5.9]): `TX { CALL tx.<name>(…) }` with its parameters.
// spec: [LQ/canonical-ast §5.9] R4
pub fn h_of_mutation(
    name: &str,
    params: &Params,
    message: Option<&str>,
    schema: &LqSchema,
    ids: &dyn Identities,
    caller: &Caller,
) -> Result<[u8; 16], (Refusal, Option<usize>)> {
    let ctx = BindCtx {
        schema,
        ids,
        params,
        caller,
    };
    let options = crate::lq::ast::Tx {
        on: None,
        if_tip: None,
        if_targets: None,
        key: None,
        lease: None,
        // The call's `message` is the block's `MESSAGE`, inside `H` ([API §7.3]; [LQ/canonical-ast] C-2).
        message: message.map(str::to_string),
        stmts: Vec::new(),
        dry: false,
        span: crate::lq::diag::Span::default(),
    };
    let short = name.strip_prefix("tx.").unwrap_or(name);
    let bound =
        crate::lq::bind::bind_mutation(&ctx, short, &options).map_err(|d| refusal(&d[0]))?;
    let bytes = encode(Root::Tx(&bound.ast));
    let h = blake3::hash(&bytes);
    let mut out = [0u8; 16];
    out.copy_from_slice(&h.as_bytes()[..16]);
    Ok(out)
}

/// Parses and binds `TX { … }` text: its canonical AST with its lints, or the binder's first refusal.
pub fn bind_tx(
    text: &str,
    params: &Params,
    schema: &LqSchema,
    ids: &dyn Identities,
    caller: &Caller,
) -> Result<crate::lq::bind::Bound<crate::lq::cast::CTx>, (Refusal, Option<usize>)> {
    let parsed =
        parse_write(text, ParseOptions { strict_gql: false }).map_err(|d| refusal(&d[0]))?;
    let ctx = BindCtx {
        schema,
        ids,
        params,
        caller,
    };
    bind_write(&ctx, text, &parsed.tree).map_err(|d| refusal(&d[0]))
}

/// Parses and binds `TX { … }` text and returns `H` of its canonical AST ([LQ/canonical-ast §7.2]), or the binder's
/// first refusal.
// spec: [LQ/canonical-ast §7.2]
// spec: [API §7.3]
pub fn h_of_tx(
    text: &str,
    params: &Params,
    schema: &LqSchema,
    ids: &dyn Identities,
    caller: &Caller,
) -> Result<[u8; 16], (Refusal, Option<usize>)> {
    let parsed =
        parse_write(text, ParseOptions { strict_gql: false }).map_err(|d| refusal(&d[0]))?;
    let ctx = BindCtx {
        schema,
        ids,
        params,
        caller,
    };
    let bound = bind_write(&ctx, text, &parsed.tree).map_err(|d| refusal(&d[0]))?;
    let bytes = encode(Root::Tx(&bound.ast));
    let h = blake3::hash(&bytes);
    let mut out = [0u8; 16];
    out.copy_from_slice(&h.as_bytes()[..16]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lq::ctx::Value as P;

    #[test]
    fn a_data_level_block_binds_and_hashes() {
        let st = State::default();
        let uids = BTreeMap::new();
        let uidx = BTreeMap::new();
        let ids = ViewIds {
            uids: &uids,
            uidx: &uidx,
            st: &st,
            next_id: 1,
            commits: &[],
        };
        let schema = lq_schema(&st.schema);
        let caller = Caller::default();
        let params = Params::new()
            .with("p1", P::Text("Ship the Store API".into()))
            .with("p2", P::Text("P1".into()))
            .with("p3", P::Text("Write the chapter".into()));
        let text = "TX { CREATE (api:task {title: $p1, priority: $p2}); CREATE (ch:task {title: $p3}) UNDER api; MOVE ch UNDER api LAST }";
        let a = h_of_tx(text, &params, &schema, &ids, &caller).unwrap();
        // The same block with literals has the same H (parameter substitution, [LQ/canonical-ast §5.5]).
        let lit = "TX { CREATE (api:task {title: 'Ship the Store API', priority: 'P1'}); CREATE (ch:task {title: 'Write the chapter'}) UNDER api; MOVE ch UNDER api LAST }";
        let b = h_of_tx(lit, &Params::new(), &schema, &ids, &caller).unwrap();
        assert_eq!(a, b);
        let e = h_of_tx(
            "TX { CREATE (x:tsk {title: 'a'}) }",
            &Params::new(),
            &schema,
            &ids,
            &caller,
        )
        .unwrap_err()
        .0;
        assert_eq!((e.code.as_str(), e.exit), ("E105", 2));
    }
}
