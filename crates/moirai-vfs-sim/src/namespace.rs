//! The simulated namespace ([F15 §2.3], FM-2, §5): nodes with never-reused identities, the current namespace that
//! operations change at their effect instant, the durable namespace, and the pending operations in issue order with the
//! parents that have synced them.
//!
//! An operation becomes durable only by FM-2.3: (i) every parent synced by a `sync_dir` that started after the effect
//! instant, (ii) every parent's creation durable, (iii) every earlier pending operation on a name it reads or writes
//! durable. At a crash any subset of the pending operations survives, replayed in issue order on the durable namespace;
//! a survivor whose precondition fails at its replay point is lost too, and nodes left without a name are gone
//! ([F15 §2.5] step 3).
//!
//! Both namespaces index names both ways (`(directory, name) → node` and `node → (directory, name)`: a node has at most
//! one name, as the simulator makes no hard links), every node counts the pending operations that name it, and every
//! volume keeps a running total of its files' sizes, so that lookups, reachability, garbage collection and free space
//! cost no scan of the whole namespace.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::sync::Arc;

use moirai_vfs::{RelPath, VfsErrorKind};

use crate::content::Content;

/// The node of the world's root directory, the parent of every absolute path.
pub(crate) const ROOT: u64 = 0;

/// A read error injected on a range of a file (FM-12).
#[derive(Clone, Debug)]
pub(crate) struct ReadError {
    pub(crate) offset: u64,
    pub(crate) len: u64,
    /// Reads left that fail; `None` for a persistent error, which also survives crashes (FM-12.2).
    pub(crate) remaining: Option<u32>,
}

/// A file's data and the faults injected on it.
#[derive(Clone, Debug, Default)]
pub(crate) struct FileNode {
    /// Changed only through [`Ns::edit`], which keeps the volume's size total and the snapshot right.
    pub(crate) content: Content,
    /// Set by `seal` (read-only on disk, [80 §2.5] rule 2).
    pub(crate) sealed: bool,
    pub(crate) read_errors: Vec<ReadError>,
    /// The next mapped read of this file meets a media fault (FM-9.1).
    pub(crate) map_fault: bool,
    /// C(f) as one shared buffer, built for mappings and dropped by every content change (FM-9, FM-10.1: mappings are
    /// coherent with the cache).
    pub(crate) snap: Option<Arc<[u8]>>,
}

/// A node's kind.
#[derive(Clone, Debug)]
pub(crate) enum Kind {
    File(Box<FileNode>),
    /// A directory; `creation_durable` is FM-2.3 (ii) for the operations inside it.
    Dir {
        creation_durable: bool,
    },
}

/// A file or directory.
#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) kind: Kind,
    /// The volume the node lies on ([F15 §5.1] NS-2).
    pub(crate) vol: u32,
    /// Open handles and mappings, of every process (FM-8.3).
    pub(crate) open: u32,
    /// An unlink that waits for the last handle to close (FM-8.3): the name it removes.
    pub(crate) delete_pending: Option<(u64, String)>,
    /// Consecutive sharing violations still to come on this node (FM-8.2); `u64::MAX` for the whole scenario.
    pub(crate) share_block: u64,
}

impl Node {
    pub(crate) fn file(&self) -> Option<&FileNode> {
        match &self.kind {
            Kind::File(f) => Some(f),
            Kind::Dir { .. } => None,
        }
    }

    pub(crate) fn file_mut(&mut self) -> Option<&mut FileNode> {
        match &mut self.kind {
            Kind::File(f) => Some(f),
            Kind::Dir { .. } => None,
        }
    }

    pub(crate) fn is_dir(&self) -> bool {
        matches!(self.kind, Kind::Dir { .. })
    }
}

/// The kind of a namespace operation, as the trace and the crash surface name it.
#[repr(u8)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum NsKind {
    /// Exclusive create of a file, or a directory create.
    Create = 0,
    /// Unlink of a file or removal of an empty directory.
    Remove = 1,
    /// `rename_noreplace`.
    Rename = 2,
    /// `rename_replace`.
    RenameReplace = 3,
    /// The native `swap_dirs` exchange.
    Exchange = 4,
}

