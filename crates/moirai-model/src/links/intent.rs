//! The intent protocol of `FileMv`, `FileRm` and `FileRevert` ([API §12.4]; [40 §3.4]–§3.6; [F05 §9.15]–§9.17;
//! [F11 §12.7]): the plan (writer tree, one volume, fresh alias sources, flushable parents, portable names), the
//! `FsIntent` record in its own group, the operations on the simulated tree, and one commit with the observations, the
//! `path_moves` entry, the glob rewrites or the removals, whose group carries the `FsIntentDone`. Items that fail are
//! reported and the rest commit (exit 8); an intent none of whose items was carried out is closed by
//! `FsIntentAborted` (reason 3). The store's trash holds `file rm --trash` removals at `trash/<intent>/<i>`
//! ([F02 §5.4]), outside every tree on the tree's volume, so `file revert` can move them back.

use crate::api::{Caller, Ctx, Data, Outcome, Reply, Store};
use crate::err::{Kv, Refusal, Res};
use crate::idem::Cj;
use crate::links::verbs::{Found, live_target};
use crate::links::{
    FileCand, IntentItem, IntentOp, IntentRow, IntentState, Landing, TreeCtx, add_alias, path_move,
    path_text, rewrite_globs, set_observation,
};
use crate::r4::cascade::Prep;
use crate::r4::path::portable_issues;
use crate::r4::tree::{OpError, TreeOp, basename, dirname};
use crate::r4::uid::FileStatus;
use crate::state::{EdgeKey, State};
use crate::status::Door;
use crate::value::{MoveClass, Nid, PathMove, PathVal, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One item of an intent's result ([API §12.4] `items`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemOut {
    /// `src`.
    pub src: PathVal,
    /// `dst`: the destination of a move; `None` for a removal.
    pub dst: Option<PathVal>,
    /// `outcome`: `done`, `busy`, `exists`, `missing` or `failed`.
    pub outcome: &'static str,
}

/// What a `FileRm` dry run shows ([40 §3.5] step 1).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Impact {
    /// Whole-file links: the referrers whose `file` anchors point at a removed file node.
    pub links: Vec<Nid>,
    /// Span anchors into a removed file node, by handle.
    pub anchors: Vec<String>,
    /// Glob entries that would match nothing afterwards: (node, field, element).
    pub globs: Vec<(Nid, String, String)>,
    /// Prose mentions of a removed path (report only): the live nodes whose title or body names it.
    pub mentions: Vec<Nid>,
    /// Pending intents: the open intents of the tree.
    pub intents: Vec<String>,
}

/// The data of `FileMv`, `FileRm` and `FileRevert` ([API §12.4] "Result").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntentData {
    /// `intent`: `i-<n>`; `None` for a dry run.
    pub intent: Option<String>,
    /// `items`.
    pub items: Vec<ItemOut>,
    /// `repointed`: the file nodes whose observation or status the commit changed, ascending.
    pub repointed: Vec<Nid>,
    /// `path_move`: the `explicit` entry of a directory move.
    pub path_move: Option<PathMove>,
    /// `globs`: (node, field, from, to).
    pub globs: Vec<(Nid, String, String, String)>,
    /// `impact`: a `FileRm` dry run's only.
    pub impact: Option<Impact>,
}

/// The name of an item outcome ([F11 §12.7] → [API §12.4]).
pub fn outcome_name(o: u8) -> &'static str {
    match o {
        1 => "done",
        2 => "busy",
        3 => "exists",
        4 => "missing",
        _ => "failed",
    }
}

/// The item outcome of a tree operation's error ([F11 §12.7]).
fn outcome_of(e: &OpError) -> u8 {
    match e {
        OpError::NotFound(_) => 4,
        OpError::Exists(_) => 3,
        OpError::CrossVolume | OpError::NoTree(_) => 5,
    }
}

/// The store's trash location of an intent's item on a tree's volume ([F02 §5.4] `trash/<intent>/<i>`): the model's
/// store lies at `/.moirai-store` of the volume, outside every tree.
pub fn trash_path(root: &str, intent: u64, i: usize) -> String {
    let vol = root.split('/').next().unwrap_or("");
    format!("{vol}/.moirai-store/trash/{intent}/{i}")
}

impl Store {
    /// Whether a command is a `FileMv` of a directory, a bulk-class command ([API §9.10]).
    pub fn moves_a_directory(&self, cmd: &crate::api::Cmd, ctx: &Ctx) -> bool {
        let crate::api::Cmd::FileMv { srcs, .. } = cmd else {
            return false;
        };
        let Ok(caller) = self.resolve(ctx, false) else {
            return false;
        };
        let Some(tc) = self.tree_ctx(&caller, ctx) else {
            return false;
        };
        srcs.iter()
            .any(|s| matches!(self.find_path(&tc, s, false), Ok(Found::Dir { .. })))
    }

    /// The runtime snapshot's `intents` ([API §15.7]): every intent still open, and every closed one within
    /// `gc.trash-expire` of its closing record (CK-6; [F11 §12.7] "Retention"), in the order opened.
    pub fn listed_intents(&self) -> Vec<IntentRow> {
        let window = self.conf.number("gc.trash-expire");
        self.files
            .intents
            .iter()
            .filter(|i| {
                i.state == IntentState::Open
                    || self.hlc.within(self.env.wall_ms, i.closed_hlc, window)
            })
            .cloned()
            .collect()
    }

