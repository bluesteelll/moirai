//! The role write policy as data ([RULES/role-write-policy]; [AR §7.3]; [90 §4.3]): where rights come from
//! (`role-rights`), and, per verb, statement and op, the allowlists `role-verbs`, `role-statements`, `role-create`,
//! `role-values`, `role-fields`, `role-status`, `role-edges` and `role-mint`. An op is allowed iff at least one row
//! matches it (§2); with a narrowing hook label it must be allowed for both roles (WR-007).

use crate::err::{Refusal, Res};
use crate::rules::{Row, rules};
use crate::schema::Schema;
use crate::state::State;
use crate::value::{Nid, Value};
use std::collections::{BTreeMap, BTreeSet};

/// The door of a call ([RULES/role-write-policy] `surface`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// The CLI, `moirai tx`, `apply` and hooks.
    Cli,
    /// An MCP tool call.
    Mcp,
}

/// The presented lease as the policy reads it (WT-002).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presented {
    /// The lease id.
    pub id: u64,
    /// A task lease's task.
    pub task: Option<Nid>,
    /// The run the lease is scoped to.
    pub run: Option<Nid>,
    /// The lease's role.
    pub role: String,
    /// The holder.
    pub holder: String,
    /// The orchestrator's session role lease.
    pub session_role: bool,
}

/// The policy data the role policy reads ([CFG §10.13]; [AR §13] "Policy data"): schema rows versioned per branch, the
/// view's `policy` items ([F08 §8.5.6]), a row without an item taking its default ([`PolicyData::of`]). A `role-*` row
/// whose `key` names one of these has its role cell replaced by it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyData {
    /// `policy.self-claim-roles`.
    pub self_claim_roles: Vec<String>,
    /// `policy.mint.role-lease`.
    pub mint_role_lease: Vec<String>,
    /// `policy.role.developer.fields`.
    pub developer_fields: Vec<String>,
    /// The roles whose `policy.role.<role>.mcp-write` is yes.
    pub mcp_write: Vec<String>,
    /// The roles whose `policy.role.<role>.define-query` is yes.
    pub define_query: Vec<String>,
    /// `policy.role.<role>.tx`, the per-statement policy: for each restricted statement class of `role-statements`
    /// whose key it is, the roles that may use it.
    pub tx: BTreeMap<String, Vec<String>>,
    /// The roles whose `policy.role.<role>.authority-owner` is yes: who may write `authority = owner` (with the
    /// attestation and quote WA-001 requires).
    pub authority_owner: Vec<String>,
}

/// The role cell of the `role-statements` rows whose key is `key`, by statement class.
fn statement_roles(key: &str) -> BTreeMap<String, Vec<String>> {
    let mut m: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in rules()
        .table("role-statements")
        .rows
        .iter()
        .filter(|r| r.tok("key") == key)
    {
        m.entry(r.tok("statement").to_string())
            .or_default()
            .extend(r.toks("roles").iter().map(|x| x.to_string()));
    }
    m
}

impl Default for PolicyData {
    /// The defaults of [CFG §10.13]: the role cells of the rows that name each policy-data row as their key.
    fn default() -> PolicyData {
        let mcp = rules()
            .table("role-rows")
            .rows
            .iter()
            .filter(|r| r.tok("mcp_write") == "yes")
            .map(|r| r.tok("role").to_string())
            .collect();
        let define_query = statement_roles("policy.role.<role>.define-query")
            .into_values()
            .next()
            .unwrap_or_default();
        let authority_owner = rules()
            .table("role-values")
            .rows
            .iter()
            .find(|r| r.tok("field") == "authority" && r.toks("value").contains(&"owner"))
            .map(|r| r.toks("roles").iter().map(|x| x.to_string()).collect())
            .unwrap_or_default();
        PolicyData {
            self_claim_roles: vec!["developer".into(), "tester".into()],
            mint_role_lease: vec!["orchestrator".into(), "owner".into()],
            developer_fields: vec!["files_owned".into()],
            mcp_write: mcp,
            define_query,
            tx: statement_roles("policy.role.<role>.tx"),
            authority_owner,
        }
    }
}

