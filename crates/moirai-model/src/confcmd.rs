//! `ConfigSet` and `ConfigUnset` ([API §8.2]; [CFG §7.1]–§7.5) on the model's simulated configuration files, and the
//! configuration snapshot the model takes after every change ([CFG §9.4]: the effective values of the class-V keys,
//! read by the model functions [RULES/policy-keys] binds).
//!
//! Group S: never keyed, nothing appended to the log ([API §7.1]); `config_gen` is engine-internal ([CFG §8.1]).

use crate::api::{Data, Reply, Store};
use crate::config;
use crate::err::{Refusal, Res};
use crate::registry::{self, Conf, Def, Proc, Reload, Scope};

/// The scope a `ConfigSet` or `ConfigUnset` writes ([API §8.2] `scope`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FileScope {
    /// The store file (the default).
    #[default]
    Store,
    /// The user file.
    User,
}

impl FileScope {
    /// The argument's text.
    pub fn token(self) -> &'static str {
        match self {
            FileScope::Store => "store",
            FileScope::User => "user",
        }
    }
}

/// The data of a `ConfigSet` or `ConfigUnset` result ([API §8.2]): `key`, `scope`, the new effective value and the
/// previous one, canonical, `None` where the key has no value (a `none` default).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigData {
    /// `key`, as the command wrote it (case-folded).
    pub key: String,
    /// `scope`.
    pub scope: FileScope,
    /// `value`: the effective value after the command.
    pub value: Option<String>,
    /// `previous`: the effective value before it.
    pub previous: Option<String>,
}

fn key_refusal(key: &str, why: String) -> Refusal {
    Refusal::new("config_key", 2, why).key("key", key)
}

/// The rules of [CFG §7.2] 1, 2 and 4 for a key and a scope: the registry row and the key instance the command
/// resolves (a store-qualified user entry `stores.<store-id>.<key>` names its inner key, [CFG §2.3]).
// spec: [CFG §7.2]
// spec: [CFG §2.3]
fn target(conf: &Conf, key: &str, scope: FileScope) -> Res<(&'static Def, String, String)> {
    let key = key.to_ascii_lowercase();
    let (inner, qualified) = match key.strip_prefix("stores.") {
        Some(rest) => {
            let (id, k) = rest.split_once('.').unwrap_or((rest, ""));
            if id.len() != 32
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(key_refusal(
                    &key,
                    format!("unknown configuration key {key}"),
                ));
            }
            (k.to_string(), true)
        }
        None => (key.clone(), false),
    };
    if let Some((name, succ)) = registry::retired(&inner) {
        return Err(key_refusal(
            &key,
            format!("{name} was removed or renamed: {succ}"),
        ));
    }
    if registry::policy_data(&inner) {
        return Err(key_refusal(
            &key,
            format!("{inner} is policy data, set by a schema write, not by config set"),
        ));
    }
    let (d, _) = registry::find(&inner)
        .ok_or_else(|| key_refusal(&key, format!("unknown configuration key {inner}")))?;
    let user_ok = d.scope.is_user() || d.scope == Scope::StoreUserLower;
    match scope {
        FileScope::Store if d.scope.is_user() => {
            return Err(key_refusal(
                &key,
                format!("{inner} is a user key; use the user scope"),
            ));
        }
        FileScope::User if !user_ok => {
            return Err(key_refusal(
                &key,
                format!("{inner} is a store key; use the store scope"),
            ));
        }
        _ => {}
    }
    if qualified
        && (scope != FileScope::User
            || !(d.scope == Scope::UserQ || d.scope == Scope::StoreUserLower))
    {
        return Err(key_refusal(
            &key,
            format!("{inner} is not a qualifiable user key"),
        ));
    }
    // IP-4: an init-fixed key of an existing store never changes ([F17 §2.2]).
    if d.reload == Reload::Init && d.param.is_none() {
        return Err(Refusal::new(
            "config_value",
            2,
            format!(
                "{inner} is fixed at creation ({}); a different value needs a new store",
                conf.text(&inner)
            ),
        )
        .key("key", inner.as_str()));
    }
    let file_key = if qualified {
        key.clone()
    } else {
        inner.clone()
    };
    Ok((d, inner, file_key))
}

