//! `FileAdd`, `LinkFile`, `UnlinkFile`, `FileRelink` and `Check` ([API §12.2], §12.3, §12.5, §12.7; [40 §3.2],
//! §3.3, §3.6; [F08 §11]): registration of file nodes by their derived uids, anchor capture onto `at` edges, the
//! removal of anchors, a move recorded after the fact, and the ancestry check of a pinned git commit.

use crate::api::{Caller, Ctx, Data, Reply, Store};
use crate::err::{Kv, Refusal, Res};
use crate::idem::Cj;
use crate::links::{
    FileCand, Observation, TreeCtx, add_alias, git_hex, git_oid, path_text, set_observation,
};
use crate::policy::Scopes;
use crate::r4::anchor::{Form, Watch, capture};
use crate::r4::path::{Os, PathError, cli_path, portable_issues};
use crate::r4::tree::{basename, dirname};
use crate::r4::uid::{CaptureId, EdgeAnchor, FileStatus, Registration, ViewNode, register};
use crate::state::{EdgeKey, EdgeProps, State};
use crate::tx::{Target, Yield};
use crate::value::{Algo, Nid, Oid, PathVal, Uid, Value};
use std::collections::{BTreeMap, BTreeSet};

/// A path argument located in a tree ([API §12.1] "Path arguments"; [40 §2.7] capture step 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    /// A file: the stored spelling (git's HEAD spelling when tracked, else the enumerated one) and the on-disk path.
    File {
        /// The stored path.
        rel: String,
        /// The on-disk path.
        disk: String,
    },
    /// A directory.
    Dir {
        /// The stored path.
        rel: String,
    },
    /// Nothing at the path.
    Missing {
        /// The path as written, root-relative.
        rel: String,
    },
}

/// The data of `Check` ([API §12.7]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckData {
    /// `node`.
    pub node: Nid,
    /// `commit`: the node's pinned git commit (`sha1:<hex>`).
    pub commit: Option<String>,
    /// `tip`: the bound worktree's HEAD commit.
    pub tip: Option<String>,
    /// `verdict`: `ancestor`, `not-ancestor` or `unknown`.
    pub verdict: &'static str,
}

/// `bad_path` of a path argument ([F19 §10.2]): the rule a refusal names.
pub fn bad_path(arg: &str, e: PathError) -> Refusal {
    let rule = match e {
        PathError::DriveRelative => "drive-relative",
        PathError::DevicePath => "device",
        PathError::Backslash | PathError::Control | PathError::NotUtf8 => "P4",
        _ => "P1",
    };
    Refusal::new(
        "bad_path",
        2,
        format!("{arg} is refused: {} ({rule})", e.name()),
    )
    .key("path", arg)
    .key("rule", rule)
}

/// An anchor spec as `LinkFile` reads it ([40 §2.7] authoring forms; [F20 §6.1]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    /// The path text.
    pub path: String,
    /// The form.
    pub form: Form,
    /// The commit prefix of a `path@<commit>:L-M` form.
    pub pin: Option<String>,
    /// The end text of a quoted range ([LQ/std §7.4] `$end`).
    pub end: Option<Vec<u8>>,
}

/// Parses an anchor spec, with `quote` (and `end`) given as literal text for the spec's path ([LQ/std §7.4] `$quote`,
/// `$end`).
pub fn parse_spec(spec: &str, quote: Option<&str>, end: Option<&str>) -> Res<Spec> {
    let mk = |path: &str, form: Form, pin: Option<String>| Spec {
        path: path.to_string(),
        form,
        pin,
        end: None,
    };
    if let Some(q) = quote {
        return Ok(Spec {
            end: end.map(|e| e.as_bytes().to_vec()),
            ..mk(spec, Form::QuoteText(q.as_bytes().to_vec()), None)
        });
    }
    if end.is_some() {
        return Err(Refusal::usage_arg("end", "end goes with quote"));
    }
    if let Some((p, sym)) = spec.split_once("::") {
        return Ok(mk(p, Form::Symbol(sym.to_string()), None));
    }
    if let Some((p, h)) = spec.split_once('#') {
        return Ok(mk(p, Form::Heading(h.to_string()), None));
    }
    let lines = |t: &str| -> Option<(u32, u32)> {
        let (a, b) = t.split_once('-').unwrap_or((t, t));
        let digits = |x: &str| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit());
        (digits(a) && digits(b)).then(|| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))?
    };
    if let Some(at) = spec.rfind('@')
        && let Some((commit, range)) = spec[at + 1..].split_once(':')
        && let Some((l, m)) = lines(range)
        && !commit.is_empty()
    {
        return Ok(mk(&spec[..at], Form::Lines(l, m), Some(commit.to_string())));
    }
    if let Some(colon) = spec.rfind(':')
        && let Some((l, m)) = lines(&spec[colon + 1..])
    {
        return Ok(mk(&spec[..colon], Form::Lines(l, m), None));
    }
    Ok(mk(spec, Form::File, None))
}

/// The view's nodes as registration reads them ([F08 §11.2]).
pub fn view_nodes(st: &State) -> Vec<ViewNode> {
    st.nodes
        .values()
        .map(|x| {
            if !x.live() {
                ViewNode::Tomb { uid: x.uid }
            } else if x.kind == "artifact" {
                let f = crate::links::file_node(Nid(0), x, 0);
                ViewNode::File {
                    uid: x.uid,
                    root: f.root,
                    path: f.path,
                    status: f.status,
                    aliases: f.aliases,
                }
            } else {
                ViewNode::Other { uid: x.uid }
            }
        })
        .collect()
}

/// The anchors of the `at` edges (s, f) of a state, as capture's identity steps read them ([F08 §11.4]).
pub fn edge_anchors(st: &State, s: Nid, f: Nid) -> Vec<EdgeAnchor> {
    st.nodes.get(&s).map_or(Vec::new(), |x| {
        x.out
            .iter()
            .filter(|(k, _)| k.kind == "at" && k.dst == f)
            .filter_map(|(k, p)| {
                let a = p.anchor.as_deref()?;
                Some(crate::r4::anchor::Anchor::from_canon(k.disc?, a).edge_anchor())
            })
            .collect()
    })
}