/// The type of a policy-data row ([CFG §10.13], §4.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PolicyTy {
    /// A role set or a field list: `words`, sorted bytewise and joined by `,`, the empty value the empty set.
    Words,
    /// `policy.hook-label`: `narrow` only.
    Narrow,
    /// `yes` or `no`.
    YesNo,
    /// `policy.role.<role>.tx`: `per-statement` or `none` (PV-005).
    Tx,
    /// `edges.<kind>.on-src-deleted`: `flag` or `drop-notify`.
    Edge,
    /// `merge.policy.<kind>`: an `auto-policy` value of [RULES/merge-table].
    Auto,
}

/// The row a policy-data instance name belongs to ([CFG §10.13]; [F14 §7.1] `pname` in its canonical, lower-case form),
/// with its parameter segment.
fn policy_row(name: &str) -> Option<(PolicyTy, Option<&str>)> {
    let segs: Vec<&str> = name.split('.').collect();
    let word = |w: &str| crate::config::is_word(w) && w.bytes().all(|b| !b.is_ascii_uppercase());
    Some(match segs.as_slice() {
        ["policy", "self-claim-roles"] | ["policy", "mint", "role-lease"] => {
            (PolicyTy::Words, None)
        }
        ["policy", "role", "developer", "fields"] => (PolicyTy::Words, None),
        ["policy", "hook-label"] => (PolicyTy::Narrow, None),
        [
            "policy",
            "role",
            r,
            "mcp-write" | "define-query" | "authority-owner",
        ] if word(r) => (PolicyTy::YesNo, Some(*r)),
        ["policy", "role", r, "tx"] if word(r) => (PolicyTy::Tx, Some(*r)),
        ["edges", "blocks" | "gates", "on-src-deleted"] => (PolicyTy::Edge, None),
        ["merge", "policy", k] if word(k) => (PolicyTy::Auto, Some(*k)),
        _ => return None,
    })
}

/// A policy row's value in [CFG §4.1]'s canonical form, or `None` when `name` is no row instance of [CFG §10.13] or the
/// value does not parse as the row's type ([F08 §8.5.6]; [API §9.8] `bad_value`).
// spec: [F08 §8.5.6]
// spec: [CFG §10.13]
pub fn canonical_policy(name: &str, value: &str) -> Option<String> {
    let (ty, _) = policy_row(name)?;
    let v = value.trim().to_ascii_lowercase();
    match ty {
        PolicyTy::Words => {
            let mut w: Vec<&str> = v
                .split(',')
                .map(str::trim)
                .filter(|x| !x.is_empty())
                .collect();
            if w.iter().any(|x| !crate::config::is_word(x)) {
                return None;
            }
            let n = w.len();
            w.sort_unstable();
            w.dedup();
            (w.len() == n).then(|| w.join(","))
        }
        PolicyTy::Narrow => (v == "narrow").then_some(v),
        PolicyTy::YesNo => matches!(v.as_str(), "yes" | "no").then_some(v),
        PolicyTy::Tx => matches!(v.as_str(), "per-statement" | "none").then_some(v),
        PolicyTy::Edge => matches!(v.as_str(), "flag" | "drop-notify").then_some(v),
        PolicyTy::Auto => rules()
            .table("auto-policy")
            .rows
            .iter()
            .any(|r| r.tok("value") == v)
            .then_some(v),
    }
}

/// A policy row's default in canonical form ([CFG §10.13]): the value a view without the row's item takes; `None`
/// when `name` is no row instance.
pub fn default_policy(name: &str) -> Option<String> {
    let (ty, param) = policy_row(name)?;
    let d = PolicyData::default();
    let yes = |set: &[String]| {
        if param.is_some_and(|r| set.iter().any(|x| x == r)) {
            "yes"
        } else {
            "no"
        }
    };
    let seg = name.rsplit('.').next().unwrap_or("");
    Some(match ty {
        PolicyTy::Words => match name {
            "policy.self-claim-roles" => d.self_claim_roles.join(","),
            "policy.mint.role-lease" => d.mint_role_lease.join(","),
            _ => d.developer_fields.join(","),
        },
        PolicyTy::Narrow => "narrow".into(),
        PolicyTy::YesNo => match seg {
            "mcp-write" => yes(&d.mcp_write),
            "define-query" => yes(&d.define_query),
            _ => yes(&d.authority_owner),
        }
        .into(),
        PolicyTy::Tx => "per-statement".into(),
        PolicyTy::Edge => "flag".into(),
        PolicyTy::Auto => "none".into(),
    })
}