impl Store {
    /// `ConfigSet` ([API §8.2]; [CFG §7.2]): validates the key, the scope and the value, checks the constraints with
    /// the new value in place ([CFG §5.3]), writes the file, and refreshes the configuration snapshot.
    // spec: [API §8.2]
    // spec: [CFG §7.2]
    pub fn config_set(&mut self, key: &str, value: &str, scope: FileScope) -> Res<Reply> {
        let (d, inner, file_key) = target(&self.conf, key, scope)?;
        let p = registry::validate(d, &inner, value).ok_or_else(|| {
            Refusal::new(
                "config_value",
                2,
                format!("{inner} takes {:?}; got {value}", d.ty),
            )
            .key("key", inner.as_str())
        })?;
        let previous = self.conf.effective(&inner, Proc::Cli).map(|x| x.0);
        let mut next = self.conf.clone();
        match scope {
            FileScope::Store => next.store.insert(file_key, p.canonical),
            FileScope::User => next.user.insert(file_key, p.canonical),
        };
        check(&next, &inner)?;
        self.conf = next;
        self.refresh_cfg();
        let value = self.conf.effective(&inner, Proc::Cli).map(|x| x.0);
        Ok(Reply::ok(Data::Config(Box::new(ConfigData {
            key: key.to_ascii_lowercase(),
            scope,
            value,
            previous,
        }))))
    }

    /// `ConfigUnset` ([API §8.2]; [CFG §7.1]): removes every entry of the key from the file; the result's `value` is
    /// the value that applies without it. An unset that would make a constraint fail is refused like a set.
    // spec: [API §8.2]
    pub fn config_unset(&mut self, key: &str, scope: FileScope) -> Res<Reply> {
        let (_, inner, file_key) = target(&self.conf, key, scope)?;
        let previous = self.conf.effective(&inner, Proc::Cli).map(|x| x.0);
        let mut next = self.conf.clone();
        match scope {
            FileScope::Store => next.store.remove(&file_key),
            FileScope::User => next.user.remove(&file_key),
        };
        check(&next, &inner)?;
        self.conf = next;
        self.refresh_cfg();
        let value = self.conf.effective(&inner, Proc::Cli).map(|x| x.0);
        Ok(Reply::ok(Data::Config(Box::new(ConfigData {
            key: key.to_ascii_lowercase(),
            scope,
            value,
            previous,
        }))))
    }

    /// Re-reads the configuration snapshot the kernel and the write path use from the effective values
    /// ([CFG §5.4]: every key the model reads is `hot`; a change applies from the next command on, [API §8.2]).
    pub fn refresh_cfg(&mut self) {
        let c = &self.conf;
        self.cfg.ttl_default_ms = c.number("lease.ttl-default");
        self.cfg.reclaim_older_than_ms = c.number("lease.reclaim-older-than");
        self.cfg.orchestrator_ttl_ms = c.number("lease.orchestrator-ttl");
        self.cfg.suspect_budget =
            u32::try_from(c.number("store.suspect-budget")).expect("P23's range fits u32");
        self.cfg.max_statements = c.number("tx.max-statements");
        self.cfg.max_ops = c.number("tx.max-ops");
        self.cfg.knowledge_strict = c.text("knowledge.owner-authority") == "strict";
        self.cfg.reflog_expire_ms = c.number("gc.reflog-expire");
        let windows = crate::idem::Windows {
            retention_ms: c.number("idempotency.retention"),
            default_ms: c.number("idempotency.default-window"),
        };
        let branch = c.text("default-branch");
        if let Some(i) = self.inited.as_mut() {
            i.windows = windows;
            i.default_branch = branch;
        }
    }
}