    /// The plan checks every file-system verb shares ([API §12.4] step 1): `tree_mismatch` (row 5), the writer tree of
    /// the caller's branch (`not_writer_tree`, exit 5).
    fn fs_plan(&self, caller: &Caller, ctx: &Ctx, arg: &str) -> Res<TreeCtx> {
        self.tree_mismatch(caller, ctx)?;
        let tc = self.need_tree(caller, ctx, arg)?;
        if !tc.writer {
            return Err(self.not_writer_tree(&tc, &caller.branch));
        }
        Ok(tc)
    }

    /// `no_dir_flush` ([API §12.4] step 1; [OS/project §6.2]): both parent directories must be flushable, which the
    /// simulated tree's `caps` decide (`dir_flush_doubtful`: `sync_dir` is refused, `Unsupported`).
    fn dir_flush(&self, tc: &TreeCtx, dirs: &[String]) -> Res<()> {
        if self.files.fs.trees[&tc.root].caps.dir_flush_doubtful {
            let d = dirs.first().cloned().unwrap_or_default();
            let dir = if d.is_empty() {
                tc.root.clone()
            } else {
                format!("{}/{d}", tc.root.trim_end_matches('/'))
            };
            return Err(Refusal::new(
                "no_dir_flush",
                7,
                format!("{dir} is on a volume where moirai cannot flush a directory: Unsupported"),
            )
            .key("dir", dir)
            .key(
                "os",
                Kv::Obj(vec![
                    ("code".into(), Kv::Int(50)),
                    ("symbol".into(), Kv::Str("Unsupported".into())),
                ]),
            ));
        }
        Ok(())
    }

    /// Opens an intent: the `FsIntent` record in its own group, which draws its HLC ([F05 §9.15]; CK-4); its id is the
    /// model's number for the record's lsn.
    fn open_intent(
        &mut self,
        op: IntentOp,
        branch: &str,
        tree: &str,
        items: Vec<IntentItem>,
        flags: (bool, bool),
    ) -> u64 {
        let hlc = self.hlc.record(self.env.wall_ms);
        self.files.next_intent += 1;
        let id = self.files.next_intent;
        self.files.intents.push(IntentRow {
            id,
            op,
            git: flags.0,
            recursive: flags.1,
            recovered: false,
            branch: branch.to_string(),
            tree: tree.to_string(),
            hlc,
            closed_hlc: 0,
            state: IntentState::Open,
            items,
            commit: None,
        });
        id
    }

    /// Closes an intent: `FsIntentDone` with the item outcomes and the commit whose group carries it, or
    /// `FsIntentAborted` with its reason; `hlc` is the closing record's.
    fn close_intent(
        &mut self,
        id: u64,
        outcomes: &[u8],
        commit: Option<u64>,
        abort: Option<u8>,
        hlc: u64,
    ) {
        let row = self
            .files
            .intents
            .iter_mut()
            .find(|i| i.id == id)
            .expect("an open intent");
        row.closed_hlc = hlc;
        row.commit = commit;
        match abort {
            Some(r) => row.state = IntentState::Aborted(r),
            None => {
                row.state = IntentState::Done;
                for (it, o) in row.items.iter_mut().zip(outcomes) {
                    it.outcome = *o;
                }
            }
        }
    }

    /// The live file nodes a move or removal of `src` affects, each with its path on the source side ([API §12.4]
    /// step 1: "affected nodes by path and aliases"; [40 §3.4] step 1): a node whose path is `src` or, for a directory,
    /// under it; and a node for which the source is **only an alias** — no live node holds that path as its current
    /// path, so the file there is the node's own under its old name in a tree that has not received its re-bind — which
    /// is accepted only when the tree is fresh for it (`not_fresh`, exit 6).
    fn affected(
        &self,
        c: &FileCand,
        tc: &TreeCtx,
        branch: &str,
        src: &str,
        dir: bool,
    ) -> Res<Vec<(Nid, String)>> {
        let tip = self.dag.live(branch).and_then(|r| r.tip);
        let view = self.view_of(&c.st, tip);
        let rt = self.files.runtime();
        let p = self.params(tc, branch, false, &view);
        let under = |x: &str| {
            if dir {
                x.starts_with(&format!("{src}/"))
            } else {
                x == src
            }
        };
        let live: Vec<&crate::r4::cascade::FileNode> = view
            .files
            .iter()
            .filter(|f| !f.tombstone && f.root == "project" && f.status != FileStatus::Removed)
            .collect();
        let held: BTreeSet<&str> = live.iter().map(|f| f.path.as_str()).collect();
        let prep = Prep::new(&view, &self.files.fs, &self.files.git, &rt, &tc.root, &p);
        let mut out = Vec::new();
        for f in &live {
            if under(&f.path) {
                out.push((Nid(f.n), f.path.clone()));
                continue;
            }
            let Some(alias) = f
                .aliases
                .iter()
                .find(|a| under(a) && !held.contains(a.as_str()))
            else {
                continue;
            };
            let r = prep.resolve(f, &p);
            if !r.fresh {
                return Err(Refusal::new(
                    "not_fresh",
                    6,
                    format!(
                        "{src} is an old path of #{}, and this tree has not seen its latest move",
                        f.n
                    ),
                )
                .key("path", format!("project:{src}"))
                .key("node", Kv::Node(Nid(f.n))));
            }
            out.push((Nid(f.n), alias.clone()));
        }
        Ok(out)
    }