impl PolicyData {
    /// The policy data of a view ([CFG §10.13]; [RULES/policy-keys] §2): the defaults, with each `policy` item of the
    /// view's schema applied ([F08 §8.5.6]). `per-statement` is `policy.role.<role>.tx`'s default, so only `none`
    /// changes the per-statement rows.
    // spec: [F08 §8.5.6]
    pub fn of(schema: &Schema) -> PolicyData {
        let mut d = PolicyData::default();
        let words = |v: &str| -> Vec<String> {
            v.split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        };
        let toggle = |set: &mut Vec<String>, r: &str, yes: bool| {
            if yes {
                if !set.iter().any(|x| x == r) {
                    set.push(r.to_string());
                }
            } else {
                set.retain(|x| x != r);
            }
        };
        for p in schema.policies() {
            let segs: Vec<&str> = p.name.split('.').collect();
            let yes = p.value.as_deref() == Some("yes");
            let v = p.value.as_deref().unwrap_or("");
            match segs.as_slice() {
                ["policy", "self-claim-roles"] => d.self_claim_roles = words(v),
                ["policy", "mint", "role-lease"] => d.mint_role_lease = words(v),
                ["policy", "role", "developer", "fields"] => d.developer_fields = words(v),
                ["policy", "role", r, "mcp-write"] => toggle(&mut d.mcp_write, r, yes),
                ["policy", "role", r, "define-query"] => toggle(&mut d.define_query, r, yes),
                ["policy", "role", r, "authority-owner"] => toggle(&mut d.authority_owner, r, yes),
                ["policy", "role", r, "tx"] if v == "none" => {
                    for roles in d.tx.values_mut() {
                        roles.retain(|x| x != r);
                    }
                }
                _ => {}
            }
        }
        d
    }
}

/// `merge.policy.<kind>` of a view by kind ([CFG §10.13]; [RULES/merge-table] AP rows), from its `policy` items; a
/// kind without an item is `none` and is not listed.
pub fn merge_policies(schema: &Schema) -> BTreeMap<String, String> {
    schema
        .policies()
        .filter_map(|p| {
            let kind = p.name.strip_prefix("merge.policy.")?;
            Some((kind.to_string(), p.value.clone()?))
        })
        .collect()
}

/// The caller as the policy sees it: the effective role, the narrowing label, the surface, the presented lease and the
/// owner attestation ([RULES/role-write-policy] WT-001 to WT-016).
#[derive(Clone, Debug)]
pub struct Rights {
    /// R: the effective role (WR-006).
    pub role: String,
    /// A hook label naming another `role-rows` role, which narrows (WR-007).
    pub label: Option<String>,
    /// The surface.
    pub surface: Surface,
    /// The presented lease.
    pub lease: Option<Presented>,
    /// The resolved actor.
    pub actor: Option<String>,
    /// WT-012: owner-attested.
    pub owner_attested: bool,
    /// WT-016: the actor recorded with the `agent/*` acceptance that a `links fix --confirm` turns into `confirmed/*`
    /// (an input of the verb that writes `relink`, a command of group F, [API §12]); `None` when there is none.
    pub acceptor: Option<String>,
    /// The policy data.
    pub data: PolicyData,
    /// `files.confirm-roles` ([CFG §10.6]): the role cell of the rows keyed by it (WV-038, WV-039).
    pub confirm_roles: Vec<String>,
}

/// WR-006: the effective role of a call. No lease: `general-purpose`. A task lease: its role. A run-scoped role lease:
/// its role. The session role lease: `orchestrator`, or `owner` when owner-attested. A role with no `role-rows` row:
/// `general-purpose`.
// spec: [RULES/role-write-policy] role-rights
// rule: WR-001, WR-006
pub fn effective_role(lease: Option<&Presented>, owner_attested: bool) -> String {
    let Some(l) = lease else {
        return "general-purpose".into();
    };
    let role = if l.session_role {
        if owner_attested {
            "owner"
        } else {
            "orchestrator"
        }
    } else {
        l.role.as_str()
    };
    if rules()
        .table("role-rows")
        .rows
        .iter()
        .any(|r| r.tok("role") == role)
    {
        role.to_string()
    } else {
        "general-purpose".into()
    }
}

