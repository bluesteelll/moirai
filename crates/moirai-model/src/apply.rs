//! `Apply` ([API §9.4]; [AR §6.4]; [90 §7.1], §7.2): one run's `result.v1` records and a data-level op batch,
//! committed as one `TX` block keyed once per run. Step 1 turns each result into a completion under the entry's own
//! lease or a release; step 2 creates the carried findings and notes, deduplicated in the batch; step 3 is the op
//! batch; step 4 releases every run-scoped lease the batch names (LE-003).

use crate::api::{After, Ctx, Data, Keying, Reply, Store};
use crate::err::{Refusal, Res};
use crate::lease;
use crate::lq::ctx::Value as P;
use crate::tx::{Stmt, Target, parse_lease};
use crate::value::{Nid, blake3_128};
use std::collections::{BTreeMap, BTreeSet};

/// A carried finding of a `result.v1` record ([90 §7.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindingV1 {
    /// `title`.
    pub title: String,
    /// `severity`.
    pub severity: String,
    /// `failure_scenario`.
    pub failure_scenario: String,
    /// `about`: `#N`s.
    pub about: Vec<u32>,
}

/// A carried note of a `result.v1` record ([90 §7.2]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteV1 {
    /// `kind`: the note's `note_kind`.
    pub kind: String,
    /// `title`.
    pub title: String,
    /// `text`: the body.
    pub text: String,
}

/// One `result.v1` record ([90 §7.2]); every self-reported identity field other than `lease` is ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultV1 {
    /// `task`.
    pub task: Option<u32>,
    /// `lease`: `L-<n>`.
    pub lease: String,
    /// `outcome`: `done`, `failed`, `abandoned` or `none`.
    pub outcome: String,
    /// `summary`.
    pub summary: String,
    /// `evidence`.
    pub evidence: Vec<String>,
    /// `recorded`: ids the worker wrote directly.
    pub recorded: Vec<u32>,
    /// `findings`.
    pub findings: Vec<FindingV1>,
    /// `notes`.
    pub notes: Vec<NoteV1>,
}

/// One entry of an `Apply` result ([API §9.4] `entries`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyEntry {
    /// `task`.
    pub task: Option<Nid>,
    /// `lease`.
    pub lease: String,
    /// `outcome`.
    pub outcome: String,
    /// `completed`: step 1 turned the entry into a `tx.complete`.
    pub completed: bool,
}

/// The data of an `Apply` result ([API §9.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyData {
    /// `run`.
    pub run: Option<String>,
    /// `entries`.
    pub entries: Vec<ApplyEntry>,
    /// `created`: the nodes steps 2 and 3 created, ascending.
    pub created: Vec<Nid>,
    /// `released`: every lease the batch ended, ascending.
    pub released: Vec<u64>,
    /// `recorded`: the entries' `recorded` ids that exist on the batch branch, ascending.
    pub recorded: Vec<Nid>,
    /// `affected`: the commit's.
    pub affected: Vec<Nid>,
}

/// The dedup key of a carried finding or note ([90 §7.2]):
/// `run:<run>/task:<n>/<kind>:<hex(BLAKE3-128(lp(title) ‖ lp(failure_scenario or text)))>`.
// spec: [90 §7.2]
fn carried_key(run: &str, task: Option<u32>, kind: &str, title: &str, second: &str) -> String {
    let mut buf = Vec::new();
    crate::value::lp(&mut buf, title.as_bytes());
    crate::value::lp(&mut buf, second.as_bytes());
    let h = blake3_128(&[&buf]);
    format!(
        "run:{run}/task:{}/{kind}:{}",
        task.map_or("null".to_string(), |t| t.to_string()),
        crate::value::hex(&h)
    )
}

