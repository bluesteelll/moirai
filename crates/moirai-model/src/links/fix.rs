//! `LinksFix` ([API §12.5]; [40 §3.7]; [F18 §5.4], §5.5; [RULES/link-merge-rules] LV-003, LV-006, LV-007): the
//! explicit answers to a link's state — accept a proposal, record a manual path, confirm a guess, accept a replacement,
//! drop, unify, split, restore, and on anchors repin, pin and drop — and `prefix`, which confirms an inferred directory
//! move. Each is one commit, whose `relink` provenance is the caller's (`owner/*` for the owner, `agent/*` otherwise,
//! `confirmed/*` for a confirmation, WA-003, WA-004).

use crate::api::{Caller, Ctx, Reply, Store};
use crate::err::{Kv, Refusal, Res};
use crate::idem::Cj;
use crate::links::verbs::{Found, fix_yield, live_target, parse_spec, target_cj};
use crate::links::{
    FileCand, Landing, TreeCtx, add_alias, path_move, rewrite_globs, set_observation,
};
use crate::r4::anchor::{AState, Anchor, Content, Form, capture, resolve};
use crate::r4::cascade::FileResult;
use crate::r4::strings::{State as LState, confirm, evidence_token, score_text};
use crate::r4::uid::FileStatus;
use crate::state::{Aspect, EdgeKey, KState, KVal, Key, State};
use crate::status::Door;
use crate::tx::Target;
use crate::value::{MoveClass, Nid, PathVal, Uid, Value};
use std::collections::BTreeMap;

/// The arguments of `LinksFix` ([API §12.5]).
#[derive(Clone, Debug, PartialEq)]
pub struct FixArgs {
    /// `target`: a node or `aN`.
    pub target: String,
    /// `action`.
    pub action: String,
    /// `expect`.
    pub expect: Option<String>,
    /// `to`.
    pub to: Option<String>,
    /// `at`.
    pub at: Option<String>,
    /// `same_as`.
    pub same_as: Option<Target>,
    /// `reason`.
    pub reason: Option<String>,
    /// `replaced_by`.
    pub replaced_by: Option<String>,
    /// `from`.
    pub from: Option<String>,
}

/// What a `LinksFix` target names.
enum Tgt {
    /// A file node.
    File(Nid),
    /// An anchor: its source, its edge key.
    Anchor(Nid, EdgeKey),
}

const ACTIONS: [&str; 11] = [
    "accept",
    "to",
    "confirm",
    "accept-replacement",
    "drop",
    "same-as",
    "split",
    "repin",
    "pin",
    "restore",
    "prefix",
];

impl Store {
    /// The resolution of a file node of a state in the command's tree, a read ([40 §4.3]).
    pub fn resolve_node(
        &self,
        st: &State,
        tip: Option<u64>,
        tc: &TreeCtx,
        branch: &str,
        n: Nid,
    ) -> FileResult {
        let view = self.view_of(st, tip);
        let rt = self.files.runtime();
        let p = self.params(tc, branch, false, &view);
        let f = view
            .files
            .iter()
            .find(|f| f.n == n.0)
            .expect("a file node of the view");
        crate::r4::cascade::resolve_file(
            f,
            &view,
            &self.files.fs,
            &self.files.git,
            &rt,
            &tc.root,
            &p,
        )
    }

    /// The actor of the newest commit of the chain from `tip` that changed a node's observation composite: the acceptor
    /// of an `agent/*` value ([RULES/role-write-policy] WT-016).
    fn acceptor(&self, tip: Option<u64>, n: Nid) -> Option<String> {
        let k = Key::Node(n, Aspect::Observation);
        self.dag.chain(tip).into_iter().find_map(|c| {
            let x = &self.dag.commits[&c];
            x.changeset.contains_key(&k).then(|| x.actor.clone())
        })
    }

    /// The observation composite before the newest commit of the chain from `tip` that changed it (`--restore`'s
    /// re-bind to revert).
    fn previous_observation(&self, tip: Option<u64>, n: Nid) -> Option<Vec<Option<Value>>> {
        let k = Key::Node(n, Aspect::Observation);
        self.dag.chain(tip).into_iter().find_map(|c| {
            let x = &self.dag.commits[&c];
            match x.changeset.get(&k)? {
                (KState::Plain(Some(KVal::Observation(v))), _) => Some(v.clone()),
                (KState::Conflict(cf), _) => match cf.ours.as_ref().or(cf.theirs.as_ref()) {
                    Some(KVal::Observation(v)) => Some(v.clone()),
                    _ => None,
                },
                _ => None,
            }
        })
    }