/// WR-007: the narrowing label, when it names a `role-rows` role other than `general-purpose` that differs from R; an
/// unleased call ignores labels, and a harness agent type (`general-purpose`, `Explore`, `Plan`) is ignored.
// rule: WR-007
pub fn narrowing_label(role: &str, leased: bool, label: Option<&str>) -> Option<String> {
    let l = label?;
    if !leased || l == role || l == "general-purpose" {
        return None;
    }
    rules()
        .table("role-rows")
        .rows
        .iter()
        .any(|r| r.tok("role") == l)
        .then(|| l.to_string())
}

/// Whether a role cell (`*`, `leased`, `holder`, `-` or a list of roles) admits `role`.
fn admits(cell: &[&str], role: &str, holder: bool) -> bool {
    cell.iter().any(|c| match *c {
        "*" => true,
        "leased" => role != "general-purpose",
        "holder" => holder,
        "-" => false,
        r => r == role,
    })
}

/// The scopes of WT-004 to WT-008 for one node.
pub struct Scopes<'a> {
    /// The view.
    pub st: &'a State,
    /// Nodes an earlier statement of the block created (WT-006).
    pub created: &'a BTreeSet<Nid>,
}

impl Scopes<'_> {
    /// Whether node `n` is in scope `scope` for role `r` under `rights`.
    fn holds(&self, scope: &str, n: Nid, r: &str, rights: &Rights) -> bool {
        match scope {
            "any" => true,
            "own-role" => self.st.nodes.get(&n).is_some_and(|x| x.creator.role == r),
            "leased-task" => rights.lease.as_ref().is_some_and(|l| l.task == Some(n)),
            "created-in-tx" => self.created.contains(&n),
            "lease-run" => rights.lease.as_ref().is_some_and(|l| l.run == Some(n)),
            other => panic!("scope {other} is not a role-terms scope"),
        }
    }
}

/// E406 naming the statement, the table and the (role, op, kind, field) no row allowed ([RULES/role-write-policy]
/// WZ-001; its OP-23).
// rule: WZ-001
// rule: WR-010
pub fn refuse(stmt: usize, table: &str, role: &str, what: &str) -> Refusal {
    let mut detail = format!("statement {stmt}: no {table} row lets {role} {what}");
    if role == "general-purpose" {
        detail.push_str("; this write needs a lease");
    }
    Refusal::lq("E406", detail)
}

impl Rights {
    fn each_role(&self) -> Vec<&str> {
        let mut v = vec![self.role.as_str()];
        if let Some(l) = &self.label {
            v.push(l);
        }
        v
    }

    /// `role-verbs`: the verb's row admits the surface and the role (WR-008; WV rows).
    // spec: [RULES/role-write-policy] role-verbs
    // rule: WR-008
    pub fn verb(&self, verb: &str) -> Res<()> {
        let r = rules()
            .table("role-verbs")
            .rows
            .iter()
            .find(|r| r.tok("verb") == verb)
            .unwrap_or_else(|| panic!("role-verbs has no row {verb}"));
        let surface_ok = matches!(
            (r.tok("surface"), self.surface),
            ("both", _) | ("cli", Surface::Cli) | ("mcp", Surface::Mcp)
        );
        if !surface_ok {
            return Err(refuse(0, "role-verbs", &self.role, verb));
        }
        let holder = self
            .lease
            .as_ref()
            .is_some_and(|l| Some(&l.holder) == self.actor.as_ref());
        // `policy.role.<role>.mcp-write` and `files.confirm-roles` replace the row's role cell ([CFG §10.13],
        // §10.6).
        let cell: Vec<String> = match r.tok("key") {
            "policy.role.<role>.mcp-write" => self.data.mcp_write.clone(),
            "files.confirm-roles" => self.confirm_roles.clone(),
            _ => r.toks("roles").iter().map(|x| x.to_string()).collect(),
        };
        let cell: Vec<&str> = cell.iter().map(String::as_str).collect();
        for role in self.each_role() {
            if !admits(&cell, role, holder) {
                let code = if r.tok("refusal") == "-" {
                    "E406"
                } else {
                    r.tok("refusal")
                };
                return Err(Refusal::lq(
                    code,
                    format!("{}: {role} may not use {verb}", r.id),
                ));
            }
        }
        Ok(())
    }

