//! Status machines as data ([RULES/status-machines]; [AR §3.6]; I8): the transitions a direct write may make and the
//! door it comes through (`transitions`, `doors`), the guards evaluated on the candidate (`guards`,
//! `transition-guards`), the views that accept status writes (`branch-mask`), the general rules and what `complete`
//! writes per outcome (`complete-outcomes`).

use crate::derived::{self, Index};
use crate::err::{Refusal, Res};
use crate::rules::{Row, rules};
use crate::state::{Aspect, EdgeKey, State};
use crate::value::{Nid, Value};

/// A door: the family of statements and verbs a status change comes through ([RULES/status-machines] `doors`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Door {
    /// DR-001.
    SetStatus,
    /// DR-002.
    TxComplete,
    /// DR-003.
    ClaimStart,
    /// DR-004.
    LeaseFirstWrite,
    /// DR-005.
    Reopen,
    /// DR-006.
    Supersede,
    /// DR-007.
    Retract,
    /// DR-008.
    Settle,
    /// DR-009.
    FileRm,
    /// DR-010.
    LinksFix,
    /// DR-011.
    DeletionInference,
    /// DR-012.
    DeletePolicy,
}

impl Door {
    /// The door's token in `transitions` and `doors`.
    pub fn token(self) -> &'static str {
        match self {
            Door::SetStatus => "set-status",
            Door::TxComplete => "tx-complete",
            Door::ClaimStart => "claim-start",
            Door::LeaseFirstWrite => "lease-first-write",
            Door::Reopen => "reopen",
            Door::Supersede => "supersede",
            Door::Retract => "retract",
            Door::Settle => "settle",
            Door::FileRm => "file-rm",
            Door::LinksFix => "links-fix",
            Door::DeletionInference => "deletion-inference",
            Door::DeletePolicy => "delete-policy",
        }
    }
}

/// History facts a guard reads that the state does not hold.
pub trait History {
    /// The actor of the commit that added the edge key (src, key) to the view, when it is known.
    fn edge_actor(&self, src: Nid, key: &EdgeKey) -> Option<String>;
    /// The wall-clock milliseconds of the newest commit that changed a node's aspect on the view.
    fn changed_ms(&self, n: Nid, aspect: &Aspect) -> Option<u64>;
}

/// The statuses of a kind with their `initial` and `done` cells ([RULES/status-machines] `statuses`; [60 §2.5]
/// "Schema as data": each kind's status set and its initial values).
// spec: [RULES/status-machines] statuses
pub fn statuses(kind: &str) -> Vec<(&'static str, bool, &'static str)> {
    rules()
        .table("statuses")
        .rows
        .iter()
        .filter(|r| r.tok("kind") == kind)
        .map(|r| (r.tok("status"), r.tok("initial") == "yes", r.tok("done")))
        .collect()
}

/// GR-001: the `transitions` row for (kind, from, to, door), or E404 naming the door that exists.
// spec: [RULES/status-machines] transitions
// rule: GR-001
pub fn transition(kind: &str, from: &str, to: &str, door: Door) -> Res<&'static Row> {
    let t = rules().table("transitions");
    if let Some(r) = t.rows.iter().find(|r| {
        r.tok("kind") == kind
            && r.tok("from") == from
            && r.tok("to") == to
            && r.tok("door") == door.token()
    }) {
        return Ok(r);
    }
    let other: Vec<&str> = t
        .rows
        .iter()
        .filter(|r| r.tok("kind") == kind && r.tok("from") == from && r.tok("to") == to)
        .map(|r| r.tok("door"))
        .collect();
    Err(Refusal::lq(
        "E404",
        if other.is_empty() {
            format!("{kind}: no transition {from} -> {to}")
        } else {
            format!("{kind}: {from} -> {to} goes through {}", other.join(", "))
        },
    ))
}

/// The guards of a transition (`transition-guards`), in row order.
// spec: [RULES/status-machines] transition-guards
pub fn guards(kind: &str, from: &str, to: &str) -> Vec<&'static str> {
    rules()
        .table("transition-guards")
        .rows
        .iter()
        .filter(|r| r.tok("kind") == kind && r.tok("from") == from && r.tok("to") == to)
        .map(|r| r.tok("guard"))
        .collect()
}

