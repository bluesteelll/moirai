//! Hooks as the model sees them ([AR §7.5]; [API §18] "Hooks that write"; [RULES/role-write-policy] `role-hooks`
//! WH-001 to WH-008, WR-014; [CFG §10.6], §10.7): which hooks `hooks install` registers for a harness, and, for each
//! hook event, exactly the writes its `role-hooks` row lists — as Store API commands through the door `hook` where the
//! model runs them (the link settle is `LinksSync`, [API §18]), as lazy records outside the API ([API] open point 25),
//! or as the commands of the milestone that builds them (the checkpoint export, M5). A hook handler never executes write
//! text a model supplied (WR-014): it composes its commands itself.

use crate::api::{Cmd, Ctx, Door, StampCtx, Store};
use crate::lease::LeaseKind;
use crate::lq::ctx::Value as P;
use crate::registry::Conf;

/// One write (or read) a hook performs.
#[derive(Clone, Debug, PartialEq)]
pub enum Write {
    /// A Store API command the model runs, with its context (door `hook`).
    Command(Box<Cmd>, Box<Ctx>),
    /// A lazy record outside the API at M0 ([API] open point 25): `session-cursor`, `session-mark`, `runtime-evidence`
    /// (the evidence hooks' `PENDING`, `FILEOBS` and `ANCHORRES` rows and the tree's dirty row), `binding-refresh` (the
    /// displayed provenance of the tree's `TREES` row).
    Lazy(&'static str),
    /// A command a later milestone builds: (what, milestone) — the checkpoint export (M5).
    Later(&'static str, &'static str),
    /// A read the hook renders: `brief`, `role-pack`, `delta`.
    Read(&'static str),
}

/// The hooks a harness gets from `hooks install` ([CFG §10.7] `hooks.<hook>.enabled`, `hooks.transport`; [CFG §10.6]
/// `files.hooks.evidence`, `files.hooks.edit-evidence`): each enabled hook, `agent-launched` and `stamp` for Claude
/// Code only, the `mv`/`rm` evidence hook when `files.hooks.evidence`, and the `Write|Edit` evidence hook when
/// `files.hooks.edit-evidence` is `on`, or `auto` with the `mcp_tool` transport (`hooks.transport = auto` is `mcp_tool`
/// in Claude Code and Codex, command hooks elsewhere).
// spec: [CFG §10.7]
// spec: [CFG §10.6] files.hooks.evidence, files.hooks.edit-evidence
pub fn installed(conf: &Conf, harness: &str) -> Vec<&'static str> {
    let mut v = Vec::new();
    for (hook, claude_only) in [
        ("session-start", false),
        ("user-prompt-submit", false),
        ("subagent-start", false),
        ("agent-launched", true),
        ("subagent-stop", false),
        ("stamp", true),
    ] {
        if (!claude_only || harness == "claude") && conf.flag(&format!("hooks.{hook}.enabled")) {
            v.push(hook);
        }
    }
    if conf.flag("files.hooks.evidence") {
        v.push("mv-rm-evidence");
    }
    let mcp = match conf.text("hooks.transport").as_str() {
        "auto" => harness == "claude" || harness == "codex",
        t => t == "mcp",
    };
    let edit = conf.text("files.hooks.edit-evidence");
    if edit == "on" || (edit == "auto" && mcp) {
        v.push("write-edit-evidence");
    }
    v
}

/// How a session started ([AR §7.5] `SessionStart` rows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// `startup`.
    Startup,
    /// `resume`.
    Resume,
    /// `clear`.
    Clear,
    /// `compact`.
    Compact,
}

