//! `LinksSync` and `Complete`'s link settle ([API §12.6], §10.5 step 4; [40 §3.7], §4.2, §5.3; [F18 §2.6] I-F6;
//! [AR §5e.3]): a settle point resolves its scope against the command's tree ([`crate::r4::settle::settle`]) and writes,
//! in one commit, every exact re-bind the write rule allows (writer tree, freshness, the quiescence re-check — which a
//! simulated tree, constant within a command, always passes — and on `main` the committed-only rule), the
//! `planned → present` bindings, the conflicts settled by observation (LV-001, LV-004, LV-005), the automatic
//! `path_moves` entries with their glob rewrites (`committed` only), and under `files.deletion-inference =
//! main-tree-commits` the removals a writer tree of `main` sees committed; the runtime rows (`FILEOBS`, `PENDING`,
//! `FPRINT`, `PREFIXEV`, `DIRMAP` and the `TREES` epoch) follow. A reader tree writes `PENDING` rows only.

use crate::api::{Caller, Ctx, Data, Reply, Store};
use crate::err::{Refusal, Res};
use crate::idem::Cj;
use crate::links::verbs::live_target;
use crate::links::{Landing, TreeCtx, add_alias, path_move, rewrite_globs, set_observation};
use crate::r4::cascade::EpochKind;
use crate::r4::settle::{Rebind, settle};
use crate::r4::strings::State as LState;
use crate::r4::uid::FileStatus;
use crate::state::{Aspect, EdgeKey, KVal, State};
use crate::status::Door;
use crate::tx::{Target, Yield};
use crate::value::{MoveClass, Nid, Oid, PathVal, Value};
use std::collections::{BTreeMap, BTreeSet};

/// What a settle wrote, for its yields ([API §12.6]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncOut {
    /// `rebound`: (file, from, to, evidence token).
    pub rebound: Vec<(Nid, PathVal, PathVal, String)>,
    /// `pending`: the `PENDING` rows written.
    pub pending: usize,
    /// `states`: the file states of the scope, by name.
    pub states: BTreeMap<String, u64>,
    /// `commit`.
    pub commit: Option<u64>,
}

/// The yield row of `LinksSync` ([API §12.6]): `rebound` and `states` as their canonical JSON text ([API §5.6]).
pub fn sync_yield(o: &SyncOut) -> Yield {
    let rebound = Cj::Arr(
        o.rebound
            .iter()
            .map(|(n, from, to, ev)| {
                Cj::Obj(vec![
                    ("file".into(), Cj::Str(n.to_string())),
                    ("from".into(), Cj::Str(crate::links::path_text(from))),
                    ("to".into(), Cj::Str(crate::links::path_text(to))),
                    ("evidence".into(), Cj::Str(ev.clone())),
                ])
            })
            .collect(),
    );
    let states = Cj::Map(
        o.states
            .iter()
            .map(|(k, v)| (k.clone(), Cj::Int(*v as i64)))
            .collect(),
    );
    Yield {
        index: 0,
        proc: "tx.links_sync".into(),
        rows: vec![vec![
            ("rebound".to_string(), rebound.text()),
            ("pending".into(), o.pending.to_string()),
            ("states".into(), states.text()),
            (
                "commit".into(),
                o.commit.map_or("null".to_string(), |s| format!("s{s}")),
            ),
        ]],
    }
}

/// The observation a re-bind of a settle writes ([40 §4.3] "What settle writes").
fn rebind_obs(
    root: &str,
    rb: &Rebind,
    algo_git: Option<crate::value::Algo>,
) -> crate::links::Observation {
    let git = |h: &Option<String>| {
        h.as_ref()
            .and_then(|x| algo_git.and_then(|a| crate::links::git_oid(a, x)))
    };
    crate::links::Observation {
        path: PathVal {
            root: root.to_string(),
            text: rb.to.clone(),
        },
        oid: rb.oid.clone(),
        bytes: Some(rb.bytes),
        observed_git: git(&rb.observed_git),
        observed_blob: git(&rb.observed_blob),
        relink: (!rb.relink.is_empty()).then(|| rb.relink.clone()),
    }
}

