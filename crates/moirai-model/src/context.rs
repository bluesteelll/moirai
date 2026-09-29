//! The caller context's field groups that read the store's heads and runs ([API §4.2]; [90 §4.1] the order of record):
//! the branch order CX-2 as data, the tree CX-5 and the model CX-6. [`crate::api::Store::resolve`] runs them with the
//! other rows (CX-1, CX-3, CX-4, CX-7 to CX-9).

use crate::heads::{HeadKind, Heads, Target};

/// One source of CX-2, in the order of record ([API §4.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BranchSource {
    /// `ctx.branch`.
    Explicit,
    /// The presented lease's branch (a task lease or a run-scoped role lease).
    Lease,
    /// The binding of `ctx.meta.sandboxCwd`, else of `ctx.stamp.cwd`.
    SandboxBinding,
    /// `ctx.env.MOIRAI_BRANCH`.
    Env,
    /// The `branch=` field of `ctx.marker`.
    Marker,
    /// The client head of `ctx.client`, else of `MOIRAI_CLIENT`.
    ClientHead,
    /// The binding of `ctx.cwd`.
    CwdBinding,
    /// The binding of the git top-level that contains `ctx.cwd` (the git-worktree hint, [API §6.5]).
    GitHint,
    /// For `door` = `mcp`, the session's client head `session:<harness>:<id>` ([API] open point 6).
    SessionHead,
    /// The `default-branch` key ([CFG §10.1]).
    DefaultBranch,
}

/// CX-2's order of record ([API §4.2]; [90 §4.1] Branch row).
pub const CX2: [BranchSource; 10] = [
    BranchSource::Explicit,
    BranchSource::Lease,
    BranchSource::SandboxBinding,
    BranchSource::Env,
    BranchSource::Marker,
    BranchSource::ClientHead,
    BranchSource::CwdBinding,
    BranchSource::GitHint,
    BranchSource::SessionHead,
    BranchSource::DefaultBranch,
];

/// The raw inputs of CX-2.
#[derive(Clone, Debug, Default)]
pub struct BranchInputs {
    /// `ctx.branch`.
    pub explicit: Option<String>,
    /// The presented lease's branch, when it fixes one (WR-005).
    pub lease: Option<String>,
    /// `ctx.meta.sandboxCwd`, else `ctx.stamp.cwd`.
    pub sandbox_cwd: Option<String>,
    /// `MOIRAI_BRANCH`.
    pub env: Option<String>,
    /// The marker's `branch=`.
    pub marker: Option<String>,
    /// `ctx.client`, else `MOIRAI_CLIENT`.
    pub client: Option<String>,
    /// `ctx.cwd`.
    pub cwd: Option<String>,
    /// The git top-level containing `ctx.cwd`, when the tree has a simulated git history (WP-92's `EnvGit`).
    pub git_top: Option<String>,
    /// For an MCP call, `session:<harness>:<id>` of the resolved session.
    pub session_key: Option<String>,
    /// The effective `default-branch`.
    pub default_branch: String,
}

/// CX-2 ([API §4.2]): the first source of [`CX2`] that yields a branch or a detached commit; the `default-branch` key
/// always yields.
// spec: [API §4.2] CX-2
// spec: [CFG §10.1] default-branch
pub fn resolve_branch(heads: &Heads, i: &BranchInputs) -> Target {
    let bound = |p: &Option<String>| {
        p.as_deref()
            .and_then(|p| heads.binding_of(p))
            .map(|h| h.target.clone())
    };
    for s in CX2 {
        let got = match s {
            BranchSource::Explicit => i.explicit.clone().map(Target::Ref),
            BranchSource::Lease => i.lease.clone().map(Target::Ref),
            BranchSource::SandboxBinding => bound(&i.sandbox_cwd),
            BranchSource::Env => i.env.clone().map(Target::Ref),
            BranchSource::Marker => i.marker.clone().map(Target::Ref),
            BranchSource::ClientHead => i
                .client
                .as_deref()
                .and_then(|c| heads.get(HeadKind::Client, c))
                .map(|h| h.target.clone()),
            BranchSource::CwdBinding => bound(&i.cwd),
            BranchSource::GitHint => bound(&i.git_top),
            BranchSource::SessionHead => i
                .session_key
                .as_deref()
                .and_then(|k| heads.get(HeadKind::Session, k))
                .map(|h| h.target.clone()),
            BranchSource::DefaultBranch => Some(Target::Ref(i.default_branch.clone())),
        };
        if let Some(t) = got {
            return t;
        }
    }
    unreachable!("the default-branch source always yields")
}

/// CX-5 ([API §4.2]): `ctx.tree` → `ctx.meta.sandboxCwd` → `ctx.stamp.cwd` → the `worktree_path` of the lane whose
/// `moirai_branch` is the presented lease's branch → `ctx.cwd`.
// spec: [API §4.2] CX-5
pub fn resolve_tree(
    tree: Option<&str>,
    sandbox_cwd: Option<&str>,
    stamp_cwd: Option<&str>,
    lane_tree: Option<&str>,
    cwd: Option<&str>,
) -> Option<String> {
    tree.or(sandbox_cwd)
        .or(stamp_cwd)
        .or(lane_tree)
        .or(cwd)
        .map(str::to_string)
}

/// CX-6 ([API §4.2]): the `model` of the run the presented lease is scoped to, else of the run `MOIRAI_RUN` names → the
/// `model=` field of `ctx.marker` → `ctx.model`, else `MOIRAI_MODEL` → `ctx.hook_model`; `None` leaves the client's
/// default family (`lq.model-profile.default.<client>`, [`crate::profile::model_profile`]).
// spec: [API §4.2] CX-6
pub fn resolve_model(
    run_model: Option<&str>,
    marker: Option<&str>,
    declared: Option<&str>,
    env: Option<&str>,
    hook: Option<&str>,
) -> Option<String> {
    let marker_model = marker.and_then(|m| {
        m.strip_prefix("moirai:")?
            .split(' ')
            .find_map(|f| f.strip_prefix("model="))
    });
    run_model
        .or(marker_model)
        .or(declared)
        .or(env)
        .or(hook)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heads::Head;

    #[test]
    fn the_branch_order_is_the_record_s() {
        let mut heads = Heads::default();
        heads.rows.insert(
            (HeadKind::Client, "ci".into()),
            Head {
                kind: HeadKind::Client,
                key: "ci".into(),
                target: Target::Detached(7),
                designated: false,
                expected_ref: None,
                base: None,
                hlc: 0,
            },
        );
        let mut i = BranchInputs {
            default_branch: "main".into(),
            client: Some("ci".into()),
            marker: Some("lane/m".into()),
            ..BranchInputs::default()
        };
        assert_eq!(resolve_branch(&heads, &i), Target::Ref("lane/m".into()));
        i.marker = None;
        assert_eq!(resolve_branch(&heads, &i), Target::Detached(7));
        i.client = None;
        assert_eq!(resolve_branch(&heads, &i), Target::Ref("main".into()));
        assert_eq!(
            resolve_model(
                None,
                Some("moirai:task=#1 model=GPT-5"),
                Some("x"),
                None,
                None
            )
            .as_deref(),
            Some("GPT-5")
        );
    }
}
