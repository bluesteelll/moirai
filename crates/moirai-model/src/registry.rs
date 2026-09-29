//! The configuration registry of [CFG §10] as data: every key pattern with its type, default, scope, reload class,
//! visibility class and environment variable ([CFG §9.1], §9.4), the retired names of [CFG §6.4] and the policy-data
//! names of [CFG §10.13]; key matching ([CFG §3.3] rule 5), value validation ([CFG §4]), default resolution
//! ([CFG §5.1] rule 5 with per-parameter and derived defaults), the sweep sets of [CFG §9.5], and the model's
//! configuration files with their resolution ([CFG §5.1]) — the configuration snapshot the model takes with each
//! command ([CFG §9.4]).
//!
//! The model has no flags and no environment for configuration (DT-3): a stream sets keys with `Init`'s `params` and
//! `ConfigSet`, which write the simulated store and user files ([API §8.1], §8.2).

use crate::config::{D, GIB, H, KIB, MIB, MIN, Parsed, S, Ty, parse};
use std::collections::BTreeMap;

/// The scope of a key ([CFG §2.2]) with its markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// `store`.
    Store,
    /// `store, user-lower`: a user value can only lower the store value ([CFG §5.1] "User-lower").
    StoreUserLower,
    /// `user`.
    User,
    /// `user, q`: qualifiable ([CFG §2.3]).
    UserQ,
}

impl Scope {
    /// Whether the key is read from the user file.
    pub fn is_user(self) -> bool {
        matches!(self, Scope::User | Scope::UserQ)
    }

    /// The registry's text ([CFG §9.3]).
    pub fn token(self) -> &'static str {
        match self {
            Scope::Store => "store",
            Scope::StoreUserLower => "store,user-lower",
            Scope::User => "user",
            Scope::UserQ => "user,q",
        }
    }
}

/// The reload class of a key ([CFG §5.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reload {
    /// `hot`.
    Hot,
    /// `restart`.
    Restart,
    /// `init`.
    Init,
    /// `install`.
    Install,
}

/// The visibility class of a key ([CFG §9.4]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vis {
    /// Changes a `Store` API result, an exit class, a commit id or a state digest: the model implements it.
    V,
    /// Physical layout, time, caches or private memory only.
    I,
    /// A resource-class refusal.
    Rs,
    /// A budget.
    B,
    /// Output only.
    O,
    /// Harness integration or effects outside the store.
    X,
}

impl Vis {
    /// The class's name as [CFG §9.4] and [RULES/policy-keys] write it.
    pub fn token(self) -> &'static str {
        match self {
            Vis::V => "V",
            Vis::I => "I",
            Vis::Rs => "Rs",
            Vis::B => "B",
            Vis::O => "O",
            Vis::X => "X",
        }
    }
}

/// The vocabulary of a key pattern's one parameter segment ([CFG §4.2]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Param {
    /// `<name>` of `roots.<name>`: [F08 §5.4.1]'s root name, starting with a letter; `project` and `abs` reserved.
    Root,
    /// `<name>` of `image.dest.<name>.*`: a destination name.
    Dest,
    /// `<role>`: any word.
    Role,
    /// `<client>`: `claude`, `codex`, `generic`.
    Client,
    /// `<kind>`: a node kind name.
    Kind,
    /// `<family>`: a model family; `default` and `unknown` reserved.
    Family,
    /// `<profile>`: `gated`, `compatible`, `unknown`.
    Profile,
}

impl Param {
    /// Whether a segment is a valid value of this vocabulary.
    // spec: [CFG §4.2]
    pub fn accepts(self, seg: &str) -> bool {
        let word = crate::config::is_word(seg);
        match self {
            Param::Root => {
                word && seg.as_bytes()[0].is_ascii_lowercase() && seg != "project" && seg != "abs"
            }
            Param::Dest | Param::Role | Param::Kind => word,
            Param::Client => matches!(seg, "claude" | "codex" | "generic"),
            Param::Family => word && seg != "default" && seg != "unknown",
            Param::Profile => matches!(seg, "gated" | "compatible" | "unknown"),
        }
    }

    /// The instance the model's sweeps use for this parameter ([CFG §9.5]).
    pub fn representative(self) -> &'static str {
        match self {
            Param::Root => "docs",
            Param::Dest => "default",
            Param::Role => "developer",
            Param::Client => "codex",
            Param::Kind => "task",
            Param::Family => "claude-opus-5-5",
            Param::Profile => "unknown",
        }
    }
}

/// The kind of process a value is resolved for (the MCP server or a CLI or hook process), which `rule:agent-max-mem`
/// reads ([CFG §10.5]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proc {
    /// A CLI or hook process.
    Cli,
    /// The MCP server.
    Mcp,
}

