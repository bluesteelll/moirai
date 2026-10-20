//! `BodyDrop` through the Store API ([API §8.7]; [F06 §8.1]): the owner-attested CLI caller (WV-051, WT-012), the
//! argument bounds, the held check over the resolvable commits' states, the dropped set with its reasons in
//! `Runtime.dropped` ([API §15.7]), CK-4's one draw per written record ([API §6.2]), and the readers and writers of a
//! dropped body.

use super::*;
use crate::api::{Data, Door, RuntimeSnap};
use crate::bodydrop::{BodyDropData, DropReason};
use crate::canon::body_hash;
use crate::state::{Aspect, Body, KVal};
use crate::value::{blake3_128, hex};

/// The 32 hex digits of a body text's hash.
fn hx(text: &str) -> String {
    hex(&hb(text))
}

/// A body text's hash ([F07 §6.3]).
fn hb(text: &str) -> [u8; 16] {
    body_hash(&Body::Text(text.into()))
}

/// A `BodyDrop` of hashes given as text, with a reason, a note and the attestation.
fn drop_raw(hashes: Vec<String>, reason: &str, note: &str, by: Option<&str>) -> Cmd {
    Cmd::BodyDrop {
        hashes,
        reason: reason.into(),
        note: note.into(),
        by: by.map(str::to_string),
    }
}

/// An owner-attested `BodyDrop` of these hashes for `secret`.
fn drop_owner(hashes: &[[u8; 16]]) -> Cmd {
    drop_raw(
        hashes.iter().map(|h| hex(h)).collect(),
        "secret",
        "",
        Some("owner"),
    )
}

/// The `data` of a `BodyDrop` reply.
fn data(r: &Reply) -> &BodyDropData {
    match &r.data {
        Data::BodyDrop(d) => d,
        other => panic!("not a BodyDrop result: {other:?}"),
    }
}

/// The orchestrator without the default idempotency key, on a branch when one is named.
fn on(branch: Option<&str>) -> Ctx {
    Ctx {
        branch: branch.map(str::to_string),
        no_dedupe: true,
        ..orch()
    }
}

/// A `set` of a node's body.
fn set_body(n: u32, text: &str) -> Stmt {
    Stmt::Set {
        target: Target::Id(Nid(n)),
        fields: vec![],
        incr: vec![],
        body: Some(Some(text.into())),
        guard: None,
    }
}

/// A `create` of a task with a body.
fn task_with_body(name: &str, title: &str, body: &str) -> Stmt {
    match task(name, title) {
        Stmt::Create {
            name,
            kind,
            fields,
            under,
            position,
            edges_out,
            edges_in,
            ..
        } => Stmt::Create {
            name,
            kind,
            fields,
            body: Some(body.into()),
            under,
            position,
            edges_out,
            edges_in,
        },
        _ => unreachable!(),
    }
}

/// The `hlc` the next local commit takes: a note committed on a copy of the store.
fn next_hlc(s: &S) -> u64 {
    let mut c = S { st: s.st.clone() };
    let r = c.ok(
        tx(vec![node("probe", "note", &[("title", t("probe"))])]),
        on(None),
    );
    c.st.dag.commits[&r.rev_new.expect("the probe commits")].hlc
}

/// The base stream with tasks #1 and #2 on `main`, whose bodies are `body one` and `body two`.
fn two_bodies() -> S {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task_with_body("a", "task a", "body one"),
            task_with_body("b", "task b", "body two"),
        ]),
        orch(),
    );
    s
}

/// An E406 that refuses the call as a whole: exit 6, `statement` null, `written` false ([LQ/errors §5.7]).
fn assert_verb_e406(r: &Reply) {
    let e = r.error.as_ref().expect("refused");
    assert_eq!((e.code.as_str(), r.exit), ("E406", 6), "{e:?}");
    assert_eq!(e.get("statement"), Some(&crate::err::Kv::Null), "{e:?}");
    assert_eq!(
        e.get("written"),
        Some(&crate::err::Kv::Bool(false)),
        "{e:?}"
    );
}