/// The `#N` of a live node a target names on a state (`not_found`, what `node`, exit 3; E111 for an id never
/// allocated).
pub fn live_target(st: &Store, view: &State, t: &Target) -> Res<Nid> {
    let n = match t {
        Target::Id(n) => *n,
        Target::Uid(u) => *st
            .alloc
            .uidx
            .get(u)
            .ok_or_else(|| Refusal::lq("E111", format!("{u} is not a node of this store")))?,
        Target::Var(v) => return Err(Refusal::usage(format!("${v} names no node here"))),
    };
    if n.0 == 0 || n.0 >= st.next_id {
        return Err(Refusal::lq("E111", format!("{n} was never allocated")));
    }
    if view.live(n).is_none() {
        return Err(Refusal::not_found("node", n.to_string()));
    }
    Ok(n)
}

/// A target's text for a payload ([API §7.3]: a node id replaced by its uid).
pub fn target_cj(st: &Store, t: &Target) -> Cj {
    Cj::Str(match t {
        Target::Id(n) => st
            .alloc
            .uids
            .get(n)
            .map_or_else(|| n.to_string(), |u| format!("#u:{}", u.hex())),
        Target::Uid(u) => format!("#u:{}", u.hex()),
        Target::Var(v) => format!("${v}"),
    })
}

/// Whether an absolute path lies in a session scratchpad: a `scratchpad` segment whose third ancestor segment is
/// `claude` (Claude Code's `…/claude/<project>/<session>/scratchpad/…` layout; the chapters name no other).
pub fn in_scratchpad(abs: &str) -> bool {
    let segs: Vec<&str> = abs.split('/').collect();
    segs.iter()
        .enumerate()
        .any(|(i, s)| *s == "scratchpad" && i >= 3 && segs[i - 3] == "claude")
}

impl Store {
    /// A path argument located in the command's tree ([API §12.1]; [OS/path §7] `cli_path` against the canonical
    /// current directory inside the tree; [40 §2.7] step 1): the on-disk entry under the directory's equivalence, its
    /// stored spelling (git's HEAD spelling of a tracked path, else the enumerated one), and, with `basename`, a token
    /// without `/` found nowhere at the root searched as a basename — unique, it is expanded; several are
    /// `ambiguous_path` (exit 2).
    // spec: [API §12.1] path arguments; [OS/path §7]; [40 §2.7] capture step 1; [F19 §10.2] ambiguous_path
    pub fn find_path(&self, tc: &TreeCtx, arg: &str, basename_search: bool) -> Res<Found> {
        let t = &self.files.fs.trees[&tc.root];
        let windows = t.os == Os::Windows;
        let rel = cli_path(windows, &tc.root, &tc.cwd, arg).map_err(|e| bad_path(arg, e))?;
        if rel.is_empty() {
            return Err(bad_path(arg, PathError::Empty));
        }
        if let Some(disk) = t.disk_spelling(&rel) {
            let stored = self.git_spelling(&tc.root, &disk);
            if t.files.contains_key(&disk) {
                return Ok(Found::File { rel: stored, disk });
            }
            return Ok(Found::Dir { rel: stored });
        }
        if basename_search && !rel.contains('/') {
            let matches: Vec<String> = t
                .files
                .keys()
                .filter(|p| basename(p) == rel)
                .cloned()
                .collect();
            match matches.len() {
                0 => {}
                1 => {
                    let disk = matches[0].clone();
                    return Ok(Found::File {
                        rel: self.git_spelling(&tc.root, &disk),
                        disk,
                    });
                }
                n => {
                    return Err(Refusal::new(
                        "ambiguous_path",
                        2,
                        format!("{arg} matches {n} files in {}", tc.root),
                    )
                    .key(
                        "matches",
                        Kv::List(
                            matches
                                .iter()
                                .take(10)
                                .map(|m| Kv::Str(m.clone()))
                                .collect(),
                        ),
                    ));
                }
            }
        }
        Ok(Found::Missing { rel })
    }

    /// The stored spelling of an on-disk path ([40 §2.4]; [OS/path §3] P2): the path as the tree's HEAD tree spells it
    /// when git tracks it (a lookup under the directories' equivalence), else the enumerated spelling.
    fn git_spelling(&self, root: &str, disk: &str) -> String {
        let t = &self.files.fs.trees[root];
        let Some((repo, head)) = self.files.git.of_tree(root) else {
            return disk.to_string();
        };
        let Some(h) = repo.head_commit(head) else {
            return disk.to_string();
        };
        let tau = repo.tau(h);
        if tau.contains_key(disk) {
            return disk.to_string();
        }
        tau.keys()
            .find(|p| {
                p.split('/').count() == disk.split('/').count()
                    && p.split('/')
                        .zip(disk.split('/'))
                        .enumerate()
                        .all(|(i, (a, b))| {
                            let dir: Vec<&str> = disk.split('/').take(i).collect();
                            let (ci, ni) = t.dir_equivalence(&dir.join("/"));
                            crate::r4::tree::equiv(a, b, ci, ni)
                        })
            })
            .cloned()
            .unwrap_or_else(|| disk.to_string())
    }

    /// The tree's HEAD commit and git's blob at a path in τ(HEAD), as `oid` values.
    fn head_and_blob(&self, root: &str, rel: &str) -> (Option<Oid>, Option<Oid>) {
        let Some((repo, head)) = self.files.git.of_tree(root) else {
            return (None, None);
        };
        let Some(h) = repo.head_commit(head) else {
            return (None, None);
        };
        (
            git_oid(repo.algo, h),
            repo.tau(h).get(rel).and_then(|b| git_oid(repo.algo, b)),
        )
    }