/// A registry default ([CFG §9.1] `default`).
#[derive(Clone, Copy, Debug)]
pub enum Dflt {
    /// A canonical value.
    Val(&'static str),
    /// `none`: the key has no value unless set.
    None,
    /// `key:<other>`: the effective value of another key.
    Key(&'static str),
    /// `rule:<name>`: a rule [CFG] defines; [`rule_value`] gives the model's value.
    Rule(&'static str),
    /// `HOLE(<id>)`: decided by an M0 measurement (WP-81a); the model uses the design value given here until then.
    Hole(&'static str, &'static str),
    /// `per-param`: `(param value, default)` pairs, the last one `*`.
    PerParam(&'static [(&'static str, Dflt)]),
}

/// One registry row ([CFG §9.1], with the specification column `vis` of §9.4).
#[derive(Clone, Copy, Debug)]
pub struct Def {
    /// The key pattern.
    pub key: &'static str,
    /// The vocabulary of its parameter segment, when it has one.
    pub param: Option<Param>,
    /// The type.
    pub ty: Ty,
    /// The default.
    pub default: Dflt,
    /// The scope.
    pub scope: Scope,
    /// The reload class.
    pub reload: Reload,
    /// The visibility class.
    pub vis: Vis,
    /// The environment variable ([CFG §10.12]).
    pub env: Option<&'static str>,
    /// The [F17 §12] test-profile value, where one exists.
    pub test: Option<&'static str>,
}

const fn def(key: &'static str, ty: Ty, default: Dflt, vis: Vis) -> Def {
    Def {
        key,
        param: None,
        ty,
        default,
        scope: Scope::Store,
        reload: Reload::Hot,
        vis,
        env: None,
        test: None,
    }
}

impl Def {
    const fn param(mut self, p: Param) -> Def {
        self.param = Some(p);
        self
    }
    const fn scope(mut self, s: Scope) -> Def {
        self.scope = s;
        self
    }
    const fn reload(mut self, r: Reload) -> Def {
        self.reload = r;
        self
    }
    const fn env(mut self, e: &'static str) -> Def {
        self.env = Some(e);
        self
    }
    const fn test(mut self, t: &'static str) -> Def {
        self.test = Some(t);
        self
    }
}

use Dflt::{Hole, Key, PerParam, Rule, Val};
use Scope::{StoreUserLower, User, UserQ};
use Vis::{B, I, O, Rs, V, X};

const E10: i64 = 10_000_000_000;
const TOOLS: [&str; 10] = [
    "brief", "pack", "get", "query", "changes", "branch", "claim", "complete", "remember", "write",
];

/// Every key pattern of [CFG §10], in the order of its tables.
pub const DEFS: &[Def] = &[
    // §10.1 Discovery, paths and roots.
    def("discovery.git-hint", Ty::Bool, Val("true"), X)
        .scope(User)
        .env("MOIRAI_GIT_HINT"),
    def("default-branch", Ty::Ref, Val("main"), V),
    def("roots.<name>", Ty::Path, Dflt::None, V)
        .param(Param::Root)
        .scope(UserQ),
    def("files.main-tree", Ty::Path, Rule("main-tree"), V).scope(UserQ),
    def("files.main-ref", Ty::GitRef, Rule("main-ref"), V),
    def(
        "files.cloud",
        Ty::Enum(&["metadata-only", "refuse"]),
        Val("metadata-only"),
        V,
    )
    .scope(UserQ),
    // §10.2 Store parameters ([F17 §3] P01–P34; HOLEs read the design figure in [F17]'s brackets).
    def(
        "store.log-extent-bytes",
        Ty::SizePow2(1 << 16, 1 << 30),
        Val("64MiB"),
        I,
    )
    .reload(Reload::Init)
    .test("64KiB"),
    def("store.log-active-extents", Ty::Int(1, 64), Val("4"), I).test("2"),
    def(
        "store.hist-frame-commits",
        Ty::Int(1, 65_536),
        Val("256"),
        I,
    )
    .reload(Reload::Init)
    .test("4"),
    def(
        "store.hist-frame-bytes",
        Ty::Size(4 * KIB, MIB),
        Val("1MiB"),
        I,
    )
    .reload(Reload::Init)
    .test("4KiB"),
    def(
        "store.commit.inline-max-bytes",
        Ty::Size(4 * KIB, GIB / 8),
        Val("1MiB"),
        I,
    )
    .test("4KiB"),
    def(
        "store.checkpoint.ops",
        Ty::Int(1, 1 << 20),
        Hole("F17-ckpt-ops", "4096"),
        I,
    )
    .test("8"),
    def(
        "store.checkpoint.bytes",
        Ty::Size(KIB, GIB),
        Hole("F17-ckpt-bytes", "4MiB"),
        I,
    )
    .test("2KiB"),
    def(
        "store.checkpoint.body-bytes",
        Ty::Size(KIB, 1 << 32),
        Hole("F17-ckpt-body", "32MiB"),
        I,
    )
    .test("8KiB"),
    def(
        "store.tail.max-overlay-bytes",
        Ty::Size(512, 1 << 26),
        Hole("F17-tail-overlay", "1MiB"),
        I,
    )
    .test("1KiB"),
    def(
        "store.tail.max-overlay-bytes.quiet",
        Ty::Size(512, 1 << 26),
        Hole("F17-tail-overlay-quiet", "2MiB"),
        I,
    )
    .test("2KiB"),
    def(
        "quiet.tail-cap-multiplier",
        Ty::Int(1, 64),
        Hole("F17-quiet-mult", "8"),
        I,
    )
    .test("2"),
    def(
        "store.tail.runtime-bytes",
        Ty::Size(512, 1 << 26),
        Val("2MiB"),
        I,
    )
    .test("1KiB"),
    def(
        "maintenance.cli-threshold-multiplier",
        Ty::Int(1, 16),
        Val("2"),
        I,
    )
    .test("2"),
    def("store.fold-width", Ty::Int(1, 5), Val("3"), I).test("2"),
    def(
        "maintenance.rollup-threshold",
        Ty::Percent(1, 1000),
        Val("25"),
        I,
    )
    .test("25"),
    def(
        "store.dict.train-sample-bytes",
        Ty::Size(4 * KIB, 1 << 26),
        Val("4MiB"),
        I,
    )
    .test("4KiB"),
    def(
        "store.dict.retrain-growth",
        Ty::Percent(1, 1000),
        Val("25"),
        I,
    )
    .test("25"),
    def(
        "store.fts.tier2-nodes",
        Ty::Int(1, (1 << 32) - 1),
        Val("20000"),
        I,
    )
    .test("16"),
    def(
        "store.promotion.overlay-ops",
        Ty::Int(1, (1 << 32) - 2),
        Hole("F17-promo-ops", "4000"),
        I,
    )
    .test("16"),
    def(
        "store.promotion.overlay-bytes",
        Ty::Size(KIB, (1 << 32) - 2),
        Hole("F17-promo-bytes", "1MiB"),
        I,
    )
    .test("1KiB"),
    def(
        "store.promotion.age-checkpoints",
        Ty::Int(1, 65_536),
        Hole("F17-promo-age", "16"),
        I,
    )
    .test("2"),
    def(
        "store.kahn-fallback-edges",
        Ty::Int(0, (1 << 32) - 1),
        Val("1000"),
        I,
    )
    .test("4"),
    def(
        "store.suspect-budget",
        Ty::Int(1, (1 << 32) - 1),
        Val("10000"),
        V,
    )
    .test("4"),
    def(
        "store.image.loose-pack-threshold",
        Ty::Int(0, 65_536),
        Hole("F17-loose-pack", "8"),
        I,
    )
    .test("2"),
    def(
        "store.pack-objects-max",
        Ty::Int(16, 1 << 24),
        Val("65536"),
        I,
    )
    .test("16"),
    def(
        "lock.writer-wait-ms",
        Ty::Int(100, 60_000),
        Hole("F17-lock-writer", "2000"),
        Rs,
    ),
    def(
        "lock.flush-wait-ms",
        Ty::Int(100, 60_000),
        Hole("F17-lock-flush", "2000"),
        Rs,
    ),
    def(
        "idempotency.retention",
        Ty::Duration(S, 3650 * D),
        Val("30d"),
        V,
    )
    .test("1h"),
    def(
        "idempotency.default-window",
        Ty::Duration(S, 3650 * D),
        Val("10m"),
        V,
    )
    .test("1m"),
    def("gc.reflog-expire", Ty::Duration(H, 3650 * D), Val("90d"), V).test("2h"),
    def("gc.cruft-delay", Ty::Duration(0, 3650 * D), Val("14d"), V).test("30m"),
    def("gc.trash-expire", Ty::Duration(0, 3650 * D), Val("14d"), I).test("30m"),
    def(
        "gc.fileobs-idle-expire",
        Ty::Duration(H, 3650 * D),
        Val("30d"),
        I,
    )
    .test("1h"),
    def("gc.delete-grace", Ty::Duration(0, H), Val("1m"), I).test("1s"),
    // §10.3 Durability, quiet mode, maintenance, leases.
    def(
        "durability.lazy-kinds",
        Ty::Set(&["heartbeat", "cursor", "session-mark"]),
        Val("heartbeat,cursor,session-mark"),
        V,
    ),
    def("quiet.from-lane-measuring", Ty::Bool, Val("true"), V),
    def(
        "maintenance.rollup",
        Ty::Enum(&["auto", "explicit"]),
        Val("auto"),
        I,
    ),
    def(
        "lease.ttl-default",
        Ty::Duration(MIN, 30 * D),
        Val("15m"),
        V,
    ),
    def(
        "lease.reclaim-older-than",
        Ty::Duration(MIN, 3650 * D),
        Val("30m"),
        V,
    ),
    def(
        "lease.orchestrator-ttl",
        Ty::Duration(MIN, 30 * D),
        Val("12h"),
        V,
    ),
    def("backup.max-age", Ty::Duration(H, 3650 * D), Val("1d"), O),
    // §10.4 Memory.
    def("mcp.overlay-bytes", Ty::Size(0, 256 * MIB), Val("4MiB"), I),
    def(
        "mcp.overlay-bytes.<client>",
        Ty::Size(0, 256 * MIB),
        PerParam(&[("codex", Val("0")), ("*", Key("mcp.overlay-bytes"))]),
        I,
    )
    .param(Param::Client),
    def("mcp.overlay-lru", Ty::Int(1, 8), Val("8"), I),
    def(
        "git.delta-cache-bytes.cli",
        Ty::Size(0, 64 * MIB),
        Val("256KiB"),
        I,
    ),
    def(
        "git.delta-cache-bytes.mcp",
        Ty::Size(0, 64 * MIB),
        Val("1MiB"),
        I,
    ),
    def("mem.rss-gate.cli", Ty::Size(MIB, GIB), Val("4000000"), Rs),
    def(
        "mem.rss-gate.cli-per-view",
        Ty::Size(0, 64 * MIB),
        Val("1MiB"),
        Rs,
    ),
    def("mem.rss-gate.mcp", Ty::Size(MIB, GIB), Val("15625KiB"), Rs),
    def(
        "files.max-read-bytes",
        Ty::Size(128 * KIB, 4 * GIB),
        Val("16MiB"),
        V,
    ),
    def(
        "files.max-line-hashes",
        Ty::Int(1024, 16_777_216),
        Val("65536"),
        V,
    ),
    def("files.deep.threads", Ty::Int(1, 64), Val("8"), I),
    def("files.deep.content-readers", Ty::Int(1, 8), Val("2"), I),
    // §10.5 Queries and TX: the budget defaults and ceilings.
    def(
        "query.budget.default.work",
        Ty::Int(1, E10),
        Val("2000000"),
        B,
    ),
    def(
        "query.budget.default.mem",
        Ty::Size(256 * KIB, GIB),
        Val("1MiB"),
        B,
    ),
    def(
        "query.budget.default.wmem",
        Ty::Size(256 * KIB, GIB),
        Val("1MiB"),
        B,
    ),
    def(
        "query.budget.default.rows",
        Ty::Int(1, 1_000_000),
        Val("50"),
        B,
    ),
    def(
        "query.budget.default.bytes",
        Ty::Size(1000, 10_000_000),
        Val("8000"),
        B,
    ),
    def(
        "query.budget.default.visited",
        Ty::Int(1, 1_000_000_000),
        Val("100000"),
        B,
    ),
    def("query.budget.default.refs", Ty::Int(1, 80), Val("4"), B),
    def(
        "query.budget.default.fs",
        Ty::Int(1, 1_000_000_000),
        Val("400"),
        B,
    ),
    def(
        "query.budget.default.deadline-cli",
        Ty::Duration(100, H),
        Val("2s"),
        B,
    ),
    def(
        "query.budget.default.deadline-mcp",
        Ty::Duration(100, H),
        Val("5s"),
        B,
    ),
    def(
        "query.caps.<role>.work",
        Ty::Int(1, E10),
        PerParam(&[
            ("orchestrator", Val("200000000")),
            ("owner", Val("200000000")),
            ("*", Val("20000000")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.mem",
        Ty::Size(256 * KIB, GIB),
        PerParam(&[
            ("orchestrator", Rule("agent-max-mem-x10")),
            ("owner", Rule("agent-max-mem-x10")),
            ("*", Rule("agent-max-mem")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.wmem",
        Ty::Size(256 * KIB, GIB),
        PerParam(&[
            ("orchestrator", Val("40MiB")),
            ("owner", Val("40MiB")),
            ("*", Val("4MiB")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.rows",
        Ty::Int(1, 1_000_000),
        PerParam(&[
            ("orchestrator", Val("5000")),
            ("owner", Val("5000")),
            ("*", Val("500")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.bytes",
        Ty::Size(1000, 10_000_000),
        PerParam(&[
            ("orchestrator", Val("240000")),
            ("owner", Val("240000")),
            ("*", Val("24000")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.visited",
        Ty::Int(1, 1_000_000_000),
        PerParam(&[
            ("orchestrator", Val("10000000")),
            ("owner", Val("10000000")),
            ("*", Val("1000000")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.refs",
        Ty::Int(1, 80),
        PerParam(&[
            ("orchestrator", Val("80")),
            ("owner", Val("80")),
            ("*", Val("8")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.fs",
        Ty::Int(1, 1_000_000_000),
        PerParam(&[
            ("orchestrator", Val("100000")),
            ("owner", Val("100000")),
            ("*", Val("10000")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.deadline-cli",
        Ty::Duration(100, H),
        PerParam(&[
            ("orchestrator", Val("20s")),
            ("owner", Val("20s")),
            ("*", Val("2s")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.caps.<role>.deadline-mcp",
        Ty::Duration(100, H),
        PerParam(&[
            ("orchestrator", Val("50s")),
            ("owner", Val("50s")),
            ("*", Val("5s")),
        ]),
        B,
    )
    .param(Param::Role),
    def(
        "query.safelist.<role>",
        Ty::Enum(&["off", "named-only"]),
        Val("off"),
        V,
    )
    .param(Param::Role),
    def(
        "query.asof.max-ops.cli",
        Ty::Int(0, 10_000_000),
        Val("16000"),
        B,
    ),
    def(
        "query.asof.max-ops.mcp",
        Ty::Int(0, 10_000_000),
        Val("100000"),
        B,
    ),
    def(
        "files.read.max-uncached-ancestry",
        Ty::Int(0, 64),
        Val("1"),
        B,
    ),
    def("files.read.max-e6-commits", Ty::Int(0, 4096), Val("32"), B),
    def("input.max-bytes", Ty::Size(64 * KIB, GIB), Val("16MiB"), O),
    def("tx.max-statements", Ty::Int(1, 1_000_000), Val("1000"), V),
    def("tx.max-ops", Ty::Int(1, 1_000_000), Val("10000"), V),
    def(
        "tx.max-work-in-lock",
        Ty::Int(0, 1_000_000_000),
        Val("500000"),
        I,
    ),
    // §10.6 File links (R4).
    def(
        "files.policy.auto",
        Ty::Enum(&["exact", "strong"]),
        Val("exact"),
        V,
    ),
    def(
        "files.scratchpads",
        Ty::Enum(&["refuse", "allow"]),
        Val("refuse"),
        V,
    ),
    def(
        "files.ignore",
        Ty::GlobList,
        Val("target/,node_modules/,build/"),
        V,
    ),
    def("files.read-budget-ms", Ty::Int(1, 60_000), Val("20"), B),
    def(
        "files.session-start-cap-ms",
        Ty::Int(1, 10_000),
        Val("150"),
        B,
    ),
    def("files.links-sync-ms", Ty::Int(1, 3_600_000), Val("2000"), B),
    def(
        "files.deep.budget-ms",
        Ty::Int(1, 3_600_000),
        Val("10000"),
        B,
    ),
    def("mcp.links-sync-slice-ms", Ty::Int(1, 1000), Val("200"), I),
    def(
        "files.settle.others-after",
        Ty::Duration(0, 3650 * D),
        Val("1d"),
        X,
    ),
    def(
        "files.pending-escalate",
        Ty::Duration(H, 3650 * D),
        Val("14d"),
        O,
    ),
    def(
        "files.deletion-inference",
        Ty::Enum(&["explicit", "main-tree-commits"]),
        Val("explicit"),
        V,
    ),
    def("files.mv-git", Ty::Bool, Val("false"), X),
    def(
        "files.confirm-roles",
        Ty::Words,
        Val("orchestrator,owner"),
        V,
    ),
    def(
        "files.portable-names",
        Ty::Enum(&["refuse", "warn"]),
        Val("refuse"),
        V,
    ),
    def("files.hooks.evidence", Ty::Bool, Val("true"), X),
    def(
        "files.hooks.edit-evidence",
        Ty::Enum(&["auto", "on", "off"]),
        Val("auto"),
        X,
    ),
    // §10.7 Hooks.
    def(
        "hooks.transport",
        Ty::Enum(&["auto", "mcp", "command"]),
        Val("auto"),
        X,
    )
    .scope(UserQ)
    .reload(Reload::Install),
    def("hooks.session-start.enabled", Ty::Bool, Val("true"), X).reload(Reload::Install),
    def("hooks.user-prompt-submit.enabled", Ty::Bool, Val("true"), X).reload(Reload::Install),
    def("hooks.subagent-start.enabled", Ty::Bool, Val("true"), X).reload(Reload::Install),
    def("hooks.agent-launched.enabled", Ty::Bool, Val("true"), X).reload(Reload::Install),
    def("hooks.subagent-stop.enabled", Ty::Bool, Val("true"), X).reload(Reload::Install),
    def("hooks.stamp.enabled", Ty::Bool, Val("true"), X).reload(Reload::Install),
    def("hooks.session-start.settle", Ty::Bool, Val("true"), X),
    def("hooks.session-start.path-export", Ty::Bool, Val("true"), X).scope(UserQ),
    def("hooks.session-start.worker-pack", Ty::Bool, Val("true"), X),
    def(
        "hooks.session-start.orchestrator-lease",
        Ty::Bool,
        Val("true"),
        X,
    ),
    def("hooks.subagent-start.auto-sync", Ty::Bool, Val("true"), X),
    def(
        "hooks.sync-auto-keys",
        Ty::Int(0, 1_000_000),
        Val("2000"),
        X,
    ),
    def(
        "hooks.stamp.permission",
        Ty::Enum(&["allow", "ask"]),
        Val("allow"),
        X,
    )
    .scope(UserQ),
    def(
        "hooks.stamp.ask-for",
        Ty::Set(&["owner-authority", "edge-delete", "links-confirm"]),
        Val("owner-authority"),
        X,
    )
    .scope(UserQ),
    def(
        "hooks.delta.max-commits",
        Ty::Int(1, 1_000_000),
        Val("2000"),
        O,
    ),
    // §10.8 Agent tokens and output.
    def(
        "pack.budget.<role>",
        Ty::Size(1000, 1_000_000),
        PerParam(&[
            ("architect", Val("24000")),
            ("architecture-critic", Val("24000")),
            ("*", Val("16000")),
        ]),
        O,
    )
    .param(Param::Role),
    def(
        "pack.cli.max-bytes",
        Ty::Size(1000, 28_000),
        Val("24000"),
        O,
    )
    .scope(StoreUserLower),
    def(
        "pack.mcp.max-bytes",
        Ty::Size(1000, 48_000),
        Val("25000"),
        O,
    ),
    def("pack.quota.c2", Ty::Percent(0, 100), Val("15"), O),
    def("pack.quota.c3", Ty::Percent(0, 100), Val("20"), O),
    def("pack.quota.c4-dev", Ty::Percent(0, 100), Val("30"), O),
    def("pack.quota.c4-critic", Ty::Percent(0, 100), Val("40"), O),
    def("pack.quota.c5", Ty::Percent(0, 100), Val("10"), O),
    def(
        "pack.staleness-notice",
        Ty::Enum(&["off", "ids", "lines"]),
        Val("lines"),
        O,
    ),
    def("brief.budget", Ty::Size(1000, 9500), Val("8000"), O),
    def("brief.lang", Ty::Enum(&["en", "ru"]), Val("en"), O).scope(UserQ),
    def(
        "hooks.subagent-start.budget",
        Ty::Size(1000, 10_000),
        Val("3000"),
        O,
    ),
    def("hooks.delta.budget", Ty::Size(100, 10_000), Val("600"), O),
    def("mcp.always-load", Ty::Set(&TOOLS), Val(""), X).reload(Reload::Restart),
    def(
        "mcp.result-max-bytes",
        Ty::Size(1000, 48_000),
        Val("25000"),
        O,
    ),
    def(
        "mcp.result-max-bytes.<client>",
        Ty::Size(1000, 48_000),
        PerParam(&[
            ("codex", Hole("CFG-codex-mcp-result", "16000")),
            ("*", Key("mcp.result-max-bytes")),
        ]),
        O,
    )
    .param(Param::Client),
    def(
        "output.nonzero-exit-max-bytes",
        Ty::Size(1000, 10_000),
        Val("8000"),
        O,
    ),
    def(
        "output.nonzero-exit-max-bytes.<client>",
        Ty::Size(1000, 10_000),
        Key("output.nonzero-exit-max-bytes"),
        O,
    )
    .param(Param::Client),
    def("mcp.ids-page-bytes", Ty::Size(1000, 25_000), Val("8000"), O),
    def(
        "output.ids-max-bytes",
        Ty::Size(0, 1_000_000_000),
        Val("24000"),
        O,
    )
    .env("MOIRAI_IDS_MAX_BYTES"),
    def(
        "export.memory-md",
        Ty::Enum(&["auto", "full", "pointer"]),
        Val("auto"),
        X,
    ),
    // §10.9 Harnesses and client profiles.
    def(
        "client.profile",
        Ty::Enum(&["auto", "claude", "codex", "generic"]),
        Val("auto"),
        O,
    )
    .scope(UserQ)
    .env("MOIRAI_CLIENT"),
    def(
        "mcp.tools",
        Ty::Enum(&["read", "core", "all"]),
        Val("all"),
        X,
    )
    .reload(Reload::Restart),
    def(
        "integrate.instructions-scope",
        Ty::Enum(&["project", "user"]),
        Val("project"),
        X,
    )
    .scope(User)
    .reload(Reload::Install),
    def(
        "integrate.claude-md",
        Ty::Enum(&["import", "copy"]),
        Val("import"),
        X,
    )
    .scope(User)
    .reload(Reload::Install),
    def(
        "integrate.codex.store-writes",
        Ty::Enum(&["writable-root", "execpolicy-store", "execpolicy", "mcp"]),
        Hole("CFG-codex-store-writes", "writable-root"),
        X,
    )
    .scope(User)
    .reload(Reload::Install),
    def(
        "integrate.codex.approval",
        Ty::Enum(&["prompt", "writes", "split", "approve"]),
        Hole("CFG-codex-approval", "split"),
        X,
    )
    .scope(User)
    .reload(Reload::Install),
    def(
        "integrate.hooks",
        Ty::Enum(&["none", "min", "full"]),
        Rule("hooks-tier"),
        X,
    )
    .scope(User)
    .reload(Reload::Install),
    def(
        "lq.model-profile.<family>",
        Ty::Enum(&["gated", "compatible", "unknown"]),
        PerParam(&[
            ("claude-opus-5-5", Hole("CFG-model-profile-opus", "gated")),
            ("*", Val("unknown")),
        ]),
        V,
    )
    .param(Param::Family),
    def(
        "lq.model-profile.default.<client>",
        Ty::Family,
        PerParam(&[
            ("claude", Val("claude-opus-5-5")),
            ("codex", Val("unknown")),
            ("generic", Val("unknown")),
            ("*", Val("unknown")),
        ]),
        V,
    )
    .param(Param::Client),
    def(
        "query.safelist.model.<profile>",
        Ty::Enum(&["off", "named-only", "dry-targets"]),
        PerParam(&[("unknown", Val("named-only")), ("*", Val("off"))]),
        V,
    )
    .param(Param::Profile),
    // §10.10 Git image.
    def(
        "image.dest.<name>.path",
        Ty::Path,
        PerParam(&[("default", Rule("dest-path")), ("*", Dflt::None)]),
        X,
    )
    .param(Param::Dest)
    .scope(UserQ),
    def(
        "image.dest.<name>.refs",
        Ty::GlobList,
        Val("main,tags/*,lane/*"),
        V,
    )
    .param(Param::Dest),
    def(
        "image.dest.<name>.granularity",
        Ty::Enum(&["checkpoint", "commit"]),
        Val("checkpoint"),
        V,
    )
    .param(Param::Dest),
    def(
        "image.dest.<name>.object-format",
        Ty::Enum(&["sha1", "sha256"]),
        Val("sha1"),
        X,
    )
    .param(Param::Dest)
    .reload(Reload::Init),
    def(
        "image.dest.<name>.kind",
        Ty::Enum(&["bare-repo"]),
        Val("bare-repo"),
        X,
    )
    .param(Param::Dest)
    .reload(Reload::Init),
    def(
        "image.dest.<name>.anchor-text",
        Ty::Enum(&["full", "hash-only"]),
        Val("full"),
        V,
    )
    .param(Param::Dest),
    def(
        "image.dest.<name>.git.pack-threads",
        Ty::Int(1, 64),
        Val("2"),
        I,
    )
    .param(Param::Dest),
    def(
        "image.dest.<name>.git.pack-window-memory",
        Ty::Size(MIB, 4 * GIB),
        Val("64MiB"),
        I,
    )
    .param(Param::Dest),
    def("image.export.on-merge-to-main", Ty::Bool, Val("true"), X),
    def(
        "image.export.max-age",
        Ty::Duration(H, 3650 * D),
        Val("1d"),
        X,
    ),
    def(
        "image.import-merge",
        Ty::Enum(&["auto", "stage"]),
        Val("auto"),
        V,
    ),
    def("image.allowed-remotes", Ty::UrlList, Val(""), X).scope(UserQ),
    def("image.transport.spawn-git", Ty::Bool, Val("true"), X).scope(UserQ),
    // §10.11 Merge and runs.
    def("merge.strict", Ty::Bool, Val("false"), V),
    def(
        "runs.granularity",
        Ty::Enum(&["workflow", "agent-call"]),
        Val("workflow"),
        V,
    ),
];

/// The retired names of [CFG §6.4], with the successor or reason `config set` names.
pub const RETIRED: &[(&str, &str)] = &[
    ("pack.cyrillic-weight", "removed: budgets are UTF-8 bytes"),
    ("pack.cli.max-chars", "pack.cli.max-bytes"),
    ("pack.mcp.max", "pack.mcp.max-bytes"),
    ("mcp.result-max-chars", "mcp.result-max-bytes"),
    (
        "mcp.result-max-chars.<client>",
        "mcp.result-max-bytes.<client>",
    ),
    (
        "output.nonzero-exit-max-chars",
        "output.nonzero-exit-max-bytes",
    ),
    (
        "output.nonzero-exit-max-chars.<client>",
        "output.nonzero-exit-max-bytes.<client>",
    ),
    ("query.budget.default.chars", "query.budget.default.bytes"),
    (
        "tx.wmem-max",
        "query.caps.<role>.wmem, with query.budget.default.wmem",
    ),
    ("files.usn", "removed with E2"),
    ("files.hooks.nudge", "removed with the move nudge"),
    ("files.budget.window", "never a key: an R-14 constant"),
    ("image.anchor-text", "image.dest.<name>.anchor-text"),
    (
        "policy.unleased-root-role",
        "withdrawn: unleased callers get the general-purpose row",
    ),
    ("files.journal", "reserved for a feature not built"),
    (
        "hooks.post-tool-batch.delta",
        "reserved for a feature not built",
    ),
    (
        "hooks.native-tasks-mirror",
        "reserved for a feature not built",
    ),
];

/// The policy-data rows of [CFG §10.13]: schema rows, not keys; `config set` refuses them and names the schema write.
pub const POLICY_DATA: &[&str] = &[
    "policy.self-claim-roles",
    "policy.mint.role-lease",
    "policy.hook-label",
    "policy.role.<role>.mcp-write",
    "policy.role.<role>.tx",
    "policy.role.developer.fields",
    "policy.role.<role>.define-query",
    "policy.role.<role>.authority-owner",
    "edges.blocks.on-src-deleted",
    "edges.gates.on-src-deleted",
    "merge.policy.<kind>",
];

/// Whether a full key matches a pattern whose parameter segments take any word (used for the retired and policy-data
/// names).
fn matches_loose(pattern: &str, key: &str) -> bool {
    let (p, k): (Vec<&str>, Vec<&str>) = (pattern.split('.').collect(), key.split('.').collect());
    p.len() == k.len()
        && p.iter()
            .zip(&k)
            .all(|(a, b)| a == b || (a.starts_with('<') && crate::config::is_word(b)))
}

/// The retired name a key names, with its successor ([CFG §6.4]).
pub fn retired(key: &str) -> Option<&'static (&'static str, &'static str)> {
    let k = key.to_ascii_lowercase();
    RETIRED.iter().find(|(n, _)| matches_loose(n, &k))
}

/// Whether a key names policy data ([CFG §2.4], §10.13).
pub fn policy_data(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    POLICY_DATA.iter().any(|n| matches_loose(n, &k))
}

/// The registry row a full key matches, with its parameter value ([CFG §3.3] rules 2 and 5): ASCII case folding, then
/// the same segment count, every fixed segment equal and the parameter segment in its vocabulary.
// spec: [CFG §3.3]
pub fn find(key: &str) -> Option<(&'static Def, Option<String>)> {
    let k = key.to_ascii_lowercase();
    let segs: Vec<&str> = k.split('.').collect();
    if segs.len() > 16 || k.len() > 255 {
        return None;
    }
    DEFS.iter().find_map(|d| {
        let p: Vec<&str> = d.key.split('.').collect();
        if p.len() != segs.len() {
            return None;
        }
        let mut val = None;
        for (a, b) in p.iter().zip(&segs) {
            if a.starts_with('<') {
                if !d.param.is_some_and(|x| x.accepts(b)) {
                    return None;
                }
                val = Some(b.to_string());
            } else if a != b {
                return None;
            }
        }
        Some((d, val))
    })
}

/// The instance of a pattern with its parameter filled.
pub fn instance(d: &Def, value: &str) -> String {
    d.key
        .split('.')
        .map(|s| if s.starts_with('<') { value } else { s })
        .collect::<Vec<_>>()
        .join(".")
}

/// The instance the sweeps use: the pattern itself, or with [`Param::representative`] filled.
pub fn representative(d: &Def) -> String {
    match d.param {
        Some(p) => instance(d, p.representative()),
        None => d.key.to_string(),
    }
}

/// Validates a value of a key instance ([CFG §4]): its type and range, a canonical form of at most 4,000 bytes, no LF,
/// and the per-instance range [CFG §10.8] gives `mcp.result-max-bytes.codex` (at most 36,000).
// spec: [CFG §4.1]
// spec: [CFG §7.2]
pub fn validate(d: &Def, instance: &str, text: &str) -> Option<Parsed> {
    if text.contains('\n') {
        return None;
    }
    let p = parse(d.ty, text)?;
    if p.canonical.len() > 4000 {
        return None;
    }
    if instance == "mcp.result-max-bytes.codex" && p.number.is_some_and(|n| n > 36_000) {
        return None;
    }
    Some(p)
}

/// The model's value of a `rule:` default ([CFG §9.1]): `main-tree`, `main-ref` and `dest-path` have none in a model
/// stream (no `init` inside a git repository and no image destination at M0); `hooks-tier` is `full` (a Tier A
/// harness); `agent-max-mem` is 2 MiB in a CLI or hook process and 4 MiB in the MCP server, ten times that for
/// `orchestrator` and `owner` ([CFG §10.5]).
// spec: [CFG §7.6] main-tree, main-ref
// spec: [CFG §10.5] agent-max-mem
pub fn rule_value(rule: &str, proc: Proc) -> Option<&'static str> {
    match (rule, proc) {
        ("main-tree" | "main-ref" | "dest-path", _) => None,
        ("hooks-tier", _) => Some("full"),
        ("agent-max-mem", Proc::Cli) => Some("2MiB"),
        ("agent-max-mem", Proc::Mcp) => Some("4MiB"),
        ("agent-max-mem-x10", Proc::Cli) => Some("20MiB"),
        ("agent-max-mem-x10", Proc::Mcp) => Some("40MiB"),
        (other, _) => panic!("no rule {other}"),
    }
}

fn resolve_default(dflt: Dflt, param: Option<&str>, proc: Proc) -> Option<String> {
    match dflt {
        Dflt::Val(v) => Some(v.to_string()),
        Dflt::None => None,
        Dflt::Key(k) => default_text(k, proc),
        Dflt::Rule(r) => rule_value(r, proc).map(str::to_string),
        Dflt::Hole(_, v) => Some(v.to_string()),
        Dflt::PerParam(pairs) => {
            let p = param.unwrap_or("*");
            let (_, d) = pairs
                .iter()
                .find(|(n, _)| *n == p)
                .or_else(|| pairs.iter().find(|(n, _)| *n == "*"))
                .expect("a per-param default ends with *");
            resolve_default(*d, param, proc)
        }
    }
}

/// The default of a key instance as text ([CFG §5.1] rule 5), `None` for a key whose default is `none`.
pub fn default_text(key: &str, proc: Proc) -> Option<String> {
    let (d, p) = find(key).unwrap_or_else(|| panic!("{key} is not a registry key"));
    resolve_default(d.default, p.as_deref(), proc)
}

/// The default of a key instance, parsed.
pub fn default_parsed(key: &str, proc: Proc) -> Option<Parsed> {
    let (d, _) = find(key)?;
    let t = default_text(key, proc)?;
    Some(parse(d.ty, &t).unwrap_or_else(|| panic!("the default {t} of {key} is invalid")))
}

/// The empty value of a list type, as the sweeps and [RULES/policy-keys] write it.
pub const EMPTY: &str = "(empty)";
/// An unset key (the `none` default, or `config unset`), as the sweeps and [RULES/policy-keys] write it.
pub const UNSET: &str = "(unset)";

/// The allowed-value set of a key's representative instance ([CFG §9.5]): `bool` both values; `enum` every value;
/// `set` the empty set, each single member and the full set; `words`, `glob-list`, `url-list` the default, the empty
/// list and one other list; `int`, `size`, `duration`, `percent` the default, the lower bound, the upper bound and the
/// [F17 §12] test value; `path` set and unset; `family`, `word`, `ref`, `git-ref` the default and one other value.
/// The default comes first; values are canonical; [`EMPTY`] and [`UNSET`] stand for the empty and the absent value.
/// The model reads no configured bound as an allocation size, so the upper bound needs no clipping to model scale
/// ([60 §4.4] item 8).
// spec: [CFG §9.5]
pub fn sweep(d: &Def) -> Vec<String> {
    let inst = representative(d);
    let canon = |t: &str| -> String {
        let c = parse(d.ty, t)
            .unwrap_or_else(|| panic!("{t} is not a value of {}", d.key))
            .canonical;
        if c.is_empty() { EMPTY.to_string() } else { c }
    };
    let dflt = default_text(&inst, Proc::Cli)
        .map(|t| canon(&t))
        .unwrap_or_else(|| UNSET.to_string());
    let mut out = vec![dflt.clone()];
    let mut push = |v: String| {
        if !out.contains(&v) {
            out.push(v);
        }
    };
    let num = |n: u64| -> String {
        match d.ty {
            Ty::Size(..) | Ty::SizePow2(..) => crate::config::size_canon(n),
            Ty::Duration(..) => crate::config::duration_canon(n),
            _ => n.to_string(),
        }
    };
    match d.ty {
        Ty::Bool => {
            push("true".into());
            push("false".into());
        }
        Ty::Enum(ws) => ws.iter().for_each(|w| push((*w).to_string())),
        Ty::Set(ws) => {
            push(EMPTY.into());
            ws.iter().for_each(|w| push((*w).to_string()));
            push(ws.join(","));
        }
        Ty::Words => {
            push(EMPTY.into());
            push("developer".into());
        }
        Ty::GlobList => {
            push(EMPTY.into());
            push("*.tmp".into());
        }
        Ty::UrlList => {
            push(EMPTY.into());
            push("https://example.invalid/image.git".into());
        }
        Ty::Int(lo, hi) => {
            push(lo.to_string());
            push(hi.to_string());
        }
        Ty::Size(lo, hi) | Ty::SizePow2(lo, hi) | Ty::Duration(lo, hi) | Ty::Percent(lo, hi) => {
            // The per-instance bound of `mcp.result-max-bytes.codex` ([CFG §10.8]).
            let hi = if inst == "mcp.result-max-bytes.codex" {
                hi.min(36_000)
            } else {
                hi
            };
            push(num(lo));
            push(num(hi));
        }
        Ty::Path => {
            push("/work/x".into());
            push(UNSET.into());
        }
        Ty::Word => push("other".into()),
        Ty::Family => push(if dflt == "gpt-5-6-luna" {
            "claude-opus-5-5".into()
        } else {
            "gpt-5-6-luna".into()
        }),
        Ty::Ref => push(if dflt == "lane/x" {
            "main".into()
        } else {
            "lane/x".into()
        }),
        Ty::GitRef => push(if dflt == "main" {
            "trunk".into()
        } else {
            "main".into()
        }),
    }
    if let Some(t) = d.test {
        push(canon(t));
    }
    out
}

/// The model's configuration: the simulated store file, the simulated user file and the `init`-recorded values
/// ([CFG §2.1], §5.1 rule 4; [API §8.1], §8.2). Values are canonical texts keyed by full key.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Conf {
    /// The store file: store-scope key instances.
    pub store: BTreeMap<String, String>,
    /// The user file: user-scope key instances, unqualified or `stores.<store-id>.<key>` ([CFG §2.3]), and
    /// user-lower store keys.
    pub user: BTreeMap<String, String>,
    /// The values recorded at creation (`InitParams`, [F17 §2]).
    pub init: BTreeMap<String, String>,
    /// The store id as 32 lower-case hex digits, which selects the qualified user entries.
    pub store_id: String,
}

/// Where an effective value comes from ([CFG §5.6] `source`, plus `default`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// The store file.
    Store,
    /// The unqualified user entry.
    User,
    /// The store-qualified user entry.
    UserQualified,
    /// The value recorded at creation.
    Init,
    /// The built-in default.
    Default,
}

impl Conf {
    /// The effective value of a key instance and its source ([CFG §5.1] rules 3–5 and "User-lower"), `None` for an
    /// unset key whose default is `none`. The model has no flags and no environment for configuration (rules 1, 2).
    // spec: [CFG §5.1]
    pub fn effective(&self, key: &str, proc: Proc) -> Option<(String, Source)> {
        let (d, _) = find(key)?;
        let v = if d.reload == Reload::Init && d.param.is_none() {
            self.init
                .get(key)
                .map(|v| (v.clone(), Source::Init))
                .or_else(|| default_text(key, proc).map(|v| (v, Source::Default)))
        } else if d.scope.is_user() {
            let q = format!("stores.{}.{key}", self.store_id);
            self.user
                .get(&q)
                .filter(|_| d.scope == Scope::UserQ)
                .map(|v| (v.clone(), Source::UserQualified))
                .or_else(|| self.user.get(key).map(|v| (v.clone(), Source::User)))
                .or_else(|| default_text(key, proc).map(|v| (v, Source::Default)))
        } else {
            self.store
                .get(key)
                .map(|v| (v.clone(), Source::Store))
                .or_else(|| match d.default {
                    // A derived default reads the other key's effective value ([CFG §5.1] "Derived defaults").
                    Dflt::Key(k) => self.effective(k, proc),
                    Dflt::PerParam(_) => {
                        let (dd, p) = find(key)?;
                        match dd.default {
                            Dflt::PerParam(pairs) => {
                                let pv = p.as_deref().unwrap_or("*");
                                let pick = pairs
                                    .iter()
                                    .find(|(n, _)| *n == pv)
                                    .or_else(|| pairs.iter().find(|(n, _)| *n == "*"))
                                    .map(|(_, x)| *x);
                                match pick {
                                    Some(Dflt::Key(k)) => self.effective(k, proc),
                                    _ => default_text(key, proc).map(|v| (v, Source::Default)),
                                }
                            }
                            _ => None,
                        }
                    }
                    _ => default_text(key, proc).map(|v| (v, Source::Default)),
                })
        };
        // User-lower: a valid user value can only lower the store value.
        if d.scope == Scope::StoreUserLower {
            let q = format!("stores.{}.{key}", self.store_id);
            if let Some(u) = self.user.get(&q).or_else(|| self.user.get(key))
                && let (Some(un), Some((sv, _))) = (parse(d.ty, u).and_then(|p| p.number), &v)
                && parse(d.ty, sv)
                    .and_then(|p| p.number)
                    .is_some_and(|sn| un < sn)
            {
                return Some((u.clone(), Source::User));
            }
        }
        v
    }

    /// The effective value of a key instance parsed; `None` for an unset `none` key.
    pub fn parsed(&self, key: &str, proc: Proc) -> Option<Parsed> {
        let (d, _) = find(key)?;
        let (t, _) = self.effective(key, proc)?;
        parse(d.ty, &t)
    }

    /// The effective number of a numeric key.
    pub fn number(&self, key: &str) -> u64 {
        self.parsed(key, Proc::Cli)
            .and_then(|p| p.number)
            .unwrap_or_else(|| panic!("{key} has no number"))
    }

    /// The effective text of a key (the empty text for an unset `none` key).
    pub fn text(&self, key: &str) -> String {
        self.effective(key, Proc::Cli)
            .map(|(t, _)| t)
            .unwrap_or_default()
    }

    /// The effective value of a `bool` key.
    pub fn flag(&self, key: &str) -> bool {
        self.number(key) == 1
    }

    /// The effective values the store's constraints read ([CFG §5.3]): the store file over the init-recorded values
    /// over the defaults.
    pub fn constraint_value(&self, key: &str) -> Option<Parsed> {
        self.parsed(key, Proc::Cli)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// RG-1: no key instance matches two patterns; tested over each pattern's representative and every fixed word
    /// another pattern could take as its parameter.
    #[test]
    fn rg1_no_instance_matches_two_patterns() {
        let mut samples: BTreeSet<String> = DEFS.iter().map(representative).collect();
        for d in DEFS {
            for seg in d.key.split('.').filter(|s| !s.starts_with('<')) {
                for e in DEFS.iter().filter(|e| e.param.is_some()) {
                    samples.insert(instance(e, seg));
                }
            }
        }
        for s in samples {
            let n = DEFS
                .iter()
                .filter(|d| {
                    let p: Vec<&str> = d.key.split('.').collect();
                    let k: Vec<&str> = s.split('.').collect();
                    p.len() == k.len()
                        && p.iter().zip(&k).all(|(a, b)| {
                            if a.starts_with('<') {
                                d.param.is_some_and(|x| x.accepts(b))
                            } else {
                                a == b
                            }
                        })
                })
                .count();
            assert!(n <= 1, "{s} matches {n} patterns");
        }
    }

    /// RG-2: every default is valid for its type and range, per parameter and per process kind; the defaults satisfy
    /// every constraint.
    #[test]
    fn rg2_defaults_are_valid_and_consistent() {
        for d in DEFS {
            let mut params: Vec<String> = vec![representative(d)];
            if let Dflt::PerParam(pairs) = d.default {
                params.extend(
                    pairs
                        .iter()
                        .filter(|(n, _)| *n != "*")
                        .map(|(n, _)| instance(d, n)),
                );
            }
            for inst in params {
                for proc in [Proc::Cli, Proc::Mcp] {
                    if let Some(t) = default_text(&inst, proc) {
                        assert!(
                            validate(d, &inst, &t).is_some(),
                            "the default {t} of {inst} is invalid"
                        );
                        assert_eq!(
                            parse(d.ty, &t).unwrap().canonical,
                            t,
                            "the default of {inst} is not canonical"
                        );
                    }
                }
            }
        }
        crate::config::check_constraints(&|_| None, &|_| false).expect("the defaults hold");
    }

    /// RG-3 to RG-5 and RG-7: one scope each (`q` only on user keys, user-lower only on store keys by construction of
    /// [`Scope`]); no key starts with `stores` or reuses a retired name; every variable starts `MOIRAI_`, names one
    /// key and contains no forbidden word; patterns are at most 255 bytes and 16 lower-case segments.
    #[test]
    fn rg3_to_rg7_hold() {
        let mut envs = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for d in DEFS {
            assert!(keys.insert(d.key), "{} is registered twice", d.key);
            assert!(!d.key.starts_with("stores."), "RG-4: {}", d.key);
            assert!(retired(d.key).is_none(), "RG-4: {} is retired", d.key);
            assert!(!policy_data(d.key), "{} is policy data", d.key);
            assert!(d.key.len() <= 255 && d.key.split('.').count() <= 16, "RG-7");
            for seg in d.key.split('.') {
                assert!(
                    seg.starts_with('<') || crate::config::is_word(seg),
                    "RG-7: {seg}"
                );
            }
            assert_eq!(
                d.key.contains('<'),
                d.param.is_some(),
                "{} and its parameter",
                d.key
            );
            if let Some(e) = d.env {
                assert!(e.starts_with("MOIRAI_") && envs.insert(e), "RG-5: {e}");
                for w in ["KEY", "TOKEN", "SECRET", "PASSWORD"] {
                    assert!(!e.contains(w), "RG-5: {e}");
                }
            }
        }
    }

    #[test]
    fn keys_match_by_pattern_and_vocabulary() {
        assert_eq!(
            find("Files.Policy.Auto").map(|(d, _)| d.key),
            Some("files.policy.auto")
        );
        let (d, p) = find("query.caps.orchestrator.rows").unwrap();
        assert_eq!(
            (d.key, p.as_deref()),
            ("query.caps.<role>.rows", Some("orchestrator"))
        );
        assert!(find("roots.project").is_none() && find("roots.1x").is_none());
        assert!(
            find("lq.model-profile.default").is_none(),
            "a reserved family"
        );
        assert!(find("mcp.overlay-bytes.gemini").is_none(), "not a client");
        assert_eq!(
            find("lq.model-profile.default.codex").map(|(d, _)| d.key),
            Some("lq.model-profile.default.<client>")
        );
        assert_eq!(
            retired("mcp.result-max-chars.codex").map(|r| r.1),
            Some("mcp.result-max-bytes.<client>")
        );
        assert!(policy_data("policy.role.tester.mcp-write") && policy_data("merge.policy.task"));
    }

    #[test]
    fn defaults_resolve_per_parameter_and_by_key() {
        assert_eq!(
            default_text("query.caps.owner.work", Proc::Cli).as_deref(),
            Some("200000000")
        );
        assert_eq!(
            default_text("query.caps.tester.mem", Proc::Mcp).as_deref(),
            Some("4MiB")
        );
        assert_eq!(
            default_text("mcp.result-max-bytes.claude", Proc::Cli).as_deref(),
            Some("25000")
        );
        assert_eq!(default_text("roots.docs", Proc::Cli), None);
        let mut c = Conf::default();
        c.store
            .insert("mcp.result-max-bytes".into(), "20000".into());
        assert_eq!(
            c.effective("mcp.result-max-bytes.generic", Proc::Cli)
                .map(|x| x.0)
                .as_deref(),
            Some("20000"),
            "a derived default reads the other key's effective value"
        );
        assert_eq!(
            c.effective("mcp.result-max-bytes.codex", Proc::Cli)
                .map(|x| x.0)
                .as_deref(),
            Some("16000")
        );
        c.user.insert("pack.cli.max-bytes".into(), "30000".into());
        assert_eq!(
            c.text("pack.cli.max-bytes"),
            "24000",
            "user-lower never raises"
        );
        c.user.insert("pack.cli.max-bytes".into(), "12000".into());
        assert_eq!(c.text("pack.cli.max-bytes"), "12000");
        c.store_id = "ab".repeat(16);
        c.user.insert("files.cloud".into(), "refuse".into());
        c.user.insert(
            format!("stores.{}.files.cloud", c.store_id),
            "metadata-only".into(),
        );
        assert_eq!(
            c.effective("files.cloud", Proc::Cli),
            Some(("metadata-only".into(), Source::UserQualified))
        );
    }

    #[test]
    fn sweeps_follow_the_type() {
        let s = |k: &str| sweep(DEFS.iter().find(|d| d.key == k).unwrap());
        assert_eq!(s("merge.strict"), ["false", "true"]);
        assert_eq!(
            s("durability.lazy-kinds"),
            [
                "heartbeat,cursor,session-mark",
                "(empty)",
                "heartbeat",
                "cursor",
                "session-mark"
            ]
        );
        assert_eq!(s("lease.ttl-default"), ["15m", "1m", "30d"]);
        assert_eq!(s("idempotency.retention"), ["30d", "1s", "3650d", "1h"]);
        assert_eq!(s("roots.<name>"), ["(unset)", "/work/x"]);
        assert_eq!(
            s("lq.model-profile.default.<client>"),
            ["unknown", "gpt-5-6-luna"]
        );
    }
}