    /// `FileMv` ([API §12.4]; [40 §3.4]): the plan, the `FsIntent`, one rename per item in the simulated tree, then one
    /// commit that sets each affected node's observation (`explicit/intent`, the old path in `aliases`, `observed_git` =
    /// HEAD, `observed_blob` empty), adds a directory's `explicit` `path_moves` entry and rewrites the globs under it,
    /// with `FsIntentDone` in its group; `stmt_origin` `file-verb`, `stmt_sym` `mv`. Several sources, or a destination
    /// that is a directory, move each source into it.
    // spec: [API §12.4]; [40 §3.4]; [F05 §9.15]; [F11 §12.7]
    // rule: WV-030, WR-013
    pub fn file_mv(
        &mut self,
        srcs: &[String],
        dst: &str,
        git: bool,
        retry_ms: Option<u64>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert(
            "srcs".to_string(),
            Cj::Arr(srcs.iter().map(|p| Cj::Str(p.clone())).collect()),
        );
        args.insert("dst".to_string(), Cj::Str(dst.to_string()));
        if git {
            args.insert("git".to_string(), Cj::Bool(true));
        }
        if let Some(r) = retry_ms.filter(|r| *r != 1000) {
            args.insert("retry_ms".to_string(), Cj::Int(r as i64));
        }
        let payload = Self::file_payload("FileMv", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("file-mv")
            .map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        if srcs.is_empty() {
            return Err(Refusal::usage_arg("srcs", "file mv needs a source"));
        }
        let tc = self.fs_plan(&caller, ctx, dst)?;
        // The items: each source with its destination.
        let into = match self.find_path(&tc, dst, false)? {
            Found::Dir { rel } => Some(rel),
            Found::File { .. } | Found::Missing { .. } if srcs.len() > 1 => {
                return Err(Refusal::not_found("directory", dst));
            }
            _ => None,
        };
        let dst_rel = match self.find_path(&tc, dst, false)? {
            Found::File { rel, .. } | Found::Dir { rel } | Found::Missing { rel } => rel,
        };
        let mut plan: Vec<(String, String, bool)> = Vec::new();
        for s in srcs {
            let (rel, dir) = match self.find_path(&tc, s, false)? {
                Found::File { rel, .. } => (rel, false),
                Found::Dir { rel } => (rel, true),
                Found::Missing { rel } => (rel, false),
            };
            let d = match &into {
                Some(dir) => format!("{dir}/{}", basename(&rel)),
                None => dst_rel.clone(),
            };
            plan.push((rel, d, dir));
        }
        // The destination parents exist ([F18] open point 28); one volume; flushable parents.
        let t = &self.files.fs.trees[&tc.root];
        for (_, d, _) in &plan {
            let parent = dirname(d);
            if !parent.is_empty() && !t.dirs.contains_key(parent) {
                return Err(Refusal::not_found("directory", format!("project:{parent}")));
            }
        }
        let parents: Vec<String> = plan
            .iter()
            .flat_map(|(s, d, _)| [dirname(s).to_string(), dirname(d).to_string()])
            .collect();
        self.dir_flush(&tc, &parents)?;
        // Portable names ([OS/path §8.2]): refused under `files.portable-names = refuse`, else warned.
        let mut warn = false;
        for (_, d, _) in &plan {
            let parent = dirname(d);
            let sibs = t.entries(parent);
            let sibs: Vec<&str> = sibs.iter().map(String::as_str).collect();
            let issues = portable_issues(basename(d), &sibs);
            if let Some(i) = issues.first() {
                if crate::links::portable_name_policy(&self.conf) {
                    let rule = match i {
                        crate::r4::path::PortableIssue::DeviceName => "device-name",
                        crate::r4::path::PortableIssue::TrailingDotOrSpace => {
                            "trailing-dot-or-space"
                        }
                        crate::r4::path::PortableIssue::ReservedChar(_) => "reserved-char",
                        crate::r4::path::PortableIssue::TooLong => "too-long",
                        crate::r4::path::PortableIssue::FoldSibling(_) => "fold-sibling",
                    };
                    return Err(Refusal::new(
                        "nonportable_name",
                        2,
                        format!("project:{d} is not portable: {rule}"),
                    )
                    .key("path", format!("project:{d}"))
                    .key("rule", rule));
                }
                warn = true;
            }
        }
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        let mut affected: Vec<Vec<(Nid, String)>> = Vec::new();
        for (s, _, dir) in &plan {
            affected.push(self.affected(&c, &tc, &caller.branch, s, *dir)?);
        }
        // Each affected node's destination: the source-side path with the source replaced by the destination.
        let dest = |s: &str, d: &str, dir: bool, from: &str| {
            if dir {
                format!("{d}{}", &from[s.len()..])
            } else {
                d.to_string()
            }
        };
        // I-F1: no destination key may have another live holder ([F18 §2.1]; `path_claimed`, exit 6).
        let moving: Vec<Nid> = affected.iter().flatten().map(|(n, _)| *n).collect();
        for ((s, d, dir), ns) in plan.iter().zip(&affected) {
            for (_, from) in ns {
                Self::claimed(&c.st, &moving, "project", &dest(s, d, *dir, from))?;
            }
        }
        let algo = self.root_algo("project", &tc.root);
        let items: Vec<IntentItem> = plan
            .iter()
            .map(|(s, d, dir)| IntentItem {
                dir: *dir,
                outcome: 0,
                src: PathVal {
                    root: "project".into(),
                    text: s.clone(),
                },
                dst: Some(PathVal {
                    root: "project".into(),
                    text: d.clone(),
                }),
                oid: (!*dir)
                    .then(|| {
                        t.disk_spelling(s)
                            .and_then(|x| t.files.get(&x))
                            .map(|f| crate::r4::text::oid(algo, &f.bytes))
                    })
                    .flatten(),
            })
            .collect();
        if ctx.dry {
            let mut reply = Reply::ok(Data::Intent(Box::new(IntentData {
                intent: None,
                items: items
                    .iter()
                    .map(|i| ItemOut {
                        src: i.src.clone(),
                        dst: i.dst.clone(),
                        outcome: "done",
                    })
                    .collect(),
                repointed: affected.iter().flatten().map(|(n, _)| *n).collect(),
                path_move: None,
                globs: Vec::new(),
                impact: None,
            })));
            reply.outcome = Outcome::Dry;
            reply.branch = Some(caller.branch.clone());
            reply.rev = Some(tip.unwrap_or(0));
            return Ok(reply);
        }
        // Step 2: the intent; step 3: the renames.
        let id = self.open_intent(
            IntentOp::Mv,
            &caller.branch,
            &tc.root,
            items.clone(),
            (git, false),
        );
        let now_ns = self.env.wall_ms.saturating_mul(1_000_000);
        // The operations name the entries as the directories spell them on disk.
        let disk = |s: &str| -> String {
            self.files.fs.trees[&tc.root]
                .disk_spelling(s)
                .unwrap_or_else(|| s.to_string())
        };
        let disks: Vec<String> = plan.iter().map(|(s, _, _)| disk(s)).collect();
        let outcomes: Vec<u8> = plan
            .iter()
            .zip(&disks)
            .map(|((_, d, _), from)| {
                match self.files.fs.apply(
                    &tc.root,
                    &TreeOp::Mv {
                        from: from.clone(),
                        to: d.clone(),
                    },
                    now_ns,
                ) {
                    Ok(()) => 1,
                    Err(e) => outcome_of(&e),
                }
            })
            .collect();
        let hlc = self.hlc.peek_commit(self.env.wall_ms);
        let (head, _) = self.head_blob(&tc.root);
        let mut repointed = Vec::new();
        let mut pm = None;
        let mut globs = Vec::new();
        for ((i, (s, d, dir)), o) in plan.iter().enumerate().zip(&outcomes) {
            if *o != 1 {
                continue;
            }
            for (n, from) in &affected[i] {
                let path = dest(s, d, *dir, from);
                let old = crate::links::file_node(*n, &c.st.nodes[n], 0).path;
                let mut obs = self.observe(&tc, "project", &path, Some(&path));
                obs.observed_blob = None;
                obs.observed_git = head.clone();
                obs.relink = Some("explicit/intent".into());
                set_observation(&mut c.st, *n, &obs);
                add_alias(
                    &mut c.st,
                    *n,
                    &PathVal {
                        root: "project".into(),
                        text: old,
                    },
                );
                repointed.push(*n);
            }
            if *dir {
                let m = path_move(
                    hlc,
                    MoveClass::Explicit,
                    "project",
                    &format!("{s}/"),
                    &format!("{d}/"),
                    head.clone(),
                );
                c.add_path_move(&self.alloc.uidx, m.clone())?;
                globs.extend(rewrite_globs(&mut c.st, &format!("{s}/"), &format!("{d}/")));
                pm.get_or_insert(m);
            }
        }
        repointed.sort();
        repointed.dedup();
        let items_out: Vec<ItemOut> = items
            .iter()
            .zip(&outcomes)
            .map(|(i, o)| ItemOut {
                src: i.src.clone(),
                dst: i.dst.clone(),
                outcome: outcome_name(*o),
            })
            .collect();
        let data = IntentData {
            intent: Some(format!("i-{id}")),
            items: items_out,
            repointed,
            path_move: pm,
            globs,
            impact: None,
        };
        self.finish_intent(
            caller, ctx, key, payload, "FileMv", "mv", id, &outcomes, c, tip, data, warn,
        )
    }

    /// The HEAD commit of a tree as an `oid`.
    fn head_blob(&self, root: &str) -> (Option<crate::value::Oid>, Option<String>) {
        let Some((repo, head)) = self.files.git.of_tree(root) else {
            return (None, None);
        };
        let h = repo.head_commit(head);
        (
            h.and_then(|h| crate::links::git_oid(repo.algo, h)),
            h.map(str::to_string),
        )
    }

    /// Step 4 and the close of an intent: the commit with `FsIntentDone` in its group when an item was carried out, else
    /// `FsIntentAborted` (reason 3); exit 8 when an item failed ([API §12.4] step 5; [F19 §7.1]).
    #[allow(clippy::too_many_arguments)]
    fn finish_intent(
        &mut self,
        caller: Caller,
        ctx: &Ctx,
        key: Option<([u8; 16], bool)>,
        payload: [u8; 16],
        cmd: &'static str,
        sym: &str,
        id: u64,
        outcomes: &[u8],
        c: FileCand,
        tip: Option<u64>,
        data: IntentData,
        warn: bool,
    ) -> Res<Reply> {
        let any_done = outcomes.contains(&1);
        let failed = outcomes.iter().any(|o| *o != 1);
        let tree = self
            .files
            .intents
            .iter()
            .find(|i| i.id == id)
            .map(|i| i.tree.clone())
            .unwrap_or_default();
        let seen: Vec<(Nid, Option<String>)> = data
            .repointed
            .iter()
            .map(|n| {
                let x = &c.st.nodes[n];
                let f = crate::links::file_node(*n, x, 0);
                (*n, (f.status != FileStatus::Removed).then_some(f.path))
            })
            .collect();
        let mut reply;
        if any_done {
            // The recorded result names the intent, whose records rebuild the data on a replay ([API §7.5]).
            let land = Landing {
                caller: &caller,
                ctx,
                key,
                payload,
                cmd,
                origin: "file-verb",
                sym: sym.to_string(),
                yields: vec![crate::tx::Yield {
                    index: 0,
                    proc: cmd.to_string(),
                    rows: vec![vec![("intent".to_string(), format!("i-{id}"))]],
                }],
            };
            let (r, seq) = self.land_file(land, c, tip, "", 1)?;
            self.observe_rows(ctx, &tree, &seen);
            reply = r;
            // The last record of the group is its FsIntentDone (CK-4).
            let hlc = self.hlc.seq;
            self.close_intent(id, outcomes, seq, None, hlc);
            self.files
                .results
                .insert(id, Data::Intent(Box::new(data.clone())));
        } else {
            let hlc = self.hlc.record(self.env.wall_ms);
            self.close_intent(id, outcomes, None, Some(3), hlc);
            reply = Reply::ok(Data::None);
            reply.branch = Some(caller.branch.clone());
            reply.rev = Some(tip.unwrap_or(0));
            reply.commit = tip;
            reply.key = ctx.key.clone();
            reply.warnings = caller.warnings.clone();
        }
        reply.ready.clear();
        reply.other.clear();
        reply.yields.clear();
        if warn {
            reply.warnings.push("nonportable_name".into());
        }
        if failed {
            reply.exit = 8;
        }
        reply.data = Data::Intent(Box::new(data));
        Ok(reply)
    }

    /// `FileRm` ([API §12.4]; [40 §3.5]): without `yes` the plan's impact as a dry run (exit 0); with `yes` the
    /// `FsIntent`, the deletions (or, with `trash`, the moves into the store's trash), then one commit setting each
    /// affected node's status `removed` with `reason` and `replaced_by` and re-pointing its anchors — with `replaced_by`
    /// Q, whole-file anchors to Q's node and span anchors that resolve `fresh` or `moved` in Q's content; the rest stay
    /// ([40 §3.5] step 3); `stmt_sym` `rm`. A directory needs `recursive`.
    // spec: [API §12.4]; [40 §3.5]
    // rule: WV-031, TR-050
    pub fn file_rm(
        &mut self,
        paths: &[String],
        reason: Option<&str>,
        replaced_by: Option<&str>,
        (trash, recursive, yes): (bool, bool, bool),
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert(
            "paths".to_string(),
            Cj::Arr(paths.iter().map(|p| Cj::Str(p.clone())).collect()),
        );
        for (k, v) in [("reason", reason), ("replaced_by", replaced_by)] {
            if let Some(v) = v {
                args.insert(k.to_string(), Cj::Str(v.to_string()));
            }
        }
        for (k, v) in [("trash", trash), ("recursive", recursive), ("yes", yes)] {
            if v {
                args.insert(k.to_string(), Cj::Bool(true));
            }
        }
        let payload = Self::file_payload("FileRm", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("file-rm")
            .map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        if paths.is_empty() {
            return Err(Refusal::usage_arg("paths", "file rm needs a path"));
        }
        let tc = self.fs_plan(&caller, ctx, &paths[0])?;
        let mut plan: Vec<(String, bool)> = Vec::new();
        for p in paths {
            match self.find_path(&tc, p, false)? {
                Found::File { rel, .. } => plan.push((rel, false)),
                Found::Dir { rel } if recursive => plan.push((rel, true)),
                Found::Dir { rel } => {
                    return Err(Refusal::usage_arg(
                        "recursive",
                        format!(
                            "project:{rel} is a directory; file rm of a directory needs recursive"
                        ),
                    ));
                }
                Found::Missing { rel } => plan.push((rel, false)),
            }
        }
        let parents: Vec<String> = plan.iter().map(|(s, _)| dirname(s).to_string()).collect();
        self.dir_flush(&tc, &parents)?;
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        let affected: Vec<Vec<Nid>> = plan
            .iter()
            .map(|(s, dir)| {
                self.affected(&c, &tc, &caller.branch, s, *dir)
                    .map(|v| v.into_iter().map(|(n, _)| n).collect())
            })
            .collect::<Res<_>>()?;
        // `replaced_by`: a node, or a path of the tree (registered by capture when it has no node yet).
        let q = match replaced_by {
            None => None,
            Some(r) => Some(match crate::tx::parse_node(r) {
                Some(t) => live_target(self, &c.st, &t)?,
                None => match self.find_path(&tc, r, false)? {
                    Found::File { rel, disk } => {
                        self.register_file(&mut c, &tc, "project", &rel, Some(&disk), false, None)?
                            .0
                    }
                    Found::Dir { rel } | Found::Missing { rel } => {
                        return Err(Refusal::not_found("path", rel));
                    }
                },
            }),
        };
        let algo = self.root_algo("project", &tc.root);
        let t = &self.files.fs.trees[&tc.root];
        let items: Vec<IntentItem> = plan
            .iter()
            .map(|(s, dir)| IntentItem {
                dir: *dir,
                outcome: 0,
                src: PathVal {
                    root: "project".into(),
                    text: s.clone(),
                },
                dst: None,
                oid: (!*dir)
                    .then(|| {
                        t.disk_spelling(s)
                            .and_then(|x| t.files.get(&x))
                            .map(|f| crate::r4::text::oid(algo, &f.bytes))
                    })
                    .flatten(),
            })
            .collect();
        if !yes || ctx.dry {
            let impact = self.impact(
                &c.st,
                &tc,
                &plan,
                affected.iter().flatten().copied().collect(),
            );
            let mut reply = Reply::ok(Data::Intent(Box::new(IntentData {
                intent: None,
                items: items
                    .iter()
                    .map(|i| ItemOut {
                        src: i.src.clone(),
                        dst: None,
                        outcome: "done",
                    })
                    .collect(),
                repointed: affected.iter().flatten().copied().collect(),
                path_move: None,
                globs: Vec::new(),
                impact: Some(impact),
            })));
            reply.outcome = Outcome::Dry;
            reply.branch = Some(caller.branch.clone());
            reply.rev = Some(tip.unwrap_or(0));
            reply.warnings = caller.warnings.clone();
            return Ok(reply);
        }
        let op = if trash {
            IntentOp::RmTrash
        } else {
            IntentOp::Rm
        };
        let id = self.open_intent(
            op,
            &caller.branch,
            &tc.root,
            items.clone(),
            (false, recursive),
        );
        let now_ns = self.env.wall_ms.saturating_mul(1_000_000);
        let disks: Vec<String> = plan
            .iter()
            .map(|(s, _)| {
                self.files.fs.trees[&tc.root]
                    .disk_spelling(s)
                    .unwrap_or_else(|| s.clone())
            })
            .collect();
        let outcomes: Vec<u8> = disks
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let op = if trash {
                    TreeOp::Mv {
                        from: s.clone(),
                        to: trash_path(&tc.root, id, i),
                    }
                } else {
                    TreeOp::Rm { path: s.clone() }
                };
                match self.files.fs.apply(&tc.root, &op, now_ns) {
                    Ok(()) => 1,
                    Err(e) => outcome_of(&e),
                }
            })
            .collect();
        let mut repointed = Vec::new();
        let consts = crate::links::anchor_consts(&self.conf);
        for (i, o) in outcomes.iter().enumerate() {
            if *o != 1 {
                continue;
            }
            for n in &affected[i] {
                let from = c.st.nodes[n].status.clone();
                crate::status::transition("artifact", &from, "removed", Door::FileRm)?;
                self.remove_file(&mut c.st, *n, "removed", reason, q);
                if let Some(qn) = q {
                    self.repoint_anchors(&mut c.st, *n, qn, &tc, algo, &consts, false);
                }
                repointed.push(*n);
            }
        }
        repointed.sort();
        repointed.dedup();
        let data = IntentData {
            intent: Some(format!("i-{id}")),
            items: items
                .iter()
                .zip(&outcomes)
                .map(|(i, o)| ItemOut {
                    src: i.src.clone(),
                    dst: None,
                    outcome: outcome_name(*o),
                })
                .collect(),
            repointed,
            path_move: None,
            globs: Vec::new(),
            impact: None,
        };
        self.finish_intent(
            caller, ctx, key, payload, "FileRm", "rm", id, &outcomes, c, tip, data, false,
        )
    }