/// `SessionStart` (WH-001; [AR §7.5]; [90 §4.3] mint (i), §7.5): in a main session the orchestrator's session role lease
/// (`hooks.session-start.orchestrator-lease`), a dispatched worker none; the link settle (`hooks.session-start.settle`:
/// `LinksSync` through the door `hook` in the session's tree, its budget `files.session-start-cap-ms`, [API §18]); the
/// checkpoint export when the last one is older than `image.export.max-age` (M5); the session cursor (lazy); and the
/// read it renders: a worker's role pack (`hooks.session-start.worker-pack`), the delta on resume, the brief otherwise.
/// `ctx` is the session's context (its environment and stamp).
// spec: [AR §7.5] SessionStart
// rule: WH-001
pub fn session_start(
    conf: &Conf,
    source: Source,
    worker: bool,
    export_due: bool,
    ctx: &Ctx,
) -> Vec<Write> {
    let mut v = Vec::new();
    if !worker && conf.flag("hooks.session-start.orchestrator-lease") {
        let c = Cmd::Claim {
            ids: vec![],
            next: false,
            scope: None,
            role: Some("orchestrator".into()),
            agent: None,
            ttl: None,
            start: false,
            run: None,
            session: true,
        };
        v.push(Write::Command(
            Box::new(c),
            Box::new(Ctx {
                door: Door::Hook,
                ..ctx.clone()
            }),
        ));
    }
    if conf.flag("hooks.session-start.settle") {
        v.push(link_settle(conf.number("files.session-start-cap-ms"), ctx));
    }
    if export_due {
        v.push(Write::Later("image-export-checkpoint", "M5"));
    }
    v.push(Write::Lazy("session-cursor"));
    v.push(Write::Read(match (worker, source) {
        (true, _) if conf.flag("hooks.session-start.worker-pack") => "role-pack",
        (_, Source::Resume) => "delta",
        _ => "brief",
    }));
    v
}

/// A `sync --check` preview of the bound lane ([AR §7.5] `SubagentStart`): conflicts, violations and keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncPreview {
    /// Conflicts.
    pub conflicts: u64,
    /// Violations.
    pub violations: u64,
    /// Keys the sync would write.
    pub keys: u64,
}

/// `SubagentStart` (WH-002; [AR §7.5]): the session mark of the rules shown (lazy), the role pack, and the clean
/// auto-sync of the bound lane — `Sync` of that lane through the door `hook` — only with
/// `hooks.subagent-start.auto-sync`, zero conflicts, zero violations and at most `hooks.sync-auto-keys` keys (0 never
/// applies). `preview` is the bound lane with its `sync --check` ([`Store::sync_preview`]).
// spec: [AR §7.5] SubagentStart
// spec: [CFG §10.7] hooks.subagent-start.auto-sync, hooks.sync-auto-keys
// rule: WH-002
pub fn subagent_start(conf: &Conf, preview: Option<(String, SyncPreview)>) -> Vec<Write> {
    let mut v = vec![Write::Lazy("session-mark"), Write::Read("role-pack")];
    let limit = conf.number("hooks.sync-auto-keys");
    if let Some((lane, p)) = preview
        && conf.flag("hooks.subagent-start.auto-sync")
        && p.conflicts == 0
        && p.violations == 0
        && p.keys > 0
        && p.keys <= limit
    {
        v.push(Write::Command(
            Box::new(Cmd::Sync {
                lane: Some(lane.clone()),
                check: false,
            }),
            Box::new(Ctx {
                door: Door::Hook,
                branch: Some(lane),
                ..Ctx::default()
            }),
        ));
    }
    v
}

/// `UserPromptSubmit` (WH-003): the session cursor (lazy, skipped when the writer byte is busy) and the delta it
/// renders.
// rule: WH-003
pub fn user_prompt_submit() -> Vec<Write> {
    vec![Write::Lazy("session-cursor"), Write::Read("delta")]
}

