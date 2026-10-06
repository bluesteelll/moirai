//! A test per allowed value of every key and policy-data row of [RULES/policy-keys] ([m0/PLAN §3.2] WP-90; [CFG §9.5];
//! [m0/PLAN §7] E8).

use super::*;
use crate::api::{Cmd, Ctx, Data, Door, Outcome, Reply, Store};
use crate::clock::{EnvClock, EnvSlots};
use crate::confcmd::FileScope;
use crate::lq::ctx::{Profile, Value as P};
use crate::pack::{self, Surface};
use crate::policy::{PolicyData, Rights};
use crate::registry::{Conf, DEFS, Proc};
use crate::tx::{Stmt, Target};
use crate::value::Nid;
use std::collections::BTreeSet;

const DAY: i64 = 86_400_000;

// ----- building stores ------------------------------------------------------------------------------------------------

/// A store with `key` = `value` (and its companions), with the session lease L-1 of `orch` minted as the suite's base
/// stream does; `None` when the configuration is refused (a failed test).
fn store_with(key: &str, value: &str) -> Store {
    let (init, user) = settings(key, value);
    store_params(&init, &user)
}

fn store_params(init: &[String], user: &[(String, String)]) -> Store {
    let mut s = Store::new();
    s.run(
        &Cmd::EnvSlots(EnvSlots {
            hold: vec!["claude:s1".into()],
            ..Default::default()
        }),
        &Ctx::default(),
    );
    let r = s.run(
        &Cmd::Init {
            seed: 42,
            params: init.to_vec(),
            default_branch: None,
        },
        &Ctx::default(),
    );
    assert_eq!(r.outcome, Outcome::Ok, "Init {init:?}: {:?}", r.error);
    for (k, v) in user {
        let r = s.run(
            &Cmd::ConfigSet {
                key: k.clone(),
                value: v.clone(),
                scope: FileScope::User,
            },
            &Ctx::default(),
        );
        assert_eq!(r.outcome, Outcome::Ok, "ConfigSet {k}={v}: {:?}", r.error);
    }
    // Every store runs the same number of commands before its stream, so the command numbers that random uids derive
    // from ([API §17.4]) agree: an empty clock step stands for each absent user setting.
    for _ in user.len()..2 {
        s.run(
            &Cmd::EnvClock(EnvClock {
                advance_ms: Some(0),
                ..Default::default()
            }),
            &Ctx::default(),
        );
    }
    let mut ctx = Ctx {
        agent: Some("orch".into()),
        ..Default::default()
    };
    ctx.env.insert("CLAUDECODE".into(), "1".into());
    ctx.env.insert("CLAUDE_CODE_SESSION_ID".into(), "s1".into());
    let r = s.run(&claim_session(), &ctx);
    assert_eq!(r.outcome, Outcome::Ok, "{:?}", r.error);
    s
}

fn claim_session() -> Cmd {
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
    }
}

fn orch() -> Ctx {
    Ctx {
        lease: Some("L-1".into()),
        client: Some("claude".into()),
        no_dedupe: true,
        ..Default::default()
    }
}

fn run_ok(s: &mut Store, c: Cmd, ctx: &Ctx) -> Reply {
    let r = s.run(&c, ctx);
    assert!(
        matches!(r.outcome, Outcome::Ok | Outcome::Replayed | Outcome::Dry),
        "{c:?}: {:?}",
        r.error
    );
    r
}

/// A task #1 on `main`, `lane/x` forked from it, then two sides that disagree on it — `main` sets its priority to P1
/// (or, with `delete`, deletes it) and `lane/x` sets it to P3 — and `merge main --into lane/x`.
fn two_sided_merge(s: &mut Store, delete: bool) -> Reply {
    run_ok(s, tx(vec![create("task", "t")]), &orch());
    run_ok(
        s,
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        &orch(),
    );
    let on_main = if delete {
        Stmt::Delete {
            target: Target::Id(Nid(1)),
            policy: None,
            replaced_by: None,
            release: false,
            reason: Some("gone".into()),
        }
    } else {
        Stmt::Set {
            target: Target::Id(Nid(1)),
            fields: vec![("priority".into(), P::Int(1))],
            incr: vec![],
            body: None,
            guard: None,
        }
    };
    run_ok(s, tx(vec![on_main]), &orch());
    let lane = Ctx {
        branch: Some("lane/x".into()),
        ..orch()
    };
    run_ok(
        s,
        tx(vec![Stmt::Set {
            target: Target::Id(Nid(1)),
            fields: vec![("priority".into(), P::Int(3))],
            incr: vec![],
            body: None,
            guard: None,
        }]),
        &lane,
    );
    s.run(
        &Cmd::Merge {
            src: "main".into(),
            into: Some("lane/x".into()),
            policy: None,
            strict: None,
            base: None,
            message: String::new(),
        },
        &orch(),
    )
}

fn tx(stmts: Vec<Stmt>) -> Cmd {
    Cmd::Tx {
        stmts,
        message: String::new(),
    }
}

fn create(kind: &str, title: &str) -> Stmt {
    Stmt::Create {
        name: None,
        kind: kind.into(),
        fields: vec![("title".into(), P::Text(title.into()))],
        body: None,
        under: None,
        position: None,
        edges_out: vec![],
        edges_in: vec![],
    }
}

fn set(n: u32, f: &str, v: P) -> Stmt {
    Stmt::Set {
        target: Target::Id(Nid(n)),
        fields: vec![(f.into(), v)],
        incr: vec![],
        body: None,
        guard: None,
    }
}

fn claim(ids: &[u32], agent: Option<&str>, role: Option<&str>, run: Option<&str>) -> Cmd {
    Cmd::Claim {
        ids: ids.iter().map(|n| Target::Id(Nid(*n))).collect(),
        next: false,
        scope: None,
        role: role.map(str::to_string),
        agent: agent.map(str::to_string),
        ttl: run.map(|_| P::Text("run".into())),
        start: false,
        run: run.map(str::to_string),
        session: false,
    }
}

fn lease_of(r: &Reply) -> String {
    r.yields[0].rows[0]
        .iter()
        .find(|(k, _)| k == "lease")
        .expect("a lease")
        .1
        .clone()
}

fn advance(s: &mut Store, ms: i64) {
    s.run(
        &Cmd::EnvClock(EnvClock {
            advance_ms: Some(ms as u64),
            ..Default::default()
        }),
        &Ctx::default(),
    );
}

fn code(r: &Reply) -> Option<&str> {
    r.error.as_ref().map(|e| e.code.as_str())
}

fn num(key: &str, v: &str) -> u64 {
    parsed(key, v).and_then(|p| p.number).expect("a number")
}

/// The configuration a value gives, without a store.
fn conf_with(key: &str, v: &str) -> Conf {
    let (init, user) = settings(key, v);
    let vals = config::parse_init(&init, None).expect("valid");
    let mut c = Conf {
        store: vals
            .iter()
            .filter(|(k, _)| !config::is_init(k))
            .map(|(k, p)| (k.clone(), p.canonical.clone()))
            .collect(),
        ..Conf::default()
    };
    for (k, v) in user {
        c.user.insert(k, v);
    }
    c
}

// ----- the table against the registry ---------------------------------------------------------------------------------

/// Every key pattern of [CFG §10] has exactly one row; every row names a registered pattern with its representative
/// instance, its class and exactly the sweep set of [CFG §9.5]; every function is in `key-functions`.
#[test]
fn the_table_equals_the_registry() {
    let rows = key_rows();
    let keys: BTreeSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys.len(), rows.len(), "a key has two rows");
    for d in DEFS {
        let r = rows
            .iter()
            .find(|r| r.key == d.key)
            .unwrap_or_else(|| panic!("{} has no policy-keys row", d.key));
        assert_eq!(r.instance, registry::representative(d), "{}", r.id);
        assert_eq!(r.vis, d.vis.token(), "{}", r.id);
        assert_eq!(r.values, registry::sweep(d), "{}: the sweep set", r.id);
        if d.vis == registry::Vis::V {
            assert_ne!(
                r.function, "invariance",
                "{}: RG-6, a class-V key has a function",
                r.id
            );
        }
    }
    assert_eq!(rows.len(), DEFS.len(), "a row names no registered key");
    for r in &rows {
        // The first checker is the one of the row's class; a second one covers a second class (KY-011).
        let classes = |c: &str| -> Vec<String> {
            rules()
                .table("key-checkers")
                .rows
                .iter()
                .find(|k| k.tok("checker") == c)
                .map(|k| k.toks("vis").iter().map(|x| x.to_string()).collect())
                .unwrap_or_default()
        };
        assert!(
            classes(&r.checker[0]).contains(&r.vis),
            "{}: {} does not check class {}",
            r.id,
            r.checker[0],
            r.vis
        );
    }
    // The policy rows are exactly the registry's policy-data names, each once, with an instance of its name.
    let names: Vec<String> = policy_rows().into_iter().map(|p| p.1).collect();
    let set: BTreeSet<&str> = names.iter().map(String::as_str).collect();
    assert_eq!(set.len(), names.len(), "a policy-data row appears twice");
    assert_eq!(
        set,
        registry::POLICY_DATA
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        "policy-rows against the registry"
    );
    for (id, name, instance, _, _) in policy_rows() {
        assert!(registry::policy_data(&instance), "{id}: {instance}");
        let (p, i): (Vec<&str>, Vec<&str>) =
            (name.split('.').collect(), instance.split('.').collect());
        assert_eq!(
            p.len(),
            i.len(),
            "{id}: {instance} is not an instance of {name}"
        );
        for (a, b) in p.iter().zip(&i) {
            assert!(
                a == b || (a.starts_with('<') && crate::config::is_word(b)),
                "{id}: {instance} is not an instance of {name}"
            );
        }
    }
}