/// GD-001: no live child of the target is a task outside {`done`, `cancelled`}.
// rule: GD-001
pub fn gd001_no_unfinished_child(ix: &Index<'_>, n: Nid) -> Res<()> {
    let open: Vec<String> = ix
        .children
        .get(&n)
        .into_iter()
        .flatten()
        .filter(|c| {
            ix.st
                .live(**c)
                .is_some_and(|x| x.kind == "task" && x.status != "done" && x.status != "cancelled")
        })
        .map(|c| c.to_string())
        .collect();
    if open.is_empty() {
        Ok(())
    } else {
        Err(Refusal::lq(
            "E404",
            format!("{n} has unfinished children: {}", open.join(", ")),
        ))
    }
}

/// GD-002: no `gates` in-edge from a live gating verdict, and none flagged.
// rule: GD-002
// rule: FL-002
pub fn gd002_not_gated(ix: &Index<'_>, n: Nid) -> Res<()> {
    for (src, _, p) in ix.in_edges(n, "gates") {
        if p.flagged || derived::gating_verdict(ix, src) {
            return Err(Refusal::lq(
                "E404",
                format!("{n} is gated by verdict {src}"),
            ));
        }
    }
    Ok(())
}

/// GD-003: a live `answers` in-edge of the question from a live `decision` or `note`.
// rule: GD-003
pub fn gd003_answers_edge(ix: &Index<'_>, n: Nid) -> Res<()> {
    if derived::answered(ix, n) == Some(true) {
        Ok(())
    } else {
        Err(Refusal::lq("E404", format!("{n} has no answers edge")))
    }
}

/// GD-004: the question's `answer` is non-empty.
// rule: GD-004
pub fn gd004_answer_text(st: &State, n: Nid) -> Res<()> {
    if st.live(n).is_some_and(|x| x.text("answer").is_some()) {
        Ok(())
    } else {
        Err(Refusal::lq("E404", format!("{n} has no answer")))
    }
}

/// GD-005 (I13): a `perf` or `complexity` finding reaches `fixed` only with a live `verifies` in-edge from a live
/// review verdict (`role` `code-reviewer` or `architecture-critic`) and a live `addresses` in-edge added by an actor
/// other than that verdict's creator.
// rule: GD-005
// spec: [F13 §3.3] I13
pub fn gd005_i13_review(ix: &Index<'_>, n: Nid, h: &dyn History) -> Res<()> {
    let Some(f) = ix.st.live(n) else {
        return Ok(());
    };
    if !matches!(f.fields.get("f_kind"), Some(Value::Enum(k)) if k == "perf" || k == "complexity") {
        return Ok(());
    }
    let reviewers: Vec<String> = ix
        .in_edges(n, "verifies")
        .filter_map(|(v, _, _)| ix.st.live(v))
        .filter(|v| {
            v.kind == "verdict"
                && matches!(v.fields.get("role"), Some(Value::Text(r)) if r == "code-reviewer" || r == "architecture-critic")
        })
        .map(|v| v.creator.actor.clone())
        .collect();
    let ok = reviewers.iter().any(|rev| {
        ix.in_edges(n, "addresses").any(|(src, k, _)| {
            ix.st.live(src).is_some() && h.edge_actor(src, k).is_some_and(|a| &a != rev)
        })
    });
    if ok {
        Ok(())
    } else {
        Err(Refusal::lq(
            "E404",
            format!("{n} needs a review verdict and an addresses edge by another actor (I13)"),
        ))
    }
}

/// GD-006 (I14): every `expected_artifacts` symbol names an artifact the run has a live `produced` edge to, whose
/// `oid` was observed after the run started. The symbol names the artifact by its path text.
// rule: GD-006
// spec: [F13 §3.3] I14
pub fn gd006_i14_artifacts(st: &State, n: Nid, h: &dyn History) -> Res<()> {
    let Some(run) = st.live(n) else {
        return Ok(());
    };
    let started_ms = match run.fields.get("started") {
        Some(Value::Int(s)) => (*s).max(0) as u64 * 1000,
        _ => 0,
    };
    let expected: Vec<String> = match run.fields.get("expected_artifacts") {
        Some(Value::Set(v)) => v
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    };
    for sym in expected {
        let found = run.out.keys().filter(|k| k.kind == "produced").any(|k| {
            st.live(k.dst).is_some_and(|a| {
                a.kind == "artifact"
                    && matches!(a.fields.get("path"), Some(Value::Path(p)) if p.text == sym)
                    && a.fields.contains_key("oid")
                    && h.changed_ms(k.dst, &Aspect::Observation)
                        .is_some_and(|t| t >= started_ms)
            })
        });
        if !found {
            return Err(Refusal::lq(
                "E404",
                format!("{n}: expected artifact {sym} was not produced and read back (I14)"),
            ));
        }
    }
    Ok(())
}

