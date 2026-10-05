//! Client heads and bindings (`HEADS`, [F11 §5]; [AR §5a.1], [AR §5a.4]; [F18 §3]): the rows `Checkout`,
//! `WorktreeBind` and `WorktreeUnbind` write ([API §11.3], §11.4), and the lookups the caller context's branch order
//! reads ([API §4.2] CX-2: the binding of a path by its longest bound prefix, a client head, a session head).
//!
//! A binding designates a tree only when its directory is a simulated tree's root ([F18 §3.3], §3.5; [API §6.5]); a
//! binding of any other directory is `binding only`, with the warning `not_a_tree`. Directory keys are canonical
//! absolute paths ([API §4.1]).

use crate::api::{Ctx, Data, Door, Reply, Store};
use crate::dag::{self, RefKind};
use crate::err::{Refusal, Res};
use crate::idem::{Cj, Recorded};
use std::collections::BTreeMap;

/// The kind of a `HEADS` row ([F11 §5] `kind`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HeadKind {
    /// 1: a directory (a binding row).
    Directory,
    /// 2: a client name.
    Client,
    /// 3: a session (`session:<harness>:<id>`).
    Session,
}

impl HeadKind {
    /// The result's name.
    pub fn name(self) -> &'static str {
        match self {
            HeadKind::Directory => "directory",
            HeadKind::Client => "client",
            HeadKind::Session => "session",
        }
    }
}

/// What a head names: a ref, or a detached commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A ref by name.
    Ref(String),
    /// A detached commit.
    Detached(u64),
}

/// One `HEADS` row ([F11 §5]; [API §15.7] `heads`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    /// `kind`.
    pub kind: HeadKind,
    /// `key`: the key's text (§5.1).
    pub key: String,
    /// The ref or the detached commit.
    pub target: Target,
    /// `designated` ([F18 §3.2]).
    pub designated: bool,
    /// `expected_ref`, in the short form of [F18 §3.2] rule 1.
    pub expected_ref: Option<String>,
    /// `base`: a git commit id as the API writes it (`sha1:<hex>`, [API §5.2]).
    pub base: Option<String>,
    /// The HLC of the `ClientHead` record that wrote the row.
    pub hlc: u64,
}

/// The `HEADS` table, by (kind, key text).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Heads {
    /// The rows.
    pub rows: BTreeMap<(HeadKind, String), Head>,
}

/// Whether `dir` is `path` or a component-wise prefix of it.
fn is_prefix(dir: &str, path: &str) -> bool {
    let d = dir.trim_end_matches('/');
    path == d
        || (path.starts_with(d) && path.as_bytes().get(d.len()) == Some(&b'/'))
        || d.is_empty()
}

impl Heads {
    /// The binding of a path: the directory row whose key is the longest component-wise prefix of it ([F11 §5];
    /// [API §4.2] CX-2 "longest bound prefix").
    // spec: [API §4.2] CX-2
    pub fn binding_of(&self, path: &str) -> Option<&Head> {
        self.rows
            .values()
            .filter(|h| h.kind == HeadKind::Directory && is_prefix(&h.key, path))
            .max_by_key(|h| h.key.len())
    }

    /// The row of a client name or a session key.
    pub fn get(&self, kind: HeadKind, key: &str) -> Option<&Head> {
        self.rows.get(&(kind, key.to_string()))
    }

    /// The rows a snapshot lists: every row, with `session` rows only within `idempotency.retention` of their HLC
    /// ([F11 §5.2] "Retention", read at the snapshot rather than at the fold, so the snapshot does not depend on when
    /// a fold ran).
    pub fn listed(&self, retention_ms: u64, hlc: &crate::clock::Hlc, wall_ms: i64) -> Vec<&Head> {
        self.rows
            .values()
            .filter(|h| h.kind != HeadKind::Session || hlc.within(wall_ms, h.hlc, retention_ms))
            .collect()
    }
}

/// The data of `Checkout` ([API §11.3]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutData {
    /// `key_kind`.
    pub key_kind: HeadKind,
    /// `key`.
    pub key: String,
    /// `ref`.
    pub ref_: Option<String>,
    /// `commit`.
    pub commit: Option<u64>,
    /// `designation_cleared`.
    pub designation_cleared: bool,
}