// ----- the registry against [CFG §10] ----------------------------------------------------------------------------------

/// One key of [CFG §10] as its table writes it: the key, its type cell, its default token and its `vis` cell.
struct CfgKey {
    key: String,
    ty: String,
    default: String,
    vis: String,
}

/// The backticked tokens of a cell, in order.
fn ticks(cell: &str) -> Vec<&str> {
    cell.split('`').skip(1).step_by(2).collect()
}

/// The first backticked token of a cell, or the whole cell.
fn ticked(cell: &str) -> &str {
    ticks(cell).first().copied().unwrap_or(cell)
}

/// The cells of a pipe-table line, split on `|` that is not escaped as `\|`.
fn cells(line: &str) -> Vec<String> {
    let body = line.trim().trim_start_matches('|').trim_end_matches('|');
    let mut out = vec![String::new()];
    let mut prev = ' ';
    for ch in body.chars() {
        if ch == '|' && prev != '\\' {
            out.push(String::new());
        } else {
            out.last_mut().expect("a cell").push(ch);
        }
        prev = ch;
    }
    out.into_iter()
        .map(|c| c.trim().replace("\\|", "|"))
        .collect()
}

/// Every key [CFG §10.1] to §10.11 registers, and the policy-data names of §10.13, parsed from the chapter. A cell
/// naming several keys (`pack.quota.c2`, `.c3`, …) gives one key each, with one default each or one for all (`` `true`
/// each``); the budget keys `query.budget.default.<b>` and `query.caps.<role>.<b>` take their types and defaults from
/// the `<b>` table of §10.5.
fn config_md() -> (Vec<CfgKey>, Vec<String>) {
    let text = include_str!("../../../../docs/spec/config.md");
    let mut keys = Vec::new();
    let mut policy = Vec::new();
    // `<b>` → (type, the default of `query.budget.default.<b>`, the agent maximum, the orchestrator's and owner's).
    let mut budgets: Vec<(String, String, String, String, String)> = Vec::new();
    let mut section = 0u32;
    let mut header: Option<Vec<String>> = None;
    // A cap default: the cell's value, or its rule, `× 10` making the tenfold rule.
    let cap = |cell: &str| -> String {
        let t = ticked(cell).to_string();
        if t.starts_with("rule:") && cell.contains("× 10") {
            format!("{t}-x10")
        } else {
            t
        }
    };
    for line in text.lines() {
        if let Some(h) = line.strip_prefix("### ") {
            section = h
                .split(' ')
                .next()
                .and_then(|x| x.strip_prefix("10."))
                .and_then(|x| x.parse().ok())
                .unwrap_or(0);
            header = None;
            continue;
        }
        if !line.starts_with('|') {
            header = None;
            continue;
        }
        let c = cells(line);
        if c.iter().all(|x| x.chars().all(|ch| ch == '-' || ch == ':')) {
            continue;
        }
        let Some(h) = &header else {
            header = Some(c);
            continue;
        };
        let col = |name: &str| -> String {
            h.iter()
                .position(|x| x == name || x.ends_with(&format!("; {name}")))
                .map(|i| c[i].clone())
                .unwrap_or_default()
        };
        if section == 5 && h[0] == "`<b>`" {
            budgets.push((
                ticked(&c[0]).to_string(),
                c[1].clone(),
                ticked(&c[2]).to_string(),
                cap(&c[3]),
                cap(&c[4]),
            ));
        } else if (1..=11).contains(&section) && h[0] == "key" {
            let (ty, dflt, vis) = (col("type"), col("default"), col("vis"));
            let named = ticks(&c[0]);
            if named.len() == 1 && named[0].ends_with(".<b>") {
                let caps = named[0].starts_with("query.caps.");
                for (b, t, d, agent, tenfold) in &budgets {
                    keys.push(CfgKey {
                        key: named[0].replace("<b>", b),
                        ty: t.clone(),
                        default: if caps {
                            format!("per-param(orchestrator:{tenfold},owner:{tenfold},*:{agent})")
                        } else {
                            d.clone()
                        },
                        vis: vis.clone(),
                    });
                }
                continue;
            }
            let first = named[0].to_string();
            let prefix = first.rsplit_once('.').map_or("", |(p, _)| p).to_string();
            let defaults = ticks(&dflt);
            for (i, k) in named.iter().enumerate() {
                let key = if k.starts_with('.') {
                    format!("{prefix}{k}")
                } else {
                    k.to_string()
                };
                let default = if named.len() > 1 && defaults.len() == named.len() {
                    defaults[i].to_string()
                } else {
                    ticked(&dflt).to_string()
                };
                keys.push(CfgKey {
                    key,
                    ty: ty.clone(),
                    default,
                    vis: vis.clone(),
                });
            }
        } else if section == 13 && h[0] == "row" {
            policy.extend(ticks(&c[0]).iter().map(|k| k.to_string()));
        }
    }
    (keys, policy)
}