/// S3-M-10 (WV-051; WT-012; [API §8.7] "Caller"): a call that is not owner-attested is E406, exit 6, before its
/// arguments are checked, and writes and draws nothing: the session role lease without the attestation or with another
/// `by`, a task lease, each run-scoped role lease of `role-rows`, no lease, and `by = owner` with any lease but the
/// session role lease or with none.
#[test]
fn a_call_that_is_not_owner_attested_is_e406_before_its_arguments() {
    let mut s = two_bodies();
    // A task lease: a developer's self-claim of #1.
    let dev = Ctx {
        agent: Some("dev".into()),
        no_dedupe: true,
        ..Default::default()
    };
    s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(1))],
            next: false,
            scope: None,
            role: None,
            agent: None,
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        dev,
    );
    // A run-scoped role lease for every role of `role-rows` that a run-role lease carries (WO rows), one holder each.
    s.ok(
        Cmd::RunOpen {
            name: "r1".into(),
            fields: vec![],
        },
        on(None),
    );
    let roles: Vec<String> = crate::rules::rules()
        .table("role-rows")
        .rows
        .iter()
        .filter(|r| r.toks("carried_by").contains(&"run-role-lease"))
        .map(|r| r.tok("role").to_string())
        .collect();
    assert_eq!(roles.len(), 10, "WO-003 to WO-012: {roles:?}");
    for role in &roles {
        s.ok(
            Cmd::Claim {
                ids: vec![],
                next: false,
                scope: None,
                role: Some(role.clone()),
                agent: Some(format!("w-{role}")),
                ttl: None,
                start: false,
                run: Some("r1".into()),
                session: false,
            },
            on(None),
        );
    }
    let lease_of = |s: &S, holder: &str| -> String {
        let l =
            s.st.leases
                .values()
                .find(|l| l.holder == holder && l.ended.is_none())
                .unwrap_or_else(|| panic!("no lease held by {holder}"));
        format!("L-{}", l.id)
    };
    let task_lease = lease_of(&s, "dev");
    assert!(
        s.st.leases
            .values()
            .any(|l| l.holder == "dev" && l.task == Some(Nid(1)))
    );
    let role_leases: Vec<String> = roles
        .iter()
        .map(|r| lease_of(&s, &format!("w-{r}")))
        .collect();
    let r0 = s.st.runtime();
    let h0 = next_hlc(&s);
    let held = vec![hx("body one")];
    let presented = |lease: Option<&str>| Ctx {
        lease: lease.map(str::to_string),
        no_dedupe: true,
        ..Default::default()
    };
    let mut calls: Vec<(String, Cmd, Ctx)> = vec![
        (
            "L-1 without by".into(),
            drop_raw(held.clone(), "secret", "", None),
            on(None),
        ),
        (
            "L-1 with by = orchestrator".into(),
            drop_raw(held.clone(), "secret", "", Some("orchestrator")),
            on(None),
        ),
        (
            "L-1 with by = Owner".into(),
            drop_raw(held.clone(), "secret", "", Some("Owner")),
            on(None),
        ),
        (
            "a task lease".into(),
            drop_raw(held.clone(), "secret", "", None),
            presented(Some(&task_lease)),
        ),
        (
            "no lease".into(),
            drop_raw(held.clone(), "secret", "", None),
            presented(None),
        ),
        (
            "by = owner with a task lease".into(),
            drop_raw(held.clone(), "secret", "", Some("owner")),
            presented(Some(&task_lease)),
        ),
        (
            "by = owner with no lease".into(),
            drop_raw(held.clone(), "secret", "", Some("owner")),
            presented(None),
        ),
        (
            "L-1 without by, empty hashes".into(),
            drop_raw(vec![], "secret", "", None),
            on(None),
        ),
    ];
    for (role, lease) in roles.iter().zip(&role_leases) {
        calls.push((
            format!("the run-scoped {role} lease"),
            drop_raw(held.clone(), "secret", "", None),
            presented(Some(lease)),
        ));
        calls.push((
            format!("by = owner with the run-scoped {role} lease"),
            drop_raw(held.clone(), "secret", "", Some("owner")),
            presented(Some(lease)),
        ));
    }
    for (what, cmd, ctx) in calls {
        let r = s.run(cmd, ctx);
        assert_eq!(r.outcome, Outcome::Refused, "{what}: was not refused");
        assert_verb_e406(&r);
        assert_eq!(s.st.runtime(), r0, "{what}: the call wrote something");
        assert_eq!(next_hlc(&s), h0, "{what}: the call drew an HLC value");
    }
}

/// WV-051; [API §8.7] "No MCP tool, hook or `Apply` batch carries the command": through any door but the CLI an
/// attested call is E406 before its arguments.
#[test]
fn mcp_hook_and_apply_doors_are_e406_even_attested() {
    let mut s = two_bodies();
    let r0 = s.st.runtime();
    let h0 = next_hlc(&s);
    for door in [Door::Mcp, Door::Hook, Door::Apply] {
        let ctx = Ctx { door, ..on(None) };
        let r = s.run(drop_owner(&[hb("body one")]), ctx);
        assert_eq!(r.outcome, Outcome::Refused, "{door:?}: was not refused");
        assert_verb_e406(&r);
        assert_eq!(s.st.runtime(), r0, "{door:?}: the call wrote something");
        assert_eq!(next_hlc(&s), h0, "{door:?}: the call drew an HLC value");
    }
}