    /// The first observation of a file at a path of the command's tree ([40 §2.2]; [40 §3.2] `--planned`): `oid` over
    /// the content (absent beyond `files.max-read-bytes`), the raw size, `observed_git` = HEAD and `observed_blob` =
    /// git's blob at the path in τ(HEAD); a planned node records only the planning tree's HEAD.
    pub fn observe(&self, tc: &TreeCtx, root: &str, rel: &str, disk: Option<&str>) -> Observation {
        let (head, blob) = self.head_and_blob(&tc.root, rel);
        let algo = self.root_algo(root, &tc.root);
        let t = &self.files.fs.trees[&tc.root];
        // The size from the entry; the content only through a read, which a denied or cloud-only entry refuses
        // ([F18 §2.11] I-F11), and only up to `files.max-read-bytes`.
        let size = disk.and_then(|d| t.files.get(d)).map(|f| f.bytes.len());
        let content = disk
            .and_then(|d| t.read(d).ok())
            .filter(|b| crate::links::content_available(&self.conf, b.len()));
        Observation {
            path: PathVal {
                root: root.to_string(),
                text: rel.to_string(),
            },
            oid: content.map(|b| crate::r4::text::oid(algo, b)),
            bytes: size.map(|b| b as u64),
            observed_git: head,
            observed_blob: if disk.is_some() { blob } else { None },
            relink: None,
        }
    }

    /// Registration of the file at (root, path) on the candidate ([F08 §11.2]; [40 §2.3]; I-F1, I-F2, I-F14): the view's
    /// live node that holds the key, or a new file node with its derived uid (its predecessor and the dead-uid rule),
    /// `#N` reuse, its identity fields, its first observation, and the root node on the root's first file. Returns the
    /// node and whether it was created.
    // spec: [F08 §11.2]; [API §12.2]; [API §12.3]
    #[allow(clippy::too_many_arguments)]
    pub fn register_file(
        &self,
        c: &mut FileCand,
        tc: &TreeCtx,
        root: &str,
        rel: &str,
        disk: Option<&str>,
        planned: bool,
        kind: Option<&str>,
    ) -> Res<(Nid, bool)> {
        let view = view_nodes(&c.st);
        match register(&view, root, rel)? {
            Registration::Existing(u) => {
                let n =
                    c.st.nodes
                        .iter()
                        .find(|(_, x)| x.uid == u)
                        .map(|(n, _)| *n)
                        .expect("the view holds the registered node");
                Ok((n, false))
            }
            Registration::New { uid, pred } => {
                let pred_n =
                    pred.and_then(|p| c.st.nodes.iter().find(|(_, x)| x.uid == p).map(|(n, _)| *n));
                // The root node before the file nodes it is created with ([API §9.6] item 2; [F08 §11.3]).
                c.root_node(&self.alloc.uidx, root)?;
                let n = c.create(&self.alloc.uidx, uid, "artifact")?;
                let schema = c.st.schema.clone();
                let path = PathVal {
                    root: root.to_string(),
                    text: rel.to_string(),
                };
                {
                    let x = c.st.nodes.get_mut(&n).expect("created");
                    x.set_field(&schema, "root", Some(Value::Text(root.to_string())));
                    x.set_field(&schema, "origin_path", Some(Value::Path(path)));
                    x.set_field(&schema, "origin_pred", pred_n.map(Value::Ref));
                    x.set_field(
                        &schema,
                        "artifact_kind",
                        kind.map(|k| Value::Enum(k.to_string())),
                    );
                    if planned {
                        x.status = "planned".into();
                    }
                }
                let o = self.observe(tc, root, rel, disk);
                set_observation(&mut c.st, n, &o);
                Ok((n, true))
            }
        }
    }

    /// The warnings of a path some supported OS cannot hold ([OS/path §8.2] P5; `files.portable-names`: `link` and
    /// `file add` warn under either value): `nonportable_name` when a segment has an issue among its siblings.
    fn portable_warning(&self, tc: &TreeCtx, rel: &str) -> bool {
        let t = &self.files.fs.trees[&tc.root];
        let mut dir = String::new();
        rel.split('/').any(|seg| {
            let sibs = t.entries(&dir);
            let sibs: Vec<&str> = sibs.iter().map(String::as_str).collect();
            let bad = !portable_issues(seg, &sibs).is_empty();
            if !dir.is_empty() {
                dir.push('/');
            }
            dir.push_str(seg);
            bad
        })
    }

    /// The refusals of a link target the policy keys exclude ([40 §9.2] decisions 2 and 14): a path in a session
    /// scratchpad under `files.scratchpads = refuse`, a path under a cloud sync root under `files.cloud = refuse`
    /// (`bad_path`, rules `scratchpad` and `cloud-root`: [F19 §10.2] has no code of their own).
    fn target_policy(&self, tc: &TreeCtx, rel: &str) -> Res<()> {
        let abs = format!("{}/{rel}", tc.root.trim_end_matches('/'));
        if in_scratchpad(&abs) && !crate::links::scratchpad_policy(&self.conf) {
            return Err(Refusal::new(
                "bad_path",
                2,
                format!(
                    "{abs} is refused: it is in a session scratchpad (files.scratchpads = refuse)"
                ),
            )
            .key("path", abs)
            .key("rule", "scratchpad"));
        }
        if self.files.fs.trees[&tc.root].cloud_root && crate::links::cloud_policy(&self.conf) {
            return Err(Refusal::new(
                "bad_path",
                2,
                format!("{abs} is refused: it is under a cloud sync root (files.cloud = refuse)"),
            )
            .key("path", abs)
            .key("rule", "cloud-root"));
        }
        Ok(())
    }

    /// The command's tree, or `not_found` (`path`) when CX-5 names no simulated tree: a file command has nothing to
    /// read ([F18 §4.8] `files: no tree bound`).
    pub fn need_tree(&self, caller: &Caller, ctx: &Ctx, what: &str) -> Res<TreeCtx> {
        self.tree_ctx(caller, ctx)
            .ok_or_else(|| Refusal::not_found("path", what.to_string()))
    }