/// Runs the guards of (kind, from, to) on the candidate, in row order; GD-007 is the settle's skip, never a refusal.
// spec: [RULES/status-machines] guards
pub fn check_guards(
    st: &State,
    n: Nid,
    kind: &str,
    from: &str,
    to: &str,
    h: &dyn History,
) -> Res<()> {
    let ix = Index::new(st);
    for g in guards(kind, from, to) {
        match g {
            "no-unfinished-child" => gd001_no_unfinished_child(&ix, n)?,
            "not-gated" => gd002_not_gated(&ix, n)?,
            "answers-edge" => gd003_answers_edge(&ix, n)?,
            "answer-text" => gd004_answer_text(st, n)?,
            "i13-review" => gd005_i13_review(&ix, n, h)?,
            "i14-artifacts" => gd006_i14_artifacts(st, n, h)?,
            "planned-bind" => {}
            other => panic!("guard {other} has no implementation"),
        }
    }
    Ok(())
}

/// The `branch-mask` row of a view kind (`work`, `plan`, `merge`, `import`, `tag`, `past-view`): status writes are
/// allowed, or refused with its code (BM rows; I33′).
// spec: [RULES/status-machines] branch-mask
// rule: BM-001, BM-002, BM-003, BM-004, BM-005, BM-006
pub fn branch_mask(view: &str) -> Res<()> {
    let r = rules()
        .table("branch-mask")
        .rows
        .iter()
        .find(|r| r.tok("view") == view)
        .unwrap_or_else(|| panic!("branch-mask has no row for view {view}"));
    if r.tok("status_writes") == "yes" {
        Ok(())
    } else {
        Err(Refusal::lq(
            r.tok("refusal"),
            format!("status writes are read-only on a {view} view"),
        ))
    }
}

/// GR-009: the status `SET x.done = true` writes: `done` (task), `answered` (question), `accepted` (verdict); E115 on a
/// kind whose `done` is absent.
// rule: GR-009
pub fn done_status(kind: &str) -> Res<&'static str> {
    match kind {
        "task" => Ok("done"),
        "question" => Ok("answered"),
        "verdict" => Ok("accepted"),
        _ => Err(Refusal::lq("E115", format!("{kind} has no done"))),
    }
}

/// GR-010: `SET x.done = false` is refused, naming `REOPEN`.
// rule: GR-010
pub fn done_false() -> Refusal {
    Refusal::lq("E404", "done = false is written with REOPEN")
}

/// GR-012: an artifact's status is never `SET`; E115 naming `moirai file rm` or `links fix --drop`.
// rule: GR-012
pub fn artifact_set() -> Refusal {
    Refusal::lq(
        "E115",
        "an artifact's status changes through moirai file rm or links fix --drop",
    )
}

/// GR-014: `resolution` is written only with a transition into a status whose `done` is `yes` (task) or out of `open`
/// (finding); a transition to `open` clears it; elsewhere it stays `none`.
// rule: GR-014
pub fn resolution_ok(kind: &str, from: &str, to: &str, resolution: &str) -> Res<()> {
    let st_done = rules()
        .table("statuses")
        .rows
        .iter()
        .any(|r| r.tok("kind") == kind && r.tok("status") == to && r.tok("done") == "yes");
    let allowed = match kind {
        "task" => st_done,
        "finding" => from == "open" && to != "open",
        _ => false,
    };
    if resolution == "none" || allowed {
        Ok(())
    } else {
        Err(Refusal::lq(
            "E404",
            format!("{kind}: resolution {resolution} does not go with {from} -> {to}"),
        ))
    }
}

/// `complete-outcomes`: the status and resolution `complete` writes for an outcome (CO rows; [API §10.5] step 1).
// spec: [RULES/status-machines] complete-outcomes
// rule: CO-001, CO-002, CO-003
pub fn complete_outcome(outcome: &str) -> Res<(&'static str, &'static str)> {
    let r = rules()
        .table("complete-outcomes")
        .rows
        .iter()
        .find(|r| r.tok("outcome") == outcome)
        .ok_or_else(|| {
            Refusal::usage_arg(
                "outcome",
                format!("outcome {outcome} is not done, failed or abandoned"),
            )
        })?;
    let resolution = match outcome {
        "done" => "completed",
        "failed" => "rework",
        _ => "wontdo",
    };
    Ok((r.tok("status"), resolution))
}

