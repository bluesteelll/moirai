//! Streams of the file-link group F with its environment commands ([API §12], §6.5, §6.6): [API §19] example 13
//! (`EnvTree`, `EnvGit`, `WorktreeBind`, `LinkFile`, a directory `FileMv`, a raw move and the `LinksSync` that re-binds
//! it) and the commands' rules, refusals and runtime rows, each against the section it cites.

use super::*;
use crate::api::{Data, Door};
use crate::links::{EnvGitCommit, EnvHead, IntentState};
use crate::r4::tree::TreeOp;
use crate::state::{Aspect, KState, KVal, Key};
use crate::value::{Algo, MoveClass, PathVal, Value};
use std::collections::BTreeMap;

const TREE: &str = "C:/work/moirai";
const HEAD: &str = "sha1:3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c3c";
const API_TEXT: &str = "# API\n\n## Commands\n\nTx, Mutation and Apply.\n";

fn w(path: &str, text: &str) -> TreeOp {
    TreeOp::Write {
        path: path.into(),
        bytes: text.as_bytes().to_vec(),
        btime_ns: None,
    }
}

fn mkdir(path: &str) -> TreeOp {
    TreeOp::Mkdir {
        path: path.into(),
        case_sensitive: None,
    }
}

fn mv(from: &str, to: &str) -> TreeOp {
    TreeOp::Mv {
        from: from.into(),
        to: to.into(),
    }
}

fn env_tree(ops: Vec<TreeOp>) -> Cmd {
    Cmd::EnvTree {
        tree: TREE.into(),
        volume: Some("C".into()),
        caps: None,
        ops,
    }
}

/// The git history of example 13: one root commit on `refs/heads/main` holding both files, HEAD of the tree.
fn env_git(commit: &str, tree: &[(&str, &str)], parents: &[&str]) -> Cmd {
    Cmd::EnvGit {
        repo: "moirai".into(),
        algo: Some(Algo::Sha1),
        commits: vec![EnvGitCommit {
            id: commit.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            committer_time: 1_789_999_000,
            author_time: 1_789_999_000,
            tree: tree
                .iter()
                .map(|(p, b)| (p.to_string(), b.to_string()))
                .collect(),
        }],
        refs: vec![("refs/heads/main".into(), Some(commit.into()))],
        heads: vec![(TREE.into(), EnvHead::Ref("refs/heads/main".into()))],
    }
}

/// The orchestrator in the tree of example 13.
fn in_tree() -> Ctx {
    Ctx {
        tree: Some(TREE.into()),
        ..orch()
    }
}

/// The stream of example 02 through `n` = 4, then example 13's `EnvTree`, `EnvGit` and `WorktreeBind`.
fn bound() -> S {
    let mut s = S::base();
    let mut ctx = orch();
    ctx.key = Some("plan-api".into());
    s.ok(
        tx(vec![
            node(
                "api",
                "task",
                &[("title", t("Ship the Store API")), ("priority", t("P1"))],
            ),
            child("ch", "Write the chapter", Target::Var("api".into())),
            child("ex", "Write the examples", Target::Var("api".into())),
            Stmt::Link {
                src: Target::Var("ch".into()),
                kind: "blocks".into(),
                dst: Target::Var("ex".into()),
                pinned: None,
            },
        ]),
        ctx,
    );
    let r = s.ok(
        env_tree(vec![
            mkdir("docs"),
            w("docs/api.md", API_TEXT),
            w("docs/notes.md", "notes\n"),
        ]),
        Ctx::default(),
    );
    let Data::Tree(d) = &r.data else {
        panic!("{:?}", r.data)
    };
    assert_eq!((d.tree.as_str(), d.files, d.dirs), (TREE, 2, 1));
    let r = s.ok(
        env_git(
            HEAD,
            &[
                (
                    "docs/api.md",
                    "sha1:4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d",
                ),
                (
                    "docs/notes.md",
                    "sha1:5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e",
                ),
            ],
            &[],
        ),
        Ctx::default(),
    );
    assert_eq!(r.data, Data::Git("moirai".into(), 1));
    s
}

fn bind(s: &mut S) -> Reply {
    s.ok(
        Cmd::WorktreeBind {
            dir: TREE.into(),
            ref_: "main".into(),
            replace: false,
        },
        orch(),
    )
}

fn link(s: &mut S, n: u32, spec: &str) -> Reply {
    s.ok(
        Cmd::LinkFile {
            node: Target::Id(Nid(n)),
            specs: vec![spec.into()],
            watch: None,
            planned: false,
            quote: None,
            end: None,
        },
        in_tree(),
    )
}

fn row<'a>(r: &'a Reply, k: &str) -> &'a str {
    r.yields[0].rows[0]
        .iter()
        .find(|(n, _)| n == k)
        .map(|(_, v)| v.as_str())
        .unwrap_or_else(|| panic!("no {k} in {:?}", r.yields))
}

/// The six values of an observation composite.
type Composite = Option<Vec<Option<Value>>>;

fn obs(r: &Reply, n: u32) -> (Composite, Composite) {
    let get = |k: &KState| match k {
        KState::Plain(Some(KVal::Observation(v))) => Some(v.clone()),
        _ => None,
    };
    let (b, a) = &r.diff[&Key::Node(Nid(n), Aspect::Observation)];
    (get(b), get(a))
}

fn path(text: &str) -> Option<Value> {
    Some(Value::Path(PathVal {
        root: "project".into(),
        text: text.into(),
    }))
}

const HEAD2: &str = "sha1:6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f";

fn sync(key: Option<&str>) -> (Cmd, Ctx) {
    let mut ctx = in_tree();
    ctx.key = key.map(str::to_string);
    (
        Cmd::LinksSync {
            scope: None,
            budget_ms: None,
            since: None,
            deep: false,
            all: false,
            force: false,
        },
        ctx,
    )
}

/// Example 13 through the directory move and the raw move ([API §19]).
fn moved() -> S {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    s.ok(
        Cmd::FileMv {
            srcs: vec!["docs".into()],
            dst: "handbook".into(),
            git: false,
            retry_ms: None,
        },
        in_tree(),
    );
    s.ok(
        env_tree(vec![mv("handbook/api.md", "handbook/reference.md")]),
        Ctx::default(),
    );
    s
}