impl Store {
    /// The file nodes a settle covers: with `scope`, those the scope node's subtree links through `at` edges; with
    /// `all`, every live file node of the view; otherwise every live file node some live node links.
    fn settle_scope(&self, st: &State, scope: Option<Nid>, all: bool) -> Vec<Nid> {
        let files = |x: &crate::state::Node| -> Vec<Nid> {
            x.out
                .keys()
                .filter(|k| k.kind == "at")
                .map(|k| k.dst)
                .collect()
        };
        let mut out: BTreeSet<Nid> = BTreeSet::new();
        match scope {
            Some(root) => {
                let mut sub: BTreeSet<Nid> = BTreeSet::from([root]);
                loop {
                    let more: Vec<Nid> = st
                        .nodes
                        .iter()
                        .filter(|(n, x)| {
                            x.live()
                                && !sub.contains(n)
                                && x.parent.is_some_and(|p| sub.contains(&p))
                        })
                        .map(|(n, _)| *n)
                        .collect();
                    if more.is_empty() {
                        break;
                    }
                    sub.extend(more);
                }
                for n in &sub {
                    out.extend(files(&st.nodes[n]));
                }
            }
            None if all => {
                out.extend(
                    st.nodes
                        .iter()
                        .filter(|(_, x)| x.live() && x.kind == "artifact")
                        .map(|(n, _)| *n),
                );
            }
            None => {
                for x in st.nodes.values().filter(|x| x.live()) {
                    out.extend(files(x));
                }
            }
        }
        out.into_iter()
            .filter(|n| st.nodes.get(n).is_some_and(|x| x.kind == "artifact"))
            .collect()
    }

