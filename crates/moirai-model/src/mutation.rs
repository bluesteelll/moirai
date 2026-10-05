//! Named mutations ([LQ/std §7.2]; [API §9.7]): each verb mutation expands to its `TX` template — the text whose `H`
//! is the payload of a CLI verb (R5, [API §7.3]) — and to the kernel's statements that run it. The two expansions this
//! chapter owns, `tx.retract` and `tx.answer`, are [API §9.7]'s. The coordination procedures (`tx.claim` …) are
//! [`crate::api`]'s group-C commands.

use crate::err::{Refusal, Res};
use crate::lq::ctx::Value as P;
use crate::schema::{Schema, Ty};
use crate::status::Door;
use crate::tx::{Guard, Stmt, Take, Target, parse_node};
use crate::value::Nid;

/// One expansion: the kernel's statements, the template text with its named parameters, and the door its status
/// changes come through when it is not the statement's own.
#[derive(Clone, Debug, PartialEq)]
pub struct Expansion {
    /// The kernel's statements.
    pub stmts: Vec<Stmt>,
    /// The `TX { … }` text of [LQ/std §7.2].
    pub text: String,
    /// The template's statements, each with the kernel statement index it renders.
    pub lq_stmts: Vec<(usize, String)>,
    /// `$name` → value.
    pub params: Vec<(String, P)>,
    /// The door of the status changes (`retract` for `tx.retract`).
    pub door: Option<Door>,
}

fn get<'a>(params: &'a [(String, P)], k: &str) -> Option<&'a P> {
    params
        .iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v)
        .filter(|v| **v != P::Null)
}

fn text(params: &[(String, P)], k: &str) -> Option<String> {
    match get(params, k) {
        Some(P::Text(s)) => Some(s.clone()),
        _ => None,
    }
}

fn need_text(name: &str, params: &[(String, P)], k: &str) -> Res<String> {
    text(params, k).ok_or_else(|| Refusal::lq("E110", format!("tx.{name} needs ${k}")))
}

fn node(p: &P) -> Option<Target> {
    match p {
        P::Text(s) => parse_node(s),
        P::Int(i) if *i > 0 && *i <= u32::MAX as i64 => Some(Target::Id(Nid(*i as u32))),
        _ => None,
    }
}

fn need_node(name: &str, params: &[(String, P)], k: &str) -> Res<Target> {
    get(params, k)
        .and_then(node)
        .ok_or_else(|| Refusal::lq("E110", format!("tx.{name} needs ${k}: node")))
}

fn nodes(params: &[(String, P)], k: &str) -> Res<Vec<Target>> {
    match get(params, k) {
        None => Ok(Vec::new()),
        Some(P::List(v)) => v
            .iter()
            .map(|x| node(x).ok_or_else(|| Refusal::lq("E110", format!("${k} takes nodes"))))
            .collect(),
        Some(other) => node(other)
            .map(|t| vec![t])
            .ok_or_else(|| Refusal::lq("E110", format!("${k} takes nodes"))),
    }
}

fn lit(t: &Target) -> String {
    match t {
        Target::Id(n) => format!("#{}", n.0),
        Target::Uid(u) => format!("#u:{}", u.hex()),
        Target::Var(v) => v.clone(),
    }
}

/// A `k=v` text converted by its field's type ([LQ/std §2.2]: the use site's type converts it).
fn kv_value(schema: &Schema, kind: &str, field: &str, v: &str) -> P {
    let ty = schema.field(kind, field).map(|f| f.ty);
    match ty {
        Some(Ty::Int) | Some(Ty::Counter) => {
            v.parse().map(P::Int).unwrap_or_else(|_| P::Text(v.into()))
        }
        Some(Ty::F64) => v
            .parse()
            .map(P::Float)
            .unwrap_or_else(|_| P::Text(v.into())),
        Some(Ty::Bool) => match v {
            "true" => P::Bool(true),
            "false" => P::Bool(false),
            _ => P::Text(v.into()),
        },
        Some(Ty::Set(_)) => P::List(v.split(',').map(|x| P::Text(x.trim().into())).collect()),
        _ => P::Text(v.into()),
    }
}