    /// `path_claimed` ([F18 §2.1] I-F1; [F19 §10.2]): the key (root, path) already has a live `present` or `planned`
    /// holder on the candidate other than the nodes `except` (the ones the write moves).
    pub(crate) fn claimed(st: &State, except: &[Nid], root: &str, path: &str) -> Res<()> {
        for (m, x) in &st.nodes {
            if except.contains(m) || !x.live() || x.kind != "artifact" {
                continue;
            }
            let f = crate::links::file_node(*m, x, 0);
            if f.root == root && f.path == path && f.status != FileStatus::Removed && !f.path_claim
            {
                return Err(Refusal::new(
                    "path_claimed",
                    6,
                    format!("{root}:{path} is already held by {m}"),
                )
                .key("root", root)
                .key("path", path)
                .key("holder", Kv::Node(*m)));
            }
        }
        Ok(())
    }

    /// Re-points a file node to a present path of the tree: its observation with `relink`, the old path in `aliases`.
    fn rebind_to(
        &self,
        c: &mut FileCand,
        tc: &TreeCtx,
        n: Nid,
        rel: &str,
        disk: &str,
        relink: &str,
    ) -> Res<()> {
        let f = crate::links::file_node(n, &c.st.nodes[&n], 0);
        Self::claimed(&c.st, &[n], &f.root, rel)?;
        let o = self.repoint(tc, &f.root, rel, disk, relink.to_string());
        set_observation(&mut c.st, n, &o);
        add_alias(
            &mut c.st,
            n,
            &PathVal {
                root: f.root.clone(),
                text: f.path,
            },
        );
        Ok(())
    }

    /// A `LinksFix` target: a node (`#N`, `#u:`) that is a file node, or an anchor `aN` with its edge.
    fn fix_target(&self, st: &State, t: &str) -> Res<Tgt> {
        if let Some(h) = t.strip_prefix('a').and_then(|x| x.parse::<u64>().ok()) {
            let u = self
                .files
                .anchors
                .iter()
                .find(|(_, x)| **x == h)
                .map(|(u, _)| *u)
                .ok_or_else(|| Refusal::not_found("anchor", t))?;
            for (s, x) in &st.nodes {
                if let Some(k) = x.out.keys().find(|k| k.kind == "at" && k.disc == Some(u)) {
                    return Ok(Tgt::Anchor(*s, k.clone()));
                }
            }
            return Err(Refusal::not_found("anchor", t));
        }
        let node =
            crate::tx::parse_node(t).ok_or_else(|| Refusal::usage_arg("target", "a node or aN"))?;
        let n = live_target(self, st, &node)?;
        if st.nodes[&n].kind != "artifact" {
            return Err(Refusal::not_found("file node", t));
        }
        Ok(Tgt::File(n))
    }

