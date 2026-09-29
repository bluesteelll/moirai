//! Leases ([AR §6.2]; [F11 §6]; [API §10.1]): the runtime rows, their liveness decided as data by
//! [RULES/state-definition] `lease-live` over the injected environment ([API §6.3] SL-1, SL-2), the events that end
//! them (`lease-ends`) and their effects (`lease-effects`), and fencing (I17′).

use crate::clock::{Deadline, Env, Liveness, Stamp, deadline_state};
use crate::rules::rules;
use crate::value::Nid;

/// The anchor kind of a lease ([F03 §10.3]; [90 §4.4]): 0 `none`, 1 `session`, 4 `session-ttl`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnchorKind {
    /// Lives by its deadline alone.
    None,
    /// A Claude Code session's slot keeps it alive.
    Session,
    /// A Codex thread's slot keeps it alive until its deadline.
    SessionTtl,
}

impl AnchorKind {
    /// The token of `lease-live`'s `anchor` column and of the runtime snapshot.
    pub fn name(self) -> &'static str {
        match self {
            AnchorKind::None => "none",
            AnchorKind::Session => "session",
            AnchorKind::SessionTtl => "session-ttl",
        }
    }
}

/// `task` or `role` ([90 §10.1] `LEASES.kind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LeaseKind {
    /// A task lease on one node.
    Task,
    /// A role lease (`#N` = 0).
    Role,
}

/// Why a lease ended ([F05 §9.4] field 18; [RULES/state-definition] `lease-ends`). Reason 8 (`SubagentStop`) has no
/// variant: the hook releases through `Release` ([API §10.3], reason 1; LE-010).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndReason {
    /// 1 `release` (LE-001).
    Release,
    /// 2 `complete`, into `settled` (LE-002).
    Complete,
    /// 3 `reclaim` (LE-005, LE-006).
    Reclaim,
    /// 4 dead: not live when a claim of its task supersedes it (its anchor Dead, its deadline passed or its boot
    /// changed; LE-009, LE-012).
    Dead,
    /// 5 branch deleted (LE-008).
    BranchDeleted,
    /// 6 `apply` (LE-003).
    Apply,
    /// 7 `run close` (LE-004).
    RunClose,
    /// 9 `rm --release` (LE-007).
    RmRelease,
}

impl EndReason {
    /// The release reason byte of [F05 §9.4] field 18.
    pub fn code(self) -> u8 {
        match self {
            EndReason::Release => 1,
            EndReason::Complete => 2,
            EndReason::Reclaim => 3,
            EndReason::Dead => 4,
            EndReason::BranchDeleted => 5,
            EndReason::Apply => 6,
            EndReason::RunClose => 7,
            EndReason::RmRelease => 9,
        }
    }
}

/// One lease row ([F11 §6]; [API §10.1] "Each new lease"; [API §15.7] `leases`).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Lease {
    /// `lease_id`, which is also its fencing token (`fence + 1` at the grant).
    pub id: u64,
    /// The current fencing token; expiry never bumps it (I17′).
    pub token: u64,
    /// The task, or `None` for a role lease.
    pub task: Option<Nid>,
    /// `task` or `role`.
    pub kind: LeaseKind,
    /// The role the lease grants.
    pub role: String,
    /// The holder.
    pub holder: String,
    /// The branch it was taken on.
    pub branch: String,
    /// The anchor kind.
    pub anchor: AnchorKind,
    /// The anchor's session identity (`claude:<id>`, `codex:<id>`), when anchored.
    pub session: Option<String>,
    /// The anchor's `boot_hash` at the grant.
    pub anchor_boot_hash: u64,
    /// The deadline; `Stamp::NEVER` for a run-scoped lease.
    pub expires: Stamp,
    /// The TTL; 0 when run-scoped.
    pub ttl_ms: u64,
    /// The run the lease is scoped to.
    pub run: Option<Nid>,
    /// Run-scoped: released only by `apply`, `run close` or `reclaim`.
    pub run_scoped: bool,
    /// The orchestrator's session role lease.
    pub session_role: bool,
    /// The HLC of the grant.
    pub claimed_hlc: u64,
    /// `bound`: the attested thread's hash, once bound (CX-9).
    pub bound: Option<[u8; 16]>,
    /// `root_session`.
    pub root_session: Option<[u8; 16]>,
    /// The task's `files_owned` at the grant.
    pub files_owned: Vec<String>,
    /// Why it ended, once it has.
    pub ended: Option<EndReason>,
}