    /// `role-statements`: the statement class admits the role on this surface (WX rows).
    // spec: [RULES/role-write-policy] role-statements
    // rule: WR-008
    pub fn statement(&self, stmt: usize, class: &str) -> Res<()> {
        let rows: Vec<&Row> = rules()
            .table("role-statements")
            .rows
            .iter()
            .filter(|r| r.tok("statement") == class)
            .collect();
        let holder = self
            .lease
            .as_ref()
            .is_some_and(|l| Some(&l.holder) == self.actor.as_ref());
        for role in self.each_role() {
            let ok = rows.iter().any(|r| {
                let surface_ok = matches!(
                    (r.tok("surface"), self.surface),
                    ("both", _) | ("cli", Surface::Cli) | ("mcp", Surface::Mcp)
                );
                // `policy.role.<role>.tx` and `.define-query` replace the row's role cell ([CFG §10.13]).
                let cell: Vec<String> = match r.tok("key") {
                    "policy.role.<role>.tx" => self.data.tx.get(class).cloned().unwrap_or_default(),
                    "policy.role.<role>.define-query" => self.data.define_query.clone(),
                    _ => r.toks("roles").iter().map(|x| x.to_string()).collect(),
                };
                let cell: Vec<&str> = cell.iter().map(String::as_str).collect();
                surface_ok && admits(&cell, role, holder)
            });
            if !ok {
                return Err(refuse(stmt, "role-statements", role, class));
            }
        }
        Ok(())
    }

    /// `role-create`: some row of R's lets it create `kind` with the node's final values: constraints hold and the
    /// required fields are present (WC rows; WR-009). `edges_from` lists the kinds of edges the block created from it.
    // spec: [RULES/role-write-policy] role-create
    pub fn create(&self, stmt: usize, st: &State, n: Nid, edges_from: &[String]) -> Res<()> {
        let node = st.nodes.get(&n).expect("created node");
        for role in self.each_role() {
            let ok = rules().table("role-create").rows.iter().any(|r| {
                (r.tok("role") == role)
                    && (r.tok("kind") == "*" || r.tok("kind") == node.kind)
                    && r.toks("constraint")
                        .iter()
                        .all(|c| constraint_holds(c, node, role, &st.schema))
                    && r.toks("required")
                        .iter()
                        .all(|q| required_holds(q, node, edges_from))
            });
            if !ok {
                return Err(refuse(
                    stmt,
                    "role-create",
                    role,
                    &format!("create {}", node.kind),
                ));
            }
        }
        Ok(())
    }

    /// `role-values`: a value of a field some row lists is written only by a role the row admits, with its
    /// requirements (WA rows). Fields no row lists are unrestricted here.
    // spec: [RULES/role-write-policy] role-values
    pub fn value(&self, stmt: usize, st: &State, n: Nid, field: &str, v: &Value) -> Res<()> {
        let rows: Vec<&Row> = rules()
            .table("role-values")
            .rows
            .iter()
            .filter(|r| r.tok("field") == field)
            .collect();
        if rows.is_empty() {
            return Ok(());
        }
        let text = match v {
            Value::Enum(s) | Value::Text(s) => s.clone(),
            _ => return Ok(()),
        };
        let node = st.nodes.get(&n);
        for role in self.each_role() {
            let ok = rows.iter().any(|r| {
                let matches_value = r.toks("value").iter().any(|x| {
                    x.strip_suffix("/*")
                        .map_or(*x == text, |p| text.starts_with(&format!("{p}/")))
                });
                // `policy.role.<role>.authority-owner` replaces the role cell of `authority = owner`, and
                // `files.confirm-roles` the cell of `relink = confirmed/*` (WA-004 cites it; WV-038).
                let cell: Vec<String> =
                    if field == "authority" && r.toks("value").contains(&"owner") {
                        self.data.authority_owner.clone()
                    } else if field == "relink" && r.toks("value").contains(&"confirmed/*") {
                        self.confirm_roles.clone()
                    } else {
                        r.toks("roles").iter().map(|x| x.to_string()).collect()
                    };
                let cell: Vec<&str> = cell.iter().map(String::as_str).collect();
                matches_value
                    && admits(&cell, role, false)
                    && r.toks("requires").iter().all(|q| match *q {
                        "-" => true,
                        "owner-attested" => self.owner_attested,
                        // A kind without an `owner_quote` field (a note answering for the owner) cannot carry one;
                        // the requirement binds the kinds that have it (rule, decision).
                        "owner_quote" => node.is_some_and(|x| {
                            x.fields.contains_key("owner_quote")
                                || st.schema.field(&x.kind, "owner_quote").is_none()
                        }),
                        // WT-016: the caller is not the actor of the acceptance being confirmed.
                        "not-acceptor" => self.acceptor.is_none() || self.acceptor != self.actor,
                        other => panic!("role-values requirement {other} has no implementation"),
                    })
            });
            if !ok {
                return Err(refuse(
                    stmt,
                    "role-values",
                    role,
                    &format!("{field} = {text}"),
                ));
            }
        }
        Ok(())
    }