    /// `FileAdd` ([API §12.2]; [40 §3.3]): each path registered in the tree — a live file node at the key is reported,
    /// not duplicated — in one commit with `stmt_origin` `file-verb`, `stmt_sym` `add`. A directory registers with
    /// `artifact_kind` `dir`. A path not in the tree is `not_found` (`path`), exit 3.
    // spec: [API §12.2]; [40 §3.3]
    // rule: WV-029, WR-013
    pub fn file_add(
        &mut self,
        paths: &[String],
        kind: Option<&str>,
        root: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert(
            "paths".to_string(),
            Cj::Arr(paths.iter().map(|p| Cj::Str(p.clone())).collect()),
        );
        if let Some(k) = kind {
            args.insert("kind".to_string(), Cj::Str(k.to_string()));
        }
        if let Some(r) = root.filter(|r| *r != "project") {
            args.insert("root".to_string(), Cj::Str(r.to_string()));
        }
        let payload = Self::file_payload("FileAdd", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        self.rights(&caller, ctx)
            .verb("file-add")
            .map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        if paths.is_empty() {
            return Err(Refusal::usage_arg("paths", "file add needs a path"));
        }
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        if let Some(k) = kind
            && c.st.schema.value("artifact", "artifact_kind", k).is_none()
        {
            return Err(Refusal::lq("E102", format!("{k} is not an artifact_kind")));
        }
        let root_name = root.unwrap_or("project").to_string();
        let tc = match root_name.as_str() {
            "project" => self.need_tree(&caller, ctx, &paths[0])?,
            "abs" => {
                return Err(Refusal::usage_arg(
                    "root",
                    "an abs path is registered by the verb that names it (lane, run), not by file add",
                ));
            }
            name => {
                // A named root: its directory from `roots.<name>`, inside a simulated tree.
                let dir = crate::links::root_dir(&self.conf, name)
                    .ok_or_else(|| Refusal::not_found("path", format!("{name}:")))?;
                let tree = self
                    .files
                    .tree_of(&dir)
                    .ok_or_else(|| Refusal::not_found("directory", dir.clone()))?;
                let mut tc = self.need_tree(&caller, ctx, &paths[0]).unwrap_or(TreeCtx {
                    root: tree.clone(),
                    eligible: true,
                    writer: false,
                    writer_tree: None,
                    cwd: dir.clone(),
                });
                tc.root = tree;
                tc.cwd = dir;
                tc
            }
        };
        if !tc.eligible {
            self.tree_mismatch(&caller, ctx)?;
        }
        let base = if root_name == "project" {
            String::new()
        } else {
            let d = crate::links::root_dir(&self.conf, &root_name).unwrap_or_default();
            d[tc.root.trim_end_matches('/').len()..]
                .trim_start_matches('/')
                .to_string()
        };
        let mut files = Vec::new();
        let mut seen = Vec::new();
        let mut warn = false;
        for p in paths {
            let found = self.find_path(&tc, p, true)?;
            let (rel, disk, dir) = match found {
                Found::File { rel, disk } => (rel, Some(disk), false),
                Found::Dir { rel } => (rel, None, true),
                Found::Missing { rel } => return Err(Refusal::not_found("path", rel)),
            };
            self.target_policy(&tc, &rel)?;
            warn |= self.portable_warning(&tc, &rel);
            let stored = if base.is_empty() {
                rel.clone()
            } else {
                rel.strip_prefix(&format!("{base}/"))
                    .ok_or_else(|| Refusal::not_found("path", format!("{root_name}:{rel}")))?
                    .to_string()
            };
            let k = if dir {
                Some(kind.unwrap_or("dir"))
            } else {
                kind
            };
            let (n, created) =
                self.register_file(&mut c, &tc, &root_name, &stored, disk.as_deref(), false, k)?;
            seen.push((n, disk.clone()));
            files.push((
                PathVal {
                    root: root_name.clone(),
                    text: stored,
                },
                n,
                created,
            ));
        }
        let rows: Vec<Vec<(String, String)>> = files
            .iter()
            .map(|(p, n, created)| {
                vec![
                    ("path".to_string(), path_text(p)),
                    ("id".into(), n.to_string()),
                    ("created".into(), created.to_string()),
                ]
            })
            .collect();
        let land = crate::links::Landing {
            caller: &caller,
            ctx,
            key,
            payload,
            cmd: "FileAdd",
            origin: "file-verb",
            sym: "add".into(),
            yields: vec![Yield {
                index: 0,
                proc: "FileAdd".into(),
                rows,
            }],
        };
        let (mut reply, seq) = self.land_file(land, c, tip, "", 0)?;
        self.observe_rows(ctx, &tc.root, &seen);
        reply.yields.clear();
        reply.ready.clear();
        reply.other.clear();
        if warn {
            reply.warnings.push("nonportable_name".into());
        }
        let _ = seq;
        reply.data = Data::FileAdd(files);
        Ok(reply)
    }

    /// `LinkFile` ([API §12.3]; [40 §3.2], §2.7; [F20 §6.1]): capture of each spec on the caller's tree, then one commit
    /// with the file nodes (created or reused), the root node on the root's first link, and the `at` edge with one
    /// anchor per spec, `aN` allocated in spec order (a capture whose selectors equal an existing anchor's reuses it,
    /// `created` false). A planned link (`planned`, a missing path, a `file` anchor) records the planning tree's HEAD
    /// in `observed_git`. Yields one row per anchor.
    // spec: [API §12.3]; [40 §3.2]; [F20 §6.1]; [F08 §11.4]
    // rule: WV-034, WR-013
    #[allow(clippy::too_many_arguments)]
    pub fn link_file(
        &mut self,
        node: &Target,
        specs: &[String],
        watch: Option<&str>,
        planned: bool,
        (quote, end): (Option<&str>, Option<&str>),
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("node".to_string(), target_cj(self, node));
        args.insert(
            "specs".to_string(),
            Cj::Arr(specs.iter().map(|p| Cj::Str(p.clone())).collect()),
        );
        for (k, v) in [("watch", watch), ("quote", quote), ("end", end)] {
            if let Some(v) = v {
                args.insert(k.to_string(), Cj::Str(v.to_string()));
            }
        }
        if planned {
            args.insert("planned".to_string(), Cj::Bool(true));
        }
        let payload = Self::file_payload("LinkFile", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        let rights = self.rights(&caller, ctx);
        rights.verb("link-at").map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        if specs.is_empty() {
            return Err(Refusal::usage_arg("specs", "link needs an anchor spec"));
        }
        let watch = match watch {
            None => None,
            Some("header") => Some(Watch::Header),
            Some("span") => Some(Watch::Span),
            Some(w) => {
                return Err(Refusal::usage_arg(
                    "watch",
                    format!("watch takes header or span, not {w}"),
                ));
            }
        };
        let tc = self.need_tree(&caller, ctx, &specs[0])?;
        if !tc.eligible {
            self.tree_mismatch(&caller, ctx)?;
        }
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        let src = live_target(self, &c.st, node)?;
        let created = BTreeSet::new();
        if !rights.may_write(
            &Scopes {
                st: &c.st,
                created: &created,
            },
            src,
        ) {
            return Err(crate::policy::refuse(
                0,
                "role-verbs",
                &rights.role,
                &format!("link {src} (WV-034: may-write)"),
            ));
        }
        let algo = self.root_algo("project", &tc.root);
        let consts = crate::links::anchor_consts(&self.conf);
        let (head, _) = self.head_and_blob(&tc.root, "");
        let mut rows = Vec::new();
        let mut fresh: Vec<Uid> = Vec::new();
        let mut seen = Vec::new();
        let mut warn = false;
        for spec in specs {
            let Spec {
                path: ptext,
                mut form,
                pin,
                end: end_text,
            } = parse_spec(spec, quote, end)?;
            let found = self.find_path(&tc, &ptext, true)?;
            let (rel, disk) = match found {
                Found::File { rel, disk } => (rel, Some(disk)),
                Found::Dir { rel } if form == Form::File => (rel, None),
                Found::Dir { rel } => {
                    return Err(Refusal::new(
                        "anchor_spec",
                        2,
                        format!("{rel} is a directory; a span anchor needs text"),
                    )
                    .key("case", "binary"));
                }
                Found::Missing { rel } if planned && form == Form::File => (rel, None),
                Found::Missing { rel } => return Err(Refusal::not_found("path", rel)),
            };
            self.target_policy(&tc, &rel)?;
            warn |= self.portable_warning(&tc, &rel);
            let t = &self.files.fs.trees[&tc.root];
            let mut content: Option<Vec<u8>> = match &disk {
                Some(d) if t.files.contains_key(d) => match t.read(d) {
                    Ok(b) if crate::links::content_available(&self.conf, b.len()) => {
                        Some(b.to_vec())
                    }
                    _ if form == Form::File => None,
                    _ => {
                        return Err(Refusal::new(
                            "anchor_spec",
                            2,
                            format!("{rel}: the content cannot be read here"),
                        )
                        .key("case", "not-recordable"));
                    }
                },
                _ => None,
            };
            // `path@<commit>:L-M`: the commit's content, from the tree's git history.
            if let Some(commit) = &pin {
                let (repo, _) = self
                    .files
                    .git
                    .of_tree(&tc.root)
                    .ok_or_else(|| Refusal::not_found("path", format!("{rel}@{commit}")))?;
                let full = repo
                    .commits
                    .keys()
                    .filter(|k| k.starts_with(&git_hex(commit)))
                    .collect::<Vec<_>>();
                let [full] = full.as_slice() else {
                    return Err(Refusal::not_found("path", format!("{rel}@{commit}")));
                };
                let bytes = repo
                    .tau(full)
                    .get(&rel)
                    .and_then(|b| repo.blobs.get(b))
                    .cloned()
                    .ok_or_else(|| {
                        Refusal::new("anchor_spec", 2, format!("{rel}@{commit}: no content"))
                            .key("case", "not-found")
                    })?;
                let oid = git_oid(repo.algo, full).expect("a commit id of the repository's format");
                if let Form::Lines(l, m) = form {
                    form = Form::Pinned(oid, l, m);
                }
                content = Some(bytes);
            }
            // A quoted range: the text from the quote to the end text's first occurrence after it.
            if let (Form::QuoteText(qs), Some(es)) = (&form, &end_text) {
                let (qs, es) = (qs.as_slice(), es.as_slice());
                let b = content.as_deref().unwrap_or(&[]);
                let norm = crate::r4::text::atext(b).unwrap_or_default();
                let find = |hay: &[u8], needle: &[u8], from: usize| {
                    (from..=hay.len().saturating_sub(needle.len())).find(|&i| {
                        i + needle.len() <= hay.len() && &hay[i..i + needle.len()] == needle
                    })
                };
                let span = find(&norm, qs, 0).and_then(|o| {
                    find(&norm, es, o + qs.len()).map(|e| norm[o..e + es.len()].to_vec())
                });
                form = Form::QuoteText(span.ok_or_else(|| {
                    Refusal::new("anchor_spec", 2, "the quoted range is not in the file")
                        .key("case", "not-found")
                })?);
            }
            let (f, _) = self.register_file(
                &mut c,
                &tc,
                "project",
                &rel,
                disk.as_deref(),
                planned && disk.is_none(),
                None,
            )?;
            if disk.is_some() {
                seen.push((f, disk.clone()));
            }
            let (src_uid, file_uid) = (c.st.nodes[&src].uid, c.st.nodes[&f].uid);
            let on = edge_anchors(&c.st, src, f);
            let (a, id) = capture(
                src_uid,
                file_uid,
                &form,
                content.as_deref(),
                watch,
                head.clone(),
                algo,
                &on,
                &consts,
            )?;
            let created = matches!(id, CaptureId::New { .. });
            if created {
                c.st.nodes.get_mut(&src).expect("the source").out.insert(
                    EdgeKey {
                        kind: "at".into(),
                        dst: f,
                        disc: Some(a.uid),
                    },
                    EdgeProps {
                        anchor: Some(Box::new(a.to_canon())),
                        ..EdgeProps::default()
                    },
                );
            }
            let handle = match self.files.anchors.get(&a.uid) {
                Some(h) => *h,
                None => {
                    if !fresh.contains(&a.uid) {
                        fresh.push(a.uid);
                    }
                    self.next_anchor
                        + fresh.iter().position(|u| *u == a.uid).expect("pushed") as u64
                }
            };
            rows.push(vec![
                ("file".to_string(), f.to_string()),
                (
                    "path".into(),
                    path_text(&PathVal {
                        root: "project".into(),
                        text: rel.clone(),
                    }),
                ),
                ("anchor".into(), format!("a{handle}")),
                ("kind".into(), a.kind.name().to_string()),
                ("created".into(), created.to_string()),
            ]);
        }
        let land = crate::links::Landing {
            caller: &caller,
            ctx,
            key,
            payload,
            cmd: "LinkFile",
            origin: Self::named_origin(ctx),
            sym: "tx.link_file".into(),
            yields: vec![Yield {
                index: 0,
                proc: "tx.link_file".into(),
                rows,
            }],
        };
        let (mut reply, seq) = self.land_file(land, c, tip, "", 0)?;
        self.observe_rows(ctx, &tc.root, &seen);
        if seq.is_some() {
            for u in fresh {
                self.files.anchors.entry(u).or_insert_with(|| {
                    let h = self.next_anchor;
                    self.next_anchor += 1;
                    h
                });
            }
        }
        if warn {
            reply.warnings.push("nonportable_name".into());
        }
        Ok(reply)
    }

    /// The role of the commit that added an `at` edge key on the first-parent chain of `tip` (WV-035 "own-role").
    fn edge_role(&self, tip: Option<u64>, src: Nid, k: &EdgeKey) -> Option<String> {
        let key = crate::state::Key::Node(src, crate::state::Aspect::Edge(k.clone()));
        self.dag.chain(tip).into_iter().find_map(|c| {
            let x = &self.dag.commits[&c];
            x.changeset
                .get(&key)
                .filter(|(before, _)| *before == crate::state::KState::ABSENT)
                .map(|_| x.role.clone())
        })
    }

    /// `UnlinkFile` ([API §12.3]; [40 §3.2]): one commit removing the anchor `aN`, or every anchor of the node into the
    /// file at `path`; the edge goes with its last anchor ([F18 §2.3]). File nodes without referrers are kept. An
    /// unknown anchor is `not_found` (`anchor`), exit 3. Yields one row per anchor removed (`created` false).
    // spec: [API §12.3]; [40 §3.2]
    // rule: WV-035
    pub fn unlink_file(
        &mut self,
        node: &Target,
        anchor: Option<&str>,
        path: Option<&str>,
        ctx: &Ctx,
    ) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("node".to_string(), target_cj(self, node));
        for (k, v) in [("anchor", anchor), ("path", path)] {
            if let Some(v) = v {
                args.insert(k.to_string(), Cj::Str(v.to_string()));
            }
        }
        let payload = Self::file_payload("UnlinkFile", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        let rights = self.rights(&caller, ctx);
        rights.verb("unlink-at").map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        self.tree_mismatch(&caller, ctx)?;
        if anchor.is_some() == path.is_some() {
            return Err(Refusal::usage(
                "unlink takes exactly one of anchor and path",
            ));
        }
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        let src = live_target(self, &c.st, node)?;
        let none = BTreeSet::new();
        if !rights.may_write(
            &Scopes {
                st: &c.st,
                created: &none,
            },
            src,
        ) {
            return Err(crate::policy::refuse(
                0,
                "role-verbs",
                &rights.role,
                &format!("unlink {src} (WV-035: may-write)"),
            ));
        }
        let keys: Vec<EdgeKey> = match (anchor, path) {
            (Some(a), _) => {
                let h: u64 = a
                    .strip_prefix('a')
                    .and_then(|x| x.parse().ok())
                    .ok_or_else(|| Refusal::not_found("anchor", a))?;
                let u = self
                    .files
                    .anchors
                    .iter()
                    .find(|(_, x)| **x == h)
                    .map(|(u, _)| *u)
                    .ok_or_else(|| Refusal::not_found("anchor", a))?;
                let k = c.st.nodes[&src]
                    .out
                    .keys()
                    .find(|k| k.kind == "at" && k.disc == Some(u))
                    .cloned()
                    .ok_or_else(|| Refusal::not_found("anchor", a))?;
                vec![k]
            }
            (None, Some(p)) => {
                let tc = self.need_tree(&caller, ctx, p)?;
                let rel = match self.find_path(&tc, p, false)? {
                    Found::File { rel, .. } | Found::Dir { rel } | Found::Missing { rel } => rel,
                };
                let files: BTreeSet<Nid> = c
                    .st
                    .nodes
                    .iter()
                    .filter(|(_, x)| {
                        x.kind == "artifact" && crate::links::file_node(Nid(0), x, 0).path == rel
                    })
                    .map(|(n, _)| *n)
                    .collect();
                let ks: Vec<EdgeKey> = c.st.nodes[&src]
                    .out
                    .keys()
                    .filter(|k| k.kind == "at" && files.contains(&k.dst))
                    .cloned()
                    .collect();
                if ks.is_empty() {
                    return Err(Refusal::not_found("anchor", format!("{src} at {rel}")));
                }
                ks
            }
            (None, None) => unreachable!("exactly one was checked"),
        };
        // WV-035: the anchor is own-role unless R is the orchestrator or the owner.
        if !matches!(rights.role.as_str(), "orchestrator" | "owner") {
            for k in &keys {
                if self.edge_role(tip, src, k).as_deref() != Some(rights.role.as_str()) {
                    return Err(crate::policy::refuse(
                        0,
                        "role-verbs",
                        &rights.role,
                        &format!("unlink another role's anchor of {src} (WV-035)"),
                    ));
                }
            }
        }
        let mut rows = Vec::new();
        for k in &keys {
            let props =
                c.st.nodes
                    .get_mut(&src)
                    .expect("the source")
                    .out
                    .remove(k)
                    .expect("the key is present");
            let kind = props
                .anchor
                .as_deref()
                .map_or("file".to_string(), |a| a.kind.clone());
            let path =
                c.st.nodes
                    .get(&k.dst)
                    .and_then(|x| match x.fields.get("path") {
                        Some(Value::Path(p)) => Some(path_text(p)),
                        _ => None,
                    })
                    .unwrap_or_default();
            let h = k
                .disc
                .and_then(|u| self.files.anchors.get(&u))
                .copied()
                .unwrap_or(0);
            rows.push(vec![
                ("file".to_string(), k.dst.to_string()),
                ("path".into(), path),
                ("anchor".into(), format!("a{h}")),
                ("kind".into(), kind),
                ("created".into(), "false".into()),
            ]);
        }
        let land = crate::links::Landing {
            caller: &caller,
            ctx,
            key,
            payload,
            cmd: "UnlinkFile",
            origin: Self::named_origin(ctx),
            sym: "tx.unlink_file".into(),
            yields: vec![Yield {
                index: 0,
                proc: "tx.unlink_file".into(),
                rows,
            }],
        };
        Ok(self.land_file(land, c, tip, "", 0)?.0)
    }

    /// The file node a `from` argument names: a node id, else the live file node whose path (or, failing that, one of
    /// whose aliases) is the path in the command's tree.
    pub fn file_of(&self, c: &FileCand, tc: Option<&TreeCtx>, from: &str) -> Res<Nid> {
        if let Some(t) = crate::tx::parse_node(from) {
            let n = live_target(self, &c.st, &t)?;
            if c.st.nodes[&n].kind != "artifact" {
                return Err(Refusal::not_found("file node", from));
            }
            return Ok(n);
        }
        let tc = tc.ok_or_else(|| Refusal::not_found("file node", from))?;
        let rel = match self.find_path(tc, from, false)? {
            Found::File { rel, .. } | Found::Dir { rel } | Found::Missing { rel } => rel,
        };
        let files: Vec<(Nid, crate::r4::cascade::FileNode)> =
            c.st.nodes
                .iter()
                .filter(|(_, x)| x.live() && x.kind == "artifact")
                .map(|(n, x)| (*n, crate::links::file_node(*n, x, 0)))
                .collect();
        files
            .iter()
            .find(|(_, f)| f.path == rel && f.status != FileStatus::Removed)
            .or_else(|| files.iter().find(|(_, f)| f.aliases.contains(&rel)))
            .map(|(n, _)| *n)
            .ok_or_else(|| Refusal::not_found("file node", format!("project:{rel}")))
    }

    /// The `relink` value a manual or accepted re-bind records by the caller's role ([F18 §5.4]; [RULES/role-write-policy]
    /// WA-003, WA-006): `owner/<evidence>` for the owner, `agent/<evidence>` for every other role.
    pub fn manual_relink(caller: &Caller, evidence: &str) -> String {
        if caller.role == "owner" {
            format!("owner/{evidence}")
        } else {
            format!("agent/{evidence}")
        }
    }

    /// The observation of a file node re-pointed to a present path of the command's tree ([40 §4.3] "What settle
    /// writes"): the content's `oid` and size, `observed_git` = HEAD, `observed_blob` = git's blob at the path in
    /// τ(HEAD), with `relink`.
    pub fn repoint(
        &self,
        tc: &TreeCtx,
        root: &str,
        rel: &str,
        disk: &str,
        relink: String,
    ) -> Observation {
        let mut o = self.observe(tc, root, rel, Some(disk));
        o.relink = Some(relink);
        o
    }

    /// `FileRelink` ([API §12.5]; [40 §3.6] `file relink --after`): `to` must exist in the tree; one commit re-pointing
    /// the file node there, the old path added to `aliases`, provenance `owner/manual` or `agent/manual` by the caller's
    /// role. Yields `{target, action, relink, commit}` with `action` `relink`.
    // spec: [API §12.5]; [40 §3.6]
    // rule: WV-033, WA-003, WA-006
    pub fn file_relink(&mut self, from: &str, to: &str, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve_keyed(ctx, false, None)?;
        let mut args = BTreeMap::new();
        args.insert("from".to_string(), Cj::Str(from.to_string()));
        args.insert("to".to_string(), Cj::Str(to.to_string()));
        let payload = Self::file_payload("FileRelink", args);
        let key = self.key_of(ctx, &caller, &caller.branch, &payload);
        if let Some(r) = self.keyed(&key, &payload, &caller.branch, ctx, &caller)? {
            return Ok(r);
        }
        let rights = self.rights(&caller, ctx);
        rights
            .verb("file-relink-after")
            .map_err(|e| e.finish(None))?;
        self.writable(&caller.branch, false)?;
        self.tree_mismatch(&caller, ctx)?;
        let tc = self.need_tree(&caller, ctx, to)?;
        let tip = self.dag.live(&caller.branch).and_then(|r| r.tip);
        let mut c = self.file_cand(&caller, tip);
        let n = self.file_of(&c, Some(&tc), from)?;
        let (rel, disk) = match self.find_path(&tc, to, false)? {
            Found::File { rel, disk } => (rel, disk),
            Found::Dir { rel } | Found::Missing { rel } => {
                return Err(Refusal::not_found("path", rel));
            }
        };
        let old = crate::links::file_node(n, &c.st.nodes[&n], 0);
        Self::claimed(&c.st, &[n], &old.root, &rel)?;
        let relink = Self::manual_relink(&caller, "manual");
        rights.value(1, &c.st, n, "relink", &Value::Text(relink.clone()))?;
        let o = self.repoint(&tc, &old.root, &rel, &disk, relink.clone());
        set_observation(&mut c.st, n, &o);
        add_alias(
            &mut c.st,
            n,
            &PathVal {
                root: old.root.clone(),
                text: old.path.clone(),
            },
        );
        let seen = vec![(n, Some(disk.clone()))];
        let land = crate::links::Landing {
            caller: &caller,
            ctx,
            key,
            payload,
            cmd: "FileRelink",
            origin: Self::named_origin(ctx),
            sym: "tx.record_move".into(),
            yields: Vec::new(),
        };
        let (mut reply, seq) = self.land_file(land, c, tip, "", 0)?;
        self.observe_rows(ctx, &tc.root, &seen);
        reply.yields = vec![fix_yield("tx.record_move", n, "relink", Some(&relink), seq)];
        self.set_recorded_yields(&reply);
        Ok(reply)
    }

    /// The recorded yields of the idempotency entry a family-T file command wrote after its commit was known.
    pub fn set_recorded_yields(&mut self, reply: &Reply) {
        if let Some(e) = self
            .idem
            .entries
            .values_mut()
            .find(|e| e.commit.map(|c| c.0) == reply.commit && reply.commit.is_some())
        {
            e.result.yields = reply.yields.clone();
        }
    }

    /// `Check` ([API §12.7]; [AR §2.14] CM7): the ancestry of the node's pinned git commit (`measured_on`,
    /// `observed_git_sha`) against the tip of the bound worktree's history — the command's tree, else the branch's
    /// designated tree; appends lazy `GitFacts` ancestry facts only, never a commit, and is never keyed.
    // spec: [API §12.7]
    // rule: WV-024
    pub fn check(&mut self, id: &Target, ctx: &Ctx) -> Res<Reply> {
        let caller = self.resolve(ctx, false)?;
        self.rights(&caller, ctx)
            .verb("check")
            .map_err(|e| e.finish(None))?;
        self.tree_mismatch(&caller, ctx)?;
        let tip = if caller.branch.is_empty() {
            caller.detached
        } else {
            self.dag.live(&caller.branch).and_then(|r| r.tip)
        };
        let st = self.dag.state_at(tip, &self.alloc);
        let n = live_target(self, &st, id)?;
        let x = &st.nodes[&n];
        let pinned =
            ["measured_on", "observed_git_sha"]
                .iter()
                .find_map(|f| match x.fields.get(*f) {
                    Some(Value::Oid(o)) => Some(o.clone()),
                    _ => None,
                });
        let tree = self.tree_ctx(&caller, ctx).map(|t| t.root).or_else(|| {
            self.designation()
                .into_iter()
                .find(|p| p.branch == caller.branch)
                .map(|p| p.tree)
        });
        let git = tree.as_deref().and_then(|t| {
            let (name, _) = self.files.git.heads.get(t)?;
            let (repo, head) = self.files.git.of_tree(t)?;
            Some((name.clone(), repo, repo.head_commit(head)?.to_string()))
        });
        let render = |algo: Algo, h: &str| format!("{}:{h}", algo.name());
        let (commit, tip_id, verdict, fact) = match (&pinned, &git) {
            (Some(o), Some((name, repo, h))) => {
                let c = crate::value::hex(&o.digest);
                let v = if o.algo != repo.algo || !repo.has(&c) {
                    "unknown"
                } else if repo.is_ancestor(&c, h) {
                    "ancestor"
                } else {
                    "not-ancestor"
                };
                let fact = (v != "unknown")
                    .then(|| ((name.clone(), c.clone(), h.clone()), v == "ancestor"));
                (
                    Some(render(o.algo, &c)),
                    Some(render(repo.algo, h)),
                    v,
                    fact,
                )
            }
            (Some(o), None) => (
                Some(render(o.algo, &crate::value::hex(&o.digest))),
                None,
                "unknown",
                None,
            ),
            (None, Some((_, repo, h))) => (None, Some(render(repo.algo, h)), "unknown", None),
            (None, None) => (None, None, "unknown", None),
        };
        if let Some((k, v)) = fact
            && !ctx.dry
        {
            self.files.facts.insert(k, v);
        }
        let mut reply = Reply::ok(Data::Check(Box::new(CheckData {
            node: n,
            commit,
            tip: tip_id,
            verdict,
        })));
        reply.branch = Some(caller.branch.clone());
        reply.rev = Some(tip.unwrap_or(0));
        reply.warnings = caller.warnings;
        Ok(reply)
    }
}

/// The yield row of `LinksFix` and `FileRelink` ([API §12.5]): `{target, action, relink, commit}`.
pub fn fix_yield(
    proc: &str,
    target: Nid,
    action: &str,
    relink: Option<&str>,
    commit: Option<u64>,
) -> Yield {
    Yield {
        index: 0,
        proc: proc.to_string(),
        rows: vec![vec![
            ("target".to_string(), target.to_string()),
            ("action".into(), action.to_string()),
            ("relink".into(), relink.unwrap_or("null").to_string()),
            (
                "commit".into(),
                commit.map_or("null".to_string(), |s| format!("s{s}")),
            ),
        ]],
    }
}

/// The directory prefix of a path (`dir/`), or the empty text at the root.
pub fn dir_of(p: &str) -> String {
    let d = dirname(p);
    if d.is_empty() {
        String::new()
    } else {
        format!("{d}/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specs_parse_into_their_forms() {
        let f = |s: &str| parse_spec(s, None, None).unwrap();
        assert_eq!(
            (f("docs/api.md:3").path, f("docs/api.md:3").form),
            ("docs/api.md".to_string(), Form::Lines(3, 3))
        );
        assert_eq!(f("a.rs:10-12").form, Form::Lines(10, 12));
        assert_eq!(f("a.rs").form, Form::File);
        let p = f("a.md@3c3c:1-2");
        assert_eq!(
            (p.path, p.form, p.pin),
            (
                "a.md".to_string(),
                Form::Lines(1, 2),
                Some("3c3c".to_string())
            )
        );
        assert!(matches!(
            f("lock.rs::LockFile/acquire").form,
            Form::Symbol(_)
        ));
        assert!(matches!(f("a.md#Recovery").form, Form::Heading(_)));
        let q = parse_spec("a.md", Some("q"), Some("e")).unwrap();
        assert!(matches!(q.form, Form::QuoteText(_)) && q.end == Some(b"e".to_vec()));
        assert!(parse_spec("a.md", None, Some("e")).is_err());
        assert_eq!(f("a:b.md").form, Form::File);
    }

    #[test]
    fn scratchpads_are_claude_session_directories() {
        assert!(in_scratchpad("C:/tmp/claude/p/s/scratchpad/x.md"));
        assert!(!in_scratchpad("C:/work/docs/scratchpad/x.md"));
    }
}