/// The inputs of one `lease-live` evaluation: the column tokens of [RULES/state-definition] `lease-live`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LiveInputs {
    /// `run` or `ttl`.
    pub scope: &'static str,
    /// `none`, `session`, `session-ttl`.
    pub anchor: &'static str,
    /// `same`, `different`, `unknown`.
    pub boot: &'static str,
    /// `named`, `not-named`, `unreadable`.
    pub slot: &'static str,
    /// `passed`, `not-passed`.
    pub deadline: &'static str,
}

/// The liveness of a lease as the runtime snapshot names it ([API §15.7] `live`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Live {
    /// Live.
    Alive,
    /// Live, but its anchor could not be judged (Unknown never ends a lease, LF-006).
    Unknown,
    /// Not live: its anchor is Dead (LE-009). A read ends nothing, so until the next claim of its task ends it
    /// (LE-012) it is judged again at each evaluation.
    Dead,
    /// Not live only because its deadline passed: its holder may renew it (LE-011) until another claim of its task
    /// ends it (LE-012).
    Deadline,
}

impl Live {
    /// Whether the lease counts as live.
    pub fn is_live(self) -> bool {
        matches!(self, Live::Alive | Live::Unknown)
    }
}

/// The `lease-live` inputs of a lease in an environment ([RULES/state-definition] §5; [API §6.3] SL-2).
pub fn inputs(l: &Lease, env: &Env) -> LiveInputs {
    let now = env.now();
    let boot = if env.unknown || l.anchor_boot_hash == 0 {
        "unknown"
    } else if l.anchor_boot_hash == env.boot_hash() {
        "same"
    } else {
        "different"
    };
    let slot = match (&l.session, l.anchor) {
        (_, AnchorKind::None) | (None, _) => "not-named",
        (Some(s), _) => match env.anchor_liveness(s, l.anchor_boot_hash) {
            Liveness::Alive => "named",
            Liveness::Unknown => "unreadable",
            Liveness::Dead => {
                if env.readable {
                    "not-named"
                } else {
                    "unreadable"
                }
            }
        },
    };
    let deadline = match deadline_state(l.expires, now) {
        Deadline::NotPassed => "not-passed",
        Deadline::Passed | Deadline::BootChanged => "passed",
    };
    LiveInputs {
        scope: if l.run_scoped { "run" } else { "ttl" },
        anchor: l.anchor.name(),
        boot,
        slot,
        deadline,
    }
}

/// `lease-live` as data ([RULES/state-definition] LL rows): the first row whose cells all match (a `*` matches
/// anything) decides. LL-014 is unreachable in format v1: reaching it is a failed internal check.
// spec: [RULES/state-definition] lease-live
// rule: LL-001, LL-002, LL-003, LL-004, LL-005, LL-006, LL-007, LL-008, LL-009, LL-010, LL-011, LL-012, LL-013, LL-014
pub fn lease_live_row(i: LiveInputs) -> &'static crate::rules::Row {
    let t = rules().table("lease-live");
    let m = |cell: &str, v: &str| cell == "*" || cell == v;
    let r = t
        .rows
        .iter()
        .find(|r| {
            m(r.tok("scope"), i.scope)
                && m(r.tok("anchor"), i.anchor)
                && m(r.tok("boot"), i.boot)
                && m(r.tok("slot"), i.slot)
                && m(r.tok("deadline"), i.deadline)
        })
        .unwrap_or_else(|| panic!("lease-live has no row for {i:?}"));
    assert_ne!(
        r.tok("live"),
        "-",
        "internal check: {} is unreachable in format v1",
        r.id
    );
    r
}