    /// `role-fields`: some row lets R set `field` of node `n` (WF rows; `*` is the writable set W of WT-009; WF-007's
    /// list is `policy.role.developer.fields`).
    // spec: [RULES/role-write-policy] role-fields
    pub fn field(&self, stmt: usize, sc: &Scopes<'_>, n: Nid, field: &str) -> Res<()> {
        let kind = sc
            .st
            .nodes
            .get(&n)
            .map(|x| x.kind.clone())
            .unwrap_or_default();
        for role in self.each_role() {
            let ok = rules().table("role-fields").rows.iter().any(|r| {
                (r.tok("role") == "*" || r.tok("role") == role)
                    && (r.tok("kind") == "*" || r.tok("kind") == kind)
                    && sc.holds(r.tok("scope"), n, role, self)
                    && {
                        let fields: Vec<String> = if r.tok("key") == "policy.role.developer.fields"
                        {
                            self.data.developer_fields.clone()
                        } else {
                            r.toks("fields").iter().map(|s| s.to_string()).collect()
                        };
                        fields.iter().any(|f| {
                            f == field || (f == "*" && writable(&sc.st.schema, &kind, field))
                        })
                    }
            });
            if !ok {
                return Err(refuse(
                    stmt,
                    "role-fields",
                    role,
                    &format!("set {kind}.{field}"),
                ));
            }
        }
        Ok(())
    }

    /// `role-status`: some row lets R move `n` from `from` to `to` through the door (WS rows; GR-002).
    // spec: [RULES/role-write-policy] role-status
    // rule: GR-002
    pub fn status(
        &self,
        stmt: usize,
        sc: &Scopes<'_>,
        n: Nid,
        from: &str,
        to: &str,
        door: &str,
    ) -> Res<()> {
        let kind = sc
            .st
            .nodes
            .get(&n)
            .map(|x| x.kind.clone())
            .unwrap_or_default();
        let via = match door {
            "set-status" => "set",
            "lease-first-write" => "claim-start",
            d => d,
        };
        for role in self.each_role() {
            let ok = rules().table("role-status").rows.iter().any(|r| {
                r.tok("role") == role
                    && (r.tok("kind") == "*" || r.tok("kind") == kind)
                    && sc.holds(r.tok("scope"), n, role, self)
                    && (r.toks("from").contains(&"*") || r.toks("from").contains(&from))
                    && (r.tok("to") == "*" || r.tok("to") == to)
                    && (r.tok("via") == "any" || r.tok("via") == via)
            });
            if !ok {
                return Err(refuse(
                    stmt,
                    "role-status",
                    role,
                    &format!("{kind} {from} -> {to} via {door}"),
                ));
            }
        }
        Ok(())
    }

    /// `role-edges`: some row lets R create or delete an edge of `kind` from `src` to `dst` (WE rows). `mentions` is
    /// never written by a statement (WE-016).
    // spec: [RULES/role-write-policy] role-edges
    pub fn edge(
        &self,
        stmt: usize,
        sc: &Scopes<'_>,
        kind: &str,
        op: &str,
        src: Nid,
        dst: Nid,
    ) -> Res<()> {
        for role in self.each_role() {
            let ok = rules().table("role-edges").rows.iter().any(|r| {
                (r.tok("role") == "*" || r.tok("role") == role)
                    && (r.tok("edge") == "*" || r.tok("edge") == kind)
                    && r.toks("ops").contains(&op)
                    && sc.holds(r.tok("src_scope"), src, role, self)
                    && sc.holds(r.tok("dst_scope"), dst, role, self)
            });
            if !ok {
                return Err(refuse(
                    stmt,
                    "role-edges",
                    role,
                    &format!("{op} {kind} {src} -> {dst}"),
                ));
            }
        }
        Ok(())
    }