    /// A settle point over `scope` in the command's tree ([API §12.6]; [40 §4.2]): the resolution, the candidate with
    /// what the write rule allows, the runtime rows. Returns the candidate, the summary and the runtime updates to
    /// apply once the commit lands (they are applied by [`Store::land_settle`]).
    fn settle_cand(
        &self,
        caller: &Caller,
        tc: &TreeCtx,
        scope: &[Nid],
        epoch: EpochKind,
    ) -> Res<(
        crate::links::FileCand,
        SyncOut,
        crate::r4::settle::SettleOut,
        EpochKind,
    )> {
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(caller, tip);
        let view = self.view_of(&c.st, tip);
        let rt = self.files.runtime();
        let p = self.params(tc, &caller.branch, true, &view);
        let ns: Vec<u32> = scope.iter().map(|n| n.0).collect();
        let hlc = self.hlc.peek_commit(self.env.wall_ms);
        let mut out = settle(
            &ns,
            &view,
            &self.files.fs,
            &self.files.git,
            &rt,
            &tc.root,
            &p,
            hlc,
        );
        // I-F1 at write time ([F18 §2.1]): a settle writes no two re-binds or bindings to one key, and none to a key a
        // node it does not move holds; such writes are dropped — the conservative answer (the nodes stay as they
        // resolved), never a silent pair.
        {
            let root_of = |n: u32| {
                view.files
                    .iter()
                    .find(|f| f.n == n)
                    .map_or_else(|| "project".to_string(), |f| f.root.clone())
            };
            let writes: Vec<(u32, String)> = out
                .rebinds
                .iter()
                .chain(&out.resolved)
                .map(|r| (r.n, r.to.clone()))
                .chain(out.binds.iter().map(|b| (b.n, b.path.clone())))
                .collect();
            let moving: BTreeSet<u32> = writes.iter().map(|(n, _)| *n).collect();
            let mut count: BTreeMap<(String, String), usize> = BTreeMap::new();
            for (n, to) in &writes {
                *count.entry((root_of(*n), to.clone())).or_insert(0) += 1;
            }
            let held: BTreeSet<(String, String)> = view
                .files
                .iter()
                .filter(|f| {
                    !f.tombstone && !moving.contains(&f.n) && f.status != FileStatus::Removed
                })
                .map(|f| (f.root.clone(), f.path.clone()))
                .collect();
            let ok = |n: u32, to: &str| {
                let k = (root_of(n), to.to_string());
                count.get(&k) == Some(&1) && !held.contains(&k)
            };
            out.rebinds.retain(|r| ok(r.n, &r.to));
            out.resolved.retain(|r| ok(r.n, &r.to));
            out.binds.retain(|b| ok(b.n, &b.path));
        }
        let mut s = SyncOut::default();
        // `states`: each link's file state after the settle — `ok` for a node this settle re-bound, bound or resolved,
        // else the state it resolved to.
        let written: BTreeSet<u32> = out
            .rebinds
            .iter()
            .chain(&out.resolved)
            .map(|r| r.n)
            .chain(out.binds.iter().map(|b| b.n))
            .collect();
        for (n, r) in &out.results {
            let name = if written.contains(n) {
                LState::Ok.name()
            } else {
                r.state.name()
            };
            *s.states.entry(name.to_string()).or_insert(0) += 1;
        }
        s.pending = out.pending.len();
        let algo_git = self.files.git.of_tree(&tc.root).map(|(r, _)| r.algo);
        let root_of = |st: &State, n: u32| {
            st.nodes
                .get(&Nid(n))
                .and_then(|x| x.text("root"))
                .unwrap_or("project")
                .to_string()
        };
        // Exact re-binds (I-F6), and the observation-resolved composite conflicts (LV-001).
        for rb in out.rebinds.iter().chain(&out.resolved) {
            let n = Nid(rb.n);
            let root = root_of(&c.st, rb.n);
            set_observation(&mut c.st, n, &rebind_obs(&root, rb, algo_git));
            add_alias(
                &mut c.st,
                n,
                &PathVal {
                    root: root.clone(),
                    text: rb.from.clone(),
                },
            );
            let ev = rb.relink.split('/').nth(1).unwrap_or("").to_string();
            s.rebound.push((
                n,
                PathVal {
                    root: root.clone(),
                    text: rb.from.clone(),
                },
                PathVal {
                    root,
                    text: rb.to.clone(),
                },
                ev,
            ));
        }
        // `planned → present` (TR-049, its guard TG-009 decided by the resolver's binding rule, [F20 §5.18]).
        for b in &out.binds {
            let n = Nid(b.n);
            crate::status::transition("artifact", "planned", "present", Door::Settle)?;
            let root = root_of(&c.st, b.n);
            let rb = Rebind {
                n: b.n,
                from: b.path.clone(),
                to: b.path.clone(),
                relink: String::new(),
                oid: b.oid.clone(),
                bytes: b.bytes,
                observed_git: b.observed_git.clone(),
                observed_blob: b.observed_blob.clone(),
            };
            set_observation(&mut c.st, n, &rebind_obs(&root, &rb, algo_git));
            c.st.nodes.get_mut(&n).expect("a planned node").status = "present".into();
        }
        // LV-005: a path claim unified by an exact rename in one commit.
        for u in &out.unified {
            let (removed, kept) = (Nid(u.removed), Nid(u.kept));
            // The removed node's own status: a `planned` node may claim the path too (PC-003).
            let from =
                c.st.nodes
                    .get(&removed)
                    .map_or_else(|| "present".to_string(), |x| x.status.clone());
            crate::status::transition("artifact", &from, "removed", Door::Settle)?;
            self.remove_file(&mut c.st, removed, "removed", Some("same-as"), Some(kept));
            let schema = c.st.schema.clone();
            for m in [removed, kept] {
                let x = c.st.nodes.get_mut(&m).expect("a claimant");
                if x.conflicts
                    .get(&Aspect::Observation)
                    .is_some_and(|cf| cf.class == "PathClaim")
                {
                    let prov = x.get(&schema, &Aspect::Observation);
                    x.conflicts.remove(&Aspect::Observation);
                    x.put_value(&schema, &Aspect::Observation, prov);
                }
            }
            let x = c.st.nodes.get_mut(&kept).expect("the kept node");
            x.set_field(&schema, "relink", Some(Value::Text(u.relink.clone())));
            let srcs: Vec<Nid> = c.st.nodes.keys().copied().collect();
            for s2 in srcs {
                let keys: Vec<EdgeKey> = c.st.nodes[&s2]
                    .out
                    .keys()
                    .filter(|k| k.kind == "at" && k.dst == removed)
                    .cloned()
                    .collect();
                for k in keys {
                    let y = c.st.nodes.get_mut(&s2).expect("a source");
                    let pr = y.out.remove(&k).expect("present");
                    y.out.insert(
                        EdgeKey {
                            kind: "at".into(),
                            dst: kept,
                            disc: k.disc,
                        },
                        pr,
                    );
                }
            }
        }
        // LV-004: an anchor conflict keeps the side that resolves `fresh`.
        for (f, anchor, side) in &out.anchors {
            for x in c.st.nodes.values_mut() {
                let key = x
                    .out
                    .keys()
                    .find(|k| k.kind == "at" && k.dst == Nid(*f) && k.disc == Some(*anchor))
                    .cloned();
                let Some(k) = key else { continue };
                let a = Aspect::Edge(k.clone());
                if let Some(cf) = x.conflicts.remove(&a) {
                    let v = if *side == 0 { cf.ours } else { cf.theirs };
                    if let Some(KVal::Edge(p)) = v {
                        x.out.insert(k, p);
                    }
                }
            }
        }
        // Automatic `path_moves` entries ([F20 §5.16]): `committed` ones rewrite globs, `observed` ones never do.
        for m in &out.moves {
            let git: Option<Oid> = m
                .git
                .as_ref()
                .and_then(|g| algo_git.and_then(|a| crate::links::git_oid(a, g)));
            let class = if m.committed {
                MoveClass::Committed
            } else {
                MoveClass::Observed
            };
            c.add_path_move(
                &self.alloc.uidx,
                path_move(hlc, class, "project", &m.from, &m.to, git),
            )?;
            if m.committed {
                rewrite_globs(&mut c.st, &m.from, &m.to);
            }
        }
        // `files.deletion-inference = main-tree-commits` (I-F7, TR-053): a writer tree of `main` records a deletion
        // committed in its HEAD.
        if caller.branch == "main" && tc.writer && crate::links::deletion_inference(&self.conf) {
            for (n, r) in &out.results {
                if r.state == LState::Missing && r.details.iter().any(|d| d.code == 43) {
                    let n = Nid(*n);
                    if c.st.nodes[&n].status == "present" {
                        crate::status::transition(
                            "artifact",
                            "present",
                            "removed",
                            Door::DeletionInference,
                        )?;
                        self.remove_file(&mut c.st, n, "removed", None, None);
                    }
                }
            }
        }
        Ok((c, s, out, epoch))
    }