/// On `main` an automatic re-bind needs the observation committed in the writer tree's HEAD ([F18 §2.6] I-F6 item 5):
/// the raw move of example 13, not yet committed, resolves `moved-auto` by file id and the settle writes no re-bind —
/// example 13's last step, which re-binds it, disagrees with the rule (a spec finding of WP-92), and the rule holds
/// ([API §19]).
#[test]
fn main_rebinds_only_a_committed_move() {
    let mut s = moved();
    let (c, ctx) = sync(None);
    let r = s.ok(c, ctx);
    assert_eq!((r.rev, r.rev_new), (Some(3), None));
    assert_eq!(row(&r, "rebound"), "[]");
    assert_eq!(row(&r, "states"), r#"{"moved-auto":1}"#);
    assert_eq!(row(&r, "commit"), "null");
    // The settle's runtime rows still follow: `FILEOBS` keeps the file's identity for the next settle.
    assert!(s.st.files.rt.fileobs.contains_key(&(5, TREE.to_string())));
}

/// [API §19] example 13, computed values: the tree and history, the designated binding with its expected ref and base,
/// the capture with its root and file nodes, the directory move with its intent, `path_moves` entry and CK-5 `hlc`, a
/// raw move committed in the tree's history, and the settle that re-binds it by file id ([API §12.1]–§12.4, §12.6,
/// §6.2 CK-4, CK-5; [F18 §2.6] I-F6).
#[test]
fn example_13_file_mv() {
    let mut s = bound();
    let b = bind(&mut s);
    let Data::Bind(bd) = &b.data else { panic!() };
    assert!(bd.designated && b.warnings.is_empty());
    assert_eq!(bd.expected_ref.as_deref(), Some("main"));
    assert_eq!(bd.base.as_deref(), Some(HEAD));
    assert_eq!((b.branch.as_deref(), b.rev), (Some("main"), Some(1)));
    let hlc0 = s.st.hlc.seq;
    // LinkFile: the file node #5, the root node #4 and the `at` edge with anchor a1.
    let r = link(&mut s, 2, "docs/api.md:3");
    assert_eq!((r.rev, r.rev_new, r.commit), (Some(1), Some(2), Some(2)));
    assert_eq!(r.other, vec![Nid(4), Nid(5)]);
    assert_eq!(r.yields[0].proc, "tx.link_file");
    assert_eq!(row(&r, "file"), "#5");
    assert_eq!(row(&r, "path"), "project:docs/api.md");
    assert_eq!(row(&r, "anchor"), "a1");
    assert_eq!(row(&r, "kind"), "quote");
    assert_eq!(row(&r, "created"), "true");
    assert_eq!(
        s.st.dag.commits[&2].hlc,
        hlc0 + 1,
        "CK-4: LinkFile's commit follows the binding"
    );
    let st = s.st.dag.state_at(Some(2), &s.st.alloc);
    let area = &st.nodes[&Nid(4)];
    assert_eq!(
        (area.kind.as_str(), area.text("root"), area.text("title")),
        ("area", Some("project"), Some("root:project"))
    );
    let f = &st.nodes[&Nid(5)];
    assert_eq!(f.kind, "artifact");
    assert_eq!(f.fields.get("origin_path"), path("docs/api.md").as_ref());
    let (_, after) = obs(&r, 5);
    let after = after.unwrap();
    assert_eq!(after[0], path("docs/api.md"));
    assert_eq!(after[2], Some(Value::Int(44)));
    assert_eq!(
        after[3],
        crate::links::git_oid(Algo::Sha1, &crate::links::git_hex(HEAD)).map(Value::Oid)
    );
    assert!(
        after[4].is_some() && after[5].is_none(),
        "observed_blob set, relink absent"
    );
    let (k, p) = st.nodes[&Nid(2)]
        .out
        .iter()
        .find(|(k, _)| k.kind == "at")
        .expect("the at edge");
    assert_eq!(k.dst, Nid(5));
    let a = p.anchor.as_deref().unwrap();
    assert_eq!(
        (a.kind.as_str(), a.mode.as_str(), a.watch.as_str()),
        ("quote", "live", "span")
    );
    assert_eq!(a.hint, Some((3, 3)));
    let text = a.text.as_deref().unwrap();
    assert_eq!(text.quote, b"## Commands");
    assert_eq!(text.prefix, b"# API\n\n");
    assert_eq!(s.st.files.anchors.get(&k.disc.unwrap()), Some(&1));
    assert_eq!(s.st.next_anchor, 2);
    // FileMv of the directory: FsIntent (7), the commit (8), FsIntentDone (9).
    let r = s.ok(
        Cmd::FileMv {
            srcs: vec!["docs".into()],
            dst: "handbook".into(),
            git: false,
            retry_ms: None,
        },
        in_tree(),
    );
    assert_eq!(
        (r.rev, r.rev_new, r.commit, r.exit),
        (Some(2), Some(3), Some(3), 0)
    );
    let Data::Intent(d) = &r.data else { panic!() };
    assert_eq!(d.intent.as_deref(), Some("i-1"));
    assert_eq!(d.items.len(), 1);
    assert_eq!(
        crate::links::intent::item_text(&d.items[0]),
        ("project:docs".into(), Some("project:handbook".into()))
    );
    assert_eq!(d.items[0].outcome, "done");
    assert_eq!(d.repointed, vec![Nid(5)]);
    let c3 = &s.st.dag.commits[&3];
    assert_eq!(c3.stmt_origin, "file-verb");
    assert_eq!(c3.stmt_sym.as_deref(), Some("mv"));
    let pm = d.path_move.as_ref().unwrap();
    assert_eq!(
        pm.hlc, c3.hlc,
        "CK-5: the entry's hlc is the command's first commit's"
    );
    assert_eq!(pm.class, MoveClass::Explicit);
    assert_eq!(
        (pm.from.text.as_str(), pm.to.text.as_str()),
        ("docs/", "handbook/")
    );
    let i = &s.st.files.intents[0];
    assert_eq!(i.state, IntentState::Done);
    assert_eq!(
        (i.hlc, c3.hlc, i.closed_hlc),
        (hlc0 + 2, hlc0 + 3, hlc0 + 4)
    );
    assert_eq!(i.commit, Some(3));
    let (_, after) = obs(&r, 5);
    let after = after.unwrap();
    assert_eq!(after[0], path("handbook/api.md"));
    assert_eq!(
        after[4], None,
        "observed_blob empty until the move is committed"
    );
    assert_eq!(after[5], Some(Value::Text("explicit/intent".into())));
    let st = s.st.dag.state_at(Some(3), &s.st.alloc);
    assert_eq!(
        st.nodes[&Nid(5)].fields.get("aliases"),
        Some(&Value::Set(vec![path("docs/api.md").unwrap()]))
    );
    assert!(
        s.st.files.fs.trees[TREE]
            .files
            .contains_key("handbook/api.md")
    );
    // A raw move, committed in the tree's history, then the settle: `lazy/file-id`.
    let r = s.ok(
        env_tree(vec![mv("handbook/api.md", "handbook/reference.md")]),
        Ctx::default(),
    );
    let Data::Tree(d) = &r.data else { panic!() };
    assert_eq!((d.files, d.dirs), (2, 1));
    s.ok(
        env_git(
            HEAD2,
            &[
                (
                    "handbook/notes.md",
                    "sha1:5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e",
                ),
                (
                    "handbook/reference.md",
                    "sha1:4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d",
                ),
            ],
            &[HEAD],
        ),
        Ctx::default(),
    );
    let (c, ctx) = sync(Some("settle-1"));
    let r = s.ok(c.clone(), ctx.clone());
    assert_eq!(
        (r.rev, r.rev_new, r.commit, r.key.as_deref()),
        (Some(3), Some(4), Some(4), Some("settle-1"))
    );
    let (before, after) = obs(&r, 5);
    assert_eq!(before.unwrap()[0], path("handbook/api.md"));
    let after = after.unwrap();
    assert_eq!(after[0], path("handbook/reference.md"));
    assert_eq!(after[2], Some(Value::Int(44)));
    assert_eq!(
        after[3],
        crate::links::git_oid(Algo::Sha1, &crate::links::git_hex(HEAD2)).map(Value::Oid)
    );
    assert_eq!(
        after[4],
        crate::links::git_oid(Algo::Sha1, &"4d".repeat(20)).map(Value::Oid)
    );
    assert_eq!(after[5], Some(Value::Text("lazy/file-id".into())));
    assert_eq!(r.yields[0].proc, "tx.links_sync");
    assert_eq!(
        row(&r, "rebound"),
        r##"[{"file":"#5","from":"project:handbook/api.md","to":"project:handbook/reference.md","evidence":"file-id"}]"##
    );
    assert_eq!(row(&r, "pending"), "0");
    assert_eq!(row(&r, "states"), r#"{"ok":1}"#);
    assert_eq!(row(&r, "commit"), "s4");
    // An explicit key replays the settle; the runtime rows followed the file.
    let again = s.run(c, ctx);
    assert_eq!(again.outcome, Outcome::Replayed);
    assert_eq!(again.yields, r.yields);
    let rt = s.st.runtime();
    assert_eq!(rt.intents.len(), 1);
    assert!(s.st.files.rt.fileobs.contains_key(&(5, TREE.to_string())));
    assert!(s.st.files.rt.trees[TREE].first_settle_done);
}

/// A second orchestrator: the session role lease L-2 of `orch2` on the slot of `claude:s2`.
fn second_orchestrator(s: &mut S) -> Ctx {
    s.ok(
        Cmd::EnvSlots(crate::clock::EnvSlots {
            hold: vec!["claude:s2".into()],
            ..Default::default()
        }),
        Ctx::default(),
    );
    let mut ctx = Ctx {
        agent: Some("orch2".into()),
        ..Default::default()
    };
    ctx.env.insert("CLAUDECODE".into(), "1".into());
    ctx.env.insert("CLAUDE_CODE_SESSION_ID".into(), "s2".into());
    let r = s.ok(
        Cmd::Claim {
            ids: vec![],
            next: false,
            scope: None,
            role: Some("orchestrator".into()),
            agent: None,
            ttl: None,
            start: false,
            run: None,
            session: true,
        },
        ctx,
    );
    let lease = r.yields[0].rows[0]
        .iter()
        .find(|(k, _)| k == "lease")
        .map(|(_, v)| v.clone())
        .unwrap();
    Ctx {
        lease: Some(lease),
        client: Some("claude".into()),
        tree: Some(TREE.into()),
        ..Default::default()
    }
}

fn fix(target: &str, action: &str) -> crate::links::fix::FixArgs {
    crate::links::fix::FixArgs {
        target: target.into(),
        action: action.into(),
        expect: None,
        to: None,
        at: None,
        same_as: None,
        reason: None,
        replaced_by: None,
        from: None,
    }
}

fn links_fix(a: crate::links::fix::FixArgs) -> Cmd {
    Cmd::LinksFix {
        target: a.target,
        action: a.action,
        expect: a.expect,
        to: a.to,
        at: a.at,
        same_as: a.same_as,
        reason: a.reason,
        replaced_by: a.replaced_by,
        from: a.from,
    }
}

fn file_node(s: &S, n: u32) -> crate::r4::cascade::FileNode {
    let tip = s.st.dag.live("main").and_then(|r| r.tip);
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    crate::links::file_node(Nid(n), &st.nodes[&Nid(n)], 0)
}

fn tip_state(s: &S) -> std::rc::Rc<crate::state::State> {
    let tip = s.st.dag.live("main").and_then(|r| r.tip);
    s.st.dag.state_at(tip, &s.st.alloc)
}

/// `FileAdd` ([API §12.2]): a new file node with its derived uid and the root node, in one `file-verb` commit; a live
/// node at the key is reported, not duplicated; a directory registers with `artifact_kind` `dir`; a missing path is
/// `not_found` (`path`); a replay rebuilds the data ([API §7.5]).
#[test]
fn file_add_registers_and_reports_existing_nodes() {
    let mut s = bound();
    bind(&mut s);
    let add = |paths: &[&str]| Cmd::FileAdd {
        paths: paths.iter().map(|p| p.to_string()).collect(),
        kind: None,
        root: None,
    };
    let mut ctx = in_tree();
    ctx.key = Some("add-1".into());
    let r = s.ok(add(&["docs/notes.md"]), ctx.clone());
    let Data::FileAdd(files) = &r.data else {
        panic!()
    };
    assert_eq!(files.len(), 1);
    assert_eq!((files[0].1, files[0].2), (Nid(5), true));
    assert_eq!(
        crate::links::path_text(&files[0].0),
        "project:docs/notes.md"
    );
    let c = &s.st.dag.commits[&r.rev_new.unwrap()];
    assert_eq!(
        (c.stmt_origin, c.stmt_sym.as_deref()),
        ("file-verb", Some("add"))
    );
    let u = s.st.alloc.uids[&Nid(5)];
    assert_eq!(
        u,
        crate::r4::uid::uid_file("project", "docs/notes.md", None),
        "I-F2"
    );
    assert_eq!(
        s.st.alloc.uids[&Nid(4)],
        crate::r4::uid::uid_root("project")
    );
    let again = s.run(add(&["docs/notes.md"]), ctx);
    assert_eq!(again.outcome, Outcome::Replayed);
    assert_eq!(again.data, r.data);
    let r = s.ok(add(&["notes.md", "docs"]), in_tree());
    let Data::FileAdd(files) = &r.data else {
        panic!()
    };
    assert_eq!(
        (files[0].1, files[0].2),
        (Nid(5), false),
        "a basename expands to the unique match"
    );
    assert!(files[1].2);
    assert_eq!(
        file_node(&s, files[1].1.0).artifact_kind.as_deref(),
        Some("dir")
    );
    s.refused(add(&["docs/none.md"]), in_tree(), "not_found");
}

/// `LinkFile` ([API §12.3]; [F20 §6.1]): the interim scanner rule refuses the symbol form (`anchor_spec`,
/// `no-scanner`); a basename matching two files is `ambiguous_path` with the matches; a missing path is `not_found`
/// unless planned; a planned link records the planning tree's HEAD; an identical capture reuses its anchor
/// (`created` false) and a new one takes the next `aN`.
#[test]
fn link_file_captures_and_refuses() {
    let mut s = bound();
    s.ok(env_tree(vec![w("src/api.md", "other\n")]), Ctx::default());
    bind(&mut s);
    let lk = |spec: &str, planned: bool| Cmd::LinkFile {
        node: Target::Id(Nid(2)),
        specs: vec![spec.into()],
        watch: None,
        planned,
        quote: None,
        end: None,
    };
    let r = s.refused(lk("docs/api.md::Commands", false), in_tree(), "anchor_spec");
    assert_eq!(r.error.unwrap().get_str("case"), Some("no-scanner"));
    let r = s.refused(lk("api.md:3", false), in_tree(), "ambiguous_path");
    assert_eq!(
        r.error.unwrap().get("matches"),
        Some(&crate::err::Kv::List(vec![
            crate::err::Kv::Str("docs/api.md".into()),
            crate::err::Kv::Str("src/api.md".into())
        ]))
    );
    s.refused(lk("docs/later.md", false), in_tree(), "not_found");
    let r = s.ok(lk("docs/later.md", true), in_tree());
    assert_eq!(row(&r, "kind"), "file");
    let n = row(&r, "file")
        .trim_start_matches('#')
        .parse::<u32>()
        .unwrap();
    let f = file_node(&s, n);
    assert_eq!(f.status, crate::r4::uid::FileStatus::Planned);
    assert_eq!(
        f.observed_git.as_deref(),
        Some(crate::links::git_hex(HEAD).as_str())
    );
    assert!(f.oid.is_none());
    let a = link(&mut s, 2, "docs/api.md:3");
    assert_eq!((row(&a, "anchor"), row(&a, "created")), ("a2", "true"));
    let again = s.run(lk("docs/api.md:3", false), in_tree());
    assert_eq!(
        again.outcome,
        Outcome::Replayed,
        "the default key replays an identical command"
    );
    let b = s.ok(
        lk("docs/api.md:3", false),
        Ctx {
            no_dedupe: true,
            ..in_tree()
        },
    );
    assert_eq!((row(&b, "anchor"), row(&b, "created")), ("a2", "false"));
    assert_eq!(b.rev_new, None, "a de-duplicated capture changes nothing");
    let c = s.ok(
        Cmd::LinkFile {
            node: Target::Id(Nid(3)),
            specs: vec!["docs/api.md".into(), "docs/api.md:5".into()],
            watch: Some("span".into()),
            planned: false,
            quote: None,
            end: None,
        },
        in_tree(),
    );
    let rows = &c.yields[0].rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][2].1, "a3");
    assert_eq!(rows[1][2].1, "a4");
    assert_eq!(rows[0][3].1, "file");
    // A quoted text for the path ([LQ/std §7.4] `$quote`), and a quoted range with `$end`.
    let q = s.ok(
        Cmd::LinkFile {
            node: Target::Id(Nid(1)),
            specs: vec!["docs/api.md".into()],
            watch: None,
            planned: false,
            quote: Some("## Commands".into()),
            end: Some("Apply.".into()),
        },
        in_tree(),
    );
    assert_eq!(row(&q, "kind"), "quote");
}