/// A namespace operation, by node identity.
#[derive(Clone, Debug)]
pub(crate) enum NsOp {
    Create {
        dir: u64,
        name: String,
        node: u64,
    },
    Remove {
        dir: u64,
        name: String,
        node: u64,
    },
    Rename {
        from: (u64, String),
        to: (u64, String),
        node: u64,
        replace: bool,
        /// The node `to` named when the replacing rename took effect.
        replaced: Option<u64>,
    },
    Exchange {
        a: (u64, String),
        b: (u64, String),
        a_node: u64,
        b_node: u64,
    },
}

impl NsOp {
    pub(crate) fn kind(&self) -> NsKind {
        match self {
            NsOp::Create { .. } => NsKind::Create,
            NsOp::Remove { .. } => NsKind::Remove,
            NsOp::Rename { replace: false, .. } => NsKind::Rename,
            NsOp::Rename { replace: true, .. } => NsKind::RenameReplace,
            NsOp::Exchange { .. } => NsKind::Exchange,
        }
    }

    /// The node the operation acts on (the first one for an exchange).
    pub(crate) fn node(&self) -> u64 {
        match self {
            NsOp::Create { node, .. } | NsOp::Remove { node, .. } | NsOp::Rename { node, .. } => {
                *node
            }
            NsOp::Exchange { a_node, .. } => *a_node,
        }
    }

    /// Every node the operation names or displaces.
    pub(crate) fn nodes(&self) -> Vec<u64> {
        match self {
            NsOp::Create { node, .. } | NsOp::Remove { node, .. } => vec![*node],
            NsOp::Rename { node, replaced, .. } => {
                let mut v = vec![*node];
                v.extend(*replaced);
                v
            }
            NsOp::Exchange { a_node, b_node, .. } => vec![*a_node, *b_node],
        }
    }

    /// The names the operation reads or writes (FM-2.3 (iii)).
    pub(crate) fn names(&self) -> Vec<(u64, &str)> {
        match self {
            NsOp::Create { dir, name, .. } | NsOp::Remove { dir, name, .. } => vec![(*dir, name)],
            NsOp::Rename { from, to, .. } => vec![(from.0, &from.1), (to.0, &to.1)],
            NsOp::Exchange { a, b, .. } => vec![(a.0, &a.1), (b.0, &b.1)],
        }
    }

    /// The parent directories, distinct, in order.
    pub(crate) fn parents(&self) -> Vec<u64> {
        let mut v: Vec<u64> = Vec::with_capacity(2);
        for (d, _) in self.names() {
            if !v.contains(&d) {
                v.push(d);
            }
        }
        v
    }
}

/// A pending operation ([F15 §2.3]).
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    pub(crate) id: u64,
    pub(crate) op: NsOp,
    pub(crate) parents: Vec<u64>,
    pub(crate) synced: Vec<bool>,
}

/// One namespace, indexed both ways.
#[derive(Clone, Debug, Default)]
pub(crate) struct Names {
    fwd: BTreeMap<(u64, String), u64>,
    rev: BTreeMap<u64, (u64, String)>,
}

impl Names {
    /// The node `name` names in `dir`.
    pub(crate) fn get(&self, dir: u64, name: &str) -> Option<u64> {
        self.fwd.get(&(dir, name.to_owned())).copied()
    }

    fn get_key(&self, key: &(u64, String)) -> Option<u64> {
        self.fwd.get(key).copied()
    }

    /// Names `node` as `name` in `dir`, displacing the node that name named.
    pub(crate) fn insert(&mut self, dir: u64, name: String, node: u64) {
        if let Some(old) = self.fwd.insert((dir, name.clone()), node)
            && old != node
        {
            self.rev.remove(&old);
        }
        let prev = self.rev.insert(node, (dir, name));
        debug_assert!(
            prev.is_none() || prev == self.rev.get(&node).cloned(),
            "simulator: a node has one name"
        );
    }