/// A registry default as [CFG §10] writes it ([CFG §9.1] `default`): the empty value as `empty`, and a per-parameter
/// default without the `*` pair when every value of a closed vocabulary (`<client>`, `<profile>`) is listed, since no
/// instance then reaches it.
fn default_text(d: &registry::Def) -> String {
    use registry::{Dflt, Param};
    fn one(d: &Dflt) -> String {
        match d {
            Dflt::Val("") => "empty".into(),
            Dflt::Val(v) => v.to_string(),
            Dflt::None => "none".into(),
            Dflt::Key(k) => format!("key:{k}"),
            Dflt::Rule(r) => format!("rule:{r}"),
            Dflt::Hole(h, _) => format!("HOLE({h})"),
            Dflt::PerParam(v) => format!(
                "per-param({})",
                v.iter()
                    .map(|(p, d)| format!("{p}:{}", one(d)))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }
    let closed: &[&str] = match d.param {
        Some(Param::Client) => &["claude", "codex", "generic"],
        Some(Param::Profile) => &["gated", "compatible", "unknown"],
        _ => &[],
    };
    match d.default {
        Dflt::PerParam(v)
            if !closed.is_empty() && closed.iter().all(|c| v.iter().any(|(p, _)| p == c)) =>
        {
            let listed: Vec<String> = v
                .iter()
                .filter(|(p, _)| *p != "*")
                .map(|(p, d)| format!("{p}:{}", one(d)))
                .collect();
            format!("per-param({})", listed.join(","))
        }
        other => one(&other),
    }
}

/// Whether a [CFG §10] type cell describes the registry's type: the same base type, the same bounds where the cell
/// gives them (§10.2 gives none: [F17] is normative for the ranges), the same vocabulary.
fn type_matches(cell: &str, ty: &crate::config::Ty) -> bool {
    use crate::config::{Ty, parse};
    let t = ticked(cell);
    let (base, rest) = match t.find(['[', '(']) {
        Some(i) => (&t[..i], Some(&t[i + 1..t.len() - 1])),
        None => (t, None),
    };
    let number = |wide: Ty, s: &str| -> Option<u64> {
        if s == "0" {
            return Some(0);
        }
        parse(wide, s)?.number
    };
    let bounds = |wide: Ty| -> Option<(u64, u64)> {
        let (lo, hi) = rest?.split_once("..")?;
        Some((number(wide, lo)?, number(wide, hi)?))
    };
    let words = |v: &[&str]| rest.is_some_and(|r| r.split('|').eq(v.iter().copied()));
    match (base, ty) {
        ("bool", Ty::Bool)
        | ("ref", Ty::Ref)
        | ("words", Ty::Words)
        | ("word", Ty::Word)
        | ("path", Ty::Path)
        | ("glob-list", Ty::GlobList)
        | ("url-list", Ty::UrlList)
        | ("git-ref", Ty::GitRef)
        | ("family", Ty::Family) => rest.is_none(),
        ("int", Ty::Int(lo, hi)) => {
            rest.is_none() || bounds(Ty::Int(0, i64::MAX)) == Some((*lo as u64, *hi as u64))
        }
        ("size", Ty::Size(lo, hi) | Ty::SizePow2(lo, hi)) => {
            rest.is_none() || bounds(Ty::Size(0, u64::MAX)) == Some((*lo, *hi))
        }
        ("duration", Ty::Duration(lo, hi)) => {
            rest.is_none() || bounds(Ty::Duration(0, u64::MAX)) == Some((*lo, *hi))
        }
        ("percent", Ty::Percent(lo, hi)) => {
            rest.is_none() || bounds(Ty::Percent(0, 100)) == Some((*lo, *hi))
        }
        ("enum", Ty::Enum(v)) => words(v),
        ("set", Ty::Set(v)) => words(v),
        _ => false,
    }
}

/// The registry the model loads ([`registry::DEFS`]) equals [CFG §10] itself, parsed at test time: every key once, with
/// its type, default and first visibility class; a second class after `;` is covered by the row's second checker
/// (KY-011). The policy-data names equal §10.13's. So a transcription error in `DEFS` cannot hide behind the same error
/// in this table, whose other test compares it with `DEFS`.
#[test]
fn the_registry_equals_config_md() {
    let (rows, policy) = config_md();
    let mut seen = BTreeSet::new();
    let mut errors = Vec::new();
    let krows = key_rows();
    for r in &rows {
        let key = r.key.as_str();
        if !seen.insert(key.to_string()) {
            errors.push(format!("{key}: twice in [CFG §10]"));
        }
        let Some(d) = DEFS.iter().find(|d| d.key == key) else {
            errors.push(format!("{key}: not in DEFS"));
            continue;
        };
        if !type_matches(&r.ty, &d.ty) {
            errors.push(format!("{key}: type {} against {:?}", r.ty, d.ty));
        }
        if r.default != default_text(d) {
            errors.push(format!(
                "{key}: default {} against {}",
                r.default,
                default_text(d)
            ));
        }
        let (first, second) = match r.vis.split_once("; ") {
            Some((a, b)) => (a, b.split(' ').next()),
            None => (r.vis.as_str(), None),
        };
        if first != d.vis.token() {
            errors.push(format!("{key}: vis {} against {}", r.vis, d.vis.token()));
        }
        let kr = krows
            .iter()
            .find(|k| k.key == key)
            .expect("a policy-keys row");
        let want_second = second.map(|c| match c {
            "Rs" => "SP-2",
            "I" => "SP-1",
            other => panic!("{key}: no checker for the class {other}"),
        });
        if kr.checker.get(1).map(String::as_str) != want_second {
            errors.push(format!(
                "{}: checkers {:?} for the classes {}",
                kr.id, kr.checker, r.vis
            ));
        }
    }
    for d in DEFS {
        if !seen.contains(d.key) {
            errors.push(format!("{}: not in [CFG §10]", d.key));
        }
    }
    assert!(errors.is_empty(), "{errors:#?}");
    assert_eq!(
        policy.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        registry::POLICY_DATA
            .iter()
            .copied()
            .collect::<BTreeSet<_>>(),
        "[CFG §10.13] against the registry's policy-data names"
    );
}

// ----- invariance -----------------------------------------------------------------------------------------------------

/// The reference stream ([RULES/policy-keys] §2): creates, a blocker edge, a claim and a completion, a fork, a write on
/// the fork, then the `State` snapshots of both refs and the `Runtime` snapshot.
fn reference(mut s: Store) -> String {
    let mut out = Vec::new();
    let o = orch();
    let mut rec = |s: &mut Store, c: Cmd, ctx: &Ctx| {
        let r = s.run(&c, ctx);
        out.push(format!("{r:?}"));
        r
    };
    rec(
        &mut s,
        tx(vec![
            create("task", "A"),
            create("task", "B"),
            create("note", "N"),
        ]),
        &o,
    );
    rec(
        &mut s,
        tx(vec![Stmt::Link {
            src: Target::Id(Nid(1)),
            kind: "blocks".into(),
            dst: Target::Id(Nid(2)),
            pinned: None,
        }]),
        &o,
    );
    let c = rec(&mut s, claim(&[1], Some("w1"), None, None), &o);
    let lease = lease_of(&c);
    rec(
        &mut s,
        Cmd::Complete {
            id: Target::Id(Nid(1)),
            outcome: "done".into(),
            summary: "ok".into(),
            evidence: vec![],
            move_lease: None,
        },
        &Ctx {
            lease: Some(lease),
            client: Some("claude".into()),
            ..Default::default()
        },
    );
    rec(
        &mut s,
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        &o,
    );
    rec(
        &mut s,
        tx(vec![set(2, "title", P::Text("B2".into()))]),
        &Ctx {
            branch: Some("lane/x".into()),
            ..o.clone()
        },
    );
    for r in ["main", "lane/x"] {
        rec(
            &mut s,
            Cmd::State {
                ref_: Some(r.into()),
                at: None,
                parts: None,
            },
            &o,
        );
    }
    rec(&mut s, Cmd::Runtime, &o);
    out.join("\n")
}

fn invariant(key: &str, v: &str, base: &str) -> Result<(), String> {
    let got = reference(store_with(key, v));
    if got != base {
        return Err(format!("{key} = {v} changes the reference stream"));
    }
    Ok(())
}

// ----- the functions --------------------------------------------------------------------------------------------------

/// The packages whose model functions exist in this crate. A row whose function belongs to another package is pending
/// ([RULES/policy-keys] §2): its values run through the invariance of the reference stream only. When a package lands
/// it is added here, and every row of its functions then needs an arm in [`check`] ("no test for the function").
const LANDED: &[&str] = &["WP-90", "WP-91", "WP-92"];

/// The rows pending on a later package (M5), by id: counted and reported apart from the tested rows, never as tests of
/// their functions.
const PENDING: &[&str] = &["KY-153", "KY-154", "KY-157", "KY-162"];

/// Whether a function's package has landed.
fn landed(function: &str) -> bool {
    LANDED.contains(&wp_of(function))
}

/// The pending rows are exactly [`PENDING`], so a package that lands, or a `wp` cell that changes, fails here until the
/// list, the landed packages and the test arms agree.
#[test]
fn pending_rows_are_the_later_packages() {
    let mut got: Vec<String> = key_rows()
        .into_iter()
        .filter(|r| r.function != "invariance" && !landed(&r.function))
        .map(|r| r.id)
        .collect();
    got.extend(
        policy_rows()
            .into_iter()
            .filter(|(_, _, _, f, _)| !landed(f))
            .map(|(id, ..)| id),
    );
    assert_eq!(got, PENDING, "the rows pending on a later package");
    for id in PENDING {
        let f = key_rows()
            .into_iter()
            .find(|r| r.id == *id)
            .map(|r| r.function)
            .or_else(|| policy_rows().into_iter().find(|p| p.0 == *id).map(|p| p.3))
            .expect("a row");
        assert!(["M5"].contains(&wp_of(&f)), "{id}: {f} is of {}", wp_of(&f));
    }
}

/// One value of one row through its function; `Err` names the failure. A pending row's value runs through the
/// invariance of the reference stream (DT-2: the model reads its key nowhere yet).
fn check(r: &KeyRow, v: &str, base: &str) -> Result<(), String> {
    let key = r.instance.as_str();
    let f = r.function.as_str();
    if f == "invariance" || !landed(f) {
        return invariant(key, v, base);
    }
    let fail = |what: String| Err(format!("{} {key} = {v}: {what}", r.id));
    match f {
        // Step 8 of a merge: a value conflict lands (`false`) or stages (`true`).
        "merge::land_or_stage" => {
            let mut s = store_with(key, v);
            let r = two_sided_merge(&mut s, false);
            let staged = r.outcome == Outcome::Staged;
            if staged != (v == "true") {
                return fail(format!("outcome {:?}", r.outcome));
            }
        }
        // CX-2's last step.
        "context::resolve_branch" => {
            // The ref need not exist ([F12 §2.5] IN-4); it is created first so the session claim resolves.
            let mut s = store_params(&[], &[]);
            run_ok(&mut s, tx(vec![create("note", "n")]), &orch());
            run_ok(
                &mut s,
                Cmd::BranchCreate {
                    name: "x".into(),
                    from: Some("main".into()),
                    kind: None,
                },
                &orch(),
            );
            run_ok(
                &mut s,
                Cmd::ConfigSet {
                    key: key.into(),
                    value: v.into(),
                    scope: FileScope::Store,
                },
                &Ctx::default(),
            );
            let got = s
                .resolve(&Ctx::default(), false)
                .map_err(|e| e.detail)?
                .branch;
            if got != v {
                return fail(format!("resolved {got}"));
            }
        }
        // CX-7 ([API §4.2]): `client.profile` names the client after `ctx.client` and `MOIRAI_CLIENT`, when it is
        // not `auto`; `auto` leaves the harness detection (no harness here: `generic`).
        "api::Store::resolve" => {
            let s = store_with(key, v);
            let bare = s
                .resolve(&Ctx::default(), false)
                .map_err(|e| e.detail)?
                .client;
            let mut env = Ctx::default();
            env.env.insert("MOIRAI_CLIENT".into(), "codex".into());
            let by_env = s.resolve(&env, false).map_err(|e| e.detail)?.client;
            let flag = Ctx {
                client: Some("claude".into()),
                ..Default::default()
            };
            let by_flag = s.resolve(&flag, false).map_err(|e| e.detail)?.client;
            let want = if v == "auto" { "generic" } else { v };
            if bare != want || by_env != "codex" || by_flag != "claude" {
                return fail(format!(
                    "clients {bare}, {by_env} (MOIRAI_CLIENT), {by_flag} (ctx.client)"
                ));
            }
        }
        // More suspects than the budget leave `affected` incomplete.
        "derived::affected_with_budget" => {
            let mut s = store_with(key, v);
            let o = orch();
            run_ok(&mut s, tx(vec![create("note", "T")]), &o);
            let cites: Vec<Stmt> = (0..5)
                .map(|i| Stmt::Create {
                    name: None,
                    kind: "note".into(),
                    fields: vec![("title".into(), P::Text(format!("c{i}")))],
                    body: None,
                    under: None,
                    position: None,
                    edges_out: vec![("cites".into(), Target::Id(Nid(1)))],
                    edges_in: vec![],
                })
                .collect();
            run_ok(&mut s, tx(cites), &o);
            let r2 = run_ok(
                &mut s,
                Cmd::Mutation {
                    name: "tx.retract".into(),
                    params: vec![
                        ("id".into(), P::Text("#1".into())),
                        ("reason".into(), P::Text("wrong".into())),
                    ],
                    message: String::new(),
                    move_lease: None,
                },
                &o,
            );
            let c = &s.dag.commits[&r2.rev_new.expect("a commit")];
            let want = 5 <= num(key, v);
            if c.affected_complete != want {
                return fail(format!("affected_complete {}", c.affected_complete));
            }
            // DS-010: an incomplete `affected` prints the hint `SuspectBudget` ([API §3.3]; [F19 §12.3]).
            let hinted = r2.hints.iter().any(|(c, _)| c == "SuspectBudget");
            if hinted == want {
                return fail(format!("hints {:?}", r2.hints));
            }
        }
        // The lookup's windows (CK-6).
        "idem::Table::lookup" => {
            let w = num(key, v) as i64;
            let explicit = key == "idempotency.retention";
            for (wait, replay) in [(w - 1000, true), (w + 1000, false)] {
                if wait < 0 {
                    continue;
                }
                let mut s = store_with(key, v);
                let ctx = Ctx {
                    key: explicit.then(|| "k1".to_string()),
                    no_dedupe: false,
                    ..orch()
                };
                run_ok(&mut s, tx(vec![create("note", "n")]), &ctx);
                advance(&mut s, wait);
                let r = s.run(&tx(vec![create("note", "n")]), &ctx);
                if (r.outcome == Outcome::Replayed) != replay {
                    return fail(format!("after {wait} ms: {:?}", r.outcome));
                }
            }
        }
        // `Gc`: reflog entries younger than P30, and commits younger than P31, keep a commit.
        "gc::reachable_after_gc" => {
            let reflog = key == "gc.reflog-expire";
            let v_ms = num(key, v) as i64;
            let other = if reflog { 14 * DAY } else { 90 * DAY };
            let cases: Vec<(i64, bool)> = if reflog {
                // Kept while the reflog is young; dropped once both windows passed.
                let mut c = vec![(v_ms.max(other) + 1000, true)];
                if v_ms > 1000 {
                    c.push((v_ms - 1000, false));
                }
                c
            } else {
                vec![(other + 1000, other + 1000 >= v_ms)]
            };
            for (wait, dropped) in cases {
                let mut s = store_with(key, v);
                let o = orch();
                run_ok(&mut s, tx(vec![create("note", "n")]), &o);
                run_ok(
                    &mut s,
                    Cmd::BranchCreate {
                        name: "y".into(),
                        from: Some("main".into()),
                        kind: None,
                    },
                    &o,
                );
                let r = run_ok(
                    &mut s,
                    tx(vec![create("note", "m")]),
                    &Ctx {
                        branch: Some("lane/y".into()),
                        ..o.clone()
                    },
                );
                let c2 = r.rev_new.expect("a commit");
                run_ok(
                    &mut s,
                    Cmd::BranchDelete {
                        name: "lane/y".into(),
                        force: true,
                    },
                    &o,
                );
                advance(&mut s, wait);
                run_ok(
                    &mut s,
                    Cmd::Gc {
                        reflog_expire_ms: None,
                        cruft_delay_ms: None,
                        force: false,
                    },
                    &o,
                );
                let st = s.run(
                    &Cmd::State {
                        ref_: None,
                        at: Some(c2),
                        parts: None,
                    },
                    &o,
                );
                if (code(&st) == Some("E301")) != dropped {
                    return fail(format!("after {wait} ms: {:?}", st.outcome));
                }
            }
        }
        // What a crash may lose.
        "crash::survives" => {
            let c = conf_with(key, v);
            let set = c.text(key);
            for k in crate::crash::KINDS {
                let lazy = set.split(',').any(|x| x == k);
                if crate::crash::survives(k, &set) == lazy {
                    return fail(k.to_string());
                }
            }
            let expect = if set.split(',').any(|x| x == "heartbeat") {
                vec![2, 1]
            } else {
                vec![2]
            };
            if crate::crash::deadlines_after_crash(1, 2, false, &set) != expect {
                return fail("the deadlines after a crash".into());
            }
        }
        // A measuring lane implies quiet mode while the key is true; quiet mode refuses `Gc` without `force`.
        "quiet::in_quiet_mode" => {
            let c = conf_with(key, v);
            let want = v == "true";
            if crate::quiet::in_quiet_mode(false, c.flag(key), true) != want {
                return fail("in_quiet_mode".into());
            }
            let gc = || Cmd::Gc {
                reflog_expire_ms: None,
                cruft_delay_ms: None,
                force: false,
            };
            // Without the flag: a lane with status `measuring` on `main` makes the store quiet exactly while the key
            // is true (DE-027).
            let mut s = store_with(key, v);
            run_ok(&mut s, tx(vec![create("lane", "L")]), &orch());
            run_ok(
                &mut s,
                tx(vec![set(1, "status", P::Text("measuring".into()))]),
                &orch(),
            );
            let r = s.run(&gc(), &orch());
            if (code(&r) == Some("quiet_mode")) != want {
                return fail(format!("Gc beside a measuring lane: {:?}", r.error));
            }
            // With the flag the store is quiet whatever the key.
            run_ok(&mut s, Cmd::Quiet { on: true }, &orch());
            if code(&s.run(&gc(), &orch())) != Some("quiet_mode") {
                return fail("Gc under the flag".into());
            }
        }
        // The TTL of a new lease.
        "lease::ttl_for" => {
            let mut s = store_with(key, v);
            let ttl = if key == "lease.ttl-default" {
                run_ok(&mut s, tx(vec![create("task", "t")]), &orch());
                let r = run_ok(&mut s, claim(&[1], Some("w1"), None, None), &orch());
                let id = crate::tx::parse_lease(&lease_of(&r)).unwrap();
                s.leases[&id].ttl_ms
            } else {
                s.leases[&1].ttl_ms
            };
            if ttl != num(key, v) {
                return fail(format!("ttl {ttl}"));
            }
        }
        // `Reclaim` without arguments releases the task leases claimed longer ago than the key.
        "lease::reclaim" => {
            let w = num(key, v) as i64;
            for (wait, released) in [(w - 1000, false), (w + 1000, true)] {
                let mut s = store_with(key, v);
                run_ok(&mut s, tx(vec![create("task", "t")]), &orch());
                run_ok(
                    &mut s,
                    Cmd::RunOpen {
                        name: "r1".into(),
                        fields: vec![],
                    },
                    &orch(),
                );
                let r = run_ok(&mut s, claim(&[1], Some("w1"), None, Some("r1")), &orch());
                let id = crate::tx::parse_lease(&lease_of(&r)).unwrap();
                advance(&mut s, wait);
                run_ok(
                    &mut s,
                    Cmd::Reclaim {
                        older_than_ms: None,
                        run: None,
                    },
                    &orch(),
                );
                if s.leases[&id].ended.is_some() != released {
                    return fail(format!("after {wait} ms"));
                }
            }
        }
        "profile::read_safelist" => {
            let c = conf_with(key, v);
            if crate::profile::read_safelist(&c, "developer") != (v == "named-only") {
                return fail("read_safelist".into());
            }
        }
        // The caps of a block, and a block at the cap on a store.
        "budget::check_caps" => {
            let cap = num(key, v);
            let (ms, mo) = if key == "tx.max-statements" {
                (cap, 10_000)
            } else {
                (1000, cap)
            };
            let at = |x: u64| {
                if key == "tx.max-statements" {
                    crate::budget::check_caps(ms, mo, x, 0)
                } else {
                    crate::budget::check_caps(ms, mo, 0, x)
                }
            };
            if at(cap).is_err() || at(cap + 1).map_err(|e| e.code).err().as_deref() != Some("E501")
            {
                return fail("check_caps".into());
            }
            if cap == 1 {
                let mut s = store_with(key, v);
                let r = s.run(&tx(vec![create("note", "a"), create("note", "b")]), &orch());
                if code(&r) != Some("E501") {
                    return fail(format!("a two-statement block: {:?}", r.error));
                }
            }
        }
        // A session's profile, and E411 for the `unknown` profile.
        "profile::model_profile" => {
            if key.starts_with("lq.model-profile.default.") {
                let mut c = conf_with(key, v);
                c.store
                    .insert("lq.model-profile.gpt-5-6-luna".into(), "compatible".into());
                let want = if v == "unknown" {
                    Profile::Unknown
                } else {
                    Profile::Compatible
                };
                if crate::profile::model_profile(&c, None, "codex") != want {
                    return fail("model_profile".into());
                }
            } else {
                // WR-012's scope ([API §9.1] E411 row; spec sync 2b): a caller with a session identity (CX-4).
                let mut s = store_with(key, v);
                let mut ctx = orch();
                ctx.env.insert("CLAUDECODE".into(), "1".into());
                ctx.env.insert("CLAUDE_CODE_SESSION_ID".into(), "s1".into());
                let r = s.run(&tx(vec![create("note", "n")]), &ctx);
                if (code(&r) == Some("E411")) != (v == "unknown") {
                    return fail(format!("{:?}", r.error));
                }
            }
        }
        // The write rule of the `unknown` profile: a Codex caller's free-form `TX`.
        "profile::model_write_rule" => {
            let mut s = store_with(key, v);
            // A Codex session identity (CX-4) puts the block in WR-012's scope (spec sync 2b).
            let mut codex = Ctx {
                client: Some("codex".into()),
                agent: Some("c1".into()),
                no_dedupe: true,
                ..Default::default()
            };
            codex.env.insert("CODEX_THREAD_ID".into(), "t1".into());
            let r = s.run(&tx(vec![create("note", "n")]), &codex);
            if (code(&r) == Some("E411")) != (v != "off") {
                return fail(format!("{:?}", r.error));
            }
            let dry = s.run(&tx(vec![create("note", "n")]), &Ctx { dry: true, ..codex });
            if (dry.outcome == Outcome::Dry) != (v != "named-only") {
                return fail(format!("DRY: {:?}", dry.error));
            }
        }
        // One run per Workflow run, or per agent call.
        "runs::open_policy" => {
            let mut s = store_with(key, v);
            let calls: Vec<crate::runs::Call> = ["a", "b", "c"]
                .iter()
                .map(|x| crate::runs::Call { id: x.to_string() })
                .collect();
            let names = crate::runs::open_policy(&s.conf.text(key), "wf1", &calls);
            for n in &names {
                run_ok(
                    &mut s,
                    Cmd::RunOpen {
                        name: n.clone(),
                        fields: vec![],
                    },
                    &orch(),
                );
            }
            let want = if v == "agent-call" { 3 } else { 1 };
            if names.len() != want || s.next_id != want as u32 + 1 {
                return fail(format!("{names:?}"));
            }
        }
        // GR-019 and WR-015: an owner-attested rule and decision (`tx.remember` with `authority=owner` and an owner
        // quote, WT-012) are created `active` and `accepted` under `orchestrator-active`, `proposed` under `strict`; a
        // rule without the attestation starts `proposed` under both. Under `strict` the orchestrator's later move of
        // the rule to `active` is refused (E406, WR-015) and the owner's attested `tx.set` confirms it; under
        // `orchestrator-active` WR-015 does not apply.
        "status::knowledge_initial" => {
            let strict = v == "strict";
            let mut s = store_with(key, v);
            let quote = |extra: &[(&str, &str)]| -> Vec<(String, P)> {
                let mut fields = vec![
                    P::Text("authority=owner".into()),
                    P::Text("owner_quote=Use fencing tokens.".into()),
                ];
                let mut out: Vec<(String, P)> = Vec::new();
                for (k, x) in extra {
                    if *k == "status" {
                        fields.push(P::Text(format!("status={x}")));
                    } else {
                        out.push((k.to_string(), P::Text(x.to_string())));
                    }
                }
                out.push(("fields".into(), P::List(fields)));
                out
            };
            let remember = |kind: &str, title: &str, owner: bool| Cmd::Mutation {
                name: "tx.remember".into(),
                params: {
                    let mut p = vec![
                        ("kind".into(), P::Text(kind.into())),
                        ("title".into(), P::Text(title.into())),
                        (
                            "text".into(),
                            P::Text("Every lease mutation carries a token.".into()),
                        ),
                    ];
                    if owner {
                        p.extend(quote(&[]));
                    }
                    p
                },
                message: String::new(),
                move_lease: None,
            };
            run_ok(&mut s, remember("rule", "r1", true), &orch());
            run_ok(&mut s, remember("decision", "d1", true), &orch());
            run_ok(&mut s, remember("rule", "r2", false), &orch());
            let status = |s: &Store, n: u32| {
                s.dag
                    .state_at(s.dag.live("main").unwrap().tip, &s.alloc)
                    .nodes[&Nid(n)]
                    .status
                    .clone()
            };
            let (rule, decision) = if strict {
                ("proposed", "proposed")
            } else {
                ("active", "accepted")
            };
            let created = status(&s, 1) == rule
                && status(&s, 2) == decision
                && status(&s, 3) == "proposed"
                && crate::status::knowledge_initial("rule", true, strict) == Some(rule);
            // A `Create` that names another status than GR-019's is refused.
            let named = s.run(
                &Cmd::Mutation {
                    name: "tx.remember".into(),
                    params: {
                        let mut p = vec![
                            ("kind".into(), P::Text("rule".into())),
                            ("title".into(), P::Text("r3".into())),
                            ("text".into(), P::Text("t".into())),
                        ];
                        p.extend(quote(&[(
                            "status",
                            if strict { "active" } else { "proposed" },
                        )]));
                        p
                    },
                    message: String::new(),
                    move_lease: None,
                },
                &orch(),
            );
            let orch_move = s.run(
                &tx(vec![set(1, "status", P::Text("active".into()))]),
                &orch(),
            );
            let confirmed = if strict {
                let r = s.run(
                    &Cmd::Mutation {
                        name: "tx.set".into(),
                        params: {
                            let mut p = vec![
                                ("id".into(), P::Text("#1".into())),
                                ("status".into(), P::Text("active".into())),
                            ];
                            p.extend(quote(&[]));
                            p
                        },
                        message: String::new(),
                        move_lease: None,
                    },
                    &orch(),
                );
                code(&orch_move) == Some("E406")
                    && r.outcome == Outcome::Ok
                    && status(&s, 1) == "active"
            } else {
                orch_move.outcome == Outcome::Ok && status(&s, 1) == "active"
            };
            if !(created && code(&named) == Some("E404") && confirmed) {
                return fail(format!(
                    "statuses {} {} {}; named {:?}; orchestrator's move {:?}",
                    status(&s, 1),
                    status(&s, 2),
                    status(&s, 3),
                    named.error,
                    orch_move.error
                ));
            }
        }
        // A budget's effective value against the ceiling.
        "budget::effective" => {
            let c = conf_with(key, v);
            let b = key.rsplit('.').next().unwrap();
            let x = num(key, v);
            let cap = |c: &Conf| c.number(&format!("query.caps.developer.{b}"));
            let (got, want) = if key.starts_with("query.budget.default.") {
                (
                    crate::budget::effective(&c, b, "developer", Proc::Cli, None, None).0,
                    x.min(cap(&c)),
                )
            } else {
                (
                    crate::budget::effective(
                        &c,
                        b,
                        "developer",
                        Proc::Cli,
                        Some(u64::MAX / 2),
                        None,
                    )
                    .0,
                    x,
                )
            };
            let floor = if b == "mem" || b == "wmem" {
                256 * 1024
            } else {
                0
            };
            if got != want.max(floor) {
                return fail(format!("{got} != {want}"));
            }
        }
        "pack::budget" => {
            let c = conf_with(key, v);
            let kind = match key {
                "brief.budget" => "brief",
                "hooks.subagent-start.budget" => "hook-role-pack",
                "hooks.delta.budget" => "prompt-delta",
                _ => "role-pack",
            };
            if pack::budget(&c, kind, "developer") != num(key, v) {
                return fail("budget".into());
            }
        }
        "pack::ceiling" => {
            let c = conf_with(key, v);
            let x = num(key, v);
            let (got, want) = match key {
                "pack.cli.max-bytes" => (pack::ceiling(&c, Surface::Cli, "claude"), x),
                "pack.mcp.max-bytes" => (pack::ceiling(&c, Surface::Mcp, "claude"), x.min(25_000)),
                "mcp.result-max-bytes" => {
                    (pack::ceiling(&c, Surface::Mcp, "claude"), x.min(25_000))
                }
                _ => (pack::ceiling(&c, Surface::Mcp, "codex"), x.min(25_000)),
            };
            if got != Some(want) {
                return fail(format!("{got:?}"));
            }
        }
        "pack::quota" => {
            let c = conf_with(key, v);
            let (class, role) = match key {
                "pack.quota.c2" => ("C2", "developer"),
                "pack.quota.c3" => ("C3", "developer"),
                "pack.quota.c4-dev" => ("C4", "developer"),
                "pack.quota.c4-critic" => ("C4", "architecture-critic"),
                _ => ("C5", "developer"),
            };
            if pack::quota(&c, class, role) != num(key, v) {
                return fail("quota".into());
            }
        }
        "pack::notice_mode" => {
            if pack::notice_mode(&conf_with(key, v)).0 != v {
                return fail("notice mode".into());
            }
        }
        "hooks::installed" => {
            let c = conf_with(key, v);
            let (harness, hook, want) = match key {
                "files.hooks.evidence" => ("claude", "mv-rm-evidence", v == "true"),
                "files.hooks.edit-evidence" => ("claude", "write-edit-evidence", v != "off"),
                "hooks.transport" => ("generic", "write-edit-evidence", v == "mcp"),
                k => (
                    "claude",
                    k.strip_prefix("hooks.")
                        .and_then(|x| x.strip_suffix(".enabled"))
                        .expect("a hook key"),
                    v == "true",
                ),
            };
            if crate::hooks::installed(&c, harness).contains(&hook) != want {
                return fail(format!("{hook} under {harness}"));
            }
        }
        "hooks::session_start" => {
            let mut s = store_with(key, v);
            let mut ctx = Ctx {
                agent: Some("orch2".into()),
                ..Default::default()
            };
            ctx.env.insert("CLAUDECODE".into(), "1".into());
            let worker = key == "hooks.session-start.worker-pack";
            let w = crate::hooks::session_start(
                &s.conf,
                crate::hooks::Source::Startup,
                worker,
                false,
                &ctx,
            );
            let on = v == "true";
            let ok = match key {
                "hooks.session-start.orchestrator-lease" => {
                    let has = w
                        .iter()
                        .any(|x| matches!(x, crate::hooks::Write::Command(c, _) if matches!(**c, Cmd::Claim { .. })));
                    let replies = crate::hooks::run(&mut s, &w);
                    has == on && replies.iter().all(|r| r.outcome == Outcome::Ok)
                }
                "hooks.session-start.settle" => {
                    let has = w.iter().any(|x| {
                        matches!(x, crate::hooks::Write::Command(c, cx) if matches!(**c, Cmd::LinksSync { .. }) && cx.door == crate::api::Door::Hook)
                    });
                    let replies = crate::hooks::run(&mut s, &w);
                    has == on && replies.iter().all(|r| r.outcome == Outcome::Ok)
                }
                _ => w.contains(&crate::hooks::Write::Read("role-pack")) == on,
            };
            if !ok {
                return fail(format!("{w:?}"));
            }
        }
        "hooks::subagent_start" => {
            let c = conf_with(key, v);
            let p = crate::hooks::SyncPreview {
                conflicts: 0,
                violations: 0,
                keys: 100,
            };
            let got = crate::hooks::subagent_start(&c, Some(("lane/x".into(), p)))
                .iter()
                .any(|w| matches!(w, crate::hooks::Write::Command(c, _) if matches!(**c, Cmd::Sync { check: false, .. })));
            let want = if key == "hooks.sync-auto-keys" {
                num(key, v) >= 100
            } else {
                v == "true"
            };
            if got != want {
                return fail("auto-sync".into());
            }
        }
        "hooks::stamp" => {
            let c = conf_with(key, v);
            let (got, want) = if key == "hooks.stamp.permission" {
                (crate::hooks::stamp(&c, None), v)
            } else {
                (
                    crate::hooks::stamp(&c, Some("edge-delete")),
                    if v.contains("edge-delete") {
                        "ask"
                    } else {
                        "allow"
                    },
                )
            };
            if got != want {
                return fail(got.to_string());
            }
        }
        f if f.starts_with("links::") => {
            if let Err(e) = links_arm(f, key, v) {
                return fail(e);
            }
        }
        other => return fail(format!("no test for the function {other}")),
    }
    Ok(())
}

/// A store with the key set, the orchestrator's session lease, a simulated tree at `root` bound to `main` as its
/// designated tree, holding `docs/a.md`, and the orchestrator's context in it.
fn tree_store(key: &str, v: &str, root: &str) -> (Store, Ctx) {
    let mut s = store_with(key, v);
    run_ok(
        &mut s,
        Cmd::EnvTree {
            tree: root.into(),
            volume: Some("C".into()),
            caps: None,
            ops: vec![crate::r4::tree::TreeOp::Write {
                path: "docs/a.md".into(),
                bytes: b"alpha\nbeta\n".to_vec(),
                btime_ns: None,
            }],
        },
        &Ctx::default(),
    );
    let o = orch();
    run_ok(
        &mut s,
        Cmd::WorktreeBind {
            dir: root.into(),
            ref_: "main".into(),
            replace: false,
        },
        &o,
    );
    (
        s,
        Ctx {
            tree: Some(root.into()),
            ..o
        },
    )
}

/// KF-037 through a resolution ([F20 §2.4] item 1): under the read limit `p` carries, the only candidate of a moved
/// file, an identical copy one byte above the limit, is `Unavailable(size)`, so the link is `unverified (size)`, never
/// `missing`; a copy at the limit (checked for the smallest limits) is read and proposed.
fn size_limit_resolves(p: &crate::r4::cascade::Params) -> bool {
    use crate::r4::cascade::{FileNode, Params, Runtime, View, resolve_file};
    use crate::r4::strings::State as L;
    use crate::r4::tree::{Fs, TreeOp, VolumeCaps};
    let limit = p.max_read_bytes.expect("a read limit") as usize;
    let q = Params {
        settle: true,
        max_read_bytes: p.max_read_bytes,
        ..Params::default()
    };
    let state = |len: usize| {
        let body: Vec<u8> = (0..len).map(|i| b'a' + (i % 23) as u8).collect();
        let root = "C:/w";
        let mut fs = Fs::default();
        fs.ensure_tree(root, "C", VolumeCaps::NTFS, crate::r4::path::Os::Windows);
        for (op, t) in [
            (
                TreeOp::Write {
                    path: "d/x.bin".into(),
                    bytes: body.clone(),
                    btime_ns: None,
                },
                1,
            ),
            (
                TreeOp::Cp {
                    from: "d/x.bin".into(),
                    to: "d/y.bin".into(),
                    keep_btime: false,
                },
                2,
            ),
            (
                TreeOp::Rm {
                    path: "d/x.bin".into(),
                },
                2,
            ),
        ] {
            fs.apply(root, &op, t).expect("a tree operation");
        }
        let f = FileNode {
            n: 1,
            root: "project".into(),
            path: "d/x.bin".into(),
            oid: Some(crate::r4::text::oid(q.algo, &body)),
            bytes: Some(len as u64),
            observed_git: None,
            observed_blob: None,
            relink: None,
            aliases: Vec::new(),
            status: crate::r4::uid::FileStatus::Present,
            artifact_kind: None,
            tombstone: false,
            obs_hlc: 0,
            conflict: None,
            path_claim: false,
        };
        let view = View {
            files: vec![f.clone()],
            ..View::default()
        };
        let r = resolve_file(
            &f,
            &view,
            &fs,
            &crate::r4::git::Git::default(),
            &Runtime::default(),
            root,
            &q,
        );
        (r.state, r.details.first().map(|d| d.code))
    };
    state(limit + 1) == (L::Unverified, Some(58))
        && (limit > (128 << 10) || state(limit) == (L::MovedNeedsConfirm, Some(10)))
}

/// The policy functions of the `files.*` and `roots.<name>` keys ([RULES/policy-keys] KF-033 to KF-044, WP-92): each
/// value's effect through its function, and where the key decides a command, through the command.
fn links_arm(f: &str, key: &str, v: &str) -> Result<(), String> {
    let c = conf_with(key, v);
    let text = value_text(v);
    let list = |t: Option<&str>| -> Vec<String> {
        t.unwrap_or("")
            .split(',')
            .filter(|x| !x.is_empty())
            .map(str::to_string)
            .collect()
    };
    let add = || Cmd::FileAdd {
        paths: vec!["docs/a.md".into()],
        kind: None,
        root: None,
    };
    let ok = match f {
        // `roots.docs`: a named root maps to its directory, else `unmapped root`; `FileAdd --root docs` registers under it.
        "links::root_dir" => {
            let got = crate::links::root_dir(&c, "docs");
            let (mut s, ctx) = tree_store(key, v, "/work/x");
            let r = s.run(
                &Cmd::FileAdd {
                    paths: vec!["docs/a.md".into()],
                    kind: None,
                    root: Some("docs".into()),
                },
                &ctx,
            );
            let registered =
                matches!(&r.data, crate::api::Data::FileAdd(f) if f[0].0.root == "docs");
            match text {
                None => got.is_none() && r.outcome == Outcome::Refused,
                Some(d) => {
                    got.as_deref() == Some(crate::links::canon_abs(d).as_str()) && registered
                }
            }
        }
        // `files.main-tree`: the configuration pair of `main` joins the designation relation.
        "links::designated_tree" => {
            let mut s = store_with(key, v);
            if let Some(d) = text {
                run_ok(
                    &mut s,
                    Cmd::EnvTree {
                        tree: d.into(),
                        volume: None,
                        caps: None,
                        ops: vec![],
                    },
                    &Ctx::default(),
                );
            }
            let d = s.designation();
            match text {
                None => d.is_empty(),
                Some(t) => {
                    d.len() == 1 && d[0].branch == "main" && d[0].tree == crate::links::canon_abs(t)
                }
            }
        }
        // `files.main-ref`: the expected git ref of the configuration pair.
        "links::tree_gate" => crate::links::tree_gate(&c).as_deref() == text,
        // `files.cloud`: `refuse` refuses a link target under a cloud sync root.
        "links::cloud_policy" => {
            let (mut s, ctx) = tree_store(key, v, "C:/cloud");
            s.files
                .fs
                .trees
                .get_mut("C:/cloud")
                .expect("the tree")
                .cloud_root = true;
            let r = s.run(&add(), &ctx);
            crate::links::cloud_policy(&c) == (v == "refuse")
                && (r.outcome == Outcome::Refused) == (v == "refuse")
        }
        // `files.max-read-bytes`: a file above it has no content read (no `oid` recorded), and in a resolution its content
        // is `Unavailable(size)` to the file and the anchor cascades ([F20 §2.4] item 1).
        "links::content_available" => {
            let limit = c.number("files.max-read-bytes") as usize;
            let (s, ctx) = tree_store(key, v, "C:/work");
            let caller = s.resolve(&ctx, false).expect("the caller");
            let tc = s.tree_ctx(&caller, &ctx).expect("the tree");
            let p = s.params(&tc, "main", true, &crate::r4::cascade::View::default());
            let wired = p.max_read_bytes == Some(limit as u64)
                && crate::links::anchor_consts(&c).max_read_bytes == Some(limit as u64);
            crate::links::content_available(&c, limit)
                && !crate::links::content_available(&c, limit + 1)
                && wired
                && (limit > (16 << 20) || size_limit_resolves(&p))
        }
        // `files.max-line-hashes`: the anchor constants' line-hash cap.
        "links::window_available" => {
            let n = text.and_then(|t| t.parse::<usize>().ok());
            crate::links::window_available(&c) == n
                && crate::links::anchor_consts(&c).max_line_hashes == n
        }
        // `files.policy.auto`: `strong` puts the policy into every resolution's parameters.
        "links::auto_policy" => {
            let (s, ctx) = tree_store(key, v, "C:/work");
            let caller = s.resolve(&ctx, false).expect("the caller");
            let tc = s.tree_ctx(&caller, &ctx).expect("the tree");
            let p = s.params(&tc, "main", true, &crate::r4::cascade::View::default());
            crate::links::auto_policy(&c) == (v == "strong") && p.policy_strong == (v == "strong")
        }
        // `files.scratchpads`: `refuse` refuses a path in a session scratchpad.
        "links::scratchpad_policy" => {
            let (mut s, ctx) = tree_store(key, v, "C:/tmp/claude/p/s/scratchpad");
            let r = s.run(&add(), &ctx);
            crate::links::scratchpad_policy(&c) == (v == "allow")
                && (r.outcome == Outcome::Refused) == (v == "refuse")
        }
        // `files.ignore`: the patterns of a tree with no git and no ignore file.
        "links::ignored" => crate::links::ignored(&c) == list(text),
        // `files.deletion-inference`.
        "links::deletion_inference" => {
            crate::links::deletion_inference(&c) == (v == "main-tree-commits")
        }
        // `files.confirm-roles`: the role cell of `links fix --confirm` (WV-038).
        "links::confirm_rights" => {
            let roles = crate::links::confirm_rights(&c);
            let s = store_with(key, v);
            let o = orch();
            let caller = s.resolve(&o, false).expect("the caller");
            let allowed = s.rights(&caller, &o).verb("links-fix-confirm").is_ok();
            roles == list(text) && allowed == roles.contains(&"orchestrator".to_string())
        }
        // `files.portable-names`: `refuse` refuses a destination some OS cannot hold, `warn` warns.
        "links::portable_name_policy" => {
            let (mut s, ctx) = tree_store(key, v, "C:/work");
            let r = s.run(
                &Cmd::FileMv {
                    srcs: vec!["docs/a.md".into()],
                    dst: "docs/aux.md".into(),
                    git: false,
                    retry_ms: None,
                },
                &ctx,
            );
            crate::links::portable_name_policy(&c) == (v == "refuse")
                && if v == "refuse" {
                    r.error.as_ref().map(|e| e.code.as_str()) == Some("nonportable_name")
                } else {
                    r.outcome == Outcome::Ok && r.warnings.contains(&"nonportable_name".to_string())
                }
        }
        other => return Err(format!("no test for the function {other}")),
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{f}: the value's effect differs"))
    }
}

/// Every allowed value of every key ([RULES/policy-keys] §2; [CFG §9.5]): through its function, or, for a key the
/// model reads nowhere, by the invariance of the reference stream. The values of pending rows are counted apart.
#[test]
fn every_allowed_value_of_every_key() {
    let base = reference(store_params(&[], &[]));
    let mut failures = Vec::new();
    let (mut tested, mut pending) = (0, 0);
    for r in key_rows() {
        for v in &r.values {
            assert!(
                v == UNSET || parsed(&r.instance, v).is_some(),
                "{}: {v} is not a valid value of {}",
                r.id,
                r.instance
            );
            if let Err(e) = check(&r, v, &base) {
                failures.push(e);
            }
            if r.function != "invariance" && !landed(&r.function) {
                pending += 1;
            } else {
                tested += 1;
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    assert!(tested > 400, "{tested} values tested");
    // Reported apart: 9 values of the 4 pending key rows.
    assert_eq!(pending, 9, "the values of the pending key rows");
}

// ----- policy data ----------------------------------------------------------------------------------------------------

/// Words of a role-set value.
fn words(v: &str) -> Vec<String> {
    value_text(v)
        .filter(|t| !t.is_empty())
        .map(|t| t.split(',').map(str::to_string).collect())
        .unwrap_or_default()
}

/// Sets a policy-data row on `main` by a `Schema` write ([RULES/policy-keys] §2; [API §9.8]; [F08 §8.5.6]).
fn set_policy(s: &mut Store, name: &str, value: &str) {
    run_ok(
        s,
        Cmd::Schema {
            items: vec![crate::schema::Item::Policy(crate::schema::PolicyItem {
                name: name.into(),
                value: Some(value.into()),
            })],
            message: String::new(),
        },
        &orch(),
    );
}

/// The policy data of `main`'s view.
fn main_policy(s: &Store) -> PolicyData {
    PolicyData::of(
        &s.dag
            .state_at(s.dag.live("main").unwrap().tip, &s.alloc)
            .schema,
    )
}

fn rights(role: &str, data: PolicyData) -> Rights {
    Rights {
        role: role.into(),
        label: None,
        surface: crate::policy::Surface::Cli,
        lease: None,
        actor: None,
        owner_attested: true,
        acceptor: None,
        data,
        confirm_roles: vec!["orchestrator".into(), "owner".into()],
        knowledge_strict: false,
    }
}

/// Every value of every policy-data row ([RULES/policy-keys] §6), each set by a `Schema` write on `main` (§2); the view
/// without the row's item takes its default.
#[test]
fn every_value_of_every_policy_row() {
    for (id, name, instance, _, values) in policy_rows() {
        for v in &values {
            let text = words(v).join(",");
            let ok = match name.as_str() {
                "policy.self-claim-roles" => {
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, &text);
                    run_ok(
                        &mut s,
                        tx(vec![create("task", "a"), create("task", "b")]),
                        &orch(),
                    );
                    let me = Ctx {
                        agent: Some("w".into()),
                        client: Some("claude".into()),
                        no_dedupe: true,
                        ..Default::default()
                    };
                    let tester =
                        s.run(&claim(&[1], None, Some("tester"), None), &me).outcome == Outcome::Ok;
                    let dev = s.run(&claim(&[2], None, None, None), &me).outcome == Outcome::Ok;
                    let set = words(v);
                    tester == set.contains(&"tester".to_string())
                        && dev == set.contains(&"developer".to_string())
                }
                "policy.mint.role-lease" => {
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, &text);
                    run_ok(
                        &mut s,
                        Cmd::RunOpen {
                            name: "r1".into(),
                            fields: vec![],
                        },
                        &orch(),
                    );
                    let r = s.run(
                        &Cmd::Claim {
                            ids: vec![],
                            next: false,
                            scope: None,
                            role: Some("developer".into()),
                            agent: Some("w".into()),
                            ttl: None,
                            start: false,
                            run: Some("r1".into()),
                            session: false,
                        },
                        &orch(),
                    );
                    (r.outcome == Outcome::Ok) == words(v).contains(&"orchestrator".to_string())
                }
                "policy.hook-label" => {
                    v == "narrow"
                        && crate::policy::narrowing_label("developer", true, Some("tester"))
                            .as_deref()
                            == Some("tester")
                }
                "policy.role.<role>.mcp-write" => {
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, v);
                    run_ok(&mut s, tx(vec![create("task", "a")]), &orch());
                    let l = lease_of(&run_ok(
                        &mut s,
                        claim(&[1], Some("dev"), None, None),
                        &orch(),
                    ));
                    let r = s.run(
                        &tx(vec![create("note", "n")]),
                        &Ctx {
                            lease: Some(l),
                            door: Door::Mcp,
                            client: Some("claude".into()),
                            no_dedupe: true,
                            ..Default::default()
                        },
                    );
                    (r.outcome == Outcome::Ok) == (v == "yes")
                }
                "policy.role.<role>.tx" => {
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, v);
                    rights("owner", main_policy(&s))
                        .statement(1, "node-delete")
                        .is_ok()
                        == (v == "per-statement")
                }
                "policy.role.developer.fields" => {
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, &text);
                    run_ok(&mut s, tx(vec![create("task", "a")]), &orch());
                    let l = lease_of(&run_ok(
                        &mut s,
                        claim(&[1], Some("dev"), None, None),
                        &orch(),
                    ));
                    let dev = Ctx {
                        lease: Some(l),
                        client: Some("claude".into()),
                        no_dedupe: true,
                        ..Default::default()
                    };
                    let title = s
                        .run(&tx(vec![set(1, "title", P::Text("A2".into()))]), &dev)
                        .outcome
                        == Outcome::Ok;
                    title == words(v).contains(&"title".to_string())
                }
                "policy.role.<role>.define-query" => {
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, v);
                    rights("orchestrator", main_policy(&s))
                        .statement(1, "define-query")
                        .is_ok()
                        == (v == "yes")
                }
                "policy.role.<role>.authority-owner" => {
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, v);
                    run_ok(&mut s, tx(vec![create("note", "n")]), &orch());
                    let st = s.dag.state_at(s.dag.live("main").unwrap().tip, &s.alloc);
                    rights("orchestrator", main_policy(&s))
                        .value(
                            1,
                            &st,
                            Nid(1),
                            "authority",
                            &crate::value::Value::Enum("owner".into()),
                        )
                        .is_ok()
                        == (v == "yes")
                }
                "edges.blocks.on-src-deleted" | "edges.gates.on-src-deleted" => {
                    let blocks = name.starts_with("edges.blocks");
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, v);
                    let src = if blocks {
                        create("task", "src")
                    } else {
                        Stmt::Create {
                            name: None,
                            kind: "verdict".into(),
                            fields: vec![
                                ("title".into(), P::Text("v".into())),
                                ("outcome".into(), P::Text("fail_fixable".into())),
                            ],
                            body: None,
                            under: None,
                            position: None,
                            edges_out: vec![],
                            edges_in: vec![],
                        }
                    };
                    run_ok(&mut s, tx(vec![src, create("task", "dst")]), &orch());
                    let kind = if blocks { "blocks" } else { "gates" };
                    run_ok(
                        &mut s,
                        tx(vec![Stmt::Link {
                            src: Target::Id(Nid(1)),
                            kind: kind.into(),
                            dst: Target::Id(Nid(2)),
                            pinned: None,
                        }]),
                        &orch(),
                    );
                    run_ok(
                        &mut s,
                        tx(vec![Stmt::Delete {
                            target: Target::Id(Nid(1)),
                            policy: None,
                            replaced_by: None,
                            release: false,
                            reason: None,
                        }]),
                        &orch(),
                    );
                    let st = s.dag.state_at(s.dag.live("main").unwrap().tip, &s.alloc);
                    let kept = st.nodes[&Nid(1)].out.keys().any(|e| e.kind == kind);
                    kept == (v == "flag")
                }
                "merge.policy.<kind>" => {
                    // `merge::auto_policy`: `ours` and `theirs` resolve a task's value conflicts to a side; `delete-wins`
                    // and `resurrect` set a task's existence policy; `none` leaves both to the tables.
                    assert!(
                        rules()
                            .table("auto-policy")
                            .rows
                            .iter()
                            .any(|r| r.tok("value") == v)
                    );
                    let prio = |s: &Store| {
                        let st = s.dag.state_at(s.dag.live("lane/x").unwrap().tip, &s.alloc);
                        let x = &st.nodes[&Nid(1)];
                        (
                            x.conflicts
                                .contains_key(&crate::state::Aspect::Field("priority".into())),
                            x.fields.get("priority").cloned(),
                            x.live(),
                            x.conflicts.contains_key(&crate::state::Aspect::Existence),
                        )
                    };
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, v);
                    two_sided_merge(&mut s, false);
                    let (conflict, value, _, _) = prio(&s);
                    let p = |x: &str| Some(crate::value::Value::Enum(x.into()));
                    let field_ok = match v.as_str() {
                        "ours" => !conflict && value == p("P3"),
                        "theirs" => !conflict && value == p("P1"),
                        _ => conflict,
                    };
                    let mut s = store_params(&[], &[]);
                    set_policy(&mut s, &instance, v);
                    two_sided_merge(&mut s, true);
                    let (_, _, live, dvm) = prio(&s);
                    let exist_ok = match v.as_str() {
                        "ours" => live && !dvm,
                        "theirs" => !live && !dvm,
                        "resurrect" => live && dvm,
                        // `delete-wins`, and `none` with the task's EP row (EP-001: delete-wins).
                        _ => !live && dvm,
                    };
                    field_ok && exist_ok
                }
                other => panic!("{id}: no test for {other}"),
            };
            assert!(ok, "{id} {name} = {v}");
        }
    }
}

/// A `Data::Config` result names the new value; `ConfigSet` and `Init` agree on a key's effective value.
#[test]
fn config_set_and_init_agree() {
    let mut a = store_params(&["lease.ttl-default=20m".into()], &[]);
    let mut b = store_params(&[], &[]);
    let r = run_ok(
        &mut b,
        Cmd::ConfigSet {
            key: "lease.ttl-default".into(),
            value: "20m".into(),
            scope: FileScope::Store,
        },
        &Ctx::default(),
    );
    assert!(matches!(r.data, Data::Config(_)));
    assert_eq!(a.cfg.ttl_default_ms, b.cfg.ttl_default_ms);
    assert_eq!(a.run(&Cmd::Runtime, &Ctx::default()).outcome, Outcome::Ok);
}