/// `SubagentStop` (WH-004, LE-010; [AR §7.5]): every live task lease the stopping agent holds is released through the
/// hook (`Release`, presented by the agent's own stamp), and, when one was still open, the agent's last message is
/// stored as a `needs-triage` note naming the task (`#N` in its text makes the `mentions` edge), written by the named
/// mutation `tx.remember` under the `general-purpose` row ([RULES/role-write-policy] WH-004).
// spec: [AR §7.5] SubagentStop
// rule: WH-004, LE-010
pub fn subagent_stop(st: &Store, session: &str, agent_id: &str, last_message: &str) -> Vec<Write> {
    let holder = format!("claude:{agent_id}");
    let mut v = Vec::new();
    let mut open = Vec::new();
    for l in st.leases.values().filter(|l| {
        l.kind == LeaseKind::Task
            && l.holder == holder
            && l.ended.is_none()
            && crate::lease::is_live(l, &st.env).is_live()
    }) {
        let ctx = Ctx {
            door: Door::Hook,
            client: Some("claude".into()),
            stamp: Some(StampCtx {
                session_id: Some(session.into()),
                agent_id: Some(agent_id.into()),
                agent_type: None,
                cwd: None,
            }),
            ..Ctx::default()
        };
        v.push(Write::Command(
            Box::new(Cmd::Release {
                lease: format!("L-{}", l.id),
            }),
            Box::new(ctx),
        ));
        if let Some(t) = l.task {
            open.push(t);
        }
    }
    if !open.is_empty() {
        let ids: Vec<String> = open.iter().map(|n| n.to_string()).collect();
        let title = format!(
            "needs-triage: {} stopped holding {}",
            holder,
            ids.join(", ")
        );
        let c = Cmd::Mutation {
            name: "tx.remember".into(),
            params: vec![
                ("kind".into(), P::Text("note".into())),
                ("title".into(), P::Text(title)),
                ("text".into(), P::Text(last_message.to_string())),
            ],
            message: String::new(),
            move_lease: None,
        };
        v.push(Write::Command(
            Box::new(c),
            Box::new(Ctx {
                door: Door::Hook,
                client: Some("claude".into()),
                ..Ctx::default()
            }),
        ));
    }
    v
}

/// `agent-launched` (WH-005): an in-memory map only; no write.
// rule: WH-005
pub fn agent_launched() -> Vec<Write> {
    Vec::new()
}

/// The stamp's `permissionDecision` for a write of a class ([CFG §10.7] `hooks.stamp.permission`,
/// `hooks.stamp.ask-for`; WH-006: no write): `ask` when the permission is `ask`, or when the write's class (an
/// owner-authority write, an edge delete, a links confirm) is in `ask-for`; `allow` otherwise.
// spec: [CFG §10.7] hooks.stamp.permission, hooks.stamp.ask-for
// rule: WH-006
pub fn stamp(conf: &Conf, class: Option<&str>) -> &'static str {
    let ask_for = conf.text("hooks.stamp.ask-for");
    if conf.text("hooks.stamp.permission") == "ask"
        || class.is_some_and(|c| ask_for.split(',').any(|x| x == c))
    {
        "ask"
    } else {
        "allow"
    }
}

/// The link settle a hook runs ([API §18]): `LinksSync` of the session's tree through the door `hook`, without a key (a
/// settle must run again when the tree changed, [API §7.1]).
fn link_settle(budget_ms: u64, ctx: &Ctx) -> Write {
    Write::Command(
        Box::new(Cmd::LinksSync {
            scope: None,
            budget_ms: Some(budget_ms),
            since: None,
            deep: false,
            all: false,
            force: false,
        }),
        Box::new(Ctx {
            door: Door::Hook,
            key: None,
            ..ctx.clone()
        }),
    )
}

/// The file-evidence hooks (WH-007): runtime evidence rows (`PENDING`, `FILEOBS`, `ANCHORRES`, the tree's dirty row) as
/// lazy records, never a versioned write; outside the API at M0 ([API §18], open point 25).
// rule: WH-007
pub fn fs_evidence() -> Vec<Write> {
    vec![Write::Lazy("runtime-evidence")]
}

/// The git hooks (WH-008): a link settle of the committed paths (`LinksSync` through the door `hook`) and the binding
/// refresh, which refreshes the displayed provenance of the tree's `TREES` row (a lazy record) and never the binding's
/// expected ref or base ([F18 §3.5] "Never changed by anything else").
// rule: WH-008
pub fn git_hooks(conf: &Conf, ctx: &Ctx) -> Vec<Write> {
    vec![
        link_settle(conf.number("files.links-sync-ms"), ctx),
        Write::Lazy("binding-refresh"),
    ]
}

/// Runs the commands of a hook's writes on the store, in order ([API §18]); returns the replies.
// rule: WR-014
pub fn run(st: &mut Store, writes: &[Write]) -> Vec<crate::api::Reply> {
    writes
        .iter()
        .filter_map(|w| match w {
            Write::Command(c, ctx) => Some(st.run(c, ctx)),
            _ => None,
        })
        .collect()
}