/// `UnlinkFile` ([API §12.3]): one anchor by `aN`, or every anchor into the file at a path; the edge goes with its last
/// anchor; an unknown anchor is `not_found` (`anchor`).
#[test]
fn unlink_file_removes_anchors() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    link(&mut s, 2, "docs/api.md:5");
    let un = |anchor: Option<&str>, path: Option<&str>| Cmd::UnlinkFile {
        node: Target::Id(Nid(2)),
        anchor: anchor.map(str::to_string),
        path: path.map(str::to_string),
    };
    s.refused(un(Some("a9"), None), in_tree(), "not_found");
    s.refused(un(None, None), in_tree(), "usage");
    let r = s.ok(un(Some("a1"), None), in_tree());
    assert_eq!(row(&r, "anchor"), "a1");
    let st = tip_state(&s);
    assert_eq!(
        st.nodes[&Nid(2)]
            .out
            .keys()
            .filter(|k| k.kind == "at")
            .count(),
        1
    );
    let r = s.ok(un(None, Some("docs/api.md")), in_tree());
    assert_eq!(r.yields[0].rows.len(), 1);
    let st = tip_state(&s);
    assert!(!st.nodes[&Nid(2)].out.keys().any(|k| k.kind == "at"));
    assert!(
        st.live(Nid(5)).is_some(),
        "file nodes without referrers are kept"
    );
}

/// `FileRelink` ([API §12.5]; [40 §3.6]): the node re-pointed to an existing path, `agent/manual` for an agent role,
/// the old path in `aliases`; a missing destination is `not_found`.
#[test]
fn file_relink_records_a_manual_move() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    s.ok(
        env_tree(vec![mv("docs/api.md", "docs/moved.md")]),
        Ctx::default(),
    );
    let rl = |to: &str| Cmd::FileRelink {
        from: "docs/api.md".into(),
        to: to.into(),
    };
    s.refused(rl("docs/none.md"), in_tree(), "not_found");
    let r = s.ok(rl("docs/moved.md"), in_tree());
    assert_eq!(row(&r, "relink"), "agent/manual");
    assert_eq!(row(&r, "target"), "#5");
    let f = file_node(&s, 5);
    assert_eq!(
        (f.path.as_str(), f.relink.as_deref()),
        ("docs/moved.md", Some("agent/manual"))
    );
    assert_eq!(f.aliases, vec!["docs/api.md".to_string()]);
}

/// `LinksFix` ([API §12.5]; [40 §3.7]; [F18 §5.4], §5.5): `--to` records `agent/manual`; `--confirm` is refused to the
/// acceptor (`confirm_refused`, `same-actor`) and to a role outside `files.confirm-roles`, and turns `agent/*` into
/// `confirmed/*` for another orchestrator; confirming again finds nothing to confirm; `--accept` whose re-evaluated top
/// proposal differs from `expect` is E404.
#[test]
fn links_fix_to_and_confirm() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    s.ok(
        env_tree(vec![mv("docs/api.md", "docs/moved.md")]),
        Ctx::default(),
    );
    let mut to = fix("#5", "to");
    to.to = Some("docs/moved.md".into());
    let r = s.ok(links_fix(to), in_tree());
    assert_eq!(row(&r, "relink"), "agent/manual");
    assert_eq!(row(&r, "action"), "to");
    let r = s.refused(
        links_fix(fix("#5", "confirm")),
        in_tree(),
        "confirm_refused",
    );
    assert_eq!(r.error.unwrap().get_str("case"), Some("same-actor"));
    let o2 = second_orchestrator(&mut s);
    let r = s.ok(links_fix(fix("#5", "confirm")), o2.clone());
    assert_eq!(row(&r, "relink"), "confirmed/manual");
    assert_eq!(file_node(&s, 5).relink.as_deref(), Some("confirmed/manual"));
    let fresh = Ctx {
        no_dedupe: true,
        ..o2
    };
    let r = s.refused(links_fix(fix("#5", "confirm")), fresh, "confirm_refused");
    assert_eq!(r.error.unwrap().get_str("case"), Some("not-a-guess"));
    let mut acc = fix("#5", "accept");
    acc.expect = Some("docs/notes.md".into());
    s.refused(links_fix(acc), in_tree(), "E404");
    s.refused(links_fix(fix("#5", "accept")), in_tree(), "usage");
    s.refused(links_fix(fix("#5", "bogus")), in_tree(), "usage");
}

/// `--drop` makes a file node `removed` (TR-051) and its referrer `suspect` ([40 §2.9]); `--restore` brings it back
/// (TR-054, I-F14's explicit door); `--same-as` unifies two nodes and re-points the anchors (LV-006).
#[test]
fn links_fix_drop_restore_and_same_as() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    let mut d = fix("#5", "drop");
    d.reason = Some("obsolete".into());
    s.ok(links_fix(d), in_tree());
    let st = tip_state(&s);
    assert_eq!(st.nodes[&Nid(5)].status, "removed");
    assert_eq!(st.nodes[&Nid(5)].text("reason"), Some("obsolete"));
    let rows = crate::derived::recompute_all(&st, &|_| None);
    assert!(
        rows[&Nid(2)].suspect,
        "an at edge into a removed file makes its source suspect"
    );
    s.ok(links_fix(fix("#5", "restore")), in_tree());
    assert_eq!(tip_state(&s).nodes[&Nid(5)].status, "present");
    let r = s.ok(
        Cmd::FileAdd {
            paths: vec!["docs/notes.md".into()],
            kind: None,
            root: None,
        },
        in_tree(),
    );
    let Data::FileAdd(files) = &r.data else {
        panic!()
    };
    let notes = files[0].1;
    let mut same = fix("#5", "same-as");
    same.same_as = Some(Target::Id(notes));
    s.ok(links_fix(same), in_tree());
    let st = tip_state(&s);
    assert_eq!(st.nodes[&Nid(5)].status, "removed");
    assert_eq!(
        st.nodes[&Nid(5)].fields.get("replaced_by"),
        Some(&Value::Ref(notes))
    );
    assert!(
        st.nodes[&Nid(2)]
            .out
            .keys()
            .any(|k| k.kind == "at" && k.dst == notes)
    );
    assert!(
        !st.nodes[&Nid(2)]
            .out
            .keys()
            .any(|k| k.kind == "at" && k.dst == Nid(5))
    );
}

/// On anchors ([40 §3.7]): `--pin` makes a historical citation; `--repin` recaptures an exact match and needs `--at`
/// after an inexact one (`repin_needs_at`, exit 6), keeping the anchor's uid and `captured`; `--drop` removes it.
#[test]
fn links_fix_on_anchors() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    // The quoted line moves down two lines: an exact `moved` match.
    s.ok(
        env_tree(vec![w(
            "docs/api.md",
            "# API\n\nintro\n\n## Commands\n\nTx, Mutation and Apply.\n",
        )]),
        Ctx::default(),
    );
    s.ok(links_fix(fix("a1", "repin")), in_tree());
    let st = tip_state(&s);
    let (k, p) = st.nodes[&Nid(2)]
        .out
        .iter()
        .find(|(k, _)| k.kind == "at")
        .unwrap();
    let a = p.anchor.as_deref().unwrap();
    assert_eq!(a.hint, Some((5, 5)));
    assert_eq!(
        s.st.files.anchors.get(&k.disc.unwrap()),
        Some(&1),
        "the uid is kept"
    );
    // The quote edited: not exact any more.
    s.ok(
        env_tree(vec![w(
            "docs/api.md",
            "# API\n\nintro\n\n## Command list\n\nTx, Mutation and Apply.\n",
        )]),
        Ctx::default(),
    );
    let fresh = Ctx {
        no_dedupe: true,
        ..in_tree()
    };
    let r = s.refused(links_fix(fix("a1", "repin")), fresh, "repin_needs_at");
    assert_eq!(r.error.unwrap().get_str("anchor"), Some("a1"));
    let mut at = fix("a1", "repin");
    at.at = Some("docs/api.md:5".into());
    s.ok(links_fix(at), in_tree());
    s.ok(links_fix(fix("a1", "pin")), in_tree());
    let st = tip_state(&s);
    let p = st.nodes[&Nid(2)]
        .out
        .iter()
        .find(|(k, _)| k.kind == "at")
        .unwrap()
        .1;
    assert_eq!(p.anchor.as_deref().unwrap().mode, "pinned");
    s.ok(links_fix(fix("a1", "drop")), in_tree());
    assert!(
        !tip_state(&s).nodes[&Nid(2)]
            .out
            .keys()
            .any(|k| k.kind == "at")
    );
    let fresh = Ctx {
        no_dedupe: true,
        ..in_tree()
    };
    s.refused(links_fix(fix("a1", "drop")), fresh, "not_found");
}

