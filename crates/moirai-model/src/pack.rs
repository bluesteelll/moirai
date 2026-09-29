//! The byte budgets of packs and briefs as data ([RULES/pack-classes] `pack-kinds`, `pack-budgets`, `pack-ceilings`,
//! `pack-quotas`, `notice-modes`; [AR §7.4]; [CFG §10.8]): the budget N of each pack kind from its key, the
//! transport ceiling of a surface and client profile, the effective budget E of PX-001, a class's minimum share, and
//! the stale-pack notice mode. Class membership, levels, order and rendering are WP-93b's evaluation of the procedure
//! tables; the byte accounting is the renderer's ([50 §4.3]).

use crate::registry::{Conf, Proc};
use crate::rules::{Row, rules};

/// A pack's delivery surface ([RULES/pack-classes] `pack-ceilings` `surface`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Surface {
    /// The CLI.
    Cli,
    /// `-o FILE`.
    File,
    /// The MCP server.
    Mcp,
    /// Hook-injected context.
    Hook,
}

impl Surface {
    fn token(self) -> &'static str {
        match self {
            Surface::Cli => "cli",
            Surface::File => "file",
            Surface::Mcp => "mcp",
            Surface::Hook => "hook",
        }
    }
}

fn pk(pack: &str) -> &'static Row {
    rules()
        .table("pack-kinds")
        .rows
        .iter()
        .find(|r| r.tok("pack") == pack)
        .unwrap_or_else(|| panic!("pack-kinds has no pack {pack}"))
}

/// N, the default budget of a pack kind for a role ([RULES/pack-classes] PK and PB rows): the effective value of the
/// row's `budget_key` (`pack.budget.<role>` for the role packs, whose defaults are the PB rows), or the row's fixed
/// `default_bytes` when it names no key (the stale-pack notice, PK-009).
// spec: [RULES/pack-classes] pack-kinds, pack-budgets
// spec: [CFG §10.8] pack.budget.<role>, brief.budget, hooks.subagent-start.budget, hooks.delta.budget
pub fn budget(conf: &Conf, pack: &str, role: &str) -> u64 {
    let r = pk(pack);
    match r.tok("budget_key") {
        "-" => r.tok("default_bytes").parse().expect("a fixed budget"),
        k => conf.number(&k.replace("<role>", role)),
    }
}

/// The ceiling of a surface under a client profile ([RULES/pack-classes] PE rows, first match): the CLI's
/// `pack.cli.max-bytes` (a user file can only lower it); none under `-o FILE`; the MCP server's `pack.mcp.max-bytes`
/// capped by the profile's MCP result ceiling `mcp.result-max-bytes.<client>`; the hook context's fixed bound.
// spec: [RULES/pack-classes] pack-ceilings
pub fn ceiling(conf: &Conf, surface: Surface, client: &str) -> Option<u64> {
    let r = rules()
        .table("pack-ceilings")
        .rows
        .iter()
        .find(|r| {
            r.tok("surface") == surface.token()
                && (r.tok("client") == "*" || r.tok("client") == client)
        })
        .unwrap_or_else(|| panic!("pack-ceilings has no row for {surface:?} {client}"));
    let v = match (r.tok("key"), r.tok("default_bytes")) {
        // PE-002: no ceiling under `-o FILE`.
        ("-", "none") => return None,
        ("-", d) => d
            .parse()
            .unwrap_or_else(|_| panic!("{}: default_bytes {d} is not a number", r.id)),
        (k, _) => conf.number(k),
    };
    Some(match surface {
        Surface::Mcp => v.min(conf.number(&format!("mcp.result-max-bytes.{client}"))),
        _ => v,
    })
}

/// E, the effective budget of a pack (PX-001): N = the explicit `--budget` (MCP `budget`) when given, else [`budget`];
/// E = N under `-o FILE`, else min(N, the surface's ceiling).
// spec: [RULES/pack-classes] PX-001
// rule: PX-001
pub fn effective(
    conf: &Conf,
    pack: &str,
    role: &str,
    surface: Surface,
    client: &str,
    explicit: Option<u64>,
) -> u64 {
    let n = explicit.unwrap_or_else(|| budget(conf, pack, role));
    match ceiling(conf, surface, client) {
        Some(c) => n.min(c),
        None => n,
    }
}

/// A class's minimum share of E in percent for a role ([RULES/pack-classes] PQ rows: the rows of the class, the first
/// that names the role, else `other`, else `*`): the row's key's effective value, or its fixed `default_pct`.
// spec: [RULES/pack-classes] pack-quotas
// spec: [CFG §10.8] pack.quota.*
pub fn quota(conf: &Conf, class: &str, role: &str) -> u64 {
    let rows: Vec<&Row> = rules()
        .table("pack-quotas")
        .rows
        .iter()
        .filter(|r| r.tok("class") == class)
        .collect();
    let r = rows
        .iter()
        .find(|r| r.toks("roles").contains(&role))
        .or_else(|| rows.iter().find(|r| r.toks("roles").contains(&"other")))
        .or_else(|| rows.iter().find(|r| r.toks("roles").contains(&"*")))
        .unwrap_or_else(|| panic!("pack-quotas has no row for {class}"));
    match r.tok("key") {
        "-" => r.int("default_pct"),
        k => conf.number(k),
    }
}