    /// Applies a settle's runtime rows ([40 §2.6]; [F11 §12.4]–§12.11): `FILEOBS`, `PENDING` (a row per (node, tree,
    /// from, to), replaced), `FPRINT`, `PREFIXEV`, `DIRMAP`, and the `TREES` row's newest epoch of the scope's kind.
    fn apply_settle_rows(
        &mut self,
        tree: &str,
        out: crate::r4::settle::SettleOut,
        epoch: EpochKind,
    ) {
        let hlc = self.hlc.peek(self.env.wall_ms);
        let rt = &mut self.files.rt;
        for (k, row) in out.fileobs {
            rt.fileobs.insert(k, row);
        }
        for p in out.pending {
            rt.pending
                .retain(|q| !(q.n == p.n && q.tree == p.tree && q.from == p.from && q.to == p.to));
            rt.pending.push(p);
        }
        for (o, fp) in out.fprint {
            rt.fprint.insert(o, fp);
        }
        for (k, ns) in out.prefixev {
            rt.prefixev.insert(k, ns);
        }
        for (k, row) in out.dirmap {
            rt.dirmap.insert(k, row);
        }
        let t = rt.trees.entry(tree.to_string()).or_default();
        t.first_settle_done = true;
        t.last_settle_hlc = hlc;
        t.epochs
            .retain(|(k, _)| std::mem::discriminant(k) != std::mem::discriminant(&epoch));
        t.epochs.push((epoch, hlc));
    }