/// `--prefix FROM TO` ([40 §3.7]): a `confirmed` `path_moves` entry on the root node and the glob rewrites; the
/// confirm roles only (WV-039).
#[test]
fn links_fix_prefix_confirms_a_directory_move() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    s.ok(
        tx(vec![set(
            3,
            &[("files_owned", P::List(vec![t("docs/**"), t("src/*.rs")]))],
        )]),
        orch(),
    );
    let mut p = fix("#4", "prefix");
    p.from = Some("docs".into());
    p.to = Some("handbook".into());
    s.ok(links_fix(p), in_tree());
    let st = tip_state(&s);
    let Some(Value::Set(ms)) = st.nodes[&Nid(4)].fields.get("path_moves") else {
        panic!()
    };
    let Value::PathMove(m) = &ms[0] else { panic!() };
    assert_eq!(m.class, MoveClass::Confirmed);
    assert_eq!(
        (m.from.text.as_str(), m.to.text.as_str()),
        ("docs/", "handbook/")
    );
    let Some(Value::Set(globs)) = st.nodes[&Nid(3)].fields.get("files_owned") else {
        panic!()
    };
    assert!(globs.contains(&Value::Text("handbook/**".into())));
    assert!(globs.contains(&Value::Text("src/*.rs".into())));
}

/// `FileRm` ([API §12.4]; [40 §3.5]): without `yes` a dry run with the impact; with `yes` the deletion, the node
/// `removed` with `reason` and `replaced_by`, and the anchors that resolve in the replacement re-pointed to it.
#[test]
fn file_rm_dry_run_then_removal() {
    let mut s = bound();
    s.ok(env_tree(vec![w("docs/v2.md", API_TEXT)]), Ctx::default());
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    link(&mut s, 3, "docs/api.md");
    let rm = |yes: bool| Cmd::FileRm {
        paths: vec!["docs/api.md".into()],
        reason: Some("superseded".into()),
        replaced_by: Some("docs/v2.md".into()),
        trash: false,
        recursive: false,
        yes,
    };
    let r = s.ok(rm(false), in_tree());
    assert_eq!(r.outcome, Outcome::Dry);
    let Data::Intent(d) = &r.data else { panic!() };
    let im = d.impact.as_ref().unwrap();
    assert_eq!(im.links, vec![Nid(3)]);
    assert_eq!(im.anchors, vec!["a1".to_string()]);
    assert!(s.st.files.intents.is_empty(), "a dry run opens no intent");
    let r = s.ok(rm(true), in_tree());
    let Data::Intent(d) = &r.data else { panic!() };
    assert_eq!(d.items[0].outcome, "done");
    assert!(!s.st.files.fs.trees[TREE].files.contains_key("docs/api.md"));
    let st = tip_state(&s);
    assert_eq!(st.nodes[&Nid(5)].status, "removed");
    let q = st
        .nodes
        .iter()
        .find(|(_, x)| {
            x.kind == "artifact" && crate::links::file_node(Nid(0), x, 0).path == "docs/v2.md"
        })
        .map(|(n, _)| *n)
        .unwrap();
    assert_eq!(
        st.nodes[&Nid(5)].fields.get("replaced_by"),
        Some(&Value::Ref(q))
    );
    assert!(
        st.nodes[&Nid(2)]
            .out
            .keys()
            .any(|k| k.kind == "at" && k.dst == q),
        "the span anchor resolves in Q"
    );
    assert!(
        st.nodes[&Nid(3)]
            .out
            .keys()
            .any(|k| k.kind == "at" && k.dst == q),
        "the whole-file anchor"
    );
    let c = &s.st.dag.commits[&r.rev_new.unwrap()];
    assert_eq!(c.stmt_sym.as_deref(), Some("rm"));
}

/// `FileRevert` ([API §12.4]; [40 §3.6]; LH-006) moves a directory move back through the protocol; a plain `revert` of
/// the moving commit warns `graph_only_revert` (LH-005); a commit that closed no intent is `not_found` (`intent`).
#[test]
fn file_revert_and_the_graph_only_revert_warning() {
    let mut s = moved();
    s.ok(
        env_tree(vec![mv("handbook/reference.md", "handbook/api.md")]),
        Ctx::default(),
    );
    let rv = s.run(
        Cmd::Revert {
            commit: "s3".into(),
            onto: None,
            mainline: None,
            message: String::new(),
        },
        Ctx {
            dry: true,
            ..in_tree()
        },
    );
    assert!(
        rv.warnings.contains(&"graph_only_revert".to_string()),
        "{:?}",
        rv.error
    );
    s.refused(
        Cmd::FileRevert {
            commit: "s2".into(),
        },
        in_tree(),
        "not_found",
    );
    let r = s.ok(
        Cmd::FileRevert {
            commit: "s3".into(),
        },
        in_tree(),
    );
    let Data::Intent(d) = &r.data else { panic!() };
    assert_eq!(d.items[0].outcome, "done");
    assert_eq!(
        crate::links::intent::item_text(&d.items[0]),
        ("project:handbook".into(), Some("project:docs".into()))
    );
    assert!(s.st.files.fs.trees[TREE].files.contains_key("docs/api.md"));
    let f = file_node(&s, 5);
    assert_eq!(
        (f.path.as_str(), f.relink.as_deref()),
        ("docs/api.md", Some("explicit/intent"))
    );
    let c = &s.st.dag.commits[&r.rev_new.unwrap()];
    assert_eq!(c.stmt_sym.as_deref(), Some("revert"));
}

/// `file rm --trash` moves the file into the store's trash; `file revert` brings it back and the node is `present`
/// again; a removal without `trash` has no bytes to restore and ends `missing` ([API] open point 38).
#[test]
fn trash_removals_revert() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    let rm = |trash: bool, p: &str| Cmd::FileRm {
        paths: vec![p.into()],
        reason: None,
        replaced_by: None,
        trash,
        recursive: false,
        yes: true,
    };
    let r = s.ok(rm(true, "docs/api.md"), in_tree());
    let seq = r.rev_new.unwrap();
    assert!(!s.st.files.fs.trees[TREE].files.contains_key("docs/api.md"));
    assert_eq!(s.st.files.intents[0].op, crate::links::IntentOp::RmTrash);
    s.ok(
        Cmd::FileRevert {
            commit: format!("s{seq}"),
        },
        in_tree(),
    );
    assert!(s.st.files.fs.trees[TREE].files.contains_key("docs/api.md"));
    assert_eq!(tip_state(&s).nodes[&Nid(5)].status, "present");
    let r = s.ok(
        Cmd::FileAdd {
            paths: vec!["docs/notes.md".into()],
            kind: None,
            root: None,
        },
        in_tree(),
    );
    let _ = r;
    let r = s.ok(rm(false, "docs/notes.md"), in_tree());
    let seq = r.rev_new.unwrap();
    let r = s.ok(
        Cmd::FileRevert {
            commit: format!("s{seq}"),
        },
        in_tree(),
    );
    assert_eq!(r.exit, 8);
    let Data::Intent(d) = &r.data else { panic!() };
    assert_eq!(d.items[0].outcome, "missing");
    assert_eq!(
        s.st.files.intents.last().unwrap().state,
        IntentState::Aborted(3)
    );
}

/// The plan of the file-system verbs ([API §12.4] step 1): outside the writer tree `not_writer_tree` (exit 5); a
/// missing destination parent `not_found` (`directory`); an item that fails reports its outcome and the rest commit
/// (exit 8); a volume that cannot flush a directory refuses with `no_dir_flush` (exit 7).
#[test]
fn file_mv_plan_and_partial_failure() {
    let mut s = bound();
    link(&mut s, 2, "docs/api.md:3");
    let fm = |srcs: &[&str], dst: &str| Cmd::FileMv {
        srcs: srcs.iter().map(|x| x.to_string()).collect(),
        dst: dst.into(),
        git: false,
        retry_ms: None,
    };
    let r = s.refused(
        fm(&["docs/api.md"], "docs/b.md"),
        in_tree(),
        "not_writer_tree",
    );
    assert_eq!(r.exit, 5);
    bind(&mut s);
    s.refused(fm(&["docs/api.md"], "gone/b.md"), in_tree(), "not_found");
    s.ok(env_tree(vec![mkdir("arch")]), Ctx::default());
    let r = s.ok(fm(&["docs/api.md", "docs/none.md"], "arch"), in_tree());
    assert_eq!(r.exit, 8);
    let Data::Intent(d) = &r.data else { panic!() };
    assert_eq!(
        d.items.iter().map(|i| i.outcome).collect::<Vec<_>>(),
        vec!["done", "missing"]
    );
    assert_eq!(file_node(&s, 5).path, "arch/api.md");
    let mut caps = crate::r4::tree::VolumeCaps::NTFS;
    caps.dir_flush_doubtful = true;
    s.st.files.fs.trees.get_mut(TREE).unwrap().caps = caps;
    let r = s.refused(
        fm(&["arch/api.md"], "docs/api.md"),
        in_tree(),
        "no_dir_flush",
    );
    assert_eq!(r.exit, 7);
}