/// Whether a lease is live now, and how it reads in the runtime snapshot ([API §15.7] `live`).
// spec: [RULES/state-definition] lease-live
// spec: [AR §6.2]
// rule: LE-009, LF-006
pub fn is_live(l: &Lease, env: &Env) -> Live {
    if l.ended.is_some() {
        return Live::Dead;
    }
    let i = inputs(l, env);
    let r = lease_live_row(i);
    if r.tok("live") == "yes" {
        if i.slot == "unreadable" && l.anchor != AnchorKind::None {
            Live::Unknown
        } else {
            Live::Alive
        }
    } else if i.boot == "different" || (i.slot == "not-named" && l.anchor != AnchorKind::None) {
        Live::Dead
    } else {
        Live::Deadline
    }
}

/// LF-001 and LF-002: a live task lease on `#N` held by another holder excludes `#N` from the caller's `ready` on
/// every branch; a caller with no actor is excluded by every live lease; role leases never count.
// spec: [RULES/state-definition] lease-effects
// rule: LF-001, LF-002
pub fn excludes_from_ready<'a>(
    leases: impl IntoIterator<Item = &'a Lease>,
    env: &Env,
    n: Nid,
    caller: Option<&str>,
) -> bool {
    leases.into_iter().any(|l| {
        l.kind == LeaseKind::Task
            && l.task == Some(n)
            && is_live(l, env).is_live()
            && caller.is_none_or(|c| c != l.holder)
    })
}

/// LF-005 (I32′): a live lease on any node of `set`, on any branch; the first in (`#N`, lease id) order.
// spec: [F13 §3.4] I32′
// rule: LF-005
pub fn i32p_rm_refused_under_lease<'a>(
    leases: impl IntoIterator<Item = &'a Lease>,
    env: &Env,
    set: &[Nid],
) -> Option<&'a Lease> {
    let mut v: Vec<&Lease> = leases
        .into_iter()
        .filter(|l| l.task.is_some_and(|t| set.contains(&t)) && is_live(l, env).is_live())
        .collect();
    v.sort_by_key(|l| (l.task, l.id));
    v.into_iter().next()
}

/// The shape of a claim, which decides where its TTL comes from ([API §10.1]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClaimShape {
    /// A task claim without `run`.
    Task,
    /// A task claim with `run` (the dispatcher's bulk claim).
    TaskOfRun,
    /// The orchestrator's session role lease.
    SessionRole,
    /// A run-scoped role lease.
    RunRole,
}

/// The TTL of a new lease in ms, or `None` for a run-scoped lease ([API §10.1]; [CFG §10.3]): the explicit `ttl`
/// (`Some(None)` is `ttl = run`), else `lease.orchestrator-ttl` for the session role lease, run-scoped for a claim that
/// names a run and for a run role lease, and `lease.ttl-default` for a task claim. The session role lease is never
/// run-scoped: `ttl = run` falls back to `lease.orchestrator-ttl`.
// spec: [API §10.1]
// spec: [CFG §10.3] lease.ttl-default, lease.orchestrator-ttl
pub fn ttl_for(
    shape: ClaimShape,
    explicit: Option<Option<u64>>,
    ttl_default_ms: u64,
    orchestrator_ttl_ms: u64,
) -> Option<u64> {
    match (shape, explicit) {
        (ClaimShape::SessionRole, e) => Some(e.flatten().unwrap_or(orchestrator_ttl_ms)),
        (_, Some(e)) => e,
        (ClaimShape::Task, None) => Some(ttl_default_ms),
        (ClaimShape::TaskOfRun | ClaimShape::RunRole, None) => None,
    }
}

/// The deadline of a lease granted at `now` ([OS/clock §4.2] `after`): `Stamp::NEVER` for a run-scoped lease.
// spec: [API §10.1] expires
pub fn deadline(now: Stamp, ttl_ms: Option<u64>) -> Stamp {
    ttl_ms.map_or(Stamp::NEVER, |t| crate::clock::after(now, t))
}

