//! Model identity, model profiles and the write and read rules they select ([90 §8.1] L2, [90 §8.2]; [CFG §4.3],
//! §10.5, §10.9; [RULES/role-write-policy] WR-012, WQ-003 to WQ-005): the family name of a declared model, the profile
//! of a family (`lq.model-profile.<family>`), a session's default model (`lq.model-profile.default.<client>`), the
//! write rule of a profile (`query.safelist.model.<profile>`), and a role's read safelist (`query.safelist.<role>`).

use crate::lq::ctx::Profile;
use crate::registry::{Conf, Proc};

/// The family name of a model identifier ([CFG §4.3]): ASCII lower-cased, every byte outside `a`–`z`, `0`–`9`, `-`
/// and `_` replaced by `-`, cut to its first 64 bytes. `claude-opus-5-5` stays; `GPT-5.6-Luna` becomes
/// `gpt-5-6-luna`.
// spec: [CFG §4.3]
pub fn family(model: &str) -> String {
    model
        .bytes()
        .map(|b| {
            let c = b.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_' {
                c as char
            } else {
                '-'
            }
        })
        .take(64)
        .collect()
}

/// The profile of a session ([90 §8.2]; [CFG §10.9]): the declared model's family, else the client's default family
/// (`lq.model-profile.default.<client>`), then `lq.model-profile.<family>`. A family the registry cannot name (empty,
/// a reserved word, not a word) and the default `unknown` give the `unknown` profile.
// spec: [90 §8.2]
// spec: [CFG §10.9] lq.model-profile.<family>, lq.model-profile.default.<client>
pub fn model_profile(conf: &Conf, model: Option<&str>, client: &str) -> Profile {
    let fam = match model {
        Some(m) => family(m),
        None => conf.text(&format!("lq.model-profile.default.{client}")),
    };
    if fam == "unknown" || crate::registry::find(&format!("lq.model-profile.{fam}")).is_none() {
        return Profile::Unknown;
    }
    match conf.text(&format!("lq.model-profile.{fam}")).as_str() {
        "gated" => Profile::Gated,
        "compatible" => Profile::Compatible,
        _ => Profile::Unknown,
    }
}

/// The write rule of a profile ([CFG §10.9] `query.safelist.model.<profile>`; [90 §8.1] L2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteRule {
    /// `off`: free-form `TX` allowed.
    Off,
    /// `named-only`: free-form `TX` refused with E411.
    NamedOnly,
    /// `dry-targets`: as `named-only`, but a `DRY` or a `TX` applied with `IF TARGETS` from a `DRY` is allowed.
    DryTargets,
}

/// `query.safelist.model.<profile>` of a profile.
// spec: [CFG §10.9] query.safelist.model.<profile>
// spec: [RULES/role-write-policy] WR-012
pub fn model_write_rule(conf: &Conf, p: Profile) -> WriteRule {
    let name = match p {
        Profile::Gated => "gated",
        Profile::Compatible => "compatible",
        Profile::Unknown => "unknown",
    };
    match conf.text(&format!("query.safelist.model.{name}")).as_str() {
        "named-only" => WriteRule::NamedOnly,
        "dry-targets" => WriteRule::DryTargets,
        _ => WriteRule::Off,
    }
}

/// The profile and the dry-targets flag the LQ binder is given for a free-form `TX` ([LQ/errors §5.5] E411): the
/// binder refuses a free-form `TX` exactly for its `unknown` profile, so a rule other than `off` is passed as
/// `unknown` (with `dry-targets` as its flag), and `off` passes the session's own profile, `unknown` read as
/// `compatible`, which differs from it only by E411 ([LQ/envelope §4.1]: the reading echo is `compatible`'s).
pub fn binder_profile(p: Profile, rule: WriteRule) -> (Profile, bool) {
    match (rule, p) {
        (WriteRule::Off, Profile::Gated) => (Profile::Gated, false),
        (WriteRule::Off, _) => (Profile::Compatible, false),
        (WriteRule::NamedOnly, _) => (Profile::Unknown, false),
        (WriteRule::DryTargets, _) => (Profile::Unknown, true),
    }
}

/// `query.safelist.<role>` ([CFG §10.5]; [RULES/role-write-policy] WQ-003): whether the role may run only named queries
/// (`named-only`); a free-form `q` or `query` is then E406.
// spec: [CFG §10.5] query.safelist.<role>
pub fn read_safelist(conf: &Conf, role: &str) -> bool {
    crate::registry::find(&format!("query.safelist.{role}")).is_some()
        && conf.text(&format!("query.safelist.{role}")) == "named-only"
}

/// The process kind of a door: the MCP server for `mcp`, a CLI or hook process otherwise.
pub fn proc_of(mcp: bool) -> Proc {
    if mcp { Proc::Mcp } else { Proc::Cli }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_fold_as_cfg_4_3_says() {
        assert_eq!(family("claude-opus-5-5"), "claude-opus-5-5");
        assert_eq!(family("GPT-5.6-Luna"), "gpt-5-6-luna");
        assert_eq!(family(&"x".repeat(80)).len(), 64);
    }

    #[test]
    fn profiles_and_rules_follow_the_keys() {
        let mut c = Conf::default();
        assert_eq!(model_profile(&c, None, "claude"), Profile::Gated);
        assert_eq!(model_profile(&c, None, "codex"), Profile::Unknown);
        assert_eq!(
            model_profile(&c, Some("GPT-5.6-Luna"), "codex"),
            Profile::Unknown
        );
        c.store
            .insert("lq.model-profile.gpt-5-6-luna".into(), "compatible".into());
        assert_eq!(
            model_profile(&c, Some("GPT-5.6-Luna"), "codex"),
            Profile::Compatible
        );
        assert_eq!(model_write_rule(&c, Profile::Unknown), WriteRule::NamedOnly);
        assert_eq!(model_write_rule(&c, Profile::Gated), WriteRule::Off);
        assert_eq!(
            binder_profile(Profile::Unknown, WriteRule::Off),
            (Profile::Compatible, false)
        );
        assert!(!read_safelist(&c, "developer"));
        c.store
            .insert("query.safelist.developer".into(), "named-only".into());
        assert!(read_safelist(&c, "developer"));
    }
}