    /// WT-010 `may-write(n)`: R is the orchestrator or the owner; or a `role-fields` or `role-status` row matches
    /// (R, kind(n), n); or n is own-role and a `role-create` row lets R create kind(n). It decides `link --at` and
    /// `unlink --at` (WV-034, WV-035).
    // spec: [RULES/role-write-policy] role-terms WT-010
    pub fn may_write(&self, sc: &Scopes<'_>, n: Nid) -> bool {
        let Some(x) = sc.st.nodes.get(&n) else {
            return false;
        };
        self.each_role().into_iter().all(|role| {
            matches!(role, "orchestrator" | "owner")
                || rules().table("role-fields").rows.iter().any(|r| {
                    (r.tok("role") == "*" || r.tok("role") == role)
                        && (r.tok("kind") == "*" || r.tok("kind") == x.kind)
                        && sc.holds(r.tok("scope"), n, role, self)
                })
                || rules().table("role-status").rows.iter().any(|r| {
                    r.tok("role") == role
                        && (r.tok("kind") == "*" || r.tok("kind") == x.kind)
                        && sc.holds(r.tok("scope"), n, role, self)
                })
                || (x.creator.role == role
                    && rules().table("role-create").rows.iter().any(|r| {
                        r.tok("role") == role && (r.tok("kind") == "*" || r.tok("kind") == x.kind)
                    }))
        })
    }

    /// `role-mint`: the row of a lease form admits the caller; its key widens or replaces the row's role cell
    /// (WM rows; WR-002 to WR-006 are the caller context's).
    // spec: [RULES/role-write-policy] role-mint
    pub fn mint(&self, form: &str) -> Res<()> {
        let r = rules()
            .table("role-mint")
            .rows
            .iter()
            .find(|r| r.tok("form") == form)
            .unwrap_or_else(|| panic!("role-mint has no form {form}"));
        let holder = self
            .lease
            .as_ref()
            .is_some_and(|l| Some(&l.holder) == self.actor.as_ref());
        let allowed: Vec<String> = match r.tok("key") {
            "policy.mint.role-lease" => self.data.mint_role_lease.clone(),
            _ => r.toks("allowed").iter().map(|s| s.to_string()).collect(),
        };
        let cell: Vec<&str> = allowed.iter().map(String::as_str).collect();
        for role in self.each_role() {
            if !admits(&cell, role, holder) {
                return Err(Refusal::lq(
                    r.tok("refusal"),
                    format!("{}: {role} may not mint a {form} lease", r.id),
                ));
            }
        }
        Ok(())
    }
}

/// I33′: on `plan/*`, `status`, `resolution`, `assignee` and claims are read-only; `blocks`, `parent` and `gates`
/// stay writable ([RULES/status-machines] BM-002).
// spec: [F13 §3.4] I33′
pub fn i33p_plan_mask(view: &str, field: &str) -> crate::err::Res<()> {
    if view == "plan" && matches!(field, "status" | "resolution" | "assignee" | "claims") {
        return Err(crate::err::Refusal::lq(
            "E305",
            format!("{field} is read-only on a plan branch (I33′)"),
        ));
    }
    Ok(())
}

/// WT-009: the writable fields of a kind — the title, abstract, body, parent, order, priority, criticality,
/// confidence, labels, the flags, `defer_until`, `due`, `reason` and every kind field of a writable class; not
/// `status`, `resolution`, `authority`, identity, observation or derived fields. A counter is in W: `incr` moves it
/// under this check, and its assignment is E103 before any policy check ([API §9.1]).
// rule: WT-009
pub fn writable(schema: &Schema, kind: &str, field: &str) -> bool {
    match field {
        "title" | "abstract" | "body" | "parent" | "order" | "priority" | "criticality"
        | "confidence" | "labels" | "pinned" | "archived" | "frozen" | "defer_until" | "due"
        | "reason" => true,
        "status" | "resolution" | "authority" => false,
        _ => schema.field(kind, field).is_some_and(|f| {
            !matches!(
                f.class,
                "identity" | "observation" | "derived" | "none" | "alias-set" | "pathmove-set"
            ) && f.kind.is_some()
        }),
    }
}

fn constraint_holds(c: &str, node: &crate::state::Node, role: &str, schema: &Schema) -> bool {
    if c == "-" {
        return true;
    }
    let (f, vals) = c
        .split_once('=')
        .unwrap_or_else(|| panic!("constraint {c} has no '='"));
    let v = node.field(schema, f);
    let text = v.as_ref().and_then(Value::as_str).unwrap_or("");
    vals.split(',')
        .any(|x| if x == "self" { text == role } else { text == x })
}