    /// Sets a file node `removed` (or back to `present`) with the tombstone-free removal fields `reason` and
    /// `replaced_by` ([40 §2.2]; [F08 §9.3]).
    pub fn remove_file(
        &self,
        st: &mut State,
        n: Nid,
        status: &str,
        reason: Option<&str>,
        q: Option<Nid>,
    ) {
        let schema = st.schema.clone();
        let x = st.nodes.get_mut(&n).expect("a file node");
        x.status = status.to_string();
        x.resolution = "none".into();
        x.set_field(
            &schema,
            "reason",
            reason.map(|r| Value::Text(r.to_string())),
        );
        x.set_field(&schema, "replaced_by", q.map(Value::Ref));
    }

    /// Re-points the anchors into file node `f` to `q` ([40 §3.5] step 3; [40 §3.7] `--same-as`, `--split`): every
    /// whole-file anchor, and every span anchor whose record resolves `fresh` or `moved` in q's content at its path in
    /// the tree (every anchor when `all`); the anchor uid and record do not change (RK-006). The rest stay on `f`.
    #[allow(clippy::too_many_arguments)]
    pub fn repoint_anchors(
        &self,
        st: &mut State,
        f: Nid,
        q: Nid,
        tc: &TreeCtx,
        algo: crate::value::Algo,
        consts: &crate::r4::anchor::Consts,
        all: bool,
    ) {
        let qpath = crate::links::file_node(q, &st.nodes[&q], 0).path;
        let content = self.files.fs.trees[&tc.root]
            .read(&qpath)
            .ok()
            .map(<[u8]>::to_vec);
        let srcs: Vec<Nid> = st.nodes.keys().copied().collect();
        for s in srcs {
            let keys: Vec<EdgeKey> = st.nodes[&s]
                .out
                .keys()
                .filter(|k| k.kind == "at" && k.dst == f)
                .cloned()
                .collect();
            for k in keys {
                let props = st.nodes[&s].out[&k].clone();
                let moves = all
                    || props.anchor.as_deref().is_none_or(|a| {
                        a.kind == "file"
                            || content.as_deref().is_some_and(|b| {
                                let r = crate::r4::anchor::resolve(
                                    &crate::r4::anchor::Anchor::from_canon(
                                        k.disc.unwrap_or(crate::value::Uid::ZERO),
                                        a,
                                    ),
                                    crate::r4::anchor::Content::Bytes(b),
                                    algo,
                                    consts,
                                );
                                matches!(
                                    r.state,
                                    crate::r4::anchor::AState::Fresh
                                        | crate::r4::anchor::AState::Moved
                                )
                            })
                    });
                if !moves {
                    continue;
                }
                let x = st.nodes.get_mut(&s).expect("a source");
                x.out.remove(&k);
                x.out.insert(
                    EdgeKey {
                        kind: "at".into(),
                        dst: q,
                        disc: k.disc,
                    },
                    props,
                );
            }
        }
    }