/// [API §8.7] "Effect", "Result"; [F06 §8.1] DB-5; [API §15.7] `dropped`: an attested drop of two held bodies lists
/// them ascending in `dropped` and in `Runtime.dropped` with their reason, and changes no state, commit id or counter.
#[test]
fn an_attested_drop_lists_the_held_hashes_and_changes_no_id_or_state() {
    let mut s = two_bodies();
    let mut hs = [hb("body one"), hb("body two")];
    hs.sort();
    let state = |s: &mut S| {
        s.ok(
            Cmd::State {
                ref_: Some("main".into()),
                at: None,
                parts: None,
            },
            on(None),
        )
        .data
    };
    let commits = |s: &S| -> Vec<(u64, [u8; 32], u64)> {
        s.st.dag
            .commits
            .values()
            .map(|c| (c.seq, c.id, c.hlc))
            .collect()
    };
    let (st0, c0, n0) = (state(&mut s), commits(&s), s.st.commit_seq);
    let rt0 = s.st.runtime();
    assert!(rt0.dropped.is_empty());
    // Named in descending order.
    let r = s.ok(drop_owner(&[hs[1], hs[0]]), on(None));
    assert_eq!((r.outcome, r.exit), (Outcome::Ok, 0));
    assert_eq!(
        data(&r),
        &BodyDropData {
            dropped: hs.to_vec(),
            already: vec![],
            backups: vec![],
            purged: true,
        }
    );
    let rt = s.st.runtime();
    assert_eq!(
        rt.dropped,
        vec![(hs[0], DropReason::Secret), (hs[1], DropReason::Secret)]
    );
    assert_eq!(state(&mut s), st0, "the State snapshot of main");
    assert_eq!(commits(&s), c0, "every commit's seq, id and hlc");
    assert_eq!(s.st.commit_seq, n0);
    let others = |r: &RuntimeSnap| RuntimeSnap {
        dropped: vec![],
        ..r.clone()
    };
    assert_eq!(others(&rt), others(&rt0), "every other Runtime member");
}

/// [API §8.7] "Refusals": `usage`, exit 2, naming the argument, with nothing written and nothing drawn; 2,048 hashes
/// and a 200-byte note pass the argument check, and the first unheld hash in ascending order is then `not_found`.
#[test]
fn bad_arguments_are_usage_and_the_bounds_pass() {
    let mut s = two_bodies();
    let r0 = s.st.runtime();
    let h0 = next_hlc(&s);
    let one = hx("body one");
    let many = |n: usize| -> Vec<String> {
        (0..n)
            .map(|i| hex(&blake3_128(&[format!("unheld {i}").as_bytes()])))
            .collect()
    };
    let cases: Vec<(&str, Cmd, &str)> = vec![
        (
            "empty",
            drop_raw(vec![], "secret", "", Some("owner")),
            "hashes",
        ),
        (
            "2,049",
            drop_raw(many(2049), "secret", "", Some("owner")),
            "hashes",
        ),
        (
            "31 digits",
            drop_raw(vec![one[..31].to_string()], "secret", "", Some("owner")),
            "hashes",
        ),
        (
            "33 digits",
            drop_raw(vec![format!("{one}0")], "secret", "", Some("owner")),
            "hashes",
        ),
        (
            "upper case",
            drop_raw(vec![one.to_uppercase()], "secret", "", Some("owner")),
            "hashes",
        ),
        (
            "unknown reason",
            drop_raw(vec![one.clone()], "leaked", "", Some("owner")),
            "reason",
        ),
        (
            "201-byte note",
            drop_raw(vec![one.clone()], "secret", &"n".repeat(201), Some("owner")),
            "note",
        ),
    ];
    assert!(
        one.chars().any(|c| c.is_ascii_alphabetic()),
        "the upper-case case changes the text"
    );
    for (what, cmd, arg) in cases {
        let r = s.run(cmd, on(None));
        let e = r
            .error
            .as_ref()
            .unwrap_or_else(|| panic!("{what}: was not refused"));
        assert_eq!((e.code.as_str(), r.exit), ("usage", 2), "{what}: {e:?}");
        assert_eq!(e.get_str("argument"), Some(arg), "{what}: {e:?}");
        assert_eq!(s.st.runtime(), r0, "{what}: the call wrote something");
        assert_eq!(next_hlc(&s), h0, "{what}: the call drew an HLC value");
    }
    // The bounds: 2,048 distinct unheld hashes and a 200-byte note pass the argument check.
    let hashes = many(2048);
    let smallest = hashes.iter().min().unwrap().clone();
    let r = s.run(
        drop_raw(hashes, "private", &"n".repeat(200), Some("owner")),
        on(None),
    );
    let e = r.error.as_ref().expect("refused");
    assert_eq!((e.code.as_str(), r.exit), ("not_found", 3), "{e:?}");
    assert_eq!(e.get_str("what"), Some("body"));
    assert_eq!(e.get_str("value"), Some(smallest.as_str()));
    assert_eq!(s.st.runtime(), r0);
    assert_eq!(next_hlc(&s), h0);
}

