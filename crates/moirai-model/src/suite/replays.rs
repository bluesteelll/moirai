//! Idempotency of the family-W commands and the default key ([API §7.2]–§7.5; [AR §4.5] step 2): a retry of a keyed
//! command that succeeded replays its result, looked up before any precondition of the command; the payload is
//! [API §7.3]'s `payload(c)` over the arguments as given; the default key names the attested thread or agent.

use super::*;
use crate::api::{Data, StampCtx};
use crate::clock::EnvClock;
use crate::dag::RefKind;
use crate::err::Kv;
use crate::schema::{FieldItem, Item, Shape, Storage, Ty};

fn field(name: &str, ty: Ty) -> Item {
    Item::Field(FieldItem {
        kind: Some("task".into()),
        name: name.into(),
        ty,
        class: "scalar",
        storage: Storage::Field,
        decl: 0,
        optional: true,
        default: None,
        range: None,
        one_line: false,
        ascii: false,
        shape: Shape::Plain,
        index: "none",
        coerce: "none",
        retired: false,
    })
}

fn keyed(k: &str) -> Ctx {
    Ctx {
        key: Some(k.into()),
        ..orch()
    }
}

#[test]
fn schema_retries_replay_and_other_items_mismatch() {
    let mut s = S::base();
    let cmd = |it: Item| Cmd::Schema {
        items: vec![it],
        message: "add effort".into(),
    };
    let a = s.ok(cmd(field("effort", Ty::Int)), orch());
    let seq = s.st.commit_seq;
    // The retry replays: the lookup precedes the "held already" check.
    let b = s.run(cmd(field("effort", Ty::Int)), orch());
    assert_eq!(b.outcome, Outcome::Replayed);
    assert_eq!((b.commit, b.rev_new), (a.commit, a.rev_new));
    assert_eq!(b.data, a.data);
    assert_eq!(s.st.commit_seq, seq);
    // Another definition under one explicit key has another payload: E408, not a replay.
    s.ok(cmd(field("size", Ty::Int)), keyed("k1"));
    let e = s.refused(cmd(field("size", Ty::Text)), keyed("k1"), "E408");
    let err = e.error.unwrap();
    assert_eq!(err.key_names(), vec!["key", "original"]);
    assert_eq!(err.get("key"), Some(&Kv::Str("k1".into())));
    let o = err.get("original").unwrap();
    assert_eq!(o.member("ref"), Some(&Kv::Str("main".into())));
    assert!(matches!(o.member("commit"), Some(Kv::Commit(_))));
}

#[test]
fn branch_create_and_delete_replay_their_data() {
    let mut s = S::base();
    s.ok(tx(vec![task("x", "x")]), orch());
    let create = |kind| Cmd::BranchCreate {
        name: "a".into(),
        from: None,
        kind,
    };
    let a = s.ok(create(None), keyed("mk"));
    let b = s.run(create(None), keyed("mk"));
    assert_eq!(b.outcome, Outcome::Replayed, "not ref_exists");
    assert_eq!(b.data, a.data);
    assert_eq!(b.branch.as_deref(), Some("lane/a"));
    // `kind` is in the payload: the same key with another kind is E408.
    s.refused(create(Some(RefKind::Work)), keyed("mk"), "E408");
    // A delete releases the lane's live leases; its retry replays the counts and the released leases.
    s.ok(tx(vec![set(1, &[("title", t("lane"))])]), orch_on("lane/a"));
    s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(1))],
            next: false,
            scope: None,
            role: None,
            agent: Some("dev".into()),
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        orch_on("lane/a"),
    );
    let del = || Cmd::BranchDelete {
        name: "lane/a".into(),
        force: true,
    };
    let d1 = s.ok(del(), orch());
    assert!(matches!(d1.data, Data::BranchDelete(_, 1, (1, 0, 0), ref rel) if rel == &vec![2]));
    let d2 = s.run(del(), orch());
    assert_eq!(d2.outcome, Outcome::Replayed, "not E301 on the deleted ref");
    assert_eq!(d2.data, d1.data);
}