    /// The impact of a `FileRm` ([40 §3.5] step 1).
    fn impact(&self, st: &State, tc: &TreeCtx, plan: &[(String, bool)], nodes: Vec<Nid>) -> Impact {
        let mut im = Impact::default();
        for (s, x) in &st.nodes {
            if !x.live() {
                continue;
            }
            for (k, p) in &x.out {
                if k.kind != "at" || !nodes.contains(&k.dst) {
                    continue;
                }
                match p.anchor.as_deref() {
                    Some(a) if a.kind != "file" => {
                        let h = k
                            .disc
                            .and_then(|u| self.files.anchors.get(&u))
                            .copied()
                            .unwrap_or(0);
                        im.anchors.push(format!("a{h}"));
                    }
                    _ => im.links.push(*s),
                }
            }
        }
        let removed: Vec<&String> = plan.iter().map(|(s, _)| s).collect();
        let t = &self.files.fs.trees[&tc.root];
        let gone = |p: &str| {
            removed
                .iter()
                .any(|r| p == r.as_str() || p.starts_with(&format!("{r}/")))
        };
        for (n, x) in &st.nodes {
            if !x.live() {
                continue;
            }
            for (f, v) in &x.fields {
                if st
                    .schema
                    .field(&x.kind, f)
                    .is_none_or(|fi| fi.class != "glob-set")
                {
                    continue;
                }
                for e in v.elems() {
                    let Some(g) = e.as_str() else { continue };
                    let g = g.strip_prefix("path:").unwrap_or(g);
                    let hits: Vec<&String> = t
                        .files
                        .keys()
                        .filter(|p| crate::r4::path::glob_match(g, p))
                        .collect();
                    if !hits.is_empty() && hits.iter().all(|p| gone(p)) {
                        im.globs
                            .push((*n, f.clone(), e.as_str().unwrap_or("").to_string()));
                    }
                }
            }
            let text = format!(
                "{}\n{}",
                x.text("title").unwrap_or(""),
                x.body.as_deref().unwrap_or("")
            );
            if removed.iter().any(|r| text.contains(r.as_str())) {
                im.mentions.push(*n);
            }
        }
        im.links.sort();
        im.links.dedup();
        im.mentions.sort();
        im.intents = self
            .files
            .intents
            .iter()
            .filter(|i| i.tree == tc.root && i.state == IntentState::Open)
            .map(|i| format!("i-{}", i.id))
            .collect();
        im
    }