/// The leases `reclaim` releases ([API §10.4]; LE-005, LE-006): with a run, every lease scoped to it that has not
/// ended; otherwise every task lease that has not ended and whose `claimed_hlc` is older than `older_than_ms`
/// (`lease.reclaim-older-than` when the call gives none) by the window rule CK-6, whatever its liveness; by lease id.
// spec: [API §10.4]
// spec: [CFG §10.3] lease.reclaim-older-than
pub fn reclaim<'a>(
    leases: impl IntoIterator<Item = &'a Lease>,
    run: Option<Nid>,
    older_than_ms: u64,
    hlc: &crate::clock::Hlc,
    wall_ms: i64,
) -> Vec<u64> {
    leases
        .into_iter()
        .filter(|l| l.ended.is_none())
        .filter(|l| match run {
            Some(r) => l.run == Some(r),
            None => l.kind == LeaseKind::Task && !hlc.within(wall_ms, l.claimed_hlc, older_than_ms),
        })
        .map(|l| l.id)
        .collect()
}

/// I17′: a lease mutation presents the current token of a lease that has not ended.
// spec: [F13 §3.4] I17′
pub fn i17p_fencing(l: &Lease, token: u64) -> bool {
    l.ended.is_none() && l.token == token
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{EnvClock, EnvSlots, after};

    fn lease(env: &Env, anchor: AnchorKind, run_scoped: bool, ttl: u64) -> Lease {
        Lease {
            id: 1,
            token: 1,
            task: Some(Nid(5)),
            kind: LeaseKind::Task,
            role: "developer".into(),
            holder: "dev".into(),
            branch: "main".into(),
            anchor,
            session: (anchor != AnchorKind::None).then(|| "claude:s1".to_string()),
            anchor_boot_hash: env.boot_hash(),
            expires: if run_scoped {
                Stamp::NEVER
            } else {
                after(env.now(), ttl)
            },
            ttl_ms: if run_scoped { 0 } else { ttl },
            run: None,
            run_scoped,
            session_role: false,
            claimed_hlc: 0,
            bound: None,
            root_session: None,
            files_owned: vec![],
            ended: None,
        }
    }

    /// Every combination of the `lease-live` inputs reaches a row: the table is exhaustive, and LL-014 is never met.
    #[test]
    fn lease_live_is_exhaustive() {
        for scope in ["run", "ttl"] {
            for anchor in ["none", "session", "session-ttl"] {
                for boot in ["same", "different", "unknown"] {
                    for slot in ["named", "not-named", "unreadable"] {
                        for deadline in ["passed", "not-passed"] {
                            let r = lease_live_row(LiveInputs {
                                scope,
                                anchor,
                                boot,
                                slot,
                                deadline,
                            });
                            assert!(r.tok("live") == "yes" || r.tok("live") == "no");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_session_lease_lives_by_its_slot_and_a_none_lease_by_its_deadline() {
        let mut env = Env::default();
        env.slots(&EnvSlots {
            hold: vec!["claude:s1".into()],
            ..Default::default()
        });
        let s = lease(&env, AnchorKind::Session, false, 1000);
        let n = lease(&env, AnchorKind::None, false, 1000);
        let r = lease(&env, AnchorKind::None, true, 0);
        env.clock(&EnvClock {
            advance_ms: Some(2000),
            ..Default::default()
        });
        assert_eq!(
            is_live(&s, &env),
            Live::Alive,
            "LL-005 whatever the deadline"
        );
        assert_eq!(is_live(&n, &env), Live::Deadline, "LL-003");
        env.slots(&EnvSlots {
            release: vec!["claude:s1".into()],
            ..Default::default()
        });
        assert_eq!(is_live(&s, &env), Live::Dead, "LL-006");
        env.clock(&EnvClock {
            reboot: true,
            ..Default::default()
        });
        assert_eq!(
            is_live(&r, &env),
            Live::Alive,
            "LL-001: a run-scoped lease survives a reboot"
        );
        assert!(excludes_from_ready([&r], &env, Nid(5), Some("other")));
        assert!(!excludes_from_ready([&r], &env, Nid(5), Some("dev")));
        assert!(excludes_from_ready([&r], &env, Nid(5), None));
    }
}