#[test]
fn run_open_and_close_replay_their_data() {
    let mut s = S::base();
    let open = || Cmd::RunOpen {
        name: "r1".into(),
        fields: vec![("wf_id".into(), t("w-1"))],
    };
    let a = s.ok(open(), orch());
    let b = s.run(open(), orch());
    assert_eq!(b.outcome, Outcome::Replayed, "not name_taken");
    assert_eq!(b.data, a.data);
    assert!(matches!(b.data, Data::RunOpen(Nid(1), ref n) if n == "r1"));
    let c = &s.st.dag.commits[&a.rev_new.unwrap()];
    assert_eq!(
        (c.stmt_origin, c.stmt_sym.as_deref(), c.stmt_hash),
        ("verb", Some("run open"), None)
    );
    s.ok(
        Cmd::Claim {
            ids: vec![],
            next: false,
            scope: None,
            role: Some("tester".into()),
            agent: Some("w1".into()),
            ttl: Some(t("run")),
            start: false,
            run: Some("r1".into()),
            session: false,
        },
        orch(),
    );
    let close = || Cmd::RunClose {
        name: "r1".into(),
        outcome: "red".into(),
    };
    let x = s.ok(close(), orch());
    assert!(
        matches!(x.data, Data::RunClose(Nid(1), ref st, ref rel) if st == "red" && rel == &vec![2])
    );
    let y = s.run(close(), orch());
    assert_eq!(y.outcome, Outcome::Replayed);
    assert_eq!(y.data, x.data, "the released leases are replayed");
}

/// [API §6.7]: under `EnvCrash in-next` the applied command's retry with the same key converges, for a family-W
/// command too.
#[test]
fn a_retry_after_a_crash_converges() {
    let mut s = S::base();
    s.ok(Cmd::EnvCrash { in_next: true }, Ctx::default());
    let open = || Cmd::RunOpen {
        name: "r1".into(),
        fields: vec![],
    };
    let r = s.run(open(), keyed("run-r1"));
    assert_eq!(r.error.map(|e| e.code), Some("outcome_unknown".into()));
    assert_eq!(s.st.commit_seq, 1, "the applied candidate");
    let again = s.run(open(), keyed("run-r1"));
    assert_eq!(again.outcome, Outcome::Replayed);
    // Quiet is a write: a crash in it keeps the store without it.
    s.ok(Cmd::EnvCrash { in_next: true }, Ctx::default());
    let q = s.run(Cmd::Quiet { on: true }, orch());
    assert_eq!(q.error.map(|e| e.code), Some("outcome_unknown".into()));
    assert!(s.st.quiet);
    s.st.adopt_without();
    assert!(!s.st.quiet);
}

/// The default key's `a` is `codex:` + `meta.threadId`, else `claude:` + `stamp.agent_id`, else the resolved actor
/// ([API §7.2]), whatever lease is presented or thread the environment names.
#[test]
fn default_keys_name_the_attested_agent() {
    let mut s = S::base();
    // Two Claude subagents presenting one lease: the actor is the lease's holder for both, the key's agent differs.
    let sub = |agent: &str| Ctx {
        lease: Some("L-1".into()),
        client: Some("claude".into()),
        stamp: Some(StampCtx {
            session_id: Some("s1".into()),
            agent_id: Some(agent.into()),
            agent_type: None,
            cwd: None,
        }),
        ..Default::default()
    };
    let n = || tx(vec![node("n", "note", &[("title", t("same"))])]);
    let a = s.ok(n(), sub("ag7"));
    let b = s.ok(n(), sub("ag8"));
    assert_eq!((a.outcome, b.outcome), (Outcome::Ok, Outcome::Ok));
    assert_eq!(s.run(n(), sub("ag7")).outcome, Outcome::Replayed);
    // In a Codex environment without `meta`, the declared agents differ and so do their keys. The Codex client's
    // default profile is `unknown` ([90 §8.2]); the store lets that profile write free-form `TX` here.
    s.ok(
        Cmd::ConfigSet {
            key: "query.safelist.model.unknown".into(),
            value: "off".into(),
            scope: crate::confcmd::FileScope::Store,
        },
        Ctx::default(),
    );
    let codex = |agent: &str| {
        let mut c = Ctx {
            agent: Some(agent.into()),
            ..Default::default()
        };
        c.env.insert("CODEX_THREAD_ID".into(), "T1".into());
        c
    };
    let m = || tx(vec![node("m", "note", &[("title", t("mine"))])]);
    assert_eq!(s.ok(m(), codex("dev1")).outcome, Outcome::Ok);
    assert_eq!(s.ok(m(), codex("dev2")).outcome, Outcome::Ok);
    assert_eq!(s.run(m(), codex("dev1")).outcome, Outcome::Replayed);
    // The default window closes.
    s.ok(
        Cmd::EnvClock(EnvClock {
            advance_ms: Some(601_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    assert_eq!(s.run(n(), sub("ag7")).outcome, Outcome::Ok);
}