/// The data of `WorktreeBind` and `WorktreeUnbind` ([API §11.4]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindData {
    /// `dir`.
    pub dir: String,
    /// `ref`: the bound ref; `None` for an unbind.
    pub ref_: Option<String>,
    /// `designated`.
    pub designated: bool,
    /// `expected_ref`.
    pub expected_ref: Option<String>,
    /// `base`.
    pub base: Option<String>,
    /// `replaced`: the rows `replace` rewrote, (dir, ref).
    pub replaced: Vec<(String, String)>,
    /// For an unbind, `removed`.
    pub removed: bool,
}

/// A revision a head can name at M0: a ref name, or a commit by its seq `s<seq>` (commit ids are WP-91's).
fn revision(st: &Store, target: &str) -> Res<Target> {
    if let Some(n) = target.strip_prefix('s').and_then(|x| x.parse::<u64>().ok())
        && st.dag.commits.contains_key(&n)
    {
        return Ok(Target::Detached(n));
    }
    let r = st
        .dag
        .live(target)
        .ok_or_else(|| Refusal::lq("E301", format!("{target} names nothing")))?;
    Ok(Target::Ref(r.name.clone()))
}

impl Store {
    /// The key a `Checkout` writes ([API §11.3]): `ctx.client` or `MOIRAI_CLIENT` (kind `client`); else, through MCP,
    /// the session key `session:<harness>:<id>`; else the directory `ctx.cwd`.
    fn checkout_key(&self, ctx: &Ctx, session: Option<&str>) -> Res<(HeadKind, String)> {
        if let Some(c) = ctx.client.clone().or_else(|| {
            ctx.env
                .get("MOIRAI_CLIENT")
                .filter(|v| !v.is_empty())
                .cloned()
        }) {
            return Ok((HeadKind::Client, c));
        }
        if ctx.door == Door::Mcp
            && let Some(s) = session
        {
            return Ok((HeadKind::Session, format!("session:{s}")));
        }
        ctx.cwd
            .as_deref()
            .map(|d| (HeadKind::Directory, crate::links::canon_abs(d)))
            .ok_or_else(|| {
                Refusal::usage("checkout needs a client, an MCP session or a working directory")
            })
    }