impl Store {
    /// `Apply` ([API §9.4]). The batch branch is `ctx.branch`, else the branch of the run's lane (`run` → `runs_in` →
    /// lane → `moirai_branch`), else the resolved branch. Every entry's lease must be a lease of the run on the batch
    /// branch (E407, exit 5, before any write). The key is `run:<run>` unless `ctx.key` names another; the payload is
    /// `H` of the expansion.
    // spec: [API §9.4]
    // rule: LE-003
    pub fn apply(
        &mut self,
        run: Option<&str>,
        results: &[ResultV1],
        stmts: &[Stmt],
        message: &str,
        ctx: &Ctx,
    ) -> Res<Reply> {
        if !results.is_empty() && run.is_none() {
            return Err(Refusal::usage_arg("run", "results need their run"));
        }
        let caller = self.resolve(ctx, false)?;
        // The run node on the caller's view, and its lane's branch.
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let view = self.dag.state_at(tip, &self.alloc);
        let run_node = match run {
            Some(name) => Some(
                view.nodes
                    .iter()
                    .find(|(_, x)| x.live() && x.kind == "run" && x.text("title") == Some(name))
                    .map(|(n, _)| *n)
                    .ok_or_else(|| Refusal::not_found("run", name))?,
            ),
            None => None,
        };
        let lane_branch = run_node.and_then(|r| {
            let lane = view.nodes[&r]
                .out
                .keys()
                .find(|e| e.kind == "runs_in")
                .map(|e| e.dst)?;
            let main = self
                .dag
                .state_at(self.dag.live("main").and_then(|m| m.tip), &self.alloc);
            main.nodes
                .get(&lane)
                .or_else(|| view.nodes.get(&lane))
                .and_then(|x| x.text("moirai_branch").map(str::to_string))
        });
        let branch = ctx
            .branch
            .clone()
            .or(lane_branch)
            .unwrap_or_else(|| caller.branch.clone());
        drop(view);
        // Step 1's validation: every entry's lease belongs to the run and the batch branch (D6).
        let mut ids = Vec::new();
        for r in results {
            let id = parse_lease(&r.lease).ok_or_else(|| {
                Refusal::e407(
                    Some(r.lease.clone()),
                    None,
                    format!("{} is not a lease", r.lease),
                )
            })?;
            let l = self.leases.get(&id).ok_or_else(|| {
                Refusal::e407(
                    Some(r.lease.clone()),
                    None,
                    format!("{} is not a lease", r.lease),
                )
            })?;
            if l.run != run_node || l.branch != branch {
                return Err(Refusal::e407(
                    Some(r.lease.clone()),
                    Some(l.holder.clone()),
                    format!(
                        "{} belongs to another run or branch than the batch's ({branch})",
                        r.lease
                    ),
                ));
            }
            ids.push(id);
        }
        // The expansion, in order.
        let mut block = Vec::new();
        let mut entries = Vec::new();
        for (r, id) in results.iter().zip(&ids) {
            let task = r.task.map(Nid);
            let completed = matches!(r.outcome.as_str(), "done" | "failed" | "abandoned");
            if completed {
                let t = task.ok_or_else(|| {
                    Refusal::usage_arg("task", "a completed entry names its task")
                })?;
                let mut args = vec![
                    ("id".to_string(), P::Text(t.to_string())),
                    ("outcome".into(), P::Text(r.outcome.clone())),
                    ("summary".into(), P::Text(r.summary.clone())),
                ];
                if !r.evidence.is_empty() {
                    args.push((
                        "evidence".into(),
                        P::List(r.evidence.iter().map(|e| P::Text(e.clone())).collect()),
                    ));
                }
                args.push(("lease".into(), P::Text(r.lease.clone())));
                block.push(Stmt::Call {
                    proc: "tx.complete".into(),
                    args,
                });
            } else if r.outcome == "none" {
                // A live TTL lease is released here; a run-scoped one by step 4 ([API] open point 42).
                let l = &self.leases[id];
                if !l.run_scoped && l.ended.is_none() && lease::is_live(l, &self.env).is_live() {
                    block.push(Stmt::Call {
                        proc: "tx.release".into(),
                        args: vec![("lease".into(), P::Text(r.lease.clone()))],
                    });
                }
            } else {
                return Err(Refusal::usage_arg(
                    "outcome",
                    format!(
                        "outcome {} is not done, failed, abandoned or none",
                        r.outcome
                    ),
                ));
            }
            entries.push(ApplyEntry {
                task,
                lease: r.lease.clone(),
                outcome: r.outcome.clone(),
                completed,
            });
        }
        // Step 2: carried findings and notes, the first of each dedup key wins.
        let run_name = run.unwrap_or("");
        let mut seen = BTreeSet::new();
        for r in results {
            for f in &r.findings {
                let k = carried_key(run_name, r.task, "finding", &f.title, &f.failure_scenario);
                if !seen.insert(k) {
                    continue;
                }
                block.push(Stmt::Create {
                    name: None,
                    kind: "finding".into(),
                    fields: vec![
                        ("title".into(), P::Text(f.title.clone())),
                        ("severity".into(), P::Text(f.severity.clone())),
                        (
                            "failure_scenario".into(),
                            P::Text(f.failure_scenario.clone()),
                        ),
                    ],
                    body: None,
                    under: None,
                    position: None,
                    edges_out: f
                        .about
                        .iter()
                        .map(|a| ("about".to_string(), Target::Id(Nid(*a))))
                        .collect(),
                    edges_in: vec![],
                });
            }
            for n in &r.notes {
                let k = carried_key(run_name, r.task, "note", &n.title, &n.text);
                if !seen.insert(k) {
                    continue;
                }
                block.push(Stmt::Create {
                    name: None,
                    kind: "note".into(),
                    fields: vec![
                        ("title".into(), P::Text(n.title.clone())),
                        ("note_kind".into(), P::Text(n.kind.clone())),
                    ],
                    body: Some(n.text.clone()),
                    under: None,
                    position: None,
                    edges_out: vec![],
                    edges_in: vec![],
                });
            }
        }
        // Step 3.
        block.extend(stmts.iter().cloned());
        // Step 4's leases: the run-scoped leases the batch names.
        let release: Vec<u64> = ids
            .iter()
            .copied()
            .filter(|id| self.leases[id].run_scoped)
            .collect();
        let bctx = Ctx {
            branch: Some(branch.clone()),
            key: ctx.key.clone().or_else(|| run.map(|r| format!("run:{r}"))),
            ..ctx.clone()
        };
        let before: BTreeMap<u64, bool> = self
            .leases
            .iter()
            .map(|(id, l)| (*id, l.ended.is_none()))
            .collect();
        let next_before = self.next_id;
        let verb = if results.is_empty() {
            "apply-batch"
        } else {
            "apply-from"
        };
        let mut reply = self.tx(
            &block,
            message,
            &bctx,
            None,
            None,
            Keying::Block,
            After::Apply { verb, release },
        )?;
        let released: Vec<u64> = self
            .leases
            .iter()
            .filter(|(id, l)| before.get(id) == Some(&true) && l.ended.is_some())
            .map(|(id, _)| *id)
            .collect();
        let tip = self.dag.live(&branch).and_then(|r| r.tip);
        let st = self.dag.state_at(tip, &self.alloc);
        let mut recorded: Vec<Nid> = results
            .iter()
            .flat_map(|r| r.recorded.iter().map(|n| Nid(*n)))
            .filter(|n| st.live(*n).is_some())
            .collect();
        recorded.sort();
        recorded.dedup();
        let created: Vec<Nid> = (next_before..self.next_id).map(Nid).collect();
        let affected = reply.commit.map_or_else(Vec::new, |c| {
            self.dag.commits.get(&c).map_or_else(Vec::new, |x| {
                if reply.rev_new == Some(c) {
                    x.affected.clone()
                } else {
                    Vec::new()
                }
            })
        });
        reply.statements.clear();
        reply.data = Data::Apply(Box::new(ApplyData {
            run: run.map(str::to_string),
            entries,
            created,
            released,
            recorded,
            affected,
        }));
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn carried_keys_follow_90_7_2() {
        let a = carried_key("r1", Some(4), "finding", "t", "f");
        assert!(
            a.starts_with("run:r1/task:4/finding:")
                && a.len() == "run:r1/task:4/finding:".len() + 32
        );
        assert_ne!(a, carried_key("r1", Some(4), "finding", "t", "g"));
    }
}
