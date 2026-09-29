//! The budget arithmetic the model computes ([CFG §10.5]; [60 §4.2] "budgets not modelled", GT9 checks the cuts;
//! [CFG] open point 12): the effective value of each query budget from its default, a per-call raise and the caller's
//! role ceiling, and the deterministic caps of a `TX` block (`tx.max-statements`, `tx.max-ops`, [50 §3.10] item 10).

use crate::err::{Refusal, Res};
use crate::registry::{Conf, Proc};

/// The ten budgets of [CFG §10.5].
pub const BUDGETS: [&str; 10] = [
    "work",
    "mem",
    "wmem",
    "rows",
    "bytes",
    "visited",
    "refs",
    "fs",
    "deadline-cli",
    "deadline-mcp",
];

/// One budget's effective value ([CFG §10.5]): the per-call `request` (else `query.budget.default.<b>`) clamped to the
/// caller's ceiling `query.caps.<role>.<b>` (an unleased caller has the role `general-purpose`); `mem` is then
/// min(value, headroom), never below 256 KiB; `wmem` is max(256 KiB, min(value, headroom)). An unknown headroom
/// (`MeterError`) leaves the requested value ([CFG §10.4]). Returns the value and whether the request was clamped
/// (a raise above the ceiling is clamped, [CFG] open point 13).
// spec: [CFG §10.5]
pub fn effective(
    conf: &Conf,
    b: &str,
    role: &str,
    proc: Proc,
    request: Option<u64>,
    headroom: Option<u64>,
) -> (u64, bool) {
    let num = |k: &str| {
        conf.parsed(k, proc)
            .and_then(|p| p.number)
            .unwrap_or_else(|| panic!("{k} has no number"))
    };
    let dflt = num(&format!("query.budget.default.{b}"));
    let cap = num(&format!("query.caps.{role}.{b}"));
    let want = request.unwrap_or(dflt);
    let clamped = want > cap;
    let v = want.min(cap);
    let floor = 256 * 1024;
    let v = match (b, headroom) {
        ("mem", Some(h)) => v.min(h).max(floor),
        ("wmem", Some(h)) => floor.max(v.min(h)),
        _ => v,
    };
    (v, clamped)
}

/// The deterministic caps of a `TX` block ([CFG §10.5] `tx.max-statements`, `tx.max-ops`; [50 §3.10] item 10): a
/// block of more statements, or whose net changeset has more ops, is refused with E501 naming the split. The model
/// counts one op per changed key of the net changeset ([AR §4.6] "Net changeset = state diff").
// spec: [CFG §10.5] tx.max-statements, tx.max-ops
pub fn check_caps(max_statements: u64, max_ops: u64, statements: u64, ops: u64) -> Res<()> {
    if statements > max_statements {
        return Err(Refusal::lq(
            "E501",
            format!(
                "the block has {statements} statements (tx.max-statements {max_statements}); split the block"
            ),
        ));
    }
    if ops > max_ops {
        return Err(Refusal::lq(
            "E501",
            format!("the block writes {ops} ops (tx.max-ops {max_ops}); split the block"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budgets_take_the_request_up_to_the_role_ceiling() {
        let c = Conf::default();
        assert_eq!(
            effective(&c, "rows", "developer", Proc::Cli, None, None),
            (50, false)
        );
        assert_eq!(
            effective(&c, "rows", "developer", Proc::Cli, Some(900), None),
            (500, true)
        );
        assert_eq!(
            effective(&c, "rows", "orchestrator", Proc::Cli, Some(900), None),
            (900, false)
        );
        assert_eq!(
            effective(&c, "mem", "developer", Proc::Mcp, Some(8 << 20), None).0,
            4 << 20,
            "agent-max-mem in the MCP server"
        );
        assert_eq!(
            effective(&c, "mem", "developer", Proc::Cli, None, Some(1000)).0,
            256 * 1024,
            "never below 256 KiB"
        );
        assert!(check_caps(2, 5, 3, 1).is_err() && check_caps(2, 5, 2, 6).is_err());
        assert!(check_caps(2, 5, 2, 5).is_ok());
    }
}