/// The constraints of [CFG §5.3] on a candidate configuration; the refusal names the command's key.
fn check(next: &Conf, key: &str) -> Res<()> {
    config::check_constraints(&|k| next.constraint_value(k), &|k| k == key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Cmd, Ctx, Outcome};

    fn store() -> Store {
        let mut s = Store::new();
        let r = s.run(
            &Cmd::Init {
                seed: 1,
                params: vec![],
                default_branch: None,
            },
            &Ctx::default(),
        );
        assert_eq!(r.outcome, Outcome::Ok);
        s
    }

    fn set(s: &mut Store, k: &str, v: &str, scope: FileScope) -> Reply {
        s.run(
            &Cmd::ConfigSet {
                key: k.into(),
                value: v.into(),
                scope,
            },
            &Ctx::default(),
        )
    }

    fn code(r: &Reply) -> Option<&str> {
        r.error.as_ref().map(|e| e.code.as_str())
    }

    #[test]
    fn config_set_validates_and_reports_both_values() {
        let mut s = store();
        let r = set(&mut s, "Lease.TTL-Default", "900s", FileScope::Store);
        assert_eq!(r.outcome, Outcome::Ok);
        match &r.data {
            Data::Config(c) => {
                assert_eq!(c.previous.as_deref(), Some("15m"));
                assert_eq!(c.value.as_deref(), Some("15m"));
            }
            other => panic!("{other:?}"),
        }
        set(&mut s, "lease.ttl-default", "20m", FileScope::Store);
        assert_eq!(s.cfg.ttl_default_ms, 20 * 60_000);
        for (k, v, sc, want) in [
            ("no.such.key", "1", FileScope::Store, "config_key"),
            ("pack.cli.max-chars", "1", FileScope::Store, "config_key"),
            (
                "edges.blocks.on-src-deleted",
                "flag",
                FileScope::Store,
                "config_key",
            ),
            ("roots.docs", "/d", FileScope::Store, "config_key"),
            ("lease.ttl-default", "1ms", FileScope::Store, "config_value"),
            (
                "store.log-extent-bytes",
                "64MiB",
                FileScope::Store,
                "config_value",
            ),
            ("pack.quota.c2", "60", FileScope::Store, "config_value"),
            (
                "mcp.result-max-bytes.codex",
                "40000",
                FileScope::Store,
                "config_value",
            ),
        ] {
            assert_eq!(code(&set(&mut s, k, v, sc)), Some(want), "{k}={v}");
        }
        assert_eq!(
            s.cfg.ttl_default_ms,
            20 * 60_000,
            "a refusal changes nothing"
        );
    }

    #[test]
    fn user_keys_qualify_and_user_lower_only_lowers() {
        let mut s = store();
        let id = s.conf.store_id.clone();
        assert_eq!(
            set(&mut s, "files.cloud", "refuse", FileScope::User).outcome,
            Outcome::Ok
        );
        let q = format!("stores.{id}.files.cloud");
        assert_eq!(
            set(&mut s, &q, "metadata-only", FileScope::User).outcome,
            Outcome::Ok
        );
        assert_eq!(s.conf.text("files.cloud"), "metadata-only");
        assert_eq!(
            code(&set(&mut s, &q, "refuse", FileScope::Store)),
            Some("config_key")
        );
        set(&mut s, "pack.cli.max-bytes", "10000", FileScope::User);
        assert_eq!(s.conf.text("pack.cli.max-bytes"), "10000");
        let r = s.run(
            &Cmd::ConfigUnset {
                key: "pack.cli.max-bytes".into(),
                scope: FileScope::User,
            },
            &Ctx::default(),
        );
        match &r.data {
            Data::Config(c) => assert_eq!(c.value.as_deref(), Some("24000")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn an_unset_that_breaks_a_constraint_is_refused() {
        let mut s = store();
        assert_eq!(
            set(&mut s, "idempotency.default-window", "1s", FileScope::Store).outcome,
            Outcome::Ok
        );
        assert_eq!(
            set(&mut s, "idempotency.retention", "1s", FileScope::Store).outcome,
            Outcome::Ok
        );
        let r = s.run(
            &Cmd::ConfigUnset {
                key: "idempotency.default-window".into(),
                scope: FileScope::Store,
            },
            &Ctx::default(),
        );
        assert_eq!(
            code(&r),
            Some("config_value"),
            "C-3 with the default window"
        );
    }
}
