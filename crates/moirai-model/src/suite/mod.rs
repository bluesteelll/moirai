//! The model's own suites for WP-90a: streams through the Store API ([API]), the node-40 rows of
//! [RULES/delete-policy-matrix] and the I26′ scenarios of [RULES/state-definition] as fixtures, and property tests
//! that compare the model's incremental answers with its from-scratch definitions ([PLAN §6.2] R12).

mod apply;
mod checks;
mod feed;
mod gt18;
mod hooks_heads;
mod n40;
mod props;
mod replays;
mod scale;
mod scenarios;
mod streams;
mod tags;
mod vcs;

use crate::api::{Cmd, Ctx, Outcome, Reply, Store};
use crate::clock::EnvSlots;
use crate::lq::ctx::Value as P;
use crate::tx::{Position, Stmt, Target};
use crate::value::Nid;

/// A stream under test.
pub struct S {
    /// The store.
    pub st: Store,
}

impl S {
    /// A store initialised with seed 42 and the test profile's init-fixed values, with the orchestrator's session role
    /// lease L-1 held by `orch` on the slot of `claude:s1` (the base stream of [API §19] example 02 through `n` = 3).
    pub fn base() -> S {
        let mut s = S { st: Store::new() };
        s.ok(
            Cmd::EnvSlots(EnvSlots {
                hold: vec!["claude:s1".into()],
                ..Default::default()
            }),
            Ctx::default(),
        );
        s.ok(
            Cmd::Init {
                seed: 42,
                params: vec![
                    "store.log-extent-bytes=64KiB".into(),
                    "store.hist-frame-commits=4".into(),
                    "store.hist-frame-bytes=4KiB".into(),
                    "store.commit.inline-max-bytes=4KiB".into(),
                ],
                default_branch: None,
            },
            Ctx::default(),
        );
        let mut ctx = Ctx {
            agent: Some("orch".into()),
            ..Default::default()
        };
        ctx.env.insert("CLAUDECODE".into(), "1".into());
        ctx.env.insert("CLAUDE_CODE_SESSION_ID".into(), "s1".into());
        s.ok(
            Cmd::Claim {
                ids: vec![],
                next: false,
                scope: None,
                role: Some("orchestrator".into()),
                agent: None,
                ttl: None,
                start: false,
                run: None,
                session: true,
            },
            ctx,
        );
        s
    }

    /// Runs a command and returns its reply.
    pub fn run(&mut self, c: Cmd, ctx: Ctx) -> Reply {
        self.st.run(&c, &ctx)
    }

    /// Runs a command that must succeed.
    pub fn ok(&mut self, c: Cmd, ctx: Ctx) -> Reply {
        let r = self.st.run(&c, &ctx);
        assert!(
            matches!(r.outcome, Outcome::Ok | Outcome::Replayed | Outcome::Dry),
            "{c:?} refused: {:?}",
            r.error
        );
        r
    }

    /// Runs a command that must be refused with `code`.
    pub fn refused(&mut self, c: Cmd, ctx: Ctx, code: &str) -> Reply {
        let r = self.st.run(&c, &ctx);
        assert_eq!(r.outcome, Outcome::Refused, "{c:?} was not refused");
        assert_eq!(
            r.error.as_ref().map(|e| e.code.as_str()),
            Some(code),
            "{:?}",
            r.error
        );
        r
    }
}

/// The orchestrator's context: lease L-1 presented, from a Claude Code session (`--client claude`), whose model
/// profile is the Claude default family's ([90 §8.2]; [CFG §10.9]), so its free-form `TX` blocks are not E411.
pub fn orch() -> Ctx {
    Ctx {
        lease: Some("L-1".into()),
        client: Some("claude".into()),
        ..Default::default()
    }
}

/// The orchestrator on another branch.
pub fn orch_on(branch: &str) -> Ctx {
    Ctx {
        branch: Some(branch.into()),
        ..orch()
    }
}

/// A `create` of a task.
pub fn task(name: &str, title: &str) -> Stmt {
    Stmt::Create {
        name: Some(name.into()),
        kind: "task".into(),
        fields: vec![("title".into(), P::Text(title.into()))],
        body: None,
        under: None,
        position: None,
        edges_out: vec![],
        edges_in: vec![],
    }
}

/// A `create` of a node of any kind with fields.
pub fn node(name: &str, kind: &str, fields: &[(&str, P)]) -> Stmt {
    Stmt::Create {
        name: Some(name.into()),
        kind: kind.into(),
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        body: None,
        under: None,
        position: None,
        edges_out: vec![],
        edges_in: vec![],
    }
}

/// A `create` under a parent at the last position.
pub fn child(name: &str, title: &str, parent: Target) -> Stmt {
    Stmt::Create {
        name: Some(name.into()),
        kind: "task".into(),
        fields: vec![("title".into(), P::Text(title.into()))],
        body: None,
        under: Some(parent),
        position: Some(Position::Last),
        edges_out: vec![],
        edges_in: vec![],
    }
}

/// A `set` of fields.
pub fn set(n: u32, fields: &[(&str, P)]) -> Stmt {
    Stmt::Set {
        target: Target::Id(Nid(n)),
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
        incr: vec![],
        body: None,
        guard: None,
    }
}

/// A `link`.
pub fn link(a: u32, kind: &str, b: u32) -> Stmt {
    Stmt::Link {
        src: Target::Id(Nid(a)),
        kind: kind.into(),
        dst: Target::Id(Nid(b)),
        pinned: None,
    }
}

/// A `Tx` command.
pub fn tx(stmts: Vec<Stmt>) -> Cmd {
    Cmd::Tx {
        stmts,
        message: String::new(),
    }
}

/// Text value.
pub fn t(s: &str) -> P {
    P::Text(s.into())
}