    /// `FileRevert` ([API §12.4]; [40 §3.6]; [AR §5e.6]): the inverse file-system operations of the intent a commit's
    /// group closed, through the same protocol — each moved item back from its destination to its source, each item a
    /// `trash` removal moved into the trash back to its path; an item removed without `trash` has no bytes to restore
    /// and ends `missing` — then one commit recording the link change: moved nodes re-pointed back
    /// (`explicit/intent`), removed nodes `present` again; `stmt_sym` `revert`. A commit whose group carried no
    /// `FsIntentDone` is `not_found` (`intent`), exit 3.
    // spec: [API §12.4]; [40 §3.6]
    // rule: LH-006, WV-032
    pub fn file_revert(&mut self, commit: &str, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("commit".to_string(), Cj::Str(commit.to_string()));
        let payload = Self::file_payload("FileRevert", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("file-revert")
            .map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        let seq = self
            .dag
            .rev_commit(commit, &self.rev_ctx(&caller))
            .ok()
            .flatten();
        let orig = seq
            .and_then(|s| {
                self.files
                    .intents
                    .iter()
                    .find(|i| i.commit == Some(s) && i.state == IntentState::Done)
            })
            .cloned()
            .ok_or_else(|| Refusal::not_found("intent", commit))?;
        let tc = self.fs_plan(&caller, ctx, &orig.tree)?;
        if tc.root != orig.tree {
            return Err(self.not_writer_tree(&tc, &caller.branch));
        }
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        // The inverse items, of the items the intent carried out.
        let done: Vec<(usize, &IntentItem)> = orig
            .items
            .iter()
            .enumerate()
            .filter(|(_, i)| i.outcome == 1)
            .collect();
        let mut inverse: Vec<(Option<String>, String, bool, Option<usize>)> = Vec::new();
        for (i, it) in &done {
            match (&orig.op, &it.dst) {
                (IntentOp::Mv, Some(d)) => {
                    inverse.push((Some(d.text.clone()), it.src.text.clone(), it.dir, None))
                }
                (IntentOp::RmTrash, _) => {
                    inverse.push((None, it.src.text.clone(), it.dir, Some(*i)))
                }
                _ => inverse.push((None, it.src.text.clone(), it.dir, None)),
            }
        }
        let items: Vec<IntentItem> = inverse
            .iter()
            .map(|(from, to, dir, t)| IntentItem {
                dir: *dir,
                outcome: 0,
                src: match (from, t) {
                    (Some(f), _) => PathVal {
                        root: "project".into(),
                        text: f.clone(),
                    },
                    (None, Some(i)) => PathVal {
                        root: "abs".into(),
                        text: trash_path(&orig.tree, orig.id, *i),
                    },
                    (None, None) => PathVal {
                        root: "project".into(),
                        text: to.clone(),
                    },
                },
                dst: Some(PathVal {
                    root: "project".into(),
                    text: to.clone(),
                }),
                oid: None,
            })
            .collect();
        let parents: Vec<String> = inverse
            .iter()
            .map(|(_, to, _, _)| dirname(to).to_string())
            .collect();
        self.dir_flush(&tc, &parents)?;
        if ctx.dry {
            let mut reply = Reply::ok(Data::Intent(Box::new(IntentData {
                intent: None,
                items: items
                    .iter()
                    .map(|i| ItemOut {
                        src: i.src.clone(),
                        dst: i.dst.clone(),
                        outcome: "done",
                    })
                    .collect(),
                repointed: Vec::new(),
                path_move: None,
                globs: Vec::new(),
                impact: None,
            })));
            reply.outcome = Outcome::Dry;
            return Ok(reply);
        }
        let id = self.open_intent(
            IntentOp::Mv,
            &caller.branch,
            &tc.root,
            items.clone(),
            (false, false),
        );
        let now_ns = self.env.wall_ms.saturating_mul(1_000_000);
        let volume = self.files.fs.trees[&tc.root].volume.clone();
        let mut outcomes = Vec::new();
        for (from, to, dir, t) in &inverse {
            let o = match (from, t) {
                (Some(f), _) => match self.files.fs.apply(
                    &tc.root,
                    &TreeOp::Mv {
                        from: f.clone(),
                        to: to.clone(),
                    },
                    now_ns,
                ) {
                    Ok(()) => 1,
                    Err(e) => outcome_of(&e),
                },
                (None, Some(i)) => {
                    // Back from the trash: the file, or every file of a directory below its trash path.
                    let base = trash_path(&orig.tree, orig.id, *i);
                    let held: Vec<String> = self
                        .files
                        .fs
                        .elsewhere
                        .keys()
                        .filter(|(v, p)| {
                            *v == volume && (*p == base || p.starts_with(&format!("{base}/")))
                        })
                        .map(|(_, p)| p.clone())
                        .collect();
                    if held.is_empty() {
                        4
                    } else if self.files.fs.trees[&tc.root].disk_spelling(to).is_some() {
                        3
                    } else {
                        let mut o = 1;
                        for p in held {
                            let dst = format!("{to}{}", &p[base.len()..]);
                            if let Err(e) = self.files.fs.apply(
                                &tc.root,
                                &TreeOp::Mv { from: p, to: dst },
                                now_ns,
                            ) {
                                o = outcome_of(&e);
                            }
                        }
                        let _ = dir;
                        o
                    }
                }
                // Removed without `trash`: no bytes to restore ([API] open point 38).
                (None, None) => 4,
            };
            outcomes.push(o);
        }
        // The commit: moved nodes back, removed nodes present again.
        let (head, _) = self.head_blob(&tc.root);
        let mut repointed = Vec::new();
        let prev = self.dag.state_at(
            seq.and_then(|s| self.dag.commits[&s].parents.first().copied()),
            &self.alloc,
        );
        let after = self.dag.state_at(seq, &self.alloc);
        for (k, o) in inverse.iter().zip(&outcomes) {
            if *o != 1 {
                continue;
            }
            let (from, to, dir, _) = k;
            let hit =
                |p: &str, base: &str| p == base || (*dir && p.starts_with(&format!("{base}/")));
            let nodes: Vec<Nid> = c
                .st
                .nodes
                .iter()
                .filter(|(n, x)| {
                    x.live() && x.kind == "artifact" && after.nodes.get(n) != prev.nodes.get(n) && {
                        let f = crate::links::file_node(**n, x, 0);
                        match from {
                            Some(fr) => hit(&f.path, fr),
                            None => hit(&f.path, to),
                        }
                    }
                })
                .map(|(n, _)| *n)
                .collect();
            for n in nodes {
                let f = crate::links::file_node(n, &c.st.nodes[&n], 0);
                match from {
                    Some(fr) => {
                        let path = format!("{to}{}", &f.path[fr.len()..]);
                        let mut obs = self.observe(&tc, "project", &path, Some(&path));
                        obs.observed_blob = None;
                        obs.observed_git = head.clone();
                        obs.relink = Some("explicit/intent".into());
                        set_observation(&mut c.st, n, &obs);
                        add_alias(
                            &mut c.st,
                            n,
                            &PathVal {
                                root: "project".into(),
                                text: f.path.clone(),
                            },
                        );
                    }
                    None if f.status == FileStatus::Removed => {
                        self.remove_file(&mut c.st, n, "present", None, None);
                    }
                    None => {}
                }
                repointed.push(n);
            }
        }
        repointed.sort();
        repointed.dedup();
        let data = IntentData {
            intent: Some(format!("i-{id}")),
            items: items
                .iter()
                .zip(&outcomes)
                .map(|(i, o)| ItemOut {
                    src: i.src.clone(),
                    dst: i.dst.clone(),
                    outcome: outcome_name(*o),
                })
                .collect(),
            repointed,
            path_move: None,
            globs: Vec::new(),
            impact: None,
        };
        self.finish_intent(
            caller,
            ctx,
            key,
            payload,
            "FileRevert",
            "revert",
            id,
            &outcomes,
            c,
            tip,
            data,
            false,
        )
    }
}

/// The text of an item for a result row.
pub fn item_text(i: &ItemOut) -> (String, Option<String>) {
    (path_text(&i.src), i.dst.as_ref().map(path_text))
}