/// `WorktreeBind` on a tree ([F18 §3.5]): a second tree for a designated branch, or a second branch for a designated
/// tree, is `binding_conflict` (exit 5) unless `replace`, whose displaced designated row keeps resolving branches.
#[test]
fn binding_conflicts_and_replace() {
    let mut s = bound();
    bind(&mut s);
    s.ok(
        Cmd::EnvTree {
            tree: "C:/work/other".into(),
            volume: Some("C".into()),
            caps: None,
            ops: vec![w("a.md", "a\n")],
        },
        Ctx::default(),
    );
    let bd = |dir: &str, r: &str, replace: bool| Cmd::WorktreeBind {
        dir: dir.into(),
        ref_: r.into(),
        replace,
    };
    let r = s.refused(
        bd("C:/work/other", "main", false),
        orch(),
        "binding_conflict",
    );
    assert_eq!(r.error.unwrap().get_str("writer_tree"), Some(TREE));
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    s.refused(bd(TREE, "lane/x", false), orch(), "binding_conflict");
    let r = s.ok(bd("C:/work/other", "main", true), orch());
    let Data::Bind(d) = &r.data else { panic!() };
    assert_eq!(d.replaced, vec![(TREE.to_string(), "main".to_string())]);
    let d = s.st.designation();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].tree, "C:/work/other");
    let old =
        s.st.heads
            .get(crate::heads::HeadKind::Directory, TREE)
            .unwrap();
    assert!(!old.designated);
    // A directory that is not a tree binds only.
    let r = s.ok(bd("C:/work/moirai/docs", "lane/x", false), orch());
    assert!(r.warnings.contains(&"not_a_tree".to_string()));
}

/// `Check` ([API §12.7]): the pinned commit of a measurement against the bound worktree's tip; a commit the history
/// does not hold is `unknown`; no commit and no key.
#[test]
fn check_reads_ancestry() {
    let mut s = bound();
    bind(&mut s);
    let head = crate::links::git_oid(Algo::Sha1, &crate::links::git_hex(HEAD)).unwrap();
    s.ok(
        tx(vec![node(
            "m",
            "measurement",
            &[
                ("title", t("latency")),
                ("measured_on", t(HEAD)),
                ("env_host", t("laptop")),
            ],
        )]),
        orch(),
    );
    let before = s.st.commit_seq;
    let r = s.ok(
        Cmd::Check {
            id: Target::Id(Nid(4)),
        },
        in_tree(),
    );
    let Data::Check(c) = &r.data else { panic!() };
    assert_eq!(c.verdict, "ancestor");
    assert_eq!(c.commit.as_deref(), Some(HEAD));
    assert_eq!(c.tip.as_deref(), Some(HEAD));
    assert_eq!(s.st.commit_seq, before, "Check never commits");
    assert!(!s.st.files.facts.is_empty());
    let _ = head;
}

/// `EnvCrash in-next` interrupts a `FileMv`: the result is `outcome_unknown` and the candidate without the command keeps
/// the tree and opens no intent ([API §6.7]); a directory move is of the bulk class ([API §9.10]).
#[test]
fn a_crash_interrupts_a_file_move() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    s.ok(Cmd::EnvCrash { in_next: true }, Ctx::default());
    let r = s.run(
        Cmd::FileMv {
            srcs: vec!["docs".into()],
            dst: "handbook".into(),
            git: false,
            retry_ms: None,
        },
        in_tree(),
    );
    assert_eq!(r.error.unwrap().code, "outcome_unknown");
    assert!(s.st.reserved.is_some(), "a directory move is bulk class");
    s.st.adopt_without();
    assert!(s.st.files.intents.is_empty());
    assert!(s.st.files.fs.trees[TREE].files.contains_key("docs/api.md"));
}

/// `Mutation` with a file named mutation is its command ([API §9.7]): `tx.link_file` captures as `LinkFile` does.
#[test]
fn a_file_named_mutation_runs_its_command() {
    let mut s = bound();
    bind(&mut s);
    let r = s.ok(
        Cmd::Mutation {
            name: "tx.link_file".into(),
            params: vec![
                ("node".into(), t("#2")),
                ("spec".into(), t("docs/api.md:3")),
            ],
            message: String::new(),
            move_lease: None,
        },
        Ctx {
            door: Door::Mcp,
            ..in_tree()
        },
    );
    assert_eq!(row(&r, "anchor"), "a1");
    let c = &s.st.dag.commits[&r.rev_new.unwrap()];
    assert_eq!(
        (c.stmt_origin, c.stmt_sym.as_deref()),
        ("named-mutation", Some("tx.link_file"))
    );
}

/// The commit header's git group ([API §4.4]; [F07 §3.6] item 5): a command in a tree bound to a history records the
/// repository format, HEAD, the branch, the tree's canonical root text and the binding's base.
#[test]
fn commits_carry_the_tree_git_group() {
    let mut s = bound();
    bind(&mut s);
    let r = s.ok(tx(vec![task("n", "in the tree")]), in_tree());
    let g = s.st.dag.commits[&r.rev_new.unwrap()].git.clone().unwrap();
    let head = crate::links::git_oid(Algo::Sha1, &crate::links::git_hex(HEAD)).unwrap();
    assert_eq!(g.algo, Algo::Sha1);
    assert_eq!(g.head, Some(head.digest.clone()));
    assert_eq!((g.branch.as_str(), g.worktree.as_str()), ("main", TREE));
    assert_eq!(g.base, Some(head.digest));
    let r = s.ok(tx(vec![task("m", "outside")]), orch());
    assert!(s.st.dag.commits[&r.rev_new.unwrap()].git.is_none());
}

/// `Complete`'s link settle ([API §10.5] step 4): the completed task's links in its branch's designated tree are
/// settled in a separate commit of the command, named by the yields' `settle_commit`.
#[test]
fn complete_settles_the_task_links() {
    let mut s = bound();
    bind(&mut s);
    let r = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: Some("developer".into()),
            agent: Some("dev".into()),
            ttl: None,
            start: true,
            run: None,
            session: false,
        },
        orch(),
    );
    let lease = r.yields[0].rows[0][0].1.clone();
    let dev = Ctx {
        lease: Some(lease),
        tree: Some(TREE.into()),
        ..Default::default()
    };
    link(&mut s, 2, "docs/api.md:3");
    s.ok(
        env_tree(vec![mv("docs/api.md", "docs/moved.md")]),
        Ctx::default(),
    );
    s.ok(
        env_git(
            HEAD2,
            &[
                (
                    "docs/moved.md",
                    "sha1:4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d",
                ),
                (
                    "docs/notes.md",
                    "sha1:5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e",
                ),
            ],
            &[HEAD],
        ),
        Ctx::default(),
    );
    let r = s.ok(
        Cmd::Complete {
            id: Target::Id(Nid(2)),
            outcome: "done".into(),
            summary: "written".into(),
            evidence: vec![],
            move_lease: None,
        },
        dev,
    );
    let settle = r.yields[0].rows[0]
        .iter()
        .find(|(k, _)| k == "settle_commit")
        .map(|(_, v)| v.clone())
        .unwrap();
    assert_eq!(settle, format!("s{}", r.rev_new.unwrap() + 1));
    assert_eq!(file_node(&s, 5).path, "docs/moved.md");
}

/// The runtime snapshot's `intents` ([API §15.7]): each intent with its state and outcomes; a closed one leaves after
/// `gc.trash-expire`.
#[test]
fn runtime_intents_keep_their_window() {
    let mut s = moved();
    let rt = s.st.runtime();
    assert_eq!(rt.intents.len(), 1);
    assert_eq!(rt.intents[0].items[0].outcome, 1);
    s.ok(
        Cmd::EnvClock(crate::clock::EnvClock {
            advance_ms: Some(15 * 86_400_000),
            ..Default::default()
        }),
        Ctx::default(),
    );
    assert!(s.st.runtime().intents.is_empty());
}

/// A reader tree writes `PENDING` rows only ([API §12.6]; [40 §5.3]): the tree designated for `lane/x` settles `main`'s
/// link without a commit and keeps the observation for the writer tree.
#[test]
fn a_reader_tree_writes_pending_rows_only() {
    let mut s = bound();
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    s.ok(
        Cmd::WorktreeBind {
            dir: TREE.into(),
            ref_: "lane/x".into(),
            replace: false,
        },
        orch(),
    );
    link(&mut s, 2, "docs/api.md:3");
    s.ok(
        env_tree(vec![mv("docs/api.md", "docs/b.md")]),
        Ctx::default(),
    );
    let (c, ctx) = sync(None);
    let r = s.ok(c, ctx);
    assert_eq!(r.rev_new, None);
    assert_eq!(row(&r, "pending"), "1");
    let p = &s.st.files.rt.pending[0];
    assert_eq!(
        (p.n, p.from.as_str(), p.to.as_str()),
        (5, "docs/api.md", "docs/b.md")
    );
    assert_eq!(p.source, crate::r4::cascade::PendingSource::ReaderSettle);
}

/// [API §4.3] row 5: a tree-derived write with `ctx.tree` outside the presented task lease's lane tree is
/// `tree_mismatch` (exit 5), with the lane tree.
#[test]
fn a_tree_outside_the_lease_lane_is_refused() {
    let mut s = bound();
    s.ok(
        tx(vec![node(
            "l",
            "lane",
            &[
                ("title", t("x")),
                ("moirai_branch", t("lane/x")),
                ("worktree_path", t("abs:C:/work/lane")),
            ],
        )]),
        orch(),
    );
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    let r = s.ok(
        Cmd::Claim {
            ids: vec![Target::Id(Nid(2))],
            next: false,
            scope: None,
            role: Some("developer".into()),
            agent: Some("dev".into()),
            ttl: None,
            start: false,
            run: None,
            session: false,
        },
        orch_on("lane/x"),
    );
    let lease = r.yields[0].rows[0][0].1.clone();
    let r = s.refused(
        Cmd::LinksSync {
            scope: None,
            budget_ms: None,
            since: None,
            deep: false,
            all: false,
            force: false,
        },
        Ctx {
            lease: Some(lease),
            tree: Some(TREE.into()),
            ..Default::default()
        },
        "tree_mismatch",
    );
    let e = r.error.unwrap();
    assert_eq!(e.exit, 5);
    assert_eq!(e.get_str("lane_tree"), Some("C:/work/lane"));
}

/// I-F1 at write time ([F18 §2.1]): a re-point to a path another live node holds is `path_claimed` (exit 6).
#[test]
fn a_held_path_is_claimed() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    link(&mut s, 3, "docs/notes.md");
    let r = s.refused(
        Cmd::FileRelink {
            from: "docs/api.md".into(),
            to: "docs/notes.md".into(),
        },
        in_tree(),
        "path_claimed",
    );
    let e = r.error.unwrap();
    assert_eq!(e.exit, 6);
    assert_eq!(e.get_str("path"), Some("docs/notes.md"));
}

