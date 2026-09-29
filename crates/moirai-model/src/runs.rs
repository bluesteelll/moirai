//! The run-open policy of the dispatcher contract ([CFG §10.11] `runs.granularity`; [AR §13] "one `run` per Workflow
//! run (~11/day) vs one per agent call"; [90 §7.1]): which `RunOpen` commands a dispatched Workflow issues.

/// One agent call of a Workflow run, as the dispatcher sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    /// The call's id within the Workflow run.
    pub id: String,
}

/// The run names the dispatcher opens for a Workflow run `wf` with its agent calls ([CFG §10.11]): `workflow` opens one
/// run named `wf`; `agent-call` opens one run per call, named `<wf>/<call id>`, so each call's leases and results are
/// scoped to its own run. The names are the `name` arguments of `RunOpen` ([API §10.7]; unique per view).
// spec: [CFG §10.11] runs.granularity
pub fn open_policy(granularity: &str, wf: &str, calls: &[Call]) -> Vec<String> {
    match granularity {
        "agent-call" => calls.iter().map(|c| format!("{wf}/{}", c.id)).collect(),
        _ => vec![wf.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_run_per_workflow_or_per_call() {
        let calls = [Call { id: "a".into() }, Call { id: "b".into() }];
        assert_eq!(open_policy("workflow", "wf1", &calls), ["wf1"]);
        assert_eq!(open_policy("agent-call", "wf1", &calls), ["wf1/a", "wf1/b"]);
    }
}