    /// `LinksSync` ([API §12.6]; [40 §3.7]): a settle point, keyed by an explicit key only ([API §7.1]); `deep` is
    /// refused in quiet mode unless `force`. With no simulated tree the settle has nothing to read and writes nothing
    /// ([F18 §4.8] `files: no tree bound`). Yields `{rebound, pending, states, commit}`.
    // spec: [API §12.6]; [40 §3.7]; [F18 §2.6] I-F6
    // rule: WV-036, LV-001, LV-004, LV-005, TR-049, TR-053, TR-081, TG-009
    pub fn links_sync(
        &mut self,
        scope: Option<&Target>,
        budget_ms: Option<u64>,
        since: Option<&str>,
        (deep, all, force): (bool, bool, bool),
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        if let Some(t) = scope {
            args.insert("scope".to_string(), crate::links::verbs::target_cj(self, t));
        }
        if let Some(b) = budget_ms {
            args.insert("budget_ms".to_string(), Cj::Int(b as i64));
        }
        if let Some(s) = since {
            args.insert("since".to_string(), Cj::Str(s.to_string()));
        }
        for (k, v) in [("deep", deep), ("all", all), ("force", force)] {
            if v {
                args.insert(k.to_string(), Cj::Bool(true));
            }
        }
        let payload = Self::file_payload("LinksSync", args);
        // Explicit key only: a settle must run again when the tree changed ([API §7.1], open point 10).
        let key = ctx
            .key
            .as_ref()
            .map(|k| (crate::idem::explicit_key(k), false));
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("links-sync")
            .map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        if deep && self.quiet && !force {
            return Err(Refusal::new(
                "quiet_mode",
                6,
                "links sync --deep is refused in quiet mode; pass force",
            ));
        }
        self.tree_mismatch(&caller, ctx)?;
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let Some(tc) = self.tree_ctx(&caller, ctx) else {
            let mut reply = Reply::ok(Data::None);
            reply.branch = Some(caller.branch.clone());
            reply.rev = Some(tip.unwrap_or(0));
            reply.commit = tip;
            reply.key = ctx.key.clone();
            reply.warnings = caller.warnings.clone();
            reply.yields = vec![sync_yield(&SyncOut::default())];
            return Ok(reply);
        };
        let st = self.dag.state_at(tip, &self.alloc);
        let scope_n = scope.map(|t| live_target(self, &st, t)).transpose()?;
        let mut ns = self.settle_scope(&st, scope_n, all);
        // `since`: the file nodes whose keys a commit after that revision changed on the branch.
        if let Some(rev) = since {
            let from = self
                .dag
                .rev_commit(rev, &self.rev_ctx(&caller))?
                .ok_or_else(|| Refusal::lq("E301", format!("{rev} names no commit")))?;
            let old = self.dag.ancestors(Some(from));
            let touched: BTreeSet<Nid> = self
                .dag
                .chain(tip)
                .into_iter()
                .filter(|c| !old.contains(c))
                .flat_map(|c| crate::state::touched(&self.dag.commits[&c].changeset))
                .collect();
            ns.retain(|n| touched.contains(n));
        }
        let epoch = if scope.is_some() || since.is_some() {
            EpochKind::Partial
        } else {
            EpochKind::Full
        };
        let (reply, _) =
            self.land_settle(&caller, ctx, &tc, &ns, epoch, key, payload, "LinksSync")?;
        Ok(reply)
    }