/// The stale-pack notice of `complete` and `apply` ([RULES/pack-classes] NM rows; [CFG §10.8]
/// `pack.staleness-notice`): the mode, its output form and its byte cap.
// spec: [RULES/pack-classes] notice-modes
pub fn notice_mode(conf: &Conf) -> (&'static str, &'static str, u64) {
    let m = conf.text("pack.staleness-notice");
    let r = rules()
        .table("notice-modes")
        .rows
        .iter()
        .find(|r| r.tok("mode") == m)
        .unwrap_or_else(|| panic!("notice-modes has no mode {m}"));
    (r.tok("mode"), r.tok("output"), r.int("cap_bytes"))
}

/// The process kind whose defaults a surface reads.
pub fn proc_of(surface: Surface) -> Proc {
    if surface == Surface::Mcp {
        Proc::Mcp
    } else {
        Proc::Cli
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_and_the_registry_agree_on_the_defaults() {
        let c = Conf::default();
        for r in &rules().table("pack-budgets").rows {
            let role = match r.tok("role") {
                "other" => "general-purpose",
                x => x,
            };
            assert_eq!(
                budget(&c, "role-pack", role),
                r.int("default_bytes"),
                "{}",
                r.id
            );
        }
        for r in &rules().table("pack-quotas").rows {
            if r.tok("key") != "-" {
                assert_eq!(
                    c.number(r.tok("key")),
                    r.int("default_pct"),
                    "{}: the registry default",
                    r.id
                );
            }
        }
        assert_eq!(budget(&c, "brief", "developer"), 8000);
        assert_eq!(budget(&c, "stale-notice", "developer"), 600);
        assert_eq!(ceiling(&c, Surface::Cli, "claude"), Some(24000));
        assert_eq!(ceiling(&c, Surface::File, "claude"), None);
        assert_eq!(
            ceiling(&c, Surface::Mcp, "codex"),
            Some(16000),
            "capped by the codex result ceiling"
        );
        assert_eq!(ceiling(&c, Surface::Hook, "generic"), Some(8000));
        assert_eq!(
            effective(&c, "role-pack", "architect", Surface::Cli, "claude", None),
            24000
        );
        assert_eq!(
            effective(
                &c,
                "role-pack",
                "architect",
                Surface::File,
                "claude",
                Some(90000)
            ),
            90000
        );
        assert_eq!(quota(&c, "C4", "architecture-critic"), 40);
        assert_eq!(quota(&c, "C4", "code-reviewer"), 0);
        assert_eq!(notice_mode(&c), ("lines", "per-node", 600));
    }

    /// The largest value a key instance accepts ([CFG §4.1] ranges and the per-instance bound of [CFG §10.8]).
    fn instance_max(instance: &str) -> u64 {
        let (d, _) = crate::registry::find(instance).expect("a registered key");
        let (mut lo, mut hi) = match d.ty {
            crate::config::Ty::Size(lo, hi) => (lo, hi),
            other => panic!("{instance} is not a size: {other:?}"),
        };
        let ok = |n: u64| crate::registry::validate(d, instance, &n.to_string()).is_some();
        assert!(ok(lo), "{instance} accepts its lower bound");
        while lo < hi {
            let mid = lo + (hi - lo).div_ceil(2);
            if ok(mid) {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo
    }

    /// Every PE row's `max_bytes` is the largest ceiling a valid configuration gives on its surface and profile: the
    /// upper bound of its key, for `mcp` capped by that of the profile's `mcp.result-max-bytes.<client>`; a fixed row's
    /// is its `default_bytes` ([RULES/README] §1: the model evaluates or checks every row). Its `default_bytes` is the
    /// ceiling of the default configuration.
    #[test]
    fn every_ceiling_row_matches_the_registry() {
        let c = Conf::default();
        for r in &rules().table("pack-ceilings").rows {
            let surface = match r.tok("surface") {
                "cli" => Surface::Cli,
                "file" => Surface::File,
                "mcp" => Surface::Mcp,
                _ => Surface::Hook,
            };
            let client = match r.tok("client") {
                "*" => "claude",
                x => x,
            };
            let (max, default) = match r.tok("key") {
                "-" => {
                    assert_eq!(r.tok("max_bytes"), r.tok("default_bytes"), "{}", r.id);
                    (
                        r.tok("max_bytes").parse().ok(),
                        r.tok("default_bytes").parse().ok(),
                    )
                }
                k => {
                    let mut m = instance_max(k);
                    if surface == Surface::Mcp {
                        m = m.min(instance_max(&format!("mcp.result-max-bytes.{client}")));
                    }
                    (Some(m), ceiling(&c, surface, client))
                }
            };
            assert_eq!(r.tok("max_bytes").parse().ok(), max, "{}: max_bytes", r.id);
            assert_eq!(
                ceiling(&c, surface, client),
                default,
                "{}: the default ceiling",
                r.id
            );
            if !r.tok("default_bytes").starts_with("HOLE(") && r.tok("key") != "-" {
                assert_eq!(
                    r.tok("default_bytes").parse().ok(),
                    default,
                    "{}: default_bytes",
                    r.id
                );
            }
        }
    }
}