/// A replay of a `FileMv` rebuilds its data from the intent's records ([API §7.5]); `EnvTree` applies its operations
/// together or not at all.
#[test]
fn file_mv_replays_and_env_tree_is_atomic() {
    let mut s = bound();
    bind(&mut s);
    link(&mut s, 2, "docs/api.md:3");
    let mut ctx = in_tree();
    ctx.key = Some("mv-1".into());
    let mvc = Cmd::FileMv {
        srcs: vec!["docs".into()],
        dst: "handbook".into(),
        git: false,
        retry_ms: None,
    };
    let r = s.ok(mvc.clone(), ctx.clone());
    let again = s.run(mvc, ctx);
    assert_eq!(again.outcome, Outcome::Replayed);
    assert_eq!(again.data, r.data);
    assert_eq!(again.commit, r.commit);
    let before = s.st.files.fs.clone();
    s.refused(
        env_tree(vec![w("x.md", "x\n"), mv("none.md", "y.md")]),
        Ctx::default(),
        "usage",
    );
    assert_eq!(s.st.files.fs, before);
}

/// One step of a random file-link stream.
#[derive(Clone, Debug)]
enum FOp {
    /// A raw move of the file at one name to another (no hook, no intent).
    Raw(usize, usize),
    /// A copy to another name.
    Copy(usize, usize),
    /// A rewrite in place.
    Edit(usize, u8),
    /// A commit of the whole tree on `refs/heads/main`.
    Commit,
    /// `FileMv`.
    Mv(usize, usize),
    /// `FileRm` with `yes`.
    Rm(usize),
    /// `LinkFile` of #2 at the file (whole-file and first-line anchors alternate).
    Link(usize, bool),
    /// `LinksSync`.
    Sync,
}

const NAMES: [&str; 6] = ["a.md", "b.md", "c.md", "d/e.md", "d/f.md", "g.md"];

fn fop() -> impl proptest::strategy::Strategy<Value = FOp> {
    use proptest::prelude::*;
    let i = 0..NAMES.len();
    prop_oneof![
        3 => (i.clone(), i.clone()).prop_map(|(a, b)| FOp::Raw(a, b)),
        1 => (i.clone(), i.clone()).prop_map(|(a, b)| FOp::Copy(a, b)),
        1 => (i.clone(), any::<u8>()).prop_map(|(a, x)| FOp::Edit(a, x)),
        2 => Just(FOp::Commit),
        2 => (i.clone(), i.clone()).prop_map(|(a, b)| FOp::Mv(a, b)),
        1 => i.clone().prop_map(FOp::Rm),
        3 => (i.clone(), any::<bool>()).prop_map(|(a, l)| FOp::Link(a, l)),
        3 => Just(FOp::Sync),
    ]
}