/// [API §8.7] "Checks": a hash the store neither dropped nor holds as a body refuses the whole command with
/// `not_found` (`what` = `body`), exit 3, and the held hashes named with it are not dropped.
#[test]
fn an_unheld_hash_is_not_found_and_nothing_drops() {
    let mut s = two_bodies();
    let held = [hb("body one"), hb("body two")];
    let top = *held.iter().max().unwrap();
    let u = (0..)
        .map(|i| blake3_128(&[format!("unheld {i}").as_bytes()]))
        .find(|h| *h > top)
        .unwrap();
    let h0 = next_hlc(&s);
    let r = s.run(drop_owner(&[held[0], u, held[1]]), on(None));
    let e = r.error.as_ref().expect("refused");
    assert_eq!((e.code.as_str(), r.exit), ("not_found", 3), "{e:?}");
    assert_eq!(e.get_str("what"), Some("body"));
    assert_eq!(e.get_str("value"), Some(hex(&u).as_str()));
    assert!(
        s.st.runtime().dropped.is_empty(),
        "the held hashes are not dropped"
    );
    assert!(s.st.dropped.is_empty());
    assert_eq!(next_hlc(&s), h0);
}

/// [F06 §8.1] DB-4; [API §8.7] "Checks": a body that a conflict value of a resolvable commit's state carries is held,
/// also when no state holds it as a node's body: the virtual base of a criss-cross merge, the base side of the outer
/// merge's `TextHunk` ([F12 §5]; [RULES/merge-table] MR-032).
#[test]
fn a_body_held_only_as_a_conflict_side_is_held() {
    let lines = |edits: &[(usize, &str)]| -> String {
        (1..=9)
            .map(|i| {
                edits
                    .iter()
                    .find(|(l, _)| *l == i)
                    .map_or(format!("l{i}"), |(_, x)| x.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut s = S::base();
    s.ok(tx(vec![task_with_body("x", "task x", &lines(&[]))]), orch());
    let branch = |s: &mut S, name: &str, from: &str| {
        s.ok(
            Cmd::BranchCreate {
                name: name.into(),
                from: Some(from.into()),
                kind: None,
            },
            on(None),
        );
    };
    let merge = |s: &mut S, src: &str, into: &str| -> Reply {
        s.ok(
            Cmd::Merge {
                src: src.into(),
                into: Some(into.into()),
                policy: None,
                strict: None,
                base: None,
                message: String::new(),
            },
            on(None),
        )
    };
    let edit = |s: &mut S, b: &str, text: String| {
        s.ok(tx(vec![set_body(1, &text)]), on(Some(b)));
    };
    branch(&mut s, "a", "main");
    branch(&mut s, "b", "main");
    edit(&mut s, "lane/a", lines(&[(1, "A")]));
    edit(&mut s, "lane/b", lines(&[(9, "D")]));
    branch(&mut s, "c", "lane/a");
    branch(&mut s, "d", "lane/b");
    edit(&mut s, "lane/a", lines(&[(1, "A"), (3, "B")]));
    edit(&mut s, "lane/b", lines(&[(7, "C"), (9, "D")]));
    // The criss-cross: lane/a takes lane/b's first edit, lane/b takes lane/a's.
    merge(&mut s, "lane/d", "lane/a");
    merge(&mut s, "lane/c", "lane/b");
    edit(
        &mut s,
        "lane/a",
        lines(&[(1, "A"), (3, "B"), (5, "P"), (9, "D")]),
    );
    edit(
        &mut s,
        "lane/b",
        lines(&[(1, "A"), (5, "Q"), (7, "C"), (9, "D")]),
    );
    let r = merge(&mut s, "lane/b", "lane/a");
    let Data::Merge(m) = &r.data else {
        panic!("{:?}", r.data)
    };
    assert!(m.virtual_base, "two LCAs: a virtual base");
    assert_eq!(
        m.conflicts,
        vec![("#1.body".to_string(), "TextHunk".to_string())]
    );
    // V, the virtual base's text: no state of any commit holds it as a node's body; the outer merge's conflict base
    // carries it.
    let v = hb(&lines(&[(1, "A"), (9, "D")]));
    for c in s.st.dag.commits.keys() {
        let st = s.st.dag.state_at(Some(*c), &s.st.alloc);
        assert!(
            st.nodes
                .values()
                .all(|x| x.body.as_ref().map(body_hash) != Some(v)),
            "commit s{c} holds V as a body"
        );
    }
    let tip = s.st.dag.live("lane/a").unwrap().tip;
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    let base = &st.nodes[&Nid(1)].conflicts[&Aspect::Body].base;
    assert!(
        matches!(base, Some(KVal::Body(b)) if body_hash(b) == v),
        "{base:?}"
    );
    let r = s.ok(drop_owner(&[v]), on(None));
    assert_eq!(data(&r).dropped, vec![v]);
    assert_eq!(s.st.runtime().dropped, vec![(v, DropReason::Secret)]);
}

/// [API §15.7] `dropped`; [F06 §8.1] DB-2: `Runtime.dropped` lists each dropped hash with the reason its drop named,
/// `private` and `other` as well as `secret`; a later call that names a dropped hash with another reason finds it
/// `already`, and the hash keeps its first reason.
#[test]
fn runtime_dropped_shows_each_hash_with_its_first_reason() {
    let mut s = two_bodies();
    let (one, two) = (hb("body one"), hb("body two"));
    s.ok(
        drop_raw(vec![hex(&one)], "private", "", Some("owner")),
        on(None),
    );
    s.ok(
        drop_raw(vec![hex(&two)], "other", "", Some("owner")),
        on(None),
    );
    let mut want = vec![(one, DropReason::Private), (two, DropReason::Other)];
    want.sort_by_key(|(h, _)| *h);
    assert_eq!(s.st.runtime().dropped, want, "each drop's own reason");
    let r = s.ok(
        drop_raw(vec![hex(&one), hex(&two)], "secret", "", Some("owner")),
        on(None),
    );
    let mut both = vec![one, two];
    both.sort();
    assert_eq!(data(&r).already, both);
    assert_eq!(data(&r).dropped, Vec::<[u8; 16]>::new());
    assert_eq!(s.st.runtime().dropped, want, "the first reason stays");
}

/// [API §8.7] "a hash named twice counts once".
#[test]
fn a_hash_named_twice_counts_once() {
    let mut s = two_bodies();
    let h = hb("body one");
    let r = s.ok(drop_owner(&[h, h]), on(None));
    assert_eq!(data(&r).dropped, vec![h]);
    assert_eq!(data(&r).already, Vec::<[u8; 16]>::new());
    assert_eq!(s.st.runtime().dropped, vec![(h, DropReason::Secret)]);
}

/// [API §8.7] "When every hash is already dropped, nothing is written"; open point 50: a retry finds the hashes
/// already dropped and writes nothing, and draws no HLC value.
#[test]
fn a_redrop_writes_and_draws_nothing() {
    let mut s = two_bodies();
    let h = hb("body one");
    s.ok(drop_owner(&[h]), on(None));
    let (rt, h0) = (s.st.runtime(), next_hlc(&s));
    let r = s.ok(drop_owner(&[h]), on(None));
    assert_eq!(data(&r).dropped, Vec::<[u8; 16]>::new());
    assert_eq!(data(&r).already, vec![h]);
    assert_eq!(s.st.runtime(), rt);
    assert_eq!(next_hlc(&s), h0);
}

/// [API §8.7] "Checks": a dropped hash goes to `already` "before and whether or not the store holds it as a body":
/// after a `Gc` pruned the only commit that held it, it is `already`, not `not_found`.
#[test]
fn a_dropped_hash_whose_commits_were_pruned_is_already() {
    let mut s = two_bodies();
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        on(None),
    );
    let r = s.ok(
        tx(vec![task_with_body("y", "task y", "lane body")]),
        on(Some("lane/x")),
    );
    let c = r.rev_new.unwrap();
    let x = hb("lane body");
    s.ok(drop_owner(&[x]), on(None));
    s.ok(
        Cmd::BranchDelete {
            name: "lane/x".into(),
            force: true,
        },
        on(None),
    );
    s.ok(
        Cmd::Gc {
            reflog_expire_ms: Some(0),
            cruft_delay_ms: Some(0),
            force: false,
        },
        on(None),
    );
    assert!(
        s.st.pruned.contains(&c),
        "the lane's commit stopped resolving"
    );
    let r = s.ok(drop_owner(&[x]), on(None));
    assert_eq!(data(&r).already, vec![x]);
    assert_eq!(data(&r).dropped, Vec::<[u8; 16]>::new());
}

/// CK-4 ([API §6.2]; [API §8.7] "it draws one value from CK-4's sequence"): a written record draws one value, so the
/// next local commit's `hlc` is one above what it would have been; a call that writes nothing draws none.
#[test]
fn a_written_record_draws_one_hlc_value() {
    let mut s = two_bodies();
    let h0 = next_hlc(&s);
    s.ok(drop_owner(&[hb("body one")]), on(None));
    assert_eq!(next_hlc(&s), h0 + 1);
    s.ok(drop_owner(&[hb("body one")]), on(None));
    assert_eq!(next_hlc(&s), h0 + 1, "an all-already call draws nothing");
}

/// [API §8.7] "Quiet mode does not refuse the command".
#[test]
fn quiet_mode_does_not_refuse_body_drop() {
    let mut s = two_bodies();
    s.ok(Cmd::Quiet { on: true }, on(None));
    let r = s.ok(drop_owner(&[hb("body one")]), on(None));
    assert_eq!(data(&r).dropped, vec![hb("body one")]);
}

/// [API §6.7]: `BodyDrop` is a write for `EnvCrash in-next`: applied, `outcome_unknown`, and the store without it kept.
#[test]
fn env_crash_in_next_treats_body_drop_as_a_write() {
    let mut s = two_bodies();
    s.ok(Cmd::EnvCrash { in_next: true }, Ctx::default());
    let h = hb("body one");
    let r = s.run(drop_owner(&[h]), on(None));
    let e = r.error.as_ref().expect("refused");
    assert_eq!((e.code.as_str(), r.exit), ("outcome_unknown", 7), "{e:?}");
    assert_eq!(s.st.runtime().dropped, vec![(h, DropReason::Secret)]);
    let without =
        s.st.without
            .as_ref()
            .expect("the store without the command");
    assert!(without.runtime().dropped.is_empty());
}

/// A three-line body whose line `i` (1-based) is `x` for each `(i, x)` of `edits`, else `l<i>`: one-line edits on
/// lines 1 and 3 leave line 2 stable, so diff3 merges them clean ([F12 §7.5]).
fn three(edits: &[(usize, &str)]) -> String {
    (1..=3)
        .map(|i| {
            edits
                .iter()
                .find(|(l, _)| *l == i)
                .map_or(format!("l{i}"), |(_, x)| x.to_string())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The `data` of a merge-family reply.
fn merged(r: &Reply) -> &crate::history::MergeData {
    match &r.data {
        Data::Merge(d) => d,
        other => panic!("not a merge result: {other:?}"),
    }
}

/// The state at a ref's tip.
fn tip_state(s: &S, r: &str) -> std::rc::Rc<crate::state::State> {
    let tip = s.st.dag.live(r).expect("the ref").tip;
    s.st.dag.state_at(tip, &s.st.alloc)
}

/// A `BranchCreate` of `lane/<name>` from `from`.
fn lane(s: &mut S, name: &str, from: &str) {
    s.ok(
        Cmd::BranchCreate {
            name: name.into(),
            from: Some(from.into()),
            kind: None,
        },
        on(None),
    );
}

/// The store with task #1 on `main`, whose body is X0 = `three(&[])`.
fn one_body() -> S {
    let mut s = S::base();
    s.ok(tx(vec![task_with_body("x", "task x", &three(&[]))]), orch());
    s
}

/// Runs `op` on a copy of the store without the drop, where #1's body merges clean to `clean` at `into`; then drops
/// `dropped` and runs `op` on the store, where #1's body is a `TextHunk` whose base, ours and theirs bodies hash to
/// `sides` (CS-011; MR-032, MR-038), listed in the reply's `conflicts`.
fn takes_a_text_hunk(s: &mut S, op: Cmd, into: &str, clean: &str, dropped: &str, sides: [&str; 3]) {
    let mut c = S { st: s.st.clone() };
    let r = c.ok(op.clone(), on(None));
    assert_eq!(
        merged(&r).conflicts,
        Vec::<(String, String)>::new(),
        "without the drop the body merges clean"
    );
    assert_eq!(
        tip_state(&c, into).nodes[&Nid(1)].body,
        Some(Body::Text(clean.into()))
    );
    s.ok(drop_owner(&[hb(dropped)]), on(None));
    let r = s.ok(op, on(None));
    assert_eq!(
        merged(&r).conflicts,
        vec![("#1.body".to_string(), "TextHunk".to_string())]
    );
    let st = tip_state(s, into);
    let k = &st.nodes[&Nid(1)].conflicts[&Aspect::Body];
    assert_eq!(k.class, "TextHunk");
    let h = |v: &Option<KVal>| match v {
        Some(KVal::Body(b)) => Some(body_hash(b)),
        other => panic!("not a body: {other:?}"),
    };
    assert_eq!(
        [h(&k.base), h(&k.ours), h(&k.theirs)],
        sides.map(|x| Some(hb(x))),
        "base, ours, theirs"
    );
}

/// CS-011 ([RULES/merge-table]; [F12 §7.5]): a merge whose base body was dropped takes `TextHunk` with the three
/// bodies, where it would have merged clean.
#[test]
fn a_merge_over_a_dropped_body_takes_a_text_hunk() {
    let mut s = one_body();
    lane(&mut s, "a", "main");
    lane(&mut s, "b", "main");
    let (x0, xa, xb) = (three(&[]), three(&[(1, "A")]), three(&[(3, "B")]));
    s.ok(tx(vec![set_body(1, &xa)]), on(Some("lane/a")));
    s.ok(tx(vec![set_body(1, &xb)]), on(Some("lane/b")));
    let op = Cmd::Merge {
        src: "lane/b".into(),
        into: Some("lane/a".into()),
        policy: None,
        strict: None,
        base: None,
        message: String::new(),
    };
    takes_a_text_hunk(
        &mut s,
        op,
        "lane/a",
        &three(&[(1, "A"), (3, "B")]),
        &x0,
        [&x0, &xa, &xb],
    );
}

/// CS-011: a sync whose base body was dropped takes `TextHunk` with the three bodies (ours the lane's, theirs
/// `main`'s).
#[test]
fn a_sync_over_a_dropped_body_takes_a_text_hunk() {
    let mut s = one_body();
    lane(&mut s, "x", "main");
    let (x0, xa, xb) = (three(&[]), three(&[(1, "A")]), three(&[(3, "B")]));
    s.ok(tx(vec![set_body(1, &xa)]), on(None));
    s.ok(tx(vec![set_body(1, &xb)]), on(Some("lane/x")));
    let op = Cmd::Sync {
        lane: Some("lane/x".into()),
        check: false,
    };
    takes_a_text_hunk(
        &mut s,
        op,
        "lane/x",
        &three(&[(1, "A"), (3, "B")]),
        &x0,
        [&x0, &xb, &xa],
    );
}

/// CS-011: a revert whose base, the reverted commit's body, was dropped takes `TextHunk` with the three bodies (base
/// the commit's, ours the tip's, theirs its parent's: DM-004).
#[test]
fn a_revert_over_a_dropped_body_takes_a_text_hunk() {
    let mut s = one_body();
    lane(&mut s, "x", "main");
    let (x0, xa, xa2) = (three(&[]), three(&[(1, "A")]), three(&[(1, "A"), (3, "C")]));
    let c = s
        .ok(tx(vec![set_body(1, &xa)]), on(Some("lane/x")))
        .commit
        .expect("C commits");
    s.ok(tx(vec![set_body(1, &xa2)]), on(Some("lane/x")));
    let op = Cmd::Revert {
        commit: format!("s{c}"),
        onto: Some("lane/x".into()),
        mainline: None,
        message: String::new(),
    };
    takes_a_text_hunk(
        &mut s,
        op,
        "lane/x",
        &three(&[(3, "C")]),
        &xa,
        [&xa, &xa2, &x0],
    );
}

/// CS-011: a cherry-pick whose base, the picked commit's parent's body, was dropped takes `TextHunk` with the three
/// bodies.
#[test]
fn a_cherry_pick_over_a_dropped_body_takes_a_text_hunk() {
    let mut s = one_body();
    lane(&mut s, "x", "main");
    lane(&mut s, "y", "main");
    let (x0, xa, xb) = (three(&[]), three(&[(1, "A")]), three(&[(3, "B")]));
    let c = s
        .ok(tx(vec![set_body(1, &xb)]), on(Some("lane/y")))
        .commit
        .expect("C commits");
    s.ok(tx(vec![set_body(1, &xa)]), on(Some("lane/x")));
    let op = Cmd::CherryPick {
        commit: format!("s{c}"),
        onto: Some("lane/x".into()),
        message: String::new(),
    };
    takes_a_text_hunk(
        &mut s,
        op,
        "lane/x",
        &three(&[(1, "A"), (3, "B")]),
        &x0,
        [&x0, &xa, &xb],
    );
}

/// A `body_dropped` refusal ([F19 §10.3]): exit 6, its keys in order `node`, `hash`, `reason`, `case`, and nothing
/// written: the store's commit count and `main`'s tip as before.
fn assert_body_dropped(
    s: &S,
    r: &Reply,
    n: u32,
    text: &str,
    reason: &str,
    case: &str,
    before: (u64, Option<u64>),
) {
    let e = r.error.as_ref().expect("refused");
    assert_eq!((e.code.as_str(), r.exit), ("body_dropped", 6), "{e:?}");
    assert_eq!(e.key_names(), ["node", "hash", "reason", "case"], "{e:?}");
    assert_eq!(e.get("node"), Some(&crate::err::Kv::Node(Nid(n))), "{e:?}");
    assert_eq!(e.get_str("hash"), Some(hx(text).as_str()), "{e:?}");
    assert_eq!(e.get_str("reason"), Some(reason), "{e:?}");
    assert_eq!(e.get_str("case"), Some(case), "{e:?}");
    assert_eq!(
        (s.st.commit_seq, s.st.dag.live("main").unwrap().tip),
        before,
        "nothing written"
    );
}

/// [F06 §8.1] DB-7 (b); [F19 §10.2] `body_dropped`: a write whose new body has the hash of a dropped body is refused
/// with `case` `supplied`, whichever door supplies it: a `TX` `set` and `create`, the `tx.add` mutation, an `Apply`
/// batch's statement, and a text that normalises to the dropped one ([F08 §5.3]).
#[test]
fn a_write_that_supplies_a_dropped_text_is_body_dropped() {
    let d = "dropped\ntext";
    let mut s = S::base();
    s.ok(
        tx(vec![
            task_with_body("a", "task a", d),
            task_with_body("b", "task b", "other"),
        ]),
        orch(),
    );
    s.ok(
        drop_raw(vec![hx(d)], "private", "", Some("owner")),
        on(None),
    );
    let before = (s.st.commit_seq, s.st.dag.live("main").unwrap().tip);
    let r = s.run(tx(vec![set_body(2, d)]), on(None));
    assert_body_dropped(&s, &r, 2, d, "private", "supplied", before);
    let r = s.run(tx(vec![task_with_body("c", "task c", d)]), on(None));
    assert_body_dropped(&s, &r, 3, d, "private", "supplied", before);
    let r = s.run(
        Cmd::Mutation {
            name: "tx.add".into(),
            params: vec![
                ("kind".into(), t("task")),
                ("title".into(), t("task c")),
                ("body".into(), t(d)),
            ],
            message: String::new(),
            move_lease: None,
        },
        on(None),
    );
    assert_body_dropped(&s, &r, 3, d, "private", "supplied", before);
    let r = s.run(
        Cmd::Apply {
            run: None,
            results: vec![],
            stmts: vec![set_body(2, d)],
            message: String::new(),
        },
        on(None),
    );
    assert_body_dropped(&s, &r, 2, d, "private", "supplied", before);
    let r = s.run(tx(vec![set_body(2, "dropped\r\ntext")]), on(None));
    assert_body_dropped(&s, &r, 2, d, "private", "supplied", before);
}

/// [LQ/std §7.2] `PATCH`: "A `PATCH` of a dropped body is `body_dropped` ([F06 §8.1] DB-7), checked first": before
/// E404's three cases, whether the removed text occurs once, never, twice, or is empty.
#[test]
fn a_patch_of_a_dropped_body_is_body_dropped_first() {
    let b = "one two two";
    let mut s = S::base();
    s.ok(tx(vec![task_with_body("a", "task a", b)]), orch());
    s.ok(drop_raw(vec![hx(b)], "other", "", Some("owner")), on(None));
    let before = (s.st.commit_seq, s.st.dag.live("main").unwrap().tip);
    for remove in ["one", "three", "two", ""] {
        let r = s.run(
            tx(vec![Stmt::Patch {
                target: Target::Id(Nid(1)),
                remove: remove.into(),
                add: "x".into(),
            }]),
            on(None),
        );
        assert_body_dropped(&s, &r, 1, b, "other", "patch", before);
    }
}

/// The rows of an LQ read on `main`.
fn query(s: &mut S, text: &str) -> Vec<Vec<crate::lq::eval::V>> {
    let r = s.run(
        Cmd::Query {
            input: crate::api::QueryInput::Lq(text.into()),
            params: crate::lq::ctx::Params::new(),
            at: None,
            mode: crate::lq::eval::Mode::Run,
            strict_gql: false,
            ablations: crate::lq::eval::Ablations::default(),
        },
        orch(),
    );
    match r.data {
        Data::Query(o) => o.rows,
        other => panic!("{text}: {other:?} {:?}", r.error),
    }
}

/// [F06 §8.1] DB-6; [API §8.7] "Effect": LQ reads a dropped body as `absent`; another node's body still reads as its
/// text.
#[test]
fn lq_reads_a_dropped_body_as_absent() {
    use crate::lq::eval::V;
    let mut s = two_bodies();
    s.ok(drop_owner(&[hb("body one")]), on(None));
    let rows = query(&mut s, "MATCH (n:task) RETURN n.id, n.body ORDER BY n.id");
    assert_eq!(
        rows,
        vec![
            vec![V::Node(Nid(1)), V::Absent],
            vec![V::Node(Nid(2)), V::text("body two")],
        ]
    );
}

/// [F06 §8.1] DB-6: search over the body field ([LQ/std §2.9] `fields`) reads a dropped body as no text, with no
/// tokens and length 0: a term only the dropped body holds finds nothing, and a term the title also holds scores as on
/// the same node without a body.
#[test]
fn search_reads_a_dropped_body_as_no_text() {
    use crate::lq::eval::V;
    let make = |body: Option<&str>| {
        let mut s = S::base();
        let c = match body {
            Some(b) => task_with_body("a", "alpha zeta", b),
            None => task("a", "alpha zeta"),
        };
        s.ok(tx(vec![c, task_with_body("b", "task b", "gamma")]), orch());
        s
    };
    let mut a = make(Some("alpha beta"));
    a.ok(drop_owner(&[hb("alpha beta")]), on(None));
    let mut b = make(None);
    let search = |s: &mut S, term: &str| {
        let call = format!("CALL search('{term}', fields: ['title', 'abstract', 'body'])");
        query(
            s,
            &format!("{call} YIELD node, score, field RETURN node, score, field"),
        )
    };
    assert_eq!(search(&mut a, "beta"), Vec::<Vec<V>>::new());
    let got = search(&mut a, "alpha");
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0][0], V::Node(Nid(1)));
    assert_eq!(got[0][2], V::text("title"));
    assert_eq!(
        got,
        search(&mut b, "alpha"),
        "the score of a node without a body"
    );
}

/// [F08 §10.4] with [F06 §8.1] DB-6 (WR-013): a write that recomputes a node's mentions reads its dropped body as no
/// text, so a title write drops the `mentions` edges only the dropped body named.
#[test]
fn a_title_write_drops_the_mentions_of_a_dropped_body() {
    let mut s = S::base();
    s.ok(
        tx(vec![
            task("a", "task a"),
            task_with_body("b", "task b", "see #1"),
        ]),
        orch(),
    );
    let mentions = |s: &S| {
        tip_state(s, "main").nodes[&Nid(2)]
            .out
            .keys()
            .any(|k| k.kind == "mentions" && k.dst == Nid(1))
    };
    assert!(mentions(&s), "the body names #1");
    s.ok(drop_owner(&[hb("see #1")]), on(None));
    s.ok(tx(vec![set(2, &[("title", t("plain"))])]), on(None));
    assert!(!mentions(&s), "a dropped body names nothing");
}