    /// Lands a settle: the commit when the candidate changed anything, then the runtime rows; the yields carry the
    /// summary with its commit.
    #[allow(clippy::too_many_arguments)]
    pub fn land_settle(
        &mut self,
        caller: &Caller,
        ctx: &Ctx,
        tc: &TreeCtx,
        scope: &[Nid],
        epoch: EpochKind,
        key: Option<([u8; 16], bool)>,
        payload: [u8; 16],
        cmd: &'static str,
    ) -> Res<(Reply, Option<u64>)> {
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let (c, mut summary, out, epoch) = self.settle_cand(caller, tc, scope, epoch)?;
        let land = Landing {
            caller,
            ctx,
            key,
            payload,
            cmd,
            origin: Self::named_origin(ctx),
            sym: "tx.links_sync".into(),
            yields: Vec::new(),
        };
        let (mut reply, seq) = self.land_file(land, c, tip, "", 0)?;
        if !ctx.dry {
            self.apply_settle_rows(&tc.root, out, epoch);
        }
        summary.commit = seq;
        reply.yields = vec![sync_yield(&summary)];
        self.set_recorded_yields(&reply);
        Ok((reply, seq))
    }

    /// `Complete`'s link settle ([API §10.5] step 4; [LQ/std §7.3] `tx.complete`; [AR §6.2]; [72 M11]): when the
    /// branch of a block that completed tasks has a designated simulated tree and the completed subtrees link files,
    /// the settle follows as a separate commit of the same command, as its caller, over those file nodes; its commit
    /// is every completion's `settle_commit`. `caller` is the command's resolved caller (its lease has just settled).
    // spec: [API §10.5] step 4
    pub fn complete_settle(&mut self, caller: &Caller, reply: &mut Reply, ctx: &Ctx) -> Res<()> {
        if ctx.dry || reply.rev_new.is_none() {
            return Ok(());
        }
        let tasks: Vec<Nid> = reply
            .yields
            .iter()
            .filter(|y| y.proc == "tx.complete")
            .flat_map(|y| &y.rows)
            .filter_map(|r| r.iter().find(|(k, _)| k == "task"))
            .filter_map(|(_, v)| v.strip_prefix('#').and_then(|x| x.parse::<u32>().ok()))
            .map(Nid)
            .collect();
        if tasks.is_empty() {
            return Ok(());
        }
        let branch = caller.branch.clone();
        let Some(tree) = self
            .designation()
            .into_iter()
            .find(|p| p.branch == branch)
            .map(|p| p.tree)
            .filter(|t| self.files.fs.trees.contains_key(t))
        else {
            return Ok(());
        };
        let tip = self.dag.live(&branch).and_then(|r| r.tip);
        let st = self.dag.state_at(tip, &self.alloc);
        let mut ns: BTreeSet<Nid> = BTreeSet::new();
        for t in &tasks {
            ns.extend(self.settle_scope(&st, Some(*t), false));
        }
        if ns.is_empty() {
            return Ok(());
        }
        let mut c = caller.clone();
        c.tree = Some(tree.clone());
        let d = self.designation();
        let tc = TreeCtx {
            root: tree.clone(),
            eligible: true,
            writer: crate::r4::settle::writer_tree(&branch, &tree, &d, &self.files.git),
            writer_tree: Some(tree.clone()),
            cwd: tree,
        };
        let sctx = Ctx {
            key: None,
            ..ctx.clone()
        };
        let ns: Vec<Nid> = ns.into_iter().collect();
        let (_, seq) = self.land_settle(
            &c,
            &sctx,
            &tc,
            &ns,
            EpochKind::Partial,
            None,
            [0; 16],
            "Complete",
        )?;
        for y in reply.yields.iter_mut().filter(|y| y.proc == "tx.complete") {
            for row in &mut y.rows {
                if let Some(slot) = row.iter_mut().find(|(k, _)| k == "settle_commit") {
                    slot.1 = seq.map_or("null".to_string(), |s| format!("s{s}"));
                }
            }
        }
        let rows = reply.yields.clone();
        if let Some(e) = self
            .idem
            .entries
            .values_mut()
            .find(|e| e.commit.map(|c| c.0) == reply.rev_new)
        {
            e.result.yields = rows;
        }
        Ok(())
    }
}

/// The status of a file node in a state.
pub fn status_of(st: &State, n: Nid) -> Option<FileStatus> {
    st.nodes
        .get(&n)
        .and_then(|x| FileStatus::from_name(&x.status))
}