/// The `$fields` list of `k=v` texts as (field, value) pairs.
fn kv_fields(schema: &Schema, kind: &str, params: &[(String, P)]) -> Res<Vec<(String, P)>> {
    let Some(P::List(items)) = get(params, "fields") else {
        return Ok(Vec::new());
    };
    items
        .iter()
        .map(|x| match x {
            P::Text(s) => {
                let (k, v) = s
                    .split_once('=')
                    .ok_or_else(|| Refusal::lq("E110", format!("$fields takes k=v; got {s}")))?;
                Ok((k.to_string(), kv_value(schema, kind, k, v)))
            }
            other => Err(Refusal::lq(
                "E110",
                format!("$fields takes k=v texts; got {other:?}"),
            )),
        })
        .collect()
}

/// The title of `tx.answer`'s note: the first line of the text cut to 200 bytes at a scalar boundary, or `answer`
/// ([API §9.7]).
pub fn answer_title(t: &str) -> String {
    let line = t.lines().next().unwrap_or("");
    let mut end = line.len().min(200);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    let s = &line[..end];
    if s.trim().is_empty() {
        "answer".into()
    } else {
        s.to_string()
    }
}

/// Expands a verb mutation by name ([LQ/std §7.2]; [API §9.7]).
// spec: [LQ/std §7.2]
// spec: [API §9.7]
pub fn expand(
    schema: &Schema,
    name: &str,
    params: &[(String, P)],
    kind_of: &dyn Fn(&Target) -> Option<String>,
) -> Res<Expansion> {
    let short = name.strip_prefix("tx.").unwrap_or(name);
    let mut ps: Vec<(String, P)> = Vec::new();
    let mut lq: Vec<(usize, String)> = Vec::new();
    let mut stmts = Vec::new();
    let mut door = None;
    let bind = |ps: &mut Vec<(String, P)>, k: &str, v: P| -> String {
        ps.push((k.to_string(), v));
        format!("${k}")
    };
    match short {
        "add" | "remember" => {
            let kind = need_text(short, params, "kind")?;
            let title = need_text(short, params, "title")?;
            let mut fields = vec![("title".to_string(), P::Text(title.clone()))];
            fields.extend(kv_fields(schema, &kind, params)?);
            if short == "remember"
                && let Some(P::List(a)) = get(params, "applies_to")
            {
                fields.push(("applies_to".into(), P::List(a.clone())));
            }
            let props: Vec<String> = fields
                .iter()
                .map(|(f, v)| format!("{f}: {}", bind(&mut ps, f, v.clone())))
                .collect();
            let parent = if short == "add" {
                get(params, "parent").and_then(node)
            } else {
                None
            };
            let mut t = format!("CREATE (n:{kind} {{{}}})", props.join(", "));
            if let Some(p) = &parent {
                t.push_str(&format!(" UNDER {}", lit(p)));
            }
            // One kernel statement per statement of the template, so a refusal names the template's statement.
            let n = || Target::Var("n".into());
            let link = |src: Target, kind: &str, dst: Target| Stmt::Link {
                src,
                kind: kind.into(),
                dst,
                pinned: None,
            };
            let set_of = |fields: Vec<(String, P)>, body: Option<Option<String>>| Stmt::Set {
                target: Target::Var("n".into()),
                fields,
                incr: Vec::new(),
                body,
                guard: None,
            };
            lq.push((1, t));
            stmts.push(Stmt::Create {
                name: Some("n".into()),
                kind: kind.clone(),
                fields,
                body: None,
                under: parent,
                position: None,
                edges_out: Vec::new(),
                edges_in: Vec::new(),
            });
            if short == "add" {
                for b in nodes(params, "blocked_by")? {
                    lq.push((
                        stmts.len() + 1,
                        format!("CREATE ({})-[:BLOCKS]->(n)", lit(&b)),
                    ));
                    stmts.push(link(b, "blocks", n()));
                }
                for b in nodes(params, "blocks")? {
                    lq.push((
                        stmts.len() + 1,
                        format!("CREATE (n)-[:BLOCKS]->({})", lit(&b)),
                    ));
                    stmts.push(link(n(), "blocks", b));
                }
                if let Some(b) = text(params, "body") {
                    let p = bind(&mut ps, "body", P::Text(b.clone()));
                    lq.push((stmts.len() + 1, format!("SET n.body = {p}")));
                    stmts.push(set_of(Vec::new(), Some(Some(b))));
                }
            } else {
                let tx = need_text(short, params, "text")?;
                let field = if kind == "rule" { "text" } else { "body" };
                let p = bind(&mut ps, "text", P::Text(tx.clone()));
                lq.push((stmts.len() + 1, format!("SET n.{field} = {p}")));
                stmts.push(if field == "body" {
                    set_of(Vec::new(), Some(Some(tx)))
                } else {
                    set_of(vec![("text".to_string(), P::Text(tx))], None)
                });
                let about = nodes(params, "about")?;
                for x in &about {
                    lq.push((
                        stmts.len() + 1,
                        format!("CREATE (n)-[:ABOUT]->({})", lit(x)),
                    ));
                    stmts.push(link(n(), "about", x.clone()));
                }
                // [LQ/std §7.2] tx.remember: "a verdict's DERIVED_FROM edges are written" ([50 §4.2]; [AR §2.12]
                // N14 "`verdict derived_from finding` edges are written by `remember{verdict}` from its inputs"):
                // one edge to each finding the verdict is about.
                if kind == "verdict" {
                    for x in about
                        .iter()
                        .filter(|x| kind_of(x).as_deref() == Some("finding"))
                    {
                        lq.push((
                            stmts.len() + 1,
                            format!("CREATE (n)-[:DERIVED_FROM]->({})", lit(x)),
                        ));
                        stmts.push(link(n(), "derived_from", x.clone()));
                    }
                }
            }
        }
        "set" => {
            let id = need_node(short, params, "id")?;
            let kind = kind_of(&id).unwrap_or_default();
            let mut fields = kv_fields(schema, &kind, params)?;
            if let Some(s) = text(params, "status") {
                fields.push(("status".into(), P::Text(s)));
            }
            if let Some(P::Bool(true)) = get(params, "done") {
                fields.push(("done".into(), P::Bool(true)));
            }
            if let Some(r) = text(params, "resolution") {
                fields.push(("resolution".into(), P::Text(r)));
            }
            let guard = Guard {
                if_rev: match get(params, "if_rev") {
                    Some(P::Int(r)) => Some(*r as u64),
                    _ => None,
                },
                if_status: text(params, "if_status"),
                if_holder: text(params, "if_holder"),
            };
            let guarded = guard != Guard::default();
            let tgt = if guarded { "n".to_string() } else { lit(&id) };
            let sets: Vec<String> = fields
                .iter()
                .map(|(f, v)| {
                    if f == "done" {
                        format!("{tgt}.done = true")
                    } else {
                        format!("{tgt}.{f} = {}", bind(&mut ps, f, v.clone()))
                    }
                })
                .collect();
            let t = if guarded {
                let mut conds = Vec::new();
                if let Some(r) = guard.if_rev {
                    conds.push(format!(
                        "n.rev = {}",
                        bind(&mut ps, "if_rev", P::Int(r as i64))
                    ));
                }
                if let Some(s) = &guard.if_status {
                    conds.push(format!(
                        "n.status = {}",
                        bind(&mut ps, "if_status", P::Text(s.clone()))
                    ));
                }
                if let Some(h) = &guard.if_holder {
                    conds.push(format!(
                        "n.lease.holder = {}",
                        bind(&mut ps, "if_holder", P::Text(h.clone()))
                    ));
                }
                format!(
                    "MATCH (n {{id: {}}}) WHERE {} EXPECT 1 SET {}",
                    lit(&id),
                    conds.join(" AND "),
                    sets.join(", ")
                )
            } else {
                format!("SET {}", sets.join(", "))
            };
            lq.push((1, t));
            stmts.push(Stmt::Set {
                target: id,
                fields,
                incr: Vec::new(),
                body: None,
                guard: guarded.then_some(guard),
            });
        }
        "link" => {
            let (a, b) = (
                need_node(short, params, "a")?,
                need_node(short, params, "b")?,
            );
            let kind = need_text(short, params, "kind")?;
            if kind == "parent" {
                lq.push((1, format!("MOVE {} UNDER {}", lit(&a), lit(&b))));
                stmts.push(Stmt::Move {
                    target: a,
                    under: Some(b),
                    position: None,
                });
            } else {
                let lqn = schema
                    .edge(&kind)
                    .map_or_else(|| kind.to_uppercase(), |e| e.lq_name.clone());
                let pinned = text(params, "pinned");
                let props = match &pinned {
                    Some(c) => format!(
                        " {{pinned: {}}}",
                        bind(&mut ps, "pinned", P::Text(c.clone()))
                    ),
                    None => String::new(),
                };
                lq.push((
                    1,
                    format!("CREATE ({})-[:{lqn}{props}]->({})", lit(&a), lit(&b)),
                ));
                stmts.push(Stmt::Link {
                    src: a,
                    kind,
                    dst: b,
                    pinned,
                });
            }
        }
        "unlink" => {
            let (a, b) = (
                need_node(short, params, "a")?,
                need_node(short, params, "b")?,
            );
            let kind = need_text(short, params, "kind")?;
            let lqn = schema
                .edge(&kind)
                .map_or_else(|| kind.to_uppercase(), |e| e.lq_name.clone());
            lq.push((
                1,
                format!(
                    "MATCH ({})-[e:{lqn}]->({}) EXPECT 1 DELETE e",
                    lit(&a),
                    lit(&b)
                ),
            ));
            stmts.push(Stmt::Unlink {
                src: a,
                kind,
                dst: b,
            });
        }
        "move" => {
            let (id, parent) = (
                need_node(short, params, "id")?,
                need_node(short, params, "parent")?,
            );
            lq.push((1, format!("MOVE {} UNDER {}", lit(&id), lit(&parent))));
            stmts.push(Stmt::Move {
                target: id,
                under: Some(parent),
                position: None,
            });
        }
        "reopen" => {
            let id = need_node(short, params, "id")?;
            let reason = need_text(short, params, "reason")?;
            lq.push((
                1,
                format!(
                    "REOPEN {} REASON {}",
                    lit(&id),
                    bind(&mut ps, "reason", P::Text(reason.clone()))
                ),
            ));
            stmts.push(Stmt::Reopen { target: id, reason });
        }
        "supersede" => {
            let (old, new) = (
                need_node(short, params, "old")?,
                need_node(short, params, "new")?,
            );
            lq.push((
                1,
                format!("CREATE ({})-[:SUPERSEDES]->({})", lit(&new), lit(&old)),
            ));
            stmts.push(Stmt::Link {
                src: new,
                kind: "supersedes".into(),
                dst: old,
                pinned: None,
            });
        }
        "doc_patch" => {
            let section = need_node(short, params, "section")?;
            let (old, new) = (
                need_text(short, params, "old")?,
                need_text(short, params, "new")?,
            );
            let (a, b) = (
                bind(&mut ps, "old", P::Text(old.clone())),
                bind(&mut ps, "new", P::Text(new.clone())),
            );
            lq.push((
                1,
                format!("PATCH {}.body REMOVE {a} ADD {b}", lit(&section)),
            ));
            stmts.push(Stmt::Patch {
                target: section.clone(),
                remove: old,
                add: new,
            });
            for s in nodes(params, "depends_on")? {
                lq.push((
                    stmts.len() + 1,
                    format!("CREATE ({})-[:DEPENDS_ON]->({})", lit(&section), lit(&s)),
                ));
                stmts.push(Stmt::Link {
                    src: section.clone(),
                    kind: "depends_on".into(),
                    dst: s,
                    pinned: None,
                });
            }
        }
        "rm" => {
            let id = need_node(short, params, "id")?;
            // [LQ/std §7.2] `tx.rm`: `--cascade|--reparent|--reassign` (spec sync 2b S2B-F-57); `restrict` is the
            // default and renders no `POLICY`.
            let policy = text(params, "policy").filter(|p| p != "restrict");
            if let Some(p) = &policy
                && !matches!(p.as_str(), "cascade" | "reparent" | "reassign")
            {
                return Err(Refusal::lq(
                    "E110",
                    format!("policy {p} is not restrict, cascade, reparent or reassign"),
                ));
            }
            let replaced_by = get(params, "replaced_by").and_then(node);
            let release = matches!(get(params, "release"), Some(P::Bool(true)));
            let reason = text(params, "reason");
            let mut t = format!("DELETE {}", lit(&id));
            if let Some(p) = &policy {
                t.push_str(&format!(" POLICY {}", p.to_uppercase()));
            }
            if let Some(y) = &replaced_by {
                t.push_str(&format!(" REPLACED BY {}", lit(y)));
            }
            if release {
                t.push_str(" RELEASE");
            }
            if let Some(r) = &reason {
                t.push_str(&format!(
                    " REASON {}",
                    bind(&mut ps, "reason", P::Text(r.clone()))
                ));
            }
            lq.push((1, t));
            stmts.push(Stmt::Delete {
                target: id,
                policy,
                replaced_by,
                release,
                reason,
            });
        }
        "resolve" => {
            let key = need_text(short, params, "key")?;
            let take = need_text(short, params, "take")?;
            let (take, word) = match take.as_str() {
                "ours" => (Take::Ours, "OURS".to_string()),
                "theirs" => (Take::Theirs, "THEIRS".to_string()),
                "base" => (Take::Base, "BASE".to_string()),
                "drop" => (Take::Drop, String::new()),
                "value" => {
                    let v = get(params, "value")
                        .cloned()
                        .ok_or_else(|| Refusal::lq("E110", "take value needs $value"))?;
                    let p = bind(&mut ps, "value", v.clone());
                    (Take::Value(v), format!("VALUE {p}"))
                }
                r => {
                    let id = r
                        .strip_prefix("repoint:")
                        .and_then(|x| parse_node(&format!("#{}", x.trim_start_matches('#'))))
                        .ok_or_else(|| {
                            Refusal::lq(
                                "E110",
                                format!(
                                    "take {r} is not ours, theirs, base, value, drop or repoint:ID"
                                ),
                            )
                        })?;
                    let w = format!("REPOINT {}", lit(&id));
                    (Take::Repoint(id), w)
                }
            };
            let quoted = format!("'{}'", key.replace('\\', "\\\\").replace('\'', "\\'"));
            // `--take drop` renders `DROP` ([LQ/grammar-v1.ebnf] `resolve_stmt`; [LQ/std §7.2] `tx.resolve`).
            lq.push((
                1,
                if take == Take::Drop {
                    format!("RESOLVE {quoted} DROP")
                } else {
                    format!("RESOLVE {quoted} TAKE {word}")
                },
            ));
            stmts.push(Stmt::Resolve { key, take });
        }
        "retract" => {
            let id = need_node(short, params, "id")?;
            let reason = need_text(short, params, "reason")?;
            let r = bind(&mut ps, "reason", P::Text(reason.clone()));
            lq.push((
                1,
                format!("SET {0}.status = 'retracted', {0}.reason = {r}", lit(&id)),
            ));
            stmts.push(Stmt::Set {
                target: id,
                fields: vec![
                    ("reason".into(), P::Text(reason)),
                    ("status".into(), P::Text("retracted".into())),
                ],
                incr: Vec::new(),
                body: None,
                guard: None,
            });
            door = Some(Door::Retract);
        }
        "answer" => {
            let q = need_node(short, params, "q")?;
            let t = need_text(short, params, "text")?;
            let by = text(params, "by").unwrap_or_else(|| "owner".into());
            let title = answer_title(&t);
            let (pt, pb, px) = (
                bind(&mut ps, "title", P::Text(title.clone())),
                bind(&mut ps, "by", P::Text(by.clone())),
                bind(&mut ps, "text", P::Text(t.clone())),
            );
            lq.push((
                1,
                format!("CREATE (a:note {{title: {pt}, authority: {pb}}})"),
            ));
            lq.push((2, format!("SET a.body = {px}")));
            lq.push((3, format!("CREATE (a)-[:ANSWERS]->({})", lit(&q))));
            lq.push((
                4,
                format!("SET {0}.answer = {px}, {0}.status = 'answered'", lit(&q)),
            ));
            stmts.push(Stmt::Create {
                name: Some("a".into()),
                kind: "note".into(),
                fields: vec![
                    ("title".into(), P::Text(title)),
                    ("authority".into(), P::Text(by)),
                ],
                body: None,
                under: None,
                position: None,
                edges_out: Vec::new(),
                edges_in: Vec::new(),
            });
            stmts.push(Stmt::Set {
                target: Target::Var("a".into()),
                fields: Vec::new(),
                incr: Vec::new(),
                body: Some(Some(t.clone())),
                guard: None,
            });
            stmts.push(Stmt::Link {
                src: Target::Var("a".into()),
                kind: "answers".into(),
                dst: q.clone(),
                pinned: None,
            });
            stmts.push(Stmt::Set {
                target: q,
                fields: vec![
                    ("answer".into(), P::Text(t)),
                    ("status".into(), P::Text("answered".into())),
                ],
                incr: Vec::new(),
                body: None,
                guard: None,
            });
        }
        other => {
            return Err(Refusal::lq(
                "E109",
                format!("unknown named mutation tx.{other}"),
            ));
        }
    }
    let text = format!(
        "TX {{ {} }}",
        lq.iter()
            .map(|(_, s)| s.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    );
    Ok(Expansion {
        stmts,
        text,
        lq_stmts: lq,
        params: ps,
        door,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_render_with_named_parameters() {
        let s = Schema::default();
        let kind = |_: &Target| Some("task".to_string());
        let e = expand(
            &s,
            "tx.set",
            &[
                ("id".into(), P::Text("#12".into())),
                (
                    "fields".into(),
                    P::List(vec![P::Text("priority=P1".into())]),
                ),
                ("if_status".into(), P::Text("open".into())),
            ],
            &kind,
        )
        .unwrap();
        assert_eq!(
            e.text,
            "TX { MATCH (n {id: #12}) WHERE n.status = $if_status EXPECT 1 SET n.priority = $priority }"
        );
        let e = expand(
            &s,
            "tx.answer",
            &[
                ("q".into(), P::Int(5)),
                ("text".into(), P::Text("Yes.\nMore".into())),
            ],
            &kind,
        )
        .unwrap();
        assert_eq!(
            e.text,
            "TX { CREATE (a:note {title: $title, authority: $by}); SET a.body = $text; CREATE (a)-[:ANSWERS]->(#5); SET #5.answer = $text, #5.status = 'answered' }"
        );
        assert_eq!(answer_title("\n"), "answer");
        assert_eq!(answer_title(&"é".repeat(150)).len(), 200);
        assert_eq!(expand(&s, "tx.nope", &[], &kind).unwrap_err().code, "E109");
    }

    /// [LQ/std §7.2] `tx.rm` (spec sync 2b S2B-F-57): `--reassign` renders `POLICY REASSIGN`; `restrict` renders no
    /// `POLICY`; any other policy word is E110.
    #[test]
    fn rm_takes_reassign() {
        let s = Schema::default();
        let kind = |_: &Target| Some("area".to_string());
        let rm = |policy: &str| {
            expand(
                &s,
                "tx.rm",
                &[
                    ("id".into(), P::Text("#7".into())),
                    ("policy".into(), P::Text(policy.into())),
                ],
                &kind,
            )
        };
        let e = rm("reassign").unwrap();
        assert_eq!(e.text, "TX { DELETE #7 POLICY REASSIGN }");
        assert!(matches!(
            &e.stmts[..],
            [Stmt::Delete { policy: Some(p), .. }] if p == "reassign"
        ));
        assert_eq!(rm("restrict").unwrap().text, "TX { DELETE #7 }");
        assert_eq!(rm("sideways").unwrap_err().code, "E110");
    }
}