    /// `Checkout` ([API §11.3]): one `ClientHead` record setting the key to the ref or the detached commit; a directory
    /// row whose ref changes loses its designation ([F18 §3.5] last row). With `branch_new`, the branch is created at
    /// `target` and checked out, in one group. Keyed; the command's branch is the checked-out ref.
    // spec: [API §11.3]
    pub fn checkout(&mut self, target: &str, branch_new: Option<&str>, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("target".to_string(), Cj::Str(target.to_string()));
        if let Some(b) = branch_new {
            args.insert("branch_new".to_string(), Cj::Str(b.to_string()));
        }
        let payload = crate::idem::payload("Checkout", &args);
        let (kind, text) = self.checkout_key(ctx, caller.session.as_deref())?;
        let mut t = revision(self, target)?;
        let branch_name = match (&t, branch_new) {
            (_, Some(b)) => dag::check_new_name(b, false, None)?.0,
            (Target::Ref(r), None) => r.clone(),
            (Target::Detached(_), None) => String::new(),
        };
        let key = self.key_of(ctx, &caller, &branch_name, &payload);
        if !branch_name.is_empty()
            && let Some(r) = self.keyed(&key, &payload, &branch_name, ctx, &caller)?
        {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("branch")
            .map_err(|e| e.finish(None))?;
        let mut reply = Reply::ok(Data::None);
        reply.warnings = caller.warnings.clone();
        if let Some(b) = branch_new {
            let from = match &t {
                Target::Ref(r) => r.clone(),
                Target::Detached(c) => format!("s{c}"),
            };
            let (created, ..) = self.create_ref(b, &from, None, &caller.actor)?;
            t = Target::Ref(created);
        }
        if ctx.dry {
            reply.outcome = crate::api::Outcome::Dry;
            return Ok(reply);
        }
        let hlc = self.hlc.record(self.env.wall_ms);
        let prev = self.heads.get(kind, &text).cloned();
        let cleared = kind == HeadKind::Directory
            && prev.as_ref().is_some_and(|h| h.designated && h.target != t);
        let keep = prev.filter(|h| h.target == t);
        let (ref_, commit) = match &t {
            Target::Ref(r) => (Some(r.clone()), self.dag.live(r).and_then(|x| x.tip)),
            Target::Detached(c) => (None, Some(*c)),
        };
        self.heads.rows.insert(
            (kind, text.clone()),
            Head {
                kind,
                key: text.clone(),
                target: t.clone(),
                designated: keep.as_ref().is_some_and(|h| h.designated),
                expected_ref: keep.as_ref().and_then(|h| h.expected_ref.clone()),
                base: keep.as_ref().and_then(|h| h.base.clone()),
                hlc,
            },
        );
        self.record_idem(
            key,
            payload,
            ref_.as_deref().unwrap_or(""),
            None,
            ctx,
            Recorded {
                cmd: "Checkout".into(),
                ..Recorded::default()
            },
        );
        reply.branch = ref_.clone();
        reply.data = Data::Checkout(Box::new(CheckoutData {
            key_kind: kind,
            key: text,
            ref_,
            commit,
            designation_cleared: cleared,
        }));
        Ok(reply)
    }

    /// `WorktreeBind` ([API §11.4]; [F18 §3.5]): a `ClientHead` record setting the directory row of `dir` to `ref`. On a
    /// tree (a simulated tree's root, [F18 §3.3]) the row is designated, with the expected git ref the tree's symbolic
    /// HEAD names and the base its HEAD commit (none without git; no expected ref when detached), after the I-F12
    /// checks of [F18 §3.5] (`binding_conflict`, exit 5, unless `replace`, whose displaced designated row is rewritten
    /// with `designated` = 0 in the same group). A directory that is not a tree gets a binding only, with the warning
    /// `not_a_tree`.
    // spec: [API §11.4]; [F18 §3.5]
    // rule: WV-003
    pub fn worktree_bind(&mut self, dir: &str, ref_: &str, replace: bool, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let dir = crate::links::canon_abs(dir);
        let mut args = BTreeMap::new();
        args.insert("dir".to_string(), Cj::Str(dir.clone()));
        args.insert("ref".to_string(), Cj::Str(ref_.to_string()));
        if replace {
            args.insert("replace".to_string(), Cj::Bool(true));
        }
        let payload = crate::idem::payload("WorktreeBind", &args);
        let key = self.key_of(ctx, &caller, ref_, &payload);
        if let Some(r) = self.keyed(&key, &payload, ref_, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("worktree")
            .map_err(|e| e.finish(None))?;
        let r = self
            .dag
            .live(ref_)
            .filter(|r| matches!(r.kind, RefKind::Work | RefKind::Plan))
            .ok_or_else(|| Refusal::lq("E301", format!("no branch {ref_}")))?
            .name
            .clone();
        let mut reply = Reply::ok(Data::None);
        reply.warnings = caller.warnings.clone();
        reply.branch = Some(r.clone());
        reply.rev = Some(self.dag.live(&r).and_then(|x| x.tip).unwrap_or(0));
        let tree = self.files.fs.trees.contains_key(&dir);
        let mut replaced: Vec<(String, String)> = Vec::new();
        let (mut expected_ref, mut base) = (None, None);
        if tree {
            // The checks of [F18 §3.5] over D, in order.
            let d = self.designation();
            if let Some(t0) = d.iter().find(|p| p.branch == r && p.tree != dir) {
                if !replace {
                    return Err(Refusal::new(
                        "binding_conflict",
                        5,
                        format!("{r} already has the designated tree {}", t0.tree),
                    )
                    .key("tree", dir.clone())
                    .key("writer_tree", t0.tree.clone())
                    .key("ref", r.clone()));
                }
                if self
                    .heads
                    .get(HeadKind::Directory, &t0.tree)
                    .is_some_and(|h| h.designated)
                {
                    replaced.push((t0.tree.clone(), r.clone()));
                }
            }
            if let Some(b1) = d.iter().find(|p| p.tree == dir && p.branch != r) {
                if !replace {
                    return Err(Refusal::new(
                        "binding_conflict",
                        5,
                        format!("{dir} is already the designated tree of {}", b1.branch),
                    )
                    .key("tree", dir.clone())
                    .key("writer_tree", dir.clone())
                    .key("ref", b1.branch.clone()));
                }
                if self
                    .heads
                    .get(HeadKind::Directory, &dir)
                    .is_some_and(|h| h.designated)
                {
                    replaced.push((dir.clone(), b1.branch.clone()));
                }
            }
            if let Some((repo, head)) = self.files.git.of_tree(&dir) {
                if let crate::r4::git::Head::Ref(x) = head {
                    expected_ref = Some(x.strip_prefix("refs/heads/").unwrap_or(x).to_string());
                }
                base = repo
                    .head_commit(head)
                    .map(|c| format!("{}:{c}", repo.algo.name()));
            }
        } else {
            reply.warnings.push("not_a_tree".into());
        }
        if ctx.dry {
            reply.outcome = crate::api::Outcome::Dry;
            return Ok(reply);
        }
        // The displaced designated row keeps resolving branches, with a zero extension, in the same group.
        for (t0, _) in replaced.iter().filter(|(t0, _)| *t0 != dir) {
            let hlc = self.hlc.record(self.env.wall_ms);
            if let Some(h) = self.heads.rows.get_mut(&(HeadKind::Directory, t0.clone())) {
                h.designated = false;
                h.expected_ref = None;
                h.base = None;
                h.hlc = hlc;
            }
        }
        let hlc = self.hlc.record(self.env.wall_ms);
        self.heads.rows.insert(
            (HeadKind::Directory, dir.clone()),
            Head {
                kind: HeadKind::Directory,
                key: dir.clone(),
                target: Target::Ref(r.clone()),
                designated: tree,
                expected_ref: expected_ref.clone(),
                base: base.clone(),
                hlc,
            },
        );
        self.record_idem(
            key,
            payload,
            &r,
            None,
            ctx,
            Recorded {
                cmd: "WorktreeBind".into(),
                ..Recorded::default()
            },
        );
        reply.data = Data::Bind(Box::new(BindData {
            dir,
            ref_: Some(r),
            designated: tree,
            expected_ref,
            base,
            replaced,
            removed: false,
        }));
        Ok(reply)
    }

    /// `WorktreeUnbind` ([API §11.4]): the directory row of `dir` is removed.
    // spec: [API §11.4]
    pub fn worktree_unbind(&mut self, dir: &str, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let dir = crate::links::canon_abs(dir);
        let dir = dir.as_str();
        let mut args = BTreeMap::new();
        args.insert("dir".to_string(), Cj::Str(dir.to_string()));
        let payload = crate::idem::payload("WorktreeUnbind", &args);
        let row_ref = self
            .heads
            .get(HeadKind::Directory, dir)
            .and_then(|h| match &h.target {
                Target::Ref(r) => Some(r.clone()),
                Target::Detached(_) => None,
            })
            .unwrap_or_else(|| caller.branch.clone());
        let key = self.key_of(ctx, &caller, &row_ref, &payload);
        if let Some(r) = self.keyed(&key, &payload, &row_ref, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("worktree")
            .map_err(|e| e.finish(None))?;
        let mut reply = Reply::ok(Data::None);
        reply.warnings = caller.warnings.clone();
        if ctx.dry {
            reply.outcome = crate::api::Outcome::Dry;
            return Ok(reply);
        }
        let removed = self
            .heads
            .rows
            .remove(&(HeadKind::Directory, dir.to_string()))
            .is_some();
        if removed {
            self.hlc.record(self.env.wall_ms);
            self.record_idem(
                key,
                payload,
                &row_ref,
                None,
                ctx,
                Recorded {
                    cmd: "WorktreeUnbind".into(),
                    ..Recorded::default()
                },
            );
        }
        reply.data = Data::Bind(Box::new(BindData {
            dir: dir.to_string(),
            ref_: None,
            designated: false,
            expected_ref: None,
            base: None,
            replaced: Vec::new(),
            removed,
        }));
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_longest_bound_prefix_wins() {
        let mut h = Heads::default();
        for (d, r) in [("/w", "main"), ("/w/lane", "lane/a")] {
            h.rows.insert(
                (HeadKind::Directory, d.into()),
                Head {
                    kind: HeadKind::Directory,
                    key: d.into(),
                    target: Target::Ref(r.into()),
                    designated: false,
                    expected_ref: None,
                    base: None,
                    hlc: 0,
                },
            );
        }
        let b = |p: &str| h.binding_of(p).map(|x| x.target.clone());
        assert_eq!(b("/w/lane/src"), Some(Target::Ref("lane/a".into())));
        assert_eq!(b("/w/lanes"), Some(Target::Ref("main".into())));
        assert_eq!(b("/x"), None);
    }
}