/// The invariants of the file layer on a branch's tip ([F18 §2.1] I-F1, §2.2 I-F2, §2.3 I-F3), and P1 against ground
/// truth ([40 §8.3.2]): each node the last command re-pointed by an automatic re-bind on file identity (`changed`)
/// holds, at its new path, the file it was linked to (an intent follows the path it moved, whatever file is there).
fn file_invariants(
    s: &S,
    branch: &str,
    ids: &BTreeMap<u32, u64>,
    changed: &[u32],
) -> Result<(), String> {
    let tip = s.st.dag.live(branch).and_then(|r| r.tip);
    let st = s.st.dag.state_at(tip, &s.st.alloc);
    let view = s.st.view_of(&st, tip);
    let bad = crate::r4::inv::if1_one_live_file_per_path(&view.files);
    if !bad.is_empty() {
        return Err(format!("I-F1: {bad:?}"));
    }
    for (n, x) in &st.nodes {
        if x.kind == "artifact" {
            let origin = match x.fields.get("origin_path") {
                Some(Value::Path(p)) => p.text.clone(),
                _ => return Err(format!("{n}: no origin_path")),
            };
            let pred = match x.fields.get("origin_pred") {
                Some(Value::Ref(m)) => Some(s.st.alloc.uids[m]),
                _ => None,
            };
            if crate::r4::uid::uid_file(x.text("root").unwrap_or("project"), &origin, pred) != x.uid
            {
                return Err(format!("I-F2: {n}"));
            }
        }
        if x.kind == "area"
            && let Some(r) = x.text("root")
            && crate::r4::uid::uid_root(r) != x.uid
        {
            return Err(format!("I-F2: root node {n}"));
        }
        for (k, p) in x.out.iter().filter(|(k, _)| k.kind == "at") {
            let (Some(d), Some(a)) = (k.disc, p.anchor.as_deref()) else {
                return Err(format!("I-F3: {n} -at-> {}", k.dst));
            };
            let pred = a.pred.map(crate::value::Uid);
            if crate::r4::uid::uid_anchor(x.uid, a.captured, pred) != d {
                return Err(format!("I-F2: anchor of {n}"));
            }
        }
    }
    let t = &s.st.files.fs.trees[TREE];
    for f in view.files.iter().filter(|f| {
        !f.tombstone && f.status == crate::r4::uid::FileStatus::Present && changed.contains(&f.n)
    }) {
        let exact = matches!(
            f.relink.as_deref(),
            Some("lazy/file-id" | "lazy/dir-id" | "hook/file-id")
        );
        if let (true, Some(id)) = (exact, ids.get(&f.n)) {
            match t.stat(&f.path) {
                crate::r4::tree::StatOut::Present(x) if x.id.id == *id => {}
                other => {
                    return Err(format!(
                        "P1: #{} at {} ({other:?}), linked to file {id}",
                        f.n, f.path
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Random file-link streams on a lane whose designated tree is the simulated tree ([60 §4.2] row "File links"): raw
/// moves, copies, edits, commits, `FileMv`, `FileRm`, `LinkFile` and `LinksSync` keep I-F1, I-F2 and I-F3 on every
/// state, and no exact automatic or intent re-bind points at another file than the one linked (P1).
#[test]
fn random_file_streams_keep_the_invariants() {
    let mut runner = crate::r4::tests::runner(24);
    // Over all cases: settles that re-bound a link, and intents carried out.
    let rebinds = std::cell::Cell::new((0usize, 0usize));
    let result = runner.run(&proptest::collection::vec(fop(), 4..24), |ops| {
        let mut s = bound();
        s.ok(
            Cmd::BranchCreate {
                name: "x".into(),
                from: Some("main".into()),
                kind: None,
            },
            orch(),
        );
        s.ok(
            Cmd::WorktreeBind {
                dir: TREE.into(),
                ref_: "lane/x".into(),
                replace: false,
            },
            orch(),
        );
        let lane = || Ctx {
            branch: Some("lane/x".into()),
            no_dedupe: true,
            ..in_tree()
        };
        s.ok(
            env_tree(vec![
                w("a.md", "alpha one\nalpha two\n"),
                w("b.md", "beta one\nbeta two\n"),
            ]),
            Ctx::default(),
        );
        let mut ids: BTreeMap<u32, u64> = BTreeMap::new();
        let mut head = HEAD.to_string();
        let mut commits = 0u32;
        for op in &ops {
            let before = s.st.dag.live("lane/x").and_then(|r| r.tip);
            let t = &s.st.files.fs.trees[TREE];
            let at = |i: usize| t.files.contains_key(NAMES[i]);
            match op {
                FOp::Raw(a, b) if at(*a) && !at(*b) && a != b => {
                    s.ok(env_tree(vec![mv(NAMES[*a], NAMES[*b])]), Ctx::default());
                }
                FOp::Copy(a, b) if at(*a) && !at(*b) && a != b => {
                    s.ok(
                        env_tree(vec![TreeOp::Cp {
                            from: NAMES[*a].into(),
                            to: NAMES[*b].into(),
                            keep_btime: false,
                        }]),
                        Ctx::default(),
                    );
                }
                FOp::Edit(a, x) if at(*a) => {
                    let text = format!("edited {x}\nsecond line {x}\n");
                    s.ok(env_tree(vec![w(NAMES[*a], &text)]), Ctx::default());
                }
                FOp::Commit => {
                    commits += 1;
                    let id = format!("sha1:{:040x}", 0xa000 + commits);
                    let tree: Vec<(String, String)> = t
                        .files
                        .iter()
                        .map(|(p, f)| {
                            let o = crate::r4::text::oid(Algo::Sha1, &f.bytes);
                            (p.clone(), format!("sha1:{}", crate::value::hex(&o.digest)))
                        })
                        .collect();
                    let tree: Vec<(&str, &str)> =
                        tree.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
                    s.ok(env_git(&id, &tree, &[&head]), Ctx::default());
                    head = id;
                }
                FOp::Mv(a, b)
                    if at(*a) && !at(*b) && a != b && !NAMES[*b].starts_with(NAMES[*a]) =>
                {
                    let r = s.run(
                        Cmd::FileMv {
                            srcs: vec![NAMES[*a].into()],
                            dst: NAMES[*b].into(),
                            git: false,
                            retry_ms: None,
                        },
                        lane(),
                    );
                    prop_ok(&r, &["not_found", "path_claimed", "not_fresh"])?;
                    if r.outcome == Outcome::Ok {
                        let (a, b) = rebinds.get();
                        rebinds.set((a, b + 1));
                    }
                }
                FOp::Rm(a) if at(*a) => {
                    let r = s.run(
                        Cmd::FileRm {
                            paths: vec![NAMES[*a].into()],
                            reason: None,
                            replaced_by: None,
                            trash: false,
                            recursive: false,
                            yes: true,
                        },
                        lane(),
                    );
                    prop_ok(&r, &["not_fresh"])?;
                }
                FOp::Link(a, line) if at(*a) => {
                    let spec = if *line {
                        format!("{}:1", NAMES[*a])
                    } else {
                        NAMES[*a].to_string()
                    };
                    let id = t.files[NAMES[*a]].id;
                    let r = s.run(
                        Cmd::LinkFile {
                            node: Target::Id(Nid(2)),
                            specs: vec![spec],
                            watch: None,
                            planned: false,
                            quote: None,
                            end: None,
                        },
                        lane(),
                    );
                    prop_ok(&r, &["path_claimed"])?;
                    if r.outcome == Outcome::Ok {
                        let n = r.yields[0].rows[0][0]
                            .1
                            .trim_start_matches('#')
                            .parse::<u32>()
                            .unwrap();
                        ids.entry(n).or_insert(id);
                    }
                }
                FOp::Sync => {
                    let (c, _) = sync(None);
                    let r = s.run(c, lane());
                    prop_ok(&r, &[])?;
                    if row(&r, "rebound") != "[]" {
                        let (a, b) = rebinds.get();
                        rebinds.set((a + 1, b));
                    }
                }
                _ => continue,
            }
            let last = s.st.dag.live("lane/x").and_then(|r| r.tip);
            let changed: Vec<u32> = last
                .filter(|c| Some(*c) != before)
                .map(|c| {
                    s.st.dag.commits[&c]
                        .changeset
                        .keys()
                        .filter_map(|k| match k {
                            Key::Node(n, Aspect::Observation) => Some(n.0),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default();
            if let Err(e) = file_invariants(&s, "lane/x", &ids, &changed) {
                return Err(proptest::test_runner::TestCaseError::fail(format!(
                    "after {op:?}: {e}"
                )));
            }
        }
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
    let (settled, moved) = rebinds.get();
    assert!(
        settled > 0 && moved > 0,
        "{settled} re-binding settles, {moved} file moves"
    );
}

/// A command of a random stream either lands or is refused with one of the codes the step allows.
fn prop_ok(r: &Reply, allowed: &[&str]) -> Result<(), proptest::test_runner::TestCaseError> {
    match (&r.outcome, &r.error) {
        (Outcome::Ok | Outcome::Dry, _) => Ok(()),
        (Outcome::Refused, Some(e)) if allowed.contains(&e.code.as_str()) => Ok(()),
        (_, e) => Err(proptest::test_runner::TestCaseError::fail(format!(
            "unexpected {:?}: {e:?}",
            r.outcome
        ))),
    }
}

/// The hooks that write for R4 ([API §18]; [RULES/role-write-policy] WH-007, WH-008): the git hooks settle the tree's
/// links with `LinksSync` through the door `hook` and refresh the displayed provenance as a lazy record; the evidence
/// hooks append lazy rows only.
#[test]
fn the_git_hooks_settle_through_links_sync() {
    use crate::hooks::{Write as HW, fs_evidence, git_hooks, run};
    let mut s = moved();
    s.ok(
        env_git(
            HEAD2,
            &[
                (
                    "handbook/notes.md",
                    "sha1:5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e",
                ),
                (
                    "handbook/reference.md",
                    "sha1:4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d",
                ),
            ],
            &[HEAD],
        ),
        Ctx::default(),
    );
    let w = git_hooks(&s.st.conf, &in_tree());
    assert!(w.contains(&HW::Lazy("binding-refresh")));
    assert!(
        matches!(&w[0], HW::Command(c, cx) if matches!(**c, Cmd::LinksSync { .. }) && cx.door == Door::Hook)
    );
    let r = run(&mut s.st, &w);
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].outcome, Outcome::Ok);
    assert_eq!(file_node(&s, 5).relink.as_deref(), Some("lazy/file-id"));
    assert_eq!(fs_evidence(), vec![HW::Lazy("runtime-evidence")]);
}

/// `EnvGit` adds a commit once and never changes it; a changed commit is refused and changes nothing ([API §6.6]).
#[test]
fn env_git_commits_never_change() {
    let mut s = bound();
    let before = s.st.files.git.clone();
    let r = s.run(
        env_git(
            HEAD,
            &[(
                "docs/api.md",
                "sha1:6161616161616161616161616161616161616161",
            )],
            &[],
        ),
        Ctx::default(),
    );
    assert_eq!(r.error.unwrap().code, "usage");
    assert_eq!(s.st.files.git, before);
}

/// One `FILEOBS.creation` rule for every writer of the row ([F20 §5.2]; [F11 §12.5]; [`crate::r4::tree::Tree`]'s
/// `recorded_creation`): on a volume without creation times (`VolumeCaps.btime` = `absent`, FAT) a file verb records
/// none, as a settle does; on NTFS it records the stat's.
#[test]
fn file_verbs_record_creation_times_only_where_the_volume_has_them() {
    let add = || Cmd::FileAdd {
        paths: vec!["docs/notes.md".into()],
        kind: None,
        root: None,
    };
    let creation = |s: &S| {
        let rows: Vec<Option<i64>> =
            s.st.files
                .rt
                .fileobs
                .iter()
                .filter(|((_, t), _)| t == TREE)
                .map(|(_, r)| r.creation_ns)
                .collect();
        assert_eq!(rows.len(), 1, "{rows:?}");
        rows[0]
    };
    let mut fat = S::base();
    fat.ok(
        Cmd::EnvTree {
            tree: TREE.into(),
            volume: Some("F".into()),
            caps: Some(crate::r4::tree::VolumeCaps::FAT),
            ops: vec![mkdir("docs"), w("docs/notes.md", "notes\n")],
        },
        Ctx::default(),
    );
    bind(&mut fat);
    fat.ok(add(), in_tree());
    assert_eq!(creation(&fat), None);
    let mut ntfs = bound();
    bind(&mut ntfs);
    ntfs.ok(add(), in_tree());
    assert!(creation(&ntfs).is_some());
}

/// A context on `lane/<l>` in the tree, without the per-key dedupe, so that one command may run on two lanes.
fn on_lane(l: &str) -> Ctx {
    Ctx {
        branch: Some(format!("lane/{l}")),
        no_dedupe: true,
        ..in_tree()
    }
}

/// Binds the tree to `lane/<l>`, replacing its binding when `replace`.
fn bind_lane(s: &mut S, l: &str, replace: bool) {
    s.ok(
        Cmd::WorktreeBind {
            dir: TREE.into(),
            ref_: format!("lane/{l}"),
            replace,
        },
        orch(),
    );
}

/// `FileAdd` of `paths` on `lane/<l>`: the `#N` of each, in the order given.
fn add_on(s: &mut S, l: &str, paths: &[&str]) -> Vec<Nid> {
    let r = s.ok(
        Cmd::FileAdd {
            paths: paths.iter().map(|p| p.to_string()).collect(),
            kind: None,
            root: None,
        },
        on_lane(l),
    );
    let Data::FileAdd(files) = &r.data else {
        panic!("{:?}", r.data)
    };
    files.iter().map(|f| f.1).collect()
}

/// `merge lane/<src> --into lane/<dst>`.
fn merge_lanes(s: &mut S, src: &str, dst: &str) -> crate::history::MergeData {
    let r = s.ok(
        Cmd::Merge {
            src: format!("lane/{src}"),
            into: Some(format!("lane/{dst}")),
            policy: None,
            strict: None,
            base: None,
            message: String::new(),
        },
        orch(),
    );
    let Data::Merge(d) = r.data else {
        panic!("{:?}", r.data)
    };
    *d
}

/// The state at the tip of a ref.
fn state_of(s: &S, r: &str) -> std::rc::Rc<crate::state::State> {
    let tip = s.st.dag.live(r).and_then(|x| x.tip);
    s.st.dag.state_at(tip, &s.st.alloc)
}

/// The lanes of an RK re-key ([RULES/link-merge-rules] LC-003): `lane/x` and `lane/y` fork from `main`, which holds
/// neither file; `lane/x` registers `docs/f.md` (#5) and `docs/g.md` (#6), links task #2 to `docs/f.md` (an `at` edge
/// with its anchor) and lets task #3's body name #6 (a `mentions` edge); `lane/y` registers both files (the same uids,
/// so the same `#N`s, I1), defines a named query that names #5, and deletes both nodes.
fn rekey_lanes() -> S {
    let mut s = bound();
    s.ok(
        env_tree(vec![w("docs/f.md", "eff\n"), w("docs/g.md", "gee\n")]),
        Ctx::default(),
    );
    for l in ["x", "y"] {
        s.ok(
            Cmd::BranchCreate {
                name: l.into(),
                from: Some("main".into()),
                kind: None,
            },
            orch(),
        );
    }
    bind_lane(&mut s, "x", false);
    assert_eq!(
        add_on(&mut s, "x", &["docs/f.md", "docs/g.md"]),
        vec![Nid(5), Nid(6)]
    );
    s.ok(
        Cmd::LinkFile {
            node: Target::Id(Nid(2)),
            specs: vec!["docs/f.md".into()],
            watch: None,
            planned: false,
            quote: None,
            end: None,
        },
        on_lane("x"),
    );
    s.ok(
        tx(vec![Stmt::Set {
            target: Target::Id(Nid(3)),
            fields: vec![],
            incr: vec![],
            body: Some(Some("see #6".into())),
            guard: None,
        }]),
        orch_on("lane/x"),
    );
    bind_lane(&mut s, "y", true);
    assert_eq!(
        add_on(&mut s, "y", &["docs/g.md", "docs/f.md"]),
        vec![Nid(6), Nid(5)]
    );
    s.ok(
        Cmd::TxLq {
            lq: "TX { DEFINE QUERY the_f() SHAPE node AS { MATCH (a:artifact) WHERE a = #5 RETURN a } }"
                .into(),
            params: crate::lq::ctx::Params::new(),
            message: String::new(),
            if_targets: None,
        },
        orch_on("lane/y"),
    );
    for n in [5, 6] {
        s.ok(
            tx(vec![Stmt::Delete {
                target: Target::Id(Nid(n)),
                policy: None,
                replaced_by: None,
                release: false,
                reason: None,
            }]),
            orch_on("lane/y"),
        );
    }
    s
}

/// A merge that re-keys file nodes lands, in either direction, the RK rows' result ([RULES/link-merge-rules] RK-001 to
/// RK-010; [F12 §7.6]): a file uid U absent in the base, live on one side S and deleted on the other (LC-003) moves on
/// S to uid′ = `uid_file(root, origin_path, U)` (RK-003; RK-009: the uid a registration after the removal derives),
/// with `origin_pred` = U (RK-005) and every edge S added to U re-pointed to uid′, the `at` edge with its anchor
/// unchanged and the `mentions` edge alike (RK-006); U keeps the other side's state, a tombstone, and is not live
/// (RK-007, RK-010); each uid′ gets a new `#N`, in ascending uid order ([API §9.6] item 2), and U keeps its own
/// (RK-008). The validators read the candidate while it holds the merge's provisional `#N`s for the uid′s, and V10
/// binds `lane/y`'s named query, which names U, against it on the merge into `lane/x` (ADV-C-5: recording those `#N`s
/// overflowed `next_id`). Both directions land equal states ([F12 §7.7]).
#[test]
fn a_merge_that_re_keys_file_nodes_lands_their_successors_in_either_direction() {
    let mut states = Vec::new();
    for (src, dst) in [("y", "x"), ("x", "y")] {
        let mut s = rekey_lanes();
        let before = state_of(&s, "lane/x");
        let anchor = before.nodes[&Nid(2)]
            .out
            .iter()
            .find(|(k, _)| k.kind == "at" && k.dst == Nid(5))
            .map(|(_, p)| p.anchor.clone())
            .expect("#2's anchor in docs/f.md");
        let next = s.st.next_id;
        let d = merge_lanes(&mut s, src, dst);
        assert_eq!(d.outcome, "landed");
        assert!(
            d.conflicts.is_empty() && d.violations.is_empty(),
            "{:?} {:?}",
            d.conflicts,
            d.violations
        );
        let st = state_of(&s, &format!("lane/{dst}"));
        let succ = |n: u32, path: &str| {
            let u = s.st.alloc.uids[&Nid(n)];
            let u2 = crate::r4::uid::uid_file("project", path, Some(u));
            (u2, s.st.alloc.uidx[&u2])
        };
        let (uf, nf) = succ(5, "docs/f.md");
        let (ug, ng) = succ(6, "docs/g.md");
        assert_eq!(s.st.next_id, next + 2, "RK-008: two new #Ns");
        let mut fresh = [(uf, nf), (ug, ng)];
        fresh.sort();
        assert_eq!(
            fresh.map(|(_, n)| n),
            [Nid(next), Nid(next + 1)],
            "[API §9.6] item 2: ascending uid order"
        );
        for (n, nn, p) in [(5, nf, "docs/f.md"), (6, ng, "docs/g.md")] {
            let u = &st.nodes[&Nid(n)];
            assert!(!u.live(), "RK-007, RK-010: #{n} keeps the deletion");
            let x = &st.nodes[&nn];
            assert!(x.live() && x.status == "present", "{x:?}");
            assert_eq!(x.fields.get("origin_pred"), Some(&Value::Ref(Nid(n))));
            assert_eq!(x.fields.get("origin_path"), path(p).as_ref());
            assert_eq!(x.fields.get("path"), path(p).as_ref());
            assert_eq!(
                x.fields.get("oid"),
                before.nodes[&Nid(n)].fields.get("oid"),
                "RK-005: S's observation moves with the node"
            );
        }
        let at: Vec<_> = st.nodes[&Nid(2)]
            .out
            .iter()
            .filter(|(k, _)| k.kind == "at")
            .map(|(k, p)| (k.dst, p.anchor.clone()))
            .collect();
        assert_eq!(at, vec![(nf, anchor)], "RK-006: the anchor follows");
        assert!(
            st.nodes[&Nid(3)]
                .out
                .keys()
                .any(|k| k.kind == "mentions" && k.dst == ng)
                && !st.nodes[&Nid(3)].out.keys().any(|k| k.dst == Nid(6)),
            "RK-006: a non-anchor edge follows"
        );
        assert!(st.schema.query("the_f").is_some());
        states.push((st.nodes.clone(), uf, ug));
    }
    assert_eq!(states[0], states[1], "RK-009; [F12 §7.7]");
}

/// RK-004: a uid′ that names a node of the merge's states is derived again over it. `lane/y` removes `docs/f.md` and
/// registers the re-created file, whose uid is `uid_file(root, path, U)` (#6); the merge into `lane/x`, which holds U
/// live, moves U to `uid_file(root, path, #6's uid)` with `origin_pred` = #6, the predecessor of the last derivation
/// (RK-005), and a new `#N` (RK-008); U keeps `lane/y`'s `removed` state (RK-007) and claims no path (PC-005), while
/// the two live files at `docs/f.md` each hold a `PathClaim` conflict value (PC-001, PC-002), which lands (PC-004).
#[test]
fn a_re_key_derives_again_past_a_successor_the_other_side_registered() {
    let mut s = bound();
    s.ok(env_tree(vec![w("docs/f.md", "eff\n")]), Ctx::default());
    for l in ["x", "y"] {
        s.ok(
            Cmd::BranchCreate {
                name: l.into(),
                from: Some("main".into()),
                kind: None,
            },
            orch(),
        );
    }
    bind_lane(&mut s, "x", false);
    assert_eq!(add_on(&mut s, "x", &["docs/f.md"]), vec![Nid(5)]);
    bind_lane(&mut s, "y", true);
    assert_eq!(add_on(&mut s, "y", &["docs/f.md"]), vec![Nid(5)]);
    s.ok(
        Cmd::FileRm {
            paths: vec!["docs/f.md".into()],
            reason: None,
            replaced_by: None,
            trash: false,
            recursive: false,
            yes: true,
        },
        on_lane("y"),
    );
    s.ok(env_tree(vec![w("docs/f.md", "new eff\n")]), Ctx::default());
    assert_eq!(add_on(&mut s, "y", &["docs/f.md"]), vec![Nid(6)]);
    let u = s.st.alloc.uids[&Nid(5)];
    let u1 = crate::r4::uid::uid_file("project", "docs/f.md", Some(u));
    assert_eq!(
        s.st.alloc.uids[&Nid(6)],
        u1,
        "F08 §11.2: the registration after the removal"
    );
    let d = merge_lanes(&mut s, "y", "x");
    assert_eq!(d.outcome, "landed");
    let classes: Vec<&str> = d.conflicts.iter().map(|(_, c)| c.as_str()).collect();
    assert_eq!(classes, vec!["PathClaim", "PathClaim"], "{:?}", d.conflicts);
    let u2 = crate::r4::uid::uid_file("project", "docs/f.md", Some(u1));
    let n2 = s.st.alloc.uidx[&u2];
    assert_eq!(n2, Nid(7), "RK-008");
    let st = state_of(&s, "lane/x");
    let x = &st.nodes[&n2];
    assert!(x.live() && x.status == "present");
    assert_eq!(x.fields.get("origin_pred"), Some(&Value::Ref(Nid(6))));
    let old = &st.nodes[&Nid(5)];
    assert!(
        old.live() && old.status == "removed",
        "RK-007: lane/y's state"
    );
    assert!(!old.conflicts.contains_key(&Aspect::Observation), "PC-005");
    for n in [Nid(6), n2] {
        assert_eq!(
            st.nodes[&n]
                .conflicts
                .get(&Aspect::Observation)
                .map(|c| c.class.as_str()),
            Some("PathClaim"),
            "PC-002: {n}"
        );
    }
}

/// A `sync` that re-keys file nodes lands their successors in ascending uid order ([RULES/link-merge-rules] RK-001 to
/// RK-003, RK-008), on the `sync` path of ADV-C-5, where S is dst: `lane/x` registers `docs/a.md`, `docs/b.md` and
/// `docs/c.md` (#5 to #7); `main` registers the same files (the same uids, so the same `#N`s) and deletes them. `sync
/// lane/x` moves each to its uid′ on lane/x; the three uid′s hold the provisional `#N`s 2^32 − 1 to 2^32 − 3 while the
/// validators run and land as three new `#N`s in ascending uid order ([API §9.6] item 2), and every commit's cached
/// state equals the fold of its changesets.
#[test]
fn a_sync_that_re_keys_three_file_nodes_lands_them_in_uid_order() {
    let mut s = bound();
    s.ok(
        env_tree(vec![
            w("docs/a.md", "a\n"),
            w("docs/b.md", "b\n"),
            w("docs/c.md", "c\n"),
        ]),
        Ctx::default(),
    );
    s.ok(
        Cmd::BranchCreate {
            name: "x".into(),
            from: Some("main".into()),
            kind: None,
        },
        orch(),
    );
    let files = ["docs/a.md", "docs/b.md", "docs/c.md"];
    bind_lane(&mut s, "x", false);
    assert_eq!(add_on(&mut s, "x", &files), vec![Nid(5), Nid(6), Nid(7)]);
    s.ok(
        Cmd::WorktreeBind {
            dir: TREE.into(),
            ref_: "main".into(),
            replace: true,
        },
        orch(),
    );
    let on_main = Ctx {
        branch: Some("main".into()),
        no_dedupe: true,
        ..in_tree()
    };
    let r = s.ok(
        Cmd::FileAdd {
            paths: files.iter().map(|p| p.to_string()).collect(),
            kind: None,
            root: None,
        },
        on_main,
    );
    let Data::FileAdd(added) = &r.data else {
        panic!("{:?}", r.data)
    };
    assert_eq!(
        added.iter().map(|f| f.1).collect::<Vec<_>>(),
        vec![Nid(5), Nid(6), Nid(7)]
    );
    for n in [5, 6, 7] {
        s.ok(
            tx(vec![Stmt::Delete {
                target: Target::Id(Nid(n)),
                policy: None,
                replaced_by: None,
                release: false,
                reason: None,
            }]),
            orch_on("main"),
        );
    }
    let next = s.st.next_id;
    let r = s.ok(
        Cmd::Sync {
            lane: Some("lane/x".into()),
            check: false,
        },
        Ctx {
            no_dedupe: true,
            ..orch_on("lane/x")
        },
    );
    let Data::Merge(d) = &r.data else {
        panic!("{:?}", r.data)
    };
    assert_eq!(d.outcome, "landed", "{:?} {:?}", d.conflicts, d.violations);
    assert_eq!(s.st.next_id, next + 3, "RK-008: three new #Ns");
    let mut fresh: Vec<_> = [(5, "docs/a.md"), (6, "docs/b.md"), (7, "docs/c.md")]
        .iter()
        .map(|(n, p)| {
            let u = s.st.alloc.uids[&Nid(*n)];
            let u2 = crate::r4::uid::uid_file("project", p, Some(u));
            (u2, s.st.alloc.uidx[&u2])
        })
        .collect();
    fresh.sort();
    assert_eq!(
        fresh.iter().map(|x| x.1).collect::<Vec<_>>(),
        vec![Nid(next), Nid(next + 1), Nid(next + 2)]
    );
    let st = state_of(&s, "lane/x");
    for (_, n) in &fresh {
        assert!(st.nodes[n].live(), "{n}");
    }
    for n in [5, 6, 7] {
        assert!(!st.nodes[&Nid(n)].live(), "#{n}");
    }
    for seq in s.st.dag.commits.keys() {
        assert!(
            *s.st.dag.state_at(Some(*seq), &s.st.alloc)
                == s.st.dag.state_from_scratch(Some(*seq), &s.st.alloc),
            "s{seq}"
        );
    }
    s.st.dag.verify_ids(&s.st.alloc).unwrap();
}