    /// `LinksFix` ([API §12.5]; [40 §3.7]). Refusals: `repin_needs_at`, `confirm_refused`, `path_claimed` (exit 6); an
    /// `accept` whose re-evaluated top proposal differs from `expect`: E404, exit 6. Yields `{target, action, relink,
    /// commit}`.
    // spec: [API §12.5]; [40 §3.7]; [F18 §5.5]
    // rule: WV-037, WV-038, WV-039, WA-003, WA-004, WA-006, LV-003, LV-006, LV-007, TR-051, TR-052, TR-054
    pub fn links_fix(&mut self, a: &FixArgs, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("target".to_string(), Cj::Str(a.target.clone()));
        args.insert("action".to_string(), Cj::Str(a.action.clone()));
        for (k, v) in [
            ("expect", &a.expect),
            ("to", &a.to),
            ("at", &a.at),
            ("reason", &a.reason),
            ("replaced_by", &a.replaced_by),
            ("from", &a.from),
        ] {
            if let Some(v) = v {
                args.insert(k.to_string(), Cj::Str(v.clone()));
            }
        }
        if let Some(t) = &a.same_as {
            args.insert("same_as".to_string(), target_cj(self, t));
        }
        let payload = Self::file_payload("LinksFix", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        if !ACTIONS.contains(&a.action.as_str()) {
            return Err(Refusal::usage_arg(
                "action",
                format!("links fix takes {}, not {}", ACTIONS.join(", "), a.action),
            ));
        }
        let mut rights = self.rights(&caller, ctx);
        rights
            .verb(match a.action.as_str() {
                "confirm" => "links-fix-confirm",
                "prefix" => "links-fix-prefix",
                _ => "links-fix",
            })
            .map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        self.tree_mismatch(&caller, ctx)?;
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        let tc = self.tree_ctx(&caller, ctx);
        let need_tc = |what: &str| {
            tc.clone()
                .ok_or_else(|| Refusal::not_found("path", what.to_string()))
        };
        let algo = tc.as_ref().map_or(crate::value::Algo::Sha1, |t| {
            self.root_algo("project", &t.root)
        });
        let consts = crate::links::anchor_consts(&self.conf);
        let mut relink_out: Option<String> = None;
        let target_n: Nid;
        if a.action == "prefix" {
            // `--prefix FROM TO`: a `confirmed` entry and the glob rewrites; the target names nothing more.
            let tcx = need_tc("prefix")?;
            let (from, to) = match (&a.from, &a.to) {
                (Some(f), Some(t)) => (f.clone(), t.clone()),
                _ => return Err(Refusal::usage_arg("from", "prefix takes from and to")),
            };
            let norm = |d: &str| format!("{}/", d.trim_end_matches('/'));
            let (from, to) = (norm(&from), norm(&to));
            let (head, _) = self.git_head(&tcx.root);
            let hlc = self.hlc.peek_commit(self.env.wall_ms);
            c.add_path_move(
                &self.alloc.uidx,
                path_move(hlc, MoveClass::Confirmed, "project", &from, &to, head),
            )?;
            rewrite_globs(&mut c.st, &from, &to);
            target_n = c.root_node(&self.alloc.uidx, "project")?;
        } else {
            match self.fix_target(&c.st, &a.target)? {
                Tgt::File(n) => {
                    target_n = n;
                    // WV-037: the architect only on doc files.
                    if rights.role == "architect"
                        && crate::links::file_node(n, &c.st.nodes[&n], 0)
                            .artifact_kind
                            .as_deref()
                            != Some("doc")
                    {
                        return Err(crate::policy::refuse(
                            0,
                            "role-verbs",
                            "architect",
                            &format!("links fix {n}: not a doc file (WV-037)"),
                        ));
                    }
                    relink_out = self.fix_file(
                        &mut c,
                        &mut rights,
                        &caller,
                        a,
                        n,
                        tip,
                        tc.as_ref(),
                        algo,
                        &consts,
                    )?;
                }
                Tgt::Anchor(s, k) => {
                    target_n = k.dst;
                    self.fix_anchor(&mut c, &caller, a, s, &k, tip, tc.as_ref(), algo, &consts)?;
                }
            }
        }
        if let Some(r) = &relink_out {
            rights.value(1, &c.st, target_n, "relink", &Value::Text(r.clone()))?;
        }
        let seen: Vec<(Nid, Option<String>)> = match (&tc, a.action.as_str()) {
            (Some(_), "accept" | "to" | "accept-replacement" | "restore") => {
                let f = crate::links::file_node(target_n, &c.st.nodes[&target_n], 0);
                vec![(
                    target_n,
                    (f.status != FileStatus::Removed).then_some(f.path),
                )]
            }
            _ => Vec::new(),
        };
        let land = Landing {
            caller: &caller,
            ctx,
            key,
            payload,
            cmd: "LinksFix",
            origin: Self::named_origin(ctx),
            sym: "tx.links_fix".into(),
            yields: Vec::new(),
        };
        let (mut reply, seq) = self.land_file(land, c, tip, "", 0)?;
        if let Some(t) = &tc {
            self.observe_rows(ctx, &t.root, &seen);
        }
        reply.yields = vec![fix_yield(
            "tx.links_fix",
            target_n,
            &a.action,
            relink_out.as_deref(),
            seq,
        )];
        self.set_recorded_yields(&reply);
        Ok(reply)
    }

    /// The HEAD commit of a tree as an `oid`, with its hex.
    fn git_head(&self, root: &str) -> (Option<crate::value::Oid>, Option<String>) {
        let Some((repo, head)) = self.files.git.of_tree(root) else {
            return (None, None);
        };
        let h = repo.head_commit(head);
        (
            h.and_then(|h| crate::links::git_oid(repo.algo, h)),
            h.map(str::to_string),
        )
    }

    /// The node actions of `LinksFix`; returns the `relink` the commit records, if any.
    #[allow(clippy::too_many_arguments)]
    fn fix_file(
        &self,
        c: &mut FileCand,
        rights: &mut crate::policy::Rights,
        caller: &Caller,
        a: &FixArgs,
        n: Nid,
        tip: Option<u64>,
        tc: Option<&TreeCtx>,
        algo: crate::value::Algo,
        consts: &crate::r4::anchor::Consts,
    ) -> Res<Option<String>> {
        let need = |what: &str| {
            tc.cloned()
                .ok_or_else(|| Refusal::not_found("path", what.to_string()))
        };
        let f = crate::links::file_node(n, &c.st.nodes[&n], 0);
        match a.action.as_str() {
            // `--accept --expect PATH`: the top proposal re-evaluated now, only if it still equals PATH.
            "accept" => {
                let expect = a
                    .expect
                    .clone()
                    .ok_or_else(|| Refusal::usage_arg("expect", "accept needs expect"))?;
                let tcx = need(&expect)?;
                let rel = match self.find_path(&tcx, &expect, false)? {
                    Found::File { rel, .. } | Found::Dir { rel } | Found::Missing { rel } => rel,
                };
                let r = self.resolve_node(&c.st, tip, &tcx, &caller.branch, n);
                let (path, ev, score) = match r.proposals.first() {
                    Some(p) => (p.path.clone(), evidence_token(p.evidence), p.score),
                    None if r.state == LState::Ambiguous && r.candidates.contains(&rel) => {
                        (rel.clone(), evidence_token(23), None)
                    }
                    None => (String::new(), "", None),
                };
                if path != rel {
                    return Err(Refusal::lq(
                        "E404",
                        format!(
                            "{n}: the top proposal is {}, not {rel}",
                            if path.is_empty() {
                                "none"
                            } else {
                                path.as_str()
                            }
                        ),
                    ));
                }
                let disk = match self.find_path(&tcx, &path, false)? {
                    Found::File { disk, .. } => disk,
                    _ => return Err(Refusal::not_found("path", format!("project:{path}"))),
                };
                let relink = match score {
                    Some(s) => Self::manual_relink(caller, &format!("{ev}/{}", score_text(s))),
                    None => Self::manual_relink(caller, ev),
                };
                self.rebind_to(c, &tcx, n, &path, &disk, &relink)?;
                Ok(Some(relink))
            }
            // `--to PATH`: a manual path; PATH must exist in the caller's tree (LV-003 for a composite conflict).
            "to" => {
                let to =
                    a.to.clone()
                        .ok_or_else(|| Refusal::usage_arg("to", "to needs a path"))?;
                let tcx = need(&to)?;
                let (rel, disk) = match self.find_path(&tcx, &to, false)? {
                    Found::File { rel, disk } => (rel, disk),
                    Found::Dir { rel } | Found::Missing { rel } => {
                        return Err(Refusal::not_found("path", format!("project:{rel}")));
                    }
                };
                let relink = Self::manual_relink(caller, "manual");
                self.rebind_to(c, &tcx, n, &rel, &disk, &relink)?;
                Ok(Some(relink))
            }
            // `--confirm`: `agent/*` and `policy/*` become `confirmed/*`, by a role of `files.confirm-roles`, never
            // by the acceptor.
            "confirm" => {
                let refused = |case: &str, why: String| {
                    Refusal::new("confirm_refused", 6, why).key("case", case)
                };
                let Some(new) = confirm(f.relink.as_deref()) else {
                    return Err(refused(
                        "not-a-guess",
                        format!(
                            "{n}'s relink is {}; nothing to confirm",
                            f.relink.as_deref().unwrap_or("absent")
                        ),
                    ));
                };
                let acceptor = self.acceptor(tip, n);
                if acceptor.as_deref() == Some(caller.actor.as_str()) {
                    return Err(refused(
                        "same-actor",
                        format!("{} set this guess; another actor confirms it", caller.actor),
                    ));
                }
                if !crate::links::confirm_rights(&self.conf).contains(&rights.role) {
                    return Err(refused(
                        "role",
                        format!(
                            "role {} may not confirm (files.confirm-roles = {})",
                            rights.role,
                            crate::links::confirm_rights(&self.conf).join(",")
                        ),
                    ));
                }
                rights.acceptor = acceptor;
                let schema = c.st.schema.clone();
                let x = c.st.nodes.get_mut(&n).expect("the node");
                x.set_field(&schema, "relink", Some(Value::Text(new.clone())));
                Ok(Some(new))
            }
            // `--accept-replacement`: a new observation at the same path, `owner|agent/replacement`.
            "accept-replacement" => {
                let tcx = need(&f.path)?;
                let disk = match self.find_path(&tcx, &f.path, false)? {
                    Found::File { disk, .. } => disk,
                    _ => return Err(Refusal::not_found("path", format!("{}:{}", f.root, f.path))),
                };
                let relink = Self::manual_relink(caller, "replacement");
                let o = self.repoint(&tcx, &f.root, &f.path, &disk, relink.clone());
                set_observation(&mut c.st, n, &o);
                Ok(Some(relink))
            }
            // `--drop`: status `removed` with `reason` and `replaced_by` (LV-007 for a status fork).
            "drop" => {
                let q = match &a.replaced_by {
                    None => None,
                    Some(r) => Some(match crate::tx::parse_node(r) {
                        Some(t) => live_target(self, &c.st, &t)?,
                        None => {
                            let tcx = need(r)?;
                            match self.find_path(&tcx, r, false)? {
                                Found::File { rel, disk } => {
                                    self.register_file(
                                        c,
                                        &tcx,
                                        "project",
                                        &rel,
                                        Some(&disk),
                                        false,
                                        None,
                                    )?
                                    .0
                                }
                                Found::Dir { rel } | Found::Missing { rel } => {
                                    return Err(Refusal::not_found("path", rel));
                                }
                            }
                        }
                    }),
                };
                let from = f.status.name();
                crate::status::transition("artifact", from, "removed", Door::LinksFix)?;
                self.remove_file(&mut c.st, n, "removed", a.reason.as_deref(), q);
                c.st.nodes
                    .get_mut(&n)
                    .expect("the node")
                    .conflicts
                    .remove(&Aspect::Status);
                Ok(None)
            }
            // `--same-as B`: A becomes `removed{reason: same-as, replaced_by: B}`, its anchors re-pointed to B
            // (LV-006 for a path claim, which it resolves on both nodes).
            "same-as" => {
                let b = live_target(
                    self,
                    &c.st,
                    a.same_as
                        .as_ref()
                        .ok_or_else(|| Refusal::usage_arg("same_as", "same-as needs a node"))?,
                )?;
                if c.st.nodes[&b].kind != "artifact" || b == n {
                    return Err(Refusal::not_found("file node", b.to_string()));
                }
                crate::status::transition("artifact", f.status.name(), "removed", Door::LinksFix)?;
                self.remove_file(&mut c.st, n, "removed", Some("same-as"), Some(b));
                let schema = c.st.schema.clone();
                for m in [n, b] {
                    let x = c.st.nodes.get_mut(&m).expect("a node");
                    if x.conflicts
                        .get(&Aspect::Observation)
                        .is_some_and(|cf| cf.class == "PathClaim")
                    {
                        let prov = x.get(&schema, &Aspect::Observation);
                        x.conflicts.remove(&Aspect::Observation);
                        x.put_value(&schema, &Aspect::Observation, prov);
                    }
                }
                let tcx = tc.cloned().unwrap_or(TreeCtx {
                    root: String::new(),
                    eligible: false,
                    writer: false,
                    writer_tree: None,
                    cwd: String::new(),
                });
                if tcx.root.is_empty() {
                    repoint_all(&mut c.st, n, b);
                } else {
                    self.repoint_anchors(&mut c.st, n, b, &tcx, algo, consts, true);
                }
                Ok(None)
            }
            // `--split`: span anchors to the pieces that hold their quotes, whole-file anchors to the piece with the
            // largest share of the old content; the old node `removed{reason: split}`.
            "split" => {
                let tcx = need(&f.path)?;
                let r = self.resolve_node(&c.st, tip, &tcx, &caller.branch, n);
                // A split proposal (token 20) lists its pieces ([F20 §5.11.4] row 2).
                let pieces: Vec<String> = r
                    .proposals
                    .iter()
                    .find(|p| p.evidence == 20)
                    .map(|p| {
                        if p.pieces.is_empty() {
                            r.candidates.clone()
                        } else {
                            p.pieces.clone()
                        }
                    })
                    .unwrap_or_default();
                if pieces.is_empty() {
                    return Err(Refusal::lq("E404", format!("{n} resolves to no split")));
                }
                let t = &self.files.fs.trees[&tcx.root];
                let old_fp = f.oid.as_ref().and_then(|o| self.files.rt.fprint.get(o));
                let mut nodes = Vec::new();
                for p in &pieces {
                    let disk = t
                        .disk_spelling(p)
                        .ok_or_else(|| Refusal::not_found("path", format!("project:{p}")))?;
                    let (m, _) =
                        self.register_file(c, &tcx, "project", p, Some(&disk), false, None)?;
                    let share = old_fp
                        .and_then(|fp| {
                            t.read(&disk)
                                .ok()
                                .map(|b| crate::r4::text::estimates(fp, b).0)
                        })
                        .unwrap_or(crate::r4::text::Ratio::int(0));
                    nodes.push((m, disk, share));
                }
                crate::status::transition("artifact", f.status.name(), "removed", Door::LinksFix)?;
                // Whole-file anchors to the largest share (the first piece in path order on a tie).
                let whole = nodes
                    .iter()
                    .fold(
                        None::<&(Nid, String, crate::r4::text::Ratio)>,
                        |best, x| match best {
                            Some(b) if b.2 >= x.2 => Some(b),
                            _ => Some(x),
                        },
                    )
                    .map(|x| x.0)
                    .expect("a piece");
                let srcs: Vec<Nid> = c.st.nodes.keys().copied().collect();
                for s in srcs {
                    let keys: Vec<EdgeKey> = c.st.nodes[&s]
                        .out
                        .keys()
                        .filter(|k| k.kind == "at" && k.dst == n)
                        .cloned()
                        .collect();
                    for k in keys {
                        let props = c.st.nodes[&s].out[&k].clone();
                        let dst = match props.anchor.as_deref() {
                            Some(an) if an.kind != "file" => {
                                nodes.iter().find_map(|(m, disk, _)| {
                                    let b = t.read(disk).ok()?;
                                    let res = resolve(
                                        &Anchor::from_canon(k.disc.unwrap_or(Uid::ZERO), an),
                                        Content::Bytes(b),
                                        algo,
                                        consts,
                                    );
                                    matches!(res.state, AState::Fresh | AState::Moved).then_some(*m)
                                })
                            }
                            _ => Some(whole),
                        };
                        if let Some(d) = dst {
                            let x = c.st.nodes.get_mut(&s).expect("a source");
                            x.out.remove(&k);
                            x.out.insert(
                                EdgeKey {
                                    kind: "at".into(),
                                    dst: d,
                                    disc: k.disc,
                                },
                                props,
                            );
                        }
                    }
                }
                self.remove_file(&mut c.st, n, "removed", Some("split"), None);
                Ok(None)
            }
            // `--restore`: a removed node back to `present` (I-F14's explicit door; LV-007), else its last re-bind
            // reverted.
            "restore" => {
                if f.status == FileStatus::Removed {
                    crate::status::transition("artifact", "removed", "present", Door::LinksFix)?;
                    Self::claimed(&c.st, &[n], &f.root, &f.path)?;
                    self.remove_file(&mut c.st, n, "present", None, None);
                    c.st.nodes
                        .get_mut(&n)
                        .expect("the node")
                        .conflicts
                        .remove(&Aspect::Status);
                    return Ok(None);
                }
                let prev = self
                    .previous_observation(tip, n)
                    .ok_or_else(|| Refusal::lq("E404", format!("{n} has no re-bind to revert")))?;
                if let Some(Some(Value::Path(p))) = prev.first() {
                    Self::claimed(&c.st, &[n], &p.root, &p.text)?;
                }
                let schema = c.st.schema.clone();
                let x = c.st.nodes.get_mut(&n).expect("the node");
                x.conflicts.remove(&Aspect::Observation);
                x.put_value(&schema, &Aspect::Observation, Some(KVal::Observation(prev)));
                Ok(None)
            }
            other => Err(Refusal::usage_arg(
                "action",
                format!("{other} applies to an anchor (aN), not to a file node"),
            )),
        }
    }

    /// The anchor actions of `LinksFix`: `repin`, `pin` and `drop`.
    #[allow(clippy::too_many_arguments)]
    fn fix_anchor(
        &self,
        c: &mut FileCand,
        caller: &Caller,
        a: &FixArgs,
        s: Nid,
        k: &EdgeKey,
        tip: Option<u64>,
        tc: Option<&TreeCtx>,
        algo: crate::value::Algo,
        consts: &crate::r4::anchor::Consts,
    ) -> Res<()> {
        let props = c.st.nodes[&s].out[k].clone();
        let rec = props
            .anchor
            .as_deref()
            .map(|x| Anchor::from_canon(k.disc.unwrap_or(Uid::ZERO), x))
            .ok_or_else(|| Refusal::not_found("anchor", a.target.clone()))?;
        match a.action.as_str() {
            "drop" => {
                c.st.nodes.get_mut(&s).expect("the source").out.remove(k);
                Ok(())
            }
            "pin" => {
                let mut p = props.clone();
                let mut an = (*p.anchor.take().expect("an anchor")).clone();
                an.mode = "pinned".into();
                p.anchor = Some(Box::new(an));
                c.st.nodes
                    .get_mut(&s)
                    .expect("the source")
                    .out
                    .insert(k.clone(), p);
                Ok(())
            }
            "repin" => {
                let tcx = tc
                    .cloned()
                    .ok_or_else(|| Refusal::not_found("path", a.target.clone()))?;
                let f = crate::links::file_node(k.dst, &c.st.nodes[&k.dst], 0);
                let fr = self.resolve_node(&c.st, tip, &tcx, &caller.branch, k.dst);
                let at_path = fr.at.clone().unwrap_or(f.path.clone());
                let t = &self.files.fs.trees[&tcx.root];
                let content = t
                    .read(&at_path)
                    .map_err(|_| Refusal::not_found("path", format!("project:{at_path}")))?
                    .to_vec();
                let form = match &a.at {
                    Some(spec) => parse_spec(spec, None, None)?.form,
                    None => {
                        let r = resolve(&rec, Content::Bytes(&content), algo, consts);
                        match (r.state, r.span) {
                            (AState::Fresh | AState::Moved, Some((l, m))) => Form::Lines(l, m),
                            (AState::Fresh | AState::Moved, None) => Form::File,
                            (st, _) => {
                                let h = k
                                    .disc
                                    .and_then(|u| self.files.anchors.get(&u))
                                    .copied()
                                    .unwrap_or(0);
                                return Err(Refusal::new(
                                    "repin_needs_at",
                                    6,
                                    format!(
                                        "a{h} matched as {}, not exactly; --repin recaptures only an exact match",
                                        st.name()
                                    ),
                                )
                                .key("anchor", format!("a{h}"))
                                .key("state", st.name()));
                            }
                        }
                    }
                };
                let (src_uid, file_uid) = (c.st.nodes[&s].uid, c.st.nodes[&k.dst].uid);
                let (head, _) = self.git_head(&tcx.root);
                let watch = Some(rec.watch);
                let (mut new, _) = capture(
                    src_uid,
                    file_uid,
                    &form,
                    Some(&content),
                    watch,
                    head,
                    algo,
                    &[],
                    consts,
                )?;
                // A repin changes the selectors, never `captured`, `pred` or the uid ([40 §2.7]).
                new.uid = rec.uid;
                new.captured = rec.captured;
                new.pred = rec.pred;
                let mut p = props.clone();
                p.anchor = Some(Box::new(new.to_canon()));
                c.st.nodes
                    .get_mut(&s)
                    .expect("the source")
                    .out
                    .insert(k.clone(), p);
                Ok(())
            }
            other => Err(Refusal::usage_arg(
                "action",
                format!("{other} applies to a file node, not to an anchor"),
            )),
        }
    }
}

/// Re-points every anchor into `f` to `q`, keeping each anchor's uid and record (RK-006).
fn repoint_all(st: &mut State, f: Nid, q: Nid) {
    let srcs: Vec<Nid> = st.nodes.keys().copied().collect();
    for s in srcs {
        let keys: Vec<EdgeKey> = st.nodes[&s]
            .out
            .keys()
            .filter(|k| k.kind == "at" && k.dst == f)
            .cloned()
            .collect();
        for k in keys {
            let x = st.nodes.get_mut(&s).expect("a source");
            let p = x.out.remove(&k).expect("present");
            x.out.insert(
                EdgeKey {
                    kind: "at".into(),
                    dst: q,
                    disc: k.disc,
                },
                p,
            );
        }
    }
}