    /// Removes the name, returning the node it named.
    pub(crate) fn remove(&mut self, dir: u64, name: &str) -> Option<u64> {
        let n = self.fwd.remove(&(dir, name.to_owned()))?;
        self.rev.remove(&n);
        Some(n)
    }

    /// The directory and name of `node`.
    pub(crate) fn name_of(&self, node: u64) -> Option<&(u64, String)> {
        self.rev.get(&node)
    }

    /// Whether `node` has a name.
    pub(crate) fn names_node(&self, node: u64) -> bool {
        self.rev.contains_key(&node)
    }

    /// The entries of directory `dir`, by name.
    pub(crate) fn children(&self, dir: u64) -> impl Iterator<Item = (&str, u64)> {
        self.fwd
            .range((dir, String::new())..)
            .take_while(move |((d, _), _)| *d == dir)
            .map(|((_, n), &v)| (n.as_str(), v))
    }

    /// Whether directory `dir` has an entry.
    pub(crate) fn has_children(&self, dir: u64) -> bool {
        self.children(dir).next().is_some()
    }

    /// Whether `dir` exists in this namespace (the root always does).
    fn present(&self, dir: u64) -> bool {
        dir == ROOT || self.rev.contains_key(&dir)
    }

    /// Keeps only the entries whose directory is in `keep`.
    fn retain_dirs(&mut self, keep: &BTreeSet<u64>) {
        self.fwd.retain(|(d, _), _| keep.contains(d));
        self.rev.retain(|_, (d, _)| keep.contains(d));
    }

    /// The nodes reachable from the root.
    fn reachable(&self) -> BTreeSet<u64> {
        let mut reach = BTreeSet::from([ROOT]);
        let mut stack = vec![ROOT];
        while let Some(d) = stack.pop() {
            for (_, n) in self.children(d) {
                if reach.insert(n) {
                    stack.push(n);
                }
            }
        }
        reach
    }
}

/// Applies `op` to `e`; with `check`, a failed precondition leaves `e` unchanged and returns `false` ([F15 §2.5] step 3:
/// a create over an existing name, a rename or unlink of a name that no longer names the operation's node, a no-replace
/// rename onto an existing name, or a target directory that does not exist).
fn apply(e: &mut Names, op: &NsOp, check: bool) -> bool {
    match op {
        NsOp::Create { dir, name, node } => {
            if check && (e.get(*dir, name).is_some() || !e.present(*dir)) {
                return false;
            }
            e.insert(*dir, name.clone(), *node);
        }
        NsOp::Remove { dir, name, node } => {
            if e.get(*dir, name) != Some(*node) {
                return false;
            }
            e.remove(*dir, name);
        }
        NsOp::Rename {
            from,
            to,
            node,
            replace,
            ..
        } => {
            if e.get_key(from) != Some(*node) {
                return false;
            }
            if check && ((!replace && e.get_key(to).is_some()) || !e.present(to.0)) {
                return false;
            }
            e.remove(from.0, &from.1);
            e.insert(to.0, to.1.clone(), *node);
        }
        NsOp::Exchange {
            a,
            b,
            a_node,
            b_node,
        } => {
            if e.get_key(a) != Some(*a_node) || e.get_key(b) != Some(*b_node) {
                return false;
            }
            e.remove(a.0, &a.1);
            e.remove(b.0, &b.1);
            e.insert(a.0, a.1.clone(), *b_node);
            e.insert(b.0, b.1.clone(), *a_node);
        }
    }
    true
}

/// The namespace of the simulated kernel.
#[derive(Clone, Debug)]
pub(crate) struct Ns {
    pub(crate) nodes: BTreeMap<u64, Node>,
    pub(crate) cur: Names,
    pub(crate) dur: Names,
    pub(crate) pending: Vec<Pending>,
    pub(crate) next_node: u64,
    pub(crate) next_op: u64,
    /// Per node, the pending operations that name or displace it.
    refs: BTreeMap<u64, u32>,
    /// Per volume, the sum of cs(f) over its file nodes.
    used: BTreeMap<u32, u64>,
}