fn required_holds(q: &str, node: &crate::state::Node, edges_from: &[String]) -> bool {
    match q {
        "-" => true,
        "refutes-or-confirms-edge" => edges_from.iter().any(|k| k == "refutes" || k == "confirms"),
        "env" => node.fields.keys().any(|k| k.starts_with("env_")),
        f => node.fields.contains_key(f),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{Creator, Node};
    use crate::value::Uid;

    fn rights(role: &str, lease: Option<Presented>) -> Rights {
        Rights {
            role: role.into(),
            label: None,
            surface: Surface::Cli,
            lease,
            actor: Some("a".into()),
            owner_attested: false,
            acceptor: None,
            data: PolicyData::default(),
            confirm_roles: vec!["orchestrator".into(), "owner".into()],
        }
    }

    #[test]
    fn rights_come_only_from_the_lease() {
        assert_eq!(effective_role(None, true), "general-purpose");
        let l = Presented {
            id: 1,
            task: None,
            run: None,
            role: "orchestrator".into(),
            holder: "a".into(),
            session_role: true,
        };
        assert_eq!(effective_role(Some(&l), false), "orchestrator");
        assert_eq!(effective_role(Some(&l), true), "owner");
        let x = Presented {
            role: "wizard".into(),
            session_role: false,
            ..l
        };
        assert_eq!(
            effective_role(Some(&x), false),
            "general-purpose",
            "WR-006: no row, fail closed"
        );
        assert_eq!(narrowing_label("developer", true, Some("Explore")), None);
        assert_eq!(
            narrowing_label("developer", true, Some("tester")),
            Some("tester".into())
        );
        assert_eq!(narrowing_label("developer", false, Some("tester")), None);
    }

    #[test]
    fn allowlists_decide_ops() {
        let s = Schema::default();
        let mut st = State::default();
        let mut x = Node::new(
            Uid([1; 16]),
            "task",
            &s,
            Creator {
                actor: "o".into(),
                role: "orchestrator".into(),
            },
        );
        x.set_field(&s, "title", Some(Value::Text("t".into())));
        st.nodes.insert(Nid(1), x);
        let created = BTreeSet::new();
        let sc = Scopes {
            st: &st,
            created: &created,
        };
        let dev = rights(
            "developer",
            Some(Presented {
                id: 3,
                task: Some(Nid(1)),
                run: None,
                role: "developer".into(),
                holder: "a".into(),
                session_role: false,
            }),
        );
        assert!(dev.field(1, &sc, Nid(1), "files_owned").is_ok(), "WF-007");
        assert_eq!(dev.field(1, &sc, Nid(1), "title").unwrap_err().code, "E406");
        assert!(
            dev.status(1, &sc, Nid(1), "open", "done", "tx-complete")
                .is_ok(),
            "WS-004"
        );
        assert!(
            dev.status(1, &sc, Nid(1), "open", "done", "set-status")
                .is_err()
        );
        let gp = rights("general-purpose", None);
        assert!(gp.statement(1, "node-delete").is_err(), "WX-001");
        assert!(gp.verb("write-verbs").is_ok());
        assert!(gp.verb("branch").is_err());
        let orch = rights("orchestrator", None);
        assert!(orch.field(1, &sc, Nid(1), "title").is_ok());
        assert!(
            orch.edge(1, &sc, "blocks", "create", Nid(1), Nid(1))
                .is_ok()
        );
        assert!(
            orch.value(1, &st, Nid(1), "authority", &Value::Enum("owner".into()))
                .is_err(),
            "WA-001 needs the attestation"
        );
        assert!(orch.mint("session-role-lease").is_ok());
        assert!(gp.mint("run-role-lease").is_err());
        // WA-004: `relink = confirmed/*` by the orchestrator, never by the acceptor (WT-016).
        let confirm = Value::Text("confirmed/exact/100".into());
        let mut o = rights("orchestrator", None);
        o.acceptor = Some("b".into());
        assert!(o.value(1, &st, Nid(1), "relink", &confirm).is_ok());
        o.acceptor = Some("a".into());
        assert_eq!(
            o.value(1, &st, Nid(1), "relink", &confirm)
                .unwrap_err()
                .code,
            "E406"
        );
    }
}