/// GR-006: the statuses a `Create` may name, as a path of `transitions` rows from the kind's initial status; the path
/// with the fewest steps, each (from, to, door) of it returned for its guards and role grant.
// rule: GR-006
pub fn create_path(kind: &str, initial: &str, to: &str) -> Option<Vec<&'static Row>> {
    if initial == to {
        return Some(Vec::new());
    }
    let t = rules().table("transitions");
    let mut frontier: Vec<(String, Vec<&'static Row>)> = vec![(initial.to_string(), Vec::new())];
    let mut seen = vec![initial.to_string()];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (s, path) in &frontier {
            for r in t
                .rows
                .iter()
                .filter(|r| r.tok("kind") == kind && r.tok("from") == s)
            {
                let d = r.tok("to").to_string();
                let mut p = path.clone();
                p.push(r);
                if d == to {
                    return Some(p);
                }
                if !seen.contains(&d) {
                    seen.push(d.clone());
                    next.push((d, p));
                }
            }
        }
        frontier = next;
    }
    None
}

/// I8 on a state: every node's status is one of its kind's statuses ([RULES/status-machines] GR-008 for merges and
/// history verbs).
// spec: [F13 §3.3] I8
// rule: GR-008
pub fn i8_status_machine(st: &State) -> Result<(), String> {
    for (n, x) in &st.nodes {
        if st.schema.value(&x.kind, "status", &x.status).is_none() {
            return Err(format!("{n}: {} is not a status of {}", x.status, x.kind));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transitions_are_data() {
        assert_eq!(
            transition("task", "open", "in_progress", Door::SetStatus)
                .unwrap()
                .id,
            "TR-001"
        );
        let e = transition("task", "done", "open", Door::SetStatus).unwrap_err();
        assert_eq!(e.code, "E404");
        assert!(e.detail.contains("reopen"), "{e}");
        assert_eq!(
            guards("task", "open", "done"),
            vec!["no-unfinished-child", "not-gated"]
        );
        assert!(branch_mask("work").is_ok());
        assert_eq!(branch_mask("plan").unwrap_err().code, "E305");
        assert_eq!(complete_outcome("failed").unwrap(), ("done", "rework"));
        assert!(resolution_ok("task", "in_progress", "done", "completed").is_ok());
        assert!(resolution_ok("task", "open", "in_progress", "completed").is_err());
        assert_eq!(
            create_path("decision", "proposed", "accepted").map(|p| p.len()),
            Some(1)
        );
        assert!(create_path("task", "open", "cancelled").is_some());
    }

    /// GR-017, second part: the transitive closure of the `move = up` transitions equals the SL order.
    #[test]
    // rule: GR-017
    fn up_moves_equal_the_lattice_order() {
        let r = rules();
        let covers: Vec<(String, String, String)> = r
            .table("status-lattice")
            .rows
            .iter()
            .flat_map(|x| {
                x.toks("covers")
                    .into_iter()
                    .filter(|c| *c != "-")
                    .map(|c| {
                        (
                            x.tok("kind").to_string(),
                            c.to_string(),
                            x.tok("status").to_string(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let ups: Vec<(String, String, String)> = r
            .table("transitions")
            .rows
            .iter()
            .filter(|x| x.tok("move") == "up")
            .map(|x| {
                (
                    x.tok("kind").to_string(),
                    x.tok("from").to_string(),
                    x.tok("to").to_string(),
                )
            })
            .collect();
        let closure = |edges: &[(String, String, String)]| {
            let mut c: std::collections::BTreeSet<(String, String, String)> =
                edges.iter().cloned().collect();
            loop {
                let mut add = Vec::new();
                for (k, a, b) in &c {
                    for (k2, b2, d) in &c {
                        if k == k2 && b == b2 && !c.contains(&(k.clone(), a.clone(), d.clone())) {
                            add.push((k.clone(), a.clone(), d.clone()));
                        }
                    }
                }
                if add.is_empty() {
                    return c;
                }
                c.extend(add);
            }
        };
        let lat = closure(&covers);
        let up = closure(&ups);
        assert_eq!(up, lat);
    }
}