/// The components of an absolute path: a Windows prefix becomes the first name, the root separator is skipped.
pub(crate) fn abs_components(path: &Path) -> Result<Vec<String>, VfsErrorKind> {
    if !path.has_root() {
        return Err(VfsErrorKind::InvalidName);
    }
    let mut out = Vec::new();
    for c in path.components() {
        match c {
            Component::Prefix(p) => {
                out.push(
                    p.as_os_str()
                        .to_str()
                        .ok_or(VfsErrorKind::InvalidName)?
                        .to_owned(),
                );
            }
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => return Err(VfsErrorKind::InvalidName),
            Component::Normal(s) => {
                let s = s.to_str().ok_or(VfsErrorKind::InvalidName)?;
                RelPath::new(s).map_err(|_| VfsErrorKind::InvalidName)?;
                out.push(s.to_owned());
            }
        }
    }
    Ok(out)
}

/// Whether `c` is a drive component (`X:`).
fn is_drive(c: &str) -> bool {
    let b = c.as_bytes();
    b.len() == 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

impl Ns {
    pub(crate) fn new() -> Ns {
        let mut nodes = BTreeMap::new();
        nodes.insert(
            ROOT,
            Node {
                kind: Kind::Dir {
                    creation_durable: true,
                },
                vol: 0,
                open: 0,
                delete_pending: None,
                share_block: 0,
            },
        );
        Ns {
            nodes,
            cur: Names::default(),
            dur: Names::default(),
            pending: Vec::new(),
            next_node: 1,
            next_op: 1,
            refs: BTreeMap::new(),
            used: BTreeMap::new(),
        }
    }

    pub(crate) fn node(&self, n: u64) -> &Node {
        self.nodes
            .get(&n)
            .expect("simulator: a live handle's node exists")
    }

    pub(crate) fn node_mut(&mut self, n: u64) -> &mut Node {
        self.nodes
            .get_mut(&n)
            .expect("simulator: a live handle's node exists")
    }

    pub(crate) fn file(&self, n: u64) -> &FileNode {
        self.node(n)
            .file()
            .expect("simulator: a file handle names a file")
    }

    /// The file's attributes (seal, injected faults). Its content changes only through [`Ns::edit`].
    pub(crate) fn file_mut(&mut self, n: u64) -> &mut FileNode {
        self.node_mut(n)
            .file_mut()
            .expect("simulator: a file handle names a file")
    }

    /// Changes the content of file `n` through `f`, keeping its volume's size total and dropping its snapshot. `None`
    /// if `n` is not an existing file.
    pub(crate) fn edit<R>(&mut self, n: u64, f: impl FnOnce(&mut Content) -> R) -> Option<R> {
        let node = self.nodes.get_mut(&n)?;
        let vol = node.vol;
        let file = node.file_mut()?;
        let before = file.content.cs();
        let r = f(&mut file.content);
        let after = file.content.cs();
        file.snap = None;
        let used = self.used.entry(vol).or_insert(0);
        *used = (*used + after).saturating_sub(before);
        Some(r)
    }

    /// The sum of the sizes of the files on volume `vol`.
    pub(crate) fn used_bytes(&self, vol: u32) -> u64 {
        self.used.get(&vol).copied().unwrap_or(0)
    }

    /// The node `name` names in directory `dir`, in the current namespace.
    pub(crate) fn child(&self, dir: u64, name: &str) -> Option<u64> {
        self.cur.get(dir, name)
    }

    /// The node `rel` names under `dir` (the directory itself for the root path).
    pub(crate) fn lookup(&self, dir: u64, rel: RelPath<'_>) -> Result<u64, VfsErrorKind> {
        // A root follows its directory's identity; a directory that was removed and forgotten names nothing.
        if !self.nodes.contains_key(&dir) {
            return Err(VfsErrorKind::NotFound);
        }
        let mut at = dir;
        for seg in rel.segments() {
            if !self.node(at).is_dir() {
                return Err(VfsErrorKind::NotFound);
            }
            at = self.child(at, seg).ok_or(VfsErrorKind::NotFound)?;
        }
        Ok(at)
    }

    /// The parent directory and the last segment of a non-root `rel` under `dir`.
    pub(crate) fn parent_of<'a>(
        &self,
        dir: u64,
        rel: RelPath<'a>,
    ) -> Result<(u64, &'a str), VfsErrorKind> {
        let name = rel.file_name().ok_or(VfsErrorKind::InvalidName)?;
        let parent = self.lookup(dir, rel.parent().unwrap_or(RelPath::ROOT))?;
        if !self.node(parent).is_dir() {
            return Err(VfsErrorKind::NotFound);
        }
        Ok((parent, name))
    }

    /// The node an absolute path names.
    pub(crate) fn lookup_abs(&self, path: &Path) -> Result<u64, VfsErrorKind> {
        let mut at = ROOT;
        for seg in abs_components(path)? {
            if !self.node(at).is_dir() {
                return Err(VfsErrorKind::NotFound);
            }
            at = self.child(at, &seg).ok_or(VfsErrorKind::NotFound)?;
        }
        Ok(at)
    }

    /// The simulator's machine-local absolute form of the current path of `n` ([80 §2.10] P12 as the swap intent uses
    /// it, [OS/fs §4.9.3]): components joined by `/`, starting with `/`, or with an upper-cased drive `X:` when the first
    /// component is one. `None` for a node without a current path.
    pub(crate) fn abs_path(&self, n: u64) -> Option<String> {
        let mut parts: Vec<&str> = Vec::new();
        let mut at = n;
        while at != ROOT {
            let (d, name) = self.cur.name_of(at)?;
            parts.push(name);
            at = *d;
        }
        parts.reverse();
        let mut s = String::new();
        for (i, p) in parts.iter().enumerate() {
            if i == 0 && is_drive(p) {
                s.push_str(&p.to_ascii_uppercase());
            } else {
                s.push('/');
                s.push_str(p);
            }
        }
        if s.is_empty() || (parts.len() == 1 && is_drive(parts[0])) {
            s.push('/');
        }
        Some(s)
    }

    /// The parent directory and last name of a path in the form of [`Ns::abs_path`], in the current namespace. The
    /// parent must exist; the name need not.
    pub(crate) fn resolve_parent(&self, path: &str) -> Result<(u64, String), VfsErrorKind> {
        let (drive, rest) = match path.as_bytes() {
            [b'/', ..] => (None, &path[1..]),
            [_, b':', b'/', ..] if is_drive(&path[..2]) => (Some(&path[..2]), &path[3..]),
            _ => return Err(VfsErrorKind::InvalidName),
        };
        let mut comps: Vec<&str> = rest.split('/').collect();
        let name = comps.pop().filter(|n| !n.is_empty());
        let Some(name) = name else {
            return Err(VfsErrorKind::InvalidName);
        };
        let mut at = ROOT;
        if let Some(d) = drive {
            at = self
                .child(at, d)
                .or_else(|| self.child(at, &d.to_ascii_lowercase()))
                .ok_or(VfsErrorKind::NotFound)?;
        }
        for c in comps {
            if c.is_empty() || !self.node(at).is_dir() {
                return Err(VfsErrorKind::NotFound);
            }
            at = self.child(at, c).ok_or(VfsErrorKind::NotFound)?;
        }
        if !self.node(at).is_dir() {
            return Err(VfsErrorKind::NotFound);
        }
        Ok((at, name.to_owned()))
    }

    /// A new node (not yet named).
    pub(crate) fn new_node(&mut self, kind: Kind, vol: u32) -> u64 {
        let id = self.next_node;
        self.next_node += 1;
        if let Kind::File(f) = &kind {
            *self.used.entry(vol).or_insert(0) += f.content.cs();
        }
        self.nodes.insert(
            id,
            Node {
                kind,
                vol,
                open: 0,
                delete_pending: None,
                share_block: 0,
            },
        );
        id
    }

    /// Applies `op` to the current namespace at its effect instant and records it as pending. Returns its id.
    pub(crate) fn push(&mut self, op: NsOp) -> u64 {
        let applied = apply(&mut self.cur, &op, false);
        debug_assert!(
            applied,
            "simulator: a namespace operation applied after its checks"
        );
        for n in op.nodes() {
            *self.refs.entry(n).or_insert(0) += 1;
        }
        let id = self.next_op;
        self.next_op += 1;
        let parents = op.parents();
        let synced = vec![false; parents.len()];
        self.pending.push(Pending {
            id,
            op,
            parents,
            synced,
        });
        id
    }

    /// Names a node durably, as part of the environment that exists before the scenario (no pending operation).
    pub(crate) fn name_durable(&mut self, dir: u64, name: &str, node: u64) {
        self.cur.insert(dir, name.to_owned(), node);
        self.dur.insert(dir, name.to_owned(), node);
    }

    /// Creates a directory durably, as part of the environment that exists before the scenario (no pending operation).
    pub(crate) fn mkdir_durable(&mut self, dir: u64, name: &str, vol: u32) -> u64 {
        if let Some(n) = self.child(dir, name) {
            return n;
        }
        let n = self.new_node(
            Kind::Dir {
                creation_durable: true,
            },
            vol,
        );
        self.name_durable(dir, name, n);
        n
    }

    /// A `sync_dir` of `dir` that started when the next operation id was `limit` succeeded: every earlier pending
    /// operation with `dir` as a parent counts it, and every operation that now meets FM-2.3 becomes durable, in issue
    /// order, in one pass (an operation waits only for earlier ones, so a pass in issue order sees every one it waits
    /// for settled). Nodes the durable operations left without any reference are removed. Returns the ids that became
    /// durable, in the order they did.
    pub(crate) fn sync_dir_ok(&mut self, dir: u64, limit: u64) -> Vec<u64> {
        for p in &mut self.pending {
            if p.id < limit {
                for (i, &parent) in p.parents.iter().enumerate() {
                    if parent == dir {
                        p.synced[i] = true;
                    }
                }
            }
        }
        let mut done = Vec::new();
        let mut touched: Vec<u64> = Vec::new();
        let mut blocked: BTreeSet<(u64, String)> = BTreeSet::new();
        let pending = core::mem::take(&mut self.pending);
        for p in pending {
            let ready = p.synced.iter().all(|&s| s)
                && p.parents.iter().all(|&d| {
                    matches!(
                        self.nodes.get(&d).map(|n| &n.kind),
                        Some(Kind::Dir {
                            creation_durable: true
                        })
                    )
                })
                && !p
                    .op
                    .names()
                    .iter()
                    .any(|&(d, n)| blocked.contains(&(d, n.to_owned())));
            if !ready {
                for (d, n) in p.op.names() {
                    blocked.insert((d, n.to_owned()));
                }
                self.pending.push(p);
                continue;
            }
            let applied = apply(&mut self.dur, &p.op, true);
            debug_assert!(
                applied,
                "simulator: a durable operation applies to the durable namespace"
            );
            if let NsOp::Create { node, .. } = p.op
                && let Some(Kind::Dir { creation_durable }) =
                    self.nodes.get_mut(&node).map(|n| &mut n.kind)
            {
                *creation_durable = true;
            }
            for n in p.op.nodes() {
                if let Some(r) = self.refs.get_mut(&n) {
                    *r -= 1;
                    if *r == 0 {
                        self.refs.remove(&n);
                    }
                }
                touched.push(n);
            }
            touched.extend(p.parents.iter().copied());
            done.push(p.id);
        }
        for n in touched {
            self.gc(n);
        }
        done
    }

    /// Removes `n` if nothing refers to it any more: no handle, no current or durable name, no pending operation, and
    /// (a directory) no entry in either namespace.
    pub(crate) fn gc(&mut self, n: u64) {
        if n == ROOT {
            return;
        }
        let Some(node) = self.nodes.get(&n) else {
            return;
        };
        if node.open != 0
            || self.cur.names_node(n)
            || self.dur.names_node(n)
            || self.refs.contains_key(&n)
            || (node.is_dir() && (self.cur.has_children(n) || self.dur.has_children(n)))
        {
            return;
        }
        if let Some(node) = self.nodes.remove(&n)
            && let Some(f) = node.file()
        {
            let used = self.used.entry(node.vol).or_insert(0);
            *used = used.saturating_sub(f.content.cs());
        }
    }

    /// A system crash's namespace step ([F15 §2.5] step 3): the survivors (by `survive`, asked once per pending operation
    /// in issue order) replayed on the durable namespace, unreachable nodes dropped, every delete-pending state and
    /// sharing violation ended, every handle gone. Returns the number of survivors that applied.
    pub(crate) fn crash(&mut self, survive: &mut dyn FnMut(&Pending) -> bool) -> u64 {
        let mut e = self.dur.clone();
        let mut applied = 0;
        for p in &self.pending {
            if survive(p) && apply(&mut e, &p.op, true) {
                applied += 1;
            }
        }
        let reach = e.reachable();
        e.retain_dirs(&reach);
        self.nodes.retain(|id, _| reach.contains(id));
        self.used.clear();
        for node in self.nodes.values_mut() {
            node.open = 0;
            node.delete_pending = None;
            node.share_block = 0;
            match &mut node.kind {
                Kind::Dir { creation_durable } => *creation_durable = true,
                Kind::File(f) => {
                    f.read_errors.retain(|r| r.remaining.is_none());
                    f.map_fault = false;
                    *self.used.entry(node.vol).or_insert(0) += f.content.cs();
                }
            }
        }
        self.cur = e.clone();
        self.dur = e;
        self.pending.clear();
        self.refs.clear();
        applied
    }

    /// Every current absolute path of `n`, for diagnostics and the crash surface (`/`-separated, from the world root;
    /// empty for a node without a name).
    pub(crate) fn paths_of(&self, n: u64) -> Vec<String> {
        if n == ROOT {
            return vec!["/".to_owned()];
        }
        let mut parts: Vec<&str> = Vec::new();
        let mut at = n;
        while at != ROOT {
            let Some((d, name)) = self.cur.name_of(at) else {
                return Vec::new();
            };
            parts.push(name);
            at = *d;
        }
        parts.reverse();
        vec![format!("/{}", parts.join("/"))]
    }

    /// Whether directory `anc` is `n` or an ancestor of `n` in the current namespace.
    pub(crate) fn is_ancestor(&self, anc: u64, n: u64) -> bool {
        let mut at = n;
        loop {
            if at == anc {
                return true;
            }
            if at == ROOT {
                return false;
            }
            match self.cur.name_of(at) {
                Some((d, _)) => at = *d,
                None => return false,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file() -> Kind {
        Kind::File(Box::default())
    }

    #[test]
    fn durability_needs_every_parent_and_earlier_operations() {
        let mut ns = Ns::new();
        let a = ns.mkdir_durable(ROOT, "a", 0);
        let b = ns.mkdir_durable(ROOT, "b", 0);
        let f = ns.new_node(file(), 0);
        ns.push(NsOp::Create {
            dir: a,
            name: "f".into(),
            node: f,
        });
        let mv = ns.push(NsOp::Rename {
            from: (a, "f".into()),
            to: (b, "g".into()),
            node: f,
            replace: false,
            replaced: None,
        });
        // Syncing b alone makes nothing durable (FM-2.4); syncing a then makes both durable, create first.
        assert!(ns.sync_dir_ok(b, ns.next_op).is_empty());
        let done = ns.sync_dir_ok(a, ns.next_op);
        assert_eq!(done, vec![1, mv]);
        assert_eq!(ns.dur.get(b, "g"), Some(f));
    }

    #[test]
    fn crash_replays_any_subset_and_drops_unnamed_nodes() {
        let mut ns = Ns::new();
        let d = ns.mkdir_durable(ROOT, "d", 0);
        let x = ns.new_node(file(), 0);
        let y = ns.new_node(file(), 0);
        ns.push(NsOp::Create {
            dir: d,
            name: "x".into(),
            node: x,
        });
        ns.push(NsOp::Create {
            dir: d,
            name: "y".into(),
            node: y,
        });
        // Keep the later create, lose the earlier one (no prefix rule, FM-2.3).
        let applied = ns.crash(&mut |p| p.id == 2);
        assert_eq!(applied, 1);
        assert_eq!(ns.child(d, "y"), Some(y));
        assert_eq!(ns.child(d, "x"), None);
        assert!(!ns.nodes.contains_key(&x));
    }

    #[test]
    fn a_rename_whose_source_is_lost_is_lost() {
        let mut ns = Ns::new();
        let d = ns.mkdir_durable(ROOT, "d", 0);
        let x = ns.new_node(file(), 0);
        ns.push(NsOp::Create {
            dir: d,
            name: "x".into(),
            node: x,
        });
        ns.push(NsOp::Rename {
            from: (d, "x".into()),
            to: (d, "z".into()),
            node: x,
            replace: false,
            replaced: None,
        });
        assert_eq!(ns.crash(&mut |p| p.id == 2), 0);
        assert!(ns.child(d, "z").is_none());
        assert_eq!(ns.paths_of(d), vec!["/d".to_owned()]);
    }

    /// Review regression: nodes a durable removal or replacement leaves unreferenced are collected, with their bytes.
    #[test]
    fn durable_removals_and_replacements_free_their_nodes() {
        let mut ns = Ns::new();
        let d = ns.mkdir_durable(ROOT, "d", 0);
        let big = |ns: &mut Ns| {
            let n = ns.new_node(file(), 0);
            ns.edit(n, |c| c.write_zeros(0, 1 << 20, &mut |_, _, _| 0));
            n
        };
        let old = big(&mut ns);
        ns.name_durable(d, "config", old);
        let new = big(&mut ns);
        ns.push(NsOp::Create {
            dir: d,
            name: "tmp".into(),
            node: new,
        });
        ns.sync_dir_ok(d, ns.next_op);
        assert_eq!(ns.used_bytes(0), 2 << 20);
        ns.push(NsOp::Rename {
            from: (d, "tmp".into()),
            to: (d, "config".into()),
            node: new,
            replace: true,
            replaced: Some(old),
        });
        ns.gc(old);
        assert!(ns.nodes.contains_key(&old), "a crash may still restore it");
        ns.sync_dir_ok(d, ns.next_op);
        assert!(!ns.nodes.contains_key(&old));
        assert_eq!(ns.used_bytes(0), 1 << 20);
        ns.push(NsOp::Remove {
            dir: d,
            name: "config".into(),
            node: new,
        });
        ns.sync_dir_ok(d, ns.next_op);
        assert!(!ns.nodes.contains_key(&new));
        assert_eq!(ns.used_bytes(0), 0);
    }

    #[test]
    fn machine_local_paths_round_trip() {
        let mut ns = Ns::new();
        let c = ns.mkdir_durable(ROOT, "c:", 0);
        let s = ns.mkdir_durable(c, "sim", 0);
        let a = ns.mkdir_durable(s, "a", 0);
        assert_eq!(ns.abs_path(a).as_deref(), Some("C:/sim/a"));
        assert_eq!(ns.resolve_parent("C:/sim/a"), Ok((s, "a".to_owned())));
        let u = ns.mkdir_durable(ROOT, "u", 0);
        let b = ns.mkdir_durable(u, "b", 0);
        assert_eq!(ns.abs_path(b).as_deref(), Some("/u/b"));
        assert_eq!(ns.resolve_parent("/u/b"), Ok((u, "b".to_owned())));
        assert_eq!(ns.resolve_parent("/u/x/b"), Err(VfsErrorKind::NotFound));
        assert_eq!(ns.resolve_parent("u/b"), Err(VfsErrorKind::InvalidName));
    }
}
